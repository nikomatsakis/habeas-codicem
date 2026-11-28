//! Habeas's review procedure and conversation policy on the SDK interfaces.

use super::*;
use crate::{
    inputs::InputSelector,
    sdk::{HabeasDomain, HabeasEdgeLabel, HabeasNode},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use trust_but_verify::domain::NodeContents;
use trust_but_verify::domain::{Domain, NodeId};
use trust_but_verify::graph::{
    Criterion, CriterionPath, CriterionVersion, Dependency, ValidationError,
};
use trust_but_verify::request::{ValidationContext, VetResult};

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
enum Determination {
    Boolean(bool),
    Fixme {
        #[serde(rename = "FIXME")]
        #[schemars(length(min = 1))]
        entries: Vec<String>,
    },
}

// {impl}`requirements.authoring.submission`
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReviewAnswer<T> {
    determination: T,
    #[schemars(length(min = 1))]
    rationale: String,
}
// {impl}`requirements.authoring.submission`
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SiteAnswer {
    determination: Determination,
    #[schemars(length(min = 1))]
    rationale: String,
    applies: bool,
}
struct Review<'a> {
    sources: &'a AssessmentSources,
    node: NodeId<HabeasDomain>,
    kind: ReviewKind,
    inputs: Vec<Value>,
    criterion: CriterionPath,
}

impl Review<'_> {
    fn missing_optional_terms(
        &self,
        cx: &ValidationContext<'_, HabeasDomain>,
    ) -> io::Result<Vec<String>> {
        use std::collections::{BTreeSet, HashSet};
        let graph = cx.graph();
        let mandatory: HashSet<_> = self
            .inputs
            .iter()
            .filter_map(InputSelector::from_input)
            .map(|selector| graph.cache().intern_node(HabeasNode::from(&selector)))
            .chain(std::iter::once(self.node))
            .collect();
        let mut definitions = BTreeSet::new();
        let mut optional = Vec::new();
        for dependency in cx.dependencies() {
            let Dependency::Node(node, _) = dependency else {
                continue;
            };
            let data = graph.cache().node_data(node).expect("observed node");
            if let HabeasNode::Definition(name) = data.as_ref() {
                definitions.insert(name.clone());
            }
            if mandatory.contains(&node) {
                continue;
            }
            if let Ok(selector) = data.selector() {
                let input = match &selector {
                    InputSelector::Outline { file, item } => {
                        // Use the retained tool snapshot even if the workspace changed after delivery.
                        let selected = trust_but_verify::outline::OutlineContent::from_path(
                            graph.cache(),
                            graph.root.join(file),
                        )
                        .and_then(|snapshot| snapshot.select(item))
                        .map_err(io::Error::other)?;
                        let text = trust_but_verify::sources::redact_text(selected.text());
                        json!({"file": file, "item": item, "text": text.lines().collect::<Vec<_>>()})
                    }
                    _ => self.sources.read_selector(&selector)?,
                };
                optional.push(input);
            }
        }
        let record = json!({"inputs": optional});
        let required = crate::terms::dependencies(
            &self.sources.root,
            &graph.definitions,
            &self.sources.outlines,
            record.as_object().unwrap(),
            None,
        )
        .map_err(io::Error::other)?;
        Ok(required.difference(&definitions).cloned().collect())
    }

    // {impl}`requirements.authoring.line_numbers.citation`
    // {impl}`requirements.authoring.prompt_parts`
    // {impl}`requirements.authoring.source_tools`
    // {impl}`requirements.authoring.submission`
    async fn ask<R>(
        &self,
        cx: &ValidationContext<'_, HabeasDomain>,
        prompt: Prompt,
        validate: impl Fn(&Value) -> io::Result<()> + Send + Sync,
    ) -> Result<Value, ValidationError<io::Error>>
    where
        R: Serialize + serde::de::DeserializeOwned + JsonSchema + Send,
    {
        let mut corrected = false;
        let mut reminded_scope = false;
        let mut rejection = line_numbers::Rejection::default();
        let citation_review = matches!(self.sources.subject_kind, AssessmentKind::Citation(_));
        let request = cx
            .ask::<R>()
            .context(prompt.system)
            .message(prompt.subject)
            .vet_answer(|answer| {
                let value = serde_json::to_value(answer)?;
                let resubmitted = !citation_review && rejection.resubmitted(&value).is_some();
                if !resubmitted {
                    if let Err(error) = validate(&value) {
                        return Ok(VetResult::CorrectAndTryAgain(error.to_string()));
                    }
                    let other_role = match self.kind {
                        ReviewKind::Structural => Some("test"), ReviewKind::Behavioral => Some("impl"), ReviewKind::Standard => None,
                    };
                    if let Some(role) = other_role && value["corrections"].as_array().into_iter().flatten().any(|c| c["role"] == role) {
                        let message = format!("The {} criterion cannot propose a {{{role}}} marker. Judge only its own obligations. If all of them are supported or none apply, submit success and explain why.",self.kind.name());
                        if reminded_scope { return Err(io::Error::other(message)); }
                        reminded_scope = true;
                        return Ok(VetResult::CorrectAndTryAgain(message));
                    }
                }
                if matches!(self.sources.subject_kind, AssessmentKind::Definition(..)) {
                    let missing = self.missing_optional_terms(cx)?;
                    if !missing.is_empty() {
                        let reads: Vec<_> = missing
                            .into_iter()
                            .map(|name| json!({"data": HabeasNode::Definition(name)}))
                            .collect();
                        let reads = serde_json::to_string(&reads)?;
                        return Ok(VetResult::CorrectAndTryAgain(format!(
                            "Reviewed inputs refer to additional definitions. Call read_node for each of {reads}, consider their contents, and submit again."
                        )));
                    }
                }
                if resubmitted {
                    return Ok(VetResult::Accept);
                }
                if !citation_review {
                    return match rejection.decide("submit_result", &value, location_guidance()) {
                        line_numbers::Outcome::Accept => Ok(VetResult::Accept),
                        line_numbers::Outcome::Reject(message) => {
                            Ok(VetResult::CorrectAndTryAgain(message))
                        }
                        line_numbers::Outcome::Fail(message) => Err(io::Error::other(message)),
                    };
                }
                if let Some(citation) = value["rationale"].as_str().and_then(line_numbers::cited) {
                    if corrected {
                        return Err(io::Error::other(format!(
                            "The revised rationale still cites a line number ({citation:?})."
                        )));
                    }
                    corrected = true;
                    return Ok(VetResult::CorrectAndTryAgain(format!(
                        "The rationale cites a line number ({citation:?}). {}",
                        location_guidance()
                    )));
                }
                Ok(VetResult::Accept)
            })
            .contents_of_current_node();
        let mut request = request;
        if self.kind == ReviewKind::Standard
            && matches!(self.sources.subject_kind, AssessmentKind::Definition(..))
        {
            request = request.contents_of_successors(HabeasEdgeLabel::Mandatory);
        } else {
            for input in &self.inputs {
                if let Some(selector) = InputSelector::from_input(input) {
                    let node = cx.graph().cache().intern_node(HabeasNode::from(&selector));
                    if node != self.node {
                        request = request.contents_of_node(node);
                    }
                }
            }
        }
        let result = serde_json::to_value(request.await?).map_err(ValidationError::Output)?;
        Ok(rejection.resubmitted(&result).unwrap_or(result))
    }
}

impl Criterion<HabeasDomain> for Review<'_> {
    type Ok = Value;
    fn source_node_id(&self) -> NodeId<HabeasDomain> {
        self.node
    }
    fn criterion_path(&self) -> CriterionPath {
        self.criterion.clone()
    }
    fn version(&self) -> CriterionVersion {
        crate::sdk::review_version()
    }
    // {impl}`requirements.authoring.citation_prompt`
    async fn execute(
        self,
        cx: &mut ValidationContext<'_, HabeasDomain>,
    ) -> Result<Value, ValidationError<io::Error>> {
        let graph = cx.graph();
        if let Some(HabeasNode::Requirement(id)) = graph.cache().node_data(self.node).as_deref() {
            cx.read(
                &graph
                    .cache()
                    .intern_node(HabeasNode::ReferenceSet(id.clone())),
            )
            .map_err(ValidationError::Graph)?;
        }
        // The SDK supplies node bodies at delivery. The task message carries only
        // addresses and metadata alongside those bodies.
        let annotations: Vec<_> = self
            .inputs
            .iter()
            .cloned()
            .map(|mut input| {
                if let Some(selector) = InputSelector::from_input(&input) {
                    let data = HabeasNode::from(&selector);
                    let mut metadata =
                        json!({"node": graph.cache().intern_node(data.clone()), "data": data});
                    if let Some(issues) = input.get_mut("github_issues") {
                        metadata["github_issues"] = issues.take();
                    }
                    metadata
                } else {
                    input
                }
            })
            .collect();
        let site = matches!(self.sources.subject_kind, AssessmentKind::Site(..));
        let prompt = initial_prompt_with_inputs(self.sources, self.kind, &annotations)
            .map_err(ValidationError::Graph)?;
        if site {
            self.ask::<SiteAnswer>(cx, prompt, |v| site_submission(v).map(drop))
                .await
        } else if matches!(self.sources.subject_kind, AssessmentKind::Citation(_)) {
            self.ask::<ReviewAnswer<bool>>(cx, prompt, |v| submission(v).map(drop))
                .await
        } else {
            self.ask::<corrections::AssessmentResult>(cx, prompt, |v| {
                serde_json::from_value::<corrections::AssessmentResult>(v.clone())?
                    .evidence()
                    .map(drop)
            })
            .await
        }
    }
}
pub(super) async fn assess(
    backend: &config::BackendConfig,
    sources: &mut AssessmentSources,
    usage: &mut UsageLog,
) -> io::Result<Assessment> {
    let record = run(backend, sources, usage).await?;
    let value = crate::evidence_format::encode(&record)?;
    let mut findings = Vec::new();
    let mut capture =
        |output: &Value, deps: &[Dependency<HabeasNode, HabeasEdgeLabel>]| -> io::Result<()> {
            if let Some(items) = output.get("corrections") {
                let items: Vec<corrections::Correction> = serde_json::from_value(items.clone())?;
                findings.extend(
                    items
                        .into_iter()
                        .map(|c| corrections::Finding::capture(c, deps)),
                );
            }
            Ok(())
        };
    if let Some(criteria) = value.get("criteria") {
        for name in ["structural", "behavioral"] {
            let criterion: trust_but_verify::graph::Record<HabeasNode, HabeasEdgeLabel> =
                serde_json::from_value(criteria[name].clone())?;
            capture(&criterion.output, &criterion.dependencies)?;
        }
    } else {
        capture(&value, &record.dependencies)?;
    }
    let determination = Judgment::from_value(&value["determination"]).map_err(io::Error::other)?;
    Ok(Assessment {
        determination,
        record: value,
        findings,
    })
}

/// Assess the tracking issues of the acknowledgments used in this judgment.
async fn review_tracking_issues(
    sources: &AssessmentSources,
    graph: &HabeasDomain,
    validator: &mut trust_but_verify::graph::Validator<'_, HabeasDomain>,
    answer: &mut Value,
    dependencies: &mut Vec<Dependency<NodeId<HabeasDomain>, HabeasEdgeLabel>>,
) -> Result<(), ValidationError<io::Error>> {
    let Some(entries) = answer
        .get("entries")
        .or_else(|| answer["determination"].get("FIXME"))
        .and_then(Value::as_array)
    else {
        return Ok(());
    };
    let mut corrections = Vec::new();
    let mut reviewed = std::collections::HashSet::new();
    let (subject, bodies) = match &sources.subject_kind {
        AssessmentKind::Definition(..) => (
            &sources.subject_name,
            sources
                .model_acknowledgments()
                .into_iter()
                .map(|a| {
                    (
                        a["file"].as_str().unwrap().to_owned(),
                        a["item"].as_str().unwrap().to_owned(),
                        a["body"].as_str().unwrap().to_owned(),
                    )
                })
                .collect::<Vec<_>>(),
        ),
        AssessmentKind::Site(site, fixmes) => (
            &site.requirement,
            fixmes
                .iter()
                .map(|f| {
                    (
                        site.file.clone(),
                        f.item.clone(),
                        crate::redaction::redact_text(&f.commentary).into_owned(),
                    )
                })
                .collect(),
        ),
        AssessmentKind::Citation(_) => return Ok(()),
    };
    for (file, item, body) in bodies {
        let fixme = crate::fixme::FixmeBody::parse(&body);
        if !fixme
            .issues
            .iter()
            .any(|issue| entries.iter().any(|e| e.as_str() == Some(issue)))
        {
            continue;
        }
        let nodes = graph
            .acknowledgments_for_review(&file, &item, subject, &body)
            .map_err(ValidationError::Graph)?;
        for node in nodes {
            if !reviewed.insert(node) {
                continue;
            }
            let checked = validator
                .verify(crate::sdk::issues::RelatedIssues(node))
                .await?;
            for dep in checked.dependencies() {
                if !dependencies.contains(dep) {
                    dependencies.push(dep.clone());
                }
            }
            for row in checked.into_output().into_iter().filter(|row| !row.related) {
                let instruction = format!(
                    "Update the acknowledgment in {file} to link an issue that describes its gap in place of {}",
                    row.issue
                );
                corrections.push(corrections::Correction::Manual {
                    instruction,
                    rationale: row.rationale,
                });
            }
        }
    }
    if !corrections.is_empty() {
        if matches!(sources.subject_kind, AssessmentKind::Site(..)) {
            answer["determination"] = json!(false);
            answer["rationale"] = json!(
                corrections
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n\n")
            );
        } else {
            *answer = json!({"status":"failed","corrections":corrections});
        }
    }
    Ok(())
}

/// Publish this assessment's counters even if its future is dropped during a request or backoff.
struct AssessmentUsage<'d, 'log> {
    validator: trust_but_verify::graph::Validator<'d, HabeasDomain>,
    log: &'log mut UsageLog,
}

impl Drop for AssessmentUsage<'_, '_> {
    fn drop(&mut self) {
        self.log.observe_usage(self.validator.usage());
    }
}

// {impl}`requirements.authoring.submission`
// {impl}`requirements.authoring.citation_prompt`
// {impl}`requirements.authoring.source_tools`
// {impl}`requirements.authoring.finish_search.inputs`
// {impl}`requirements.authoring.usage_requests`
async fn run(
    backend: &config::BackendConfig,
    sources: &mut AssessmentSources,
    usage: &mut UsageLog,
) -> io::Result<trust_but_verify::graph::Record<HabeasNode, HabeasEdgeLabel>> {
    let subject = match &sources.subject_kind {
        AssessmentKind::Definition(..) => {
            AuthoringSubject::Definition(sources.subject_name.clone())
        }
        AssessmentKind::Citation(item) => AuthoringSubject::Citation {
            file: item.file.clone(),
            item: item.item.clone(),
        },
        AssessmentKind::Site(site, _) => AuthoringSubject::Site {
            requirement: site.requirement.clone(),
            file: site.file.clone(),
            item: site.item.clone(),
        },
    };
    let node = HabeasNode::review_subject(&subject);
    let default_criterion = node.review_criterion();
    let mut graph = sources.domain.clone();
    domain_tools::configure(&mut graph, backend, sources.github.clone())?;
    let node = graph.cache().intern_node(node);
    let mut assessment = AssessmentUsage {
        validator: trust_but_verify::graph::Validator::new(&graph),
        log: usage,
    };
    let result = async {
        let kinds: &[ReviewKind] = if sources.review_roots.is_some() {
            &[ReviewKind::Structural, ReviewKind::Behavioral]
        } else {
            &[ReviewKind::Standard]
        };
        let mut criteria = serde_json::Map::new();
        let mut dependencies = Vec::new();
        for &kind in kinds {
            let verified = assessment
                .validator
                .verify(Review {
                    sources,
                    kind,
                    inputs: sources.inputs_for(kind).map_err(ValidationError::Graph)?,
                    criterion: if kind == ReviewKind::Standard {
                        default_criterion.clone()
                    } else {
                        CriterionPath::new(vec![kind.name().into()])
                    },
                    node,
                })
                .await?;
            let mut answer = verified.output().clone();
            let mut observed = verified.dependencies().to_vec();
            review_tracking_issues(
                sources,
                &graph,
                &mut assessment.validator,
                &mut answer,
                &mut observed,
            )
            .await?;
            for dep in &observed {
                if !dependencies.contains(dep) {
                    dependencies.push(dep.clone());
                }
            }
            if kind == ReviewKind::Standard {
                return Ok((answer, None, dependencies));
            }
            let record = trust_but_verify::graph::Record::new(
                verified.version().cloned(),
                stored_dependencies(&graph, &observed),
                answer,
            );
            criteria.insert(
                kind.name().into(),
                serde_json::to_value(record).map_err(ValidationError::Output)?,
            );
        }
        let criteria = Value::Object(criteria);
        let answer = corrections::combine_criteria(&criteria).map_err(ValidationError::Graph)?;
        Ok((
            serde_json::to_value(answer).map_err(ValidationError::Output)?,
            Some(criteria),
            dependencies,
        ))
    }
    .await;
    drop(assessment);
    usage.save()?;
    let (answer, criteria, dependencies) = result.map_err(|error| match error {
        ValidationError::Graph(error) => error,
        ValidationError::Provider(error) => {
            io::Error::new(error.kind(), ValidationError::<io::Error>::Provider(error))
        }
        other => io::Error::other(other.to_string()),
    })?;
    // Project the SDK's observed nodes into the existing application evidence fields.
    for dependency in &dependencies {
        if let Dependency::Node(id, observed) = dependency {
            let data = graph.cache().node_data(*id).expect("observed node");
            if let Ok(selector) = data.selector() {
                let input = if let (HabeasNode::Source(_), InputSelector::Outline { file, item }) =
                    (data.as_ref(), &selector)
                {
                    // A selected search excerpt or read pins the SDK file snapshot.
                    // Reopening the workspace here could record edits made in-flight.
                    let contents = trust_but_verify::outline::OutlineContent::from_path(
                        graph.cache(),
                        graph.root.join(file),
                    )
                    .and_then(|snapshot| snapshot.select(item))
                    .map_err(io::Error::other)?;
                    if contents.fingerprint() != *observed {
                        return Err(io::Error::other(format!(
                            "observed snapshot unavailable for {file}#{item}"
                        )));
                    }
                    json!({"file":file,"item":item,"hash":contents.structural_hash()})
                } else {
                    sources.read_selector(&selector)?
                };
                sources.record_inputs(&[input]);
            }
        }
    }
    let (judgment, rationale) = if matches!(sources.subject_kind, AssessmentKind::Definition(..)) {
        serde_json::from_value::<corrections::AssessmentResult>(answer.clone())?.evidence()?
    } else {
        (
            Judgment::from_value(&answer["determination"]).map_err(io::Error::other)?,
            answer["rationale"]
                .as_str()
                .expect("validated rationale")
                .to_owned(),
        )
    };
    usage.record.determination = Some(judgment.kind().into());
    usage.save()?;
    let mut output = record(sources, &judgment, &rationale, answer["applies"].as_bool());
    if let Some(criteria) = criteria {
        output["criteria"] = criteria;
    }
    if let Some(items) = answer.get("corrections") {
        output["corrections"] = items.clone();
    }
    Ok(trust_but_verify::graph::Record::new(
        Some(crate::sdk::review_version()),
        stored_dependencies(&graph, &dependencies),
        output,
    ))
}

fn stored_dependencies(
    graph: &HabeasDomain,
    dependencies: &[Dependency<NodeId<HabeasDomain>, HabeasEdgeLabel>],
) -> Vec<Dependency<HabeasNode, HabeasEdgeLabel>> {
    dependencies
        .iter()
        .map(|dep| {
            use trust_but_verify::graph::Dependency;
            match dep {
                Dependency::Node(id, fp) => {
                    Dependency::Node((*graph.cache().node_data(*id).unwrap()).clone(), fp.clone())
                }
                Dependency::OutgoingEdges(id, label, fp) => Dependency::OutgoingEdges(
                    (*graph.cache().node_data(*id).unwrap()).clone(),
                    label.clone(),
                    fp.clone(),
                ),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    // {test}`requirements.sdk.habeas.graph_discovery`
    // {test}`requirements.sdk.habeas.implementation-edge`
    // {test}`requirements.sdk.graph.node_path_method`
    // {test}`requirements.sdk.graph.domain_types`
    // {test}`requirements.sdk.graph.validation-context.tracked_inspection`
    #[test]
    fn domain_shares_discovery_snapshots_with_tracked_reads() {
        let root = inputs::tests::project();
        fs::write(
            root.path().join("src/lib.rs"),
            "// {impl}`requirements.ready`\nfn helper() { call(); }\n",
        )
        .unwrap();
        let subject =
            HabeasNode::Requirement(crate::sdk::RequirementId("requirements.ready".into()));
        let graph = HabeasDomain::discover(root.path()).unwrap();
        let outlines = graph.outlines.clone();
        let subject = graph.cache().intern_node(subject);
        assert_eq!(
            graph.node_path(&subject).segments(),
            ["requirement", "requirements.ready"]
        );
        let selector = InputSelector::Outline {
            file: "src/lib.rs".into(),
            item: "fn[helper]".into(),
        };
        let item =
            trust_but_verify::outline::OutlineData::new("src/lib.rs", "fn[helper]".into()).unwrap();
        let source = graph.source_node(&item).unwrap();
        assert_eq!(source, HabeasNode::from(&selector));
        let source = graph.cache().intern_node(source);
        assert_eq!(
            graph
                .outgoing_edges(&subject, HabeasEdgeLabel::Implements)
                .unwrap(),
            [source]
        );
        let path = root.path().join("src/lib.rs");
        let snapshot =
            trust_but_verify::outline::OutlineContent::from_path(graph.cache(), &path).unwrap();
        fs::write(&path, "fn edited() {}").unwrap();
        let reread =
            trust_but_verify::outline::OutlineContent::from_path(&outlines.validation, &path)
                .unwrap();
        assert_eq!(snapshot.text(), reread.text());
        assert_eq!(
            graph.node_path(&source).segments(),
            ["source", "repo", "src/lib.rs", "fn[helper]"]
        );
        let inspection = ValidationContext::new(&graph, subject.clone());
        assert!(inspection.dependencies().is_empty());
        let delivered = inspection.read(&source).unwrap();
        assert_eq!(
            delivered.format(),
            snapshot.select("fn[helper]").unwrap().format()
        );
        assert!(delivered.format().contains("call();"));
        assert_eq!(
            inspection.dependencies(),
            vec![trust_but_verify::graph::Dependency::Node(
                source.clone(),
                delivered.fingerprint()
            )]
        );
        assert!(
            graph
                .outgoing_edges(&source, HabeasEdgeLabel::Tests)
                .unwrap()
                .is_empty()
        );
        let missing = HabeasNode::from(&InputSelector::Outline {
            file: "src/lib.rs".into(),
            item: "fn[missing]".into(),
        });
        assert!(
            inspection
                .read(&graph.cache().intern_node(missing))
                .is_err()
        );
        assert_eq!(inspection.dependencies().len(), 1);
    }

    // {test}`requirements.sdk.habeas.evidence_conversion.writes`
    // {test}`requirements.sdk.habeas.evidence_conversion.provenance`
    // {test}`requirements.sdk.habeas.evidence_conversion.legacy_freshness`
    #[tokio::test]
    async fn current_legacy_authoring_converts_without_configuration_and_stdout_never_converts() {
        let root = inputs::tests::project();
        let mut sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        sources.record_inputs(&sources.mandatory_inputs().to_vec());
        let old = record(&sources, &Judgment::True, "Historical review.", None);
        let subject = AuthoringSubject::Definition("requirements.ready".into());
        let path = subject.evidence_path(root.path());
        crate::evidence_format::write_atomic(&path, &old).unwrap();
        let result = author_evidence_with_options(
            root.path(),
            "requirements.ready",
            Some("missing-configuration"),
            EvidenceOutput::File,
            AuthoringOptions::default(),
        )
        .await
        .unwrap();
        assert!(result.is_none());
        let new: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(new["review_version"], "*");
        assert_eq!(crate::evidence_format::view(&new).unwrap(), old);
        crate::evidence_format::write_atomic(&path, &old).unwrap();
        let before = fs::read(&path).unwrap();
        assert!(
            author_evidence_with_options(
                root.path(),
                "requirements.ready",
                Some("missing-configuration"),
                EvidenceOutput::Stdout,
                AuthoringOptions::default(),
            )
            .await
            .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }
    // {test}`requirements.sdk.habeas.evidence_format`
    // {test}`requirements.sdk.habeas.evidence_format.errors`
    // {test}`requirements.sdk.habeas.evidence_format.loading`
    // {test}`requirements.sdk.habeas.evidence_conversion.legacy_freshness`
    // {test}`requirements.sdk.habeas.assessment_policy`
    // {test}`requirements.ci.schema`
    // {test}`requirements.ci.errors`
    // {test}`requirements.ci.requirement_text`
    #[tokio::test]
    async fn every_review_kind_loads_both_formats_and_reports_each_application_failure_once() {
        use trust_but_verify::domain::Domain;
        let source = format!(
            "{}\n// {{cite}}`requirements.rule`\nfn helper() {{}}\n// {{test}}`requirements.sample`\n#[test]\nfn sample() {{}}\n",
            crate::sites::tests::SOURCE
        );
        let chapter = format!(
            "{}\n:::{{example}} sample\nAn empty checker succeeds. {{cite}}`requirements.rule`\n:::\n",
            crate::sites::tests::RULE
        );
        let root = crate::sites::tests::project(&chapter, &source);
        let subjects = [
            AuthoringSubject::Definition("requirements.rule".into()),
            AuthoringSubject::Definition("requirements.sample".into()),
            AuthoringSubject::Citation {
                file: "src/lib.rs".into(),
                item: "fn[helper]".into(),
            },
            AuthoringSubject::Site {
                requirement: "requirements.rule".into(),
                file: "src/lib.rs".into(),
                item: "fn[check_two]".into(),
            },
        ];
        for subject in subjects {
            let mut sources = subject.sources(root.path()).unwrap();
            sources.record_inputs(&sources.mandatory_inputs().to_vec());
            let old = record(&sources, &Judgment::True, "Reviewed inputs.", Some(true));
            let node = HabeasNode::review_subject(&subject);
            let path = subject.evidence_path(root.path());
            let label = path
                .strip_prefix(root.path())
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let findings = || {
                crate::evidence::inspect_repository(root.path())
                    .unwrap()
                    .report
                    .errors
                    .into_iter()
                    .filter(|error| error.starts_with(&format!("{label}:")))
                    .collect::<Vec<_>>()
            };
            let definitions = crate::evidence::load_definitions(root.path()).unwrap();
            let outlines = crate::inputs::OutlineCache::default();
            let graph = HabeasDomain::new(root.path(), &definitions, &outlines);
            let decoded = crate::evidence_format::decode(&old, &node).unwrap();
            for version in ["*", crate::sdk::REVIEW_VERSION] {
                let mut native = crate::evidence_format::encode(&decoded).unwrap();
                native["review_version"] = json!(version);
                crate::evidence_format::write_atomic(&path, &old).unwrap();
                let legacy_findings = findings();
                assert!(legacy_findings.is_empty(), "{label}: {legacy_findings:?}");
                crate::evidence_format::write_atomic(&path, &native).unwrap();
                assert_eq!(findings(), legacy_findings, "{label}");
                let before = fs::read(&path).unwrap();
                let loaded = graph
                    .load_evidence(
                        &graph.cache().intern_node(node.clone()),
                        &node.review_criterion(),
                    )
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(loaded.version, CriterionVersion::from_recorded(version));
                assert_eq!(loaded.dependencies, decoded.dependencies);
                assert_eq!(loaded.output, decoded.output);
                assert_eq!(fs::read(&path).unwrap(), before);
            }
            for mutations in [
                json!({"rationale":""}),
                json!({"determination":"bad"}),
                json!({"inputs":null}),
                json!({"unexpected":true}),
                json!({"rationale":"","determination":"bad"}),
            ] {
                let mut legacy = old.clone();
                legacy
                    .as_object_mut()
                    .unwrap()
                    .extend(mutations.as_object().unwrap().clone());
                let mut native = crate::evidence_format::encode(&decoded).unwrap();
                native
                    .as_object_mut()
                    .unwrap()
                    .extend(mutations.as_object().unwrap().clone());
                crate::evidence_format::write_atomic(&path, &legacy).unwrap();
                let legacy_findings = findings();
                crate::evidence_format::write_atomic(&path, &native).unwrap();
                let native_findings = findings();
                assert!(!native_findings.is_empty(), "{label}: {mutations}");
                assert_eq!(native_findings, legacy_findings, "{label}: {mutations}");
                let mut unique = native_findings.clone();
                unique.sort();
                unique.dedup();
                assert_eq!(unique.len(), native_findings.len());
                let before = fs::read(&path).unwrap();
                assert!(
                    graph
                        .load_evidence(
                            &graph.cache().intern_node(node.clone()),
                            &node.review_criterion()
                        )
                        .await
                        .is_err()
                );
                assert_eq!(fs::read(&path).unwrap(), before);
            }
            // '*' accepts an unknown procedure version while preserving all
            // application checks. Each mutation changes one semantic concern.
            let mut stale_input = old["inputs"][0].clone();
            if stale_input.get("hash").is_some() {
                stale_input["hash"] = json!("0".repeat(64));
            } else {
                stale_input["text"] = json!(["Different historical text."]);
            }
            let mut changed_inputs = old["inputs"].clone();
            changed_inputs[0] = stale_input;
            let mut semantic_mutations = vec![
                (json!({"determination":false}), "determination is false"),
                (json!({"inputs":changed_inputs}), "changed"),
            ];
            match &subject {
                AuthoringSubject::Definition(_) => {
                    let text_field = if old.get("example").is_some() {
                        "example"
                    } else {
                        "requirement"
                    };
                    semantic_mutations
                        .push((json!({text_field:"Changed subject text."}), "text changed"));
                    semantic_mutations.push((
                        json!({"references":"0".repeat(64)}),
                        "reference set changed",
                    ));
                }
                AuthoringSubject::Citation { .. } => {
                    for field in ["file", "item"] {
                        semantic_mutations.push((json!({field:"different"}), "must be"));
                    }
                    let mut changed = old["citations"].clone();
                    changed[0]["commentary"] = json!("Changed commentary.");
                    semantic_mutations.push((json!({"citations":changed}), "citations changed"));
                }
                AuthoringSubject::Site { .. } => {
                    for field in ["requirement", "file", "item"] {
                        semantic_mutations.push((json!({field:"different"}), "must be"));
                    }
                    semantic_mutations.push((json!({"queries":["different"]}), "queries changed"));
                    semantic_mutations.push((
                        json!({"fixmes":[{"item":"fn[check_two]","commentary":"Different"}]}),
                        "changed",
                    ));
                    semantic_mutations.push((
                        json!({"applies":false,"determination":false}),
                        "applies is false",
                    ));
                }
            }
            for (mutations, expected) in semantic_mutations {
                let mut legacy = old.clone();
                legacy
                    .as_object_mut()
                    .unwrap()
                    .extend(mutations.as_object().unwrap().clone());
                // Convert historical fields without resolving current inputs.
                let changed_record = crate::evidence_format::decode(&legacy, &node).unwrap();
                let native = crate::evidence_format::encode(&changed_record).unwrap();
                assert_eq!(native["review_version"], "*");
                crate::evidence_format::write_atomic(&path, &legacy).unwrap();
                let legacy_findings = findings();
                assert!(
                    legacy_findings
                        .iter()
                        .any(|finding| finding.contains(expected)),
                    "{label}: {mutations}: {legacy_findings:?}"
                );
                crate::evidence_format::write_atomic(&path, &native).unwrap();
                assert_eq!(findings(), legacy_findings, "{label}: {mutations}");
                // Staleness is an application finding, not a loading error.
                let before = fs::read(&path).unwrap();
                assert!(
                    graph
                        .load_evidence(
                            &graph.cache().intern_node(node.clone()),
                            &node.review_criterion()
                        )
                        .await
                        .unwrap()
                        .is_some()
                );
                assert_eq!(fs::read(&path).unwrap(), before);
            }
            for version in [json!(3), json!("unsupported")] {
                let mut invalid = crate::evidence_format::encode(&decoded).unwrap();
                invalid["format_version"] = version;
                crate::evidence_format::write_atomic(&path, &invalid).unwrap();
                let errors = findings();
                assert_eq!(errors.len(), 1, "{label}: {errors:?}");
                assert!(errors[0].contains("unsupported evidence format version"));
                let before = fs::read(&path).unwrap();
                assert!(
                    graph
                        .load_evidence(
                            &graph.cache().intern_node(node.clone()),
                            &node.review_criterion()
                        )
                        .await
                        .is_err()
                );
                assert_eq!(fs::read(&path).unwrap(), before);
            }
            for header in [
                json!({"format_version":2}),
                json!({"review_version":"v1"}),
                json!({"dependencies":[]}),
            ] {
                let mut invalid = old.clone();
                invalid
                    .as_object_mut()
                    .unwrap()
                    .extend(header.as_object().unwrap().clone());
                crate::evidence_format::write_atomic(&path, &invalid).unwrap();
                assert_eq!(findings().len(), 1, "{label}: {:?}", findings());
                assert!(
                    graph
                        .load_evidence(
                            &graph.cache().intern_node(node.clone()),
                            &node.review_criterion()
                        )
                        .await
                        .is_err()
                );
            }
            fs::write(&path, b"malformed JSON").unwrap();
            assert!(
                graph
                    .load_evidence(
                        &graph.cache().intern_node(node.clone()),
                        &node.review_criterion()
                    )
                    .await
                    .is_err()
            );
            assert_eq!(fs::read(&path).unwrap(), b"malformed JSON");
            crate::evidence_format::write_atomic(&path, &old).unwrap();
            let loaded = graph
                .load_evidence(
                    &graph.cache().intern_node(node.clone()),
                    &node.review_criterion(),
                )
                .await
                .unwrap()
                .unwrap();
            assert_eq!(loaded.version, None);
            assert_eq!(loaded.output, decoded.output);
        }
    }
}

//! Habeas's review procedure and conversation policy on the SDK interfaces.

use super::*;
use crate::{
    inputs::InputSelector,
    sdk::{HabeasDomain, HabeasEdgeLabel, HabeasNode},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[cfg(test)]
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

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReviewAnswer<T> {
    determination: T,
    #[schemars(length(min = 1))]
    rationale: String,
}
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SiteAnswer {
    determination: Determination,
    #[schemars(length(min = 1))]
    rationale: String,
    applies: bool,
}
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct VerificationAnswer {
    real: bool,
    acknowledged: bool,
    acknowledgments: Vec<String>,
    #[schemars(length(min = 1))]
    rationale: String,
}
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SiteVerification {
    real: bool,
    acknowledged: bool,
    acknowledgments: Vec<String>,
    #[schemars(length(min = 1))]
    rationale: String,
    applies: bool,
}

enum Operation {
    Assess,
    Verify(Judgment, String),
}

// {impl}`requirements.sdk.graph.criteria.criterion_trait`
// {impl}`requirements.authoring.mandatory_inputs`
struct Review<'a> {
    sources: &'a AssessmentSources,
    node: NodeId<HabeasDomain>,
    criterion: CriterionPath,
    operation: Operation,
}

impl Review<'_> {
    // {impl}`requirements.authoring.line_numbers.citation`
    // {impl}`requirements.authoring.behavioral_review`
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
        let mut rejection = line_numbers::Rejection::default();
        let citation_review = matches!(self.sources.subject_kind, AssessmentKind::Citation(_));
        let request = cx
            .ask::<R>()
            .context(prompt.system)
            .message(prompt.subject)
            .vet_answer(|answer| {
                let value = serde_json::to_value(answer)?;
                if !citation_review && rejection.resubmitted(&value).is_some() {
                    return Ok(VetResult::Accept);
                }
                validate(&value)?;
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
        if matches!(self.sources.subject_kind, AssessmentKind::Definition(..)) {
            request = request.contents_of_successors(HabeasEdgeLabel::Mandatory);
        } else {
            for input in self.sources.mandatory_inputs() {
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
            .sources
            .model_mandatory_inputs()
            .into_iter()
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
        match &self.operation {
            Operation::Assess => {
                let prompt = initial_prompt_with_inputs(self.sources, &annotations)
                    .map_err(ValidationError::Graph)?;
                if site {
                    self.ask::<SiteAnswer>(cx, prompt, |v| site_submission(v).map(drop))
                        .await
                } else if matches!(self.sources.subject_kind, AssessmentKind::Citation(_)) {
                    self.ask::<ReviewAnswer<bool>>(cx, prompt, |v| submission(v).map(drop))
                        .await
                } else {
                    self.ask::<ReviewAnswer<Determination>>(cx, prompt, |v| submission(v).map(drop))
                        .await
                }
            }
            Operation::Verify(judgment, rationale) => {
                let prompt = verification_prompt_with_inputs(
                    self.sources,
                    judgment,
                    rationale,
                    &annotations,
                )
                .map_err(ValidationError::Graph)?;
                if site {
                    self.ask::<SiteVerification>(cx, prompt, |v| site_verification(v).map(drop))
                        .await
                } else {
                    self.ask::<VerificationAnswer>(cx, prompt, |v| verification(v).map(drop))
                        .await
                }
            }
        }
    }
}
// {impl}`requirements.sdk.habeas.evidence_format`
// {impl}`requirements.sdk.habeas.evidence_conversion.legacy_freshness`
// {impl}`requirements.sdk.habeas.assessment_policy`
pub(super) async fn assess(
    backend: &config::BackendConfig,
    sources: &mut AssessmentSources,
    usage: &mut UsageLog,
) -> io::Result<(Judgment, Value)> {
    let record = run(backend, sources, usage, Operation::Assess).await?;
    let value = crate::evidence_format::encode(&record)?;
    let determination = Judgment::from_value(&value["determination"]).map_err(io::Error::other)?;
    Ok((determination, value))
}

// {cite}`habeas:util`
pub(super) async fn verify(
    backend: &config::BackendConfig,
    sources: &mut AssessmentSources,
    usage: &mut UsageLog,
    judgment: &Judgment,
    rationale: &str,
) -> io::Result<Verification> {
    let site = matches!(sources.subject_kind, AssessmentKind::Site(..));
    let record = run(
        backend,
        sources,
        usage,
        Operation::Verify(judgment.clone(), rationale.into()),
    )
    .await?;
    let mut value = record.output;
    if !site {
        value
            .as_object_mut()
            .expect("verification")
            .remove("applies");
    }
    let mut output = if site {
        site_verification(&value)?
    } else {
        verification(&value)?
    };
    output.dependencies = record.dependencies;
    output.inputs = sources.recorded_inputs().to_vec();
    Ok(output)
}

/// Publish this assessment's counters even if its future is dropped during a request or backoff.
struct AssessmentUsage<'d, 'log> {
    validator: trust_but_verify::graph::Validator<'d, HabeasDomain>,
    log: &'log mut UsageLog,
}

impl Drop for AssessmentUsage<'_, '_> {
    // {impl}`requirements.authoring.usage_requests`
    // {impl}`requirements.authoring.usage_inputs`
    fn drop(&mut self) {
        self.log.observe_usage(self.validator.usage());
    }
}

async fn run(
    backend: &config::BackendConfig,
    sources: &mut AssessmentSources,
    usage: &mut UsageLog,
    operation: Operation,
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
    let criterion = node.review_criterion();
    let mut graph = sources.domain.clone();
    domain_tools::configure(&mut graph, backend, sources.github.clone())?;
    let node = graph.cache().intern_node(node);
    let mut assessment = AssessmentUsage {
        validator: trust_but_verify::graph::Validator::new(&graph),
        log: usage,
    };
    let verifying = matches!(operation, Operation::Verify(..));
    let result = assessment
        .validator
        .verify(Review {
            sources,
            criterion,
            node,
            operation,
        })
        .await;
    drop(assessment);
    usage.save()?;
    let verified = result.map_err(|error| match error {
        ValidationError::Graph(error) => error,
        ValidationError::Provider(error) => {
            io::Error::new(error.kind(), ValidationError::<io::Error>::Provider(error))
        }
        other => io::Error::other(other.to_string()),
    })?;
    // Project the SDK's observed nodes into the existing application evidence fields.
    for dependency in verified.dependencies() {
        if let Dependency::Node(id, _) = dependency {
            let data = graph.cache().node_data(*id).expect("observed node");
            if let Ok(selector) = data.selector() {
                let input = sources.read_selector(&selector)?;
                sources.record_inputs(&[input]);
            }
        }
    }
    let output = if verifying {
        verified.output().clone()
    } else {
        let answer = verified.output();
        let judgment = Judgment::from_value(&answer["determination"]).map_err(io::Error::other)?;
        usage.record.determination = Some(judgment.kind().into());
        usage.save()?;
        record(
            sources,
            &judgment,
            answer["rationale"].as_str().expect("validated rationale"),
            answer["applies"].as_bool(),
        )
    };
    Ok(trust_but_verify::graph::Record::new(
        verified.version().cloned(),
        verified
            .dependencies()
            .iter()
            .map(|dep| {
                use trust_but_verify::graph::Dependency;
                match dep {
                    Dependency::Node(id, fp) => Dependency::Node(
                        (*graph.cache().node_data(*id).unwrap()).clone(),
                        fp.clone(),
                    ),
                    Dependency::OutgoingEdges(id, label, fp) => Dependency::OutgoingEdges(
                        (*graph.cache().node_data(*id).unwrap()).clone(),
                        label.clone(),
                        fp.clone(),
                    ),
                }
            })
            .collect(),
        output,
    ))
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
            for version in [json!(2), json!("unsupported")] {
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

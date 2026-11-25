//! Habeas's review procedure and conversation policy on the SDK interfaces.

use super::*;
#[cfg(test)]
use crate::inputs::OutlineCache;
use crate::{
    inputs::{InputSelector, recorded_input},
    sdk::{HabeasDomain, HabeasEdgeLabel, HabeasNode, input_fingerprint},
};
use std::sync::{Arc, Mutex};
use trust_but_verify::domain::Domain;
#[cfg(test)]
use trust_but_verify::domain::NodeContents;
use trust_but_verify::domain::NodeId;
use trust_but_verify::graph::Criterion;
use trust_but_verify::graph::CriterionPath;
use trust_but_verify::graph::CriterionVersion;
use trust_but_verify::graph::ValidationError;
use trust_but_verify::request::Agent;
use trust_but_verify::request::AgentRequest;
use trust_but_verify::request::ValidationContext;

// {cite}`habeas:util`
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ReviewAnswer {
    determination: Value,
    rationale: String,
    applies: Option<bool>,
}

// {cite}`habeas:util`
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct VerificationAnswer {
    real: bool,
    acknowledged: bool,
    acknowledgments: Vec<String>,
    rationale: String,
    applies: Option<bool>,
}

// {cite}`habeas:util`
#[derive(Clone)]
enum Operation {
    Assess,
    Verify(Judgment, String),
}

// {cite}`habeas:util`
struct ReviewState<'a> {
    system: String,
    operation: Operation,
    conversation: Mutex<Option<(&'a mut AssessmentSources, &'a mut UsageLog)>>,
    agent_failure: Mutex<Option<io::Error>>,
}

// {impl}`requirements.sdk.graph.criteria.criterion_trait`
// {impl}`requirements.sdk.graph.agent-calls.procedures`
// {impl}`requirements.authoring.mandatory_inputs`
struct Review<'a> {
    state: Arc<ReviewState<'a>>,
    node: NodeId<HabeasDomain>,
}

impl Criterion<HabeasDomain> for Review<'_> {
    type Ok = Value;

    fn source_node_id(&self) -> NodeId<HabeasDomain> {
        self.node.clone()
    }

    fn criterion_path(&self) -> CriterionPath {
        crate::sdk::review_criterion()
    }

    fn version(&self) -> CriterionVersion {
        crate::sdk::review_version()
    }

    // {cite}`habeas:util`
    async fn execute(
        self,
        ctx: &mut ValidationContext<'_, HabeasDomain>,
    ) -> Result<Self::Ok, ValidationError<io::Error>> {
        let graph = &self.state;
        let data = ctx
            .graph()
            .cache()
            .node_data(*ctx.node_id())
            .expect("review subject");
        if let HabeasNode::Requirement(id) = data.as_ref() {
            ctx.read(
                &ctx.graph()
                    .cache()
                    .intern_node(HabeasNode::ReferenceSet(id.clone())),
            )
            .map_err(ValidationError::Graph)?;
        }
        match &graph.operation {
            Operation::Assess => {
                let request = ctx
                    .ask::<ReviewAnswer>()
                    .context(&graph.system)
                    .contents_of_current_node();
                let answer = if matches!(data.as_ref(), HabeasNode::Requirement(_)) {
                    request
                        .contents_of_successors(HabeasEdgeLabel::Mandatory)
                        .await?
                } else {
                    request.await?
                };
                let judgment =
                    Judgment::from_value(&answer.determination).map_err(ValidationError::Agent)?;
                let mut conversation = graph.conversation.lock().expect("review lease lock");
                let (sources, usage) = conversation
                    .as_mut()
                    .expect("conversation returned its lease");
                usage.record.determination = Some(judgment.kind().into());
                usage.save().map_err(ValidationError::Graph)?;
                Ok(record(
                    sources,
                    &judgment,
                    &answer.rationale,
                    answer.applies,
                ))
            }
            Operation::Verify(..) => {
                let request = ctx
                    .ask::<VerificationAnswer>()
                    .context(&graph.system)
                    .contents_of_current_node();
                let answer = if matches!(data.as_ref(), HabeasNode::Requirement(_)) {
                    request
                        .contents_of_successors(HabeasEdgeLabel::Mandatory)
                        .await?
                } else {
                    request.await?
                };
                Ok(
                    json!({"real":answer.real, "acknowledged":answer.acknowledged,
                    "acknowledgments":answer.acknowledgments, "rationale":answer.rationale, "applies":answer.applies}),
                )
            }
        }
    }
}

// {cite}`habeas:util`
struct ReviewAgent<'a, P>(P, Arc<ReviewState<'a>>);

// {impl}`requirements.sdk.graph.agent-calls.author_procedure`
// {impl}`requirements.sdk.graph.agent-calls.judge.caching`
// {impl}`requirements.sdk.graph.agent-calls.judge.submission`
impl<P> Agent<HabeasDomain> for ReviewAgent<'_, P>
where
    P: std::ops::Deref + Send + Sync,
    P::Target: CompletionProvider + Sized,
{
    fn run<'a>(
        &'a self,
        request: AgentRequest<NodeId<HabeasDomain>>,
        inspection: &'a ValidationContext<'_, HabeasDomain>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Value, std::io::Error>> + Send + 'a>,
    > {
        Box::pin(async move {
            let graph = &self.1;
            let cache = inspection.graph().cache();
            // A short lease exchange protects ownership, never an interaction.
            let (sources, usage) = graph
                .conversation
                .lock()
                .expect("review lease lock")
                .take()
                .ok_or_else(|| io::Error::other("this Habeas review already has a conversation"))?;
            let result = async {
                let mut mandatory = sources.model_mandatory_inputs();
                for input in &request.inputs {
                    let mut rendered: Value =
                        serde_json::from_str(&input.text).map_err(io::Error::other)?;
                    let selector = cache
                        .node_data(input.node)
                        .expect("supplied node")
                        .selector()?;
                    let target = mandatory
                        .iter_mut()
                        .find(|value| InputSelector::from_input(value).as_ref() == Some(&selector))
                        .ok_or_else(|| {
                            io::Error::other(
                                "SDK selected an input outside this review's mandatory set",
                            )
                        })?;
                    if let Some(issues) = target.get("github_issues") {
                        rendered["github_issues"] = issues.clone();
                    }
                    *target = rendered;
                }
                let mut prompt = match &graph.operation {
                    Operation::Assess => initial_prompt_with_inputs(sources, &mandatory)?,
                    Operation::Verify(judgment, rationale) => {
                        verification_prompt_with_inputs(sources, judgment, rationale, &mandatory)?
                    }
                };
                prompt.system = request.prefix;
                let site = matches!(sources.subject_kind, AssessmentKind::Site(..));
                let observe = |inputs: &[Value]| {
                    for input in inputs {
                        if input.get("builtin").is_some() {
                            continue;
                        }
                        let input = recorded_input(input);
                        if let Some(selector) = InputSelector::from_input(&input) {
                            inspection.observe_read(
                                cache.intern_node(HabeasNode::from(&selector)),
                                input_fingerprint(&input),
                            );
                            for supplied in &request.inputs {
                                if cache
                                    .node_data(supplied.node)
                                    .and_then(|data| data.selector().ok())
                                    .as_ref()
                                    == Some(&selector)
                                {
                                    inspection.observe_read(
                                        supplied.node.clone(),
                                        supplied.fingerprint.clone(),
                                    );
                                }
                            }
                        }
                    }
                };
                let accepted = |arguments: &Value| {
                    match &graph.operation {
                        Operation::Assess => {
                            if site {
                                site_submission(arguments)?;
                            } else {
                                submission(arguments)?;
                            }
                        }
                        Operation::Verify(..) => {
                            if site {
                                site_verification(arguments)?;
                            } else {
                                verification(arguments)?;
                            }
                        }
                    }
                    Ok(arguments.clone())
                };
                let submit = match graph.operation {
                    Operation::Assess => "submit_judgment",
                    Operation::Verify(..) => "submit_verification",
                };
                let value = converse_observed(
                    &*self.0,
                    sources,
                    usage,
                    prompt,
                    (submit, site),
                    accepted,
                    Some(&observe),
                )
                .await?;
                Ok::<_, io::Error>(value)
            }
            .await;
            *graph.conversation.lock().expect("review lease lock") = Some((sources, usage));
            match result {
                Ok(value) => Ok(value),
                Err(error) => {
                    let message = error.to_string();
                    *graph.agent_failure.lock().expect("review error lock") = Some(error);
                    Err(io::Error::other(message))
                }
            }
        })
    }
}

// {impl}`requirements.sdk.habeas.evidence_format`
// {impl}`requirements.sdk.habeas.evidence_conversion.legacy_freshness`
// {impl}`requirements.sdk.habeas.assessment_policy`
pub(super) async fn assess(
    provider: &impl CompletionProvider,
    sources: &mut AssessmentSources,
    usage: &mut UsageLog,
) -> io::Result<(Judgment, Value)> {
    let record = run(provider, sources, usage, Operation::Assess).await?;
    let value = crate::evidence_format::encode(&record)?;
    let determination = Judgment::from_value(&value["determination"]).map_err(io::Error::other)?;
    Ok((determination, value))
}

// {cite}`habeas:util`
pub(super) async fn verify(
    provider: &impl CompletionProvider,
    sources: &mut AssessmentSources,
    usage: &mut UsageLog,
    judgment: &Judgment,
    rationale: &str,
) -> io::Result<Verification> {
    let site = matches!(sources.subject_kind, AssessmentKind::Site(..));
    let record = run(
        provider,
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

async fn run(
    provider: &impl CompletionProvider,
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
    let graph = sources.domain.clone();
    let node = graph.cache().intern_node(node);
    let state = Arc::new(ReviewState {
        system: match &operation {
            Operation::Assess => initial_prompt(sources)?.system,
            Operation::Verify(judgment, rationale) => {
                verification_prompt(sources, judgment, rationale)?.system
            }
        },
        operation,
        conversation: Mutex::new(Some((sources, usage))),
        agent_failure: Mutex::new(None),
    });
    let agent = ReviewAgent(provider, state.clone());
    let review = Review {
        node,
        state: state.clone(),
    };
    let verified =
        match trust_but_verify::agent_runtime::verify_with_agent(&graph, &agent, review).await {
            Ok(result) => result,
            Err(ValidationError::Graph(error)) => return Err(error),
            Err(ValidationError::Agent(message)) => {
                return Err(state
                    .agent_failure
                    .lock()
                    .expect("review error lock")
                    .take()
                    .unwrap_or_else(|| io::Error::other(message)));
            }
            Err(ValidationError::NoAgentConfigured) => {
                return Err(io::Error::other("no agent is configured"));
            }
            Err(ValidationError::Output(error)) => return Err(io::Error::other(error)),
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
        verified.into_output(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig_core::{
        completion::{CompletionResponse, FinishReason, Usage},
        message::{AssistantContent, Origin, ToolCall, ToolFunction, ToolName},
    };
    use std::collections::VecDeque;

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
            serde_json::from_str::<Value>(&delivered.format()).unwrap(),
            selector.to_model_input(
                &selector
                    .resolve(&graph.root, &graph.definitions, &graph.outlines)
                    .unwrap()
            )
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

    // {test}`requirements.sdk.graph.criteria.criterion_trait`
    // {test}`requirements.sdk.graph.criteria.criterion_trait.answer`
    // {test}`requirements.sdk.graph.criteria.criterion_trait.source_node_id`
    // {test}`requirements.sdk.graph.criteria.criterion_trait.version`
    // {test}`requirements.sdk.graph.criteria.execute`
    // {test}`requirements.sdk.graph.evidence.check_node`
    // {test}`requirements.sdk.graph.running-criteria.validate_node`
    // {test}`requirements.sdk.graph.evidence.evidence_storage`
    // {test}`requirements.sdk.habeas.evidence_format.loading`
    #[tokio::test]
    async fn domain_persists_and_reuses_a_negative_validation_through_the_sdk() {
        use trust_but_verify::graph as validation;

        let root = inputs::tests::project();
        let state = tempfile::tempdir().unwrap();
        let mut sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let mut usage = UsageLog::start(state.path(), root.path(), "requirements.ready").unwrap();
        let node = HabeasNode::Requirement(crate::sdk::RequirementId("requirements.ready".into()));
        let graph = sources.domain.clone();
        let state = Arc::new(ReviewState {
            system: initial_prompt(&sources).unwrap().system,
            operation: Operation::Assess,
            conversation: Mutex::new(Some((&mut sources, &mut usage))),
            agent_failure: Mutex::new(None),
        });
        let provider = Arc::new(script(vec![(
            "submit_judgment",
            json!({"determination":false,"rationale":"The readiness case is missing."}),
        )]));
        let agent = Arc::new(ReviewAgent(provider.clone(), state.clone()));
        let path = node.evidence_path(root.path()).unwrap();
        let node = graph.cache().intern_node(node);
        let review = || Review {
            node,
            state: state.clone(),
        };
        assert_eq!(review().source_node_id(), node);
        assert_eq!(review().criterion_path().segments(), ["review"]);
        assert_eq!(review().version().as_str(), crate::sdk::REVIEW_VERSION);
        assert!(
            validation::load_evidence(&graph, &review())
                .await
                .unwrap()
                .is_none()
        );
        assert!(!path.exists());
        assert!(provider.requests.lock().unwrap().is_empty());

        let fingerprinted =
            trust_but_verify::agent_runtime::verify_with_agent(&graph, agent.as_ref(), review())
                .await
                .unwrap();
        let answer = fingerprinted.output();
        assert_eq!(answer["determination"], false);
        assert_eq!(answer["rationale"], "The readiness case is missing.");
        assert!(answer.get("result").is_none());
        assert!(answer.get("metadata").is_none());
        assert!(!path.exists());
        validation::store_evidence(&graph, &fingerprinted)
            .await
            .unwrap();
        let saved = fs::read(&path).unwrap();
        let record = graph
            .load_evidence(&node, &review().criterion_path())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            record.version.as_ref().unwrap().as_str(),
            crate::sdk::REVIEW_VERSION
        );
        assert_eq!(record.output, *answer);
        assert!(!record.dependencies.is_empty());
        let cached = validation::load_evidence(&graph, &review())
            .await
            .unwrap()
            .unwrap();
        assert!(review().version().is_compatible_with(cached.version()));
        assert!(validation::check_inputs(&graph, &cached).unwrap());
        assert_eq!(cached.output(), answer);
        assert_eq!(provider.requests.lock().unwrap().len(), 1);
        assert_eq!(fs::read(&path).unwrap(), saved);

        let other = CriterionPath::new(vec!["another".into(), "review".into()]);
        assert!(graph.load_evidence(&node, &other).await.unwrap().is_none());
        assert!(graph.store_evidence(&node, &other, &record).await.is_err());
        assert_eq!(fs::read(&path).unwrap(), saved);
    }

    struct Script {
        turns: Mutex<VecDeque<(&'static str, Value)>>,
        requests: Mutex<Vec<CompletionRequest>>,
    }
    impl CompletionProvider for Script {
        async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
            self.requests.lock().unwrap().push(request);
            let (name, arguments) = self
                .turns
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected request");
            Ok(CompletionResponse::new(
                vec![AssistantContent::ToolCall(ToolCall::from_wire(
                    "call",
                    ToolFunction::new(ToolName::new(name).unwrap(), arguments),
                ))],
                Usage::default(),
                Origin::new("test", "test", "test"),
                Value::Null,
            )
            .with_finish_reason(FinishReason::ToolCalls))
        }
    }
    fn script(turns: Vec<(&'static str, Value)>) -> Script {
        Script {
            turns: Mutex::new(turns.into()),
            requests: Mutex::default(),
        }
    }

    // {test}`requirements.authoring.provider_backoff`
    // {test}`requirements.authoring.usage_requests.failures`
    // {test}`requirements.sdk.habeas.assessment_policy`
    #[tokio::test]
    async fn sdk_string_errors_preserve_habeas_provider_reports_and_error_kinds() {
        struct FailingProvider(io::ErrorKind);
        impl CompletionProvider for FailingProvider {
            async fn complete(&self, _: CompletionRequest) -> io::Result<CompletionResponse> {
                Err(io::Error::new(
                    self.0,
                    rig_core::error::ProviderError::from_provider_body("Original provider report.")
                        .with_provider_status(Some(403.try_into().unwrap()))
                        .with_provider_code(Some("AccessDeniedException".into())),
                ))
            }
        }
        for kind in [io::ErrorKind::Other, io::ErrorKind::PermissionDenied] {
            let root = inputs::tests::project();
            let state = tempfile::tempdir().unwrap();
            let mut sources =
                AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
            let mut usage =
                UsageLog::start(state.path(), root.path(), "requirements.ready").unwrap();
            let error = assess(&FailingProvider(kind), &mut sources, &mut usage)
                .await
                .unwrap_err();
            assert_eq!(error.kind(), kind);
            assert_eq!(
                providers::failure_codes(&error),
                (Some(403), Some("AccessDeniedException".into()))
            );
            assert!(
                error
                    .get_ref()
                    .unwrap()
                    .downcast_ref::<rig_core::error::ProviderError>()
                    .is_some()
            );
            assert!(error.to_string().contains("Original provider report."));
            assert!(
                !AuthoringSubject::Definition("requirements.ready".into())
                    .evidence_path(root.path())
                    .exists()
            );
        }
    }

    // {test}`requirements.sdk.habeas.evidence_format`
    // {test}`requirements.sdk.habeas.evidence_conversion.legacy_freshness`
    // {test}`requirements.sdk.graph.agent-calls.judge.rendering`
    // {test}`requirements.sdk.graph.agent-calls.judge.caching`
    #[tokio::test]
    async fn authoring_keeps_the_prepared_subject_snapshot_and_concrete_version() {
        let root = inputs::tests::project();
        let state = tempfile::tempdir().unwrap();
        let mut sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let original = sources.mandatory_inputs()[0].clone();
        let original_subject_text = sources.subject_text.clone();
        let original_prompt = initial_prompt(&sources).unwrap();
        fs::write(
            root.path().join("md/requirements.md"),
            ":::{req} ready\nThe project MUST have changed.\n:::\n",
        )
        .unwrap();
        let mut usage = UsageLog::start(state.path(), root.path(), "requirements.ready").unwrap();
        let provider = script(vec![(
            "submit_judgment",
            json!({"determination":true,"rationale":"Reviewed original snapshot."}),
        )]);
        let (_, value) = assess(&provider, &mut sources, &mut usage).await.unwrap();
        assert_eq!(value["review_version"], crate::sdk::REVIEW_VERSION);
        assert_ne!(value["review_version"], "*");
        assert_eq!(value["requirement"], original_subject_text);
        let mut expected = original_prompt.request();
        expected.tools = tool_definitions_with("submit_judgment", false).unwrap();
        assert_eq!(
            serde_json::to_value(&provider.requests.lock().unwrap()[0]).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        let node = HabeasNode::Requirement(crate::sdk::RequirementId("requirements.ready".into()));
        let record = crate::evidence_format::decode(&value, &node).unwrap();
        assert!(
            record
                .dependencies
                .contains(&trust_but_verify::graph::Dependency::Node(
                    node,
                    input_fingerprint(&original)
                ))
        );
        let definitions = crate::evidence::load_definitions(root.path()).unwrap();
        let outlines = OutlineCache::default();
        let graph = HabeasDomain::new(root.path(), &definitions, &outlines);
        assert!(
            !record
                .is_current(&graph, &crate::sdk::review_version())
                .unwrap()
        );
    }

    // {test}`requirements.authoring.confirmation`
    // {test}`requirements.authoring.delivered_inputs`
    // {test}`requirements.sdk.habeas.evidence_format`
    // {test}`requirements.sdk.graph.agent-calls.author_procedure`
    #[tokio::test]
    async fn confirmed_evidence_retains_reads_from_both_assessments_and_the_verifier() {
        let root = inputs::tests::project();
        fs::write(
            root.path().join("src/lib.rs"),
            "fn first() {}\nfn second() {}\nfn third() {}\n",
        )
        .unwrap();
        let state = tempfile::tempdir().unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let mut usage = UsageLog::start(state.path(), root.path(), "requirements.ready").unwrap();
        let provider = script(vec![
            ("habeas", json!({"file":"src/lib.rs","item":"fn[first]"})),
            (
                "submit_judgment",
                json!({"determination":true,"rationale":"First assessment."}),
            ),
            ("habeas", json!({"file":"src/lib.rs","item":"fn[second]"})),
            (
                "submit_judgment",
                json!({"determination":false,"rationale":"Second assessment."}),
            ),
            ("habeas", json!({"file":"src/lib.rs","item":"fn[third]"})),
            (
                "submit_verification",
                json!({"real":false,"acknowledged":false,"acknowledgments":[],"rationale":"The claimed gap is absent."}),
            ),
        ]);
        let (judgment, value) = assess_subject(
            &provider,
            root.path(),
            state.path(),
            &AuthoringSubject::Definition("requirements.ready".into()),
            sources,
            &mut usage,
            AssessmentPolicy::Confirm,
        )
        .await
        .unwrap();
        assert_eq!(judgment, Judgment::True);
        assert!(
            value["rationale"]
                .as_str()
                .unwrap()
                .contains(DISAGREEMENT_NOTE)
        );
        let record = crate::evidence_format::decode(
            &value,
            &HabeasNode::Requirement(crate::sdk::RequirementId("requirements.ready".into())),
        )
        .unwrap();
        for item in ["fn[first]", "fn[second]", "fn[third]"] {
            let input = value["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|input| input["item"] == item)
                .unwrap();
            let node = HabeasNode::from(&InputSelector::from_input(input).unwrap());
            assert!(
                record
                    .dependencies
                    .contains(&trust_but_verify::graph::Dependency::Node(
                        node,
                        input_fingerprint(input)
                    ))
            );
        }
        assert_eq!(provider.requests.lock().unwrap().len(), 6);
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
                        &crate::sdk::review_criterion(),
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
                            &crate::sdk::review_criterion()
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
                            &crate::sdk::review_criterion()
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
                            &crate::sdk::review_criterion()
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
                            &crate::sdk::review_criterion()
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
                        &crate::sdk::review_criterion()
                    )
                    .await
                    .is_err()
            );
            assert_eq!(fs::read(&path).unwrap(), b"malformed JSON");
            crate::evidence_format::write_atomic(&path, &old).unwrap();
            let loaded = graph
                .load_evidence(
                    &graph.cache().intern_node(node.clone()),
                    &crate::sdk::review_criterion(),
                )
                .await
                .unwrap()
                .unwrap();
            assert_eq!(loaded.version, None);
            assert_eq!(loaded.output, decoded.output);
        }
    }
}

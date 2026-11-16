//! Habeas's review procedure and conversation policy on the SDK interfaces.

use super::*;
use crate::{
    inputs::{InputSelector, OutlineCache, recorded_input},
    sdk::{HabeasDomain, HabeasEdgeLabel, HabeasNode, input_fingerprint},
};
use habeas_sdk::{
    graph::{
        Edge, Fingerprint, Fingerprinting, Graph, InspectionContext, NodePath, Snapshot,
        Validation, ValidationContext, ValidationError,
    },
    procedure::{Agent, AgentInput, AgentInputRenderer, AgentRequest},
};
use std::sync::Mutex;

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ReviewAnswer {
    determination: Value,
    rationale: String,
    applies: Option<bool>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct VerificationAnswer {
    real: bool,
    acknowledged: bool,
    acknowledgments: Vec<String>,
    rationale: String,
    applies: Option<bool>,
}

enum Operation {
    Assess,
    Verify(Judgment, String),
}

/// Immutable snapshots identify the contents used to assemble this review.
struct ReviewGraph<'a> {
    root: PathBuf,
    node: HabeasNode,
    definitions: std::collections::BTreeMap<String, crate::evidence::Declaration>,
    outlines: OutlineCache,
    mandatory: Vec<(HabeasNode, Value)>,
    reference_set: String,
    system: String,
    operation: Operation,
    conversation: Mutex<Option<(&'a mut AssessmentSources, &'a mut UsageLog)>>,
}

impl Graph for ReviewGraph<'_> {
    type NodeId = HabeasNode;
    type NodeContent = Value;
    type EdgeLabel = HabeasEdgeLabel;
    type JudgmentMetadata = Value;
    type Error = io::Error;

    fn node_content(&self, node: &HabeasNode) -> io::Result<Value> {
        if let Some((_, input)) = self.mandatory.iter().find(|(id, _)| id == node) {
            return Ok(input.clone());
        }
        let selector = node.selector()?;
        let contents = self.domain().node_content(node)?;
        Ok(selector.to_model_input(&contents))
    }
    fn node_path(&self, node: &HabeasNode) -> NodePath {
        self.domain().node_path(node)
    }
    fn outgoing_edges(
        &self,
        node: &HabeasNode,
    ) -> io::Result<Snapshot<Vec<Edge<HabeasNode, HabeasEdgeLabel>>>> {
        if node == &self.node && matches!(node, HabeasNode::Requirement(_)) {
            // This graph presents the already selected mandatory relationships.
            // Their provenance remains protected by the complete reference set.
            Ok(Snapshot {
                content: self
                    .mandatory
                    .iter()
                    .filter(|(id, _)| id != node)
                    .map(|(id, _)| Edge {
                        label: HabeasEdgeLabel::Mandatory,
                        target: id.clone(),
                    })
                    .collect(),
                fingerprint: Fingerprint {
                    scheme: "habeas-reference-set-v1".into(),
                    value: self.reference_set.clone(),
                },
            })
        } else {
            self.domain().outgoing_edges(node)
        }
    }
}

impl ReviewGraph<'_> {
    fn domain(&self) -> HabeasDomain<'_> {
        HabeasDomain {
            root: &self.root,
            definitions: &self.definitions,
            outlines: &self.outlines,
        }
    }
}

impl Fingerprinting for ReviewGraph<'_> {
    fn fingerprint(&self, _: &HabeasNode, content: &Value) -> Fingerprint {
        input_fingerprint(&model_recorded_input(content))
    }
}

fn model_recorded_input(value: &Value) -> Value {
    let mut value = value.clone();
    if let Some(object) = value.as_object_mut() {
        object.remove("github_issues");
    }
    recorded_input(&value)
}

struct Renderer;
impl AgentInputRenderer<ReviewGraph<'_>> for Renderer {
    fn render(&self, input: &AgentInput<'_, ReviewGraph<'_>>) -> io::Result<String> {
        serde_json::to_string(input.contents).map_err(io::Error::other)
    }
}

// {impl}`requirements.sdk.graph.validation.check_procedure`
// {impl}`requirements.sdk.graph.agent-calls.procedures`
// {impl}`requirements.authoring.mandatory_inputs`
impl Validation for ReviewGraph<'_> {
    async fn validate<'a, 'd>(
        &'a self,
        ctx: &'a ValidationContext<'d, Self>,
    ) -> Result<habeas_sdk::graph::Judgment<Value>, ValidationError<io::Error>>
    where
        'd: 'a,
    {
        let (judgment, rationale, applies, verification) = match &self.operation {
            Operation::Assess => {
                let request = ctx
                    .ask::<ReviewAnswer>()
                    .context(&self.system)
                    .current_node_contents();
                let answer = if matches!(ctx.node_id(), HabeasNode::Requirement(_)) {
                    request
                        .successors_content(HabeasEdgeLabel::Mandatory)
                        .await?
                } else {
                    request.await?
                };
                let judgment = Judgment::from_value(&answer.determination)
                    .map_err(|error| ValidationError::Agent(io::Error::other(error)))?;
                (judgment, answer.rationale, answer.applies, None)
            }
            Operation::Verify(..) => {
                let request = ctx
                    .ask::<VerificationAnswer>()
                    .context(&self.system)
                    .current_node_contents();
                let answer = if matches!(ctx.node_id(), HabeasNode::Requirement(_)) {
                    request
                        .successors_content(HabeasEdgeLabel::Mandatory)
                        .await?
                } else {
                    request.await?
                };
                let metadata = json!({"real":answer.real, "acknowledged":answer.acknowledged,
                    "acknowledgments":answer.acknowledgments, "rationale":answer.rationale, "applies":answer.applies});
                (
                    if !answer.real || answer.acknowledged {
                        Judgment::True
                    } else {
                        Judgment::False
                    },
                    answer.rationale,
                    answer.applies,
                    Some(metadata),
                )
            }
        };
        let mut conversation = self.conversation.lock().expect("review lease lock");
        let (sources, usage) = conversation
            .as_mut()
            .expect("conversation returned its lease");
        let metadata = if let Some(metadata) = verification {
            metadata
        } else {
            usage.record.determination = Some(judgment.kind().into());
            usage.save().map_err(ValidationError::Graph)?;
            record(sources, &judgment, &rationale, applies)
        };
        Ok(habeas_sdk::graph::Judgment {
            result: judgment != Judgment::False,
            rationale,
            metadata,
        })
    }
}

struct ReviewAgent<'p, P>(&'p P);

// {impl}`requirements.sdk.graph.agent-calls.author_procedure`
// {impl}`requirements.sdk.graph.agent-calls.judge.caching`
// {impl}`requirements.sdk.graph.agent-calls.judge.submission`
impl<P: CompletionProvider> Agent<ReviewGraph<'_>> for ReviewAgent<'_, P> {
    fn run<'a>(
        &'a self,
        request: AgentRequest<HabeasNode>,
        inspection: &'a InspectionContext<'_, ReviewGraph<'_>>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Value, std::io::Error>> + Send + 'a>,
    > {
        Box::pin(async move {
            let graph = inspection.graph();
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
                    let rendered: Value =
                        serde_json::from_str(&input.text).map_err(io::Error::other)?;
                    let selector = input.node.selector()?;
                    let target = mandatory
                        .iter_mut()
                        .find(|value| InputSelector::from_input(value).as_ref() == Some(&selector))
                        .ok_or_else(|| {
                            io::Error::other(
                                "SDK selected an input outside this review's mandatory set",
                            )
                        })?;
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
                                HabeasNode::from(&selector),
                                input_fingerprint(&input),
                            );
                            for supplied in &request.inputs {
                                if supplied.node.selector().ok().as_ref() == Some(&selector) {
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
                    self.0,
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
            result
        })
    }
}

// {impl}`requirements.sdk.habeas.evidence_format`
// {impl}`requirements.sdk.habeas.evidence_conversion.legacy_freshness`
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
    let mut value = record.output.metadata;
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
) -> io::Result<habeas_sdk::graph::Record<HabeasNode, Value>> {
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
    let mut mandatory = sources
        .model_mandatory_inputs()
        .into_iter()
        .filter_map(|input| {
            InputSelector::from_input(&input).map(|selector| (HabeasNode::from(&selector), input))
        })
        .collect::<Vec<_>>();
    // The reviewed source is also the current node for citation/site reviews.
    if !matches!(node, HabeasNode::Requirement(_)) {
        let selector = node.selector()?;
        let input = mandatory
            .iter()
            .find(|(id, _)| id.selector().ok().as_ref() == Some(&selector))
            .ok_or_else(|| io::Error::other("missing review subject input"))?
            .1
            .clone();
        mandatory.push((node.clone(), input));
    }
    let graph = ReviewGraph {
        root: sources.root.clone(),
        node: node.clone(),
        definitions: crate::evidence::load_definitions(&sources.root)?,
        outlines: sources.outlines.clone(),
        mandatory,
        reference_set: sources.reference_set.clone(),
        system: match &operation {
            Operation::Assess => initial_prompt(sources)?.system,
            Operation::Verify(judgment, rationale) => {
                verification_prompt(sources, judgment, rationale)?.system
            }
        },
        operation,
        conversation: Mutex::new(Some((sources, usage))),
    };
    let agent = ReviewAgent(provider);
    let ctx = ValidationContext::with_agent(&graph, node.clone(), &agent, &Renderer);
    let judgment = match graph.validate(&ctx).await {
        Ok(judgment) => judgment,
        Err(ValidationError::Graph(error) | ValidationError::Agent(error)) => return Err(error),
        Err(ValidationError::NoAgentConfigured) => {
            return Err(io::Error::other("no agent is configured"));
        }
    };
    Ok(ctx
        .inspection()
        .finish(crate::sdk::review_version(&node)?, judgment))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig_core::{
        completion::{CompletionResponse, FinishReason, Usage},
        message::{AssistantContent, Origin, ToolCall, ToolFunction, ToolName},
    };
    use std::collections::VecDeque;

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

    // {test}`requirements.sdk.habeas.evidence_format`
    // {test}`requirements.sdk.habeas.evidence_conversion.legacy_freshness`
    // {test}`requirements.sdk.graph.agent-calls.judge.rendering`
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
                .contains(&habeas_sdk::graph::Dependency::Node(
                    node,
                    input_fingerprint(&original)
                ))
        );
        let definitions = crate::evidence::load_definitions(root.path()).unwrap();
        let outlines = OutlineCache::default();
        let graph = HabeasDomain {
            root: root.path(),
            definitions: &definitions,
            outlines: &outlines,
        };
        assert!(
            !habeas_sdk::graph::record_is_current(&graph, crate::sdk::REVIEW_VERSION, &record)
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
                    .contains(&habeas_sdk::graph::Dependency::Node(
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
        let state = tempfile::tempdir().unwrap();
        let mut usage = UsageLog::start(state.path(), root.path(), "requirements.ready").unwrap();
        assert!(
            configured_assessment(
                root.path(),
                "requirements.ready",
                Some("missing-configuration"),
                EvidenceOutput::Stdout,
                None,
                &mut usage
            )
            .await
            .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }
}

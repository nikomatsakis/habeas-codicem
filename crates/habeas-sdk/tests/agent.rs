use habeas_sdk::{agent::*, graph::*, procedure::*, resource::*, sources::*};
use rig_core::{
    completion::{CompletionRequest, CompletionResponse, FinishReason, Usage},
    message::{AssistantContent, Message, Origin, ToolCall, ToolFunction, ToolName},
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io,
    sync::{Arc, Mutex},
};

// These graph values deliberately have no Serde or Hash implementations.
#[derive(Clone, Eq, PartialEq)]
struct Node(u8);
#[derive(Clone)]
struct Label;
struct Contents(String);
struct Metadata;
struct Memory(Arc<Mutex<String>>);
impl Graph for Memory {
    type NodeId = Node;
    type NodeContent = Contents;
    type EdgeLabel = Label;
    type JudgmentMetadata = Metadata;
    type Error = io::Error;
    fn node_content(&self, _: &Node) -> io::Result<Contents> {
        Ok(Contents(self.0.lock().unwrap().clone()))
    }
    fn node_path(&self, node: &Node) -> NodePath {
        NodePath {
            segments: vec![node.0.to_string()],
        }
    }
    fn outgoing_edges(&self, node: &Node) -> io::Result<Snapshot<Vec<Edge<Node, Label>>>> {
        Ok(Snapshot {
            content: if node.0 == 1 {
                vec![Edge {
                    label: Label,
                    target: Node(2),
                }]
            } else {
                vec![]
            },
            fingerprint: Fingerprint {
                scheme: "edges".into(),
                value: "ordered-edge-snapshot".into(),
            },
        })
    }
}
impl Fingerprinting for Memory {
    fn fingerprint(&self, _: &Node, value: &Contents) -> Fingerprint {
        Fingerprint {
            scheme: "content".into(),
            value: value.0.clone(),
        }
    }
}
impl Validation for Memory {
    async fn validate<'a, 'd>(
        &'a self,
        _: &'a ValidationContext<'d, Self>,
    ) -> Result<Judgment<Metadata>, ValidationError<io::Error>>
    where
        'd: 'a,
    {
        unreachable!("tests invoke the builder directly")
    }
}
struct Renderer;
impl AgentInputRenderer<Memory> for Renderer {
    fn render(&self, input: &AgentInput<'_, Memory>) -> io::Result<String> {
        Ok(input.contents.0.clone())
    }
}
struct Script {
    turns: Mutex<VecDeque<Option<Vec<(&'static str, Value)>>>>,
    requests: Mutex<Vec<CompletionRequest>>,
}
impl Script {
    fn new(turns: Vec<Option<Vec<(&'static str, Value)>>>) -> Self {
        Self {
            turns: Mutex::new(turns.into()),
            requests: Mutex::default(),
        }
    }
}
impl CompletionProvider for Script {
    async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
        self.requests.lock().unwrap().push(request);
        let Some(calls) = self
            .turns
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected extra request")
        else {
            return Err(io::Error::other("permanent provider failure"));
        };
        let choice = calls
            .into_iter()
            .enumerate()
            .map(|(index, (name, args))| {
                AssistantContent::ToolCall(ToolCall::from_wire(
                    format!("call-{index}"),
                    ToolFunction::new(ToolName::new(name).unwrap(), args),
                ))
            })
            .collect();
        Ok(CompletionResponse::new(
            choice,
            Usage::default(),
            Origin::new("test", "test", "test"),
            Value::Null,
        )
        .with_finish_reason(FinishReason::ToolCalls))
    }
}

// {test}`requirements.sdk.graph.agent-calls.judge.submission`
// {test}`requirements.sdk.graph.agent-calls.judge.caching`
// {test}`requirements.sdk.graph.domain_types`
#[tokio::test]
async fn default_tool_accepts_scalar_results_and_keeps_the_exact_prefix() {
    let graph = Memory(Arc::new(Mutex::new("source".into())));
    let agent = ToolAgent::new(
        Script::new(vec![Some(vec![("submit_result", json!({"result":true}))])]),
        Renderer,
    );
    let ctx = ValidationContext::with_agent(&graph, Node(1), &agent, &agent.renderer);
    assert!(
        ctx.ask::<bool>()
            .context("Question é".to_owned())
            .await
            .unwrap()
    );
    assert!(ctx.inspection().dependencies().is_empty());
    let requests = agent.provider.requests.lock().unwrap();
    assert_eq!(requests[0].chat_history[0], Message::system("Question é"));
    let schema = &requests[0]
        .tools
        .iter()
        .find(|tool| tool.name == "submit_result")
        .unwrap()
        .parameters;
    assert_eq!(schema["properties"]["result"]["type"], "boolean");
}

#[derive(Deserialize, JsonSchema)]
struct Answer {
    decision: Decision,
}
#[derive(Deserialize, JsonSchema)]
enum Decision {
    Yes,
    No,
}

// {test}`requirements.sdk.graph.agent-calls.judge.submission`
#[tokio::test]
async fn typed_submission_references_resolve_and_invalid_results_are_agent_errors() {
    let graph = Memory(Arc::new(Mutex::new("source".into())));
    let agent = ToolAgent::new(
        Script::new(vec![Some(vec![(
            "submit_result",
            json!({"result":{"decision":"Yes"}}),
        )])]),
        Renderer,
    );
    let ctx = ValidationContext::with_agent(&graph, Node(1), &agent, &agent.renderer);
    assert!(matches!(
        ctx.ask::<Answer>().await.unwrap().decision,
        Decision::Yes
    ));
    let requests = agent.provider.requests.lock().unwrap();
    let schema = &requests[0]
        .tools
        .iter()
        .find(|tool| tool.name == "submit_result")
        .unwrap()
        .parameters;
    assert!(matches!(requests[0].chat_history[0], Message::User { .. }));
    assert!(schema["$defs"]["Decision"].is_object());
    assert_eq!(
        schema["properties"]["result"]["properties"]["decision"]["$ref"],
        "#/$defs/Decision"
    );
    drop(requests);
    for calls in [
        vec![("submit_result", json!({"result":"wrong"}))],
        vec![
            ("submit_result", json!({"result":true})),
            ("submit_result", json!({"result":true})),
        ],
        vec![("submit_result", json!({"result":true,"extra":true}))],
    ] {
        let agent = ToolAgent::new(Script::new(vec![Some(calls)]), Renderer);
        let ctx = ValidationContext::with_agent(&graph, Node(1), &agent, &agent.renderer);
        assert!(matches!(
            ctx.ask::<bool>().await,
            Err(ValidationError::Agent(_))
        ));
    }
}

// {test}`requirements.sdk.graph.context-tools.tracked_inspection`
// {test}`requirements.sdk.graph.agent-calls.author_procedure`
#[tokio::test]
async fn graph_tools_record_delivered_snapshots_and_prepared_pages_do_not_record_delivery() {
    let graph = Memory(Arc::new(Mutex::new("x".repeat(18_000))));
    let agent = ToolAgent::new(
        Script::new(vec![
            Some(vec![("outgoing_edges", json!({"path":{"segments":["1"]}}))]),
            Some(vec![("read_node", json!({"path":{"segments":["2"]}}))]),
            Some(vec![("get_node_page", json!({"page":1}))]),
            Some(vec![("submit_result", json!({"result":true}))]),
        ]),
        Renderer,
    );
    let ctx = ValidationContext::with_agent(&graph, Node(1), &agent, &agent.renderer);
    assert!(ctx.ask::<bool>().await.unwrap());
    let observations = ctx.inspection().dependencies();
    assert_eq!(observations.len(), 2);
    assert!(
        matches!(&observations[0],Dependency::OutgoingEdges(Node(1),fingerprint) if fingerprint.value == "ordered-edge-snapshot")
    );
    assert!(
        matches!(&observations[1],Dependency::Node(Node(2),fingerprint) if fingerprint.value.len() == 18_000)
    );
    let failed = ToolAgent::new(
        Script::new(vec![
            Some(vec![("read_node", json!({"path":{"segments":["1"]}}))]),
            None,
        ]),
        Renderer,
    );
    let ctx = ValidationContext::with_agent(&graph, Node(1), &failed, &failed.renderer);
    assert!(ctx.ask::<bool>().await.is_err());
    assert!(ctx.inspection().dependencies().is_empty());
    let failed = ToolAgent::new(
        Script::new(vec![
            Some(vec![("read_node", json!({"path":{"segments":["1"]}}))]),
            None,
        ]),
        Renderer,
    );
    let ctx = ValidationContext::with_agent(&graph, Node(1), &failed, &failed.renderer);
    assert!(ctx.ask::<bool>().current_node_contents().await.is_err());
    assert_eq!(ctx.inspection().dependencies().len(), 1);
}

// {test}`requirements.sdk.sources.provider_observations`
// {test}`requirements.sdk.sources.provider_defaults`
// {test}`requirements.sdk.graph.context-tools.tracked_inspection`
#[tokio::test]
async fn source_tools_keep_search_discovery_separate_from_delivered_source_reads() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("note.txt"), "observed source\n").unwrap();
    let graph = Memory(Arc::new(Mutex::new("unrelated graph bytes".into())));
    let factory = SourceTools {
        analysis: SourceAnalysis::new(
            LocalSources::new(root.path()),
            habeas_sdk::language::Registry::new([]),
        ),
        observation: |_: &OutlineId, parsed: &ParsedSource| {
            Ok(Some(Dependency::Node(
                Node(7),
                Fingerprint {
                    scheme: "provided-snapshot".into(),
                    value: parsed.source.text().unwrap().into(),
                },
            )))
        },
    };
    let agent = ToolAgent::new(
        Script::new(vec![
            Some(vec![("search_text", json!({"pattern":"observed"}))]),
            Some(vec![(
                "read_source",
                json!({"resource":{"scheme":"repo","location":"note.txt"},"selector":""}),
            )]),
            Some(vec![("submit_result", json!({"result":true}))]),
        ]),
        Renderer,
    )
    .with_tools(factory);
    let ctx = ValidationContext::with_agent(&graph, Node(1), &agent, &agent.renderer);
    assert!(ctx.ask::<bool>().await.unwrap());
    let observations = ctx.inspection().dependencies();
    assert_eq!(observations.len(), 1);
    assert!(
        matches!(&observations[0],Dependency::Node(Node(7),fingerprint) if fingerprint.value == "observed source\n")
    );
}

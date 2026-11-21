use habeas_sdk::{agent::*, graph::*, procedure::Context, resource::*, sources::*};
use rig_core::{
    completion::{CompletionRequest, CompletionResponse, FinishReason, ToolDefinition, Usage},
    message::{
        AssistantContent, Message, Origin, ToolCall, ToolFunction, ToolName, ToolResultContent,
        UserContent,
    },
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io,
    sync::{Arc, Mutex},
};

// The test application chooses its own completed-answer format.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Assessment<M> {
    result: bool,
    rationale: String,
    metadata: M,
}

// Node identities deliberately have no Serde or Hash implementations.
#[derive(Clone, Eq, PartialEq)]
struct Node(u8);
// {cite}`habeas:util`
#[derive(Clone, PartialEq, Eq, Deserialize, JsonSchema)]
struct Label;
// {cite}`habeas:util`
struct Content(String);
// {cite}`habeas:util`
#[derive(Serialize, Deserialize)]
struct Metadata;
#[derive(Clone)]
struct Memory {
    contents: Arc<Mutex<String>>,
    node_contents: Arc<Mutex<std::collections::BTreeMap<u8, String>>>,
    reads: Arc<Mutex<Vec<u8>>>,
    formatter: fn(&Node, &Content) -> io::Result<String>,
    records: Arc<Mutex<Vec<(Node, CriterionPath, Record<Node, Label>)>>>,
}
impl Memory {
    fn new(contents: String) -> Self {
        Self {
            contents: Arc::new(Mutex::new(contents)),
            node_contents: Arc::default(),
            reads: Arc::default(),
            records: Arc::default(),
            formatter: |_, content| Ok(content.0.clone()),
        }
    }
}
impl Storage for Memory {
    async fn load_evidence(
        &self,
        node: &Node,
        criterion: &CriterionPath,
    ) -> io::Result<Option<Record<Node, Label>>> {
        Ok(self
            .records
            .lock()
            .unwrap()
            .iter()
            .find(|(n, c, _)| n == node && c == criterion)
            .map(|(_, _, r)| r.clone()))
    }
    async fn store_evidence(
        &self,
        node: &Node,
        criterion: &CriterionPath,
        record: &Record<Node, Label>,
    ) -> io::Result<()> {
        let mut records = self.records.lock().unwrap();
        records.retain(|(n, c, _)| n != node || c != criterion);
        records.push((node.clone(), criterion.clone(), record.clone()));
        Ok(())
    }
}
impl Graph for Memory {
    type NodeId = Node;
    type EdgeLabel = Label;

    type Error = io::Error;

    fn node_path(&self, node: &Node) -> NodePath {
        NodePath::new(vec![node.0.to_string()])
    }
    fn outgoing_edges(&self, node: &Node, _: Label) -> io::Result<Vec<Node>> {
        Ok(if node.0 == 1 { vec![Node(2)] } else { vec![] })
    }
}
// {cite}`habeas:util`
impl Contents for Memory {
    type NodeContent = Content;

    fn node_content(&self, node: &Node) -> io::Result<Content> {
        self.reads.lock().unwrap().push(node.0);
        Ok(Content(
            self.node_contents
                .lock()
                .unwrap()
                .get(&node.0)
                .cloned()
                .unwrap_or_else(|| self.contents.lock().unwrap().clone()),
        ))
    }

    fn fingerprint(&self, _: &Node, value: &Content) -> Fingerprint {
        Fingerprint {
            scheme: "content".into(),
            value: value.0.clone(),
        }
    }

    fn format_node_content(&self, node: &Node, contents: &Content) -> io::Result<String> {
        (self.formatter)(node, contents)
    }
}
struct Review(Node);
impl Criterion<Memory> for Review {
    type Ok = Assessment<Metadata>;
    fn source_node_id(&self) -> Node {
        self.0.clone()
    }
    fn criterion_path(&self) -> CriterionPath {
        CriterionPath::new(vec![])
    }
    fn revision(&self) -> String {
        "v1".into()
    }
    async fn execute(
        self,
        ctx: &mut ValidationContext<'_, Memory>,
    ) -> Result<Assessment<Metadata>, ValidationError<io::Error>> {
        let text = ctx
            .inspection()
            .read(ctx.node_id())
            .map_err(ValidationError::Graph)?;
        Ok(Assessment {
            result: false,
            rationale: text.0,
            metadata: Metadata,
        })
    }
}

// {cite}`habeas:util`
struct Script {
    turns: Mutex<VecDeque<Option<Vec<(&'static str, Value)>>>>,
    requests: Mutex<Vec<CompletionRequest>>,
}
impl Script {
    // {cite}`habeas:util`
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
// {test}`requirements.sdk.graph.agent-calls.judge.rendering`
#[tokio::test]
async fn default_tool_accepts_scalar_results_and_keeps_the_exact_prefix() {
    let graph = Memory::new("source".into());
    let agent = ToolAgent::new(Script::new(vec![Some(vec![(
        "submit_result",
        json!({"result":true}),
    )])]));
    let ctx = ValidationContext::with_agent(&graph, Node(1), &agent);
    assert!(
        ctx.ask::<bool>()
            .context("Question é".to_owned())
            .contents_of_current_node()
            .await
            .unwrap()
    );
    assert!(
        matches!(&ctx.inspection().dependencies()[0], Dependency::Node(Node(1), fingerprint) if fingerprint.value == "source")
    );
    let requests = agent.provider.requests.lock().unwrap();
    assert_eq!(requests[0].chat_history[0], Message::system("Question é"));
    assert!(
        serde_json::to_string(&requests[0].chat_history[1])
            .unwrap()
            .contains("source")
    );
    let schema = &requests[0]
        .tools
        .iter()
        .find(|tool| tool.name == "submit_result")
        .unwrap()
        .parameters;
    assert_eq!(schema["properties"]["result"]["type"], "boolean");
}

// {cite}`habeas:util`
fn distinct_pages(node: &Node, contents: &Content) -> io::Result<String> {
    let label = if node.0 == 1 {
        "ALPHA-CONTENTS"
    } else {
        "BETA-CONTENTS"
    };
    Ok(format!("{label}:{}", contents.0).repeat(1_500))
}

// {cite}`habeas:util`
struct ParallelConversations {
    barrier: tokio::sync::Barrier,
    calls: Mutex<std::collections::BTreeMap<String, usize>>,
}
impl CompletionProvider for ParallelConversations {
    // {cite}`habeas:util`
    async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
        let alpha = request.chat_history[0] == Message::system("alpha");
        let (prefix, contents, other, node) = if alpha {
            ("alpha", "ALPHA-CONTENTS", "BETA-CONTENTS", "1")
        } else {
            assert_eq!(request.chat_history[0], Message::system("beta"));
            ("beta", "BETA-CONTENTS", "ALPHA-CONTENTS", "2")
        };
        let turn = {
            let mut calls = self.calls.lock().unwrap();
            let count = calls.entry(prefix.into()).or_default();
            let turn = *count;
            *count += 1;
            turn
        };
        assert_eq!(request.chat_history.len(), 2 + turn * 2);
        let history = serde_json::to_string(&request.chat_history).unwrap();
        assert!(history.contains(contents));
        assert!(!history.contains(other), "{prefix}: {history}");
        if turn > 0 {
            // The last delivered page must belong to this conversation, even
            // though both calls use the same numeric page IDs and tool call ID.
            let page = serde_json::to_string(request.chat_history.last().unwrap()).unwrap();
            assert!(page.contains(contents));
            assert!(!page.contains(other), "{prefix}: {page}");
        }
        self.barrier.wait().await;
        let (name, arguments) = match turn {
            0 => ("read_node", json!({"path":{"segments":[node]}})),
            1 => ("get_node_page", json!({"page":1})),
            2 => ("submit_result", json!({"result":prefix})),
            _ => panic!("unexpected conversation turn {turn}"),
        };
        Ok(CompletionResponse::new(
            vec![AssistantContent::ToolCall(ToolCall::from_wire(
                "same-call-id",
                ToolFunction::new(ToolName::new(name).unwrap(), arguments),
            ))],
            Usage::default(),
            Origin::new("test", "test", "test"),
            Value::Null,
        )
        .with_finish_reason(FinishReason::ToolCalls))
    }
}

// {test}`requirements.sdk.domain`
// {test}`requirements.sdk.graph.agent-calls.author_procedure`
// {test}`requirements.sdk.graph.context-tools.tracked_inspection`
// {test}`requirements.sdk.graph.agent-harness.tool_agent`
#[tokio::test]
async fn concurrent_tool_agents_isolate_histories_pages_and_submissions_and_share_reads() {
    use std::future::IntoFuture;
    let mut graph = Memory::new("source".into());
    graph.formatter = distinct_pages;
    let agent = ToolAgent::new(ParallelConversations {
        barrier: tokio::sync::Barrier::new(2),
        calls: Mutex::default(),
    });
    let ctx = ValidationContext::with_agent(&graph, Node(1), &agent);
    let (alpha, beta) = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        futures::join!(
            ctx.ask::<String>()
                .context("alpha".to_owned())
                .contents_of_current_node()
                .into_future(),
            ctx.ask::<String>()
                .context("beta".to_owned())
                .contents_of_successors(Label)
                .into_future(),
        )
    })
    .await
    .unwrap();
    assert_eq!(alpha.unwrap(), "alpha");
    assert_eq!(beta.unwrap(), "beta");
    let calls = agent.provider.calls.lock().unwrap();
    assert_eq!(calls["alpha"], 3);
    assert_eq!(calls["beta"], 3);
    let dependencies = ctx.inspection().dependencies();
    assert_eq!(dependencies.len(), 3);
    for expected in [
        Dependency::OutgoingEdges(Node(1), Label, edge_fingerprint(&graph, &[Node(2)])),
        Dependency::Node(
            Node(1),
            Fingerprint {
                scheme: "content".into(),
                value: "source".into(),
            },
        ),
        Dependency::Node(
            Node(2),
            Fingerprint {
                scheme: "content".into(),
                value: "source".into(),
            },
        ),
    ] {
        assert!(dependencies.contains(&expected));
    }
    assert!(
        ctx.inspection()
            .finish(
                "v1".into(),
                Assessment {
                    result: true,
                    rationale: "both calls".into(),
                    metadata: Metadata,
                }
            )
            .unwrap()
            .dependencies
            == dependencies
    );
}

struct Question {
    name: String,
    successors: bool,
}
impl Criterion<Memory> for Question {
    type Ok = String;
    fn source_node_id(&self) -> Node {
        Node(1)
    }
    fn criterion_path(&self) -> CriterionPath {
        CriterionPath::new(vec![self.name.clone()])
    }
    fn revision(&self) -> String {
        "v1".into()
    }
    async fn execute(
        self,
        ctx: &mut ValidationContext<'_, Memory>,
    ) -> Result<String, ValidationError<io::Error>> {
        let request = ctx.ask::<String>().context(self.name);
        if self.successors {
            request.contents_of_successors(Label).await
        } else {
            request.contents_of_current_node().await
        }
    }
}

// {test}`requirements.sdk.graph.validation.validator`
// {test}`requirements.sdk.graph.validation.validate_node`
// {test}`requirements.sdk.graph.agent-calls.judge.rendering`
// {test}`requirements.sdk.graph.agent-calls.author_procedure`
// {test}`requirements.sdk.graph.agent-harness.tool_agent`
// {test}`requirements.sdk.graph.agent-harness.setup`
#[tokio::test]
async fn validator_agent_executions_isolate_conversations_and_dependency_traces() {
    let mut graph = Memory::new("source".into());
    graph.formatter = distinct_pages;
    let agent = ToolAgent::new(ParallelConversations {
        barrier: tokio::sync::Barrier::new(2),
        calls: Mutex::default(),
    });
    let agent = Arc::new(agent);
    let validator = Validator::new(graph.clone()).with_agent(agent.clone());
    let question = |name: &str, successors| Question {
        name: name.into(),
        successors,
    };
    let (alpha, beta) = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        futures::join!(
            validator.verify(question("alpha", false)),
            validator.verify(question("beta", true)),
        )
    })
    .await
    .unwrap();
    let (alpha, beta) = (alpha.unwrap(), beta.unwrap());
    assert_eq!(alpha.output(), "alpha");
    assert_eq!(beta.output(), "beta");
    validator.store_evidence(&alpha).await.unwrap();
    validator.store_evidence(&beta).await.unwrap();
    {
        let calls = agent.provider.calls.lock().unwrap();
        assert_eq!(calls["alpha"], 3);
        assert_eq!(calls["beta"], 3);
    }
    let alpha_record = graph
        .load_evidence(&Node(1), &question("alpha", false).criterion_path())
        .await
        .unwrap()
        .unwrap();
    let beta_record = graph
        .load_evidence(&Node(1), &question("beta", true).criterion_path())
        .await
        .unwrap()
        .unwrap();
    assert!(
        alpha_record.dependencies
            == [Dependency::Node(
                Node(1),
                Fingerprint {
                    scheme: "content".into(),
                    value: "source".into()
                }
            ),]
    );
    assert!(
        beta_record.dependencies
            == [
                Dependency::OutgoingEdges(Node(1), Label, edge_fingerprint(&graph, &[Node(2)])),
                Dependency::Node(
                    Node(2),
                    Fingerprint {
                        scheme: "content".into(),
                        value: "source".into()
                    }
                ),
            ]
    );
    assert_eq!(
        validator
            .load_evidence(&question("alpha", false))
            .await
            .unwrap()
            .unwrap()
            .into_output(),
        "alpha"
    );
    assert_eq!(agent.provider.calls.lock().unwrap()["alpha"], 3);
}

// {test}`requirements.sdk.graph.context-tools.tracked_inspection`
// {test}`requirements.sdk.graph.fingerprinting.snapshot`
// {test}`requirements.sdk.graph.validation.validate_node`
// {test}`requirements.sdk.graph.validation.check_node`
// {test}`requirements.sdk.graph.validation.store_evidence`
// {test}`requirements.sdk.graph.evidence.inputs`
#[tokio::test]
async fn agent_edge_tools_record_selections_without_validating_targets() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[derive(Clone)]
    struct GraphWithChangingEdges {
        lookups: Arc<AtomicUsize>,
        records: Arc<Mutex<Vec<(u8, Record<u8, ()>)>>>,
    }
    impl Graph for GraphWithChangingEdges {
        type NodeId = u8;
        type EdgeLabel = ();

        type Error = io::Error;

        fn node_path(&self, node: &u8) -> NodePath {
            NodePath::new(vec![node.to_string()])
        }
        fn outgoing_edges(&self, node: &u8, _: ()) -> io::Result<Vec<u8>> {
            if *node == 1 && self.lookups.fetch_add(1, Ordering::SeqCst) != 0 {
                return Err(io::Error::other("must not relist inspected edges"));
            }
            Ok(if *node == 1 { vec![2] } else { vec![] })
        }
    }

    impl Contents for GraphWithChangingEdges {
        type NodeContent = String;

        // :::{cite} habeas:coverage
        // This edge-only validation regression deliberately supplies no node
        // contents. Its records assert selection dependencies only. Contents
        // requires this callback, but reaching it would introduce an unwanted
        // content read and fail this guard.
        // :::
        fn node_content(&self, _: &u8) -> io::Result<String> {
            panic!("edge inspection must not read node contents")
        }

        // :::{cite} habeas:coverage
        // Contents is required by validation, but this regression
        // observes only the outgoing-edge selection. No node contents
        // are read, so producing a content fingerprint would violate the
        // asserted selection-only dependencies.
        // :::
        fn fingerprint(&self, _: &u8, _: &String) -> Fingerprint {
            panic!("edge inspection must not fingerprint node contents")
        }

        // :::{cite} habeas:coverage
        // The agent asks only for outgoing edges and submits without reading
        // contents. Contents requires a formatter, but any
        // rendering would imply an unwanted content input. Other agent tests
        // exercise rendering with explicitly requested contents.
        // :::
        fn format_node_content(&self, _: &u8, _: &String) -> io::Result<String> {
            panic!("edge-only conversation must not render contents")
        }
    }
    impl Storage for GraphWithChangingEdges {
        async fn load_evidence(
            &self,
            node: &u8,
            _: &CriterionPath,
        ) -> io::Result<Option<Record<u8, ()>>> {
            Ok(self
                .records
                .lock()
                .unwrap()
                .iter()
                .find(|(id, _)| id == node)
                .map(|(_, record)| record.clone()))
        }
        async fn store_evidence(
            &self,
            node: &u8,
            _: &CriterionPath,
            record: &Record<u8, ()>,
        ) -> io::Result<()> {
            self.records.lock().unwrap().push((*node, record.clone()));
            Ok(())
        }
    }
    struct EdgeReview(u8);
    impl Criterion<GraphWithChangingEdges> for EdgeReview {
        type Ok = Assessment<()>;
        fn source_node_id(&self) -> u8 {
            self.0
        }
        fn criterion_path(&self) -> CriterionPath {
            CriterionPath::new(vec![])
        }
        fn revision(&self) -> String {
            "v1".into()
        }
        async fn execute(
            self,
            ctx: &mut ValidationContext<'_, GraphWithChangingEdges>,
        ) -> Result<Assessment<()>, ValidationError<io::Error>> {
            let result = if *ctx.node_id() == 1 {
                ctx.ask::<bool>().await?
            } else {
                true
            };
            Ok(Assessment {
                result,
                rationale: "reviewed".into(),
                metadata: (),
            })
        }
    }

    let graph = GraphWithChangingEdges {
        lookups: Arc::new(AtomicUsize::new(0)),
        records: Arc::default(),
    };
    let agent = ToolAgent::new(Script::new(vec![
        Some(vec![(
            "outgoing_edges",
            json!({"path":{"segments":["1"]},"label":null}),
        )]),
        Some(vec![("submit_result", json!({"result":true}))]),
    ]));
    let agent = Arc::new(agent);
    let validator = Validator::new(graph.clone()).with_agent(agent.clone());
    assert!(
        validator
            .load_evidence(&EdgeReview(1))
            .await
            .unwrap()
            .is_none()
    );
    let result = validator.verify(EdgeReview(1)).await.unwrap();
    assert!(result.output().result);
    validator.store_evidence(&result).await.unwrap();
    let saved = validator
        .load_evidence(&EdgeReview(1))
        .await
        .unwrap()
        .unwrap();
    assert!(saved.output().result);
    assert_eq!(saved.dependencies(), result.dependencies());
    assert_eq!(graph.lookups.load(Ordering::SeqCst), 1);
    let records = graph.records.lock().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].0, 1);
    assert_eq!(
        records
            .iter()
            .find(|(id, _)| *id == 1)
            .unwrap()
            .1
            .dependencies,
        vec![Dependency::OutgoingEdges(
            1,
            (),
            edge_fingerprint(&graph, &[2])
        )]
    );
    let requests = agent.provider.requests.lock().unwrap();
    let delivered = serde_json::to_string(requests[1].chat_history.last().unwrap()).unwrap();
    assert!(
        delivered.contains("targets") && delivered.contains("2"),
        "{delivered}"
    );
}

// {cite}`habeas:util`
#[derive(Deserialize, JsonSchema)]
struct Answer {
    decision: Decision,
}
// {cite}`habeas:util`
#[derive(Deserialize, JsonSchema)]
enum Decision {
    Yes,
    No,
}

// {test}`requirements.sdk.graph.agent-calls.judge.submission`
#[tokio::test]
async fn typed_submission_references_resolve_and_invalid_results_are_agent_errors() {
    let graph = Memory::new("source".into());
    let agent = ToolAgent::new(Script::new(vec![Some(vec![(
        "submit_result",
        json!({"result":{"decision":"Yes"}}),
    )])]));
    let ctx = ValidationContext::with_agent(&graph, Node(1), &agent);
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
        let agent = ToolAgent::new(Script::new(vec![Some(calls)]));
        let ctx = ValidationContext::with_agent(&graph, Node(1), &agent);
        assert!(matches!(
            ctx.ask::<bool>().await,
            Err(ValidationError::Agent(_))
        ));
    }
}

// {test}`requirements.sdk.graph.context-tools.tracked_inspection`
// {test}`requirements.sdk.graph.agent-calls.author_procedure`
// {test}`requirements.sdk.graph.evidence.node_inputs`
// {test}`requirements.sdk.graph.fingerprinting.snapshot`
#[tokio::test]
async fn graph_tools_record_delivered_snapshots_and_prepared_pages_do_not_record_delivery() {
    let graph = Memory::new("x".repeat(18_000));
    let agent = ToolAgent::new(Script::new(vec![
        Some(vec![(
            "outgoing_edges",
            json!({"path":{"segments":["1"]},"label":null}),
        )]),
        Some(vec![("read_node", json!({"path":{"segments":["2"]}}))]),
        Some(vec![("get_node_page", json!({"page":1}))]),
        Some(vec![("submit_result", json!({"result":true}))]),
    ]));
    let ctx = ValidationContext::with_agent(&graph, Node(1), &agent);
    assert!(ctx.ask::<bool>().await.unwrap());
    let observations = ctx.inspection().dependencies();
    assert_eq!(observations.len(), 2);
    assert!(
        matches!(&observations[0],Dependency::OutgoingEdges(Node(1), Label, fingerprint) if *fingerprint == edge_fingerprint(&graph, &[Node(2)]))
    );
    assert!(
        matches!(&observations[1],Dependency::Node(Node(2),fingerprint) if fingerprint.value == "x".repeat(18_000))
    );
    let record = ctx
        .inspection()
        .finish(
            "v1".into(),
            Assessment {
                result: true,
                rationale: "read".into(),
                metadata: Metadata,
            },
        )
        .unwrap();
    assert!(record.dependencies == observations);
    let failed = ToolAgent::new(Script::new(vec![
        Some(vec![("read_node", json!({"path":{"segments":["1"]}}))]),
        None,
    ]));
    let ctx = ValidationContext::with_agent(&graph, Node(1), &failed);
    assert!(ctx.ask::<bool>().await.is_err());
    assert!(ctx.inspection().dependencies().is_empty());
    let failed = ToolAgent::new(Script::new(vec![
        Some(vec![("read_node", json!({"path":{"segments":["1"]}}))]),
        None,
    ]));
    let ctx = ValidationContext::with_agent(&graph, Node(1), &failed);
    assert!(ctx.ask::<bool>().contents_of_current_node().await.is_err());
    assert_eq!(ctx.inspection().dependencies().len(), 1);
}

// {test}`requirements.sdk.sources.provider_observations`
// {test}`requirements.sdk.sources.provider_defaults`
// {test}`requirements.sdk.graph.context-tools.tracked_inspection`
// {test}`requirements.sdk.graph.agent-harness.tool_agent`
#[tokio::test]
async fn source_tools_keep_search_discovery_separate_from_delivered_source_reads() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("note.txt"), "observed source\n").unwrap();
    let graph = Memory::new("unrelated graph bytes".into());
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
    let agent = ToolAgent::new(Script::new(vec![
        Some(vec![("search_text", json!({"pattern":"observed"}))]),
        Some(vec![(
            "read_source",
            json!({"resource":{"scheme":"repo","location":"note.txt"},"selector":""}),
        )]),
        Some(vec![("submit_result", json!({"result":true}))]),
    ]))
    .with_tools(factory);
    let ctx = ValidationContext::with_agent(&graph, Node(1), &agent);
    assert!(ctx.ask::<bool>().await.unwrap());
    let observations = ctx.inspection().dependencies();
    assert_eq!(observations.len(), 1);
    assert!(
        matches!(&observations[0],Dependency::Node(Node(7),fingerprint) if fingerprint.value == "observed source\n")
    );
}

// {test}`requirements.sdk.graph.validation.check_procedure`
// {test}`requirements.sdk.graph.domain_types`
// {test}`requirements.sdk.domain`
// {test}`requirements.sdk.graph.agent-calls.author_procedure`
// {test}`requirements.sdk.graph.execution.deterministic_procedures`
#[tokio::test]
async fn deterministic_validation_uses_required_storage_without_an_agent() {
    let graph = Memory::new("ordinary computation".into());
    let validator = Validator::new(graph.clone());
    let result = validator.verify(Review(Node(1))).await.unwrap();
    validator.store_evidence(&result).await.unwrap();
    let saved = graph
        .load_evidence(&Node(1), &CriterionPath::new(vec![]))
        .await
        .unwrap()
        .unwrap();
    assert!(!result.output().result);
    assert_eq!(result.output().rationale, "ordinary computation");
    assert!(
        matches!(&saved.dependencies[0],Dependency::Node(Node(1),fingerprint) if fingerprint.value=="ordinary computation")
    );
}

// {cite}`habeas:util`
struct TextOnly(Mutex<Vec<CompletionRequest>>);
impl CompletionProvider for TextOnly {
    async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
        self.0.lock().unwrap().push(request);
        Ok(CompletionResponse::new(
            vec![AssistantContent::text("true")],
            Usage::default(),
            Origin::new("test", "test", "test"),
            Value::Null,
        )
        .with_finish_reason(FinishReason::Stop))
    }
}

// {test}`requirements.sdk.graph.agent-calls.judge.submission`
#[tokio::test]
async fn assistant_text_and_provider_failure_cannot_become_typed_results() {
    let graph = Memory::new("source".into());
    let agent = ToolAgent::new(TextOnly(Mutex::default()));
    let ctx = ValidationContext::with_agent(&graph, Node(1), &agent);
    assert!(
        matches!(ctx.ask::<bool>().await,Err(ValidationError::Agent(error)) if error.to_string().contains("without invoking submit_result"))
    );
    let requests = agent.provider.0.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(
        serde_json::to_string(&requests[0].chat_history)
            .unwrap()
            .contains("Finish by calling submit_result")
    );
    let failing = ToolAgent::new(Script::new(vec![None]));
    let ctx = ValidationContext::with_agent(&graph, Node(1), &failing);
    assert!(
        matches!(ctx.ask::<bool>().await,Err(ValidationError::Agent(error)) if error.to_string().contains("permanent provider failure"))
    );
    assert!(ctx.inspection().dependencies().is_empty());
}

// {test}`requirements.sdk.graph.context-tools.tracked_inspection`
// {test}`requirements.sdk.sources.provider_observations`
#[tokio::test]
async fn source_discovery_and_its_pages_do_not_register_reads() {
    let root = tempfile::tempdir().unwrap();
    let text = (0..1500)
        .map(|n| format!("line {n} hit\n"))
        .collect::<String>();
    std::fs::write(root.path().join("note.txt"), text).unwrap();
    let calls = Arc::new(Mutex::new(0));
    let seen = calls.clone();
    let factory = SourceTools {
        analysis: SourceAnalysis::new(
            LocalSources::new(root.path()),
            habeas_sdk::language::Registry::new([]),
        ),
        observation: move |_: &OutlineId, _: &ParsedSource| {
            *seen.lock().unwrap() += 1;
            Ok(Some(Dependency::Node(
                Node(7),
                Fingerprint {
                    scheme: "snapshot".into(),
                    value: "unused".into(),
                },
            )))
        },
    };
    let mut session = <SourceTools<_, _> as ToolFactory<Memory>>::session(&factory);
    for (name, args) in [
        ("list_sources", json!({})),
        ("search_text", json!({"pattern":"hit"})),
        (
            "outline_source",
            json!({"resource":{"scheme":"repo","location":"note.txt"}}),
        ),
    ] {
        let delivery = session.dispatch(name, &args).await.unwrap();
        assert!(delivery.dependencies.is_empty());
        if let Some(pages) =
            serde_json::from_str::<Value>(&delivery.text).unwrap()["pages"].as_array()
        {
            for page in pages {
                assert!(
                    session
                        .dispatch("get_source_page", &json!({"page":page}))
                        .await
                        .unwrap()
                        .dependencies
                        .is_empty()
                );
            }
        }
    }
    assert_eq!(*calls.lock().unwrap(), 0);
}

#[cfg(feature = "default-languages")]
struct CachedProvider;
#[cfg(feature = "default-languages")]
impl SourceProvider for CachedProvider {
    type Error = SourceError;
    async fn list(&self, _: &SourceScope) -> Result<Vec<ResourceId>, SourceError> {
        Ok(vec![ResourceId {
            scheme: "generated".into(),
            location: "cached".into(),
        }])
    }
    // :::{cite} habeas:coverage
    // The provider overrides parse with a cached snapshot. Selected-source
    // reads must deliver and fingerprint that snapshot without fetching again;
    // this trait-required callback rejects a refetch. Primitive provider reads
    // are covered separately, and executing this guard would fail this test.
    // :::
    async fn read(&self, _: &ResourceId) -> Result<SourceFile, SourceError> {
        Err(SourceError::Invalid("must not refetch".into()))
    }
    async fn parse(
        &self,
        resource: &ResourceId,
        languages: &habeas_sdk::language::Registry,
    ) -> Result<ParsedSource, SourceError> {
        parse_snapshot(
            resource,
            SourceFile {
                bytes: b"fn observed() { call(); }\n".as_slice().into(),
                language: Some("rust".into()),
            },
            languages,
        )
    }
}

// {cite}`habeas:util`
struct Scenario {
    turns: Mutex<VecDeque<io::Result<CompletionResponse>>>,
    requests: Mutex<Vec<CompletionRequest>>,
}
impl Scenario {
    // {cite}`habeas:util`
    fn new(turns: Vec<io::Result<CompletionResponse>>) -> Self {
        Self {
            turns: Mutex::new(turns.into()),
            requests: Mutex::default(),
        }
    }
}
impl CompletionProvider for Scenario {
    // {cite}`habeas:util`
    async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
        self.requests.lock().unwrap().push(request);
        self.turns
            .lock()
            .unwrap()
            .pop_front()
            .expect("extra provider request")
    }
    // {cite}`habeas:util`
    fn output_limit(&self) -> Option<u64> {
        Some(32)
    }
}

// {cite}`habeas:util`
fn tool_turn(reason: FinishReason, calls: Vec<(&str, Value)>) -> CompletionResponse {
    CompletionResponse::new(
        calls
            .into_iter()
            .enumerate()
            .map(|(index, (name, args))| {
                AssistantContent::ToolCall(ToolCall::from_wire(
                    format!("call-{index}"),
                    ToolFunction::new(ToolName::new(name).unwrap(), args),
                ))
            })
            .collect(),
        Usage::default(),
        Origin::new("test", "test", "test"),
        Value::Null,
    )
    .with_finish_reason(reason)
}

// {cite}`habeas:util`
fn throttled() -> io::Error {
    io::Error::other(
        rig_core::error::ProviderError::from_provider_body("retry this request")
            .with_provider_status(Some(429.try_into().unwrap()))
            .with_provider_code(Some("ThrottlingException".into())),
    )
}

// {test}`requirements.sdk.graph.agent-calls.judge.submission`
// {test}`requirements.sdk.graph.context-tools.tracked_inspection`
#[tokio::test]
async fn unknown_tools_return_errors_without_creating_inputs_or_preventing_submission() {
    let graph = Memory::new("source".into());
    let agent = ToolAgent::new(Script::new(vec![
        Some(vec![("invented", json!({}))]),
        Some(vec![("submit_result", json!({"result":true}))]),
    ]));
    let ctx = ValidationContext::with_agent(&graph, Node(1), &agent);
    assert!(ctx.ask::<bool>().await.unwrap());
    assert!(ctx.inspection().dependencies().is_empty());
    let requests = agent.provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    let delivered = serde_json::to_string(requests[1].chat_history.last().unwrap()).unwrap();
    assert!(delivered.contains("invented") && delivered.contains("unknown tool"));
}

// {test}`requirements.sdk.graph.agent-calls.judge.rendering`
// {test}`requirements.sdk.graph.context-tools.tracked_inspection`
// {test}`requirements.sdk.graph.agent-calls.author_procedure`
#[tokio::test]
async fn node_rendering_tool_errors_allow_recovery_without_recording_undelivered_contents() {
    let mut graph = Memory::new("source".into());
    graph.formatter = |_, _| Err(io::Error::other("unsupported node contents"));
    let agent = ToolAgent::new(Script::new(vec![
        Some(vec![("read_node", json!({"path":{"segments":["1"]}}))]),
        Some(vec![("submit_result", json!({"result":true}))]),
    ]));
    let ctx = ValidationContext::with_agent(&graph, Node(1), &agent);
    assert!(ctx.ask::<bool>().await.unwrap());
    assert!(ctx.inspection().dependencies().is_empty());
    let requests = agent.provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    let Message::User { content } = requests[1].chat_history.last().unwrap() else {
        panic!("expected the tool error to be delivered to the agent");
    };
    let [UserContent::ToolResult(result)] = content.as_slice() else {
        panic!("expected one failed tool result");
    };
    assert!(result.is_error);
    assert_eq!(result.name.as_str(), "read_node");
    assert_eq!(
        result.content,
        vec![ToolResultContent::text("unsupported node contents")]
    );
}

// {test}`requirements.authoring.output_limit`
// {test}`requirements.authoring.provider_backoff`
// {test}`requirements.authoring.submission_count`
// {test}`requirements.sdk.graph.context-tools.tracked_inspection`
#[tokio::test(start_paused = true)]
async fn retries_and_cut_off_tools_preserve_inputs_and_submission_skips_other_reads() {
    let graph = Memory::new("source".into());
    let calls = || {
        vec![
            ("read_node", json!({"path":{"segments":["2"]}})),
            ("submit_result", json!({"result":false})),
        ]
    };
    let agent = ToolAgent::new(Scenario::new(vec![
        Err(throttled()),
        Ok(tool_turn(FinishReason::Length, calls())),
        Ok(tool_turn(
            FinishReason::ToolCalls,
            vec![
                ("read_node", json!({"path":{"segments":["2"]}})),
                ("submit_result", json!({"result":true})),
            ],
        )),
    ]));
    let ctx = ValidationContext::with_agent(&graph, Node(1), &agent);
    assert!(
        ctx.ask::<bool>()
            .context("review".to_owned())
            .contents_of_current_node()
            .await
            .unwrap()
    );
    assert!(matches!(&ctx.inspection().dependencies()[..],
        [Dependency::Node(Node(1), fingerprint)] if fingerprint.value == "source"));
    let requests = agent.provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert_eq!(
        serde_json::to_value(&requests[0]).unwrap(),
        serde_json::to_value(&requests[1]).unwrap()
    );
    assert_eq!(
        &requests[2].chat_history[..requests[0].chat_history.len()],
        requests[0].chat_history
    );
    let continuation = serde_json::to_string(&requests[2].chat_history).unwrap();
    assert_eq!(
        continuation
            .matches("not run: the call was cut off")
            .count(),
        2
    );
    assert!(continuation.contains("32 tokens"));
    assert!(agent.provider.turns.lock().unwrap().is_empty());
}

// {cite}`habeas:util`
#[derive(Default)]
struct MinimalConversation {
    dispatched: usize,
}
impl Conversation for MinimalConversation {
    type Output = bool;
    // {cite}`habeas:util`
    fn tools(&self) -> io::Result<Vec<ToolDefinition>> {
        serde_json::from_value(json!([
            {"name":"lookup","description":"fixture lookup","parameters":{"type":"object"}},
            {"name":"submit_result","description":"submit","parameters":{"type":"object"}}
        ]))
        .map_err(io::Error::other)
    }
    // {cite}`habeas:util`
    fn submit(&mut self, name: &str, arguments: &Value) -> io::Result<Submission<bool>> {
        assert_eq!(name, "submit_result");
        Ok(Submission::Accepted(arguments["result"].as_bool().unwrap()))
    }
    // {cite}`habeas:util`
    async fn dispatch(&mut self, name: &str, arguments: &Value) -> io::Result<String> {
        assert_eq!(name, "lookup");
        assert_eq!(arguments, &json!({"key":"value"}));
        self.dispatched += 1;
        Ok("fixture answer".into())
    }
    // {cite}`habeas:util`
    fn cut_off(&mut self, response: &CompletionResponse, limit: u64) -> Vec<Message> {
        assert_eq!(response.finish_reason(), Some(FinishReason::Length));
        vec![
            response.message().unwrap(),
            Message::user(format!("continue within {limit} tokens")),
        ]
    }
}

// {test}`requirements.sdk.graph.agent-calls.author_procedure`
// {test}`requirements.authoring.provider_backoff`
// {test}`requirements.authoring.output_limit`
// {test}`requirements.authoring.submission_count`
#[tokio::test(start_paused = true)]
async fn generic_conversations_can_use_default_telemetry_through_retry_and_continuation() {
    let provider = Scenario::new(vec![
        Err(throttled()),
        Ok(tool_turn(
            FinishReason::ToolCalls,
            vec![("lookup", json!({"key":"value"}))],
        )),
        Ok(CompletionResponse::new(
            vec![AssistantContent::text("unfinished")],
            Usage::default(),
            Origin::new("test", "test", "test"),
            Value::Null,
        )
        .with_finish_reason(FinishReason::Length)),
        Ok(tool_turn(
            FinishReason::ToolCalls,
            vec![
                ("lookup", json!({"key":"must not be read"})),
                ("submit_result", json!({"result":true})),
            ],
        )),
    ]);
    let mut session = MinimalConversation::default();
    assert!(
        converse(
            &provider,
            &mut session,
            CompletionRequest::new("review"),
            "submit_result"
        )
        .await
        .unwrap()
    );
    assert_eq!(session.dispatched, 1);
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    assert_eq!(
        serde_json::to_value(&requests[0]).unwrap(),
        serde_json::to_value(&requests[1]).unwrap()
    );
    let history = serde_json::to_string(&requests[3].chat_history).unwrap();
    assert!(history.contains("fixture answer"));
    assert!(history.contains("continue within 32 tokens"));
}

// {test}`requirements.sdk.sources.provider_observations`
// {test}`requirements.sdk.graph.context-tools.tracked_inspection`
// {test}`requirements.sdk.sources.provider_results.parse_snapshot`
#[cfg(feature = "default-languages")]
// {test}`requirements.sdk.graph.evidence.node_inputs`
#[tokio::test]
async fn cached_selected_source_reads_record_exact_delivered_contents_and_errors_add_no_input() {
    let graph = Memory::new("different current source".into());
    let factory = SourceTools {
        analysis: SourceAnalysis::new(CachedProvider, habeas_languages::registry()),
        observation: |id: &OutlineId, parsed: &ParsedSource| {
            let index = parsed
                .outline
                .resolve(&id.selector)
                .map_err(|error| error.to_string())?;
            Ok(Some(Dependency::Node(
                Node(7),
                Fingerprint {
                    scheme: "selected-content".into(),
                    value: parsed
                        .outline
                        .node_lines(parsed.source.text().unwrap(), index)
                        .join("\n"),
                },
            )))
        },
    };
    let agent=ToolAgent::new(Script::new(vec![
        Some(vec![("search_structure",json!({"language":"rust","rule":{"rule":{"pattern":"call()"}}}))]),
        Some(vec![("outline_source",json!({"resource":{"scheme":"generated","location":"cached"}}))]),
        Some(vec![("read_source",json!({"resource":{"scheme":"generated","location":"cached"},"selector":"fn[observed]"}))]),
        Some(vec![("read_source",json!({"resource":{"scheme":"generated","location":"cached"},"selector":"fn[missing]"}))]),
        Some(vec![("submit_result",json!({"result":true}))]),
    ])).with_tools(factory);
    let ctx = ValidationContext::with_agent(&graph, Node(1), &agent);
    assert!(ctx.ask::<bool>().await.unwrap());
    assert!(
        matches!(&ctx.inspection().dependencies()[..],[Dependency::Node(Node(7),fingerprint)] if fingerprint.value=="fn observed() { call(); }" && fingerprint.scheme=="selected-content")
    );
}

/// Determine whether tests cover the requirement's behavior and edge cases.
struct BehavioralAdequacy {
    requirement: Node,
    review_policy: String,
}
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct TestReview {
    adequate: bool,
    rationale: String,
}
impl Criterion<Memory> for BehavioralAdequacy {
    type Ok = TestReview;
    fn source_node_id(&self) -> Node {
        self.requirement.clone()
    }
    fn criterion_path(&self) -> CriterionPath {
        CriterionPath::new(vec!["behavior".into()])
    }
    fn revision(&self) -> String {
        "test-review-v1".into()
    }
    async fn execute(
        self,
        cx: &mut ValidationContext<'_, Memory>,
    ) -> Result<Self::Ok, ValidationError<io::Error>> {
        cx.ask()
            .context(&self.review_policy)
            .context("Check whether these tests cover the requirement's behavior and edge cases.")
            .contents_of_current_node()
            .contents_of_successors(Label)
            .await
    }
}

// {test}`requirements.sdk.graph.execution.behavioral_adequacy`
// {test}`requirements.sdk.graph.agent-harness.setup`
// {test}`requirements.sdk.graph.agent-harness.tool_agent`
// {test}`requirements.sdk.domain`
// {test}`requirements.sdk.graph.agent-calls.judge.rendering`
#[tokio::test]
async fn behavioral_adequacy_uses_domain_contents_and_returns_a_typed_fingerprinted_answer() {
    let domain = Memory::new("unselected".into());
    domain.node_contents.lock().unwrap().extend([
        (1, "Tokens expire at their expiration boundary.".into()),
        (2, "Tests check earlier, equal, and later times.".into()),
    ]);
    let expected = TestReview {
        adequate: true,
        rationale: "All boundary cases are covered.".into(),
    };
    let provider = Script::new(vec![Some(vec![(
        "submit_result",
        json!({"result":expected}),
    )])]);
    let agent = Arc::new(ToolAgent::new(provider));
    let validator = Validator::new(domain.clone()).with_agent(agent.clone());
    let inputs = validator
        .verify(BehavioralAdequacy {
            requirement: Node(1),
            review_policy: "Review test adequacy.".into(),
        })
        .await
        .unwrap();
    assert_eq!(inputs.output(), &expected);
    assert_eq!(inputs.criterion_path().segments(), ["behavior"]);
    assert_eq!(*domain.reads.lock().unwrap(), [1, 2]);
    assert_eq!(inputs.dependencies().len(), 3);
    for (id, value) in [
        (1, "Tokens expire at their expiration boundary."),
        (2, "Tests check earlier, equal, and later times."),
    ] {
        assert!(inputs.dependencies().iter().any(|dep| matches!(dep,
            Dependency::Node(Node(n), fp) if *n == id && fp.value == value)));
    }
    assert!(inputs.dependencies().iter().any(|dep| matches!(dep,
        Dependency::OutgoingEdges(Node(1), Label, fp) if *fp == edge_fingerprint(&domain, &[Node(2)]))));
    assert!(domain.records.lock().unwrap().is_empty());
    assert!(validator.check_inputs(&inputs).unwrap());
    domain
        .node_contents
        .lock()
        .unwrap()
        .insert(2, "Only earlier times are tested.".into());
    assert!(!validator.check_inputs(&inputs).unwrap());
    let requests = agent.provider.requests.lock().unwrap();
    assert_eq!(
        requests[0].chat_history[0],
        Message::system(
            "Review test adequacy.\n\nCheck whether these tests cover the requirement's behavior and edge cases."
        )
    );
    let message = serde_json::to_string(&requests[0].chat_history[1]).unwrap();
    assert!(message.find("Tokens expire").unwrap() < message.find("Tests check").unwrap());
    let tool = requests[0]
        .tools
        .iter()
        .find(|t| t.name == "submit_result")
        .unwrap();
    assert_eq!(
        tool.parameters["properties"]["result"]["properties"]["adequate"]["type"],
        "boolean"
    );
}

struct Instructions {
    text: String,
}
impl Context for Instructions {
    fn contents(&self) -> std::borrow::Cow<'_, String> {
        std::borrow::Cow::Borrowed(&self.text)
    }
}

// {test}`requirements.sdk.graph.agent-calls.test_review`
#[tokio::test]
async fn test_review_example_combines_instructions_before_requirement_and_test_contents() {
    let domain = Memory::new("source".into());
    domain
        .node_contents
        .lock()
        .unwrap()
        .extend([(1, "Requirement".into()), (2, "Tests".into())]);
    let expected = TestReview {
        adequate: false,
        rationale: "The boundary test is missing.".into(),
    };
    let agent = ToolAgent::new(Script::new(vec![
        Some(vec![(
            "submit_result",
            json!({"result":expected})
        )]);
        4
    ]));
    let cx = ValidationContext::with_agent(&domain, Node(1), &agent);
    let review_policy = Instructions {
        text: "Review policy".into(),
    };
    let project_conventions = Instructions {
        text: "Project conventions".into(),
    };
    let question = "Do tests cover all behavior and edge cases?";
    let answer: TestReview = cx
        .ask()
        .context((&review_policy, &project_conventions))
        .context(question)
        .contents_of_current_node()
        .contents_of_successors(Label)
        .await
        .unwrap();
    let repeated = cx
        .ask::<TestReview>()
        .context(&review_policy)
        .context(&project_conventions)
        .context(question)
        .contents_of_current_node()
        .contents_of_successors(Label)
        .await
        .unwrap();
    let contexts = vec![&review_policy, &project_conventions];
    let vector = cx
        .ask::<TestReview>()
        .context(contexts.clone())
        .context(question)
        .contents_of_current_node()
        .contents_of_successors(Label)
        .await
        .unwrap();
    let slice = cx
        .ask::<TestReview>()
        .context(contexts.as_slice())
        .context(question)
        .contents_of_current_node()
        .contents_of_successors(Label)
        .await
        .unwrap();
    assert_eq!(vector, expected);
    assert_eq!(slice, expected);
    assert_eq!(answer, expected);
    assert_eq!(repeated, expected);
    assert_eq!(cx.inspection().dependencies().len(), 3);
    let requests = agent.provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    assert!(
        requests
            .iter()
            .all(|r| r.chat_history == requests[0].chat_history)
    );
    assert_eq!(
        requests[0].chat_history[0],
        Message::system(
            "Review policy\n\nProject conventions\n\nDo tests cover all behavior and edge cases?"
        )
    );
    let text = serde_json::to_string(&requests[0].chat_history[1]).unwrap();
    assert!(text.find("Requirement").unwrap() < text.find("Tests").unwrap());
}

struct CounterTools(Arc<std::sync::atomic::AtomicUsize>);
struct CounterSession {
    count: usize,
}
impl ToolFactory<Memory> for CounterTools {
    fn session<'a>(&'a self) -> Box<dyn ToolSession<Memory> + 'a>
    where
        Node: 'a,
        Label: 'a,
    {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::new(CounterSession { count: 0 })
    }
}
impl ToolSession<Memory> for CounterSession {
    fn definitions(&self) -> io::Result<Vec<ToolDefinition>> {
        serde_json::from_value(json!([{"name":"increment", "description":"Increment this call's counter", "parameters":{"type":"object"}}])).map_err(io::Error::other)
    }
    fn dispatch<'a>(
        &'a mut self,
        name: &'a str,
        _: &'a Value,
    ) -> futures::future::BoxFuture<'a, io::Result<ToolDelivery<Node, Label>>> {
        Box::pin(async move {
            assert_eq!(name, "increment");
            self.count += 1;
            Ok(ToolDelivery {
                text: format!("counter={}", self.count),
                dependencies: vec![],
            })
        })
    }
}

// {test}`requirements.sdk.graph.agent-harness.tool_agent`
#[tokio::test]
async fn application_tool_factories_create_fresh_state_for_each_agent_call() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let previous = Arc::new(AtomicUsize::new(0));
    let created = Arc::new(AtomicUsize::new(0));
    let graph = Memory::new("source".into());
    let agent = ToolAgent::new(Script::new(vec![
        Some(vec![("increment", json!({})), ("increment", json!({}))]),
        Some(vec![("submit_result", json!({"result":true}))]),
        Some(vec![("increment", json!({})), ("increment", json!({}))]),
        Some(vec![("submit_result", json!({"result":true}))]),
    ]))
    .with_tools(CounterTools(previous.clone()))
    .with_tools(CounterTools(created.clone()));
    let ctx = ValidationContext::with_agent(&graph, Node(1), &agent);
    for _ in 0..2 {
        assert!(ctx.ask::<bool>().await.unwrap());
    }
    assert_eq!(created.load(Ordering::SeqCst), 2);
    assert_eq!(previous.load(Ordering::SeqCst), 0);
    let requests = agent.provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    for index in [1, 3] {
        let text = serde_json::to_string(requests[index].chat_history.last().unwrap()).unwrap();
        assert!(text.contains("counter=1"), "{text}");
        assert!(text.contains("counter=2"), "{text}");
        assert!(!text.contains("counter=3"), "{text}");
        assert!(
            requests[index - 1]
                .tools
                .iter()
                .any(|tool| tool.name == "increment")
        );
    }
    assert!(ctx.inspection().dependencies().is_empty());
}

use crate::agent_runtime::*;
use crate::domain::Domain;
use crate::domain::NodeContents;
use crate::domain::NodeId;
use crate::graph as validation;
use crate::graph::*;
use crate::request::Context;
use crate::request::{ValidationContext, VetResult};
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

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema, Hash)]
struct Node(u8);
// {cite}`habeas:util`
#[derive(Clone, PartialEq, Eq, Deserialize, JsonSchema)]
struct Label;
// {cite}`habeas:util`
struct Content(String, String);
impl NodeContents for Content {
    fn fingerprint(&self) -> Fingerprint {
        Fingerprint {
            scheme: "content".into(),
            value: self.0.clone(),
        }
    }
    fn format(&self) -> String {
        self.1.clone()
    }
}
// {cite}`habeas:util`
#[derive(Serialize, Deserialize)]
struct Metadata;
#[derive(Clone)]
struct Memory {
    cache: crate::domain::ValidationCache<Self>,
    contents: Arc<Mutex<String>>,
    node_contents: Arc<Mutex<std::collections::BTreeMap<u8, String>>>,
    reads: Arc<Mutex<Vec<u8>>>,
    formatter: fn(&Node, &Content) -> io::Result<String>,
    records: Arc<Mutex<Vec<(Node, CriterionPath, Record<Node, Label>)>>>,
}
impl Memory {
    fn new(contents: String) -> Self {
        let cache = crate::domain::ValidationCache::new();
        // The scripted agent addresses these fixture nodes by their assigned integers.
        for n in 0..=7 {
            cache.intern_node(Node(n));
        }
        Self {
            cache,
            contents: Arc::new(Mutex::new(contents)),
            node_contents: Arc::default(),
            reads: Arc::default(),
            records: Arc::default(),
            formatter: |_, content| Ok(content.0.clone()),
        }
    }
}
impl Domain for Memory {
    fn cache(&self) -> &crate::domain::ValidationCache<Self> {
        &self.cache
    }
    async fn load_evidence(
        &self,
        node: &NodeId<Self>,
        criterion: &CriterionPath,
    ) -> io::Result<Option<Record<Node, Label>>> {
        let node = &*self.cache().node_data(*node).unwrap();
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
        node: &NodeId<Self>,
        criterion: &CriterionPath,
        record: &Record<Node, Label>,
    ) -> io::Result<()> {
        let node = &*self.cache().node_data(*node).unwrap();
        let mut records = self.records.lock().unwrap();
        records.retain(|(n, c, _)| n != node || c != criterion);
        records.push((node.clone(), criterion.clone(), record.clone()));
        Ok(())
    }

    type NodeData = Node;
    type EdgeLabel = Label;

    type Error = io::Error;

    fn node_path(&self, node: &NodeId<Self>) -> NodePath {
        let node = &*self.cache().node_data(*node).unwrap();
        NodePath::new(vec![node.0.to_string()])
    }
    fn outgoing_edges(&self, node: &NodeId<Self>, _: Label) -> io::Result<Vec<NodeId<Self>>> {
        let node = &*self.cache().node_data(*node).unwrap();
        Ok((if node.0 == 1 { vec![Node(2)] } else { vec![] })
            .into_iter()
            .map(|data| self.cache().intern_node(data))
            .collect())
    }

    type NodeContent = Content;

    fn node_content(&self, node: &NodeId<Self>) -> io::Result<Content> {
        let node = &*self.cache().node_data(*node).unwrap();
        self.reads.lock().unwrap().push(node.0);
        let mut content = Content(
            self.node_contents
                .lock()
                .unwrap()
                .get(&node.0)
                .cloned()
                .unwrap_or_else(|| self.contents.lock().unwrap().clone()),
            String::new(),
        );
        content.1 = (self.formatter)(node, &content)?;
        Ok(content)
    }
}

// {cite}`habeas:util`

struct Review(NodeId<Memory>);
impl Criterion<Memory> for Review {
    type Ok = Assessment<Metadata>;
    fn source_node_id(&self) -> NodeId<Memory> {
        self.0.clone()
    }
    fn criterion_path(&self) -> CriterionPath {
        CriterionPath::new(vec![])
    }
    fn version(&self) -> CriterionVersion {
        CriterionVersion::new("v1")
    }
    async fn execute(
        self,
        ctx: &mut ValidationContext<'_, Memory>,
    ) -> Result<Assessment<Metadata>, ValidationError<io::Error>> {
        let text = ctx.read(ctx.node_id()).map_err(ValidationError::Graph)?;
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
        self.requests.lock().unwrap().push(request.clone());
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
            .map(|(index, (name, mut args))| {
                if name == "get_next_page" {
                    if args.get("use_next").is_some() {
                        args = json!({"uuid":returned_node_page(&request).to_string()});
                    }
                }
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

// {cite}`habeas:util`
fn returned_node_page(request: &CompletionRequest) -> uuid::Uuid {
    for message in request.chat_history.iter().rev() {
        if let Message::User { content } = message.as_ref() {
            for item in content {
                if let UserContent::ToolResult(result) = item {
                    if result.name.as_str() == "read_node" {
                        let ToolResultContent::Text(text) = &result.content[0] else {
                            panic!("expected node-read result text");
                        };
                        let value: Value = serde_json::from_str(&text.text).unwrap();
                        return uuid::Uuid::parse_str(value["next"].as_str().unwrap()).unwrap();
                    }
                }
            }
        }
    }
    panic!("expected a prepared node page in the conversation")
}

// {test}`requirements.sdk.graph.validation-context.pages.mcp.description`
// {test}`requirements.sdk.graph.agent-calls.judge.submission`
// {test}`requirements.sdk.graph.agent-harness.submission`
// {test}`requirements.sdk.graph.agent-harness.submission.mcp`
// {test}`requirements.sdk.graph.agent-harness.fixed_tools`
// {test}`requirements.sdk.graph.agent-calls.judge.caching`
// {test}`requirements.sdk.graph.domain_types`
// {test}`requirements.sdk.graph.validation-context.pages.mcp`
// {test}`requirements.sdk.graph.agent-calls.judge.rendering`
#[tokio::test]
async fn default_tool_accepts_scalar_results_and_keeps_the_exact_prefix() {
    let mut graph = Memory::new("source".into());
    graph.formatter = |_, contents| Ok(format!("formatted input: {}", contents.0));
    let agent = ToolAgent::new(Script::new(vec![Some(vec![(
        "submit_result",
        json!({"result":true}),
    )])]));
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &agent);
    assert!(
        ctx.ask::<bool>()
            .context("Question é".to_owned())
            .contents_of_current_node()
            .await
            .unwrap()
    );
    assert!(
        matches!(&ctx.dependencies()[0], Dependency::Node(observed, fingerprint) if *observed == graph.cache().intern_node(Node(1)) && fingerprint.value == "source")
    );
    let requests = agent.provider.requests.lock().unwrap();
    assert_eq!(
        requests[0]
            .tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        [
            "read_node",
            "outgoing_edges",
            "get_next_page",
            "submit_result"
        ]
    );
    let next_page = requests[0]
        .tools
        .iter()
        .find(|tool| tool.name == "get_next_page")
        .unwrap();
    assert_eq!(
        next_page.parameters,
        json!({"type":"object","required":["uuid"],"properties":{"uuid":{"type":"string","format":"uuid"}}})
    );
    assert_eq!(*requests[0].chat_history[0], Message::system("Question é"));
    assert!(
        serde_json::to_string(&requests[0].chat_history[1])
            .unwrap()
            .contains("formatted input: source")
    );
    assert_eq!(*graph.reads.lock().unwrap(), [1]);
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
        let alpha = *request.chat_history[0] == Message::system("alpha");
        let (prefix, contents, other, node) = if alpha {
            ("alpha", "ALPHA-CONTENTS", "BETA-CONTENTS", 1)
        } else {
            assert_eq!(*request.chat_history[0], Message::system("beta"));
            ("beta", "BETA-CONTENTS", "ALPHA-CONTENTS", 2)
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
            // though both calls use the same tool call ID.
            let page = serde_json::to_string(request.chat_history.last().unwrap()).unwrap();
            assert!(page.contains(contents));
            assert!(!page.contains(other), "{prefix}: {page}");
        }
        self.barrier.wait().await;
        let (name, arguments) = match turn {
            0 => ("read_node", json!({"node":node})),
            1 => (
                "get_next_page",
                json!({"uuid":returned_node_page(&request).to_string()}),
            ),
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
// {test}`requirements.sdk.graph.validation-context.tracked_inspection`
// {test}`requirements.sdk.graph.agent-harness.tool_agent`
// {test}`requirements.sdk.graph.agent-harness.tool_agent.isolation`
// {test}`requirements.sdk.graph.validation-context.read_node.behavior`
// {test}`requirements.sdk.graph.validation-context.read_node.inputs`
// {test}`requirements.sdk.graph.validation-context.ValidationContext`
#[tokio::test]
// {test}`requirements.sdk.graph.validation-context.node_reference`
// {test}`requirements.sdk.graph.validation-context.shared_context`
async fn concurrent_tool_agents_isolate_histories_pages_and_submissions_and_share_reads() {
    use std::future::IntoFuture;
    let mut graph = Memory::new("source".into());
    graph.formatter = distinct_pages;
    let agent = ToolAgent::new(ParallelConversations {
        barrier: tokio::sync::Barrier::new(2),
        calls: Mutex::default(),
    });
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &agent);
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
    let dependencies = crate::graph::persist_dependencies(&graph, &ctx.dependencies());
    assert_eq!(dependencies.len(), 3);
    for expected in [
        Dependency::OutgoingEdges(
            Node(1),
            Label,
            edge_fingerprint(&graph, &[graph.cache().intern_node(Node(2))]),
        ),
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
        ctx.finish(
            CriterionVersion::new("v1"),
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
    subject: NodeId<Memory>,
    name: String,
    successors: bool,
}
impl Criterion<Memory> for Question {
    type Ok = String;
    fn source_node_id(&self) -> NodeId<Memory> {
        self.subject
    }
    fn criterion_path(&self) -> CriterionPath {
        CriterionPath::new(vec!["question".into()])
    }
    fn version(&self) -> CriterionVersion {
        CriterionVersion::new("v1")
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

// {test}`requirements.sdk.graph.running-criteria.verify.concurrent`
// {test}`requirements.sdk.graph.running-criteria.validate_node`
// {test}`requirements.sdk.graph.agent-calls.judge.rendering`
// {test}`requirements.sdk.graph.agent-calls.author_procedure`
// {test}`requirements.sdk.graph.agent-harness.tool_agent`
// {test}`requirements.sdk.graph.agent-harness.with_agent`
// {test}`requirements.sdk.graph.running-criteria.Validator`
// {test}`requirements.sdk.validation-cache.usage`
#[tokio::test]
async fn agent_executions_isolate_conversations_and_dependency_traces() {
    let mut graph = Memory::new("source".into());
    graph.formatter = distinct_pages;
    let agent = ToolAgent::new(ParallelConversations {
        barrier: tokio::sync::Barrier::new(2),
        calls: Mutex::default(),
    });
    let agent = Arc::new(agent);
    let question = |name: &str, successors| Question {
        subject: graph.cache().intern_node(Node(1)),
        name: name.into(),
        successors,
    };
    let mut alpha_validator = validation::Validator::new(&graph);
    let mut beta_validator = validation::Validator::new(&graph);
    let cache = graph.cache().clone();
    let initial = cache.usage();
    let (alpha, beta) = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        futures::join!(
            crate::agent_runtime::verify_with_agent(
                &mut alpha_validator,
                agent.as_ref(),
                question("alpha", false)
            ),
            crate::agent_runtime::verify_with_agent(
                &mut beta_validator,
                agent.as_ref(),
                question("beta", true)
            ),
        )
    })
    .await
    .unwrap();
    let (alpha, beta) = (alpha.unwrap(), beta.unwrap());
    // The two prompt labels distinguish executions of the same criterion.
    assert_eq!(alpha.criterion_path(), beta.criterion_path());
    assert_eq!(alpha.source_node_id(), beta.source_node_id());
    assert_eq!(alpha_validator.usage().attempts, 3);
    assert_eq!(beta_validator.usage().attempts, 3);
    assert_eq!(cache.usage().attempts, 6);
    assert!(initial.tools.is_empty());
    let totals = cache.usage().tools;
    assert_eq!(totals.len(), 3);
    for name in ["read_node", "get_next_page", "submit_result"] {
        let a = &alpha_validator.usage().tools[name];
        let b = &beta_validator.usage().tools[name];
        assert_eq!((a.calls, b.calls, totals[name].calls), (1, 1, 2));
        assert_eq!(totals[name].result_bytes, a.result_bytes + b.result_bytes);
        if name != "submit_result" {
            assert!(a.result_bytes > 0 && b.result_bytes > 0);
        }
    }
    assert_eq!(initial.attempts, 0);
    assert_eq!(alpha.output(), "alpha");
    assert_eq!(beta.output(), "beta");
    validation::Validator::new(&graph)
        .store_evidence(&alpha)
        .await
        .unwrap();
    {
        let calls = agent.provider.calls.lock().unwrap();
        assert_eq!(calls["alpha"], 3);
        assert_eq!(calls["beta"], 3);
    }
    let alpha_record = graph
        .load_evidence(
            &graph.cache().intern_node(Node(1)),
            &question("alpha", false).criterion_path(),
        )
        .await
        .unwrap()
        .unwrap();
    validation::Validator::new(&graph)
        .store_evidence(&beta)
        .await
        .unwrap();
    let beta_record = graph
        .load_evidence(
            &graph.cache().intern_node(Node(1)),
            &question("beta", true).criterion_path(),
        )
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
                Dependency::OutgoingEdges(
                    Node(1),
                    Label,
                    edge_fingerprint(&graph, &[graph.cache().intern_node(Node(2))])
                ),
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
        validation::Validator::new(&graph)
            .load_evidence(&question("alpha", false))
            .await
            .unwrap()
            .unwrap()
            .into_output(),
        "beta"
    );
    assert_eq!(agent.provider.calls.lock().unwrap()["alpha"], 3);
}

// {test}`requirements.sdk.graph.validation-context.tracked_inspection`
// {test}`requirements.sdk.graph.snapshot`
// {test}`requirements.sdk.graph.running-criteria.validate_node`
// {test}`requirements.sdk.graph.evidence.check_node`
// {test}`requirements.sdk.graph.evidence.store_evidence`
// {test}`requirements.sdk.graph.evidence.inputs`
// {test}`requirements.sdk.graph.validation-context.outgoing_edges.behavior`
// {test}`requirements.sdk.graph.validation-context.outgoing_edges.inputs`
// {test}`requirements.sdk.graph.agent-harness.fixed_tools`
// {test}`requirements.sdk.graph.validation-context.outgoing_edges.mcp`
// {test}`requirements.sdk.graph.validation-context.ValidationContext`
#[tokio::test]
// {test}`requirements.sdk.graph.validation-context.node_reference`
async fn agent_edge_tools_record_selections_without_validating_targets() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[derive(Clone)]
    struct GraphWithChangingEdges {
        cache: crate::domain::ValidationCache<Self>,
        lookups: Arc<AtomicUsize>,
        records: Arc<Mutex<Vec<(u8, Record<u8, u8>)>>>,
    }
    impl Domain for GraphWithChangingEdges {
        fn cache(&self) -> &crate::domain::ValidationCache<Self> {
            &self.cache
        }
        type NodeData = u8;
        type EdgeLabel = u8;

        type Error = io::Error;

        fn node_path(&self, node: &NodeId<Self>) -> NodePath {
            let node = &*self.cache().node_data(*node).unwrap();
            NodePath::new(vec![node.to_string()])
        }
        fn outgoing_edges(&self, node: &NodeId<Self>, label: u8) -> io::Result<Vec<NodeId<Self>>> {
            let node = &*self.cache().node_data(*node).unwrap();
            assert!(matches!(label, 7 | 8));
            if *node == 1 && self.lookups.fetch_add(1, Ordering::SeqCst) != 0 {
                return Err(io::Error::other("must not relist inspected edges"));
            }
            Ok((match (*node, label) {
                (1, 7) => vec![3, 2, 3],
                (1, 8) => vec![3],
                _ => vec![],
            })
            .into_iter()
            .map(|data| self.cache().intern_node(data))
            .collect())
        }

        type NodeContent = Content;

        // :::{cite} habeas:coverage
        // This edge-only validation regression deliberately supplies no node
        // contents. Its records assert selection dependencies only. Domain
        // requires this callback, but reaching it would introduce an unwanted
        // content read and fail this guard.
        // :::
        fn node_content(&self, _: &NodeId<Self>) -> io::Result<Content> {
            panic!("edge inspection must not read node contents")
        }

        async fn load_evidence(
            &self,
            node: &NodeId<Self>,
            _: &CriterionPath,
        ) -> io::Result<Option<Record<u8, u8>>> {
            let node = &*self.cache().node_data(*node).unwrap();
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
            node: &NodeId<Self>,
            _: &CriterionPath,
            record: &Record<u8, u8>,
        ) -> io::Result<()> {
            let node = &*self.cache().node_data(*node).unwrap();
            self.records.lock().unwrap().push((*node, record.clone()));
            Ok(())
        }
    }

    struct EdgeReview(NodeId<GraphWithChangingEdges>);
    impl Criterion<GraphWithChangingEdges> for EdgeReview {
        type Ok = Assessment<()>;
        fn source_node_id(&self) -> NodeId<GraphWithChangingEdges> {
            self.0
        }
        fn criterion_path(&self) -> CriterionPath {
            CriterionPath::new(vec![])
        }
        fn version(&self) -> CriterionVersion {
            CriterionVersion::new("v1")
        }
        async fn execute(
            self,
            ctx: &mut ValidationContext<'_, GraphWithChangingEdges>,
        ) -> Result<Assessment<()>, ValidationError<io::Error>> {
            let result = if *ctx.node_id() == ctx.graph().cache().intern_node(1) {
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
        cache: Default::default(),
        lookups: Arc::new(AtomicUsize::new(0)),
        records: Arc::default(),
    };
    for id in 0..=3 {
        graph.cache().intern_node(id);
    }
    assert!(graph.project_sources().is_none());
    let agent = ToolAgent::new(Script::new(vec![
        Some(vec![("outgoing_edges", json!({"node":1,"label":7}))]),
        Some(vec![("submit_result", json!({"result":true}))]),
    ]));
    let agent = Arc::new(agent);
    assert!(
        validation::Validator::new(&graph)
            .load_evidence(&EdgeReview(graph.cache().intern_node(1)))
            .await
            .unwrap()
            .is_none()
    );
    let result = crate::agent_runtime::verify_with_agent(
        &mut validation::Validator::new(&graph),
        agent.as_ref(),
        EdgeReview(graph.cache().intern_node(1)),
    )
    .await
    .unwrap();
    assert!(result.output().result);
    validation::Validator::new(&graph)
        .store_evidence(&result)
        .await
        .unwrap();
    let saved = validation::Validator::new(&graph)
        .load_evidence(&EdgeReview(graph.cache().intern_node(1)))
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
            7,
            edge_fingerprint(
                &graph,
                &[graph.cache().intern_node(2), graph.cache().intern_node(3)]
            )
        )]
    );
    let requests = agent.provider.requests.lock().unwrap();
    let Message::User { content } = requests[1].chat_history.last().unwrap().as_ref() else {
        panic!("expected edge tool delivery");
    };
    let [UserContent::ToolResult(result)] = content.as_slice() else {
        panic!("expected one edge result");
    };
    assert!(!result.is_error);
    assert_eq!(
        result.content,
        vec![ToolResultContent::text(
            json!({"targets":[3,2,3]}).to_string()
        )]
    );
    drop(requests);
    drop(records);
    // A malformed node ID fails before a graph query can add an observation.
    let graph = GraphWithChangingEdges {
        cache: Default::default(),
        lookups: Arc::default(),
        records: Arc::default(),
    };
    for id in 0..=3 {
        graph.cache().intern_node(id);
    }
    let agent = ToolAgent::new(Script::new(vec![
        Some(vec![(
            "outgoing_edges",
            json!({"node":"unknown","label":7}),
        )]),
        Some(vec![("submit_result", json!({"result":true}))]),
    ]));
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(1), &agent);
    assert!(ctx.ask::<bool>().await.unwrap());
    assert!(ctx.dependencies().is_empty());
    assert_eq!(graph.lookups.load(Ordering::SeqCst), 0);
    let requests = agent.provider.requests.lock().unwrap();
    let Message::User { content } = requests[1].chat_history.last().unwrap().as_ref() else {
        panic!("expected rejected path result");
    };
    let [UserContent::ToolResult(result)] = content.as_slice() else {
        panic!("expected one rejected edge query");
    };
    assert!(result.is_error);
    assert_eq!(
        result.content,
        vec![ToolResultContent::text(
            "invalid node ID: invalid type: string \"unknown\", expected usize"
        )]
    );
    drop(requests);
    // A prepared selection is not an input until delivery succeeds.
    let failed = ToolAgent::new(Script::new(vec![
        Some(vec![("outgoing_edges", json!({"node":1,"label":7}))]),
        None,
    ]));
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(1), &failed);
    assert!(ctx.ask::<bool>().await.is_err());
    assert_eq!(graph.lookups.load(Ordering::SeqCst), 1);
    assert!(ctx.dependencies().is_empty());
    let graph = GraphWithChangingEdges {
        cache: Default::default(),
        lookups: Arc::default(),
        records: Arc::default(),
    };
    for id in 0..=3 {
        graph.cache().intern_node(id);
    }
    let agent = ToolAgent::new(Script::new(vec![
        Some(vec![("outgoing_edges", json!({"node":1,"label":8}))]),
        Some(vec![("outgoing_edges", json!({"node":3,"label":7}))]),
        Some(vec![("submit_result", json!({"result":true}))]),
    ]));
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(1), &agent);
    assert!(ctx.ask::<bool>().await.unwrap());
    assert_eq!(
        ctx.dependencies(),
        vec![
            Dependency::OutgoingEdges(
                graph.cache().intern_node(1),
                8,
                edge_fingerprint(&graph, &[graph.cache().intern_node(3)])
            ),
            Dependency::OutgoingEdges(
                graph.cache().intern_node(3),
                7,
                edge_fingerprint(&graph, &[])
            )
        ]
    );
    let requests = agent.provider.requests.lock().unwrap();
    let Message::User { content } = requests[1].chat_history.last().unwrap().as_ref() else {
        panic!("expected second label's edge delivery");
    };
    let [UserContent::ToolResult(result)] = content.as_slice() else {
        panic!("expected one edge result");
    };
    assert_eq!(
        result.content,
        vec![ToolResultContent::text(json!({"targets":[3]}).to_string())]
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

// {test}`requirements.sdk.graph.agent-harness.submission.acceptance`
// {test}`requirements.sdk.graph.agent-calls.judge.submission`
// {test}`requirements.sdk.graph.agent-harness.submission`
// {test}`requirements.sdk.graph.agent-harness.submission.mcp`
#[tokio::test]
async fn typed_submission_references_resolve() {
    let graph = Memory::new("source".into());
    let agent = ToolAgent::new(Script::new(vec![Some(vec![(
        "submit_result",
        json!({"result":{"decision":"Yes"}}),
    )])]));
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &agent);
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
    assert!(matches!(*requests[0].chat_history[0], Message::User { .. }));
    assert!(schema["$defs"]["Decision"].is_object());
    assert_eq!(schema["required"], json!(["result"]));
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["properties"].as_object().unwrap().len(), 1);
    assert_eq!(
        schema["properties"]["result"]["properties"]["decision"]["$ref"],
        "#/$defs/Decision"
    );
}

// {test}`requirements.sdk.graph.agent-calls.judge.submission.decoding`
// {test}`requirements.sdk.graph.agent-harness.submission.invalid`
// {test}`requirements.sdk.graph.agent-harness.submission_count`
#[tokio::test]
async fn malformed_submissions_receive_feedback_and_can_be_corrected() {
    let graph = Memory::new("source".into());
    let envelope = "exactly one field named `result`";
    let schema_guidance = "submit_result tool schema in the `result` field";
    for (arguments, diagnostic, guidance) in [
        (json!({}), envelope, envelope),
        (json!({"other":true}), envelope, envelope),
        (json!({"result":{},"extra":true}), envelope, envelope),
        (
            json!({"result":{}}),
            "missing field `decision`",
            schema_guidance,
        ),
        (
            json!({"result":r#"{"decision":"Yes"}"#}),
            "invalid type: string",
            schema_guidance,
        ),
        (
            json!({"result":r#"{"decision":"Yes"oops"}"#}),
            "invalid type: string",
            schema_guidance,
        ),
        (
            json!({"result":{"decision":"Maybe"}}),
            "unknown variant `Maybe`",
            schema_guidance,
        ),
        (json!({"result":42}), "invalid type", schema_guidance),
    ] {
        let agent = ToolAgent::new(Script::new(vec![
            Some(vec![("submit_result", arguments)]),
            Some(vec![("submit_result", json!({"result":{"decision":"No"}}))]),
        ]));
        let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &agent);
        let mut vetted = 0;
        let answer = ctx
            .ask::<Answer>()
            .vet_answer(|_| {
                vetted += 1;
                Ok(VetResult::Accept)
            })
            .await
            .unwrap();
        assert!(matches!(answer.decision, Decision::No));
        assert_eq!(vetted, 1);
        let requests = agent.provider.requests.lock().unwrap();
        let Some(Message::User { content }) = requests[1].chat_history.last() else {
            panic!("expected tool feedback")
        };
        let UserContent::ToolResult(result) = &content[0] else {
            panic!("expected tool feedback")
        };
        assert!(result.is_error);
        let feedback = serde_json::to_string(result).unwrap();
        assert!(feedback.contains(diagnostic), "{feedback}");
        assert!(feedback.contains(guidance), "{feedback}");
    }
}

// {test}`requirements.sdk.graph.agent-calls.judge.submission.decoding`
#[tokio::test]
async fn string_answers_preserve_json_text() {
    let graph = Memory::new("source".into());
    let text = r#"{"decision":"Yes"}"#;
    let turn = Some(vec![("submit_result", json!({"result":text}))]);
    let agent = ToolAgent::new(Script::new(vec![turn]));
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &agent);
    assert_eq!(ctx.ask::<String>().await.unwrap(), text);
}

// {test}`requirements.sdk.graph.agent-calls.vet_answer`
// {test}`requirements.sdk.graph.agent-calls.vet_answer.ok`
// {test}`requirements.sdk.graph.agent-calls.vet_answer.corrections`
// {test}`requirements.sdk.graph.agent-calls.VetResult`
// {test}`requirements.sdk.graph.agent-calls.judge.submission`
#[tokio::test]
async fn vetting_collects_corrections_and_rechecks_every_callback() {
    use crate::request::VetResult::*;
    let graph = Memory::new("source".into());
    let agent = ToolAgent::new(Script::new(vec![
        Some(vec![("submit_result", json!({"result":false}))]),
        Some(vec![("submit_result", json!({"result":true}))]),
    ]));
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &agent);
    let mut seen = Vec::new();
    let answer = ctx
        .ask::<bool>()
        .vet_answer(|answer| {
            seen.push(*answer);
            Ok(Accept)
        })
        .contents_of_current_node()
        .vet_answer(|answer| {
            Ok(if *answer {
                Accept
            } else {
                CorrectAndTryAgain("first correction".into())
            })
        })
        .vet_answer({
            let mut first = true;
            move |_| {
                Ok(if std::mem::take(&mut first) {
                    CorrectAndTryAgain("second correction".into())
                } else {
                    Accept
                })
            }
        })
        .await
        .unwrap();
    assert!(answer);
    assert_eq!(seen, [false, true]);
    let requests = agent.provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1]
            .chat_history
            .starts_with(&requests[0].chat_history)
    );
    assert!(matches!(
        requests[1].chat_history[requests[0].chat_history.len()].as_ref(),
        Message::Assistant { .. }
    ));
    let Some(Message::User { content }) = requests[1].chat_history.last().map(AsRef::as_ref) else {
        panic!("expected a tool response with the corrections");
    };
    assert_eq!(content.len(), 1);
    let UserContent::ToolResult(result) = &content[0] else {
        panic!("expected submit_result feedback")
    };
    let feedback = serde_json::to_string(result).unwrap();
    assert!(feedback.contains("resubmit"));
    assert!(
        feedback.contains(r"first correction\nsecond correction"),
        "{feedback}"
    );
}

// {test}`requirements.sdk.graph.agent-calls.vet_answer`
// {test}`requirements.sdk.graph.agent-calls.judge.submission`
#[tokio::test]
async fn vetting_preserves_domain_errors_and_skips_later_callbacks() {
    let graph = Memory::new("source".into());
    let agent = ToolAgent::new(Script::new(vec![Some(vec![(
        "submit_result",
        json!({"result":false}),
    )])]));
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &agent);
    let result = ctx
        .ask::<bool>()
        .vet_answer(|_| Ok(VetResult::CorrectAndTryAgain("amend".into())))
        .vet_answer(|_| Err(io::Error::from_raw_os_error(42)))
        .vet_answer(|_| panic!("called after domain error"))
        .await;
    assert!(
        matches!(result, Err(ValidationError::Graph(error)) if error.raw_os_error() == Some(42))
    );
    assert_eq!(agent.provider.requests.lock().unwrap().len(), 1);
}

// {test}`requirements.sdk.graph.validation-context.read_node.behavior`
// {test}`requirements.sdk.graph.validation-context.node_reference`
#[tokio::test]
async fn read_node_accepts_a_domain_id_first_supplied_by_the_agent() {
    let graph = Memory::new("source".into());
    let agent = ToolAgent::new(Script::new(vec![
        Some(vec![("read_node", json!({"node": 999}))]),
        Some(vec![("read_node", json!({"node":2}))]),
        Some(vec![("submit_result", json!({"result":true}))]),
    ]));
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &agent);
    assert!(ctx.ask::<bool>().await.unwrap());
    let requests = agent.provider.requests.lock().unwrap();
    let Message::User { content } = requests[1].chat_history.last().unwrap().as_ref() else {
        panic!("expected a read_node response");
    };
    let [UserContent::ToolResult(result)] = content.as_slice() else {
        panic!("expected one rejected read");
    };
    assert!(result.is_error);
    assert_eq!(
        result.content,
        [ToolResultContent::text("unassigned node ID")]
    );
    assert_eq!(*graph.reads.lock().unwrap(), [2]);
    assert!(matches!(
        &ctx.dependencies()[..],
        [Dependency::Node(observed, _)] if *observed == graph.cache().intern_node(Node(2))));
}

// {test}`requirements.sdk.graph.validation-context.tracked_inspection`
// {test}`requirements.sdk.graph.agent-calls.author_procedure`
// {test}`requirements.sdk.graph.evidence.node_inputs`
// {test}`requirements.sdk.graph.snapshot`
// {test}`requirements.sdk.graph.validation-context.read_node.behavior`
// {test}`requirements.sdk.graph.validation-context.read_node.inputs`
// {test}`requirements.sdk.graph.validation-context.pages.behavior`
// {test}`requirements.sdk.graph.validation-context.pages.inputs`
// {test}`requirements.sdk.graph.agent-harness.fixed_tools`
#[tokio::test]
// {test}`requirements.sdk.graph.validation-context.node_reference`
// {test}`requirements.sdk.graph.running-criteria.UsageStats`
// {test}`requirements.authoring.delivered_inputs`
async fn graph_tools_record_delivered_snapshots_and_prepared_pages_do_not_record_delivery() {
    let graph = Memory::new("x".repeat(18_000));
    let agent = ToolAgent::new(Script::new(vec![
        Some(vec![("outgoing_edges", json!({"node":1,"label":null}))]),
        Some(vec![("read_node", json!({"node":2}))]),
        Some(vec![("get_next_page", json!({"use_next":true}))]),
        Some(vec![("submit_result", json!({"result":true}))]),
    ]));
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &agent);
    assert!(ctx.ask::<bool>().await.unwrap());
    let usage = graph.cache().usage();
    assert_eq!((usage.prepared_inputs, usage.delivered_inputs), (3, 3));
    let observations = ctx.dependencies();
    assert_eq!(observations.len(), 2);
    assert!(
        matches!(&observations[0],Dependency::OutgoingEdges(observed, Label, fingerprint) if *observed == graph.cache().intern_node(Node(1)) && *fingerprint == edge_fingerprint(&graph, &[graph.cache().intern_node(Node(2))]))
    );
    assert!(
        matches!(&observations[1],Dependency::Node(observed,fingerprint) if *observed == graph.cache().intern_node(Node(2)) && fingerprint.value == "x".repeat(18_000))
    );
    let record = ctx
        .finish(
            CriterionVersion::new("v1"),
            Assessment {
                result: true,
                rationale: "read".into(),
                metadata: Metadata,
            },
        )
        .unwrap();
    assert!(record.dependencies == crate::graph::persist_dependencies(&graph, &observations));
    let failed = ToolAgent::new(Script::new(vec![
        Some(vec![("read_node", json!({"node":1}))]),
        None,
    ]));
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &failed);
    assert!(ctx.ask::<bool>().await.is_err());
    assert!(ctx.dependencies().is_empty());
    let failed = ToolAgent::new(Script::new(vec![
        Some(vec![("read_node", json!({"node":1}))]),
        None,
    ]));
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &failed);
    assert!(ctx.ask::<bool>().contents_of_current_node().await.is_err());
    assert_eq!(ctx.dependencies().len(), 1);
    let usage = graph.cache().usage();
    assert_eq!((usage.prepared_inputs, usage.delivered_inputs), (6, 4));
}

// {test}`requirements.sdk.graph.running-criteria.UsageStats`
#[tokio::test]
async fn inputs_count_each_opening_insertion_and_each_repeated_node_read() {
    let graph = Memory::new("content".into());
    let node = graph.cache().intern_node(Node(1));
    let failed = ToolAgent::new(Script::new(vec![None]));
    let ctx = ValidationContext::with_agent(&graph, node, &failed);
    assert!(
        ctx.ask::<bool>()
            .contents_of_current_node()
            .contents_of_successors(Label)
            .contents_of_current_node()
            .await
            .is_err()
    );
    let stats = graph.cache().usage();
    assert_eq!((stats.prepared_inputs, stats.delivered_inputs), (3, 0));

    let agent = ToolAgent::new(Script::new(vec![
        Some(vec![
            ("read_node", json!({"node":node})),
            ("read_node", json!({"node":node})),
        ]),
        Some(vec![("submit_result", json!({"result":true}))]),
    ]));
    let ctx = ValidationContext::with_agent(&graph, node, &agent);
    assert!(ctx.ask::<bool>().await.unwrap());
    let stats = graph.cache().usage();
    assert_eq!((stats.prepared_inputs, stats.delivered_inputs), (5, 2));
}

// {test}`requirements.sdk.graph.criteria.criterion_trait`
// {test}`requirements.sdk.graph.domain_types`
// {test}`requirements.sdk.domain`
// {test}`requirements.sdk.graph.agent-calls.author_procedure`
// {test}`requirements.sdk.graph.criteria.deterministic_procedures`
// {test}`requirements.sdk.graph.criteria.execute`
// {test}`requirements.sdk.graph.agent-harness.with_agent`
#[tokio::test]
async fn deterministic_validation_uses_required_storage_without_an_agent() {
    let graph = Memory::new("ordinary computation".into());
    assert!(matches!(
        ValidationContext::new(&graph, graph.cache().intern_node(Node(1)))
            .ask::<bool>()
            .await,
        Err(ValidationError::NoAgentConfigured)
    ));
    let mut validator = validation::Validator::new(&graph);
    let result = validator
        .verify(Review(graph.cache().intern_node(Node(1))))
        .await
        .unwrap();
    validator.store_evidence(&result).await.unwrap();
    let saved = graph
        .load_evidence(
            &graph.cache().intern_node(Node(1)),
            &CriterionPath::new(vec![]),
        )
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
// {test}`requirements.sdk.graph.agent-harness.submission`
// {test}`requirements.sdk.graph.agent-harness.submission.reminder`
// {test}`requirements.sdk.graph.agent-harness.tool_agent.provider_failure`
#[tokio::test]
// {test}`requirements.sdk.graph.running-criteria.ValidationError`
async fn assistant_text_and_provider_failure_cannot_become_typed_results() {
    let graph = Memory::new("source".into());
    let agent = ToolAgent::new(TextOnly(Mutex::default()));
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &agent);
    assert!(
        matches!(ctx.ask::<bool>().await,Err(ValidationError::Agent(error)) if error.to_string().contains("exactly once, even after a reminder"))
    );
    let requests = agent.provider.0.lock().unwrap();
    assert_eq!(requests.len(), 2);
    let reminder = serde_json::to_string(requests[1].chat_history.last().unwrap()).unwrap();
    assert!(
        reminder.contains("invoke submit_result exactly once"),
        "{reminder}"
    );
    assert!(
        serde_json::to_string(&requests[0].chat_history)
            .unwrap()
            .contains("Finish by calling submit_result")
    );
    let failing = ToolAgent::new(Script::new(vec![None]));
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &failing);
    assert!(
        matches!(ctx.ask::<bool>().await,Err(ValidationError::Provider(error)) if error.to_string().contains("permanent provider failure"))
    );
    assert!(ctx.dependencies().is_empty());
}

// {test}`requirements.sdk.graph.agent-harness.submission.acceptance`
// {test}`requirements.sdk.graph.agent-calls.vet_answer.ok`
#[tokio::test]
async fn one_submission_accepts_only_its_value_and_skips_other_calls_and_prose() {
    let graph = Memory::new("source".into());
    let mut response = tool_turn(
        FinishReason::ToolCalls,
        vec![
            ("read_node", json!({"node":1})),
            ("submit_result", json!({"result":false})),
        ],
    );
    let mut choice = vec![AssistantContent::text("The answer is true.")];
    choice.extend(response.choice.iter().cloned());
    response.choice = choice;
    let agent = ToolAgent::new(Scenario::new(vec![Ok(response)]));
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &agent);
    assert!(!ctx.ask::<bool>().await.unwrap());
    assert!(graph.reads.lock().unwrap().is_empty());
    assert!(ctx.dependencies().is_empty());
    assert_eq!(agent.provider.requests.lock().unwrap().len(), 1);
}

// {test}`requirements.sdk.graph.agent-harness.submission.acceptance`
// {test}`requirements.sdk.graph.agent-harness.submission.reminder`
#[tokio::test]
// {test}`requirements.sdk.graph.running-criteria.ToolUsage.calls`
// {test}`requirements.sdk.graph.agent-harness.submission_count`
async fn count_is_checked_before_arguments_and_one_reminder_allows_recovery() {
    let graph = Memory::new("source".into());
    let agent = ToolAgent::new(Script::new(vec![
        Some(vec![("read_node", json!({"node":1}))]),
        Some(vec![
            ("read_node", json!({"node":1})),
            ("submit_result", json!({})),
            ("submit_result", json!({"other":true})),
        ]),
        Some(vec![("submit_result", json!({"result":true}))]),
    ]));
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &agent);
    assert!(ctx.ask::<bool>().await.unwrap());
    let tools = graph.cache().usage().tools;
    assert_eq!(tools["read_node"].calls, 1);
    assert_eq!(tools["submit_result"].calls, 1);
    assert_eq!(tools["submit_result"].result_bytes, 0);
    assert_eq!(*graph.reads.lock().unwrap(), [1]);
    assert_eq!(ctx.dependencies().len(), 1);
    let requests = agent.provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    let history = &requests[2].chat_history;
    let Message::User { content } = history[history.len() - 2].as_ref() else {
        panic!("expected skipped tool errors before reminder");
    };
    assert_eq!(content.len(), 3);
    for item in content {
        let UserContent::ToolResult(result) = item else {
            panic!("expected one error for each skipped call");
        };
        assert!(result.is_error);
        assert!(
            serde_json::to_string(result)
                .unwrap()
                .contains("not run: invoke submit_result exactly once")
        );
    }
    let history = serde_json::to_string(history).unwrap();
    assert_eq!(history.matches("Reminder:").count(), 1);
    assert!(history.contains("invoke submit_result exactly once"));
}

// {test}`requirements.sdk.graph.agent-harness.submission.acceptance`
// {test}`requirements.sdk.graph.agent-harness.submission.reminder`
#[tokio::test]
async fn zero_and_multiple_submissions_share_one_reminder_and_a_second_violation_ends() {
    let multi = || {
        vec![
            ("read_node", json!({"node":1})),
            ("submit_result", json!({"result":true})),
            ("submit_result", json!({"result":false})),
        ]
    };
    for first_multiple in [false, true] {
        for second_multiple in [false, true] {
            let graph = Memory::new("source".into());
            let agent = ToolAgent::new(Script::new(vec![
                Some(if first_multiple { multi() } else { vec![] }),
                Some(if second_multiple { multi() } else { vec![] }),
                Some(vec![("submit_result", json!({"result":true}))]),
            ]));
            let ctx =
                ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &agent);
            assert!(
                matches!(ctx.ask::<bool>().await, Err(ValidationError::Agent(error))
                    if error.to_string().contains("exactly once, even after a reminder"))
            );
            assert!(graph.reads.lock().unwrap().is_empty());
            assert!(ctx.dependencies().is_empty());
            let requests = agent.provider.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert_eq!(
                serde_json::to_string(&requests[1].chat_history)
                    .unwrap()
                    .matches("Reminder:")
                    .count(),
                1
            );
        }
    }
    let graph = Memory::new("source".into());
    let agent = ToolAgent::new(Script::new(vec![
        Some(vec![]),
        Some(vec![("read_node", json!({"node":1}))]),
        Some(multi()),
    ]));
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &agent);
    assert!(
        matches!(ctx.ask::<bool>().await, Err(ValidationError::Agent(message)) if message.contains("even after a reminder"))
    );
    assert_eq!(*graph.reads.lock().unwrap(), [1]);
    let graph = Memory::new("source".into());
    let agent = ToolAgent::new(Script::new(vec![
        Some(vec![]),
        Some(vec![("submit_result", json!({"result":true}))]),
    ]));
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &agent);
    assert!(ctx.ask::<bool>().await.unwrap());
    assert_eq!(agent.provider.requests.lock().unwrap().len(), 2);
}

// {test}`requirements.sdk.sources.redaction`
// {test}`requirements.sdk.graph.agent-harness.fixed_tools`
#[tokio::test]
// {test}`requirements.authoring.redaction`
// {test}`requirements.authoring.redaction_notice`
async fn queued_contents_and_graph_read_tools_redact_credentials_before_delivery() {
    let secret = "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890";
    let original = format!("credential = {secret}\n");
    let graph = Memory::new(original.clone());
    let agent = ToolAgent::new(Script::new(vec![
        Some(vec![("read_node", json!({"node":1}))]),
        Some(vec![("submit_result", json!({"result":true}))]),
    ]));
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &agent);
    assert!(ctx.ask::<bool>().contents_of_current_node().await.unwrap());
    let requests = agent.provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    for request in requests.iter() {
        let visible = serde_json::to_string(&request.chat_history).unwrap();
        assert!(!visible.contains(secret), "{visible}");
        assert!(visible.contains("[REDACTED:GitHub Token]"), "{visible}");
    }
    assert!(
        matches!(&ctx.dependencies()[..], [Dependency::Node(observed, fingerprint)] if *observed == graph.cache().intern_node(Node(1)) && fingerprint.value == original)
    );
    assert_eq!(*graph.contents.lock().unwrap(), original);
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

// {test}`requirements.sdk.graph.running-criteria.Validator.usage`
// {test}`requirements.sdk.graph.running-criteria.UsageStats`
// {test}`requirements.sdk.graph.running-criteria.TokenCount`
// {test}`requirements.sdk.graph.running-criteria.Validator`
// {test}`requirements.sdk.graph.running-criteria.Validator.new`
// {test}`requirements.sdk.graph.running-criteria.usage_after_failure`
// {test}`requirements.sdk.validation-cache.usage`
#[tokio::test(start_paused = true)]
async fn validators_keep_measured_tokens_after_failed_attempts_and_evidence_reuse() {
    let graph = Memory::new("source".into());
    let question = || Question {
        subject: graph.cache().intern_node(Node(1)),
        name: "check".into(),
        successors: false,
    };
    let mut answer = tool_turn(
        FinishReason::ToolCalls,
        vec![("submit_result", json!({"result":"ok"}))],
    );
    answer.usage = Usage::new()
        .input_tokens(120)
        .output_tokens(15)
        .cached_input_tokens(20)
        .reasoning_tokens(0);
    let agent = ToolAgent::new(Scenario::new(vec![
        Err(throttled()),
        Ok(answer),
        Err(io::Error::other("terminal")),
    ]));
    let mut validator = Validator::new(&graph);
    let evidence = verify_with_agent(&mut validator, &agent, question())
        .await
        .unwrap();
    validator.store_evidence(&evidence).await.unwrap();
    validator.refresh_evidence(question()).await.unwrap();
    let before_failure = graph.cache().usage();
    assert_eq!(before_failure.attempts, 2);
    assert_eq!(
        (
            before_failure.prepared_inputs,
            before_failure.delivered_inputs
        ),
        (1, 1)
    );
    assert!(
        verify_with_agent(&mut validator, &agent, question())
            .await
            .is_err()
    );
    for stats in [validator.usage().clone(), graph.cache().usage()] {
        assert_eq!(
            (stats.attempts, stats.retries, stats.failed_attempts),
            (3, 1, 2)
        );
        assert_eq!(
            (
                stats.input_tokens.reported,
                stats.input_tokens.missing_attempts
            ),
            (120, 2)
        );
        assert_eq!(
            (
                stats.output_tokens.reported,
                stats.output_tokens.missing_attempts
            ),
            (15, 2)
        );
        assert_eq!((stats.prepared_inputs, stats.delivered_inputs), (2, 1));
        assert_eq!(stats.cached_input_tokens.reported, 20);
        assert_eq!(stats.cache_creation_input_tokens.missing_attempts, 3);
        assert_eq!(
            (
                stats.reasoning_tokens.reported,
                stats.reasoning_tokens.missing_attempts
            ),
            (0, 2)
        );
    }
    let fresh = Validator::new(&graph);
    let zero = fresh.usage();
    assert!(zero.tools.is_empty());
    assert_eq!((zero.prepared_inputs, zero.delivered_inputs), (0, 0));
    assert_eq!(
        (zero.attempts, zero.retries, zero.failed_attempts),
        (0, 0, 0)
    );
    for count in [
        zero.input_tokens,
        zero.output_tokens,
        zero.cached_input_tokens,
        zero.cache_creation_input_tokens,
        zero.reasoning_tokens,
    ] {
        assert_eq!((count.reported, count.missing_attempts), (0, 0));
    }
    assert_eq!(before_failure.failed_attempts, 1);
}

// {test}`requirements.sdk.validation-cache.cache`
// {test}`requirements.sdk.validation-cache.shared_usage`
// {test}`requirements.sdk.validation-cache.usage`
// {test}`requirements.sdk.graph.running-criteria.UsageStats`
// {test}`requirements.sdk.graph.running-criteria.TokenCount`
#[tokio::test(start_paused = true)]
async fn cache_retains_usage_across_validators_and_sums_each_token_category() {
    let graph = Memory::new("source".into());
    let peer = graph.clone();
    let cache = peer.cache().clone();
    let question = || Question {
        subject: graph.cache().intern_node(Node(1)),
        name: "check".into(),
        successors: false,
    };
    let answer = |usage| {
        let mut response = tool_turn(
            FinishReason::ToolCalls,
            vec![("submit_result", json!({"result":"ok"}))],
        );
        response.usage = usage;
        Ok(response)
    };
    let agent = ToolAgent::new(Scenario::new(vec![
        Err(throttled()),
        answer(
            Usage::new()
                .input_tokens(12)
                .output_tokens(9)
                .cached_input_tokens(3)
                .cache_creation_input_tokens(2)
                .reasoning_tokens(5),
        ),
        answer(
            Usage::new()
                .input_tokens(7)
                .output_tokens(4)
                .cached_input_tokens(2)
                .cache_creation_input_tokens(1)
                .reasoning_tokens(3),
        ),
    ]));
    {
        let mut first = Validator::new(&graph);
        let mut second = Validator::new(&peer);
        verify_with_agent(&mut first, &agent, question())
            .await
            .unwrap();
        verify_with_agent(&mut second, &agent, question())
            .await
            .unwrap();
        assert_eq!(first.usage().attempts, 2);
        assert_eq!(second.usage().attempts, 1);
    }
    let total = cache.usage();
    assert_eq!((total.prepared_inputs, total.delivered_inputs), (2, 2));
    assert_eq!(graph.cache().usage().attempts, 3);
    assert_eq!(peer.cache().usage().attempts, 3);
    assert_eq!(
        (total.attempts, total.retries, total.failed_attempts),
        (3, 1, 1)
    );
    for (count, sum) in [
        (total.input_tokens, 19),
        (total.output_tokens, 13),
        (total.cached_input_tokens, 5),
        (total.cache_creation_input_tokens, 3),
        (total.reasoning_tokens, 8),
    ] {
        assert_eq!((count.reported, count.missing_attempts), (sum, 1));
    }
}

// {test}`requirements.authoring.provider_backoff`
#[tokio::test(start_paused = true)]
async fn a_timed_out_request_retries_and_each_success_resets_the_backoff_window() {
    struct Provider(Mutex<Vec<(tokio::time::Instant, CompletionRequest)>>);
    impl CompletionProvider for Provider {
        async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
            let attempt = {
                let mut seen = self.0.lock().unwrap();
                seen.push((tokio::time::Instant::now(), request));
                seen.len()
            };
            match attempt {
                1 => std::future::pending().await,
                2 | 3 | 5 => Err(throttled()),
                4 => Ok(tool_turn(
                    FinishReason::ToolCalls,
                    vec![("lookup", json!({}))],
                )),
                6 => Ok(tool_turn(
                    FinishReason::ToolCalls,
                    vec![("submit_result", json!({"result":true}))],
                )),
                _ => panic!("unexpected retry"),
            }
        }
    }
    let provider = Provider(Mutex::default());
    let graph = Memory::new(String::new());
    let ctx = ValidationContext::new(&graph, graph.cache().intern_node(Node(0)));
    let mut session = MinimalConversation::default();
    let accepted: bool = converse(
        &ctx,
        &provider,
        &mut session,
        CompletionRequest::new("review"),
        Vec::new(),
        json!({"type":"boolean"}),
        |answer| Ok(Submission::Accepted(answer)),
    )
    .await
    .unwrap();
    assert!(accepted);
    let requests = provider.0.lock().unwrap();
    let delay = |n: usize| (requests[n].0 - requests[n - 1].0).as_millis();
    assert!((600_500..=601_001).contains(&delay(1)));
    assert!((1000..=2001).contains(&delay(2)));
    assert!((2000..=4001).contains(&delay(3)));
    assert!((500..=1001).contains(&delay(5)));
    for i in [1, 2, 3, 5] {
        assert_eq!(
            serde_json::to_value(&requests[i].1).unwrap(),
            serde_json::to_value(&requests[i - 1].1).unwrap()
        );
    }
    assert_eq!(session.dispatched, 1);
}

// {test}`requirements.authoring.provider_backoff`
#[tokio::test(start_paused = true)]
async fn cancelling_during_backoff_leaves_prepared_inputs_undelivered() {
    let graph = Memory::new("source".into());
    let mut validator = Validator::new(&graph);
    let provider = Scenario::new(vec![Err(throttled())]);
    let agent = ToolAgent::new(provider);
    let question = Question {
        subject: graph.cache().intern_node(Node(1)),
        name: "check".into(),
        successors: false,
    };
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            verify_with_agent(&mut validator, &agent, question)
        )
        .await
        .is_err()
    );
    tokio::time::advance(std::time::Duration::from_secs(90)).await;
    let stats = validator.usage();
    assert_eq!(
        (stats.attempts, stats.failed_attempts, stats.retries),
        (1, 1, 0)
    );
    assert!(stats.prepared_inputs > 0);
    assert_eq!(stats.delivered_inputs, 0);
}

// {test}`requirements.sdk.validation-cache.usage`
// {test}`requirements.sdk.graph.running-criteria.Validator.usage`
#[tokio::test(start_paused = true)]
async fn dropping_an_inflight_request_preserves_its_attempt() {
    struct WaitingProvider;
    impl CompletionProvider for WaitingProvider {
        async fn complete(&self, _: CompletionRequest) -> io::Result<CompletionResponse> {
            std::future::pending().await
        }
    }
    let graph = Memory::new("source".into());
    let mut validator = Validator::new(&graph);
    let agent = ToolAgent::new(WaitingProvider);
    let question = Question {
        subject: graph.cache().intern_node(Node(1)),
        name: "check".into(),
        successors: false,
    };
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            verify_with_agent(&mut validator, &agent, question)
        )
        .await
        .is_err()
    );
    for stats in [validator.usage().clone(), graph.cache().usage()] {
        assert_eq!(
            (
                stats.attempts,
                stats.failed_attempts,
                stats.input_tokens.missing_attempts
            ),
            (1, 0, 1)
        );
    }
}

// {test}`requirements.sdk.graph.agent-calls.judge.submission`
// {test}`requirements.sdk.graph.validation-context.tracked_inspection`
#[tokio::test]
async fn unknown_tools_return_errors_without_creating_inputs_or_preventing_submission() {
    let graph = Memory::new("source".into());
    let agent = ToolAgent::new(Script::new(vec![
        Some(vec![("invented", json!({}))]),
        Some(vec![("submit_result", json!({"result":true}))]),
    ]));
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &agent);
    assert!(ctx.ask::<bool>().await.unwrap());
    assert!(ctx.dependencies().is_empty());
    let requests = agent.provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    let delivered = serde_json::to_string(requests[1].chat_history.last().unwrap()).unwrap();
    assert!(delivered.contains("invented") && delivered.contains("unknown tool"));
}

// {test}`requirements.sdk.graph.agent-calls.judge.rendering`
// {test}`requirements.sdk.graph.validation-context.tracked_inspection`
// {test}`requirements.sdk.graph.agent-calls.author_procedure`
// {test}`requirements.sdk.graph.validation-context.read_node.behavior`
// {test}`requirements.sdk.graph.validation-context.read_node.inputs`
// {test}`requirements.sdk.graph.evidence.node_inputs`
#[tokio::test]
async fn node_preparation_tool_errors_allow_recovery_without_recording_undelivered_contents() {
    let mut graph = Memory::new("source".into());
    graph.formatter = |_, _| Err(io::Error::other("unsupported node contents"));
    let agent = ToolAgent::new(Script::new(vec![
        Some(vec![("read_node", json!({"node":1}))]),
        Some(vec![("submit_result", json!({"result":true}))]),
    ]));
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &agent);
    assert!(ctx.ask::<bool>().await.unwrap());
    assert!(ctx.dependencies().is_empty());
    let requests = agent.provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    let Message::User { content } = requests[1].chat_history.last().unwrap().as_ref() else {
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
// {test}`requirements.sdk.graph.agent-harness.submission_count`
// {test}`requirements.sdk.graph.validation-context.tracked_inspection`
#[tokio::test(start_paused = true)]
async fn retries_and_cut_off_tools_preserve_inputs_and_submission_skips_other_reads() {
    let graph = Memory::new("source".into());
    let calls = || {
        vec![
            ("read_node", json!({"node":2})),
            ("submit_result", json!({"result":false})),
        ]
    };
    let agent = ToolAgent::new(Scenario::new(vec![
        Err(throttled()),
        Ok(tool_turn(FinishReason::Length, calls())),
        Ok(tool_turn(
            FinishReason::ToolCalls,
            vec![
                ("read_node", json!({"node":2})),
                ("submit_result", json!({"result":true})),
            ],
        )),
    ]));
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &agent);
    assert!(
        ctx.ask::<bool>()
            .context("review".to_owned())
            .contents_of_current_node()
            .await
            .unwrap()
    );
    assert!(matches!(&ctx.dependencies()[..],
        [Dependency::Node(observed, fingerprint)] if *observed == graph.cache().intern_node(Node(1)) && fingerprint.value == "source"));
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
    fn tools(&self, submission: Option<ToolDefinition>) -> io::Result<Vec<ToolDefinition>> {
        let mut tools = Vec::new();
        for name in ["lookup", "read_node"] {
            tools.push(serde_json::from_value(
                json!({"name":name,"description":"fixture tool","parameters":{"type":"object"}}),
            )?);
        }
        tools.extend(submission);
        Ok(tools)
    }
    // {cite}`habeas:util`
    fn submit(&mut self, name: &str, arguments: &Value) -> io::Result<Submission<bool>> {
        assert_eq!(name, "submit_result");
        Ok(Submission::Accepted(arguments.as_bool().unwrap()))
    }
    // {cite}`habeas:util`
    async fn dispatch(&mut self, name: &str, arguments: &Value) -> io::Result<(String, usize)> {
        self.dispatched += 1;
        match (name, arguments.get("key").and_then(Value::as_str)) {
            ("read_node", _) => Ok(("λ".repeat(320), 1)),
            ("lookup", Some("value")) => Ok(("fixture answer".into(), 0)),
            ("lookup", _) => Err(io::Error::other("absent")),
            _ => panic!("unexpected tool {name}"),
        }
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

// {test}`requirements.sdk.graph.running-criteria.ToolUsage.calls`
// {test}`requirements.sdk.graph.running-criteria.ToolUsage.result_bytes`
// {test}`requirements.sdk.graph.running-criteria.usage_by_tool`
// {test}`requirements.sdk.graph.running-criteria.ToolUsage.replay`
#[tokio::test(start_paused = true)]
async fn tool_usage_counts_utf8_results_and_errors_once_across_retries() {
    let provider = Scenario::new(vec![
        Ok(tool_turn(
            FinishReason::ToolCalls,
            vec![
                ("read_node", json!({})),
                ("read_node", json!({})),
                ("lookup", json!({"key":"missing"})),
            ],
        )),
        Err(throttled()),
        Ok(tool_turn(
            FinishReason::ToolCalls,
            vec![("submit_result", json!({"result":true}))],
        )),
    ]);
    let domain = Memory::new(String::new());
    let ctx = ValidationContext::new(&domain, domain.cache().intern_node(Node(0)));
    let mut session = MinimalConversation::default();
    converse(
        &ctx,
        &provider,
        &mut session,
        CompletionRequest::new("review"),
        Vec::new(),
        json!({"type":"boolean"}),
        |answer| Ok(Submission::Accepted(answer)),
    )
    .await
    .unwrap();
    assert_eq!(
        serde_json::to_value(domain.cache().usage().tools).unwrap(),
        json!({
            "read_node":{"calls":2,"result_bytes":1280},
            "lookup":{"calls":1,"result_bytes":6},
            "submit_result":{"calls":1,"result_bytes":0}
        })
    );
}

// {test}`requirements.authoring.output_limit`
#[tokio::test]
async fn truncation_without_a_provider_output_limit_fails_before_dispatch() {
    struct Provider;
    impl CompletionProvider for Provider {
        async fn complete(&self, _: CompletionRequest) -> io::Result<CompletionResponse> {
            Ok(tool_turn(
                FinishReason::Length,
                vec![("lookup", json!({"key":"value"}))],
            ))
        }
    }
    let domain = Memory::new(String::new());
    let ctx = ValidationContext::new(&domain, domain.cache().intern_node(Node(0)));
    let mut session = MinimalConversation::default();
    let result = converse(
        &ctx,
        &Provider,
        &mut session,
        CompletionRequest::new("review"),
        Vec::new(),
        json!({"type":"boolean"}),
        |answer| Ok(Submission::Accepted(answer)),
    )
    .await;
    let Err(AgentError::Conversation(error)) = result else {
        panic!("expected an incomplete-response error, got {result:?}");
    };
    assert!(
        error
            .to_string()
            .contains("an output limit the configuration does not set")
    );
    assert_eq!(session.dispatched, 0);
}

// {test}`requirements.sdk.graph.agent-calls.author_procedure`
// {test}`requirements.authoring.provider_backoff`
// {test}`requirements.authoring.output_limit`
// {test}`requirements.sdk.graph.agent-harness.submission_count`
// {test}`requirements.sdk.graph.running-criteria.UsageStats`
#[tokio::test(start_paused = true)]
// {test}`requirements.sdk.graph.running-criteria.ToolUsage.calls`
// {test}`requirements.sdk.graph.running-criteria.ToolUsage.replay`
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
    let domain = Memory::new(String::new());
    let ctx = ValidationContext::new(&domain, domain.cache().intern_node(Node(0)));
    let mut session = MinimalConversation::default();
    assert!(
        converse(
            &ctx,
            &provider,
            &mut session,
            CompletionRequest::new("review"),
            Vec::new(),
            json!({"type":"boolean"}),
            |answer| Ok(Submission::Accepted(answer)),
        )
        .await
        .unwrap()
    );
    // The successful retry dispatches a tool. The next turn and continuation are fresh requests.
    let usage = domain.cache().usage();
    assert_eq!(
        (usage.attempts, usage.retries, usage.failed_attempts),
        (4, 1, 1)
    );
    assert_eq!(session.dispatched, 1);
    assert_eq!(usage.tools["lookup"].calls, 1);
    assert_eq!(
        usage.tools["lookup"].result_bytes,
        "fixture answer".len() as u64
    );
    assert_eq!(usage.tools["submit_result"].calls, 1);
    assert_eq!(usage.tools["submit_result"].result_bytes, 0);
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

/// Determine whether tests cover the requirement's behavior and edge cases.
struct BehavioralAdequacy {
    requirement: NodeId<Memory>,
    review_policy: String,
}
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct TestReview {
    adequate: bool,
    rationale: String,
}
impl Criterion<Memory> for BehavioralAdequacy {
    type Ok = TestReview;
    fn source_node_id(&self) -> NodeId<Memory> {
        self.requirement.clone()
    }
    fn criterion_path(&self) -> CriterionPath {
        CriterionPath::new(vec!["behavior".into()])
    }
    fn version(&self) -> CriterionVersion {
        CriterionVersion::new("test-review-v1")
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

// {test}`requirements.sdk.graph.criteria.behavioral_adequacy`
// {test}`requirements.sdk.graph.criteria.criterion_definition`
// {test}`requirements.sdk.graph.running-criteria.evaluate_criterion`
// {test}`requirements.sdk.graph.evidence.refresh_evidence`
// {test}`requirements.sdk.graph.evidence.store_evidence`
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
    let mut validator = validation::Validator::new(&domain);
    let inputs = crate::agent_runtime::verify_with_agent(
        &mut validator,
        agent.as_ref(),
        BehavioralAdequacy {
            requirement: domain.cache().intern_node(Node(1)),
            review_policy: "Review test adequacy.".into(),
        },
    )
    .await
    .unwrap();
    validator.store_evidence(&inputs).await.unwrap();
    assert_eq!(inputs.output(), &expected);
    assert_eq!(inputs.criterion_path().segments(), ["behavior"]);
    assert_eq!(*domain.reads.lock().unwrap(), [1, 2]);
    assert_eq!(inputs.dependencies().len(), 3);
    for (id, value) in [
        (1, "Tokens expire at their expiration boundary."),
        (2, "Tests check earlier, equal, and later times."),
    ] {
        assert!(inputs.dependencies().iter().any(|dep| matches!(dep,
            Dependency::Node(n, fp) if *n == domain.cache().intern_node(Node(id)) && fp.value == value)));
    }
    assert!(inputs.dependencies().iter().any(|dep| matches!(dep,
        Dependency::OutgoingEdges(observed, Label, fp) if *observed == domain.cache().intern_node(Node(1)) && *fp == edge_fingerprint(&domain, &[domain.cache().intern_node(Node(2))]))));
    assert_eq!(domain.records.lock().unwrap().len(), 1);
    assert!(validator.check_inputs(&inputs).unwrap());
    let restored = validator
        .load_evidence(&BehavioralAdequacy {
            requirement: domain.cache().intern_node(Node(1)),
            review_policy: "Review test adequacy.".into(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(restored.output(), &expected);
    assert!(restored.source_node_id() == inputs.source_node_id());
    assert_eq!(restored.criterion_path(), inputs.criterion_path());
    assert_eq!(restored.version(), inputs.version());
    assert!(restored.dependencies() == inputs.dependencies());
    let reused = validator
        .refresh_evidence(BehavioralAdequacy {
            requirement: domain.cache().intern_node(Node(1)),
            review_policy: "Review test adequacy.".into(),
        })
        .await
        .unwrap();
    assert_eq!(reused.output(), &expected);
    assert!(reused.dependencies() == inputs.dependencies());
    assert_eq!(domain.records.lock().unwrap().len(), 1);
    domain
        .node_contents
        .lock()
        .unwrap()
        .insert(2, "Only earlier times are tested.".into());
    assert!(!validator.check_inputs(&inputs).unwrap());
    let requests = agent.provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        *requests[0].chat_history[0],
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
    let cx = ValidationContext::with_agent(&domain, domain.cache().intern_node(Node(1)), &agent);
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
    assert_eq!(cx.dependencies().len(), 3);
    let requests = agent.provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    assert!(
        requests
            .iter()
            .all(|r| r.chat_history == requests[0].chat_history)
    );
    assert_eq!(
        *requests[0].chat_history[0],
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
        _: &'a mut super::Pages<NodeId<Memory>, Label>,
    ) -> futures::future::BoxFuture<'a, io::Result<ToolDelivery<NodeId<Memory>, Label>>> {
        Box::pin(async move {
            assert_eq!(name, "increment");
            self.count += 1;
            Ok(ToolDelivery {
                text: format!("counter={}", self.count),
                input_count: 0,
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
    .with_tools(Arc::new(CounterTools(created.clone())));
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &agent);
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
    assert!(ctx.dependencies().is_empty());
}

// {test}`requirements.sdk.graph.agent-harness.tool_agent`
// {test}`requirements.sdk.graph.validation-context.read_node.mcp`
// {test}`requirements.sdk.graph.validation-context.outgoing_edges.mcp`
// {test}`requirements.sdk.graph.validation-context.pages.mcp`
// {test}`requirements.sdk.graph.validation-context.outgoing_edges.behavior`
#[tokio::test]
async fn application_tools_preserve_graph_tools_in_the_same_conversation() {
    let contents = "x".repeat(18_000);
    let graph = Memory::new(contents.clone());
    let created = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let agent = ToolAgent::new(Script::new(vec![
        Some(vec![
            ("outgoing_edges", json!({"node":1,"label":null})),
            ("increment", json!({})),
        ]),
        Some(vec![
            ("read_node", json!({"node":2})),
            ("increment", json!({})),
        ]),
        Some(vec![("get_next_page", json!({"use_next":true}))]),
        Some(vec![("submit_result", json!({"result":true}))]),
    ]))
    .with_tools(CounterTools(created));
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &agent);
    assert!(ctx.ask::<bool>().await.unwrap());

    let requests = agent.provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    for request in requests.iter() {
        for name in [
            "outgoing_edges",
            "read_node",
            "get_next_page",
            "increment",
            "submit_result",
        ] {
            assert!(request.tools.iter().any(|tool| tool.name == name));
        }
    }
    let deliveries: Vec<Vec<String>> = requests[1..]
        .iter()
        .map(|request| {
            let Message::User { content } = request.chat_history.last().unwrap().as_ref() else {
                panic!("expected tool results");
            };
            content
                .iter()
                .map(|item| {
                    let UserContent::ToolResult(result) = item else {
                        panic!("expected tool result");
                    };
                    assert!(!result.is_error);
                    let [ToolResultContent::Text(text)] = result.content.as_slice() else {
                        panic!("expected text result");
                    };
                    text.text.clone()
                })
                .collect()
        })
        .collect();
    assert_eq!(
        serde_json::from_str::<Value>(&deliveries[0][0]).unwrap(),
        json!({"targets":[2]})
    );
    assert_eq!(deliveries[0][1], "counter=1");
    assert_eq!(
        serde_json::from_str::<Value>(&deliveries[1][0]).unwrap(),
        json!({"text":"x".repeat(8_192), "next":returned_node_page(&requests[2]).to_string()})
    );
    assert_eq!(deliveries[1][1], "counter=2");
    let following: Value = serde_json::from_str(&deliveries[2][0]).unwrap();
    assert_eq!(following["text"], "x".repeat(8_192));
    assert_ne!(
        following["next"],
        returned_node_page(&requests[2]).to_string()
    );
    assert!(uuid::Uuid::parse_str(following["next"].as_str().unwrap()).is_ok());
    let observations = ctx.dependencies();
    assert_eq!(observations.len(), 2);
    assert!(
        matches!(&observations[0], Dependency::OutgoingEdges(observed, Label, fingerprint) if *observed == graph.cache().intern_node(Node(1)) && *fingerprint == edge_fingerprint(&graph, &[graph.cache().intern_node(Node(2))]))
    );
    assert!(
        matches!(&observations[1], Dependency::Node(observed, fingerprint) if *observed == graph.cache().intern_node(Node(2)) && fingerprint.value == contents)
    );
}

// {test}`requirements.sdk.graph.validation-context.read_node.mcp`
// {test}`requirements.sdk.graph.validation-context.read_node.behavior`
#[tokio::test]
async fn a_complete_node_read_has_text_and_no_continuation() {
    let graph = Memory::new("short text".into());
    let agent = ToolAgent::new(Script::new(vec![
        Some(vec![("read_node", json!({"node":1}))]),
        Some(vec![("submit_result", json!({"result":true}))]),
    ]));
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &agent);
    assert!(ctx.ask::<bool>().await.unwrap());
    let requests = agent.provider.requests.lock().unwrap();
    let Message::User { content } = requests[1].chat_history.last().unwrap().as_ref() else {
        panic!("expected read_node response")
    };
    let [UserContent::ToolResult(reply)] = content.as_slice() else {
        panic!("expected one tool result")
    };
    assert_eq!(
        reply.content,
        [ToolResultContent::text(
            json!({"text":"short text"}).to_string()
        )]
    );
    assert_eq!(ctx.dependencies().len(), 1);
}

// {test}`requirements.sdk.graph.validation-context.pages.behavior`
// {test}`requirements.sdk.graph.validation-context.pages.inputs`
// {test}`requirements.sdk.graph.validation-context.read_node.inputs`
// {test}`requirements.sdk.graph.evidence.node_inputs`
// {test}`requirements.sdk.graph.validation-context.read_node.behavior`
// {test}`requirements.sdk.graph.validation-context.read_node.mcp`
#[tokio::test]
async fn graph_read_pages_retain_the_prepared_snapshot_after_contents_change() {
    struct ChangingProvider {
        script: Script,
        graph: Memory,
    }
    impl CompletionProvider for ChangingProvider {
        async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
            if self.script.requests.lock().unwrap().len() == 1 {
                *self.graph.contents.lock().unwrap() = "replacement contents".into();
            }
            self.script.complete(request).await
        }
    }
    let original = format!("{}/second page", "é".repeat(8192));
    let graph = Memory::new(original.clone());
    let agent = ToolAgent::new(ChangingProvider {
        graph: graph.clone(),
        script: Script::new(vec![
            Some(vec![("read_node", json!({"node":1}))]),
            Some(vec![("get_next_page", json!({"use_next":true}))]),
            Some(vec![("submit_result", json!({"result":true}))]),
        ]),
    });
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &agent);
    assert!(ctx.ask::<bool>().await.unwrap());
    assert_eq!(*graph.reads.lock().unwrap(), [1]);
    assert!(
        matches!(&ctx.dependencies()[..], [Dependency::Node(observed, fingerprint)] if *observed == graph.cache().intern_node(Node(1)) && fingerprint.value == original)
    );
    let requests = agent.provider.script.requests.lock().unwrap();
    let mut retained = String::new();
    for (index, request) in requests[1..].iter().enumerate() {
        let Message::User { content } = request.chat_history.last().unwrap().as_ref() else {
            panic!("expected tool response")
        };
        let [UserContent::ToolResult(result)] = content.as_slice() else {
            panic!("expected one page")
        };
        let [ToolResultContent::Text(text)] = result.content.as_slice() else {
            panic!("expected page text")
        };
        let page: Value = serde_json::from_str(&text.text).unwrap();
        assert_eq!(page.get("next").is_some(), index == 0);
        retained.push_str(page["text"].as_str().unwrap());
    }
    assert_eq!(retained, original);
}

// {test}`requirements.sdk.graph.validation-context.pages.behavior`
// {test}`requirements.sdk.graph.agent-harness.tool_agent.isolation`
#[tokio::test]
async fn graph_page_ids_from_a_populated_peer_conversation_are_rejected() {
    struct ForeignPage {
        state: Mutex<(usize, Option<uuid::Uuid>)>,
    }
    impl CompletionProvider for ForeignPage {
        async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
            let mut state = self.state.lock().unwrap();
            let call = match state.0 {
                0 | 2 => ("read_node", json!({"node":1})),
                1 => {
                    state.1 = Some(returned_node_page(&request));
                    ("submit_result", json!({"result":true}))
                }
                3 => {
                    let local = returned_node_page(&request);
                    let foreign = state.1.unwrap();
                    assert_ne!(local, foreign);
                    ("get_next_page", json!({"uuid":foreign.to_string()}))
                }
                4 => {
                    let Message::User { content } = request.chat_history.last().unwrap().as_ref()
                    else {
                        panic!("expected foreign-page error");
                    };
                    let [UserContent::ToolResult(result)] = content.as_slice() else {
                        panic!("expected one foreign-page result");
                    };
                    assert!(result.is_error);
                    assert!(
                        serde_json::to_string(result)
                            .unwrap()
                            .contains("unknown page")
                    );
                    ("submit_result", json!({"result":true}))
                }
                _ => panic!("unexpected page fixture request"),
            };
            state.0 += 1;
            Ok(tool_turn(FinishReason::ToolCalls, vec![call]))
        }
    }
    let graph = Memory::new("long contents ".repeat(1500));
    let agent = ToolAgent::new(ForeignPage {
        state: Mutex::new((0, None)),
    });
    let ctx = ValidationContext::with_agent(&graph, graph.cache().intern_node(Node(1)), &agent);
    assert!(ctx.ask::<bool>().await.unwrap());
    assert!(ctx.ask::<bool>().await.unwrap());
    assert_eq!(*graph.reads.lock().unwrap(), [1, 1]);
    assert_eq!(agent.provider.state.lock().unwrap().0, 5);
}

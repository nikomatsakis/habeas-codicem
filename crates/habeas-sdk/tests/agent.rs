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
// {cite}`habeas:util`
#[derive(Clone, PartialEq, Eq)]
struct Label;
// {cite}`habeas:util`
struct Contents(String);
// {cite}`habeas:util`
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
        ctx: &'a ValidationContext<'d, Self>,
    ) -> Result<Judgment<Metadata>, ValidationError<io::Error>>
    where
        'd: 'a,
    {
        let text = ctx
            .inspection()
            .read(ctx.node_id())
            .map_err(ValidationError::Graph)?;
        Ok(Judgment {
            result: false,
            rationale: text.0,
            metadata: Metadata,
        })
    }
}
// {cite}`habeas:util`
struct Renderer;
impl AgentInputRenderer<Memory> for Renderer {
    fn render(&self, input: &AgentInput<'_, Memory>) -> io::Result<String> {
        Ok(input.contents.0.clone())
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
    let graph = Memory(Arc::new(Mutex::new("source".into())));
    let agent = ToolAgent::new(
        Script::new(vec![Some(vec![("submit_result", json!({"result":true}))])]),
        Renderer,
    );
    let ctx = ValidationContext::with_agent(&graph, Node(1), &agent, &agent.renderer);
    assert!(
        ctx.ask::<bool>()
            .context("Question é".to_owned())
            .current_node_contents()
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
struct DistinctPages;
impl AgentInputRenderer<Memory> for DistinctPages {
    // {cite}`habeas:util`
    fn render(&self, input: &AgentInput<'_, Memory>) -> io::Result<String> {
        let label = if input.node.0 == 1 {
            "ALPHA-CONTENTS"
        } else {
            "BETA-CONTENTS"
        };
        Ok(format!("{label}:{}", input.contents.0).repeat(1_500))
    }
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
#[tokio::test]
async fn concurrent_tool_agents_isolate_histories_pages_and_submissions_and_share_reads() {
    use std::future::IntoFuture;
    let graph = Memory(Arc::new(Mutex::new("source".into())));
    let agent = ToolAgent::new(
        ParallelConversations {
            barrier: tokio::sync::Barrier::new(2),
            calls: Mutex::default(),
        },
        DistinctPages,
    );
    let ctx = ValidationContext::with_agent(&graph, Node(1), &agent, &agent.renderer);
    let (alpha, beta) = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        futures::join!(
            ctx.ask::<String>()
                .context("alpha".to_owned())
                .current_node_contents()
                .into_future(),
            ctx.ask::<String>()
                .context("beta".to_owned())
                .successors_content(Label)
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
        Dependency::OutgoingEdges(
            Node(1),
            Fingerprint {
                scheme: "edges".into(),
                value: "ordered-edge-snapshot".into(),
            },
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
        ctx.inspection()
            .finish(
                "v1".into(),
                Judgment {
                    result: true,
                    rationale: "both calls".into(),
                    metadata: Metadata,
                }
            )
            .dependencies
            == dependencies
    );
}

// {test}`requirements.sdk.graph.traversal.validate_transitive`
// {test}`requirements.sdk.graph.context-tools.tracked_inspection`
// {test}`requirements.sdk.graph.fingerprinting.snapshot`
#[tokio::test]
async fn agent_edge_tools_use_traversals_retained_selection_without_relisting() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct GraphWithChangingEdges {
        lookups: AtomicUsize,
        records: Mutex<Vec<(u8, Record<u8, ()>)>>,
    }
    impl Graph for GraphWithChangingEdges {
        type NodeId = u8;
        type NodeContent = String;
        type EdgeLabel = ();
        type JudgmentMetadata = ();
        type Error = io::Error;
        fn node_content(&self, node: &u8) -> io::Result<String> {
            Ok(format!("node {node}"))
        }
        fn node_path(&self, node: &u8) -> NodePath {
            NodePath {
                segments: vec![node.to_string()],
            }
        }
        fn outgoing_edges(&self, node: &u8) -> io::Result<Snapshot<Vec<Edge<u8, ()>>>> {
            if *node == 1 && self.lookups.fetch_add(1, Ordering::SeqCst) != 0 {
                return Err(io::Error::other("must not relist discovered edges"));
            }
            Ok(Snapshot {
                content: if *node == 1 {
                    vec![Edge {
                        label: (),
                        target: 2,
                    }]
                } else {
                    vec![]
                },
                fingerprint: Fingerprint {
                    scheme: "selection".into(),
                    value: format!("retained {node}"),
                },
            })
        }
    }
    impl Fingerprinting for GraphWithChangingEdges {
        fn fingerprint(&self, _: &u8, contents: &String) -> Fingerprint {
            Fingerprint {
                scheme: "content".into(),
                value: contents.clone(),
            }
        }
    }
    impl EvidenceStorage for GraphWithChangingEdges {
        async fn load_evidence(&self, _: &u8) -> io::Result<Option<Record<u8, ()>>> {
            Ok(None)
        }
        async fn store_evidence(&self, node: &u8, record: &Record<u8, ()>) -> io::Result<()> {
            self.records.lock().unwrap().push((*node, record.clone()));
            Ok(())
        }
    }
    impl Validation for GraphWithChangingEdges {
        async fn validate<'a, 'd>(
            &'a self,
            ctx: &'a ValidationContext<'d, Self>,
        ) -> Result<Judgment<()>, ValidationError<io::Error>>
        where
            'd: 'a,
        {
            let result = if *ctx.node_id() == 1 {
                ctx.ask::<bool>().await?
            } else {
                true
            };
            Ok(Judgment {
                result,
                rationale: "reviewed".into(),
                metadata: (),
            })
        }
    }
    struct Render;
    impl AgentInputRenderer<GraphWithChangingEdges> for Render {
        fn render(&self, input: &AgentInput<'_, GraphWithChangingEdges>) -> io::Result<String> {
            Ok(input.contents.clone())
        }
    }
    let graph = GraphWithChangingEdges {
        lookups: AtomicUsize::new(0),
        records: Mutex::default(),
    };
    let agent = ToolAgent::new(
        Script::new(vec![
            Some(vec![("outgoing_edges", json!({"path":{"segments":["1"]}}))]),
            Some(vec![("submit_result", json!({"result":true}))]),
        ]),
        Render,
    );
    let result = Validator::new(&graph, |_| Ok("v1".into()))
        .with_agent(&agent, &agent.renderer)
        .validate_transitive(&1, TraversalOptions::default())
        .await;
    assert!(result.complete);
    assert_eq!(result.nodes.len(), 2);
    assert!(
        result
            .nodes
            .iter()
            .all(|(_, result)| result.as_ref().unwrap().result)
    );
    assert_eq!(graph.lookups.load(Ordering::SeqCst), 1);
    let records = graph.records.lock().unwrap();
    assert_eq!(
        records
            .iter()
            .find(|(id, _)| *id == 1)
            .unwrap()
            .1
            .dependencies,
        vec![Dependency::OutgoingEdges(
            1,
            Fingerprint {
                scheme: "selection".into(),
                value: "retained 1".into()
            }
        )]
    );
    assert!(
        records
            .iter()
            .find(|(id, _)| *id == 2)
            .unwrap()
            .1
            .dependencies
            .is_empty()
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
// {test}`requirements.sdk.graph.evidence.node_inputs`
// {test}`requirements.sdk.graph.fingerprinting.snapshot`
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
        matches!(&observations[1],Dependency::Node(Node(2),fingerprint) if fingerprint.value == "x".repeat(18_000))
    );
    let record = ctx.inspection().finish(
        "v1".into(),
        Judgment {
            result: true,
            rationale: "read".into(),
            metadata: Metadata,
        },
    );
    assert!(record.dependencies == observations);
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

// {test}`requirements.sdk.graph.validation.check_procedure`
// {test}`requirements.sdk.graph.domain_types`
// {test}`requirements.sdk.domain`
// {test}`requirements.sdk.graph.agent-calls.author_procedure`
#[tokio::test]
async fn validation_is_usable_without_any_evidence_storage() {
    let graph = Memory(Arc::new(Mutex::new("ordinary computation".into())));
    let ctx = ValidationContext::new(&graph, Node(1));
    let result = graph.validate(&ctx).await.unwrap();
    assert!(!result.result);
    assert_eq!(result.rationale, "ordinary computation");
    assert!(
        matches!(&ctx.inspection().dependencies()[0],Dependency::Node(Node(1),fingerprint) if fingerprint.value=="ordinary computation")
    );
}

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
    let graph = Memory(Arc::new(Mutex::new("source".into())));
    let agent = ToolAgent::new(TextOnly(Mutex::default()), Renderer);
    let ctx = ValidationContext::with_agent(&graph, Node(1), &agent, &agent.renderer);
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
    let failing = ToolAgent::new(Script::new(vec![None]), Renderer);
    let ctx = ValidationContext::with_agent(&graph, Node(1), &failing, &failing.renderer);
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

// {test}`requirements.sdk.sources.provider_observations`
// {test}`requirements.sdk.graph.context-tools.tracked_inspection`
// {test}`requirements.sdk.sources.provider_results.parse_snapshot`
#[cfg(feature = "default-languages")]
// {test}`requirements.sdk.graph.evidence.node_inputs`
#[tokio::test]
async fn cached_selected_source_reads_record_exact_delivered_contents_and_errors_add_no_input() {
    let graph = Memory(Arc::new(Mutex::new("different current source".into())));
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
    ]),Renderer).with_tools(factory);
    let ctx = ValidationContext::with_agent(&graph, Node(1), &agent, &agent.renderer);
    assert!(ctx.ask::<bool>().await.unwrap());
    assert!(
        matches!(&ctx.inspection().dependencies()[..],[Dependency::Node(Node(7),fingerprint)] if fingerprint.value=="fn observed() { call(); }" && fingerprint.scheme=="selected-content")
    );
}

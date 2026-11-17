use futures::future::BoxFuture;
use habeas_sdk::{
    graph::*,
    procedure::{
        Agent, AgentInput, AgentInputRenderer, AgentRequest, Context, context_fingerprint,
    },
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    borrow::Cow,
    cell::Cell,
    collections::BTreeMap,
    future::IntoFuture,
    num::NonZeroUsize,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Default)]
struct State {
    contents: BTreeMap<u8, String>,
    edges: BTreeMap<u8, Vec<(u8, u8)>>,
    records: BTreeMap<u8, Record<u8, ()>>,
    reads: Vec<u8>,
    selections: Vec<u8>,
    started: Vec<u8>,
    finished: Vec<u8>,
    fail_read: bool,
    fail_load: bool,
    fail_store: bool,
    fail_edges: Option<u8>,
}

#[derive(Default)]
struct Memory {
    state: Mutex<State>,
    read_in_callback: bool,
    edges_in_callback: bool,
    ask_in_callback: bool,
    delay: bool,
    active: AtomicUsize,
    peak: AtomicUsize,
}

fn fp(value: impl Into<String>) -> Fingerprint {
    Fingerprint {
        scheme: "test-v1".into(),
        value: value.into(),
    }
}

impl Graph for Memory {
    type NodeId = u8;
    type NodeContent = String;
    type EdgeLabel = u8;
    type JudgmentMetadata = ();
    type Error = &'static str;

    fn node_content(&self, node: &u8) -> Result<String, Self::Error> {
        let mut state = self.state.lock().unwrap();
        state.reads.push(*node);
        if state.fail_read {
            return Err("read");
        }
        state.contents.get(node).cloned().ok_or("missing")
    }
    // :::{cite} habeas:coverage
    // This validation fixture stores records directly by opaque u8 node ID.
    // Its in-memory EvidenceStorage does not translate IDs into filesystem
    // paths; Graph still requires the addressing callback. Filesystem storage
    // and its consumer delegation tests exercise path-based storage separately.
    // :::
    fn node_path(&self, node: &u8) -> NodePath {
        NodePath {
            segments: vec![node.to_string()],
        }
    }
    fn outgoing_edges(&self, node: &u8) -> Result<Snapshot<Vec<Edge<u8, u8>>>, Self::Error> {
        let mut state = self.state.lock().unwrap();
        state.selections.push(*node);
        if state.fail_edges == Some(*node) {
            return Err("edges");
        }
        let edges = state.edges.get(node).cloned().unwrap_or_default();
        Ok(Snapshot {
            fingerprint: fp(format!("{edges:?}")),
            content: edges
                .into_iter()
                .map(|(label, target)| Edge { label, target })
                .collect(),
        })
    }
}
impl Fingerprinting for Memory {
    fn fingerprint(&self, _: &u8, content: &String) -> Fingerprint {
        fp(content.clone())
    }
}
impl EvidenceStorage for Memory {
    async fn load_evidence(&self, node: &u8) -> Result<Option<Record<u8, ()>>, Self::Error> {
        let state = self.state.lock().unwrap();
        if state.fail_load {
            return Err("load");
        }
        Ok(state.records.get(node).cloned())
    }
    async fn store_evidence(&self, node: &u8, record: &Record<u8, ()>) -> Result<(), Self::Error> {
        let mut state = self.state.lock().unwrap();
        if state.fail_store {
            return Err("store");
        }
        state.records.insert(*node, record.clone());
        Ok(())
    }
}
impl Validation for Memory {
    async fn validate<'a, 'd>(
        &'a self,
        ctx: &'a ValidationContext<'d, Self>,
    ) -> Result<Judgment<()>, ValidationError<Self::Error>>
    where
        'd: 'a,
    {
        let node = *ctx.node_id();
        self.state.lock().unwrap().started.push(node);
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(active, Ordering::SeqCst);
        if self.delay {
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        if self.read_in_callback {
            ctx.inspection()
                .read(&node)
                .map_err(ValidationError::Graph)?;
        }
        if self.edges_in_callback {
            ctx.successors(&1).map_err(ValidationError::Graph)?;
        }
        if self.ask_in_callback {
            let first: Answer = ctx.ask().current_node_contents().await?;
            let second: Answer = ctx
                .ask()
                .context(format!("Check intermediate result {}", first.n))
                .await?;
            assert_eq!(second.n, first.n);
        }
        self.active.fetch_sub(1, Ordering::SeqCst);
        self.state.lock().unwrap().finished.push(node);
        Ok(Judgment {
            result: node != 9,
            rationale: format!("review {node}"),
            metadata: (),
        })
    }
}

// {test}`requirements.sdk.graph.evidence.checking_records`
// {test}`requirements.sdk.graph.validation.validate_node`
#[tokio::test]
async fn historical_freshness_reads_do_not_enter_the_replacement_trace() {
    let graph = Memory {
        read_in_callback: true,
        ..Memory::default()
    };
    {
        let mut state = graph.state.lock().unwrap();
        state.contents.insert(1, "root".into());
        state.contents.insert(2, "changed".into());
        state.records.insert(
            1,
            Record {
                procedure: "v1".into(),
                dependencies: vec![Dependency::Node(2, fp("old"))],
                output: Judgment {
                    result: true,
                    rationale: "historical".into(),
                    metadata: (),
                },
            },
        );
    }
    Validator::new(&graph, |_| Ok("v1".into()))
        .validate_node(&1)
        .await
        .unwrap();
    let state = graph.state.lock().unwrap();
    assert_eq!(state.reads, vec![2, 1]);
    assert_eq!(
        state.records[&1].dependencies,
        vec![Dependency::Node(1, fp("root"))]
    );
}

// {test}`requirements.sdk.graph.agent-calls.model_capability`
// {test}`requirements.sdk.graph.agent-calls.procedures`
// {test}`requirements.sdk.graph.validation.validate_node.errors`
#[tokio::test]
async fn runner_does_not_store_a_missing_agent_error_and_supports_intermediate_calls() {
    let graph = Memory {
        ask_in_callback: true,
        ..Memory::default()
    };
    graph
        .state
        .lock()
        .unwrap()
        .contents
        .insert(1, "root".into());
    let validator = Validator::new(&graph, |_| Ok("v1".into()));
    assert!(matches!(
        validator.validate_node(&1).await,
        Err(ValidationError::NoAgentConfigured)
    ));
    {
        let state = graph.state.lock().unwrap();
        assert!(state.records.is_empty());
        assert!(state.reads.is_empty());
    }
    let agent = MockAgent::default();
    let validator = validator.with_agent(&agent, &Renderer);
    assert!(validator.validate_node(&1).await.unwrap().result);
    let state = graph.state.lock().unwrap();
    assert_eq!(
        state.records[&1].dependencies,
        vec![Dependency::Node(1, fp("root"))]
    );
    let requests = agent.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].0, "Check intermediate result 7");
    assert!(requests[1].2.is_empty());
}

struct Renderer;
impl AgentInputRenderer<Memory> for Renderer {
    fn render(&self, input: &AgentInput<'_, Memory>) -> Result<String, &'static str> {
        Ok(input.contents.clone())
    }
}

// {cite}`habeas:util`
#[derive(Default)]
struct MockAgent {
    requests: Mutex<Vec<(String, Fingerprint, Vec<(u8, String)>, Value)>>,
    barrier: Option<Arc<tokio::sync::Barrier>>,
    invalid: bool,
    prefix_result: bool,
}
impl Agent<Memory> for MockAgent {
    fn run<'a>(
        &'a self,
        request: AgentRequest<u8>,
        inspection: &'a InspectionContext<'_, Memory>,
    ) -> BoxFuture<'a, Result<Value, std::io::Error>> {
        Box::pin(async move {
            if let Some(barrier) = &self.barrier {
                barrier.wait().await;
            }
            assert_eq!(request.submission_name, "submit_result");
            for input in &request.inputs {
                inspection.observe_read(input.node, input.fingerprint.clone());
            }
            let n = if self.prefix_result && request.prefix == "independent" {
                11
            } else {
                7
            };
            self.requests.lock().unwrap().push((
                request.prefix,
                request.prefix_fingerprint,
                request
                    .inputs
                    .into_iter()
                    .map(|input| (input.node, input.text))
                    .collect(),
                request.result_schema,
            ));
            Ok(if self.invalid {
                json!({"n": "wrong"})
            } else {
                json!({"n": n})
            })
        })
    }
}

// {cite}`habeas:util`
#[derive(Debug, Deserialize, JsonSchema)]
struct Answer {
    n: usize,
}

// {test}`requirements.sdk.graph.fingerprinting.snapshot`
// {test}`requirements.sdk.graph.evidence.node_inputs`
// {test}`requirements.sdk.graph.context-tools.tracked_inspection`
#[test]
fn snapshots_record_exact_reads_and_retain_distinct_versions() {
    let graph = Memory::default();
    graph
        .state
        .lock()
        .unwrap()
        .contents
        .insert(1, "before".into());
    let inspection = InspectionContext::new(&graph);
    assert_eq!(inspection.read(&1).unwrap(), "before");
    assert_eq!(
        inspection.read_snapshot(&1).unwrap().fingerprint,
        fp("before")
    );
    graph
        .state
        .lock()
        .unwrap()
        .contents
        .insert(1, "after".into());
    assert_eq!(inspection.read(&1).unwrap(), "after");
    assert!(inspection.read(&2).is_err());
    assert_eq!(
        inspection.dependencies(),
        vec![
            Dependency::Node(1, fp("before")),
            Dependency::Node(1, fp("after"))
        ]
    );
    assert_eq!(graph.state.lock().unwrap().reads, vec![1, 1, 1, 2]);
    let record = inspection.finish(
        "v1".into(),
        Judgment {
            result: true,
            rationale: "ok".into(),
            metadata: (),
        },
    );
    assert_eq!(
        record.dependencies,
        vec![
            Dependency::Node(1, fp("before")),
            Dependency::Node(1, fp("after"))
        ]
    );
    assert!(!record_is_current(&graph, "v1", &record).unwrap());
}

// {test}`requirements.sdk.graph.evidence.checking_records.procedure_match`
// {test}`requirements.sdk.graph.evidence.checking_records`
// {test}`requirements.sdk.graph.fingerprinting.fingerprint`
#[test]
fn wildcard_versions_still_check_inputs_and_errors() {
    let graph = Memory::default();
    graph
        .state
        .lock()
        .unwrap()
        .contents
        .insert(1, "same".into());
    let mut record = Record {
        procedure: "*".into(),
        dependencies: vec![Dependency::Node(1, fp("same"))],
        output: Judgment {
            result: false,
            rationale: "negative".into(),
            metadata: (),
        },
    };
    assert!(record_is_current(&graph, "v42", &record).unwrap());
    assert!(!record.output.result);
    record.procedure = "v*".into();
    assert!(!record_is_current(&graph, "v42", &record).unwrap());
    record.procedure = "v42".into();
    assert!(record_is_current(&graph, "v42", &record).unwrap());
    record.dependencies = vec![Dependency::Node(
        1,
        Fingerprint {
            scheme: "other".into(),
            value: "same".into(),
        },
    )];
    assert!(!record_is_current(&graph, "v42", &record).unwrap());
    record.procedure = "*".into();
    record.dependencies = vec![Dependency::Node(1, fp("changed"))];
    assert!(!record_is_current(&graph, "v42", &record).unwrap());
    graph.state.lock().unwrap().fail_read = true;
    assert_eq!(record_is_current(&graph, "v42", &record), Err("read"));
    for (stored, requested) in [
        ("v41", "v42"),
        ("v42", "v420"),
        ("V42", "v42"),
        ("v42", "*"),
    ] {
        assert!(!procedure_matches(stored, requested));
    }
    assert!(procedure_matches("v42", "v42"));
    assert!(!procedure_matches("v*", "v42"));
    assert!(!procedure_matches("", "v42"));
    assert!(procedure_matches("*", "anything"));
}

// {test}`requirements.sdk.graph.evidence.checking_records`
// {test}`requirements.sdk.graph.edges`
// {test}`requirements.sdk.graph.validation.root_inputs`
// {test}`requirements.sdk.graph.outgoing_edges`
// {test}`requirements.sdk.graph.fingerprinting.snapshot`
#[test]
fn labeled_selection_records_all_edges_without_reading_targets() {
    let graph = Memory::default();
    let ctx = ValidationContext::new(&graph, 1);
    assert_eq!(ctx.node_id(), &1);
    assert!(graph.state.lock().unwrap().reads.is_empty());
    assert!(ctx.successors(&7).unwrap().is_empty());
    let record = ctx.inspection().finish(
        "v1".into(),
        Judgment {
            result: true,
            rationale: "ok".into(),
            metadata: (),
        },
    );
    assert!(record_is_current(&graph, "v1", &record).unwrap());
    graph
        .state
        .lock()
        .unwrap()
        .edges
        .insert(1, vec![(8, 2), (7, 5), (7, 3), (7, 5)]);
    assert!(!record_is_current(&graph, "v1", &record).unwrap());
    let selected = ctx.successors(&7).unwrap();
    assert_eq!(
        selected.iter().map(|edge| edge.target).collect::<Vec<_>>(),
        vec![5, 3, 5]
    );
    assert!(graph.state.lock().unwrap().reads.is_empty());
    let edge = selected[0].clone();
    assert_eq!((edge.label, edge.target), (7, 5));
    assert!(format!("{edge:?}").contains("target: 5"));
    let selected_ctx = ValidationContext::new(&graph, 1);
    selected_ctx.successors(&7).unwrap();
    let selected_record = selected_ctx.inspection().finish(
        "v1".into(),
        Judgment {
            result: false,
            rationale: "selection".into(),
            metadata: (),
        },
    );
    assert!(record_is_current(&graph, "v1", &selected_record).unwrap());
    let original = graph.state.lock().unwrap().edges[&1].clone();
    let original_fingerprint = graph.outgoing_edges(&1).unwrap().fingerprint;
    graph.state.lock().unwrap().edges.get_mut(&1).unwrap()[1].0 = 9;
    assert_ne!(
        graph.outgoing_edges(&1).unwrap().fingerprint,
        original_fingerprint
    );
    assert!(!record_is_current(&graph, "v1", &selected_record).unwrap());
    graph
        .state
        .lock()
        .unwrap()
        .edges
        .insert(1, original.clone());
    graph.state.lock().unwrap().edges.get_mut(&1).unwrap().pop();
    assert_ne!(
        graph.outgoing_edges(&1).unwrap().fingerprint,
        original_fingerprint
    );
    assert!(!record_is_current(&graph, "v1", &selected_record).unwrap());
    graph.state.lock().unwrap().edges.insert(1, original);
    assert!(record_is_current(&graph, "v1", &selected_record).unwrap());
    graph.state.lock().unwrap().edges.get_mut(&1).unwrap()[0].1 = 99;
    assert!(!record_is_current(&graph, "v1", &selected_record).unwrap());
    graph.state.lock().unwrap().fail_edges = Some(1);
    assert_eq!(
        record_is_current(&graph, "v1", &selected_record),
        Err("edges")
    );
    graph.state.lock().unwrap().fail_edges = None;
    let old = graph.outgoing_edges(&1).unwrap().fingerprint;
    graph
        .state
        .lock()
        .unwrap()
        .edges
        .get_mut(&1)
        .unwrap()
        .reverse();
    assert_ne!(old, graph.outgoing_edges(&1).unwrap().fingerprint);
}

// {test}`requirements.sdk.graph.validation.validate_node`
// {test}`requirements.sdk.graph.validation.check_node`
// {test}`requirements.sdk.graph.agent-calls.procedures`
// {test}`requirements.sdk.graph.validation.check_procedure`
// {test}`requirements.sdk.graph.validation.deterministic_procedures`
// {test}`requirements.sdk.graph.validation.procedure_identity`
#[tokio::test]
async fn runner_reuses_negative_judgments_and_records_only_callback_observations() {
    let graph = Memory::default();
    let validator = Validator::new(&graph, |_| Ok("v1".into()));
    assert!(!validator.validate_node(&9).await.unwrap().result);
    assert!(!validator.validate_node(&9).await.unwrap().result);
    {
        let state = graph.state.lock().unwrap();
        assert_eq!(state.started, vec![9]);
        assert!(state.reads.is_empty());
        assert!(state.selections.is_empty());
        assert!(state.records[&9].dependencies.is_empty());
    }
    assert!(
        matches!(validator.check_node(&9).await.unwrap(), EvidenceState::Current(j) if !j.result)
    );
    assert!(matches!(
        validator.check_node(&8).await.unwrap(),
        EvidenceState::Missing
    ));
    let v2 = Validator::new(&graph, |_| Ok("v2".into()));
    assert!(matches!(
        v2.check_node(&9).await.unwrap(),
        EvidenceState::Stale
    ));
    v2.validate_node(&9).await.unwrap();
    assert_eq!(graph.state.lock().unwrap().records[&9].procedure, "v2");
    graph
        .state
        .lock()
        .unwrap()
        .records
        .get_mut(&9)
        .unwrap()
        .procedure = "*".into();
    assert!(matches!(v2.check_node(&9).await.unwrap(), EvidenceState::Current(j) if !j.result));
}

// {test}`requirements.sdk.graph.validation.validate_node.errors`
// {test}`requirements.sdk.graph.validation.check_node`
// {test}`requirements.sdk.graph.validation.procedure_identity`
// {test}`requirements.sdk.graph.validation.judgment`
#[tokio::test]
async fn resolution_storage_and_callback_errors_are_not_judgments() {
    let graph = Memory::default();
    let bad_version = Validator::new(&graph, |_| Err("version"));
    assert!(matches!(
        bad_version.validate_node(&1).await,
        Err(ValidationError::Graph("version"))
    ));
    assert!(graph.state.lock().unwrap().started.is_empty());
    let validator = Validator::new(&graph, |_| Ok("v1".into()));
    graph.state.lock().unwrap().fail_load = true;
    assert!(matches!(
        validator.validate_node(&1).await,
        Err(ValidationError::Graph("load"))
    ));
    graph.state.lock().unwrap().fail_load = false;
    graph.state.lock().unwrap().fail_store = true;
    assert!(matches!(
        validator.validate_node(&1).await,
        Err(ValidationError::Graph("store"))
    ));
    assert!(graph.state.lock().unwrap().records.is_empty());
    let graph = Memory {
        read_in_callback: true,
        ..Memory::default()
    };
    let validator = Validator::new(&graph, |_| Ok("v1".into()));
    assert!(matches!(
        validator.validate_node(&1).await,
        Err(ValidationError::Graph("missing"))
    ));
    assert!(graph.state.lock().unwrap().records.is_empty());
}

// {test}`requirements.sdk.graph.agent-calls.judge`
// {test}`requirements.sdk.graph.agent-calls.author_procedure`
// {test}`requirements.sdk.graph.agent-calls.model_capability`
// {test}`requirements.sdk.graph.agent-calls.judge.node_contents`
#[tokio::test]
async fn dropped_builders_and_missing_agents_do_not_resolve_queued_inputs() {
    let graph = Memory::default();
    graph.state.lock().unwrap().fail_edges = Some(1);
    let ctx = ValidationContext::new(&graph, 1);
    drop(
        ctx.ask::<Answer>()
            .current_node_contents()
            .successors_content(7),
    );
    assert!(ctx.inspection().dependencies().is_empty());
    assert!(matches!(
        ctx.ask::<Answer>()
            .successors_content(7)
            .current_node_contents()
            .await,
        Err(ValidationError::NoAgentConfigured)
    ));
    let state = graph.state.lock().unwrap();
    assert!(state.reads.is_empty());
    assert!(state.selections.is_empty());
}

// {test}`requirements.sdk.graph.agent-calls.judge`
// {test}`requirements.sdk.graph.agent-calls.judge.node_contents`
// {test}`requirements.sdk.graph.agent-calls.judge.phases`
// {test}`requirements.sdk.graph.agent-calls.judge.rendering`
// {test}`requirements.sdk.graph.agent-calls.judge.submission`
// {test}`requirements.sdk.graph.fingerprinting.snapshot`
#[tokio::test]
async fn explicit_insertions_preserve_order_repetition_and_selection_snapshots() {
    let graph = Memory::default();
    {
        let mut state = graph.state.lock().unwrap();
        state.contents.insert(1, "subject".into());
        state.contents.insert(2, "test".into());
        state.contents.insert(3, "implementation".into());
        state.edges.insert(1, vec![(7, 3), (8, 99), (7, 2), (7, 3)]);
    }
    let agent = MockAgent::default();
    let ctx = ValidationContext::with_agent(&graph, 1, &agent, &Renderer);
    let future = ctx
        .ask::<Answer>()
        .context(String::from("question"))
        .current_node_contents()
        .successors_content(7)
        .current_node_contents()
        .into_future();
    fn assert_send<T: Send>(_: &T) {}
    assert_send(&future);
    let answer = future.await.unwrap();
    assert_eq!(answer.n, 7);
    let requests = agent.requests.lock().unwrap();
    assert_eq!(requests[0].0, "question");
    assert_eq!(
        requests[0].2,
        vec![
            (1, "subject".into()),
            (3, "implementation".into()),
            (2, "test".into()),
            (3, "implementation".into()),
            (1, "subject".into())
        ]
    );
    assert_eq!(requests[0].3["properties"]["n"]["type"], "integer");
    assert_eq!(graph.state.lock().unwrap().reads, vec![1, 3, 2, 3, 1]);
    assert_eq!(
        ctx.inspection().dependencies(),
        vec![
            Dependency::OutgoingEdges(1, fp("[(7, 3), (8, 99), (7, 2), (7, 3)]")),
            Dependency::Node(1, fp("subject")),
            Dependency::Node(3, fp("implementation")),
            Dependency::Node(2, fp("test")),
        ]
    );
}

struct CountingContext {
    renders: Rc<Cell<usize>>,
    text: String,
}
impl Context for CountingContext {
    fn contents(&self) -> Cow<'_, String> {
        self.renders.set(self.renders.get() + 1);
        Cow::Borrowed(&self.text)
    }
}

// {test}`requirements.sdk.graph.agent-calls.judge`
// {test}`requirements.sdk.graph.agent-calls.agent_context`
// {test}`requirements.sdk.graph.agent-calls.judge.caching`
// {test}`requirements.sdk.domain`
// {test}`requirements.sdk.graph.agent-calls.author_procedure`
#[tokio::test]
async fn contexts_render_once_allow_non_send_values_and_hash_exact_prefix_text() {
    let graph = Memory::default();
    let agent = MockAgent::default();
    let ctx = ValidationContext::with_agent(&graph, 1, &agent, &Renderer);
    let renders = Rc::new(Cell::new(0));
    let mut context = CountingContext {
        renders: renders.clone(),
        text: "héllo".into(),
    };
    let builder = ctx
        .ask::<Answer>()
        .context(&context)
        .context((String::from("policy"), String::from("question")));
    context.text = "changed".into();
    assert_eq!(renders.get(), 1);
    fn assert_send<T: Send>(_: &T) {}
    let future = builder.into_future();
    assert_send(&future);
    future.await.unwrap();
    ctx.ask::<Answer>().await.unwrap();
    ctx.ask::<Answer>()
        .context(Vec::<String>::new())
        .await
        .unwrap();
    let requests = agent.requests.lock().unwrap();
    assert_eq!(requests[0].0, "héllo\n\npolicy\n\nquestion");
    assert_eq!(requests[0].1, context_fingerprint(&requests[0].0));
    assert_eq!(requests[1].1, requests[2].1);
    assert_ne!(context_fingerprint("a\n\nb"), context_fingerprint("b\n\na"));
    assert_eq!(
        (
            vec![String::from("a"), String::from("a")],
            String::from("b")
        )
            .contents()
            .as_ref(),
        "a\n\na\n\nb"
    );
    assert_eq!(
        (
            String::from("1"),
            String::from("2"),
            String::from("3"),
            String::from("4"),
            String::from("5"),
            String::from("6"),
            String::from("7"),
            String::from("8"),
            String::from("9"),
            String::from("10"),
            String::from("11"),
            String::from("12")
        )
            .contents()
            .as_ref(),
        "1\n\n2\n\n3\n\n4\n\n5\n\n6\n\n7\n\n8\n\n9\n\n10\n\n11\n\n12"
    );
}

// {test}`requirements.sdk.graph.agent-calls.agent_context`
#[test]
fn every_heterogeneous_tuple_arity_renders_elements_in_order() {
    let custom = CountingContext {
        renders: Rc::new(Cell::new(0)),
        text: "custom".into(),
    };
    assert_eq!(
        (&custom, String::from("2")).contents().as_ref(),
        "custom\n\n2"
    );
    assert_eq!(
        (&custom, String::from("2"), String::from("3"))
            .contents()
            .as_ref(),
        "custom\n\n2\n\n3"
    );
    assert_eq!(
        (
            &custom,
            String::from("2"),
            String::from("3"),
            String::from("4")
        )
            .contents()
            .as_ref(),
        "custom\n\n2\n\n3\n\n4"
    );
    assert_eq!(
        (
            &custom,
            String::from("2"),
            String::from("3"),
            String::from("4"),
            String::from("5")
        )
            .contents()
            .as_ref(),
        "custom\n\n2\n\n3\n\n4\n\n5"
    );
    assert_eq!(
        (
            &custom,
            String::from("2"),
            String::from("3"),
            String::from("4"),
            String::from("5"),
            String::from("6")
        )
            .contents()
            .as_ref(),
        "custom\n\n2\n\n3\n\n4\n\n5\n\n6"
    );
    assert_eq!(
        (
            &custom,
            String::from("2"),
            String::from("3"),
            String::from("4"),
            String::from("5"),
            String::from("6"),
            String::from("7")
        )
            .contents()
            .as_ref(),
        "custom\n\n2\n\n3\n\n4\n\n5\n\n6\n\n7"
    );
    assert_eq!(
        (
            &custom,
            String::from("2"),
            String::from("3"),
            String::from("4"),
            String::from("5"),
            String::from("6"),
            String::from("7"),
            String::from("8")
        )
            .contents()
            .as_ref(),
        "custom\n\n2\n\n3\n\n4\n\n5\n\n6\n\n7\n\n8"
    );
    assert_eq!(
        (
            &custom,
            String::from("2"),
            String::from("3"),
            String::from("4"),
            String::from("5"),
            String::from("6"),
            String::from("7"),
            String::from("8"),
            String::from("9")
        )
            .contents()
            .as_ref(),
        "custom\n\n2\n\n3\n\n4\n\n5\n\n6\n\n7\n\n8\n\n9"
    );
    assert_eq!(
        (
            &custom,
            String::from("2"),
            String::from("3"),
            String::from("4"),
            String::from("5"),
            String::from("6"),
            String::from("7"),
            String::from("8"),
            String::from("9"),
            String::from("10")
        )
            .contents()
            .as_ref(),
        "custom\n\n2\n\n3\n\n4\n\n5\n\n6\n\n7\n\n8\n\n9\n\n10"
    );
    assert_eq!(
        (
            &custom,
            String::from("2"),
            String::from("3"),
            String::from("4"),
            String::from("5"),
            String::from("6"),
            String::from("7"),
            String::from("8"),
            String::from("9"),
            String::from("10"),
            String::from("11")
        )
            .contents()
            .as_ref(),
        "custom\n\n2\n\n3\n\n4\n\n5\n\n6\n\n7\n\n8\n\n9\n\n10\n\n11"
    );
    assert_eq!(
        (
            &custom,
            String::from("2"),
            String::from("3"),
            String::from("4"),
            String::from("5"),
            String::from("6"),
            String::from("7"),
            String::from("8"),
            String::from("9"),
            String::from("10"),
            String::from("11"),
            String::from("12")
        )
            .contents()
            .as_ref(),
        "custom\n\n2\n\n3\n\n4\n\n5\n\n6\n\n7\n\n8\n\n9\n\n10\n\n11\n\n12"
    );
    assert_eq!(custom.renders.get(), 11);
    let empty: &[String] = &[];
    assert_eq!(empty.contents().as_ref(), "");
}

// {test}`requirements.sdk.graph.agent-calls.author_procedure`
// {test}`requirements.sdk.graph.agent-calls.procedures`
// {test}`requirements.sdk.domain`
#[tokio::test]
async fn shared_context_allows_independent_concurrent_agent_calls() {
    let graph = Memory::default();
    graph
        .state
        .lock()
        .unwrap()
        .contents
        .insert(1, "subject".into());
    let agent = MockAgent {
        barrier: Some(Arc::new(tokio::sync::Barrier::new(2))),
        prefix_result: true,
        ..MockAgent::default()
    };
    let ctx = ValidationContext::with_agent(&graph, 1, &agent, &Renderer);
    let (first, second) = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        futures::join!(
            ctx.ask::<Answer>().current_node_contents().into_future(),
            ctx.ask::<Answer>()
                .context(String::from("independent"))
                .into_future()
        )
    })
    .await
    .unwrap();
    assert_eq!(first.unwrap().n, 7);
    assert_eq!(second.unwrap().n, 11);
    assert_eq!(agent.requests.lock().unwrap().len(), 2);
    assert_eq!(
        ctx.inspection().dependencies(),
        vec![Dependency::Node(1, fp("subject"))]
    );
}

// {test}`requirements.sdk.domain`
// {test}`requirements.sdk.graph.agent-calls.author_procedure`
// {test}`requirements.sdk.graph.evidence.node_inputs`
#[tokio::test]
async fn later_lighter_agent_calls_preserve_the_contexts_existing_trace() {
    let graph = Memory::default();
    graph
        .state
        .lock()
        .unwrap()
        .contents
        .insert(1, "retained".into());
    let agent = MockAgent::default();
    let ctx = ValidationContext::with_agent(&graph, 1, &agent, &Renderer);
    fn assert_send_sync<T: Send + Sync>(_: &T) {}
    assert_send_sync(&ctx);
    let _: Answer = ctx
        .ask()
        .current_node_contents()
        .successors_content(7)
        .await
        .unwrap();
    let retained = ctx.inspection().dependencies();
    assert_eq!(
        retained,
        vec![
            Dependency::OutgoingEdges(1, fp("[]")),
            Dependency::Node(1, fp("retained")),
        ]
    );
    let _: Answer = ctx.ask().context("lighter".to_owned()).await.unwrap();
    assert_eq!(ctx.inspection().dependencies(), retained);
    assert!(agent.requests.lock().unwrap()[1].2.is_empty());
    assert_eq!(
        ctx.inspection()
            .finish(
                "v1".into(),
                Judgment {
                    result: true,
                    rationale: "retained".into(),
                    metadata: (),
                }
            )
            .dependencies,
        retained
    );
}

// {test}`requirements.sdk.graph.traversal.validation_parallelism`
// {test}`requirements.sdk.graph.traversal.validation_cycles`
// {test}`requirements.sdk.domain`
#[tokio::test]
async fn cyclic_ready_work_is_bounded_by_each_configured_parallelism_limit() {
    for limit in [1, 2, 3] {
        let graph = Memory {
            read_in_callback: true,
            edges_in_callback: true,
            delay: true,
            ..Memory::default()
        };
        {
            let mut state = graph.state.lock().unwrap();
            for node in 1..=6 {
                state.contents.insert(node, format!("node {node}"));
                state.edges.insert(node, vec![(1, node % 6 + 1)]);
            }
        }
        let validator = Validator::new(&graph, |_| Ok("v1".into()));
        let outcome = validator
            .validate_transitive(
                &1,
                TraversalOptions {
                    concurrency: NonZeroUsize::new(limit).unwrap(),
                },
            )
            .await;
        assert!(outcome.complete);
        assert_eq!(outcome.nodes.len(), 6);
        assert_eq!(graph.peak.load(Ordering::SeqCst), limit);
        let state = graph.state.lock().unwrap();
        let mut started = state.started.clone();
        started.sort();
        assert_eq!(started, vec![1, 2, 3, 4, 5, 6]);
        for (node, record) in &state.records {
            assert_eq!(
                record.dependencies,
                vec![
                    Dependency::Node(*node, fp(format!("node {node}"))),
                    Dependency::OutgoingEdges(*node, fp(format!("{:?}", state.edges[node]))),
                ]
            );
        }
    }
}

// {test}`requirements.sdk.graph.traversal.validation_parallelism`
// {test}`requirements.sdk.graph.traversal.validate_transitive`
#[tokio::test]
async fn independent_acyclic_components_share_the_validation_limit() {
    let graph = Memory {
        delay: true,
        ..Memory::default()
    };
    graph
        .state
        .lock()
        .unwrap()
        .edges
        .insert(1, vec![(1, 2), (1, 3), (1, 4)]);
    let outcome = Validator::new(&graph, |_| Ok("v1".into()))
        .validate_transitive(
            &1,
            TraversalOptions {
                concurrency: NonZeroUsize::new(2).unwrap(),
            },
        )
        .await;
    assert!(outcome.complete);
    assert_eq!(outcome.nodes.len(), 4);
    assert_eq!(graph.peak.load(Ordering::SeqCst), 2);
    let state = graph.state.lock().unwrap();
    assert_eq!(state.finished.last(), Some(&1));
    let mut leaves = state.finished[..3].to_vec();
    leaves.sort();
    assert_eq!(leaves, vec![2, 3, 4]);
}

// {test}`requirements.sdk.graph.agent-calls.judge.submission`
#[tokio::test]
async fn invalid_typed_submission_is_an_agent_error() {
    let graph = Memory::default();
    let agent = MockAgent {
        invalid: true,
        ..MockAgent::default()
    };
    let ctx = ValidationContext::with_agent(&graph, 1, &agent, &Renderer);
    assert!(matches!(
        ctx.ask::<Answer>().await,
        Err(ValidationError::Agent(_))
    ));
}

// {test}`requirements.sdk.graph.traversal.validate_transitive`
// {test}`requirements.sdk.graph.traversal.validation_cycles`
// {test}`requirements.sdk.graph.traversal.validation_parallelism`
// {test}`requirements.sdk.domain`
// {test}`requirements.sdk.graph.outgoing_edges`
#[tokio::test]
async fn cycles_shared_targets_negative_judgments_and_parallelism() {
    let graph = Memory {
        read_in_callback: true,
        edges_in_callback: true,
        delay: true,
        ..Memory::default()
    };
    {
        let mut state = graph.state.lock().unwrap();
        state.edges.insert(1, vec![(1, 2), (1, 3), (1, 2)]);
        state.edges.insert(2, vec![(1, 3), (1, 9)]);
        state.edges.insert(3, vec![(1, 2), (1, 9)]);
        state.edges.insert(9, vec![(1, 9)]);
        for node in [1, 2, 3, 9] {
            state.contents.insert(node, format!("content {node}"));
        }
    }
    let validator = Validator::new(&graph, |_| Ok("v1".into()));
    let results = validator
        .validate_transitive(
            &1,
            TraversalOptions {
                concurrency: NonZeroUsize::new(2).unwrap(),
            },
        )
        .await;
    assert!(results.complete);
    assert_eq!(results.nodes.len(), 4);
    assert!(
        results
            .nodes
            .iter()
            .any(|(node, judgment)| *node == 9 && !judgment.as_ref().unwrap().result)
    );
    let state = graph.state.lock().unwrap();
    assert_eq!(state.started.len(), 4);
    assert_eq!(state.finished[0], 9);
    assert_eq!(state.finished[3], 1);
    assert_eq!(graph.peak.load(Ordering::SeqCst), 2);
    for (node, record) in &state.records {
        assert_eq!(
            record.dependencies,
            vec![
                Dependency::Node(*node, fp(format!("content {node}"))),
                Dependency::OutgoingEdges(*node, fp(format!("{:?}", state.edges[node]))),
            ]
        );
    }
}

// {test}`requirements.sdk.graph.traversal.validate_transitive`
// {test}`requirements.sdk.graph.traversal.validation_cycles`
#[tokio::test]
async fn discovery_errors_report_incomplete_traversal_and_do_not_deadlock() {
    let graph = Memory::default();
    graph
        .state
        .lock()
        .unwrap()
        .edges
        .insert(1, vec![(1, 2), (1, 3)]);
    graph.state.lock().unwrap().fail_edges = Some(2);
    let validator = Validator::new(&graph, |_| Ok("v1".into()));
    let results = validator
        .validate_transitive(
            &1,
            TraversalOptions {
                concurrency: NonZeroUsize::new(1).unwrap(),
            },
        )
        .await;
    assert!(!results.complete);
    assert_eq!(results.nodes.len(), 3);
    assert!(
        results
            .nodes
            .iter()
            .any(|(node, result)| *node == 2
                && matches!(result, Err(ValidationError::Graph("edges"))))
    );
    assert_eq!(graph.state.lock().unwrap().finished, vec![3, 1]);
    assert_eq!(graph.peak.load(Ordering::SeqCst), 1);
}

// {test}`requirements.sdk.graph.traversal.validate_transitive`
// {test}`requirements.sdk.graph.validation.root_inputs`
// {test}`requirements.sdk.graph.outgoing_edges`
// {test}`requirements.sdk.graph.fingerprinting.snapshot`
#[tokio::test]
async fn traversal_keeps_discovered_edges_and_reuse_does_not_skip_children() {
    let graph = Memory {
        edges_in_callback: true,
        ..Memory::default()
    };
    graph.state.lock().unwrap().edges.insert(1, vec![(1, 2)]);
    let validator = Validator::new(&graph, |_| Ok("v1".into()));
    validator
        .validate_transitive(&1, TraversalOptions::default())
        .await;
    assert_eq!(graph.state.lock().unwrap().selections, vec![1, 2]);
    let record = graph.state.lock().unwrap().records[&1].clone();
    assert_eq!(
        record.dependencies,
        vec![Dependency::OutgoingEdges(1, fp("[(1, 2)]"))]
    );
    graph.state.lock().unwrap().records.remove(&2);
    let results = validator
        .validate_transitive(&1, TraversalOptions::default())
        .await;
    assert_eq!(results.nodes.len(), 2);
    assert_eq!(graph.state.lock().unwrap().started, vec![2, 1, 2]);
}

// {test}`requirements.sdk.graph.agent-calls.judge.caching`
// {test}`requirements.sdk.graph.agent-calls.agent_context`
#[test]
fn prefix_hashes_match_known_utf8_sha256_without_normalization() {
    assert_eq!(
        context_fingerprint(""),
        Fingerprint {
            scheme: "context-prefix-sha256-v1".into(),
            value: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
        }
    );
    assert_eq!(
        context_fingerprint("héllo").value,
        "3c48591d8d098a4538f5e013dfcf406e948eac4d3277b10bf614e295d6068179"
    );
    assert_eq!(context_fingerprint("héllo"), context_fingerprint("héllo"));
    assert_ne!(context_fingerprint("é"), context_fingerprint("e\u{301}"));
    assert_ne!(context_fingerprint("a b"), context_fingerprint("a  b"));
}

// {test}`requirements.sdk.graph.validation.check_procedure`
#[test]
fn agent_errors_expose_the_agreed_string_payload() {
    use std::error::Error;
    let error = ValidationError::<std::io::Error>::Agent("provider detail".to_owned());
    assert_eq!(error.to_string(), "agent: provider detail");
    assert!(error.source().is_none());
    let ValidationError::Agent(message) = error else {
        panic!("agent error variant")
    };
    let message: String = message;
    assert_eq!(message, "provider detail");
    let graph = ValidationError::Graph(std::io::Error::other("graph detail"));
    assert_eq!(graph.source().unwrap().to_string(), "graph detail");
}

struct FailingRenderer;
impl AgentInputRenderer<Memory> for FailingRenderer {
    fn render(&self, _: &AgentInput<'_, Memory>) -> Result<String, &'static str> {
        Err("render")
    }
}

// {test}`requirements.sdk.graph.agent-calls.judge`
// {test}`requirements.sdk.graph.agent-calls.judge.node_contents`
// {test}`requirements.sdk.graph.agent-calls.judge.rendering`
// {test}`requirements.sdk.graph.agent-calls.author_procedure`
#[tokio::test]
async fn queued_reads_selection_and_rendering_fail_before_agent_invocation() {
    let graph = Memory::default();
    let agent = MockAgent::default();
    let ctx = ValidationContext::with_agent(&graph, 1, &agent, &Renderer);
    drop(ctx.ask::<Answer>().current_node_contents());
    assert!(agent.requests.lock().unwrap().is_empty());
    for edges in [vec![], vec![(8, 99)]] {
        graph.state.lock().unwrap().edges.insert(1, edges.clone());
        let ctx = ValidationContext::with_agent(&graph, 1, &agent, &Renderer);
        ctx.ask::<Answer>().successors_content(7).await.unwrap();
        assert!(graph.state.lock().unwrap().reads.is_empty());
        assert_eq!(
            ctx.inspection().dependencies(),
            vec![Dependency::OutgoingEdges(1, fp(format!("{edges:?}")))]
        );
    }
    let initial_calls = agent.requests.lock().unwrap().len();
    graph.state.lock().unwrap().fail_read = true;
    assert!(matches!(
        ctx.ask::<Answer>().current_node_contents().await,
        Err(ValidationError::Graph("read"))
    ));
    graph.state.lock().unwrap().fail_read = false;
    graph.state.lock().unwrap().fail_edges = Some(1);
    assert!(matches!(
        ctx.ask::<Answer>().successors_content(7).await,
        Err(ValidationError::Graph("edges"))
    ));
    graph.state.lock().unwrap().fail_edges = None;
    graph
        .state
        .lock()
        .unwrap()
        .contents
        .insert(1, "root".into());
    let ctx = ValidationContext::with_agent(&graph, 1, &agent, &FailingRenderer);
    assert!(matches!(
        ctx.ask::<Answer>().current_node_contents().await,
        Err(ValidationError::Graph("render"))
    ));
    assert!(ctx.inspection().dependencies().is_empty());
    assert_eq!(agent.requests.lock().unwrap().len(), initial_calls);
}

// {test}`requirements.sdk.graph.traversal.validation_cycles`
// {test}`requirements.sdk.graph.traversal.validate_transitive`
#[tokio::test]
async fn callback_errors_inside_cycles_release_dependents() {
    let graph = Memory {
        read_in_callback: true,
        ..Memory::default()
    };
    {
        let mut state = graph.state.lock().unwrap();
        state.edges.insert(1, vec![(1, 2)]);
        state.edges.insert(2, vec![(1, 3)]);
        state.edges.insert(3, vec![(1, 2)]);
        state.contents.insert(1, "source".into());
        state.contents.insert(3, "peer".into());
    }
    let validator = Validator::new(&graph, |_| Ok("v1".into()));
    let results = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        validator.validate_transitive(&1, TraversalOptions::default()),
    )
    .await
    .unwrap();
    assert!(results.complete);
    assert_eq!(results.nodes.len(), 3);
    assert!(results.nodes.iter().any(
        |(node, result)| *node == 2 && matches!(result, Err(ValidationError::Graph("missing")))
    ));
    let state = graph.state.lock().unwrap();
    assert!(state.records.contains_key(&1) && state.records.contains_key(&3));
    assert!(!state.records.contains_key(&2));
    assert_eq!(state.finished.last(), Some(&1));
}

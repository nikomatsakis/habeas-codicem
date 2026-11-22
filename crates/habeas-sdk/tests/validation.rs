use futures::future::BoxFuture;
use habeas_sdk::{
    graph::*,
    procedure::{Agent, AgentRequest, Context, context_fingerprint},
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    borrow::Cow,
    cell::Cell,
    collections::BTreeMap,
    future::IntoFuture,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

// The test application chooses its own completed-answer format.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Assessment<M> {
    result: bool,
    rationale: String,
    metadata: M,
}

#[derive(Default)]
struct State {
    contents: BTreeMap<u8, String>,
    edges: BTreeMap<u8, Vec<(u8, u8)>>,
    records: BTreeMap<u8, Record<u8, u8>>,
    reads: Vec<u8>,
    selections: Vec<u8>,
    started: Vec<u8>,
    finished: Vec<u8>,
    fail_read: bool,
    fail_format: bool,
    fail_load: bool,
    fail_store: bool,
    fail_edges: Option<u8>,
}

#[derive(Clone, Default)]
struct Memory {
    state: Arc<Mutex<State>>,
    read_in_callback: bool,
    edges_in_callback: bool,
    ask_in_callback: bool,
    delay: bool,
    active: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
}

fn fp(value: impl Into<String>) -> Fingerprint {
    Fingerprint {
        scheme: "test-v1".into(),
        value: value.into(),
    }
}

impl Graph for Memory {
    type NodeId = u8;
    type EdgeLabel = u8;

    type Error = &'static str;

    fn node_path(&self, node: &u8) -> NodePath {
        NodePath::new(vec![node.to_string()])
    }
    fn outgoing_edges(&self, node: &u8, label: u8) -> Result<Vec<u8>, Self::Error> {
        let mut state = self.state.lock().unwrap();
        state.selections.push(*node);
        if state.fail_edges == Some(*node) {
            return Err("edges");
        }
        Ok(state
            .edges
            .get(node)
            .into_iter()
            .flatten()
            .filter(|(kind, _)| *kind == label)
            .map(|(_, target)| *target)
            .collect())
    }
}
impl Contents for Memory {
    type NodeContent = String;

    fn node_content(&self, node: &u8) -> Result<String, Self::Error> {
        let mut state = self.state.lock().unwrap();
        state.reads.push(*node);
        if state.fail_read {
            return Err("read");
        }
        state.contents.get(node).cloned().ok_or("missing")
    }

    fn fingerprint(&self, _: &u8, content: &String) -> Fingerprint {
        fp(content.clone())
    }

    fn format_node_content(&self, _: &u8, contents: &String) -> Result<String, &'static str> {
        if self.state.lock().unwrap().fail_format {
            Err("render")
        } else {
            Ok(contents.clone())
        }
    }
}
impl Storage for Memory {
    async fn load_evidence(
        &self,
        node: &u8,
        _: &CriterionPath,
    ) -> Result<Option<Record<u8, u8>>, Self::Error> {
        let state = self.state.lock().unwrap();
        if state.fail_load {
            return Err("load");
        }
        Ok(state.records.get(node).cloned())
    }
    async fn store_evidence(
        &self,
        node: &u8,
        _: &CriterionPath,
        record: &Record<u8, u8>,
    ) -> Result<(), Self::Error> {
        let mut state = self.state.lock().unwrap();
        if state.fail_store {
            return Err("store");
        }
        state.records.insert(*node, record.clone());
        Ok(())
    }
}
struct Review {
    node: u8,
    revision: &'static str,
}
impl Criterion<Memory> for Review {
    type Ok = Assessment<()>;
    fn source_node_id(&self) -> u8 {
        self.node
    }
    fn criterion_path(&self) -> CriterionPath {
        CriterionPath::new(vec![])
    }
    fn revision(&self) -> String {
        self.revision.into()
    }
    async fn execute(
        self,
        cx: &mut ValidationContext<'_, Memory>,
    ) -> Result<Self::Ok, ValidationError<&'static str>> {
        cx.inspection().graph().run_review(cx).await
    }
}

impl Memory {
    async fn run_review<'a, 'd>(
        &'a self,
        ctx: &'a ValidationContext<'d, Self>,
    ) -> Result<Assessment<()>, ValidationError<&'static str>>
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
            let first: Answer = ctx.ask().contents_of_current_node().await?;
            let second: Answer = ctx
                .ask()
                .context(format!("Check intermediate result {}", first.n))
                .await?;
            assert_eq!(second.n, first.n);
        }
        self.active.fetch_sub(1, Ordering::SeqCst);
        self.state.lock().unwrap().finished.push(node);
        Ok(Assessment {
            result: node != 9,
            rationale: format!("review {node}"),
            metadata: (),
        })
    }
}

// {test}`requirements.sdk.graph.evidence.checking_records`
// {test}`requirements.sdk.graph.validators.validate_node`
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
                revision: "v1".into(),
                dependencies: vec![Dependency::Node(2, fp("old"))],
                output: serde_json::to_value(Assessment {
                    result: true,
                    rationale: "historical".into(),
                    metadata: (),
                })
                .unwrap(),
            },
        );
    }
    let validator = Validator::new(graph.clone());
    let review = || Review {
        node: 1,
        revision: "v1",
    };
    let old = validator.load_evidence(&review()).await.unwrap().unwrap();
    assert!(!validator.check_inputs(&old).unwrap());
    let value = validator.verify(review()).await.unwrap();
    validator.store_evidence(&value).await.unwrap();
    let state = graph.state.lock().unwrap();
    assert_eq!(state.reads, vec![2, 1]);
    assert_eq!(
        state.records[&1].dependencies,
        vec![Dependency::Node(1, fp("root"))]
    );
}

// {test}`requirements.sdk.graph.agent-calls.model_capability`
// {test}`requirements.sdk.graph.agent-calls.procedures`
// {test}`requirements.sdk.graph.evidence.store_evidence`
// {test}`requirements.sdk.graph.validators.validate_node.errors`
// {test}`requirements.sdk.graph.agent-harness.with_agent`
// {test}`requirements.sdk.graph.agent-calls.judge.rendering`
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
    let validator = Validator::new(graph.clone());
    assert!(matches!(
        validator
            .verify(Review {
                node: 1,
                revision: "v1"
            })
            .await,
        Err(ValidationError::NoAgentConfigured)
    ));
    {
        let state = graph.state.lock().unwrap();
        assert!(state.records.is_empty());
        assert!(state.reads.is_empty());
    }
    let agent = Arc::new(MockAgent::default());
    let validator = validator.with_agent(agent.clone());
    let value = validator
        .verify(Review {
            node: 1,
            revision: "v1",
        })
        .await
        .unwrap();
    assert!(value.output().result);
    validator.store_evidence(&value).await.unwrap();
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
// {test}`requirements.sdk.graph.context-tools.read_node`
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
    let record = inspection
        .finish(
            "v1".into(),
            Assessment {
                result: true,
                rationale: "ok".into(),
                metadata: (),
            },
        )
        .unwrap();
    assert_eq!(
        record.dependencies,
        vec![
            Dependency::Node(1, fp("before")),
            Dependency::Node(1, fp("after"))
        ]
    );
    assert!(!record_is_current(&graph, "v1", &record).unwrap());
}

// {test}`requirements.sdk.graph.evidence.checking_records`
#[test]
fn a_matching_dependency_does_not_hide_a_later_changed_dependency() {
    for edge_dependency in [false, true] {
        let graph = Memory::default();
        {
            let mut state = graph.state.lock().unwrap();
            state
                .contents
                .extend([(1, "first".into()), (2, "second".into())]);
            state.edges.insert(1, vec![(7, 2)]);
        }
        let later = if edge_dependency {
            Dependency::OutgoingEdges(1, 7, edge_fingerprint(&graph, &[2]))
        } else {
            Dependency::Node(2, fp("second"))
        };
        let record = Record {
            revision: "v1".into(),
            dependencies: vec![Dependency::Node(1, fp("first")), later],
            output: json!(false),
        };
        assert!(record_is_current(&graph, "v1", &record).unwrap());
        {
            let mut state = graph.state.lock().unwrap();
            if edge_dependency {
                state.edges.get_mut(&1).unwrap().push((7, 3));
            } else {
                state.contents.insert(2, "changed".into());
            }
            assert_eq!(state.contents[&1], "first");
        }
        assert!(!record_is_current(&graph, "v1", &record).unwrap());
    }
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
        revision: "*".into(),
        dependencies: vec![Dependency::Node(1, fp("same"))],
        output: serde_json::to_value(Assessment {
            result: false,
            rationale: "negative".into(),
            metadata: (),
        })
        .unwrap(),
    };
    assert!(record_is_current(&graph, "v42", &record).unwrap());
    assert!(!record.output["result"].as_bool().unwrap());
    record.revision = "v*".into();
    assert!(!record_is_current(&graph, "v42", &record).unwrap());
    record.revision = "v42".into();
    assert!(record_is_current(&graph, "v42", &record).unwrap());
    record.dependencies = vec![Dependency::Node(
        1,
        Fingerprint {
            scheme: "other".into(),
            value: "same".into(),
        },
    )];
    assert!(!record_is_current(&graph, "v42", &record).unwrap());
    record.revision = "*".into();
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
// {test}`requirements.sdk.graph.execution.root_inputs`
// {test}`requirements.sdk.graph.outgoing_edges`
// {test}`requirements.sdk.graph.fingerprinting.snapshot`
// {test}`requirements.sdk.graph.context-tools.outgoing_edges`
#[test]
fn labeled_selections_track_only_target_membership_without_reading_contents() {
    let graph = Memory::default();
    let ctx = ValidationContext::new(&graph, 1);
    assert_eq!(ctx.node_id(), &1);
    assert!(ctx.successors(&7).unwrap().is_empty());
    let output = || Assessment {
        result: true,
        rationale: "selection".into(),
        metadata: (),
    };
    let empty = ctx.inspection().finish("v1".into(), output()).unwrap();
    graph.state.lock().unwrap().edges.insert(1, vec![(8, 2)]);
    assert!(record_is_current(&graph, "v1", &empty).unwrap());
    graph
        .state
        .lock()
        .unwrap()
        .edges
        .insert(1, vec![(8, 2), (7, 5), (7, 3), (7, 5)]);
    assert!(!record_is_current(&graph, "v1", &empty).unwrap());
    let selected = ValidationContext::new(&graph, 1);
    assert_eq!(selected.successors(&7).unwrap(), [5, 3, 5]);
    let snapshot = selected.inspection().outgoing_edges(&1, 7).unwrap();
    assert_eq!(snapshot.content, [5, 3, 5]);
    assert_eq!(snapshot.fingerprint, edge_fingerprint(&graph, &[3, 5]));
    selected.successors(&7).unwrap();
    let record = selected.inspection().finish("v1".into(), output()).unwrap();
    assert_eq!(
        record.dependencies,
        [Dependency::OutgoingEdges(
            1,
            7,
            edge_fingerprint(&graph, &[3, 5])
        )]
    );
    graph
        .state
        .lock()
        .unwrap()
        .edges
        .insert(1, vec![(7, 3), (7, 5), (8, 99)]);
    assert!(record_is_current(&graph, "v1", &record).unwrap());
    graph
        .state
        .lock()
        .unwrap()
        .edges
        .insert(1, vec![(7, 3), (7, 6), (8, 99)]);
    assert!(!record_is_current(&graph, "v1", &record).unwrap());
    assert!(graph.state.lock().unwrap().reads.is_empty());
    graph.state.lock().unwrap().fail_edges = Some(1);
    assert_eq!(selected.successors(&7), Err("edges"));
    assert_eq!(record_is_current(&graph, "v1", &record), Err("edges"));
}

// {test}`requirements.sdk.graph.validators.validate_node`
// {test}`requirements.sdk.graph.evidence.check_node`
// {test}`requirements.sdk.graph.agent-calls.procedures`
// {test}`requirements.sdk.graph.validation.criterion_trait`
// {test}`requirements.sdk.graph.execution.deterministic_procedures`
// {test}`requirements.sdk.graph.evidence.procedure_identity`
// {test}`requirements.sdk.graph.evidence.inputs`
#[tokio::test]
async fn caller_reuses_negative_answers_and_compares_revisions_explicitly() {
    let graph = Memory::default();
    let validator = Validator::new(graph.clone());
    let review = || Review {
        node: 9,
        revision: "v1",
    };
    let value = validator.verify(review()).await.unwrap();
    assert!(!value.output().result);
    assert!(graph.state.lock().unwrap().records.is_empty());
    validator.store_evidence(&value).await.unwrap();
    let cached = validator.load_evidence(&review()).await.unwrap().unwrap();
    assert!(!cached.output().result);
    assert!(procedure_matches(cached.revision(), &review().revision()));
    assert!(validator.check_inputs(&cached).unwrap());
    {
        let state = graph.state.lock().unwrap();
        assert_eq!(state.started, vec![9]);
        assert!(state.reads.is_empty());
        assert!(state.selections.is_empty());
        assert!(state.records[&9].dependencies.is_empty());
    }
    assert!(
        validator
            .load_evidence(&Review {
                node: 8,
                revision: "v1"
            })
            .await
            .unwrap()
            .is_none()
    );
    let v2 = Review {
        node: 9,
        revision: "v2",
    };
    let old = validator.load_evidence(&v2).await.unwrap().unwrap();
    assert_eq!(old.revision(), "v1");
    assert!(validator.check_inputs(&old).unwrap());
    assert!(!procedure_matches(old.revision(), &v2.revision()));
    let replacement = validator.verify(v2).await.unwrap();
    validator.store_evidence(&replacement).await.unwrap();
    assert_eq!(graph.state.lock().unwrap().records[&9].revision, "v2");
    graph
        .state
        .lock()
        .unwrap()
        .records
        .get_mut(&9)
        .unwrap()
        .revision = "*".into();
    let legacy = validator
        .load_evidence(&Review {
            node: 9,
            revision: "v2",
        })
        .await
        .unwrap()
        .unwrap();
    assert!(procedure_matches(legacy.revision(), "v2"));
    assert!(!legacy.output().result);
    assert!(validator.check_inputs(&legacy).unwrap());
}

// {test}`requirements.sdk.graph.evidence.store_evidence`
// {test}`requirements.sdk.graph.validators.validate_node.errors`
// {test}`requirements.sdk.graph.evidence.check_node`
// {test}`requirements.sdk.graph.evidence.procedure_identity`
// {test}`requirements.sdk.graph.evidence.record.output`
#[tokio::test]
async fn resolution_storage_and_callback_errors_are_not_judgments() {
    let graph = Memory::default();
    let validator = Validator::new(graph.clone());
    graph.state.lock().unwrap().fail_load = true;
    assert!(matches!(
        validator
            .load_evidence(&Review {
                node: 1,
                revision: "v1"
            })
            .await,
        Err(ValidationError::Graph("load"))
    ));
    let value = validator
        .verify(Review {
            node: 1,
            revision: "v1",
        })
        .await
        .unwrap();
    graph.state.lock().unwrap().fail_store = true;
    assert!(matches!(
        validator.store_evidence(&value).await,
        Err(ValidationError::Graph("store"))
    ));
    assert!(graph.state.lock().unwrap().records.is_empty());
    let graph = Memory {
        read_in_callback: true,
        ..Memory::default()
    };
    let validator = Validator::new(graph.clone());
    assert!(matches!(
        validator
            .verify(Review {
                node: 1,
                revision: "v1"
            })
            .await,
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
            .contents_of_current_node()
            .contents_of_successors(7),
    );
    assert!(ctx.inspection().dependencies().is_empty());
    assert!(matches!(
        ctx.ask::<Answer>()
            .contents_of_successors(7)
            .contents_of_current_node()
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
    let ctx = ValidationContext::with_agent(&graph, 1, &agent);
    let future = ctx
        .ask::<Answer>()
        .context(String::from("question"))
        .contents_of_current_node()
        .contents_of_successors(7)
        .contents_of_current_node()
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
            Dependency::OutgoingEdges(1, 7, edge_fingerprint(&graph, &[2, 3])),
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
    let ctx = ValidationContext::with_agent(&graph, 1, &agent);
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
    for request in &requests[1..=2] {
        assert_eq!(request.0, "");
        assert_eq!(request.1, context_fingerprint(""));
        assert!(request.2.is_empty());
    }
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
// {test}`requirements.sdk.graph.agent-calls.judge.caching`
#[tokio::test]
async fn string_and_str_contexts_deliver_identical_prefixes_and_fingerprints() {
    let graph = Memory::default();
    {
        let mut state = graph.state.lock().unwrap();
        state.contents.insert(1, "first source".into());
        state.contents.insert(2, "second source".into());
        state.edges.insert(1, vec![(7, 2)]);
    }
    let agent = MockAgent::default();
    let ctx = ValidationContext::with_agent(&graph, 1, &agent);
    for text in ["", "héllo\0\n\npolicy"] {
        ctx.ask::<Answer>().context(text.to_owned()).await.unwrap();
        ctx.ask::<Answer>().context(text).await.unwrap();
    }

    ctx.ask::<Answer>()
        .context("policy")
        .context("question")
        .await
        .unwrap();
    ctx.ask::<Answer>()
        .context(("policy", String::from("question")))
        .await
        .unwrap();
    let contexts = ["policy", "question"];
    ctx.ask::<Answer>()
        .context(contexts.as_slice())
        .await
        .unwrap();
    ctx.ask::<Answer>()
        .context(contexts.to_vec())
        .await
        .unwrap();

    let mut temporary = String::from("short-lived");
    let request = ctx.ask::<Answer>().context(temporary.as_str());
    temporary.clear();
    request.await.unwrap();
    assert!(ctx.inspection().dependencies().is_empty());

    ctx.ask::<Answer>()
        .context("short-lived")
        .contents_of_current_node()
        .await
        .unwrap();
    ctx.ask::<Value>()
        .context("short-lived")
        .contents_of_current_node()
        .await
        .unwrap();
    ctx.ask::<Answer>()
        .context("short-lived")
        .contents_of_successors(7)
        .await
        .unwrap();
    ValidationContext::with_agent(&graph, 2, &agent)
        .ask::<Answer>()
        .context("short-lived")
        .await
        .unwrap();

    let requests = agent.requests.lock().unwrap();
    assert_eq!(requests.len(), 13);
    for (pair, text) in requests[..4].chunks_exact(2).zip(["", "héllo\0\n\npolicy"]) {
        for request in pair {
            assert_eq!(request.0, text);
            assert_eq!(request.1, context_fingerprint(text));
        }
    }
    for request in &requests[4..8] {
        assert_eq!(request.0, "policy\n\nquestion");
        assert_eq!(request.1, context_fingerprint("policy\n\nquestion"));
    }
    for request in &requests[8..] {
        assert_eq!(request.0, "short-lived");
        assert_eq!(request.1, context_fingerprint("short-lived"));
    }
    assert!(requests[..9].iter().all(|request| request.2.is_empty()));
    assert_eq!(requests[9].2, vec![(1, "first source".into())]);
    assert_eq!(requests[10].2, requests[9].2);
    assert_ne!(requests[10].3, requests[9].3);
    assert_eq!(requests[11].2, vec![(2, "second source".into())]);
    assert!(requests[12].2.is_empty());
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
// {test}`requirements.sdk.graph.agent-calls.judge`
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
    let ctx = ValidationContext::with_agent(&graph, 1, &agent);
    let (first, second) = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        futures::join!(
            ctx.ask::<Answer>().contents_of_current_node().into_future(),
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
    graph.state.lock().unwrap().contents.extend([
        (1, "retained".into()),
        (2, "direct before".into()),
        (3, "direct after".into()),
    ]);
    let agent = MockAgent::default();
    let ctx = ValidationContext::with_agent(&graph, 1, &agent);
    fn assert_send_sync<T: Send + Sync>(_: &T) {}
    assert_send_sync(&ctx);
    assert_eq!(ctx.inspection().read(&2).unwrap(), "direct before");
    let _: Answer = ctx
        .ask()
        .contents_of_current_node()
        .contents_of_successors(7)
        .await
        .unwrap();
    assert_eq!(ctx.inspection().read(&3).unwrap(), "direct after");
    let retained = ctx.inspection().dependencies();
    assert_eq!(
        retained,
        vec![
            Dependency::Node(2, fp("direct before")),
            Dependency::OutgoingEdges(1, 7, edge_fingerprint(&graph, &[])),
            Dependency::Node(1, fp("retained")),
            Dependency::Node(3, fp("direct after")),
        ]
    );
    assert_eq!(graph.state.lock().unwrap().reads, [2, 1, 3]);
    let _: Answer = ctx.ask().context("lighter".to_owned()).await.unwrap();
    assert_eq!(ctx.inspection().dependencies(), retained);
    assert!(agent.requests.lock().unwrap()[1].2.is_empty());
    assert_eq!(
        ctx.inspection()
            .finish(
                "v1".into(),
                Assessment {
                    result: true,
                    rationale: "retained".into(),
                    metadata: (),
                }
            )
            .unwrap()
            .dependencies,
        retained
    );
}

// {test}`requirements.sdk.domain`
// {test}`requirements.sdk.graph.validators.validate_node`
// {test}`requirements.sdk.graph.execution.root_inputs`
// {test}`requirements.sdk.graph.evidence.node_inputs`
#[tokio::test]
async fn concurrent_single_node_validations_keep_separate_traces() {
    let graph = Memory {
        read_in_callback: true,
        edges_in_callback: true,
        delay: true,
        ..Memory::default()
    };
    {
        let mut state = graph.state.lock().unwrap();
        for node in 1..=3 {
            state.contents.insert(node, format!("node {node}"));
            state.edges.insert(node, vec![(1, 99)]);
        }
    }
    let validator = Validator::new(graph.clone());
    let (first, second, third) = futures::join!(
        validator.verify(Review {
            node: 1,
            revision: "v1"
        }),
        validator.verify(Review {
            node: 2,
            revision: "v1"
        }),
        validator.verify(Review {
            node: 3,
            revision: "v1"
        }),
    );
    let (first, second, third) = (first.unwrap(), second.unwrap(), third.unwrap());
    assert!(first.output().result);
    assert!(second.output().result);
    assert!(third.output().result);
    let (a, b, c) = futures::join!(
        validator.store_evidence(&first),
        validator.store_evidence(&second),
        validator.store_evidence(&third)
    );
    a.unwrap();
    b.unwrap();
    c.unwrap();
    assert_eq!(graph.peak.load(Ordering::SeqCst), 3);
    let state = graph.state.lock().unwrap();
    let mut started = state.started.clone();
    started.sort();
    assert_eq!(started, vec![1, 2, 3]);
    assert_eq!(state.records.len(), 3);
    for (node, record) in &state.records {
        assert_eq!(
            record.dependencies,
            vec![
                Dependency::Node(*node, fp(format!("node {node}"))),
                Dependency::OutgoingEdges(*node, 1, edge_fingerprint(&graph, &[99])),
            ]
        );
    }
}

// {test}`requirements.sdk.graph.agent-calls.judge.submission`
#[tokio::test]
async fn invalid_typed_submission_is_an_agent_error() {
    let graph = Memory::default();
    let agent = MockAgent {
        invalid: true,
        ..MockAgent::default()
    };
    let ctx = ValidationContext::with_agent(&graph, 1, &agent);
    assert!(matches!(
        ctx.ask::<Answer>().await,
        Err(ValidationError::Agent(_))
    ));
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

// {test}`requirements.sdk.graph.validation.criterion_trait`
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

// {test}`requirements.sdk.graph.agent-calls.judge`
// {test}`requirements.sdk.graph.agent-calls.judge.node_contents`
// {test}`requirements.sdk.graph.agent-calls.judge.rendering`
// {test}`requirements.sdk.graph.agent-calls.author_procedure`
#[tokio::test]
async fn queued_reads_selection_and_rendering_fail_before_agent_invocation() {
    let graph = Memory::default();
    let agent = MockAgent::default();
    let ctx = ValidationContext::with_agent(&graph, 1, &agent);
    drop(ctx.ask::<Answer>().contents_of_current_node());
    assert!(agent.requests.lock().unwrap().is_empty());
    for edges in [vec![], vec![(8, 99)]] {
        graph.state.lock().unwrap().edges.insert(1, edges.clone());
        let ctx = ValidationContext::with_agent(&graph, 1, &agent);
        ctx.ask::<Answer>().contents_of_successors(7).await.unwrap();
        assert!(graph.state.lock().unwrap().reads.is_empty());
        assert_eq!(
            ctx.inspection().dependencies(),
            vec![Dependency::OutgoingEdges(
                1,
                7,
                edge_fingerprint(&graph, &[])
            )]
        );
    }
    let initial_calls = agent.requests.lock().unwrap().len();
    graph.state.lock().unwrap().fail_read = true;
    assert!(matches!(
        ctx.ask::<Answer>().contents_of_current_node().await,
        Err(ValidationError::Graph("read"))
    ));
    graph.state.lock().unwrap().fail_read = false;
    graph.state.lock().unwrap().fail_edges = Some(1);
    assert!(matches!(
        ctx.ask::<Answer>().contents_of_successors(7).await,
        Err(ValidationError::Graph("edges"))
    ));
    graph.state.lock().unwrap().fail_edges = None;
    graph
        .state
        .lock()
        .unwrap()
        .contents
        .insert(1, "root".into());
    graph.state.lock().unwrap().fail_format = true;
    let ctx = ValidationContext::with_agent(&graph, 1, &agent);
    assert!(matches!(
        ctx.ask::<Answer>().contents_of_current_node().await,
        Err(ValidationError::Graph("render"))
    ));
    assert!(ctx.inspection().dependencies().is_empty());
    assert_eq!(agent.requests.lock().unwrap().len(), initial_calls);
}

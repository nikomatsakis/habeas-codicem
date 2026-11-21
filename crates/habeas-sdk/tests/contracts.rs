use habeas_sdk::graph::*;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::json;
use std::{
    cell::Cell,
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

// Opaque graph identities and contents deliberately do not implement Serde,
// Hash, or Display. Only completed answers need serialization.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Node(u8);
struct Content(String);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Answer {
    satisfied: bool,
    details: Vec<String>,
}

#[derive(Default)]
struct State {
    contents: BTreeMap<u8, String>,
    edges: BTreeMap<(u8, u8), Vec<Node>>,
    records: Vec<(Node, CriterionPath, Record<Node, u8>)>,
    reads: Vec<u8>,
    executions: usize,
    writes: usize,
    fail_load: bool,
    fail_store: bool,
    fail_read: bool,
    fail_edges: bool,
}
#[derive(Clone, Default)]
struct Application(Arc<Mutex<State>>);

impl Graph for Application {
    type NodeId = Node;
    type EdgeLabel = u8;
    type Error = &'static str;

    fn node_path(&self, node: &Node) -> NodePath {
        NodePath::new(vec!["application".into(), node.0.to_string()])
    }
    fn outgoing_edges(&self, node: &Node, label: u8) -> Result<Vec<Node>, Self::Error> {
        let state = self.0.lock().unwrap();
        if state.fail_edges {
            return Err("edges");
        }
        Ok(state
            .edges
            .get(&(node.0, label))
            .cloned()
            .unwrap_or_default())
    }
}
impl Contents for Application {
    type NodeContent = Content;

    fn node_content(&self, node: &Node) -> Result<Content, Self::Error> {
        let mut state = self.0.lock().unwrap();
        state.reads.push(node.0);
        if state.fail_read {
            return Err("read");
        }
        state
            .contents
            .get(&node.0)
            .cloned()
            .map(Content)
            .ok_or("missing")
    }

    fn fingerprint(&self, _: &Node, content: &Content) -> Fingerprint {
        Fingerprint {
            scheme: "text-v1".into(),
            value: content.0.clone(),
        }
    }

    fn format_node_content(&self, _: &Node, content: &Content) -> Result<String, Self::Error> {
        Ok(content.0.clone())
    }
}
impl Storage for Application {
    async fn load_evidence(
        &self,
        node: &Node,
        criterion: &CriterionPath,
    ) -> Result<Option<Record<Node, u8>>, Self::Error> {
        let state = self.0.lock().unwrap();
        if state.fail_load {
            return Err("load");
        }
        Ok(state
            .records
            .iter()
            .find(|(n, c, _)| n == node && c == criterion)
            .map(|(_, _, record)| record.clone()))
    }
    async fn store_evidence(
        &self,
        node: &Node,
        criterion: &CriterionPath,
        record: &Record<Node, u8>,
    ) -> Result<(), Self::Error> {
        let mut state = self.0.lock().unwrap();
        if state.fail_store {
            return Err("store");
        }
        state
            .records
            .retain(|(n, c, _)| n != node || c != criterion);
        state
            .records
            .push((node.clone(), criterion.clone(), record.clone()));
        state.writes += 1;
        Ok(())
    }
}

// Cell makes validation values Send but not Sync, exercising the public bounds.
struct Review<O> {
    source: Node,
    criterion: CriterionPath,
    revision: String,
    output: O,
    reads: Vec<Node>,
    label: Option<u8>,
    barrier: Option<Arc<tokio::sync::Barrier>>,
    failure: Option<&'static str>,
    descriptor_reads: Cell<usize>,
}
impl<O> Review<O> {
    fn new(criterion: &[&str], output: O) -> Self {
        Self {
            source: Node(1),
            criterion: path(criterion),
            revision: "v1".into(),
            output,
            reads: vec![],
            label: None,
            barrier: None,
            failure: None,
            descriptor_reads: Cell::new(0),
        }
    }
}
impl<O: Serialize + serde::de::DeserializeOwned + Send> Criterion<Application> for Review<O> {
    type Ok = O;
    fn source_node_id(&self) -> Node {
        self.descriptor_reads.set(self.descriptor_reads.get() + 1);
        self.source.clone()
    }
    fn criterion_path(&self) -> CriterionPath {
        self.criterion.clone()
    }
    fn revision(&self) -> String {
        self.revision.clone()
    }
    async fn execute(
        self,
        cx: &mut ValidationContext<'_, Application>,
    ) -> Result<O, ValidationError<&'static str>> {
        cx.inspection().graph().0.lock().unwrap().executions += 1;
        if let Some(barrier) = self.barrier {
            barrier.wait().await;
        }
        for node in &self.reads {
            cx.inspection().read(node).map_err(ValidationError::Graph)?;
        }
        if let Some(label) = self.label {
            cx.successors(&label).map_err(ValidationError::Graph)?;
        }
        if let Some(error) = self.failure {
            return Err(ValidationError::Graph(error));
        }
        Ok(self.output)
    }
}
fn path(segments: &[&str]) -> CriterionPath {
    CriterionPath::new(segments.iter().map(|s| (*s).into()).collect())
}
fn answer(satisfied: bool, details: &str) -> Answer {
    Answer {
        satisfied,
        details: vec![details.into()],
    }
}
fn send<T: Send>(value: T) -> T {
    value
}

async fn persist<O: Serialize + serde::de::DeserializeOwned + Send>(
    validator: &Validator<Application>,
    review: Review<O>,
) -> Fingerprinted<Application, O> {
    let value = validator.verify(review).await.unwrap();
    validator.store_evidence(&value).await.unwrap();
    value
}

// {test}`requirements.sdk.graph.domain_types`
// {test}`requirements.sdk.graph.outgoing_edges`
// {test}`requirements.sdk.graph.node_path_method`
#[test]
fn graph_relationships_work_without_node_contents_or_storage() {
    // The API accepts opaque identities and labels, and errors that are Send
    // without also being Sync, Debug, Clone, or std::error::Error.
    #[derive(Clone, PartialEq, Eq)]
    struct Identity(u8);
    #[derive(Clone)]
    struct Label(u8);
    struct GraphError(Cell<u8>);
    struct Relationships;
    impl Graph for Relationships {
        type NodeId = Identity;
        type EdgeLabel = Label;
        type Error = GraphError;

        fn node_path(&self, node: &Identity) -> NodePath {
            NodePath::new(vec!["relationships".into(), node.0.to_string()])
        }

        fn outgoing_edges(
            &self,
            node: &Identity,
            label: Label,
        ) -> Result<Vec<Identity>, Self::Error> {
            if node.0 == 255 {
                return Err(GraphError(Cell::new(node.0)));
            }
            Ok(match (node.0, label.0) {
                (1, 2) => vec![Identity(3), Identity(2), Identity(3)],
                (1, 3) => vec![Identity(4)],
                _ => vec![],
            })
        }
    }

    // Compile generic consumer code against the declared bounds, so loosening
    // a required bound fails as well as adding a bound the opaque fixture lacks.
    fn check_bounds<G: Graph>() {
        fn send_sync<T: Send + Sync>() {}
        fn identity<T: Clone + Eq + Send + Sync>() {}
        fn label<T: Clone + Send + Sync>() {}
        fn error<T: Send>() {}
        send_sync::<G>();
        identity::<G::NodeId>();
        label::<G::EdgeLabel>();
        error::<G::Error>();
    }

    // This helper requires only Graph. Relationships has neither contents nor storage.
    fn successors<G: Graph>(graph: &G, node: &G::NodeId, label: G::EdgeLabel) -> Vec<G::NodeId> {
        graph.outgoing_edges(node, label).ok().unwrap()
    }
    check_bounds::<Relationships>();
    let graph = Relationships;
    assert_eq!(
        graph.node_path(&Identity(1)).segments(),
        ["relationships", "1"]
    );
    assert_ne!(graph.node_path(&Identity(1)), graph.node_path(&Identity(2)));
    assert!(successors(&graph, &Identity(1), Label(2)) == [Identity(3), Identity(2), Identity(3)]);
    assert!(successors(&graph, &Identity(1), Label(3)) == [Identity(4)]);
    assert!(successors(&graph, &Identity(1), Label(4)).is_empty());
    assert!(successors(&graph, &Identity(2), Label(2)).is_empty());
    let error = graph
        .outgoing_edges(&Identity(255), Label(2))
        .err()
        .unwrap();
    assert_eq!(send(error).0.get(), 255);
}

// {test}`requirements.sdk.graph.validation.criterion_path`
// {test}`requirements.sdk.graph.validation.check_procedure`
// {test}`requirements.sdk.graph.validation.validate_node`
// {test}`requirements.sdk.graph.validation.procedure_identity`
// {test}`requirements.sdk.graph.validation.check_node`
// {test}`requirements.sdk.graph.validation.deterministic_procedures`
// {test}`requirements.sdk.graph.evidence.record`
// {test}`requirements.sdk.graph.validation.store_evidence`
// {test}`requirements.sdk.graph.evidence.record.output`
// {test}`requirements.sdk.graph.evidence.evidence_storage`
// {test}`requirements.sdk.graph.domain_types`
// {test}`requirements.sdk.domain`
// {test}`requirements.sdk.graph.evidence.inputs`
// {test}`requirements.sdk.graph.evidence.checking_records`
#[tokio::test]
async fn criteria_keep_typed_answers_and_persist_only_when_requested() {
    let domain = Application::default();
    let validator = Validator::new(domain.clone());
    let negative = answer(false, "missing edge case");
    let descriptor = || Review::new(&["behavior"], negative.clone());
    assert!(
        validator
            .load_evidence(&descriptor())
            .await
            .unwrap()
            .is_none()
    );
    let value = validator.verify(descriptor()).await.unwrap();
    assert_eq!(value.output(), &negative);
    assert_eq!(value.source_node_id(), &Node(1));
    assert_eq!(value.criterion_path(), &path(&["behavior"]));
    assert_eq!(value.revision(), "v1");
    assert!(value.dependencies().is_empty());
    {
        let state = domain.0.lock().unwrap();
        assert_eq!((state.executions, state.writes), (1, 0));
    }
    assert!(
        validator
            .load_evidence(&descriptor())
            .await
            .unwrap()
            .is_none()
    );
    validator.store_evidence(&value).await.unwrap();
    let loaded = validator
        .load_evidence(&descriptor())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.output(), &negative);
    assert_eq!(loaded.source_node_id(), value.source_node_id());
    assert_eq!(loaded.criterion_path(), value.criterion_path());
    assert_eq!(loaded.revision(), value.revision());
    assert_eq!(loaded.dependencies(), value.dependencies());
    assert!(validator.check_inputs(&loaded).unwrap());
    assert_eq!(loaded.into_output(), negative);
    let structure = vec!["both dispatchers use the helper".to_owned()];
    assert_eq!(
        persist(&validator, Review::new(&["structure"], structure.clone()))
            .await
            .into_output(),
        structure
    );
    // Execution ignores saved records and leaves them unchanged until an explicit save.
    let replacement = validator
        .verify(Review::new(&["behavior"], answer(true, "new review")))
        .await
        .unwrap();
    assert!(replacement.output().satisfied);
    assert_eq!(
        validator
            .load_evidence(&descriptor())
            .await
            .unwrap()
            .unwrap()
            .into_output(),
        negative
    );
    assert!(
        validator
            .load_evidence(&Review::new(&["text"], ()))
            .await
            .unwrap()
            .is_none()
    );
    persist(&validator, Review::new(&["text"], ()))
        .await
        .into_output();
    validator
        .load_evidence(&Review::new(&["text"], ()))
        .await
        .unwrap()
        .unwrap()
        .into_output();
    let state = domain.0.lock().unwrap();
    assert_eq!((state.executions, state.writes), (4, 3));
    assert_eq!(
        state
            .records
            .iter()
            .find(|(_, c, _)| c == &path(&["behavior"]))
            .unwrap()
            .2
            .output,
        json!({"satisfied":false,"details":["missing edge case"]})
    );
    assert_eq!(
        state
            .records
            .iter()
            .find(|(_, c, _)| c == &path(&["text"]))
            .unwrap()
            .2
            .output,
        json!(null)
    );
    assert!(state.reads.is_empty());
    assert!(
        state
            .records
            .iter()
            .all(|(_, _, r)| r.dependencies.is_empty())
    );
}

// {test}`requirements.sdk.graph.validation.criterion_path`
#[test]
fn criterion_paths_preserve_hierarchy_empty_segments_order_and_unicode() {
    let paths = [
        path(&[]),
        path(&[""]),
        path(&["a", "b"]),
        path(&["a/b"]),
        path(&["b", "a"]),
        path(&["é"]),
        path(&["e\u{301}"]),
    ];
    for (i, criterion) in paths.iter().enumerate() {
        let bytes = serde_json::to_vec(criterion).unwrap();
        assert_eq!(
            serde_json::from_slice::<CriterionPath>(&bytes).unwrap(),
            *criterion
        );
        for other in &paths[i + 1..] {
            assert_ne!(criterion, other);
        }
    }
    use std::hash::{Hash, Hasher};
    let mut first = std::collections::hash_map::DefaultHasher::new();
    let mut second = std::collections::hash_map::DefaultHasher::new();
    path(&["nested", "criterion"]).hash(&mut first);
    path(&["nested", "criterion"]).hash(&mut second);
    assert_eq!(first.finish(), second.finish());
}

// {test}`requirements.sdk.graph.validation.check_node`
// {test}`requirements.sdk.graph.validation.validate_node`
// {test}`requirements.sdk.graph.evidence.node_inputs`
// {test}`requirements.sdk.graph.evidence.checking_records`
#[tokio::test]
async fn changing_one_criteria_input_does_not_invalidate_another() {
    let domain = Application::default();
    domain
        .0
        .lock()
        .unwrap()
        .contents
        .extend([(2, "test".into()), (3, "implementation".into())]);
    let validator = Validator::new(domain.clone());
    let mut behavior = Review::new(&["behavior"], true);
    behavior.reads = vec![Node(2)];
    persist(&validator, behavior).await;
    let mut structure = Review::new(&["structure"], true);
    structure.reads = vec![Node(3)];
    persist(&validator, structure).await;
    domain
        .0
        .lock()
        .unwrap()
        .contents
        .insert(2, "changed test".into());
    let behavior = validator
        .load_evidence(&Review::new(&["behavior"], true))
        .await
        .unwrap()
        .unwrap();
    let structure = validator
        .load_evidence(&Review::new(&["structure"], true))
        .await
        .unwrap()
        .unwrap();
    assert!(!validator.check_inputs(&behavior).unwrap());
    assert!(validator.check_inputs(&structure).unwrap());
    assert_eq!(domain.0.lock().unwrap().executions, 2);
    let mut replacement = Review::new(&["behavior"], false);
    replacement.reads = vec![Node(3)];
    let value = persist(&validator, replacement).await;
    assert!(!value.output());
    assert_eq!(
        value.dependencies(),
        [Dependency::Node(
            Node(3),
            Fingerprint {
                scheme: "text-v1".into(),
                value: "implementation".into()
            }
        )]
    );
    let saved = domain
        .load_evidence(&Node(1), &path(&["behavior"]))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.dependencies, value.dependencies());
}

// {test}`requirements.sdk.graph.validation.store_evidence`
// {test}`requirements.sdk.graph.evidence.record.output`
// {test}`requirements.sdk.graph.validation.procedure_identity`
// {test}`requirements.sdk.graph.validation.validate_node.errors`
// {test}`requirements.sdk.graph.validation.check_node`
// {test}`requirements.sdk.graph.evidence.checking_records`
#[tokio::test]
async fn typed_loading_reports_decode_errors_even_for_stale_evidence() {
    let domain = Application::default();
    let validator = Validator::new(domain.clone());
    persist(&validator, Review::new(&["behavior"], json!({"old":true}))).await;
    {
        let mut state = domain.0.lock().unwrap();
        state.records[0].2.dependencies = vec![Dependency::Node(
            Node(99),
            Fingerprint {
                scheme: "old".into(),
                value: "old".into(),
            },
        )];
        state.fail_read = true;
    }
    let mut changed = Review::new(&["behavior"], String::new());
    changed.revision = "v2".into();
    assert!(matches!(
        validator.load_evidence(&changed).await,
        Err(ValidationError::Output(_))
    ));
    assert!(domain.0.lock().unwrap().reads.is_empty());
    assert!(matches!(
        validator
            .load_evidence(&Review::new(&["behavior"], String::new()))
            .await,
        Err(ValidationError::Output(_))
    ));
    // No saved-input resolution or output decoding happens during a new execution.
    changed.output = "new answer".into();
    let value = validator.verify(changed).await.unwrap();
    assert_eq!(value.output(), "new answer");
    assert_eq!(value.revision(), "v2");
    {
        let state = domain.0.lock().unwrap();
        assert_eq!((state.executions, state.writes), (2, 1));
        assert!(state.reads.is_empty());
        assert_eq!(state.records[0].2.revision, "v1");
    }
    validator.store_evidence(&value).await.unwrap();
    assert_eq!(domain.0.lock().unwrap().records.len(), 1);
}

// {test}`requirements.sdk.graph.validation.store_evidence`
// {test}`requirements.sdk.graph.evidence.record.output`
// {test}`requirements.sdk.graph.validation.validate_node`
// {test}`requirements.sdk.graph.evidence.record`
#[tokio::test]
async fn input_checking_and_execution_keep_separate_observations() {
    let domain = Application::default();
    domain.0.lock().unwrap().contents.insert(2, "before".into());
    let validator = Validator::new(domain.clone());
    let mut old = Review::new(&["behavior"], json!({"old":true}));
    old.reads = vec![Node(2)];
    persist(&validator, old).await;
    {
        let mut state = domain.0.lock().unwrap();
        state.contents.insert(2, "after".into());
        state.reads.clear();
    }
    let loaded = validator
        .load_evidence(&Review::new(&["behavior"], json!(null)))
        .await
        .unwrap()
        .unwrap();
    assert!(!validator.check_inputs(&loaded).unwrap());
    assert!(matches!(
        validator
            .load_evidence(&Review::new(&["behavior"], String::new()))
            .await,
        Err(ValidationError::Output(_))
    ));
    domain.0.lock().unwrap().reads.clear();
    let mut new = Review::new(&["behavior"], "new answer".to_owned());
    new.reads = vec![Node(2)];
    let value = persist(&validator, new).await;
    assert_eq!(value.output(), "new answer");
    let state = domain.0.lock().unwrap();
    assert_eq!(state.reads, [2]);
    assert_eq!(state.executions, 2);
    assert_eq!(
        state.records[0].2.dependencies,
        [Dependency::Node(
            Node(2),
            Fingerprint {
                scheme: "text-v1".into(),
                value: "after".into()
            }
        )]
    );
}

// {test}`requirements.sdk.graph.validation.check_node`
// {test}`requirements.sdk.graph.evidence.checking_records`
// {test}`requirements.sdk.graph.evidence.inputs`
#[tokio::test]
async fn dependency_resolution_failures_do_not_execute_or_replace_saved_answers() {
    let domain = Application::default();
    domain.0.lock().unwrap().contents.insert(2, "input".into());
    let validator = Validator::new(domain.clone());
    let mut review = Review::new(&["behavior"], false);
    review.reads = vec![Node(2)];
    review.label = Some(7);
    let value = persist(&validator, review).await;
    let expected = [
        Dependency::Node(
            Node(2),
            Fingerprint {
                scheme: "text-v1".into(),
                value: "input".into(),
            },
        ),
        Dependency::OutgoingEdges(Node(1), 7, edge_fingerprint(&domain, &[])),
    ];
    assert_eq!(value.dependencies(), expected);
    for (read, edges, message) in [(true, false, "read"), (false, true, "edges")] {
        {
            let mut state = domain.0.lock().unwrap();
            state.fail_read = read;
            state.fail_edges = edges;
        }
        // Loading is independent of input resolution, even when inputs cannot resolve.
        let loaded = validator
            .load_evidence(&Review::new(&["behavior"], false))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.dependencies(), expected);
        assert_eq!(validator.check_inputs(&loaded), Err(message));
        let state = domain.0.lock().unwrap();
        assert_eq!((state.executions, state.writes), (1, 1));
        assert_eq!(state.records[0].2.dependencies, value.dependencies());
        assert_eq!(state.records[0].2.output, json!(false));
    }
}

// {test}`requirements.sdk.graph.evidence.checking_records.procedure_match`
// {test}`requirements.sdk.graph.validation.check_node`
// {test}`requirements.sdk.graph.validation.criterion_path`
#[tokio::test]
async fn wildcard_revision_does_not_supply_other_criteria_or_nodes() {
    let domain = Application::default();
    let validator = Validator::new(domain.clone());
    persist(&validator, Review::new(&[], false)).await;
    domain.0.lock().unwrap().records[0].2.revision = "*".into();
    let mut future = Review::new(&[], true);
    future.revision = "future".into();
    let loaded = validator.load_evidence(&future).await.unwrap().unwrap();
    assert!(!loaded.output());
    assert_eq!(loaded.revision(), "*");
    assert!(procedure_matches(loaded.revision(), &future.revision()));
    assert!(validator.check_inputs(&loaded).unwrap());
    assert!(
        validator
            .load_evidence(&Review::new(&["new"], false))
            .await
            .unwrap()
            .is_none()
    );
    future.source = Node(2);
    assert!(validator.load_evidence(&future).await.unwrap().is_none());
    assert_eq!(domain.0.lock().unwrap().executions, 1);
}

// {test}`requirements.sdk.graph.validation.check_procedure`
// {test}`requirements.sdk.graph.validation.check_node`
// {test}`requirements.sdk.graph.validation.validate_node`
// {test}`requirements.sdk.graph.validation.validator`
// {test}`requirements.sdk.graph.validation.store_evidence`
#[tokio::test]
async fn validation_and_answer_need_send_without_sync() {
    let domain = Application::default();
    let validator = Validator::new(domain.clone());
    let value = send(validator.verify(Review::new(&["cell"], Cell::new(17u32))))
        .await
        .unwrap();
    assert_eq!(value.output().get(), 17);
    send(validator.store_evidence(&value)).await.unwrap();
    let descriptor = Review::new(&["cell"], Cell::new(0u32));
    let loaded = send(validator.load_evidence(&descriptor))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.into_output().get(), 17);
    assert_eq!(descriptor.descriptor_reads.get(), 1);
    assert_eq!(domain.0.lock().unwrap().executions, 1);
}

// {test}`requirements.sdk.graph.validation.root_inputs`
// {test}`requirements.sdk.graph.evidence.checking_records`
#[tokio::test]
async fn absence_certification_is_invalidated_by_the_first_selected_root() {
    let domain = Application::default();
    let validator = Validator::new(domain.clone());
    for (name, label) in [("behavior", 1), ("structure", 2)] {
        let mut review = Review::new(&[name], true);
        review.label = Some(label);
        persist(&validator, review).await;
    }
    domain.0.lock().unwrap().edges.insert((1, 1), vec![Node(2)]);
    let behavior = validator
        .load_evidence(&Review::new(&["behavior"], true))
        .await
        .unwrap()
        .unwrap();
    let structure = validator
        .load_evidence(&Review::new(&["structure"], true))
        .await
        .unwrap()
        .unwrap();
    assert!(!validator.check_inputs(&behavior).unwrap());
    assert!(validator.check_inputs(&structure).unwrap());
    assert!(domain.0.lock().unwrap().reads.is_empty());
}

#[derive(Deserialize)]
struct Unencodable;
impl Serialize for Unencodable {
    fn serialize<S: Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("cannot encode answer"))
    }
}

// {test}`requirements.sdk.graph.validation.validate_node.errors`
// {test}`requirements.sdk.graph.validation.store_evidence`
// {test}`requirements.sdk.graph.evidence.record.output`
#[tokio::test]
async fn serialization_failure_leaves_the_existing_record_unchanged() {
    let domain = Application::default();
    let validator = Validator::new(domain.clone());
    persist(&validator, Review::new(&[], "old answer".to_owned())).await;
    let before = domain
        .load_evidence(&Node(1), &path(&[]))
        .await
        .unwrap()
        .unwrap();
    let mut new = Review::new(&[], Unencodable);
    new.revision = "v2".into();
    let value = validator.verify(new).await.unwrap();
    let error = validator.store_evidence(&value).await.err().unwrap();
    assert!(matches!(error, ValidationError::Output(_)));
    assert!(error.to_string().contains("cannot encode answer"));
    let after = domain
        .load_evidence(&Node(1), &path(&[]))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(before.revision, after.revision);
    assert_eq!(before.dependencies, after.dependencies);
    assert_eq!(before.output, after.output);
    assert_eq!(domain.0.lock().unwrap().writes, 1);
}

// {test}`requirements.sdk.graph.validation.validate_node.errors`
// {test}`requirements.sdk.graph.validation.check_node`
// {test}`requirements.sdk.graph.evidence.record.output`
#[tokio::test]
async fn loading_execution_resolution_and_saving_failures_do_not_publish_answers() {
    let domain = Application::default();
    let validator = Validator::new(domain.clone());
    domain.0.lock().unwrap().fail_load = true;
    assert!(matches!(
        validator.load_evidence(&Review::new(&[], true)).await,
        Err(ValidationError::Graph("load"))
    ));
    let value = validator.verify(Review::new(&[], true)).await.unwrap();
    assert!(value.output());
    domain.0.lock().unwrap().fail_load = false;
    let mut review = Review::new(&[], true);
    review.failure = Some("execute");
    assert!(matches!(
        validator.verify(review).await,
        Err(ValidationError::Graph("execute"))
    ));
    let mut review = Review::new(&[], true);
    review.reads = vec![Node(99)];
    assert!(matches!(
        validator.verify(review).await,
        Err(ValidationError::Graph("missing"))
    ));
    let mut review = Review::new(&[], true);
    review.label = Some(1);
    domain.0.lock().unwrap().fail_edges = true;
    assert!(matches!(
        validator.verify(review).await,
        Err(ValidationError::Graph("edges"))
    ));
    domain.0.lock().unwrap().fail_store = true;
    assert!(matches!(
        validator.store_evidence(&value).await,
        Err(ValidationError::Graph("store"))
    ));
    let state = domain.0.lock().unwrap();
    assert!(state.records.is_empty());
    assert_eq!(state.writes, 0);
}

// {test}`requirements.sdk.graph.validation.validator`
// {test}`requirements.sdk.graph.evidence.evidence_storage`
// {test}`requirements.sdk.domain`
// {test}`requirements.sdk.graph.validation.store_evidence`
#[tokio::test]
async fn concurrent_same_criterion_executions_keep_complete_independent_records() {
    let domain = Application::default();
    domain
        .0
        .lock()
        .unwrap()
        .contents
        .extend([(2, "first input".into()), (3, "second input".into())]);
    let validator = Validator::new(domain.clone());
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mut first = Review::new(&["behavior"], "first".to_owned());
    first.revision = "first-revision".into();
    first.reads = vec![Node(2)];
    first.barrier = Some(barrier.clone());
    let mut second = Review::new(&["behavior"], "second".to_owned());
    second.revision = "second-revision".into();
    second.reads = vec![Node(3)];
    second.barrier = Some(barrier);
    let (a, b) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        futures::join!(validator.verify(first), validator.verify(second))
    })
    .await
    .expect("same-criterion executions must not wait for each other");
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(a.output(), "first");
    assert_eq!(b.output(), "second");
    assert!(domain.0.lock().unwrap().records.is_empty());
    let (saved_a, saved_b) =
        futures::join!(validator.store_evidence(&a), validator.store_evidence(&b));
    saved_a.unwrap();
    saved_b.unwrap();
    let saved = domain
        .load_evidence(&Node(1), &path(&["behavior"]))
        .await
        .unwrap()
        .unwrap();
    match saved.output.as_str().unwrap() {
        "first" => {
            assert_eq!(saved.revision, "first-revision");
            assert_eq!(
                saved.dependencies,
                [Dependency::Node(
                    Node(2),
                    Fingerprint {
                        scheme: "text-v1".into(),
                        value: "first input".into()
                    }
                )]
            );
        }
        "second" => {
            assert_eq!(saved.revision, "second-revision");
            assert_eq!(
                saved.dependencies,
                [Dependency::Node(
                    Node(3),
                    Fingerprint {
                        scheme: "text-v1".into(),
                        value: "second input".into()
                    }
                )]
            );
        }
        other => panic!("unexpected answer {other:?}"),
    }
    let state = domain.0.lock().unwrap();
    assert_eq!(state.executions, 2);
    assert_eq!(state.writes, 2);
    assert_eq!(state.records.len(), 1);
}

// {test}`requirements.sdk.graph.hierarchical-paths`
#[test]
fn hierarchical_requirement_identity_maps_each_component_to_a_path_segment() {
    // The chapter's illustrative domain, rather than Habeas's persisted IDs.
    enum HabeasNodeId {
        Requirement(Vec<String>),
    }
    let node_path = |node: &HabeasNodeId| match node {
        HabeasNodeId::Requirement(parts) => NodePath::new(
            std::iter::once("requirement".to_owned())
                .chain(parts.iter().cloned())
                .collect(),
        ),
    };
    let requirement = HabeasNodeId::Requirement(vec!["a".into(), "b".into(), "c".into()]);
    assert_eq!(
        node_path(&requirement).segments(),
        ["requirement", "a", "b", "c"]
    );
    assert_eq!(node_path(&requirement), node_path(&requirement));
}

// {test}`requirements.sdk.graph.node_path_method`
// {test}`requirements.sdk.graph.node_path_struct`
// {test}`requirements.sdk.graph.fingerprinting.fingerprint_content`
// {test}`requirements.sdk.graph.fingerprinting.snapshot`
// {test}`requirements.sdk.graph.agent-calls.judge.rendering`
#[test]
fn identities_remain_stable_and_fingerprinting_uses_the_supplied_snapshot() {
    let graph = Application::default();
    graph.0.lock().unwrap().contents.insert(1, "before".into());
    let snapshot = InspectionContext::new(&graph)
        .read_snapshot(&Node(1))
        .unwrap();
    assert_eq!(
        graph
            .format_node_content(&Node(1), &snapshot.content)
            .unwrap(),
        "before"
    );
    let path = graph.node_path(&Node(1));
    graph.0.lock().unwrap().contents.insert(1, "after".into());
    assert_eq!(
        graph.fingerprint(&Node(1), &snapshot.content),
        snapshot.fingerprint
    );
    assert_eq!(
        graph
            .format_node_content(&Node(1), &snapshot.content)
            .unwrap(),
        "before"
    );
    assert_eq!(graph.0.lock().unwrap().reads, [1]);
    assert_eq!(snapshot.fingerprint.value, "before");
    assert_eq!(graph.node_path(&Node(1)), path);
    assert_eq!(Application::default().node_path(&Node(1)), path);
    assert_ne!(graph.node_path(&Node(2)), path);
    let paths = [
        vec![],
        vec![""],
        vec!["", ""],
        vec!["a"],
        vec!["a", "a"],
        vec!["a", "b"],
        vec!["b", "a"],
        vec!["A", "b"],
        vec!["a ", "b"],
        vec!["a/b"],
        vec!["é"],
        vec!["e\u{301}"],
    ]
    .map(|segments| NodePath::new(segments.into_iter().map(str::to_owned).collect()));
    for (index, path) in paths.iter().enumerate() {
        assert_eq!(*path, path.clone());
        for other in &paths[index + 1..] {
            assert_ne!(path, other);
        }
    }
}

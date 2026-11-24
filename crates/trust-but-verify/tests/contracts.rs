use serde::{Deserialize, Serialize, Serializer};
use serde_json::json;
use std::{
    cell::Cell,
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use trust_but_verify::domain::Domain;
use trust_but_verify::domain::NodeContents;
use trust_but_verify::graph::validator::Validator;
use trust_but_verify::graph::*;
use trust_but_verify::request::ValidationContext;

// Opaque graph identities and contents deliberately do not implement Serde,
// Hash, or Display. Only completed answers need serialization.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Node(u8);
struct Content(String);
impl NodeContents for Content {
    fn fingerprint(&self) -> Fingerprint {
        Fingerprint::new("text-v1".into(), self.0.clone())
    }
    fn format(&self) -> String {
        self.0.clone()
    }
}

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
struct Application(Arc<Mutex<State>>, BTreeMap<u8, NodePath>);

impl Domain for Application {
    type NodeId = Node;
    type EdgeLabel = u8;
    type Error = &'static str;

    fn node_path(&self, node: &Node) -> NodePath {
        self.1
            .get(&node.0)
            .cloned()
            .unwrap_or_else(|| NodePath::new(vec!["application".into(), node.0.to_string()]))
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
    fn version(&self) -> CriterionVersion {
        CriterionVersion::new(self.revision.as_str())
    }
    async fn execute(
        self,
        cx: &mut ValidationContext<'_, Application>,
    ) -> Result<O, ValidationError<&'static str>> {
        cx.graph().0.lock().unwrap().executions += 1;
        if let Some(barrier) = self.barrier {
            barrier.wait().await;
        }
        for node in &self.reads {
            cx.read(node).map_err(ValidationError::Graph)?;
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

// {test}`requirements.sdk.domain`
// {test}`requirements.sdk.errors`
// {test}`requirements.sdk.graph.domain_types`
// {test}`requirements.sdk.graph-edges.outgoing_edges`
// {test}`requirements.sdk.graph.node_path_method`
#[tokio::test]
async fn domain_relationships_accept_opaque_types_and_send_only_errors() {
    // The API accepts opaque identities and labels, and errors that are Send
    // without also being Sync, Debug, Clone, or std::error::Error.
    #[derive(Clone, PartialEq, Eq)]
    struct Identity(u8);
    #[derive(Clone)]
    struct Label(u8);
    struct GraphError(Cell<u8>);
    #[derive(Clone)]
    struct Relationships;
    impl Domain for Relationships {
        type NodeId = Identity;
        type EdgeLabel = Label;
        type Error = GraphError;
        type NodeContent = Content;

        fn node_content(&self, node: &Identity) -> Result<Content, Self::Error> {
            Ok(Content(node.0.to_string()))
        }
        async fn load_evidence(
            &self,
            _: &Identity,
            _: &CriterionPath,
        ) -> Result<Option<Record<Identity, Label>>, Self::Error> {
            Ok(None)
        }
        async fn store_evidence(
            &self,
            _: &Identity,
            _: &CriterionPath,
            _: &Record<Identity, Label>,
        ) -> Result<(), Self::Error> {
            Ok(())
        }

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
    fn check_bounds<G: Domain>() {
        fn domain<T: Clone + Send + Sync>() {}
        fn identity<T: Clone + Eq + Send + Sync>() {}
        fn label<T: Clone + Send + Sync>() {}
        fn error<T: Send>() {}
        fn contents<T: NodeContents + Send + Sync>() {}
        domain::<G>();
        identity::<G::NodeId>();
        label::<G::EdgeLabel>();
        error::<G::Error>();
        contents::<G::NodeContent>();
    }

    fn successors<G: Domain>(graph: &G, node: &G::NodeId, label: G::EdgeLabel) -> Vec<G::NodeId> {
        graph.outgoing_edges(node, label).ok().unwrap()
    }
    check_bounds::<Relationships>();
    let graph = Relationships;
    let content = graph.node_content(&Identity(1)).ok().unwrap();
    assert_eq!(content.format(), "1");
    assert_eq!(content.fingerprint().value, "1");
    // Storage futures remain Send even when the application's error is not Sync.
    let criterion = CriterionPath::new(vec!["review".into()]);
    assert!(
        send(graph.load_evidence(&Identity(1), &criterion))
            .await
            .ok()
            .unwrap()
            .is_none()
    );
    let record = Record::new(Some(CriterionVersion::new("v1")), vec![], json!(false));
    assert!(
        send(graph.store_evidence(&Identity(1), &criterion, &record))
            .await
            .is_ok()
    );
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

// {test}`requirements.sdk.graph.node_path_struct`
#[test]
fn node_paths_round_trip_empty_and_separated_segments() {
    for parts in [vec![], vec![""], vec!["a/b"], vec!["a", "b"]] {
        let path = NodePath::new(parts.iter().map(|part| part.to_string()).collect());
        let bytes = serde_json::to_vec(&path).unwrap();
        let decoded: NodePath = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded.segments(), parts);
        assert_eq!(decoded, path);
    }
}

// {test}`requirements.sdk.graph.criteria.criterion_path`
// {test}`requirements.sdk.graph.criteria.criterion_trait`
// {test}`requirements.sdk.graph.criteria.criterion_trait.answer`
// {test}`requirements.sdk.graph.criteria.criterion_trait.source_node_id`
// {test}`requirements.sdk.graph.validators.validate_node`
// {test}`requirements.sdk.graph.evidence.procedure_identity`
// {test}`requirements.sdk.graph.evidence.check_node`
// {test}`requirements.sdk.graph.criteria.deterministic_procedures`
// {test}`requirements.sdk.graph.evidence.record`
// {test}`requirements.sdk.graph.evidence.store_evidence`
// {test}`requirements.sdk.graph.evidence.record.output`
// {test}`requirements.sdk.graph.evidence.evidence_storage`
// {test}`requirements.sdk.graph.domain_types`
// {test}`requirements.sdk.domain`
// {test}`requirements.sdk.graph.evidence.inputs`
// {test}`requirements.sdk.graph.evidence.checking_records`
// {test}`requirements.sdk.graph.criteria.output_policy`
// {test}`requirements.sdk.graph.evidence.recorded_criterion_version`
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
    assert_eq!(value.version().unwrap().as_str(), "v1");
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
    assert_eq!(loaded.version(), value.version());
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

// {test}`requirements.sdk.graph.criteria.criterion_path`
// {test}`requirements.sdk.graph.criteria.CriterionPath`
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
    assert_eq!(paths[2].clone().segments(), ["a", "b"]);
    assert_ne!(format!("{:?}", paths[2]), format!("{:?}", paths[3]));
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

// {test}`requirements.sdk.graph.evidence.check_node`
// {test}`requirements.sdk.graph.validators.validate_node`
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
            Fingerprint::new("text-v1".into(), "implementation".into())
        )]
    );
    let saved = domain
        .load_evidence(&Node(1), &path(&["behavior"]))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.dependencies, value.dependencies());
}

// {test}`requirements.sdk.graph.evidence.store_evidence`
// {test}`requirements.sdk.graph.evidence.record.output`
// {test}`requirements.sdk.graph.evidence.procedure_identity`
// {test}`requirements.sdk.graph.validators.validate_node.errors`
// {test}`requirements.sdk.graph.evidence.check_node`
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
            Fingerprint::new("old".into(), "old".into()),
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
    assert_eq!(value.version().unwrap().as_str(), "v2");
    {
        let state = domain.0.lock().unwrap();
        assert_eq!((state.executions, state.writes), (2, 1));
        assert!(state.reads.is_empty());
        assert_eq!(state.records[0].2.version.as_ref().unwrap().as_str(), "v1");
    }
    validator.store_evidence(&value).await.unwrap();
    assert_eq!(domain.0.lock().unwrap().records.len(), 1);
}

// {test}`requirements.sdk.graph.evidence.store_evidence`
// {test}`requirements.sdk.graph.evidence.record.output`
// {test}`requirements.sdk.graph.validators.validate_node`
// {test}`requirements.sdk.graph.evidence.record`
// {test}`requirements.sdk.graph.evidence.checking_records`
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
            Fingerprint::new("text-v1".into(), "after".into())
        )]
    );
}

// {test}`requirements.sdk.graph.evidence.check_node`
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
        Dependency::Node(Node(2), Fingerprint::new("text-v1".into(), "input".into())),
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

// {test}`requirements.sdk.graph.evidence.checking_records.version_match`
// {test}`requirements.sdk.graph.evidence.check_node`
// {test}`requirements.sdk.graph.criteria.criterion_path`
// {test}`requirements.sdk.graph.evidence.recorded_criterion_version`
#[tokio::test]
async fn wildcard_revision_does_not_supply_other_criteria_or_nodes() {
    let domain = Application::default();
    let validator = Validator::new(domain.clone());
    persist(&validator, Review::new(&[], false)).await;
    domain.0.lock().unwrap().records[0].2.version = None;
    let mut future = Review::new(&[], true);
    future.revision = "future".into();
    let loaded = validator.load_evidence(&future).await.unwrap().unwrap();
    assert!(!loaded.output());
    assert_eq!(loaded.version(), None);
    assert!(future.version().is_compatible_with(loaded.version()));
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

// {test}`requirements.sdk.graph.criteria.criterion_trait`
// {test}`requirements.sdk.graph.evidence.check_node`
// {test}`requirements.sdk.graph.validators.validate_node`
// {test}`requirements.sdk.graph.validators.validator`
// {test}`requirements.sdk.graph.evidence.store_evidence`
// {test}`requirements.sdk.graph.evidence.refresh_evidence`
// {test}`requirements.sdk.graph.criteria.criterion_trait.answer`
// {test}`requirements.sdk.graph.criteria.execute`
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
    let fresh = send(validator.refresh_evidence(Review::new(&["refresh"], Cell::new(23u32))))
        .await
        .unwrap();
    assert_eq!(fresh.output().get(), 23);
    let reused = send(validator.refresh_evidence(Review::new(&["refresh"], Cell::new(0u32))))
        .await
        .unwrap();
    assert_eq!(reused.into_output().get(), 23);
    let state = domain.0.lock().unwrap();
    assert_eq!((state.executions, state.writes), (2, 2));
}

// {test}`requirements.sdk.graph.evidence.refresh_evidence`
// {test}`requirements.sdk.graph.evidence.checking_records.version_match`
#[tokio::test]
async fn refresh_reuses_current_positive_negative_and_legacy_answers_without_saving() {
    for satisfied in [true, false] {
        for revision in ["v1", "*"] {
            let domain = Application::default();
            domain.0.lock().unwrap().contents.insert(2, "input".into());
            let validator = Validator::new(domain.clone());
            let mut first = Review::new(&["behavior"], answer(satisfied, "saved answer"));
            first.reads = vec![Node(2)];
            let saved = validator.refresh_evidence(first).await.unwrap();
            domain.0.lock().unwrap().records[0].2.version =
                CriterionVersion::from_recorded(revision);

            let mut next = Review::new(&["behavior"], answer(!satisfied, "unused answer"));
            next.failure = Some("must reuse");
            let reused = validator.refresh_evidence(next).await.unwrap();
            assert_eq!(reused.output(), saved.output());
            assert_eq!(reused.dependencies(), saved.dependencies());
            assert_eq!(reused.source_node_id(), saved.source_node_id());
            assert_eq!(reused.criterion_path(), saved.criterion_path());
            assert_eq!(
                reused.version(),
                CriterionVersion::from_recorded(revision).as_ref()
            );
            let state = domain.0.lock().unwrap();
            assert_eq!((state.executions, state.writes), (1, 1));
            assert_eq!(state.reads, [2, 2]);
            assert_eq!(state.records.len(), 1);
            assert_eq!(
                state.records[0].2.version,
                CriterionVersion::from_recorded(revision)
            );
            assert_eq!(
                state.records[0].2.output,
                serde_json::to_value(saved.output()).unwrap()
            );
        }
    }
}

// {test}`requirements.sdk.graph.evidence.refresh_evidence`
// {test}`requirements.sdk.graph.evidence.checking_records`
// {test}`requirements.sdk.graph.evidence.checking_records.version_match`
#[tokio::test]
async fn refresh_evaluates_missing_incompatible_or_changed_evidence_with_a_fresh_trace() {
    for cause in ["missing", "revision", "contents", "successors"] {
        let domain = Application::default();
        {
            let mut state = domain.0.lock().unwrap();
            state
                .contents
                .extend([(1, "current input".into()), (2, "old input".into())]);
            state.edges.insert((1, 7), vec![Node(2)]);
        }
        let validator = Validator::new(domain.clone());
        if cause != "missing" {
            let mut old = Review::new(&["behavior"], "old answer".to_owned());
            if cause == "successors" {
                old.label = Some(7);
            } else {
                old.reads = vec![Node(2)];
            }
            persist(&validator, old).await;
        }
        {
            let mut state = domain.0.lock().unwrap();
            state.reads.clear();
            match cause {
                "revision" => {
                    // An incompatible revision bypasses resolution of obsolete inputs.
                    state.contents.remove(&2);
                }
                "contents" => {
                    state.contents.insert(2, "changed input".into());
                }
                "successors" => {
                    state.edges.insert((1, 7), vec![Node(3)]);
                }
                _ => {}
            }
        }
        let mut next = Review::new(&["behavior"], "new answer".to_owned());
        next.reads = vec![Node(1)];
        if cause == "revision" {
            next.revision = "v2".into();
        }
        let replacement = validator.refresh_evidence(next).await.unwrap();
        assert_eq!(replacement.output(), "new answer");
        assert_eq!(
            replacement.dependencies(),
            [Dependency::Node(
                Node(1),
                Fingerprint::new("text-v1".into(), "current input".into())
            )]
        );
        let state = domain.0.lock().unwrap();
        let expected_count = if cause == "missing" { 1 } else { 2 };
        assert_eq!(
            (state.executions, state.writes),
            (expected_count, expected_count)
        );
        assert_eq!(
            state.reads,
            if cause == "contents" {
                vec![2, 1]
            } else {
                vec![1]
            }
        );
        assert_eq!(state.records.len(), 1);
        assert_eq!(state.records[0].2.output, json!("new answer"));
        assert_eq!(state.records[0].2.dependencies, replacement.dependencies());
        assert_eq!(state.records[0].2.version.as_ref(), replacement.version());
    }
}

// {test}`requirements.sdk.graph.evidence.refresh_evidence`
// {test}`requirements.sdk.graph.evidence.record.output`
#[tokio::test]
async fn refresh_propagates_load_decode_check_execution_and_save_errors() {
    for stage in [
        "load",
        "decode",
        "stale decode",
        "read",
        "edges",
        "execute",
        "store",
    ] {
        let domain = Application::default();
        domain.0.lock().unwrap().contents.insert(2, "input".into());
        let validator = Validator::new(domain.clone());
        let mut old = Review::new(&[], true);
        if stage == "read" {
            old.reads = vec![Node(2)];
        } else if stage == "edges" {
            old.label = Some(7);
        }
        persist(&validator, old).await;
        let mut next = Review::new(&[], false);
        {
            let mut state = domain.0.lock().unwrap();
            match stage {
                "load" => state.fail_load = true,
                "decode" | "stale decode" => state.records[0].2.output = json!("not a boolean"),
                "read" => state.fail_read = true,
                "edges" => state.fail_edges = true,
                "execute" => next.failure = Some("execute"),
                "store" => state.fail_store = true,
                _ => unreachable!(),
            }
            if matches!(stage, "stale decode" | "execute" | "store") {
                next.revision = "v2".into();
            }
        }
        let before = domain.0.lock().unwrap().records[0].2.clone();
        let error = validator.refresh_evidence(next).await.err().unwrap();
        match (stage, error) {
            ("decode" | "stale decode", ValidationError::Output(_)) => {}
            (expected, ValidationError::Graph(actual)) => assert_eq!(actual, expected),
            (stage, other) => panic!("{stage}: unexpected error {other:?}"),
        }
        let state = domain.0.lock().unwrap();
        assert_eq!(
            state.executions,
            if matches!(stage, "execute" | "store") {
                2
            } else {
                1
            }
        );
        assert_eq!(state.writes, 1);
        assert_eq!(state.records.len(), 1);
        let after = &state.records[0].2;
        assert_eq!(after.output, before.output);
        assert_eq!(after.version, before.version);
        assert_eq!(after.dependencies, before.dependencies);
    }
}

// {test}`requirements.sdk.graph.evidence.refresh_evidence`
// {test}`requirements.sdk.graph.evidence.record.output`
#[tokio::test]
async fn refresh_serialization_failure_preserves_the_saved_record() {
    let domain = Application::default();
    let validator = Validator::new(domain.clone());
    // A unit struct decodes from null, but this criterion's new answer cannot encode.
    domain.0.lock().unwrap().records.push((
        Node(1),
        path(&[]),
        Record::new(Some(CriterionVersion::new("v1")), vec![], json!(null)),
    ));
    let mut next = Review::new(&[], Unencodable);
    next.revision = "v2".into();
    assert!(matches!(
        validator.refresh_evidence(next).await,
        Err(ValidationError::Output(_))
    ));
    let state = domain.0.lock().unwrap();
    assert_eq!((state.executions, state.writes), (1, 0));
    assert_eq!(state.records.len(), 1);
    assert_eq!(state.records[0].2.version.as_ref().unwrap().as_str(), "v1");
    assert_eq!(state.records[0].2.output, json!(null));
}

// {test}`requirements.sdk.graph.evidence.refresh_evidence`
// {test}`requirements.sdk.graph.validators.validator`
// {test}`requirements.sdk.graph.evidence.evidence_storage`
#[tokio::test]
async fn concurrent_refreshes_execute_independently_and_store_complete_records() {
    let domain = Application::default();
    domain
        .0
        .lock()
        .unwrap()
        .contents
        .extend([(2, "first".into()), (3, "second".into())]);
    let validator = Validator::new(domain.clone());
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mut first = Review::new(&[], "first".to_owned());
    first.revision = "first".into();
    first.reads = vec![Node(2)];
    first.barrier = Some(barrier.clone());
    let mut second = Review::new(&[], "second".to_owned());
    second.revision = "second".into();
    second.reads = vec![Node(3)];
    second.barrier = Some(barrier);
    let (a, b) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        futures::join!(
            validator.refresh_evidence(first),
            validator.refresh_evidence(second)
        )
    })
    .await
    .expect("refreshes must execute independently");
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a.output(), "first");
    assert_eq!(b.output(), "second");
    let state = domain.0.lock().unwrap();
    assert_eq!((state.executions, state.writes), (2, 2));
    assert_eq!(state.records.len(), 1);
    let saved = &state.records[0].2;
    let winner = if saved.output == json!("first") {
        &a
    } else {
        &b
    };
    assert_eq!(saved.output, serde_json::to_value(winner.output()).unwrap());
    assert_eq!(saved.version.as_ref(), winner.version());
    assert_eq!(saved.dependencies, winner.dependencies());
}

// {test}`requirements.sdk.graph.validation-context.root_inputs`
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

// {test}`requirements.sdk.graph.validators.validate_node.errors`
// {test}`requirements.sdk.graph.evidence.store_evidence`
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
    assert_eq!(before.version, after.version);
    assert_eq!(before.dependencies, after.dependencies);
    assert_eq!(before.output, after.output);
    assert_eq!(domain.0.lock().unwrap().writes, 1);
}

// {test}`requirements.sdk.graph.validators.validate_node.errors`
// {test}`requirements.sdk.graph.evidence.check_node`
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

// {test}`requirements.sdk.graph.validators.validator`
// {test}`requirements.sdk.graph.evidence.evidence_storage`
// {test}`requirements.sdk.domain`
// {test}`requirements.sdk.graph.evidence.store_evidence`
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
            assert_eq!(saved.version.as_ref().unwrap().as_str(), "first-revision");
            assert_eq!(
                saved.dependencies,
                [Dependency::Node(
                    Node(2),
                    Fingerprint::new("text-v1".into(), "first input".into())
                )]
            );
        }
        "second" => {
            assert_eq!(saved.version.as_ref().unwrap().as_str(), "second-revision");
            assert_eq!(
                saved.dependencies,
                [Dependency::Node(
                    Node(3),
                    Fingerprint::new("text-v1".into(), "second input".into())
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

// {test}`requirements.sdk.graph.node_path_method`
// {test}`requirements.sdk.graph.node_path_struct`
// {test}`requirements.sdk.graph.node_content`
// {test}`requirements.sdk.graph.node_content.fingerprint`
// {test}`requirements.sdk.graph.node_content.format`
// {test}`requirements.sdk.graph.snapshot`
// {test}`requirements.sdk.graph.agent-calls.judge.rendering`
#[test]
fn identities_remain_stable_and_fingerprinting_uses_the_supplied_snapshot() {
    let graph = Application::default();
    graph.0.lock().unwrap().contents.insert(1, "before".into());
    let snapshot = ValidationContext::new(&graph, Node(1))
        .read_snapshot(&Node(1))
        .unwrap();
    assert_eq!(snapshot.content.format(), "before");
    let path = graph.node_path(&Node(1));
    graph.0.lock().unwrap().contents.insert(1, "after".into());
    assert_eq!(snapshot.content.fingerprint(), snapshot.fingerprint);
    assert_eq!(snapshot.content.format(), "before");
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

// {test}`requirements.sdk.graph.validation-context.snapshot`
#[test]
fn edge_fingerprints_preserve_path_boundaries_and_version_membership_encoding() {
    let mut domain = Application::default();
    let paths = [
        vec!["a/b"],
        vec!["a", "b"],
        vec![""],
        vec![],
        vec!["é"],
        vec!["e\u{301}"],
        vec!["a\"b"],
        vec!["a\nb"],
        vec!["a\\b"],
        vec!["a/b", "c"],
        vec!["a", "b/c"],
    ];
    for (index, segments) in paths.into_iter().enumerate() {
        domain.1.insert(
            index as u8,
            NodePath::new(segments.into_iter().map(str::to_owned).collect()),
        );
    }
    let empty = edge_fingerprint(&domain, &[]);
    assert_eq!(
        empty,
        Fingerprint::new(
            "node-path-set-sha256-v1".into(),
            "4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945".into()
        )
    );
    let fingerprints = (0..domain.1.len())
        .map(|index| edge_fingerprint(&domain, &[Node(index as u8)]))
        .collect::<Vec<_>>();
    assert_eq!(
        fingerprints[0].value,
        "54650042aa19836de70a913d9e2d7099ba9b2d1e711aafbd98000367154b9dd1"
    );
    for (index, fingerprint) in fingerprints.iter().enumerate() {
        assert_eq!(fingerprint.scheme, empty.scheme);
        assert_ne!(fingerprint, &empty);
        for other in &fingerprints[index + 1..] {
            assert_ne!(fingerprint, other);
        }
    }
    assert_eq!(
        edge_fingerprint(&domain, &[Node(0), Node(1), Node(0)]),
        edge_fingerprint(&domain, &[Node(1), Node(0)])
    );
    assert!(domain.0.lock().unwrap().reads.is_empty());
}

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
#[derive(Default)]
struct Application(Mutex<State>);
impl Graph for Application {
    type NodeId = Node;
    type NodeContent = Content;
    type EdgeLabel = u8;
    type Error = &'static str;
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
    fn node_path(&self, node: &Node) -> NodePath {
        NodePath {
            segments: vec!["application".into(), node.0.to_string()],
        }
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
impl Fingerprinting for Application {
    fn fingerprint(&self, _: &Node, content: &Content) -> Fingerprint {
        Fingerprint {
            scheme: "text-v1".into(),
            value: content.0.clone(),
        }
    }
}
impl EvidenceStorage for Application {
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
impl<O: Serialize + serde::de::DeserializeOwned + Send> Validation<Application> for Review<O> {
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
    CriterionPath {
        segments: segments.iter().map(|s| (*s).into()).collect(),
    }
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

// {test}`requirements.sdk.graph.validation.criterion_path`
// {test}`requirements.sdk.graph.validation.check_procedure`
// {test}`requirements.sdk.graph.validation.validate_node`
// {test}`requirements.sdk.graph.validation.procedure_identity`
// {test}`requirements.sdk.graph.validation.check_node`
// {test}`requirements.sdk.graph.validation.judgment`
// {test}`requirements.sdk.graph.validation.deterministic_procedures`
// {test}`requirements.sdk.graph.evidence.record`
// {test}`requirements.sdk.graph.evidence.evidence_storage`
// {test}`requirements.sdk.graph.domain_types`
// {test}`requirements.sdk.domain`
#[tokio::test]
async fn criteria_have_independent_typed_answers_and_cache_negative_results() {
    let domain = Application::default();
    let validator = Validator::new(&domain);
    let negative = answer(false, "missing edge case");
    assert_eq!(
        validator
            .verify(Review::new(&["behavior"], negative.clone()))
            .await
            .unwrap(),
        negative
    );
    let structure = vec!["both dispatchers use the helper".to_owned()];
    assert_eq!(
        validator
            .verify(Review::new(&["structure"], structure.clone()))
            .await
            .unwrap(),
        structure
    );
    assert_eq!(
        validator
            .verify(Review::new(&["behavior"], answer(true, "must not execute")))
            .await
            .unwrap(),
        negative
    );
    assert!(
        matches!(validator.check(&Review::new(&["behavior"], answer(true, "unused"))).await.unwrap(),
        EvidenceState::Current(value) if value == negative)
    );
    assert!(matches!(
        validator
            .check(&Review::new(&["text"], json!(null)))
            .await
            .unwrap(),
        EvidenceState::Missing
    ));
    let judgment = Judgment {
        result: false,
        rationale: "a negative judgment".to_owned(),
        metadata: answer(true, "metadata need not agree with the result"),
    };
    assert_eq!(
        validator
            .verify(Review::new(&["judgment"], judgment.clone()))
            .await
            .unwrap(),
        judgment
    );
    assert!(
        matches!(validator.check(&Review::new(&["judgment"], judgment.clone())).await.unwrap(), EvidenceState::Current(value) if value == judgment)
    );
    let state = domain.0.lock().unwrap();
    assert_eq!(state.executions, 3);
    assert_eq!(state.writes, 3);
    assert_eq!(
        state
            .records
            .iter()
            .find(|(_, criterion, _)| criterion == &path(&["behavior"]))
            .unwrap()
            .2
            .output,
        json!({"satisfied": false, "details": ["missing edge case"]}),
    );
    assert_eq!(
        state
            .records
            .iter()
            .find(|(_, criterion, _)| criterion == &path(&["judgment"]))
            .unwrap()
            .2
            .output,
        serde_json::to_value(&judgment).unwrap(),
    );
    assert!(
        state.reads.is_empty(),
        "source identity must not imply a content read"
    );
    assert!(
        state
            .records
            .iter()
            .all(|(_, _, record)| record.dependencies.is_empty())
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
    let validator = Validator::new(&domain);
    let mut behavior = Review::new(&["behavior"], true);
    behavior.reads = vec![Node(2)];
    validator.verify(behavior).await.unwrap();
    let mut structure = Review::new(&["structure"], true);
    structure.reads = vec![Node(3)];
    validator.verify(structure).await.unwrap();
    domain
        .0
        .lock()
        .unwrap()
        .contents
        .insert(2, "changed test".into());
    assert!(matches!(
        validator
            .check(&Review::new(&["behavior"], true))
            .await
            .unwrap(),
        EvidenceState::Stale
    ));
    assert!(matches!(
        validator
            .check(&Review::new(&["structure"], true))
            .await
            .unwrap(),
        EvidenceState::Current(true)
    ));
    assert_eq!(
        domain.0.lock().unwrap().executions,
        2,
        "checking never executes a criterion"
    );
    let mut replacement = Review::new(&["behavior"], false);
    replacement.reads = vec![Node(3)];
    assert!(!validator.verify(replacement).await.unwrap());
    let saved = domain
        .load_evidence(&Node(1), &path(&["behavior"]))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        saved.dependencies,
        [Dependency::Node(
            Node(3),
            Fingerprint {
                scheme: "text-v1".into(),
                value: "implementation".into()
            }
        )]
    );
}

// {test}`requirements.sdk.graph.evidence.record.output`
// {test}`requirements.sdk.graph.validation.procedure_identity`
// {test}`requirements.sdk.graph.validation.validate_node.errors`
// {test}`requirements.sdk.graph.validation.check_node`
// {test}`requirements.sdk.graph.evidence.checking_records`
#[tokio::test]
async fn revision_and_dependency_checks_precede_output_decoding() {
    let domain = Application::default();
    let validator = Validator::new(&domain);
    validator
        .verify(Review::new(&["behavior"], json!({"old": true})))
        .await
        .unwrap();
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
        validator.check(&changed).await.unwrap(),
        EvidenceState::Stale
    ));
    assert!(domain.0.lock().unwrap().reads.is_empty());
    domain.0.lock().unwrap().fail_read = false;
    domain.0.lock().unwrap().records[0].2.dependencies.clear();
    let same_revision = Review::new(&["behavior"], String::new());
    assert!(matches!(
        validator.check(&same_revision).await,
        Err(ValidationError::Output(_))
    ));
    assert!(matches!(
        validator.verify(same_revision).await,
        Err(ValidationError::Output(_))
    ));
    assert_eq!(domain.0.lock().unwrap().executions, 1);
    assert_eq!(domain.0.lock().unwrap().writes, 1);
    changed.output = "new answer".into();
    assert_eq!(validator.verify(changed).await.unwrap(), "new answer");
    assert_eq!(
        domain.0.lock().unwrap().records.len(),
        1,
        "revision changes replace the same criterion"
    );
}

// {test}`requirements.sdk.graph.evidence.record.output`
// {test}`requirements.sdk.graph.validation.validate_node`
// {test}`requirements.sdk.graph.evidence.record`
#[tokio::test]
async fn changed_dependencies_precede_decoding_and_execution_records_its_own_reads() {
    let domain = Application::default();
    domain.0.lock().unwrap().contents.insert(2, "before".into());
    let validator = Validator::new(&domain);
    let mut old = Review::new(&["behavior"], json!({"old": true}));
    old.reads = vec![Node(2)];
    validator.verify(old).await.unwrap();
    {
        let mut state = domain.0.lock().unwrap();
        state.contents.insert(2, "after".into());
        state.reads.clear();
    }
    let mut new = Review::new(&["behavior"], "new answer".to_owned());
    new.reads = vec![Node(2)];
    assert!(matches!(
        validator.check(&new).await.unwrap(),
        EvidenceState::Stale
    ));
    domain.0.lock().unwrap().reads.clear();
    assert_eq!(validator.verify(new).await.unwrap(), "new answer");
    let state = domain.0.lock().unwrap();
    assert_eq!(
        state.reads,
        [2, 2],
        "freshness and execution have separate observations"
    );
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
// {test}`requirements.sdk.graph.validation.validate_node.errors`
#[tokio::test]
async fn dependency_resolution_failures_do_not_execute_or_replace_saved_answers() {
    let domain = Application::default();
    domain.0.lock().unwrap().contents.insert(2, "input".into());
    let validator = Validator::new(&domain);
    let mut review = Review::new(&["behavior"], false);
    review.reads = vec![Node(2)];
    review.label = Some(7);
    validator.verify(review).await.unwrap();
    let before = domain
        .load_evidence(&Node(1), &path(&["behavior"]))
        .await
        .unwrap()
        .unwrap();
    for (read, edges, message) in [(true, false, "read"), (false, true, "edges")] {
        {
            let mut state = domain.0.lock().unwrap();
            state.fail_read = read;
            state.fail_edges = edges;
        }
        assert!(
            matches!(validator.check(&Review::new(&["behavior"], true)).await, Err(ValidationError::Graph(error)) if error == message)
        );
        assert!(
            matches!(validator.verify(Review::new(&["behavior"], true)).await, Err(ValidationError::Graph(error)) if error == message)
        );
        let state = domain.0.lock().unwrap();
        assert_eq!((state.executions, state.writes), (1, 1));
        assert_eq!(state.records[0].2.revision, before.revision);
        assert_eq!(state.records[0].2.dependencies, before.dependencies);
        assert_eq!(state.records[0].2.output, before.output);
    }
}

// {test}`requirements.sdk.graph.evidence.checking_records.procedure_match`
// {test}`requirements.sdk.graph.validation.check_node`
// {test}`requirements.sdk.graph.validation.criterion_path`
#[tokio::test]
async fn wildcard_revision_does_not_supply_other_criteria_or_nodes() {
    let domain = Application::default();
    let validator = Validator::new(&domain);
    validator.verify(Review::new(&[], false)).await.unwrap();
    domain.0.lock().unwrap().records[0].2.revision = "*".into();
    let mut future = Review::new(&[], true);
    future.revision = "future".into();
    assert!(matches!(
        validator.check(&future).await.unwrap(),
        EvidenceState::Current(false)
    ));
    assert!(matches!(
        validator
            .check(&Review::new(&["new"], false))
            .await
            .unwrap(),
        EvidenceState::Missing
    ));
    future.source = Node(2);
    assert!(matches!(
        validator.check(&future).await.unwrap(),
        EvidenceState::Missing
    ));
    assert_eq!(domain.0.lock().unwrap().executions, 1);
}

// {test}`requirements.sdk.graph.validation.check_procedure`
// {test}`requirements.sdk.graph.validation.check_node`
// {test}`requirements.sdk.graph.validation.validate_node`
// {test}`requirements.sdk.graph.validation.validator`
#[tokio::test]
async fn validation_and_answer_need_send_without_sync() {
    let domain = Application::default();
    let validator = Validator::new(&domain);
    let value = send(validator.verify(Review::new(&["cell"], Cell::new(17u32))))
        .await
        .unwrap();
    assert_eq!(value.get(), 17);
    let descriptor = Review::new(&["cell"], Cell::new(0u32));
    assert!(
        matches!(send(validator.check(&descriptor)).await.unwrap(), EvidenceState::Current(value) if value.get() == 17)
    );
    assert_eq!(descriptor.descriptor_reads.get(), 1);
    assert_eq!(domain.0.lock().unwrap().executions, 1);
}

// {test}`requirements.sdk.graph.validation.root_inputs`
// {test}`requirements.sdk.graph.evidence.checking_records`
#[tokio::test]
async fn absence_certification_is_invalidated_by_the_first_selected_root() {
    let domain = Application::default();
    let validator = Validator::new(&domain);
    for (name, label) in [("behavior", 1), ("structure", 2)] {
        let mut review = Review::new(&[name], true);
        review.label = Some(label);
        validator.verify(review).await.unwrap();
    }
    domain.0.lock().unwrap().edges.insert((1, 1), vec![Node(2)]);
    assert!(matches!(
        validator
            .check(&Review::new(&["behavior"], true))
            .await
            .unwrap(),
        EvidenceState::Stale
    ));
    assert!(matches!(
        validator
            .check(&Review::new(&["structure"], true))
            .await
            .unwrap(),
        EvidenceState::Current(true)
    ));
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
// {test}`requirements.sdk.graph.evidence.record.output`
#[tokio::test]
async fn serialization_failure_leaves_the_existing_record_unchanged() {
    let domain = Application::default();
    let validator = Validator::new(&domain);
    validator
        .verify(Review::new(&[], "old answer".to_owned()))
        .await
        .unwrap();
    let before = domain
        .load_evidence(&Node(1), &path(&[]))
        .await
        .unwrap()
        .unwrap();
    let mut new = Review::new(&[], Unencodable);
    new.revision = "v2".into();
    let error = validator.verify(new).await.err().unwrap();
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
#[tokio::test]
async fn loading_execution_resolution_and_saving_failures_do_not_publish_answers() {
    let domain = Application::default();
    let validator = Validator::new(&domain);
    domain.0.lock().unwrap().fail_load = true;
    assert!(matches!(
        validator.check(&Review::new(&[], true)).await,
        Err(ValidationError::Graph("load"))
    ));
    assert!(matches!(
        validator.verify(Review::new(&[], true)).await,
        Err(ValidationError::Graph("load"))
    ));
    assert_eq!(domain.0.lock().unwrap().executions, 0);
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
        validator.verify(Review::new(&[], true)).await,
        Err(ValidationError::Graph("store"))
    ));
    let state = domain.0.lock().unwrap();
    assert!(state.records.is_empty());
    assert_eq!(state.writes, 0);
}

// {test}`requirements.sdk.graph.validation.validator`
// {test}`requirements.sdk.graph.evidence.evidence_storage`
// {test}`requirements.sdk.domain`
#[tokio::test]
async fn concurrent_same_criterion_executions_keep_complete_independent_records() {
    let domain = Application::default();
    domain
        .0
        .lock()
        .unwrap()
        .contents
        .extend([(2, "first input".into()), (3, "second input".into())]);
    let validator = Validator::new(&domain);
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
    assert_eq!(a.unwrap(), "first");
    assert_eq!(b.unwrap(), "second");
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
        HabeasNodeId::Requirement(parts) => NodePath {
            segments: std::iter::once("requirement".to_owned())
                .chain(parts.iter().cloned())
                .collect(),
        },
    };
    let requirement = HabeasNodeId::Requirement(vec!["a".into(), "b".into(), "c".into()]);
    assert_eq!(
        node_path(&requirement).segments,
        ["requirement", "a", "b", "c"]
    );
    assert_eq!(node_path(&requirement), node_path(&requirement));
}

// {test}`requirements.sdk.graph.node_path_method`
// {test}`requirements.sdk.graph.node_path_struct`
// {test}`requirements.sdk.graph.fingerprinting.fingerprint_content`
// {test}`requirements.sdk.graph.fingerprinting.snapshot`
#[test]
fn identities_remain_stable_and_fingerprinting_uses_the_supplied_snapshot() {
    let graph = Application::default();
    graph.0.lock().unwrap().contents.insert(1, "before".into());
    let snapshot = InspectionContext::new(&graph)
        .read_snapshot(&Node(1))
        .unwrap();
    let path = graph.node_path(&Node(1));
    graph.0.lock().unwrap().contents.insert(1, "after".into());
    assert_eq!(
        graph.fingerprint(&Node(1), &snapshot.content),
        snapshot.fingerprint
    );
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
    .map(|segments| NodePath {
        segments: segments.into_iter().map(str::to_owned).collect(),
    });
    for (index, path) in paths.iter().enumerate() {
        assert_eq!(*path, path.clone());
        for other in &paths[index + 1..] {
            assert_ne!(path, other);
        }
    }
}

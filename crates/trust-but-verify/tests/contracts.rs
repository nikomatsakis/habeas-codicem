use serde::{Deserialize, Serialize, Serializer};
use serde_json::json;
use std::{
    cell::Cell,
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use trust_but_verify::domain::Domain;
use trust_but_verify::domain::NodeContents;
use trust_but_verify::domain::NodeId;
use trust_but_verify::domain::ValidationCache;
use trust_but_verify::graph as validation;
use trust_but_verify::graph::*;
use trust_but_verify::request::ValidationContext;

// {test}`requirements.sdk.graph.search.source_identity`
// {test}`requirements.sdk.sources.provider.attachment`
#[test]
fn domain_without_project_sources_leaves_source_tools_disabled() {
    assert!(Application::default().project_sources().is_none());
}

// {test}`requirements.sdk.outlines.outline_file`
// {test}`requirements.sdk.outlines.snapshot`
// {test}`requirements.sdk.outlines.outline_file.lifetime`
// {test}`requirements.sdk.outlines.outline_cache`
// {test}`requirements.sdk.outlines.outline_cache.fresh`
// {test}`requirements.sdk.outlines.outline_cache.retry`
// {test}`requirements.sdk.validation-cache.ValidationCache`
// {test}`requirements.sdk.validation-cache.cache`
// {test}`requirements.sdk.validation-cache.new`
// {test}`requirements.sdk.outlines.parser`
#[test]
fn domain_clones_share_the_first_successful_outline_for_each_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lib.rs");
    let cache = ValidationCache::<Application>::new();
    assert!(trust_but_verify::outline::OutlineContent::from_path(&cache, &path).is_err());

    let original = "fn first() {}\nfn incomplete(\n";
    std::fs::write(&path, original).unwrap();
    let snapshot = trust_but_verify::outline::OutlineContent::from_path(&cache, &path).unwrap();
    assert!(snapshot.outline().diagnostic.is_some());
    assert!(snapshot.id().selector.is_empty());
    assert_eq!(snapshot.text(), original);

    std::fs::write(&path, "fn second() {}\n").unwrap();
    let domain = Application(Default::default(), Default::default(), cache);
    let peer = domain.clone();
    let fresh =
        trust_but_verify::outline::OutlineContent::from_path(Application::default().cache(), &path)
            .unwrap();
    assert!(fresh.outline().diagnostic.is_none());
    assert_eq!(fresh.select("fn[second]").unwrap().text(), "fn second() {}");
    let other = dir.path().join("other.rs");
    std::fs::write(&other, "fn other() {}").unwrap();
    assert_eq!(
        trust_but_verify::outline::OutlineContent::from_path(peer.cache(), &other)
            .unwrap()
            .text(),
        "fn other() {}"
    );
    std::fs::remove_file(&other).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        trust_but_verify::outline::OutlineContent::from_path(domain.cache(), &other)
            .unwrap()
            .text(),
        "fn other() {}"
    );
    let id = trust_but_verify::outline::OutlineData::new(&path, "fn[first]".into()).unwrap();
    let selected = trust_but_verify::outline::OutlineContent::from_id(peer.cache(), &id).unwrap();
    assert_eq!(selected.text(), "fn first() {}");
    let cached = trust_but_verify::outline::OutlineContent::from_path(peer.cache(), &path).unwrap();
    assert!(std::ptr::eq(snapshot.outline(), cached.outline()));
    assert_eq!(cached.text(), original);
    assert_eq!(cached.fingerprint(), snapshot.fingerprint());
    assert_eq!(cached.fingerprint_scheme(), "rust-structural-v1");
    assert_eq!(cached.structural_hash(), snapshot.structural_hash());
    assert_ne!(cached.fingerprint(), fresh.fingerprint());
    drop(domain);
    drop(peer);
    let retained = cached.clone();
    drop(cached);
    drop(snapshot);
    assert_eq!(
        retained.select("fn[first]").unwrap().text(),
        "fn first() {}"
    );
}

// {test}`requirements.sdk.graph.snapshot`
#[test]
fn callers_can_pair_content_with_its_fingerprint() {
    let fingerprint = Fingerprint::new("example".into(), "value".into());
    let snapshot = Snapshot::new(vec![1, 2], fingerprint.clone());
    assert_eq!(snapshot.content, [1, 2]);
    assert_eq!(snapshot.fingerprint, fingerprint);
}

// Opaque graph identities and contents deliberately do not implement Serde,
// Hash, or Display. Only completed answers need serialization.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
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
struct Application(
    Arc<Mutex<State>>,
    BTreeMap<u8, NodePath>,
    ValidationCache<Self>,
);

impl Domain for Application {
    fn cache(&self) -> &ValidationCache<Self> {
        &self.2
    }
    type NodeData = Node;
    type EdgeLabel = u8;
    type Error = &'static str;

    fn node_path(&self, node: &NodeId<Self>) -> NodePath {
        let node = &*self.cache().node_data(*node).unwrap();
        self.1
            .get(&node.0)
            .cloned()
            .unwrap_or_else(|| NodePath::new(vec!["application".into(), node.0.to_string()]))
    }
    fn outgoing_edges(
        &self,
        node: &NodeId<Self>,
        label: u8,
    ) -> Result<Vec<NodeId<Self>>, Self::Error> {
        let node = &*self.cache().node_data(*node).unwrap();
        let state = self.0.lock().unwrap();
        if state.fail_edges {
            return Err("edges");
        }
        Ok(state
            .edges
            .get(&(node.0, label))
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|node| self.cache().intern_node(node))
            .collect())
    }

    type NodeContent = Content;

    fn node_content(&self, node: &NodeId<Self>) -> Result<Content, Self::Error> {
        let node = &*self.cache().node_data(*node).unwrap();
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
        node: &NodeId<Self>,
        criterion: &CriterionPath,
    ) -> Result<Option<Record<Node, u8>>, Self::Error> {
        let node = &*self.cache().node_data(*node).unwrap();
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
        node: &NodeId<Self>,
        criterion: &CriterionPath,
        record: &Record<Node, u8>,
    ) -> Result<(), Self::Error> {
        let node = &*self.cache().node_data(*node).unwrap();
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

fn durable_inputs(
    domain: &Application,
    inputs: &[Dependency<NodeId<Application>, u8>],
) -> Vec<Dependency<Node, u8>> {
    inputs
        .iter()
        .map(|entry| match entry {
            Dependency::Node(id, fp) => Dependency::Node(
                (*domain.cache().node_data(*id).unwrap()).clone(),
                fp.clone(),
            ),
            Dependency::OutgoingEdges(id, label, fp) => Dependency::OutgoingEdges(
                (*domain.cache().node_data(*id).unwrap()).clone(),
                *label,
                fp.clone(),
            ),
        })
        .collect()
}

// Cell makes validation values Send but not Sync, exercising the public bounds.
struct Review<O> {
    source: NodeId<Application>,
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
    fn new(domain: &Application, criterion: &[&str], output: O) -> Self {
        Self {
            source: domain.cache().intern_node(Node(1)),
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
    fn source_node_id(&self) -> NodeId<Application> {
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
            cx.read(&cx.graph().cache().intern_node(node.clone()))
                .map_err(ValidationError::Graph)?;
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
    domain: &Application,
    review: Review<O>,
) -> Fingerprinted<Application, O> {
    let value = validation::verify(domain, review).await.unwrap();
    validation::store_evidence(domain, &value).await.unwrap();
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
    #[derive(Clone, PartialEq, Eq, Hash)]
    struct Identity(u8);
    #[derive(Clone)]
    struct Label(u8);
    struct GraphError(Cell<u8>);
    #[derive(Clone)]
    struct Relationships(ValidationCache<Self>);
    impl Domain for Relationships {
        fn cache(&self) -> &ValidationCache<Self> {
            &self.0
        }
        type NodeData = Identity;
        type EdgeLabel = Label;
        type Error = GraphError;
        type NodeContent = Content;

        fn node_content(&self, node: &NodeId<Self>) -> Result<Content, Self::Error> {
            let node = &*self.cache().node_data(*node).unwrap();
            Ok(Content(node.0.to_string()))
        }
        async fn load_evidence(
            &self,
            _: &NodeId<Self>,
            _: &CriterionPath,
        ) -> Result<Option<Record<Identity, Label>>, Self::Error> {
            Ok(None)
        }
        async fn store_evidence(
            &self,
            _: &NodeId<Self>,
            _: &CriterionPath,
            _: &Record<Identity, Label>,
        ) -> Result<(), Self::Error> {
            Ok(())
        }

        fn node_path(&self, node: &NodeId<Self>) -> NodePath {
            let node = &*self.cache().node_data(*node).unwrap();
            NodePath::new(vec!["relationships".into(), node.0.to_string()])
        }

        fn outgoing_edges(
            &self,
            node: &NodeId<Self>,
            label: Label,
        ) -> Result<Vec<NodeId<Self>>, Self::Error> {
            let node = &*self.cache().node_data(*node).unwrap();
            if node.0 == 255 {
                return Err(GraphError(Cell::new(node.0)));
            }
            Ok((match (node.0, label.0) {
                (1, 2) => vec![Identity(3), Identity(2), Identity(3)],
                (1, 3) => vec![Identity(4)],
                _ => vec![],
            })
            .into_iter()
            .map(|data| self.cache().intern_node(data))
            .collect())
        }
    }

    // Compile generic consumer code against the declared bounds, so loosening
    // a required bound fails as well as adding a bound the opaque fixture lacks.
    fn check_bounds<G: Domain>(
        graph: &G,
        node: &NodeId<G>,
        criterion: &CriterionPath,
        record: &Record<G::NodeData, G::EdgeLabel>,
    ) {
        fn domain<T: Clone + Send + Sync>() {}
        fn identity<T: Clone + Eq + Send + Sync>() {}
        fn label<T: Clone + Send + Sync>() {}
        fn error<T: Send>() {}
        fn contents<T: NodeContents + Send + Sync>() {}
        domain::<G>();
        identity::<G::NodeData>();
        label::<G::EdgeLabel>();
        error::<G::Error>();
        contents::<G::NodeContent>();
        drop(send(graph.load_evidence(node, criterion)));
        drop(send(graph.store_evidence(node, criterion, record)));
    }

    fn successors<G: Domain>(graph: &G, node: &NodeId<G>, label: G::EdgeLabel) -> Vec<NodeId<G>> {
        graph.outgoing_edges(node, label).ok().unwrap()
    }
    let graph = Relationships(Default::default());
    assert!(graph.project_sources().is_none());
    let item = trust_but_verify::outline::OutlineData::root("lib.rs");
    assert!(matches!(
        graph.source_node(&item),
        Err(trust_but_verify::sources::SourceError::Invalid(_))
    ));
    let content = graph
        .node_content(&graph.cache().intern_node(Identity(1)))
        .ok()
        .unwrap();
    assert_eq!(content.format(), "1");
    assert_eq!(content.fingerprint().value, "1");
    // Storage futures remain Send even when the application's error is not Sync.
    let criterion = CriterionPath::new(vec!["review".into()]);
    assert!(
        send(graph.load_evidence(&graph.cache().intern_node(Identity(1)), &criterion))
            .await
            .ok()
            .unwrap()
            .is_none()
    );
    let record = Record::new(Some(CriterionVersion::new("v1")), vec![], json!(false));
    check_bounds(
        &graph,
        &graph.cache().intern_node(Identity(1)),
        &criterion,
        &record,
    );
    assert!(
        send(graph.store_evidence(&graph.cache().intern_node(Identity(1)), &criterion, &record))
            .await
            .is_ok()
    );
    assert_eq!(
        graph
            .node_path(&graph.cache().intern_node(Identity(1)))
            .segments(),
        ["relationships", "1"]
    );
    assert_ne!(
        graph.node_path(&graph.cache().intern_node(Identity(1))),
        graph.node_path(&graph.cache().intern_node(Identity(2)))
    );
    assert!(
        successors(&graph, &graph.cache().intern_node(Identity(1)), Label(2))
            == [Identity(3), Identity(2), Identity(3)].map(|n| graph.cache().intern_node(n))
    );
    assert!(
        successors(&graph, &graph.cache().intern_node(Identity(1)), Label(3))
            == [Identity(4)].map(|n| graph.cache().intern_node(n))
    );
    assert!(successors(&graph, &graph.cache().intern_node(Identity(1)), Label(4)).is_empty());
    assert!(successors(&graph, &graph.cache().intern_node(Identity(2)), Label(2)).is_empty());
    let error = graph
        .outgoing_edges(&graph.cache().intern_node(Identity(255)), Label(2))
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
// {test}`requirements.sdk.graph.running-criteria.validate_node`
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

// {test}`requirements.sdk.graph.running-criteria.verify`
// {test}`requirements.sdk.graph.running-criteria.store_evidence`
// {test}`requirements.sdk.graph.running-criteria.load_evidence`
#[tokio::test]
async fn criteria_keep_typed_answers_and_persist_only_when_requested() {
    let domain = Application::default();
    let negative = answer(false, "missing edge case");
    let descriptor = || Review::new(&domain, &["behavior"], negative.clone());
    assert!(
        validation::load_evidence(&domain, &descriptor())
            .await
            .unwrap()
            .is_none()
    );
    let value = validation::verify(&domain, descriptor()).await.unwrap();
    assert_eq!(value.output(), &negative);
    assert_eq!(value.source_node_id(), &domain.cache().intern_node(Node(1)));
    assert_eq!(value.criterion_path(), &path(&["behavior"]));
    assert_eq!(value.version().unwrap().as_str(), "v1");
    assert!(value.dependencies().is_empty());
    {
        let state = domain.0.lock().unwrap();
        assert_eq!((state.executions, state.writes), (1, 0));
    }
    assert!(
        validation::load_evidence(&domain, &descriptor())
            .await
            .unwrap()
            .is_none()
    );
    validation::store_evidence(&domain, &value).await.unwrap();
    let loaded = validation::load_evidence(&domain, &descriptor())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.output(), &negative);
    assert_eq!(loaded.source_node_id(), value.source_node_id());
    assert_eq!(loaded.criterion_path(), value.criterion_path());
    assert_eq!(loaded.version(), value.version());
    assert_eq!(loaded.dependencies(), value.dependencies());
    assert!(validation::check_inputs(&domain, &loaded).unwrap());
    assert_eq!(loaded.into_output(), negative);
    let structure = vec!["both dispatchers use the helper".to_owned()];
    assert_eq!(
        persist(
            &domain,
            Review::new(&domain, &["structure"], structure.clone())
        )
        .await
        .into_output(),
        structure
    );
    // Execution ignores saved records and leaves them unchanged until an explicit save.
    let replacement = validation::verify(
        &domain,
        Review::new(&domain, &["behavior"], answer(true, "new review")),
    )
    .await
    .unwrap();
    assert!(replacement.output().satisfied);
    assert_eq!(
        validation::load_evidence(&domain, &descriptor())
            .await
            .unwrap()
            .unwrap()
            .into_output(),
        negative
    );
    assert!(
        validation::load_evidence(&domain, &Review::new(&domain, &["text"], ()))
            .await
            .unwrap()
            .is_none()
    );
    persist(&domain, Review::new(&domain, &["text"], ()))
        .await
        .into_output();
    validation::load_evidence(&domain, &Review::new(&domain, &["text"], ()))
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
// {test}`requirements.sdk.graph.running-criteria.validate_node`
// {test}`requirements.sdk.graph.evidence.node_inputs`
// {test}`requirements.sdk.graph.evidence.checking_records`
// {test}`requirements.sdk.graph.running-criteria.check_inputs`

#[tokio::test]
async fn changing_one_criteria_input_does_not_invalidate_another() {
    let domain = Application::default();
    domain
        .0
        .lock()
        .unwrap()
        .contents
        .extend([(2, "test".into()), (3, "implementation".into())]);
    let mut behavior = Review::new(&domain, &["behavior"], true);
    behavior.reads = vec![Node(2)];
    persist(&domain, behavior).await;
    let mut structure = Review::new(&domain, &["structure"], true);
    structure.reads = vec![Node(3)];
    persist(&domain, structure).await;
    domain
        .0
        .lock()
        .unwrap()
        .contents
        .insert(2, "changed test".into());
    let behavior = validation::load_evidence(&domain, &Review::new(&domain, &["behavior"], true))
        .await
        .unwrap()
        .unwrap();
    let structure = validation::load_evidence(&domain, &Review::new(&domain, &["structure"], true))
        .await
        .unwrap()
        .unwrap();
    assert!(!validation::check_inputs(&domain, &behavior).unwrap());
    assert!(validation::check_inputs(&domain, &structure).unwrap());
    assert_eq!(domain.0.lock().unwrap().executions, 2);
    let mut replacement = Review::new(&domain, &["behavior"], false);
    replacement.reads = vec![Node(3)];
    let value = persist(&domain, replacement).await;
    assert!(!value.output());
    assert_eq!(
        durable_inputs(&domain, value.dependencies()),
        [Dependency::Node(
            Node(3),
            Fingerprint::new("text-v1".into(), "implementation".into())
        )]
    );
    let saved = domain
        .load_evidence(&domain.cache().intern_node(Node(1)), &path(&["behavior"]))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        saved.dependencies,
        durable_inputs(&domain, value.dependencies())
    );
}

// {test}`requirements.sdk.graph.evidence.store_evidence`
// {test}`requirements.sdk.graph.evidence.record.output`
// {test}`requirements.sdk.graph.evidence.procedure_identity`
// {test}`requirements.sdk.graph.running-criteria.validate_node.errors`
// {test}`requirements.sdk.graph.evidence.check_node`
// {test}`requirements.sdk.graph.evidence.checking_records`
// {test}`requirements.sdk.graph.running-criteria.load_evidence`
#[tokio::test]
async fn typed_loading_reports_decode_errors_even_for_stale_evidence() {
    let domain = Application::default();
    persist(
        &domain,
        Review::new(&domain, &["behavior"], json!({"old":true})),
    )
    .await;
    {
        let mut state = domain.0.lock().unwrap();
        state.records[0].2.dependencies = vec![Dependency::Node(
            Node(99),
            Fingerprint::new("old".into(), "old".into()),
        )];
        state.fail_read = true;
    }
    let mut changed = Review::new(&domain, &["behavior"], String::new());
    changed.revision = "v2".into();
    assert!(matches!(
        validation::load_evidence(&domain, &changed).await,
        Err(ValidationError::Output(_))
    ));
    assert!(domain.0.lock().unwrap().reads.is_empty());
    assert!(matches!(
        validation::load_evidence(&domain, &Review::new(&domain, &["behavior"], String::new()))
            .await,
        Err(ValidationError::Output(_))
    ));
    // No saved-input resolution or output decoding happens during a new execution.
    changed.output = "new answer".into();
    let value = validation::verify(&domain, changed).await.unwrap();
    assert_eq!(value.output(), "new answer");
    assert_eq!(value.version().unwrap().as_str(), "v2");
    {
        let state = domain.0.lock().unwrap();
        assert_eq!((state.executions, state.writes), (2, 1));
        assert!(state.reads.is_empty());
        assert_eq!(state.records[0].2.version.as_ref().unwrap().as_str(), "v1");
    }
    validation::store_evidence(&domain, &value).await.unwrap();
    assert_eq!(domain.0.lock().unwrap().records.len(), 1);
}

// {test}`requirements.sdk.graph.evidence.store_evidence`
// {test}`requirements.sdk.graph.evidence.record.output`
// {test}`requirements.sdk.graph.running-criteria.validate_node`
// {test}`requirements.sdk.graph.evidence.record`
// {test}`requirements.sdk.graph.evidence.checking_records`
// {test}`requirements.sdk.graph.running-criteria.check_inputs`
// {test}`requirements.sdk.graph.running-criteria.verify`
#[tokio::test]
async fn input_checking_and_execution_keep_separate_observations() {
    let domain = Application::default();
    domain.0.lock().unwrap().contents.insert(2, "before".into());
    let mut old = Review::new(&domain, &["behavior"], json!({"old":true}));
    old.reads = vec![Node(2)];
    persist(&domain, old).await;
    {
        let mut state = domain.0.lock().unwrap();
        state.contents.insert(2, "after".into());
        state.reads.clear();
    }
    let loaded =
        validation::load_evidence(&domain, &Review::new(&domain, &["behavior"], json!(null)))
            .await
            .unwrap()
            .unwrap();
    assert!(!validation::check_inputs(&domain, &loaded).unwrap());
    assert!(matches!(
        validation::load_evidence(&domain, &Review::new(&domain, &["behavior"], String::new()))
            .await,
        Err(ValidationError::Output(_))
    ));
    domain.0.lock().unwrap().reads.clear();
    let mut new = Review::new(&domain, &["behavior"], "new answer".to_owned());
    new.reads = vec![Node(2)];
    let value = persist(&domain, new).await;
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
// {test}`requirements.sdk.graph.running-criteria.check_inputs`
// {test}`requirements.sdk.graph.running-criteria.store_evidence`
#[tokio::test]
async fn dependency_resolution_failures_do_not_execute_or_replace_saved_answers() {
    let domain = Application::default();
    domain.0.lock().unwrap().contents.insert(2, "input".into());
    let mut review = Review::new(&domain, &["behavior"], false);
    review.reads = vec![Node(2)];
    review.label = Some(7);
    let value = persist(&domain, review).await;
    let expected = [
        Dependency::Node(Node(2), Fingerprint::new("text-v1".into(), "input".into())),
        Dependency::OutgoingEdges(Node(1), 7, edge_fingerprint(&domain, &[])),
    ];
    assert_eq!(durable_inputs(&domain, value.dependencies()), expected);
    assert_eq!(validation::check_inputs(&domain, &value), Ok(true));
    domain.0.lock().unwrap().edges.insert((1, 7), vec![Node(3)]);
    assert_eq!(validation::check_inputs(&domain, &value), Ok(false));
    for (read, edges, message) in [(true, false, "read"), (false, true, "edges")] {
        {
            let mut state = domain.0.lock().unwrap();
            state.fail_read = read;
            state.fail_edges = edges;
        }
        // Loading is independent of input resolution, even when inputs cannot resolve.
        let loaded =
            validation::load_evidence(&domain, &Review::new(&domain, &["behavior"], false))
                .await
                .unwrap()
                .unwrap();
        assert_eq!(durable_inputs(&domain, loaded.dependencies()), expected);
        assert_eq!(validation::check_inputs(&domain, &loaded), Err(message));
        let state = domain.0.lock().unwrap();
        assert_eq!((state.executions, state.writes), (1, 1));
        assert_eq!(
            state.records[0].2.dependencies,
            durable_inputs(&domain, value.dependencies())
        );
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
    persist(&domain, Review::new(&domain, &[], false)).await;
    domain.0.lock().unwrap().records[0].2.version = None;
    let mut future = Review::new(&domain, &[], true);
    future.revision = "future".into();
    let loaded = validation::load_evidence(&domain, &future)
        .await
        .unwrap()
        .unwrap();
    assert!(!loaded.output());
    assert_eq!(loaded.version(), None);
    assert!(future.version().is_compatible_with(loaded.version()));
    assert!(validation::check_inputs(&domain, &loaded).unwrap());
    assert!(
        validation::load_evidence(&domain, &Review::new(&domain, &["new"], false))
            .await
            .unwrap()
            .is_none()
    );
    future.source = domain.cache().intern_node(Node(2));
    assert!(
        validation::load_evidence(&domain, &future)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(domain.0.lock().unwrap().executions, 1);
}

// {test}`requirements.sdk.graph.criteria.criterion_trait`
// {test}`requirements.sdk.graph.evidence.check_node`
// {test}`requirements.sdk.graph.running-criteria.validate_node`

// {test}`requirements.sdk.graph.evidence.store_evidence`
// {test}`requirements.sdk.graph.evidence.refresh_evidence`
// {test}`requirements.sdk.graph.criteria.criterion_trait.answer`
// {test}`requirements.sdk.graph.criteria.execute`
// {test}`requirements.sdk.graph.running-criteria.verify`
#[tokio::test]
async fn validation_and_answer_need_send_without_sync() {
    let domain = Application::default();
    let value = send(validation::verify(
        &domain,
        Review::new(&domain, &["cell"], Cell::new(17u32)),
    ))
    .await
    .unwrap();
    assert_eq!(value.output().get(), 17);
    send(validation::store_evidence(&domain, &value))
        .await
        .unwrap();
    let descriptor = Review::new(&domain, &["cell"], Cell::new(0u32));
    let loaded = send(validation::load_evidence(&domain, &descriptor))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.into_output().get(), 17);
    assert_eq!(descriptor.descriptor_reads.get(), 1);
    assert_eq!(domain.0.lock().unwrap().executions, 1);
    let fresh = send(validation::refresh_evidence(
        &domain,
        Review::new(&domain, &["refresh"], Cell::new(23u32)),
    ))
    .await
    .unwrap();
    assert_eq!(fresh.output().get(), 23);
    let reused = send(validation::refresh_evidence(
        &domain,
        Review::new(&domain, &["refresh"], Cell::new(0u32)),
    ))
    .await
    .unwrap();
    assert_eq!(reused.into_output().get(), 23);
    let state = domain.0.lock().unwrap();
    assert_eq!((state.executions, state.writes), (2, 2));
}

// {test}`requirements.sdk.graph.evidence.refresh_evidence`
// {test}`requirements.sdk.graph.evidence.checking_records.version_match`
// {test}`requirements.sdk.graph.running-criteria.refresh_evidence`
#[tokio::test]
async fn refresh_reuses_current_positive_negative_and_legacy_answers_without_saving() {
    for satisfied in [true, false] {
        for revision in ["v1", "*"] {
            let domain = Application::default();
            domain.0.lock().unwrap().contents.insert(2, "input".into());
            let mut first = Review::new(&domain, &["behavior"], answer(satisfied, "saved answer"));
            first.reads = vec![Node(2)];
            let saved = validation::refresh_evidence(&domain, first).await.unwrap();
            domain.0.lock().unwrap().records[0].2.version =
                CriterionVersion::from_recorded(revision);

            let mut next = Review::new(&domain, &["behavior"], answer(!satisfied, "unused answer"));
            next.failure = Some("must reuse");
            let reused = validation::refresh_evidence(&domain, next).await.unwrap();
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
// {test}`requirements.sdk.graph.running-criteria.refresh_evidence`
// {test}`requirements.sdk.graph.running-criteria.evaluate_criterion`
// {test}`requirements.sdk.graph.evidence.evidence_storage`
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
        if cause != "missing" {
            let mut old = Review::new(&domain, &["behavior"], "old answer".to_owned());
            if cause == "successors" {
                old.label = Some(7);
            } else {
                old.reads = vec![Node(2)];
            }
            persist(&domain, old).await;
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
        let mut next = Review::new(&domain, &["behavior"], "new answer".to_owned());
        next.reads = vec![Node(1)];
        if cause == "revision" {
            next.revision = "v2".into();
        }
        let replacement = validation::refresh_evidence(&domain, next).await.unwrap();
        assert_eq!(replacement.output(), "new answer");
        assert_eq!(
            durable_inputs(&domain, replacement.dependencies()),
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
        assert_eq!(
            state.records[0].2.dependencies,
            durable_inputs(&domain, replacement.dependencies())
        );
        assert_eq!(state.records[0].2.version.as_ref(), replacement.version());
    }
}

// {test}`requirements.sdk.graph.evidence.refresh_evidence`
// {test}`requirements.sdk.graph.evidence.record.output`
// {test}`requirements.sdk.graph.running-criteria.ValidationError`
// {test}`requirements.sdk.graph.evidence.checking_records.version_match`
#[tokio::test]
async fn refresh_propagates_load_decode_check_execution_and_save_errors() {
    for stage in [
        "load",
        "decode",
        "stale decode",
        "unversioned decode",
        "read",
        "edges",
        "execute",
        "store",
    ] {
        let domain = Application::default();
        domain.0.lock().unwrap().contents.insert(2, "input".into());
        let mut old = Review::new(&domain, &[], true);
        if stage == "read" {
            old.reads = vec![Node(2)];
        } else if stage == "edges" {
            old.label = Some(7);
        }
        persist(&domain, old).await;
        let mut next = Review::new(&domain, &[], false);
        {
            let mut state = domain.0.lock().unwrap();
            match stage {
                "load" => state.fail_load = true,
                "decode" | "stale decode" => state.records[0].2.output = json!("not a boolean"),
                "unversioned decode" => {
                    state.records[0].2.version = None;
                    state.records[0].2.output = json!("not a boolean");
                }
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
        let error = validation::refresh_evidence(&domain, next)
            .await
            .err()
            .unwrap();
        match (stage, error) {
            ("decode" | "stale decode" | "unversioned decode", ValidationError::Output(_)) => {}
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
// {test}`requirements.sdk.graph.running-criteria.ValidationError`
#[tokio::test]
async fn refresh_serialization_failure_preserves_the_saved_record() {
    let domain = Application::default();
    // A unit struct decodes from null, but this criterion's new answer cannot encode.
    domain.0.lock().unwrap().records.push((
        Node(1),
        path(&[]),
        Record::new(Some(CriterionVersion::new("v1")), vec![], json!(null)),
    ));
    let mut next = Review::new(&domain, &[], Unencodable);
    next.revision = "v2".into();
    assert!(matches!(
        validation::refresh_evidence(&domain, next).await,
        Err(ValidationError::Output(_))
    ));
    let state = domain.0.lock().unwrap();
    assert_eq!((state.executions, state.writes), (1, 0));
    assert_eq!(state.records.len(), 1);
    assert_eq!(state.records[0].2.version.as_ref().unwrap().as_str(), "v1");
    assert_eq!(state.records[0].2.output, json!(null));
}

// {test}`requirements.sdk.graph.evidence.refresh_evidence`

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
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mut first = Review::new(&domain, &[], "first".to_owned());
    first.revision = "first".into();
    first.reads = vec![Node(2)];
    first.barrier = Some(barrier.clone());
    let mut second = Review::new(&domain, &[], "second".to_owned());
    second.revision = "second".into();
    second.reads = vec![Node(3)];
    second.barrier = Some(barrier);
    let (a, b) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        futures::join!(
            validation::refresh_evidence(&domain, first),
            validation::refresh_evidence(&domain, second)
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
    assert_eq!(
        saved.dependencies,
        durable_inputs(&domain, winner.dependencies())
    );
}

// {test}`requirements.sdk.graph.validation-context.root_inputs`
// {test}`requirements.sdk.graph.evidence.checking_records`
#[tokio::test]
async fn absence_certification_is_invalidated_by_the_first_selected_root() {
    let domain = Application::default();
    for (name, label) in [("behavior", 1), ("structure", 2)] {
        let mut review = Review::new(&domain, &[name], true);
        review.label = Some(label);
        persist(&domain, review).await;
    }
    domain.0.lock().unwrap().edges.insert((1, 1), vec![Node(2)]);
    let behavior = validation::load_evidence(&domain, &Review::new(&domain, &["behavior"], true))
        .await
        .unwrap()
        .unwrap();
    let structure = validation::load_evidence(&domain, &Review::new(&domain, &["structure"], true))
        .await
        .unwrap()
        .unwrap();
    assert!(!validation::check_inputs(&domain, &behavior).unwrap());
    assert!(validation::check_inputs(&domain, &structure).unwrap());
    assert!(domain.0.lock().unwrap().reads.is_empty());
}

#[derive(Deserialize)]
struct Unencodable;
impl Serialize for Unencodable {
    fn serialize<S: Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("cannot encode answer"))
    }
}

// {test}`requirements.sdk.graph.running-criteria.validate_node.errors`
// {test}`requirements.sdk.graph.evidence.store_evidence`
// {test}`requirements.sdk.graph.evidence.record.output`
// {test}`requirements.sdk.graph.running-criteria.ValidationError`
#[tokio::test]
async fn serialization_failure_leaves_the_existing_record_unchanged() {
    let domain = Application::default();
    persist(&domain, Review::new(&domain, &[], "old answer".to_owned())).await;
    let before = domain
        .load_evidence(&domain.cache().intern_node(Node(1)), &path(&[]))
        .await
        .unwrap()
        .unwrap();
    let mut new = Review::new(&domain, &[], Unencodable);
    new.revision = "v2".into();
    let value = validation::verify(&domain, new).await.unwrap();
    let error = validation::store_evidence(&domain, &value)
        .await
        .err()
        .unwrap();
    assert!(matches!(error, ValidationError::Output(_)));
    assert!(error.to_string().contains("cannot encode answer"));
    let after = domain
        .load_evidence(&domain.cache().intern_node(Node(1)), &path(&[]))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(before.version, after.version);
    assert_eq!(before.dependencies, after.dependencies);
    assert_eq!(before.output, after.output);
    assert_eq!(domain.0.lock().unwrap().writes, 1);
}

// {test}`requirements.sdk.graph.running-criteria.validate_node.errors`
// {test}`requirements.sdk.graph.evidence.check_node`
// {test}`requirements.sdk.graph.evidence.record.output`
#[tokio::test]
async fn loading_execution_resolution_and_saving_failures_do_not_publish_answers() {
    let domain = Application::default();
    domain.0.lock().unwrap().fail_load = true;
    assert!(matches!(
        validation::load_evidence(&domain, &Review::new(&domain, &[], true)).await,
        Err(ValidationError::Graph("load"))
    ));
    let value = validation::verify(&domain, Review::new(&domain, &[], true))
        .await
        .unwrap();
    assert!(value.output());
    domain.0.lock().unwrap().fail_load = false;
    let mut review = Review::new(&domain, &[], true);
    review.failure = Some("execute");
    assert!(matches!(
        validation::verify(&domain, review).await,
        Err(ValidationError::Graph("execute"))
    ));
    let mut review = Review::new(&domain, &[], true);
    review.reads = vec![Node(99)];
    assert!(matches!(
        validation::verify(&domain, review).await,
        Err(ValidationError::Graph("missing"))
    ));
    let mut review = Review::new(&domain, &[], true);
    review.label = Some(1);
    domain.0.lock().unwrap().fail_edges = true;
    assert!(matches!(
        validation::verify(&domain, review).await,
        Err(ValidationError::Graph("edges"))
    ));
    domain.0.lock().unwrap().fail_store = true;
    assert!(matches!(
        validation::store_evidence(&domain, &value).await,
        Err(ValidationError::Graph("store"))
    ));
    let state = domain.0.lock().unwrap();
    assert!(state.records.is_empty());
    assert_eq!(state.writes, 0);
}

// {test}`requirements.sdk.graph.evidence.evidence_storage`
// {test}`requirements.sdk.domain`
// {test}`requirements.sdk.graph.evidence.store_evidence`
// {test}`requirements.sdk.graph.running-criteria.verify.concurrent`
#[tokio::test]
async fn concurrent_same_criterion_executions_keep_complete_independent_records() {
    let domain = Application::default();
    domain
        .0
        .lock()
        .unwrap()
        .contents
        .extend([(2, "first input".into()), (3, "second input".into())]);
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mut first = Review::new(&domain, &["behavior"], "first".to_owned());
    first.revision = "first-revision".into();
    first.reads = vec![Node(2)];
    first.barrier = Some(barrier.clone());
    let mut second = Review::new(&domain, &["behavior"], "second".to_owned());
    second.revision = "second-revision".into();
    second.reads = vec![Node(3)];
    second.barrier = Some(barrier);
    let (a, b) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        futures::join!(
            validation::verify(&domain, first),
            validation::verify(&domain, second)
        )
    })
    .await
    .expect("same-criterion executions must not wait for each other");
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(a.output(), "first");
    assert_eq!(b.output(), "second");
    assert!(domain.0.lock().unwrap().records.is_empty());
    let (saved_a, saved_b) = futures::join!(
        validation::store_evidence(&domain, &a),
        validation::store_evidence(&domain, &b)
    );
    saved_a.unwrap();
    saved_b.unwrap();
    let saved = domain
        .load_evidence(&domain.cache().intern_node(Node(1)), &path(&["behavior"]))
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
    let snapshot = ValidationContext::new(&graph, graph.cache().intern_node(Node(1)))
        .read_snapshot(&graph.cache().intern_node(Node(1)))
        .unwrap();
    assert_eq!(snapshot.content.format(), "before");
    let path = graph.node_path(&graph.cache().intern_node(Node(1)));
    graph.0.lock().unwrap().contents.insert(1, "after".into());
    assert_eq!(snapshot.content.fingerprint(), snapshot.fingerprint);
    assert_eq!(snapshot.content.format(), "before");
    assert_eq!(graph.0.lock().unwrap().reads, [1]);
    assert_eq!(snapshot.fingerprint.value, "before");
    assert_eq!(graph.node_path(&graph.cache().intern_node(Node(1))), path);
    let fresh = Application::default();
    assert_eq!(fresh.node_path(&fresh.cache().intern_node(Node(1))), path);
    assert_ne!(graph.node_path(&graph.cache().intern_node(Node(2))), path);
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
        vec!["a"],
        vec!["b"],
    ];
    for (index, segments) in paths.into_iter().enumerate() {
        domain.1.insert(
            index as u8,
            NodePath::new(segments.into_iter().map(str::to_owned).collect()),
        );
    }
    assert_ne!(
        edge_fingerprint(&domain, &[domain.cache().intern_node(Node(1))]),
        edge_fingerprint(
            &domain,
            &[
                domain.cache().intern_node(Node(11)),
                domain.cache().intern_node(Node(12))
            ]
        )
    );
    let empty = edge_fingerprint(&domain, &[]);
    assert_eq!(
        empty,
        Fingerprint::new(
            "node-path-set-sha256-v1".into(),
            "4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945".into()
        )
    );
    let fingerprints = (0..domain.1.len())
        .map(|index| edge_fingerprint(&domain, &[domain.cache().intern_node(Node(index as u8))]))
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
        edge_fingerprint(
            &domain,
            &[
                domain.cache().intern_node(Node(0)),
                domain.cache().intern_node(Node(1)),
                domain.cache().intern_node(Node(0))
            ]
        ),
        edge_fingerprint(
            &domain,
            &[
                domain.cache().intern_node(Node(1)),
                domain.cache().intern_node(Node(0))
            ]
        )
    );
    assert!(domain.0.lock().unwrap().reads.is_empty());
}

// {test}`requirements.sdk.validation-cache.shared_node_ids`
// {test}`requirements.sdk.validation-cache.intern_node`
// {test}`requirements.sdk.validation-cache.node_data`
#[test]
fn clones_interning_the_same_data_concurrently_get_one_handle() {
    let cache = ValidationCache::<Application>::new();
    let start = std::sync::Barrier::new(4);
    let ids = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let cache = cache.clone();
                let start = &start;
                scope.spawn(move || {
                    start.wait();
                    cache.intern_node(Node(42))
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(ids.iter().all(|id| *id == ids[0]));
    assert_eq!(
        format!("{:?}", ids[0]),
        serde_json::to_string(&ids[0]).unwrap()
    );
    assert_eq!(cache.node_data(ids[0]).as_deref(), Some(&Node(42)));
    assert_ne!(cache.intern_node(Node(43)), ids[0]);
}

// {test}`requirements.sdk.graph.NodeData`
// {test}`requirements.sdk.graph.NodeId`
// {test}`requirements.sdk.validation-cache.intern_node`
// {test}`requirements.sdk.validation-cache.node_data`
// {test}`requirements.sdk.validation-cache.shared_node_ids`
// {test}`requirements.sdk.graph.evidence.persisted_dependencies`
// {test}`requirements.sdk.validation-cache.cache`
// {test}`requirements.sdk.graph.validation-context.snapshot`
// {test}`requirements.sdk.graph.evidence.reassigned-handles`
#[tokio::test]
async fn saved_node_and_edge_inputs_survive_a_different_handle_allocation_order() {
    let first = Application::default();
    let mut review = Review::new(&first, &["review"], true);
    let subject = review.source;
    let target = first.cache().intern_node(Node(2));
    assert_eq!(first.clone().cache().intern_node(Node(1)), subject);
    {
        let mut state = first.0.lock().unwrap();
        state.contents.insert(2, "current".into());
        state.edges.insert((1, 7), vec![Node(2)]);
    }
    review.reads.push(Node(2));
    review.label = Some(7);
    let observed = persist(&first, review).await;
    let expected = durable_inputs(&first, observed.dependencies());
    assert_eq!(first.0.lock().unwrap().records[0].2.dependencies, expected);
    let fresh = Application(first.0.clone(), first.1.clone(), ValidationCache::new());
    assert!(fresh.cache().node_data(subject).is_none());
    let fresh_target = fresh.cache().intern_node(Node(2));
    let saved = first.0.lock().unwrap().records[0].2.clone();
    assert!(
        saved
            .is_current(&fresh, &CriterionVersion::new("v1"))
            .unwrap()
    );
    let descriptor = Review::new(&fresh, &["review"], true);
    assert_ne!(fresh_target, target);
    let loaded = validation::load_evidence(&fresh, &descriptor)
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(loaded.dependencies(), [Dependency::Node(n, _), Dependency::OutgoingEdges(s, 7, _)] if *n == fresh_target && *s == descriptor.source)
    );
    assert_eq!(durable_inputs(&fresh, loaded.dependencies()), expected);
    assert!(validation::check_inputs(&fresh, &loaded).unwrap());
    fresh.0.lock().unwrap().edges.clear();
    assert!(!validation::check_inputs(&fresh, &loaded).unwrap());
}

// {test}`requirements.sdk.graph.NodeId`
#[test]
fn node_handles_round_trip_as_unsigned_integers_for_any_domain() {
    fn check<D: Domain>(node: NodeId<D>) {
        let encoded = serde_json::to_value(node).unwrap();
        assert!(encoded.is_u64());
        let decoded: NodeId<D> = serde_json::from_value(encoded).unwrap();
        assert_eq!(node, decoded);
        assert!(std::collections::HashSet::from([node]).contains(&decoded));
        assert!(serde_json::from_value::<NodeId<D>>(json!(-1)).is_err());
        let schema = serde_json::to_value(schemars::schema_for!(NodeId<D>)).unwrap();
        assert_eq!(schema["type"], "integer");
        assert_eq!(schema["minimum"].as_f64(), Some(0.0));
    }
    check(Application::default().cache().intern_node(Node(3)));
}

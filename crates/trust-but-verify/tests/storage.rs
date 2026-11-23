use serde::{Serialize, Serializer};
use serde_json::json;
use std::sync::{
    Arc, Barrier,
    atomic::{AtomicUsize, Ordering},
};
use trust_but_verify::domain::Domain;
use trust_but_verify::domain::NodeContents;
use trust_but_verify::graph::validator::Validator;
use trust_but_verify::graph::*;
use trust_but_verify::request::ValidationContext;
use trust_but_verify::storage::*;

fn criterion() -> CriterionPath {
    CriterionPath::new(vec![])
}

fn record(id: &str) -> Record<String> {
    Record {
        revision: "v1".into(),
        dependencies: vec![
            Dependency::Node(
                "source".into(),
                Fingerprint {
                    scheme: "sha-v1".into(),
                    value: "a".repeat(64),
                },
            ),
            Dependency::OutgoingEdges(
                "subject".into(),
                (),
                Fingerprint {
                    scheme: "edges-v1".into(),
                    value: "ordered targets".into(),
                },
            ),
        ],
        output: json!({"result": false, "rationale": "a negative review", "metadata": {"id": id, "padding": "x".repeat(8192)}}),
    }
}

// {test}`requirements.sdk.sources.redaction`
// {test}`requirements.sdk.filesystem-storage.store`
// {test}`requirements.sdk.filesystem-storage.load`
#[tokio::test]
async fn storage_preserves_completed_answers_containing_credential_shaped_text() {
    let temp = tempfile::tempdir().unwrap();
    let store = FileEvidenceStorage::new(temp.path());
    let path = NodePath::new(vec!["answer".into()]);
    let mut answer = record("unmodified");
    // A synthetic token that the model-input scanner recognizes. Completed
    // application answers must still round-trip exactly through evidence storage.
    let token = format!("ghp_{}", "a".repeat(36));
    answer.output = json!({"quoted_input": token, "satisfied": false});
    store.store(&path, &criterion(), &answer).await.unwrap();
    let saved = store
        .load::<String, ()>(&path, &criterion())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.output, answer.output);
    let bytes = std::fs::read_to_string(store.record_path(&path, &criterion())).unwrap();
    assert!(bytes.contains(&token));
}

// {test}`requirements.sdk.filesystem-storage.interface`
// {test}`requirements.sdk.filesystem-storage.keys`
// {test}`requirements.sdk.filesystem-storage.load`
#[tokio::test]
async fn paths_are_injective_and_loading_missing_records_has_no_side_effects() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("missing");
    let store = FileEvidenceStorage::new(&root);
    let cases = [
        (vec![], "criteria/record.json"),
        (vec![""], "s/criteria/record.json"),
        (vec!["a", "b"], "s61/s62/criteria/record.json"),
        (vec!["a/b"], "s612f62/criteria/record.json"),
        (vec![".."], "s2e2e/criteria/record.json"),
        (vec!["\0"], "s00/criteria/record.json"),
        (vec!["\t\n\r\x0f"], "s090a0d0f/criteria/record.json"),
        (vec!["\x01#"], "s0123/criteria/record.json"),
        (vec!["\x12\x03"], "s1203/criteria/record.json"),
        (vec!["é"], "sc3a9/criteria/record.json"),
        (vec!["e\u{301}"], "s65cc81/criteria/record.json"),
    ];
    for (segments, expected) in cases {
        let path = NodePath::new(segments.iter().map(|s| s.to_string()).collect());
        assert_eq!(store.record_path(&path, &criterion()), root.join(expected));
        assert!(
            store
                .load::<String, ()>(&path, &criterion())
                .await
                .unwrap()
                .is_none()
        );
    }
    assert!(!root.exists());
}

// {test}`requirements.sdk.graph.evidence.evidence_storage.filesystem`
// {test}`requirements.sdk.graph.validation.criterion_path`
// {test}`requirements.sdk.graph.evidence.evidence_storage`
// {test}`requirements.sdk.filesystem-storage.keys`
// {test}`requirements.sdk.filesystem-storage.store`
// {test}`requirements.sdk.filesystem-storage.load`
#[tokio::test]
async fn source_and_criterion_boundaries_preserve_independent_records() {
    let temp = tempfile::tempdir().unwrap();
    let store = FileEvidenceStorage::new(temp.path());
    let pairs = [
        (
            vec!["a"],
            vec!["b", "c"],
            "s61/criteria/s62/s63/record.json",
        ),
        (
            vec!["a", "b"],
            vec!["c"],
            "s61/s62/criteria/s63/record.json",
        ),
        (vec![], vec![], "criteria/record.json"),
        (vec![], vec![""], "criteria/s/record.json"),
        (vec![""], vec![], "s/criteria/record.json"),
        (
            vec!["criteria"],
            vec![],
            "s6372697465726961/criteria/record.json",
        ),
        (
            vec!["a"],
            vec!["../é"],
            "s61/criteria/s2e2e2fc3a9/record.json",
        ),
        (vec!["\x01#"], vec!["\0"], "s0123/criteria/s00/record.json"),
        (
            vec!["\x12\x03"],
            vec!["\0"],
            "s1203/criteria/s00/record.json",
        ),
        (vec!["\0"], vec!["\x01#"], "s00/criteria/s0123/record.json"),
        (
            vec!["\0"],
            vec!["\x12\x03"],
            "s00/criteria/s1203/record.json",
        ),
    ];
    for (index, (source, segments, expected)) in pairs.iter().enumerate() {
        let source = NodePath::new(source.iter().map(|s| (*s).into()).collect());
        let criterion = CriterionPath::new(segments.iter().map(|s| (*s).into()).collect());
        assert_eq!(
            store.record_path(&source, &criterion),
            temp.path().join(expected)
        );
        assert!(
            store
                .load::<String, ()>(&source, &criterion)
                .await
                .unwrap()
                .is_none()
        );
        store
            .store(&source, &criterion, &record(&index.to_string()))
            .await
            .unwrap();
        let entries = std::fs::read_dir(store.record_path(&source, &criterion).parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(entries, vec![std::ffi::OsString::from("record.json")]);
    }
    for (index, (source, segments, _)) in pairs.iter().enumerate() {
        let source = NodePath::new(source.iter().map(|s| (*s).into()).collect());
        let criterion = CriterionPath::new(segments.iter().map(|s| (*s).into()).collect());
        let saved = store
            .load::<String, ()>(&source, &criterion)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.output, record(&index.to_string()).output);
    }
}

// {test}`requirements.sdk.filesystem-storage.interface`
// {test}`requirements.sdk.filesystem-storage.load`
// {test}`requirements.sdk.filesystem-storage.store`
// {test}`requirements.sdk.graph.evidence.record`
// {test}`requirements.sdk.graph.evidence.dependencies`
// {test}`requirements.sdk.graph.fingerprint`
#[tokio::test]
async fn records_round_trip_and_malformed_or_incompatible_data_is_an_error() {
    let temp = tempfile::tempdir().unwrap();
    let store = FileEvidenceStorage::new(temp.path());
    let path = NodePath::new(vec!["node".into()]);
    store
        .store(&path, &criterion(), &record("A"))
        .await
        .unwrap();
    let saved = store
        .load::<String, ()>(&path, &criterion())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.revision, "v1");
    assert_eq!(saved.dependencies, record("A").dependencies);
    assert_eq!(saved.output, record("A").output);
    assert!(matches!(
        store.load::<u8, ()>(&path, &criterion()).await,
        Err(FileStorageError::Json(_))
    ));
    let labeled = Record {
        revision: saved.revision.clone(),
        dependencies: vec![Dependency::OutgoingEdges(
            "subject".to_owned(),
            "tests/é".to_owned(),
            Fingerprint {
                scheme: "targets-v1".into(),
                value: "preserved".into(),
            },
        )],
        output: saved.output.clone(),
    };
    store.store(&path, &criterion(), &labeled).await.unwrap();
    let loaded = store
        .load::<String, String>(&path, &criterion())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.revision, labeled.revision);
    assert_eq!(loaded.dependencies, labeled.dependencies);
    assert_eq!(loaded.output, labeled.output);
    assert!(matches!(
        store.load::<String, u8>(&path, &criterion()).await,
        Err(FileStorageError::Json(_))
    ));
    std::fs::write(store.record_path(&path, &criterion()), b"not JSON").unwrap();
    let error = store
        .load::<String, ()>(&path, &criterion())
        .await
        .err()
        .unwrap();
    let FileStorageError::Json(cause) = &error else {
        panic!("expected JSON error")
    };
    assert!(!error.to_string().is_empty());
    assert_eq!(error.to_string(), cause.to_string());
    std::fs::remove_file(store.record_path(&path, &criterion())).unwrap();
    std::fs::create_dir(store.record_path(&path, &criterion())).unwrap();
    let error = store
        .load::<String, ()>(&path, &criterion())
        .await
        .err()
        .unwrap();
    let FileStorageError::Io(cause) = &error else {
        panic!("expected I/O error")
    };
    assert_eq!(error.to_string(), cause.to_string());
}

// {test}`requirements.sdk.filesystem-storage.interface`
// {test}`requirements.sdk.filesystem-storage.store`
// {test}`requirements.sdk.filesystem-storage.keys`
// {test}`requirements.sdk.filesystem-storage.load`
#[tokio::test]
async fn send_futures_create_missing_roots_and_nested_paths_without_shortening_keys() {
    fn send<T: Send>(value: T) -> T {
        value
    }
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("missing").join("root");
    let store = FileEvidenceStorage::new(&root);
    let path = NodePath::new(vec!["parent".into(), "".into(), "child".into()]);
    assert!(!root.exists());
    let record = record("nested");
    send(store.store(&path, &criterion(), &record))
        .await
        .unwrap();
    assert!(store.record_path(&path, &criterion()).is_file());
    assert_eq!(
        send(store.load::<String, ()>(&path, &criterion()))
            .await
            .unwrap()
            .unwrap()
            .output,
        record.output
    );
    // The encoded component exceeds common filesystem name limits. It must
    // fail rather than silently hash or truncate the consumer's key.
    let too_long = NodePath::new(vec!["x".repeat(4096)]);
    assert!(matches!(
        store.load::<String, ()>(&too_long, &criterion()).await,
        Err(FileStorageError::Io(_))
    ));
    assert!(matches!(
        store.store(&too_long, &criterion(), &record).await,
        Err(FileStorageError::Io(_))
    ));
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
}

// {cite}`habeas:util`
struct BadNode;
impl Serialize for BadNode {
    // {cite}`habeas:util`
    fn serialize<S: Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("cannot serialize"))
    }
}

// {test}`requirements.sdk.filesystem-storage.store`
#[tokio::test]
async fn failed_serialization_preserves_records_and_failed_replacement_cleans_up() {
    let temp = tempfile::tempdir().unwrap();
    let store = FileEvidenceStorage::new(temp.path());
    let path = NodePath::new(Vec::new());
    store
        .store(&path, &criterion(), &record("original"))
        .await
        .unwrap();
    let before = std::fs::read(store.record_path(&path, &criterion())).unwrap();
    let bad: Record<BadNode> = Record {
        revision: "v2".into(),
        dependencies: vec![Dependency::Node(
            BadNode,
            Fingerprint {
                scheme: "fail".into(),
                value: "fail".into(),
            },
        )],
        output: json!("new"),
    };
    assert!(matches!(
        store.store(&path, &criterion(), &bad).await,
        Err(FileStorageError::Json(_))
    ));
    assert_eq!(
        std::fs::read(store.record_path(&path, &criterion())).unwrap(),
        before
    );
    std::fs::remove_file(store.record_path(&path, &criterion())).unwrap();
    std::fs::create_dir(store.record_path(&path, &criterion())).unwrap();
    assert!(matches!(
        store
            .store(&path, &criterion(), &record("replacement"))
            .await,
        Err(FileStorageError::Io(_))
    ));
    let names: Vec<_> = std::fs::read_dir(store.record_path(&path, &criterion()).parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, vec!["record.json"]);
}

// {test}`requirements.sdk.filesystem-storage.store`
// {test}`requirements.sdk.filesystem-storage.interface`
// {test}`requirements.sdk.graph.evidence.evidence_storage`
#[test]
fn concurrent_readers_only_observe_complete_records_from_competing_writes() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(FileEvidenceStorage::new(temp.path()));
    let path = NodePath::new(Vec::new());
    futures::executor::block_on(store.store(&path, &criterion(), &record("A"))).unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let finished = Arc::new(AtomicUsize::new(0));
    std::thread::scope(|scope| {
        for id in ["A", "B"] {
            let store = store.clone();
            let barrier = barrier.clone();
            let finished = finished.clone();
            let path = &path;
            scope.spawn(move || {
                barrier.wait();
                for _ in 0..30 {
                    let mut value = record(id);
                    value.revision = id.into();
                    value.dependencies = vec![Dependency::Node(
                        id.into(),
                        Fingerprint {
                            scheme: id.into(),
                            value: id.repeat(1024),
                        },
                    )];
                    futures::executor::block_on(store.store(path, &criterion(), &value)).unwrap();
                }
                finished.fetch_add(1, Ordering::SeqCst);
            });
        }
        barrier.wait();
        let mut reads = 0;
        while finished.load(Ordering::SeqCst) != 2 {
            let saved = futures::executor::block_on(store.load::<String, ()>(&path, &criterion()))
                .unwrap()
                .unwrap();
            assert!(saved.output["metadata"]["id"] == "A" || saved.output["metadata"]["id"] == "B");
            assert_eq!(
                saved.output["metadata"]["padding"].as_str().unwrap().len(),
                8192
            );
            let id = saved.output["metadata"]["id"].as_str().unwrap();
            // The initial record can still be observed before either writer.
            if saved.revision != "v1" {
                assert_eq!(saved.revision, id);
                assert_eq!(
                    saved.dependencies,
                    [Dependency::Node(
                        id.into(),
                        Fingerprint {
                            scheme: id.into(),
                            value: id.repeat(1024),
                        }
                    )]
                );
            } else {
                assert_eq!(id, "A");
                assert_eq!(saved.dependencies, record("A").dependencies);
            }
            reads += 1;
        }
        assert!(reads > 0);
    });
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
}

#[derive(Clone)]
struct StoredGraph(FileEvidenceStorage);

enum UnreadContents {}
impl NodeContents for UnreadContents {
    // :::{cite} habeas:coverage
    // UnreadContents is uninhabited: this storage-only fixture cannot construct
    // a value or call a content method. The implementation discharges the
    // Domain associated-type bound; its empty match has no executable case.
    // :::
    fn fingerprint(&self) -> Fingerprint {
        match *self {}
    }
    // :::{cite} habeas:coverage
    // Formatting requires a value of the uninhabited UnreadContents type.
    // This empty match has no executable case; storage tests deliberately
    // supply no content values and reject node reads at the domain boundary.
    // :::
    fn format(&self) -> String {
        match *self {}
    }
}

impl Domain for StoredGraph {
    type NodeId = String;
    type EdgeLabel = ();

    type Error = FileStorageError;

    fn node_path(&self, node: &String) -> NodePath {
        NodePath::new(vec!["consumer".into(), node.clone()])
    }
    // :::{cite} habeas:coverage
    // Domain requires this callback even for a storage-only consumer. The
    // delegation test must use only node_path; discovering edges would be
    // an unwanted graph access, which this guard rejects. Traversal fixtures
    // separately exercise edge discovery.
    // :::
    fn outgoing_edges(&self, _: &String, _: ()) -> Result<Vec<String>, Self::Error> {
        panic!("storage operations must not discover graph edges")
    }

    async fn load_evidence(
        &self,
        node: &String,
        criterion: &CriterionPath,
    ) -> Result<Option<Record<String>>, Self::Error> {
        self.0.load(&self.node_path(node), criterion).await
    }
    async fn store_evidence(
        &self,
        node: &String,
        criterion: &CriterionPath,
        record: &Record<String>,
    ) -> Result<(), Self::Error> {
        self.0.store(&self.node_path(node), criterion, record).await
    }

    type NodeContent = UnreadContents;

    // :::{cite} habeas:coverage
    // This storage-only consumer must delegate load/store using node_path
    // without resolving contents. Domain requires the callback, but invoking
    // this guard would violate the no-content-read invariant of the test.
    // Other graph fixtures exercise content resolution.
    // :::
    fn node_content(&self, _: &String) -> Result<UnreadContents, Self::Error> {
        panic!("storage operations must not read graph contents")
    }
}

struct StoredReview<'a>(&'a AtomicUsize);
impl Criterion<StoredGraph> for StoredReview<'_> {
    type Ok = String;
    fn source_node_id(&self) -> String {
        "opaque/é".into()
    }
    fn criterion_path(&self) -> CriterionPath {
        CriterionPath::new(vec!["review".into()])
    }
    fn revision(&self) -> String {
        "v1".into()
    }
    async fn execute(
        self,
        _: &mut ValidationContext<'_, StoredGraph>,
    ) -> Result<String, ValidationError<FileStorageError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok("completed answer".into())
    }
}

// {test}`requirements.sdk.graph.evidence.store_evidence`
// {test}`requirements.sdk.graph.evidence.record.output`
// {test}`requirements.sdk.graph.evidence.check_node`
// {test}`requirements.sdk.graph.validators.validate_node.errors`
#[tokio::test]
async fn malformed_records_are_storage_errors_and_bad_outputs_are_output_errors() {
    let temp = tempfile::tempdir().unwrap();
    let graph = StoredGraph(FileEvidenceStorage::new(temp.path()));
    let validator = Validator::new(graph.clone());
    let executions = AtomicUsize::new(0);
    let value = validator.verify(StoredReview(&executions)).await.unwrap();
    assert_eq!(value.output(), "completed answer");
    validator.store_evidence(&value).await.unwrap();
    let source = StoredReview(&executions).source_node_id();
    let criterion = StoredReview(&executions).criterion_path();
    let path = graph.0.record_path(&graph.node_path(&source), &criterion);
    let mut saved = graph
        .load_evidence(&source, &criterion)
        .await
        .unwrap()
        .unwrap();
    std::fs::write(&path, "not JSON").unwrap();
    assert!(matches!(
        validator.load_evidence(&StoredReview(&executions)).await,
        Err(ValidationError::Graph(FileStorageError::Json(_)))
    ));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "not JSON");
    assert_eq!(executions.load(Ordering::SeqCst), 1);
    saved.output = json!({"not":"a String"});
    saved.revision = "obsolete".into();
    graph
        .store_evidence(&source, &criterion, &saved)
        .await
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(matches!(
        validator.load_evidence(&StoredReview(&executions)).await,
        Err(ValidationError::Output(_))
    ));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let value = validator.verify(StoredReview(&executions)).await.unwrap();
    assert_eq!(value.output(), "completed answer");
    assert_eq!(executions.load(Ordering::SeqCst), 2);
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

// {test}`requirements.sdk.graph.evidence.evidence_storage.filesystem`
// {test}`requirements.sdk.graph.evidence.evidence_storage`
// {test}`requirements.sdk.filesystem-storage.interface`
// {test}`requirements.sdk.filesystem-storage.load`
#[tokio::test]
async fn consumer_delegates_evidence_storage_using_only_the_requested_nodes_path() {
    let temp = tempfile::tempdir().unwrap();
    let graph = StoredGraph(FileEvidenceStorage::new(temp.path().join("new")));
    let node = "opaque/é".to_string();
    assert!(
        graph
            .load_evidence(&node, &criterion())
            .await
            .unwrap()
            .is_none()
    );
    let record = record("delegated");
    graph
        .store_evidence(&node, &criterion(), &record)
        .await
        .unwrap();
    let loaded = graph
        .load_evidence(&node, &criterion())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.dependencies, record.dependencies);
    assert_eq!(loaded.output, record.output);
    assert!(
        graph
            .load_evidence(&"other".into(), &criterion())
            .await
            .unwrap()
            .is_none()
    );
    std::fs::write(
        graph.0.record_path(&graph.node_path(&node), &criterion()),
        b"bad JSON",
    )
    .unwrap();
    assert!(matches!(
        graph.load_evidence(&node, &criterion()).await,
        Err(FileStorageError::Json(_))
    ));
}

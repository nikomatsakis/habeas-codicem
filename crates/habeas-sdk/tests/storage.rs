use habeas_sdk::{graph::*, storage::*};
use serde::{Serialize, Serializer};
use serde_json::{Value, json};
use std::sync::{
    Arc, Barrier,
    atomic::{AtomicUsize, Ordering},
};

fn record(id: &str) -> Record<String, Value> {
    Record {
        procedure: "v1".into(),
        dependencies: vec![Dependency::Node(
            "source".into(),
            Fingerprint {
                scheme: "sha-v1".into(),
                value: "a".repeat(64),
            },
        )],
        output: Judgment {
            result: false,
            rationale: "a negative review".into(),
            metadata: json!({"id": id, "padding": "x".repeat(8192)}),
        },
    }
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
        (vec![], "record.json"),
        (vec![""], "s/record.json"),
        (vec!["a", "b"], "s61/s62/record.json"),
        (vec!["a/b"], "s612f62/record.json"),
        (vec![".."], "s2e2e/record.json"),
        (vec!["é"], "sc3a9/record.json"),
        (vec!["e\u{301}"], "s65cc81/record.json"),
    ];
    for (segments, expected) in cases {
        let path = NodePath {
            segments: segments.iter().map(|s| s.to_string()).collect(),
        };
        assert_eq!(store.record_path(&path), root.join(expected));
        assert!(store.load::<String, Value>(&path).await.unwrap().is_none());
    }
    assert!(!root.exists());
}

// {test}`requirements.sdk.filesystem-storage.interface`
// {test}`requirements.sdk.filesystem-storage.load`
// {test}`requirements.sdk.filesystem-storage.store`
#[tokio::test]
async fn records_round_trip_and_malformed_or_incompatible_data_is_an_error() {
    let temp = tempfile::tempdir().unwrap();
    let store = FileEvidenceStorage::new(temp.path());
    let path = NodePath {
        segments: vec!["node".into()],
    };
    store.store(&path, &record("A")).await.unwrap();
    let saved = store.load::<String, Value>(&path).await.unwrap().unwrap();
    assert_eq!(saved.procedure, "v1");
    assert_eq!(saved.dependencies, record("A").dependencies);
    assert_eq!(saved.output, record("A").output);
    assert!(matches!(
        store.load::<u8, Value>(&path).await,
        Err(FileStorageError::Json(_))
    ));
    std::fs::write(store.record_path(&path), b"not JSON").unwrap();
    assert!(matches!(
        store.load::<String, Value>(&path).await,
        Err(FileStorageError::Json(_))
    ));
    std::fs::remove_file(store.record_path(&path)).unwrap();
    std::fs::create_dir(store.record_path(&path)).unwrap();
    assert!(matches!(
        store.load::<String, Value>(&path).await,
        Err(FileStorageError::Io(_))
    ));
}

struct BadMetadata;
impl Serialize for BadMetadata {
    fn serialize<S: Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("cannot serialize"))
    }
}

// {test}`requirements.sdk.filesystem-storage.store`
#[tokio::test]
async fn failed_serialization_preserves_records_and_failed_replacement_cleans_up() {
    let temp = tempfile::tempdir().unwrap();
    let store = FileEvidenceStorage::new(temp.path());
    let path = NodePath {
        segments: Vec::new(),
    };
    store.store(&path, &record("original")).await.unwrap();
    let before = std::fs::read(store.record_path(&path)).unwrap();
    let bad: Record<String, BadMetadata> = Record {
        procedure: "v2".into(),
        dependencies: vec![],
        output: Judgment {
            result: true,
            rationale: "new".into(),
            metadata: BadMetadata,
        },
    };
    assert!(matches!(
        store.store(&path, &bad).await,
        Err(FileStorageError::Json(_))
    ));
    assert_eq!(std::fs::read(store.record_path(&path)).unwrap(), before);
    std::fs::remove_file(store.record_path(&path)).unwrap();
    std::fs::create_dir(store.record_path(&path)).unwrap();
    assert!(matches!(
        store.store(&path, &record("replacement")).await,
        Err(FileStorageError::Io(_))
    ));
    let names: Vec<_> = std::fs::read_dir(temp.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, vec!["record.json"]);
}

// {test}`requirements.sdk.filesystem-storage.store`
#[test]
fn concurrent_readers_only_observe_complete_records_from_competing_writes() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(FileEvidenceStorage::new(temp.path()));
    let path = NodePath {
        segments: Vec::new(),
    };
    futures::executor::block_on(store.store(&path, &record("A"))).unwrap();
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
                    futures::executor::block_on(store.store(path, &record(id))).unwrap();
                }
                finished.fetch_add(1, Ordering::SeqCst);
            });
        }
        barrier.wait();
        let mut reads = 0;
        while finished.load(Ordering::SeqCst) != 2 {
            let saved = futures::executor::block_on(store.load::<String, Value>(&path))
                .unwrap()
                .unwrap();
            assert!(saved.output.metadata["id"] == "A" || saved.output.metadata["id"] == "B");
            assert_eq!(
                saved.output.metadata["padding"].as_str().unwrap().len(),
                8192
            );
            reads += 1;
        }
        assert!(reads > 0);
    });
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
}

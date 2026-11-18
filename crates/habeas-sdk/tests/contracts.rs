use habeas_sdk::graph::*;
use std::sync::{Arc, Mutex};

// These application types intentionally have no Hash, Serde, or Display bounds.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Node(u8);
struct Content(String);
#[derive(Clone, Debug, PartialEq, Eq)]
struct Metadata {
    reason: String,
    count: usize,
}
#[derive(Default)]
struct State {
    records: Vec<(Node, Record<Node, Metadata, u8>)>,
    text: String,
    edge_version: usize,
    reads: usize,
    writes: usize,
    callbacks: usize,
    fail_load: bool,
    fail_read: bool,
    fail_edges: bool,
    fail_store: bool,
}
#[derive(Clone, Default)]
struct GraphOnly(Arc<Mutex<State>>);
#[derive(Clone)]
struct Application(GraphOnly);

macro_rules! graph {
    ($ty:ty, $state:path) => {
        impl Graph for $ty {
            type NodeId = Node;
            type NodeContent = Content;
            type EdgeLabel = u8;
            type JudgmentMetadata = Metadata;
            type Error = &'static str;
            fn node_content(&self, _: &Node) -> Result<Content, Self::Error> {
                let mut state = $state(self).lock().unwrap();
                state.reads += 1;
                if state.fail_read {
                    return Err("read");
                }
                Ok(Content(state.text.clone()))
            }
            fn node_path(&self, node: &Node) -> NodePath {
                NodePath {
                    segments: vec!["domain".into(), node.0.to_string()],
                }
            }
            fn outgoing_edges(&self, _: &Node, _: u8) -> Result<Vec<Node>, Self::Error> {
                let state = $state(self).lock().unwrap();
                if state.fail_edges {
                    return Err("edges");
                }
                Ok(if state.edge_version == 0 {
                    vec![]
                } else {
                    vec![Node(state.edge_version as u8)]
                })
            }
        }
        impl Fingerprinting for $ty {
            fn fingerprint(&self, node: &Node, content: &Content) -> Fingerprint {
                fingerprint(format!("{}:{}", node.0, content.0))
            }
        }
        impl EvidenceStorage for $ty {
            async fn load_evidence(
                &self,
                node: &Node,
            ) -> Result<Option<Record<Node, Metadata, u8>>, Self::Error> {
                let state = $state(self).lock().unwrap();
                if state.fail_load {
                    return Err("load");
                }
                Ok(state
                    .records
                    .iter()
                    .find(|(key, _)| key == node)
                    .map(|(_, record)| record.clone()))
            }
            async fn store_evidence(
                &self,
                node: &Node,
                record: &Record<Node, Metadata, u8>,
            ) -> Result<(), Self::Error> {
                let mut state = $state(self).lock().unwrap();
                if state.fail_store {
                    return Err("store");
                }
                state.writes += 1;
                state.records.retain(|(key, _)| key != node);
                state.records.push((node.clone(), record.clone()));
                Ok(())
            }
        }
    };
}
// {cite}`habeas:util`
fn graph_state(graph: &GraphOnly) -> &Mutex<State> {
    &graph.0
}
graph!(GraphOnly, graph_state);
// {cite}`habeas:util`
fn application_state(graph: &Application) -> &Mutex<State> {
    &graph.0.0
}
graph!(Application, application_state);
impl Validation for Application {
    async fn validate<'a, 'd>(
        &'a self,
        ctx: &'a ValidationContext<'d, Self>,
    ) -> Result<Judgment<Metadata>, ValidationError<Self::Error>>
    where
        'd: 'a,
    {
        self.0.0.lock().unwrap().callbacks += 1;
        let content = ctx
            .inspection()
            .read(ctx.node_id())
            .map_err(ValidationError::Graph)?;
        assert!(ctx.successors(&7).map_err(ValidationError::Graph)?.len() <= 1);
        Ok(Judgment {
            result: ctx.node_id().0 % 2 == 0,
            rationale: format!("reviewed {}", content.0),
            metadata: Metadata {
                reason: "application reason".into(),
                count: 17,
            },
        })
    }
}
fn fingerprint(value: String) -> Fingerprint {
    Fingerprint {
        scheme: "consumer-v1".into(),
        value,
    }
}
fn record() -> Record<Node, Metadata, u8> {
    Record {
        procedure: "v1".into(),
        dependencies: vec![
            Dependency::Node(Node(1), fingerprint("1:source".into())),
            Dependency::OutgoingEdges(Node(1), 7, edge_fingerprint(&GraphOnly::default(), &[])),
        ],
        output: Judgment {
            result: false,
            rationale: "negative".into(),
            metadata: Metadata {
                reason: "meaning independent of bool".into(),
                count: 42,
            },
        },
    }
}

// {test}`requirements.sdk.graph.domain_types`
// {test}`requirements.sdk.graph.evidence.record`
// {test}`requirements.sdk.graph.evidence.evidence_storage`
// {test}`requirements.sdk.graph.validation.check_node`
// {test}`requirements.sdk.graph.validation.procedure_identity`
// {test}`requirements.sdk.graph.evidence.checking_records`
// {test}`requirements.sdk.filesystem-storage.interface`
// {test}`requirements.sdk.domain`
#[tokio::test]
async fn checker_needs_no_validation_and_storage_preserves_nonserializable_metadata() {
    fn send<T: Send>(value: T) -> T {
        value
    }
    let graph = GraphOnly::default();
    graph.0.lock().unwrap().text = "source".into();
    let checker = Checker::new(&graph, |_| Ok("v1".into()));
    assert!(matches!(
        checker.check_node(&Node(1)).await.unwrap(),
        EvidenceState::Missing
    ));
    let saved = record();
    send(graph.store_evidence(&Node(1), &saved)).await.unwrap();
    let loaded = send(graph.load_evidence(&Node(1))).await.unwrap().unwrap();
    assert_eq!(loaded.procedure, saved.procedure);
    assert_eq!(loaded.dependencies, saved.dependencies);
    assert_eq!(loaded.output, saved.output);
    assert!(
        matches!(checker.check_node(&Node(1)).await.unwrap(), EvidenceState::Current(j) if j == saved.output)
    );
    assert!(matches!(
        Checker::new(&graph, |_| Ok("v2".into()))
            .check_node(&Node(1))
            .await
            .unwrap(),
        EvidenceState::Stale
    ));
    graph.0.lock().unwrap().text = "changed".into();
    assert!(matches!(
        checker.check_node(&Node(1)).await.unwrap(),
        EvidenceState::Stale
    ));
    graph.0.lock().unwrap().text = "source".into();
    assert!(matches!(
        Checker::new(&graph, |_| Err("version"))
            .check_node(&Node(1))
            .await,
        Err("version")
    ));
    for failure in ["load", "read", "edges"] {
        {
            let mut state = graph.0.lock().unwrap();
            state.fail_load = failure == "load";
            state.fail_read = failure == "read";
            state.fail_edges = failure == "edges";
        }
        assert!(matches!(checker.check_node(&Node(1)).await, Err(error) if error == failure));
    }
    let state = graph.0.lock().unwrap();
    assert_eq!(state.writes, 1);
    assert_eq!(state.callbacks, 0);
    assert_eq!(state.records[0].1.output, saved.output);
}

// {test}`requirements.sdk.domain`
// {test}`requirements.sdk.graph.validation.judgment`
// {test}`requirements.sdk.graph.validation.validate_node`
// {test}`requirements.sdk.graph.validation.validate_node.errors`
// {test}`requirements.sdk.graph.validation.check_procedure`
// {test}`requirements.sdk.graph.agent-calls.procedures`
// {test}`requirements.sdk.graph.validation.root_inputs`
// {test}`requirements.sdk.graph.evidence.record`
// {test}`requirements.sdk.graph.evidence.evidence_storage`
#[tokio::test]
async fn domain_validation_preserves_metadata_reuses_traces_and_propagates_failures() {
    fn domain<T: Domain>(_: &T) {}
    let graph = Application(GraphOnly::default());
    domain(&graph);
    graph.0.0.lock().unwrap().text = "source".into();
    let validator = Validator::new(&graph, |_| Ok("v1".into()));
    let judgment = validator.validate_node(&Node(1)).await.unwrap();
    assert!(!judgment.result);
    assert_eq!(judgment.rationale, "reviewed source");
    assert_eq!(
        judgment.metadata,
        Metadata {
            reason: "application reason".into(),
            count: 17
        }
    );
    let saved = graph.load_evidence(&Node(1)).await.unwrap().unwrap();
    assert_eq!(saved.output, judgment);
    assert_eq!(saved.dependencies, record().dependencies);
    assert_eq!(validator.validate_node(&Node(1)).await.unwrap(), judgment);
    {
        let state = graph.0.0.lock().unwrap();
        assert_eq!((state.callbacks, state.writes), (1, 1));
    }
    for failure in ["read", "edges"] {
        {
            let mut state = graph.0.0.lock().unwrap();
            state.fail_read = failure == "read";
            state.fail_edges = failure == "edges";
        }
        assert!(
            matches!(validator.validate_node(&Node(1)).await, Err(ValidationError::Graph(error)) if error==failure)
        );
        let state = graph.0.0.lock().unwrap();
        assert_eq!((state.callbacks, state.writes), (1, 1));
        assert_eq!(state.records[0].1.dependencies, saved.dependencies);
    }
    {
        let mut state = graph.0.0.lock().unwrap();
        state.fail_edges = false;
        state.text = "changed".into();
        state.fail_store = true;
    }
    assert!(matches!(
        validator.validate_node(&Node(1)).await,
        Err(ValidationError::Graph("store"))
    ));
    assert_eq!(
        graph.load_evidence(&Node(1)).await.unwrap().unwrap().output,
        judgment
    );
    graph.0.0.lock().unwrap().fail_store = false;
    assert_eq!(
        validator.validate_node(&Node(1)).await.unwrap().rationale,
        "reviewed changed"
    );
    let positive = validator.validate_node(&Node(2)).await.unwrap();
    assert!(positive.result);
    assert_eq!(positive.rationale, "reviewed changed");
    assert_eq!(positive.metadata, judgment.metadata);
    assert_eq!(
        graph.load_evidence(&Node(2)).await.unwrap().unwrap().output,
        positive
    );
    assert_eq!(validator.validate_node(&Node(2)).await.unwrap(), positive);
}

// {test}`requirements.sdk.graph.validation.validate_node`
// {test}`requirements.sdk.graph.evidence.checking_records`
// {test}`requirements.sdk.graph.outgoing_edges`
#[tokio::test]
async fn changed_target_membership_revalidates_and_replaces_the_saved_selection() {
    let graph = Application(GraphOnly::default());
    graph.0.0.lock().unwrap().text = "unchanged content".into();
    let validator = Validator::new(&graph, |_| Ok("v1".into()));
    let original = validator.validate_node(&Node(1)).await.unwrap();
    let before = graph.load_evidence(&Node(1)).await.unwrap().unwrap();
    graph.0.0.lock().unwrap().edge_version = 3;
    let reassessed = validator.validate_node(&Node(1)).await.unwrap();
    assert_eq!(reassessed, original);
    let after = graph.load_evidence(&Node(1)).await.unwrap().unwrap();
    assert_ne!(after.dependencies, before.dependencies);
    assert_eq!(
        after.dependencies,
        vec![
            Dependency::Node(Node(1), fingerprint("1:unchanged content".into())),
            Dependency::OutgoingEdges(Node(1), 7, edge_fingerprint(&graph, &[Node(3)])),
        ]
    );
    assert_eq!(validator.validate_node(&Node(1)).await.unwrap(), reassessed);
    let state = graph.0.0.lock().unwrap();
    assert_eq!((state.callbacks, state.writes), (2, 2));
}

// {test}`requirements.sdk.graph.hierarchical-paths`
#[test]
fn hierarchical_requirement_identity_maps_each_component_to_a_path_segment() {
    // This is the chapter's illustrative domain, rather than Habeas's persisted IDs.
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
fn identities_remain_stable_and_fingerprinting_uses_the_supplied_node_snapshot() {
    let graph = GraphOnly::default();
    graph.0.lock().unwrap().text = "before".into();
    let snapshot = InspectionContext::new(&graph)
        .read_snapshot(&Node(1))
        .unwrap();
    let path = graph.node_path(&Node(1));
    graph.0.lock().unwrap().text = "after".into();
    assert_eq!(
        graph.fingerprint(&Node(1), &snapshot.content),
        fingerprint("1:before".into())
    );
    assert_eq!(snapshot.fingerprint, fingerprint("1:before".into()));
    assert_ne!(
        graph.fingerprint(&Node(2), &snapshot.content),
        snapshot.fingerprint
    );
    assert_eq!(graph.node_path(&Node(1)), path);
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

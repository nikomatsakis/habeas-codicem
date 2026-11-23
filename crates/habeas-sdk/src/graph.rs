//! Consumer-defined graphs, tracked observations, and validation.

use crate::domain::{Domain, NodeContents};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{future::Future, sync::Mutex};

mod runtime;
pub use crate::procedure::ValidationContext;
pub use runtime::Validator;

// {impl}`requirements.sdk.graph.fingerprint`
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub scheme: String,
    pub value: String,
}

// {impl}`requirements.sdk.graph.snapshot`
#[derive(Clone, Debug)]
pub struct Snapshot<T> {
    pub content: T,
    pub fingerprint: Fingerprint,
}

/// An ordered path with read-only access to its segments.
///
/// ```compile_fail
/// use habeas_sdk::graph::NodePath;
/// let mut path = NodePath::new(vec!["part".into()]);
/// path.segments.clear();
/// ```
// {test}`requirements.sdk.graph.node_path_struct`
// {impl}`requirements.sdk.graph.node_path_struct`
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodePath {
    segments: Vec<String>,
}

impl NodePath {
    /// Construct a path, preserving segment order and boundaries.
    // {impl}`requirements.sdk.graph.node_path_struct`
    pub fn new(segments: Vec<String>) -> Self {
        Self { segments }
    }

    /// Read the ordered path segments.
    // {impl}`requirements.sdk.graph.node_path_struct`
    pub fn segments(&self) -> &[String] {
        &self.segments
    }
}

/// An ordered path with read-only access to its segments.
///
/// ```compile_fail
/// use habeas_sdk::graph::CriterionPath;
/// let mut path = CriterionPath::new(vec!["part".into()]);
/// path.segments.clear();
/// ```
// {test}`requirements.sdk.graph.validation.criterion_path`
// {impl}`requirements.sdk.graph.validation.criterion_path`
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CriterionPath {
    segments: Vec<String>,
}

impl CriterionPath {
    /// Construct a path, preserving segment order and boundaries.
    // {impl}`requirements.sdk.graph.validation.criterion_path`
    pub fn new(segments: Vec<String>) -> Self {
        Self { segments }
    }

    /// Read the ordered path segments.
    // {impl}`requirements.sdk.graph.validation.criterion_path`
    pub fn segments(&self) -> &[String] {
        &self.segments
    }
}

// {impl}`requirements.sdk.graph.evidence.dependencies`
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Dependency<N, L = ()> {
    Node(N, Fingerprint),
    OutgoingEdges(N, L, Fingerprint),
}

// {impl}`requirements.sdk.graph.evidence.record`
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record<N, L = ()> {
    pub revision: String,
    pub dependencies: Vec<Dependency<N, L>>,
    pub output: serde_json::Value,
}

/// A computed value and the fingerprints of its dependencies.
///
/// The captured evidence metadata is available through read-only accessors.
///
/// ```compile_fail
/// use habeas_sdk::{domain::Domain, graph::Fingerprinted};
/// fn change_revision<D: Domain>(answer: &mut Fingerprinted<D, bool>) {
///     answer.revision = "different rules".into();
/// }
/// ```
// {test}`requirements.sdk.graph.evidence.inputs`
// {impl}`requirements.sdk.graph.evidence.inputs`
pub struct Fingerprinted<D: Domain, O> {
    source: D::NodeId,
    criterion: CriterionPath,
    revision: String,
    dependencies: Vec<Dependency<D::NodeId, D::EdgeLabel>>,
    output: O,
}

// {impl}`requirements.sdk.graph.evidence.inputs`
impl<D: Domain, O> Fingerprinted<D, O> {
    /// The source node this answer concerns.
    pub fn source_node_id(&self) -> &D::NodeId {
        &self.source
    }

    /// The question's path relative to that source.
    pub fn criterion_path(&self) -> &CriterionPath {
        &self.criterion
    }

    /// The review rules used to produce this answer.
    pub fn revision(&self) -> &str {
        &self.revision
    }

    /// Recorded node-content and successor-set observations.
    pub fn dependencies(&self) -> &[Dependency<D::NodeId, D::EdgeLabel>] {
        &self.dependencies
    }

    /// Borrow the completed answer.
    pub fn output(&self) -> &O {
        &self.output
    }

    /// Take ownership of the completed answer.
    pub fn into_output(self) -> O {
        self.output
    }
}

// {cite}`habeas:util`
/// An ordered, deduplicated log for adapters retaining their own wire inputs.
#[derive(Clone, Debug)]
pub struct Trace<T> {
    observations: Vec<T>,
}
impl<T> Default for Trace<T> {
    // {cite}`habeas:util`
    fn default() -> Self {
        Self {
            observations: Vec::new(),
        }
    }
}
impl<T: PartialEq> Trace<T> {
    // {cite}`habeas:util`
    pub fn observe(&mut self, value: T) {
        if !self.observations.contains(&value) {
            self.observations.push(value);
        }
    }
    // {cite}`habeas:util`
    pub fn observations(&self) -> &[T] {
        &self.observations
    }
}

// {impl}`requirements.sdk.graph.validation.criterion_trait`
#[derive(Debug)]
pub enum ValidationError<E> {
    Graph(E),
    NoAgentConfigured,
    Agent(String),
    Output(serde_json::Error),
}

impl<E: std::fmt::Display> std::fmt::Display for ValidationError<E> {
    // {cite}`habeas:util`
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Graph(error) => error.fmt(f),
            Self::NoAgentConfigured => f.write_str("no agent is configured"),
            Self::Agent(error) => write!(f, "agent: {error}"),
            Self::Output(error) => write!(f, "output: {error}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for ValidationError<E> {
    // {cite}`habeas:util`
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Graph(error) => Some(error),
            Self::Output(error) => Some(error),
            Self::Agent(_) | Self::NoAgentConfigured => None,
        }
    }
}

// {impl}`requirements.sdk.graph.validation.criterion_trait`
pub trait Criterion<D: Domain>: Send + Sized {
    /// The completed answer, serialized for storage and reconstructed on reuse.
    type Ok: Serialize + serde::de::DeserializeOwned + Send;

    /// The node this question is about.
    fn source_node_id(&self) -> D::NodeId;
    /// Identify this question within the source node's saved evidence.
    fn criterion_path(&self) -> CriterionPath;
    /// Version of the review rules and output encoding.
    /// Change it when an old answer should no longer be reused.
    fn revision(&self) -> String;

    /// Select inputs through the context and compute the completed answer.
    // {impl}`requirements.sdk.graph.validation.criterion_trait`
    fn execute(
        self,
        ctx: &mut ValidationContext<'_, D>,
    ) -> impl Future<Output = Result<Self::Ok, ValidationError<D::Error>>> + Send;
}

/// The trace lock only protects observations, never graph or agent operations.
// {impl}`requirements.sdk.graph.context-tools.tracked_inspection`
pub struct InspectionContext<'d, D: Domain> {
    graph: &'d D,
    observations: Mutex<Vec<Dependency<D::NodeId, D::EdgeLabel>>>,
}

impl<'d, D: Domain> InspectionContext<'d, D> {
    // {cite}`habeas:util`
    pub fn new(graph: &'d D) -> Self {
        Self {
            graph,
            observations: Mutex::new(Vec::new()),
        }
    }

    // {cite}`habeas:util`
    pub fn graph(&self) -> &'d D {
        self.graph
    }

    // {cite}`habeas:util`
    // {impl}`requirements.sdk.graph.context-tools.read_node.behavior`
    // {impl}`requirements.sdk.graph.context-tools.read_node.fn`
    // {impl}`requirements.sdk.graph.context-tools.read_node.inputs`
    pub fn read(&self, node: &D::NodeId) -> Result<D::NodeContent, D::Error> {
        Ok(self.read_snapshot(node)?.content)
    }

    // {impl}`requirements.sdk.graph.evidence.node_inputs`
    // {impl}`requirements.sdk.graph.snapshot`
    pub fn read_snapshot(&self, node: &D::NodeId) -> Result<Snapshot<D::NodeContent>, D::Error> {
        let snapshot = self.prepare_read(node)?;
        self.observe_read(node.clone(), snapshot.fingerprint.clone());
        Ok(snapshot)
    }

    /// Prepare a paged result without claiming its delivery.
    pub fn prepare_read(&self, node: &D::NodeId) -> Result<Snapshot<D::NodeContent>, D::Error> {
        let content = self.graph.node_content(node)?;
        let fingerprint = content.fingerprint();
        Ok(Snapshot {
            content,
            fingerprint,
        })
    }

    // {impl}`requirements.sdk.graph.context-tools.outgoing_edges.behavior`
    // {impl}`requirements.sdk.graph.context-tools.outgoing_edges.fn`
    // {impl}`requirements.sdk.graph.context-tools.outgoing_edges.inputs`
    // {impl}`requirements.sdk.graph.context-tools.snapshot`
    pub fn outgoing_edges(
        &self,
        node: &D::NodeId,
        label: D::EdgeLabel,
    ) -> Result<Snapshot<Vec<D::NodeId>>, D::Error>
    where
        D::EdgeLabel: PartialEq,
    {
        let snapshot = self.prepare_edges(node, label.clone())?;
        self.observe_edges(node.clone(), label, snapshot.fingerprint.clone());
        Ok(snapshot)
    }

    /// Prepare a relationship result without claiming delivery.
    // {cite}`habeas:util`
    pub fn prepare_edges(
        &self,
        node: &D::NodeId,
        label: D::EdgeLabel,
    ) -> Result<Snapshot<Vec<D::NodeId>>, D::Error> {
        let content = self.graph.outgoing_edges(node, label)?;
        let fingerprint = edge_fingerprint(self.graph, &content);
        Ok(Snapshot {
            content,
            fingerprint,
        })
    }

    /// Import the fingerprint of the exact snapshot delivered to the caller.
    pub fn observe_read(&self, node: D::NodeId, fingerprint: Fingerprint) {
        let mut observations = self.observations.lock().expect("dependency trace poisoned");
        if !observations.iter().any(|dependency| {
            matches!(dependency,
            Dependency::Node(previous, hash) if previous == &node && hash == &fingerprint)
        }) {
            observations.push(Dependency::Node(node, fingerprint));
        }
    }

    // {cite}`habeas:util`
    pub fn observe_edges(&self, node: D::NodeId, label: D::EdgeLabel, fingerprint: Fingerprint)
    where
        D::EdgeLabel: PartialEq,
    {
        self.observe_edges_by(node, label, fingerprint, |a, b| a == b);
    }

    // {cite}`habeas:util`
    pub(crate) fn observe_edges_by(
        &self,
        node: D::NodeId,
        label: D::EdgeLabel,
        fingerprint: Fingerprint,
        matches: fn(&D::EdgeLabel, &D::EdgeLabel) -> bool,
    ) {
        let mut observations = self.observations.lock().expect("dependency trace poisoned");
        if !observations.iter().any(|dependency| {
            matches!(dependency,
            Dependency::OutgoingEdges(previous, previous_label, hash)
            if previous == &node && matches(previous_label, &label) && hash == &fingerprint)
        }) {
            observations.push(Dependency::OutgoingEdges(node, label, fingerprint));
        }
    }

    // {cite}`habeas:util`
    pub fn dependencies(&self) -> Vec<Dependency<D::NodeId, D::EdgeLabel>> {
        self.observations
            .lock()
            .expect("dependency trace poisoned")
            .clone()
    }

    // {impl}`requirements.sdk.graph.evidence.record`
    pub fn finish(
        &self,
        revision: String,
        output: impl Serialize,
    ) -> Result<Record<D::NodeId, D::EdgeLabel>, serde_json::Error> {
        Ok(Record {
            revision,
            dependencies: self.dependencies(),
            output: serde_json::to_value(output)?,
        })
    }
}

// {impl}`requirements.sdk.graph.evidence.checking_records.procedure_match`
pub fn procedure_matches(stored: &str, requested: &str) -> bool {
    stored == "*" || stored == requested
}

/// Freshness-only reads do not enter a new assessment's trace.
// {impl}`requirements.sdk.graph.evidence.checking_records`
pub fn record_is_current<D: Domain>(
    graph: &D,
    revision: &str,
    record: &Record<D::NodeId, D::EdgeLabel>,
) -> Result<bool, D::Error> {
    if !procedure_matches(&record.revision, revision) {
        return Ok(false);
    }
    dependencies_are_current(graph, &record.dependencies)
}

fn dependencies_are_current<D: Domain>(
    graph: &D,
    dependencies: &[Dependency<D::NodeId, D::EdgeLabel>],
) -> Result<bool, D::Error> {
    for dependency in dependencies {
        let (current, previous) = match dependency {
            Dependency::Node(node, previous) => {
                let content = graph.node_content(node)?;
                (content.fingerprint(), previous)
            }
            Dependency::OutgoingEdges(node, label, previous) => (
                edge_fingerprint(graph, &graph.outgoing_edges(node, label.clone())?),
                previous,
            ),
        };
        if &current != previous {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Hash the target set without requiring serialization or hashing of opaque IDs.
/// JSON encodes segment boundaries and Unicode unambiguously; ordering and
/// duplicate edges do not affect membership. Empty selections have a digest too.
// {impl}`requirements.sdk.graph.context-tools.snapshot`
pub fn edge_fingerprint<D: Domain>(graph: &D, targets: &[D::NodeId]) -> Fingerprint {
    let mut paths = targets
        .iter()
        .map(|node| graph.node_path(node).segments)
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    let bytes = serde_json::to_vec(&paths).expect("string paths serialize");
    Fingerprint {
        scheme: "node-path-set-sha256-v1".into(),
        value: Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    }
}

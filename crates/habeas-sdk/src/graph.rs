//! Consumer-defined graphs, tracked observations, and validation.

use serde::{Deserialize, Serialize};
use std::{future::Future, sync::Mutex};

mod runtime;
pub use crate::procedure::ValidationContext;
pub use runtime::{Checker, EvidenceState, Validator};

// {impl}`requirements.sdk.graph.fingerprinting.fingerprint`
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub scheme: String,
    pub value: String,
}

// {impl}`requirements.sdk.graph.fingerprinting.snapshot`
#[derive(Clone, Debug)]
pub struct Snapshot<T> {
    pub content: T,
    pub fingerprint: Fingerprint,
}

// {impl}`requirements.sdk.graph.node_path`
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodePath {
    pub segments: Vec<String>,
}

// {impl}`requirements.sdk.graph.edges`
#[derive(Clone, Debug)]
pub struct Edge<N, L> {
    pub label: L,
    pub target: N,
}

// {impl}`requirements.sdk.graph.domain_types`
// {impl}`requirements.sdk.graph.outgoing_edges`
pub trait Graph: Send + Sync {
    type NodeId: Clone + Eq + Send + Sync;
    type NodeContent: Send + Sync;
    type EdgeLabel: Clone + Send + Sync;
    type JudgmentMetadata: Send + Sync;
    type Error: Send;

    fn node_content(&self, node: &Self::NodeId) -> Result<Self::NodeContent, Self::Error>;
    // {impl}`requirements.sdk.graph.node_path`
    fn node_path(&self, node: &Self::NodeId) -> NodePath;
    fn outgoing_edges(
        &self,
        node: &Self::NodeId,
    ) -> Result<Snapshot<Vec<Edge<Self::NodeId, Self::EdgeLabel>>>, Self::Error>;
}

// {impl}`requirements.sdk.graph.fingerprinting.fingerprint_content`
pub trait Fingerprinting: Graph {
    // {impl}`requirements.sdk.graph.fingerprinting.fingerprint_content`
    fn fingerprint(&self, node: &Self::NodeId, content: &Self::NodeContent) -> Fingerprint;
}

// {impl}`requirements.sdk.graph.validation.judgment`
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Judgment<M> {
    pub result: bool,
    pub rationale: String,
    pub metadata: M,
}

// {impl}`requirements.sdk.graph.evidence.dependencies`
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Dependency<N> {
    Node(N, Fingerprint),
    OutgoingEdges(N, Fingerprint),
}

// {impl}`requirements.sdk.graph.evidence.record`
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record<N, M> {
    pub procedure: String,
    pub dependencies: Vec<Dependency<N>>,
    pub output: Judgment<M>,
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

// {impl}`requirements.sdk.graph.evidence.evidence_storage`
pub trait EvidenceStorage: Graph {
    // {impl}`requirements.sdk.graph.evidence.evidence_storage`
    fn load_evidence(
        &self,
        node: &Self::NodeId,
    ) -> impl Future<
        Output = Result<Option<Record<Self::NodeId, Self::JudgmentMetadata>>, Self::Error>,
    > + Send;

    // {impl}`requirements.sdk.graph.evidence.evidence_storage`
    fn store_evidence(
        &self,
        node: &Self::NodeId,
        record: &Record<Self::NodeId, Self::JudgmentMetadata>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

// {impl}`requirements.sdk.domain`
pub trait Domain: Graph + Validation + Fingerprinting + EvidenceStorage {}

impl<T: Graph + Validation + Fingerprinting + EvidenceStorage> Domain for T {}

// {impl}`requirements.sdk.graph.validation.check_procedure`
#[derive(Debug)]
pub enum ValidationError<E> {
    Graph(E),
    NoAgentConfigured,
    Agent(String),
}

impl<E: std::fmt::Display> std::fmt::Display for ValidationError<E> {
    // {cite}`habeas:util`
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Graph(error) => error.fmt(f),
            Self::NoAgentConfigured => f.write_str("no agent is configured"),
            Self::Agent(error) => write!(f, "agent: {error}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for ValidationError<E> {
    // {cite}`habeas:util`
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Graph(error) => Some(error),
            Self::Agent(_) | Self::NoAgentConfigured => None,
        }
    }
}

// {impl}`requirements.sdk.graph.validation.check_procedure`
pub trait Validation: Graph + Fingerprinting + Sized {
    // {impl}`requirements.sdk.graph.validation.check_procedure`
    fn validate<'a, 'd>(
        &'a self,
        ctx: &'a ValidationContext<'d, Self>,
    ) -> impl Future<Output = Result<Judgment<Self::JudgmentMetadata>, ValidationError<Self::Error>>>
    + Send
    + 'a
    where
        'd: 'a;
}

/// The trace lock only protects observations, never graph or agent operations.
// {impl}`requirements.sdk.graph.context-tools.tracked_inspection`
pub struct InspectionContext<'d, D: Graph + Fingerprinting> {
    graph: &'d D,
    observations: Mutex<Vec<Dependency<D::NodeId>>>,
}

impl<'d, D: Graph + Fingerprinting> InspectionContext<'d, D> {
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
    pub fn read(&self, node: &D::NodeId) -> Result<D::NodeContent, D::Error> {
        Ok(self.read_snapshot(node)?.content)
    }

    // {impl}`requirements.sdk.graph.evidence.node_inputs`
    // {impl}`requirements.sdk.graph.fingerprinting.snapshot`
    pub fn read_snapshot(&self, node: &D::NodeId) -> Result<Snapshot<D::NodeContent>, D::Error> {
        let snapshot = self.prepare_read(node)?;
        self.observe_read(node.clone(), snapshot.fingerprint.clone());
        Ok(snapshot)
    }

    /// Prepare a paged result without claiming its delivery.
    pub fn prepare_read(&self, node: &D::NodeId) -> Result<Snapshot<D::NodeContent>, D::Error> {
        let content = self.graph.node_content(node)?;
        let fingerprint = self.graph.fingerprint(node, &content);
        Ok(Snapshot {
            content,
            fingerprint,
        })
    }

    pub fn outgoing_edges(
        &self,
        node: &D::NodeId,
    ) -> Result<Snapshot<Vec<Edge<D::NodeId, D::EdgeLabel>>>, D::Error> {
        let snapshot = self.prepare_edges(node)?;
        self.observe(Dependency::OutgoingEdges(
            node.clone(),
            snapshot.fingerprint.clone(),
        ));
        Ok(snapshot)
    }

    /// Prepare a relationship result without claiming delivery.
    // {cite}`habeas:util`
    pub fn prepare_edges(
        &self,
        node: &D::NodeId,
    ) -> Result<Snapshot<Vec<Edge<D::NodeId, D::EdgeLabel>>>, D::Error> {
        self.graph.outgoing_edges(node)
    }

    /// Import the fingerprint of the exact snapshot delivered to the caller.
    pub fn observe_read(&self, node: D::NodeId, fingerprint: Fingerprint) {
        self.observe(Dependency::Node(node, fingerprint));
    }

    // {cite}`habeas:util`
    pub fn observe_edges(&self, node: D::NodeId, fingerprint: Fingerprint) {
        self.observe(Dependency::OutgoingEdges(node, fingerprint));
    }

    // {cite}`habeas:util`
    fn observe(&self, observation: Dependency<D::NodeId>) {
        let mut observations = self.observations.lock().expect("dependency trace poisoned");
        if !observations.contains(&observation) {
            observations.push(observation);
        }
    }

    // {cite}`habeas:util`
    pub fn dependencies(&self) -> Vec<Dependency<D::NodeId>> {
        self.observations
            .lock()
            .expect("dependency trace poisoned")
            .clone()
    }

    // {impl}`requirements.sdk.graph.evidence.record`
    pub fn finish(
        &self,
        procedure: String,
        output: Judgment<D::JudgmentMetadata>,
    ) -> Record<D::NodeId, D::JudgmentMetadata> {
        Record {
            procedure,
            dependencies: self.dependencies(),
            output,
        }
    }
}

// {impl}`requirements.sdk.graph.evidence.checking_records.procedure_match`
pub fn procedure_matches(stored: &str, requested: &str) -> bool {
    stored == "*" || stored == requested
}

/// Freshness-only reads do not enter a new assessment's trace.
// {impl}`requirements.sdk.graph.evidence.checking_records`
pub fn record_is_current<D: Graph + Fingerprinting>(
    graph: &D,
    procedure: &str,
    record: &Record<D::NodeId, D::JudgmentMetadata>,
) -> Result<bool, D::Error> {
    if !procedure_matches(&record.procedure, procedure) {
        return Ok(false);
    }
    for dependency in &record.dependencies {
        let (current, previous) = match dependency {
            Dependency::Node(node, previous) => {
                let content = graph.node_content(node)?;
                (graph.fingerprint(node, &content), previous)
            }
            Dependency::OutgoingEdges(node, previous) => {
                (graph.outgoing_edges(node)?.fingerprint, previous)
            }
        };
        if &current != previous {
            return Ok(false);
        }
    }
    Ok(true)
}

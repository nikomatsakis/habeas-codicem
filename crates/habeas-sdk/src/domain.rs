//! Application data and operations used to evaluate criteria.

use crate::graph::{CriterionPath, Fingerprint, NodePath, Record};
use std::future::Future;

/// The application's graph and evidence persistence.
///
/// Clones can share application state while each execution records its own inputs.
// {impl}`requirements.sdk.domain`
pub trait Domain: Clone + Send + Sync {
    /// Application-defined errors from domain operations.
    // {impl}`requirements.sdk.errors`
    type Error: Send;

    /// The identity of a particular node in the graph.
    // {impl}`requirements.sdk.graph.domain_types`
    type NodeId: Clone + Eq + Send + Sync;

    /// The application's representation of a node's prepared contents.
    // {impl}`requirements.sdk.graph.fingerprint_content`
    type NodeContent: NodeContents;

    /// The kind of relationship between two nodes.
    // {impl}`requirements.sdk.graph-edges.label`
    type EdgeLabel: Clone + Send + Sync;

    /// Convert a node identity to its distinct, stable path.
    // {impl}`requirements.sdk.graph.node_path_method`
    fn node_path(&self, node: &Self::NodeId) -> NodePath;

    /// Load and prepare contents for fingerprinting and presentation.
    // {impl}`requirements.sdk.graph.fingerprint_content`
    fn node_content(&self, node: &Self::NodeId) -> Result<Self::NodeContent, Self::Error>;

    /// Select targets with this label, preserving order and duplicates.
    // {impl}`requirements.sdk.graph-edges.outgoing_edges`
    fn outgoing_edges(
        &self,
        node: &Self::NodeId,
        label: Self::EdgeLabel,
    ) -> Result<Vec<Self::NodeId>, Self::Error>;

    /// Load this source/criterion pair's record; `Ok(None)` means absent.
    // {impl}`requirements.sdk.graph.evidence.evidence_storage`
    fn load_evidence(
        &self,
        node: &Self::NodeId,
        criterion: &CriterionPath,
    ) -> impl Future<Output = Result<Option<Record<Self::NodeId, Self::EdgeLabel>>, Self::Error>> + Send;

    /// Atomically replace this source/criterion pair's complete record.
    // {impl}`requirements.sdk.graph.evidence.evidence_storage`
    fn store_evidence(
        &self,
        node: &Self::NodeId,
        criterion: &CriterionPath,
        record: &Record<Self::NodeId, Self::EdgeLabel>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

/// A retained observation, ready for fingerprinting and presentation.
// {impl}`requirements.sdk.graph.fingerprint_content`
pub trait NodeContents: Send + Sync {
    /// Describe these retained contents without rereading external state.
    ///
    /// The same contents under the same rules give the same fingerprint.
    /// Retain the rules and their version; change the scheme when rules change.
    // {impl}`requirements.sdk.graph.fingerprint_content.fingerprint`
    fn fingerprint(&self) -> Fingerprint;

    /// Present the same observation described by `fingerprint`.
    ///
    /// All resource reads and fallible preparation happen when loading contents.
    // {impl}`requirements.sdk.graph.fingerprint_content.format`
    fn format(&self) -> String;
}

//! Application data and operations used to evaluate criteria.

use crate::graph::CriterionPath;
use crate::graph::Fingerprint;
use crate::graph::NodePath;
use crate::graph::Record;
use std::future::Future;

use crate::agent::AgentConfig;
use crate::agent_runtime::ConfiguredAgent;
use crate::outline::OutlineContent;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Shared outline snapshots and the agent configured for a domain.
// {impl}`requirements.sdk.validation-cache.ValidationCache`
#[derive(Clone, Default)]
pub struct ValidationCache {
    pub(crate) outlines: Arc<Mutex<HashMap<PathBuf, CachedFile>>>,
    pub(crate) sources: Option<Arc<crate::sources::Sources>>,
    pub(crate) agent: Option<Arc<ConfiguredAgent>>,
}

pub(crate) struct CachedFile {
    pub source: Arc<str>,
    pub outline: Option<OutlineContent>,
}

impl ValidationCache {
    // {impl}`requirements.sdk.validation-cache.new`
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach the project files available to search and inspection.
    // {impl}`requirements.sdk.validation-cache.with_sources`
    pub fn with_sources(mut self, sources: crate::sources::Sources) -> Self {
        self.sources = Some(Arc::new(sources));
        self
    }

    // {impl}`requirements.sdk.validation-cache.with_agent`
    pub fn with_agent(config: AgentConfig) -> Self {
        Self {
            agent: Some(Arc::new(ConfiguredAgent::new(config))),
            ..Self::new()
        }
    }
}

/// The application's graph and evidence persistence.
///
/// Clones can share application state while each execution records its own inputs.
// {impl}`requirements.sdk.domain`
pub trait Domain: Clone + Send + Sync {
    /// The cache shared by this domain and its clones.
    // {impl}`requirements.sdk.validation-cache.cache`
    fn cache(&self) -> &ValidationCache;

    // {impl}`requirements.sdk.graph.search.source_identity`
    /// Project source tools available to the configured agent.
    fn project_sources(&self) -> Option<crate::sources::SourceProject<Self>> {
        Some(crate::sources::SourceProject {
            sources: self.cache().sources.clone()?,
            domain: Arc::new(self.clone()),
        })
    }

    /// Graph identity for a project-relative outline item.
    fn source_node(
        &self,
        item: &crate::outline::OutlineId,
    ) -> Result<Self::NodeId, crate::sources::SourceError> {
        Err(crate::sources::SourceError::Invalid(format!(
            "domain has no source node mapping for {item:?}"
        )))
    }

    /// Application-defined errors from domain operations.
    // {impl}`requirements.sdk.errors`
    type Error: Send;

    /// The identity of a particular node in the graph.
    // {impl}`requirements.sdk.graph.domain_types`
    type NodeId: Clone + Eq + Send + Sync;

    /// The application's representation of a node's prepared contents.
    // {impl}`requirements.sdk.graph.node_content`
    type NodeContent: NodeContents;

    /// The kind of relationship between two nodes.
    // {impl}`requirements.sdk.graph-edges.label`
    type EdgeLabel: Clone + Send + Sync;

    /// Convert a node identity to its distinct, stable path.
    // {impl}`requirements.sdk.graph.node_path_method`
    fn node_path(&self, node: &Self::NodeId) -> NodePath;

    /// Load and prepare contents for fingerprinting and presentation.
    // {impl}`requirements.sdk.graph.node_content`
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
// {impl}`requirements.sdk.graph.node_content`
pub trait NodeContents: Send + Sync {
    /// Describe these retained contents without rereading external state.
    ///
    /// The same contents under the same rules give the same fingerprint.
    /// Retain the rules and their version; change the scheme when rules change.
    // {impl}`requirements.sdk.graph.node_content.fingerprint`
    fn fingerprint(&self) -> Fingerprint;

    /// Present the same observation described by `fingerprint`.
    ///
    /// All resource reads and fallible preparation happen when loading contents.
    // {impl}`requirements.sdk.graph.node_content.format`
    fn format(&self) -> String;
}

#[cfg(test)]
mod tests {
    use super::*;

    // {test}`requirements.sdk.validation-cache.new`
    #[test]
    fn new_cache_starts_without_files_or_agent() {
        let cache = ValidationCache::new();
        assert!(cache.outlines.lock().unwrap().is_empty());
        assert!(cache.agent.is_none());
    }
}

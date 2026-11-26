//! Application data and operations used to evaluate criteria.

use crate::graph::CriterionPath;
use crate::graph::Fingerprint;
use crate::graph::NodePath;
use crate::graph::Record;
use crate::graph::UsageStats;
use std::future::Future;
use std::hash::Hash;

use crate::agent::{AgentConfig, AgentSettings, RegisteredTool};
use crate::agent_runtime::ConfiguredAgent;
use crate::outline::OutlineContent;
use crate::procedure::Agent;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// A compact node handle assigned by one domain cache.
// {impl}`requirements.sdk.graph.NodeId`
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(transparent, bound = "")]
#[schemars(transparent, inline, bound = "")]
pub struct NodeId<D: Domain> {
    index: usize,
    #[serde(skip)]
    marker: std::marker::PhantomData<fn(D) -> D>,
}
impl<D: Domain> Copy for NodeId<D> {}
impl<D: Domain> Clone for NodeId<D> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<D: Domain> std::fmt::Debug for NodeId<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.index.fmt(f)
    }
}
impl<D: Domain> PartialEq for NodeId<D> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}
impl<D: Domain> Eq for NodeId<D> {}
impl<D: Domain> std::hash::Hash for NodeId<D> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state)
    }
}

/// Shared node identities, source snapshots, and agent configuration for a domain.
// {impl}`requirements.sdk.validation-cache.ValidationCache`
pub struct ValidationCache<D: Domain> {
    pub(crate) usage: Arc<Mutex<crate::graph::UsageStats>>,
    nodes: Arc<Mutex<indexmap::IndexSet<Arc<D::NodeData>>>>,
    pub(crate) source_addresses: Arc<Mutex<HashMap<NodeId<D>, crate::outline::OutlineData>>>,
    pub(crate) outlines: Arc<Mutex<HashMap<PathBuf, CachedFile>>>,
    pub(crate) sources: Option<Arc<crate::sources::Sources>>,
    pub(crate) agent: Option<Arc<CachedAgent<D>>>,
}

pub(crate) struct CachedAgent<D: Domain> {
    pub(crate) runner: Box<dyn Agent<D> + Send + Sync>,
    pub(crate) backend: AgentConfig,
    pub(crate) tools: Vec<Arc<dyn RegisteredTool<D>>>,
}

pub(crate) struct CachedFile {
    pub source: Arc<str>,
    pub outline: Option<OutlineContent>,
}

impl<D: Domain> Clone for ValidationCache<D> {
    fn clone(&self) -> Self {
        Self {
            usage: self.usage.clone(),
            nodes: self.nodes.clone(),
            source_addresses: self.source_addresses.clone(),
            outlines: self.outlines.clone(),
            sources: self.sources.clone(),
            agent: self.agent.clone(),
        }
    }
}

impl<D: Domain> Default for ValidationCache<D> {
    fn default() -> Self {
        Self {
            usage: Default::default(),
            nodes: Default::default(),
            source_addresses: Default::default(),
            outlines: Default::default(),
            sources: None,
            agent: None,
        }
    }
}

impl<D: Domain> ValidationCache<D> {
    // {impl}`requirements.sdk.validation-cache.new`
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot provider usage across all clones of this cache.
    // {impl}`requirements.sdk.validation-cache.usage`
    pub fn usage(&self) -> UsageStats {
        *self.usage.lock().expect("usage lock poisoned")
    }

    /// Return the handle for this durable identity in this cache.
    // {impl}`requirements.sdk.validation-cache.intern_node`
    // {impl}`requirements.sdk.validation-cache.shared_node_ids`
    pub fn intern_node(&self, data: D::NodeData) -> NodeId<D> {
        let mut nodes = self.nodes.lock().expect("node cache poisoned");
        let index = nodes
            .get_index_of(&data)
            .unwrap_or_else(|| nodes.insert_full(Arc::new(data)).0);
        NodeId {
            index,
            marker: std::marker::PhantomData,
        }
    }

    // {impl}`requirements.sdk.sources.source_nodes`
    pub(crate) fn intern_source(
        &self,
        data: D::NodeData,
        address: &crate::outline::OutlineData,
    ) -> NodeId<D> {
        let node = self.intern_node(data);
        self.source_addresses
            .lock()
            .expect("source address lock poisoned")
            .insert(node, address.clone());
        node
    }

    /// Resolve an assigned handle to its durable identity.
    // {impl}`requirements.sdk.validation-cache.node_data`
    pub fn node_data(&self, node: NodeId<D>) -> Option<Arc<D::NodeData>> {
        self.nodes
            .lock()
            .expect("node cache poisoned")
            .get_index(node.index)
            .cloned()
    }

    /// Attach the project files available to search and inspection.
    // {impl}`requirements.sdk.validation-cache.with_sources`
    pub fn with_sources(mut self, sources: crate::sources::Sources) -> Self {
        self.sources = Some(Arc::new(sources));
        self
    }

    // {impl}`requirements.sdk.validation-cache.with_agent`
    pub fn with_agent(config: impl AgentSettings<D>) -> Self
    where
        D::NodeData: serde::Serialize + serde::de::DeserializeOwned + schemars::JsonSchema,
        D::EdgeLabel: serde::de::DeserializeOwned + schemars::JsonSchema + PartialEq,
        D::Error: std::fmt::Display,
    {
        let (backend, tools) = config.into_parts();
        Self {
            agent: Some(Arc::new(CachedAgent {
                runner: Box::new(ConfiguredAgent::default()),
                backend,
                tools,
            })),
            ..Self::default()
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
    fn cache(&self) -> &ValidationCache<Self>;

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
        item: &crate::outline::OutlineData,
    ) -> Result<Self::NodeData, crate::sources::SourceError> {
        Err(crate::sources::SourceError::Invalid(format!(
            "domain has no source node mapping for {item:?}"
        )))
    }

    /// Recover a source address for a domain identity supplied independently of discovery.
    // {impl}`requirements.sdk.sources.source_nodes`
    fn source_address(&self, _data: &Self::NodeData) -> Option<crate::outline::OutlineData> {
        None
    }

    /// Application-defined errors from domain operations.
    // {impl}`requirements.sdk.errors`
    type Error: Send;

    /// The identity of a particular node in the graph.
    // {impl}`requirements.sdk.graph.domain_types`
    // {impl}`requirements.sdk.graph.NodeData`
    type NodeData: Clone + Eq + Hash + Send + Sync;

    /// The application's representation of a node's prepared contents.
    // {impl}`requirements.sdk.graph.node_content`
    type NodeContent: NodeContents;

    /// The kind of relationship between two nodes.
    // {impl}`requirements.sdk.graph-edges.label`
    type EdgeLabel: Clone + Send + Sync;

    /// Convert a node identity to its distinct, stable path.
    // {impl}`requirements.sdk.graph.node_path_method`
    fn node_path(&self, node: &NodeId<Self>) -> NodePath;

    /// Load and prepare contents for fingerprinting and presentation.
    // {impl}`requirements.sdk.graph.node_content`
    fn node_content(&self, node: &NodeId<Self>) -> Result<Self::NodeContent, Self::Error>;

    /// Select targets with this label, preserving order and duplicates.
    // {impl}`requirements.sdk.graph-edges.outgoing_edges`
    fn outgoing_edges(
        &self,
        node: &NodeId<Self>,
        label: Self::EdgeLabel,
    ) -> Result<Vec<NodeId<Self>>, Self::Error>;

    /// Load this source/criterion pair's record; `Ok(None)` means absent.
    // {impl}`requirements.sdk.graph.evidence.evidence_storage`
    fn load_evidence(
        &self,
        node: &NodeId<Self>,
        criterion: &CriterionPath,
    ) -> impl Future<Output = Result<Option<Record<Self::NodeData, Self::EdgeLabel>>, Self::Error>> + Send;

    /// Atomically replace this source/criterion pair's complete record.
    // {impl}`requirements.sdk.graph.evidence.evidence_storage`
    fn store_evidence(
        &self,
        node: &NodeId<Self>,
        criterion: &CriterionPath,
        record: &Record<Self::NodeData, Self::EdgeLabel>,
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

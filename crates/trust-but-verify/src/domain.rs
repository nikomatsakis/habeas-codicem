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

// {impl}`requirements.sdk.graph.NodeId`
// {impl}`requirements.sdk.graph.validation-context.node_reference`
/// A compact node handle assigned by one domain cache.
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(transparent, bound = "")]
#[schemars(transparent, inline, bound = "")]
pub struct NodeId<D: Domain> {
    index: usize,
    #[serde(skip)]
    marker: std::marker::PhantomData<fn(D) -> D>,
}
// {impl}`requirements.sdk.graph.NodeId`
impl<D: Domain> Copy for NodeId<D> {}
// {impl}`requirements.sdk.graph.NodeId`
impl<D: Domain> Clone for NodeId<D> {
    fn clone(&self) -> Self {
        *self
    }
}
// {impl}`requirements.sdk.graph.NodeId`
impl<D: Domain> std::fmt::Debug for NodeId<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.index.fmt(f)
    }
}
// {impl}`requirements.sdk.graph.NodeId`
impl<D: Domain> PartialEq for NodeId<D> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}
// {impl}`requirements.sdk.graph.NodeId`
impl<D: Domain> Eq for NodeId<D> {}
// {impl}`requirements.sdk.graph.NodeId`
impl<D: Domain> std::hash::Hash for NodeId<D> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state)
    }
}

// {impl}`requirements.sdk.validation-cache.shared_node_ids`
// {impl}`requirements.sdk.graph.search.source_identity`
// {impl}`requirements.sdk.validation-cache.ValidationCache`
/// Shared node identities, source snapshots, and agent configuration for a domain.
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

// {impl}`requirements.sdk.validation-cache.ValidationCache`
impl<D: Domain> Clone for ValidationCache<D> {
    // {impl}`requirements.sdk.validation-cache.shared_node_ids`
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

// {impl}`requirements.sdk.validation-cache.ValidationCache`
impl<D: Domain> Default for ValidationCache<D> {
    // {impl}`requirements.sdk.validation-cache.shared_node_ids`
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

    // {impl}`requirements.sdk.validation-cache.usage`
    /// Snapshot provider usage across all clones of this cache.
    pub fn usage(&self) -> UsageStats {
        self.usage.lock().expect("usage lock poisoned").clone()
    }

    // {impl}`requirements.sdk.validation-cache.intern_node`
    /// Return the handle for this durable identity in this cache.
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

    // {impl}`requirements.sdk.validation-cache.node_data`
    /// Resolve an assigned handle to its durable identity.
    pub fn node_data(&self, node: NodeId<D>) -> Option<Arc<D::NodeData>> {
        self.nodes
            .lock()
            .expect("node cache poisoned")
            .get_index(node.index)
            .cloned()
    }

    // {impl}`requirements.sdk.validation-cache.with_sources`
    /// Attach the project files available to search and inspection.
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
        let mut cache = Self::new();
        cache.set_agent(config);
        cache
    }

    // {impl}`requirements.sdk.validation-cache.set_agent`
    /// Select the agent for requests through this handle and future clones.
    pub fn set_agent(&mut self, config: impl AgentSettings<D>)
    where
        D::NodeData: serde::Serialize + serde::de::DeserializeOwned + schemars::JsonSchema,
        D::EdgeLabel: serde::de::DeserializeOwned + schemars::JsonSchema + PartialEq,
        D::Error: std::fmt::Display,
    {
        let (backend, tools) = config.into_parts();
        self.agent = Some(Arc::new(CachedAgent {
            runner: Box::new(ConfiguredAgent::default()),
            backend,
            tools,
        }));
    }
}

// {impl}`requirements.sdk.domain`
/// The application's graph and evidence persistence.
///
/// Clones can share application state while each execution records its own inputs.
pub trait Domain: Clone + Send + Sync {
    // {impl}`requirements.sdk.validation-cache.cache`
    /// The cache shared by this domain and its clones.
    fn cache(&self) -> &ValidationCache<Self>;

    // {impl}`requirements.sdk.sources.provider.attachment`
    /// Project source tools available to the configured agent.
    fn project_sources(&self) -> Option<crate::sources::SourceProject<Self>> {
        Some(crate::sources::SourceProject {
            sources: self.cache().sources.clone()?,
            domain: Arc::new(self.clone()),
        })
    }

    // {impl}`requirements.sdk.graph.search.source_identity`
    // {impl}`requirements.sdk.sources.source_nodes`
    /// Graph identity for a project-relative outline item.
    fn source_node(
        &self,
        item: &crate::outline::OutlineData,
    ) -> Result<Self::NodeData, crate::sources::SourceError> {
        Err(crate::sources::SourceError::Invalid(format!(
            "domain has no source node mapping for {item:?}"
        )))
    }

    // {impl}`requirements.sdk.graph.search.source_identity`
    // {impl}`requirements.sdk.sources.source_nodes`
    /// Recover a source address for a domain identity supplied independently of discovery.
    fn source_address(&self, _data: &Self::NodeData) -> Option<crate::outline::OutlineData> {
        None
    }

    // {impl}`requirements.sdk.errors`
    /// Application-defined errors from domain operations.
    type Error: Send;

    // {impl}`requirements.sdk.graph.NodeData`
    /// The identity of a particular node in the graph.
    type NodeData: Clone + Eq + Hash + Send + Sync;

    // {impl}`requirements.sdk.graph.node_content`
    /// The application's representation of a node's prepared contents.
    type NodeContent: NodeContents;

    /// The kind of relationship between two nodes.
    // {impl}`requirements.sdk.graph-edges.label`
    type EdgeLabel: Clone + Send + Sync;

    // {impl}`requirements.sdk.graph.node_path_method`
    /// Convert a node identity to its distinct, stable path.
    fn node_path(&self, node: &NodeId<Self>) -> NodePath;

    // {impl}`requirements.sdk.errors`
    // {impl}`requirements.sdk.graph.node_content`
    /// Load and prepare contents for fingerprinting and presentation.
    fn node_content(&self, node: &NodeId<Self>) -> Result<Self::NodeContent, Self::Error>;

    // {impl}`requirements.sdk.errors`
    // {impl}`requirements.sdk.graph-edges.outgoing_edges`
    /// Select targets with this label, preserving order and duplicates.
    fn outgoing_edges(
        &self,
        node: &NodeId<Self>,
        label: Self::EdgeLabel,
    ) -> Result<Vec<NodeId<Self>>, Self::Error>;

    // {impl}`requirements.sdk.errors`
    // {impl}`requirements.sdk.graph.evidence.evidence_storage`
    /// Load this source/criterion pair's record; `Ok(None)` means absent.
    fn load_evidence(
        &self,
        node: &NodeId<Self>,
        criterion: &CriterionPath,
    ) -> impl Future<Output = Result<Option<Record<Self::NodeData, Self::EdgeLabel>>, Self::Error>> + Send;

    // {impl}`requirements.sdk.errors`
    // {impl}`requirements.sdk.graph.evidence.evidence_storage`
    /// Atomically replace this source/criterion pair's complete record.
    fn store_evidence(
        &self,
        node: &NodeId<Self>,
        criterion: &CriterionPath,
        record: &Record<Self::NodeData, Self::EdgeLabel>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

// {impl}`requirements.sdk.graph.NodeContents`
/// A retained observation, ready for fingerprinting and presentation.
pub trait NodeContents: Send + Sync {
    // {impl}`requirements.sdk.graph.node_content.fingerprint`
    /// Describe these retained contents without rereading external state.
    ///
    /// The same contents under the same rules give the same fingerprint.
    /// Retain the rules and their version; change the scheme when rules change.
    fn fingerprint(&self) -> Fingerprint;

    // {impl}`requirements.sdk.graph.node_content.format`
    /// Present the same observation described by `fingerprint`.
    ///
    /// All resource reads and fallible preparation happen when loading contents.
    fn format(&self) -> String;
}

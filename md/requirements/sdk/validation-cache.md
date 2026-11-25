# Validation cache

:::{definition} validation_cache
:phrase: validation cache

A *validation cache* assigns node handles and retains file snapshots for outline lookups. It also holds
the agent configuration for criteria run on an {term}`requirements.sdk.application_domain`. Each criterion execution has its own dependency trace and agent
conversation.
:::

## Access the domain's cache

:::{req} cache

Every domain supplies its validation cache through `Domain::cache`. Repeated
calls on the domain and its clones provide access to the same node handles, cached outlines
and agent configuration.

```rust,ignore
pub trait Domain: Clone + Send + Sync {
    // ...
    fn cache(&self) -> &ValidationCache<Self>;
}
```
:::

A domain stores `ValidationCache<Self>` with or without registered tools.
`Domain::node_content` can obtain an outline by calling
`OutlineContent::from_path(self.cache(), path)`. Applications can also create the
cache before constructing the domain and pass it directly to outline constructors.

## Create a cache

:::{req} ValidationCache

`trust_but_verify::domain::ValidationCache` is a concrete, cloneable type.
Cloning it shares its state and takes constant time in the number of interned nodes and cached outlines.

```rust,ignore
pub struct ValidationCache<D: Domain> { /* private fields */ }
```

Trait implementations

```rust,ignore
impl<D: Domain> Clone for ValidationCache<D>;
impl<D: Domain> Default for ValidationCache<D>;
```
:::

:::{req} new

`ValidationCache::new()` creates an empty cache for running criteria without
an agent.

```rust,ignore
impl<D: Domain> ValidationCache<D> {
    pub fn new() -> Self;
}
```
:::

The first successful file lookup retains a snapshot. To read edited files,
give the domain a newly constructed cache. Cloning the existing cache keeps
the snapshots it has already observed.

## Intern and look up nodes

:::{req} intern_node
`intern_node` returns a handle for the supplied node data. Equal data always yields the same handle in this cache, including concurrent calls. Distinct data yields distinct handles.

```rust,ignore
impl<D: Domain> ValidationCache<D> {
    pub fn intern_node(&self, data: D::NodeData) -> NodeId<D>;
}
```
:::

:::{req} node_data
`node_data` returns the data associated with an assigned handle. An unassigned integer returns `None`.

```rust,ignore
impl<D: Domain> ValidationCache<D> {
    pub fn node_data(&self, node: NodeId<D>) -> Option<Arc<D::NodeData>>;
}
```
:::

:::{req} shared_node_ids
Interning through any clone makes the handle and its data available through every clone. A fresh cache has its own assignments.
:::

## Configure the agent

:::{req} with_agent

`ValidationCache::with_agent` accepts either a plain `AgentConfig` or an agent
configured with tools under {cite}`requirements.sdk.graph.agent-harness.with_tool`.
It creates an empty cache. Requests through the cache and its clones run that
agent with its registered tools. The SDK defers backend setup until a criterion
awaits an agent request.

```rust,ignore
impl<D: Domain> ValidationCache<D> {
    pub fn with_agent(config: impl AgentSettings<D>) -> Self
    where
        D::EdgeLabel: DeserializeOwned + JsonSchema + PartialEq,
        D::Error: Display;
}
```
:::

For example, an application that stores an optional `AgentConfig` in its
settings can construct its domain's cache when it loads those settings:

```rust,ignore
let cache = match settings.agent {
    Some(config) => ValidationCache::with_agent(config),
    None => ValidationCache::new(),
};
```

[Agent harness](graph/agent-harness.md) describes the available backends and
model settings.

## Configure project sources

:::{req} with_sources
`with_sources` attaches a project directory to the cache. Domains with project sources map each selectable outline item to a graph node under {cite}`requirements.sdk.sources.source_nodes`.

```rust,ignore
impl<D: Domain> ValidationCache<D> {
    pub fn with_sources(self, sources: Sources) -> Self;
}
```
:::

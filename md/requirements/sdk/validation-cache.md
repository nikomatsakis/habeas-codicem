# Validation cache

:::{definition} validation_cache
:phrase: validation cache

A *validation cache* retains file snapshots for outline lookups. It also holds
the agent configuration for criteria run on an {term}`requirements.sdk.application_domain`. Each criterion execution has its own dependency trace and agent
conversation.
:::

## Access the domain's cache

:::{req} cache

Every domain supplies its validation cache through `Domain::cache`. Repeated
calls on the domain and its clones provide access to the same cached outlines
and agent configuration.

```rust,ignore
pub trait Domain: Clone + Send + Sync {
    // ...
    fn cache(&self) -> &ValidationCache;
}
```
:::

A domain can store the cache in a field. A domain that wraps another domain
can return `self.inner.cache()` to share its view of the files. This lets
`Domain::node_content` obtain an outline by calling
`OutlineContent::from_path(self.cache(), path)`. Applications can also create the
cache before constructing the domain and pass it directly to outline constructors.

## Create a cache

:::{req} ValidationCache

`trust_but_verify::domain::ValidationCache` is a concrete, cloneable type.
Cloning it shares its state and takes constant time in the number of cached
outlines.

```rust,ignore
pub struct ValidationCache { /* private fields */ }
```

Trait implementations

```rust,ignore
impl Clone for ValidationCache;
impl Default for ValidationCache;
```
:::

:::{req} new

`ValidationCache::new()` creates an empty cache for running criteria without
an agent.

```rust,ignore
impl ValidationCache {
    pub fn new() -> Self;
}
```
:::

The first successful file lookup retains a snapshot. To read edited files,
give the domain a newly constructed cache. Cloning the existing cache keeps
the snapshots it has already observed.

## Configure the agent

:::{req} with_agent

`ValidationCache::with_agent(config)` creates an empty cache and selects the
agent for requests made by criteria on this domain. The SDK defers backend
setup until a criterion awaits an agent request.

```rust,ignore
impl ValidationCache {
    pub fn with_agent(config: AgentConfig) -> Self;
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
impl ValidationCache {
    pub fn with_sources(self, sources: Sources) -> Self;
}
```
:::

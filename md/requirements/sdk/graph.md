# Graph

Define the nodes, labeled edges, and successors of the application's graph.
[Contents](graph/fingerprinting.md) adds the contents associated with those
nodes.

## Identify nodes and relationships

:::{definition} node
:phrase: graph node

A thing identified in the application's graph, such as a requirement, a
function, a test, or an issue. The application's `Graph::NodeId` identifies it;
its contents and the criteria evaluated on it are separate from that identity.
:::

The graph can use enums to represent several node and relationship kinds.

:::{req} domain_types
`habeas_sdk::graph` defines the graph's associated types:

```rust,ignore
pub trait Graph: Send + Sync {
    /// The *identifier* identifies a particular node in the graph.
    type NodeId: Clone + Eq + Send + Sync;

    /// The *label* on an edge defines the kind of relationship
    /// between two nodes.
    type EdgeLabel: Clone + Send + Sync;

    /// Application-defined errors from graph operations.
    type Error: Send;

    // ... additional methods defined below ...
}
```
:::

## Edges between nodes

:::{definition} edge
:phrase: labeled edge

A directed relationship from one {term}`requirements.sdk.graph.node` to another,
with an application-defined `Graph::EdgeLabel` describing the relationship.
The targets of a node's outgoing edges with a given label are its successors
for that label. In Habeas, labels can identify a requirement's implementations
or tests.
:::

:::{req} outgoing_edges
The `habeas_sdk::graph::Graph` trait includes a method for obtaining a node's outgoing edges with a given label:

```rust,ignore
pub trait Graph: Send + Sync {
    // ...

    /// Returns the outgoing edges from `source_node` with the given `label`.
    fn outgoing_edges(
        &self,
        source_node: &Self::NodeId,
        label: Self::EdgeLabel,
    ) -> Result<Vec<Self::NodeId>, Self::Error>;
}
```
:::


:::{example} habeas-test
{cite}`requirements.sdk.graph.outgoing_edges`

Habeas defines edges like `Impl` and `Test` that respectively indicate the code which implements and tests a given requirement `R`. Its `EdgeLabel` type therefore might look something like:

```rust,ignore
enum HabeasEdge {
    Impl,
    Test,
    // ...
}
```

When outgoing edges of type `Test` are requested from a given requirement `R`, the Habeas impl will look for `#[test]` functions tagged with `R`:

```rust,ignore
// {test}`R`
#[test]
fn test_something() {
    /* ... */
}
```
:::

## Give nodes a stable path

The application chooses its `NodeId` type. The SDK also needs a
{term}`requirements.sdk.graph.node_path` that it can display in diagnostics
without knowing that type.

:::{definition} node_path
:phrase: node path

An ordered sequence of string segments describing a graph node's identity.
In the SDK, `NodePath` represents this path. `Graph::node_path` maps each
application-defined `NodeId` to a distinct path that stays stable across runs
and changes to the node's contents.
:::

The SDK exposes the following type for representing these paths:

:::{req} node_path_struct
`habeas_sdk::graph::NodePath` describes the logical structure of a node identity:

```rust,ignore
/// Defines the logical structure of a node-id.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NodePath {
    segments: Vec<String>,
}

impl NodePath {
    /// Construct a node path, preserving segment order and boundaries.
    pub fn new(segments: Vec<String>) -> Self;

    /// Read the ordered path segments.
    pub fn segments(&self) -> &[String];
}
```
:::

The application provides a method that converts from a `NodeId` to a `NodePath`:

:::{req} node_path_method
`habeas_sdk::graph::Graph::node_path` converts a node identity to its stable path:
```rust,ignore


pub trait Graph: Send + Sync {
    // ...

    /// Convert a `NodeId` into its `NodePath`.
    ///
    /// The relationship between a `NodeId` and its `NodePath` must be 1:1.
    /// That is: converting the same `NodeId` twice gives the same `NodePath` each time.
    /// And distinct `NodeId`s get distinct `NodePath`s.
    /// The mapping stays stable across runs and changes to node contents.
    fn node_path(&self, node: &Self::NodeId) -> NodePath;
}
```
:::

:::{example} hierarchical-paths
{cite}`requirements.sdk.graph.node_path_method`

For example, in the Habeas domain, one type of `NodeId` is a *requirement*, identified by a dotted path like `a.b.c`. Habeas might define its `NodeId` types as an enum like so:

```rust
pub enum HabeasNodeId {
    Requirement(Vec<String>),
    // ... additional variants for functions, etc ...
}
```

and then define `node_path` to convert that into a path like `["requirement", "a", "b", "c"]`.
:::

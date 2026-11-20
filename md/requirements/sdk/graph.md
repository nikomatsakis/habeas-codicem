# Graph

Define what nodes and edge labels mean, and how the SDK obtains contents and
outgoing relationships. These operations supply observations; they do not run
validation callbacks.

## Give nodes contents and relationships

`NodeId` identifies a thing; `NodeContent` is the application's representation
of what it contains. Contents may be text, parsed source, issue data, or another
structured value. The graph can use enums to represent several node kinds.
Reading a node does not validate it.

:::{req} domain_types
`habeas_sdk::graph` defines the graph's associated types and content lookup
operation:

```rust,ignore
pub trait Graph: Send + Sync {
    /// The *identifier* identifies a particular node in the graph.
    type NodeId: Clone + Eq + Send + Sync;

    /// The *contents* of a node defines what is given to a validator
    /// when the node is read.
    type NodeContent: Send + Sync;

    /// The *label* between edges defines the kind of verification relationship
    /// between two nodes.
    type EdgeLabel: Clone + Send + Sync;

    /// User-defined errors that can occur when fetching node content or performing
    /// other operations.
    type Error: Send;

    /// Fetch the content of a node.
    fn node_content(&self, node: &Self::NodeId) -> Result<Self::NodeContent, Self::Error>;

    // ... additional methods defined below ...
}
```
:::

## Edges between nodes

Edges in the application's verification graph define *root verification relationships* between nodes.

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

The `NodeId` type that identifies a particular node in the graph is defined by the application domain, which allows the application domain to use a precise and well-typed definition. However some parts of the SDK want to have a transparent structure that they can use for diagnostics and other purposes.

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

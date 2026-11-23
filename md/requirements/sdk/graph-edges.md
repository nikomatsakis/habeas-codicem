# Graph edges

[Graph nodes](graph.md) defines node identities and contents. Edges connect
those nodes by relationships chosen by the application. The SDK uses their
labels to look up related nodes.

## Label a relationship

:::{definition} edge
:phrase: labeled edge

A *labeled edge* is a directed relationship from one
{term}`requirements.sdk.graph.node` to another, with an application-defined
[`Domain::EdgeLabel`](../../rustdocs/habeas_sdk/domain/trait.Domain.html#associatedtype.EdgeLabel)
describing the relationship.
The targets of a node's outgoing edges with a given label are its successors
for that label. In Habeas Codicem, labels can identify a requirement's
implementations or tests.
:::

:::{req} label
{cite}`requirements.sdk.domain`

The {term}`requirements.sdk.application_domain` uses
`habeas_sdk::domain::Domain::EdgeLabel` to describe relationships between its nodes:

```rust,ignore
pub trait Domain: Clone + Send + Sync {
    /// The label describes the kind of relationship between two nodes.
    type EdgeLabel: Clone + Send + Sync;

    // ... additional graph members ...
}
```
:::

## Find a node's successors

:::{req} outgoing_edges
{cite}`requirements.sdk.domain`

The `habeas_sdk::domain::Domain` trait includes a method for obtaining a node's
successors with a given label:

```rust,ignore
pub trait Domain: Clone + Send + Sync {
    // ...

    /// Return the target node identities for this source and label.
    fn outgoing_edges(
        &self,
        source_node: &Self::NodeId,
        label: Self::EdgeLabel,
    ) -> Result<Vec<Self::NodeId>, Self::Error>;
}
```
:::

:::{example} habeas-test
{cite}`requirements.sdk.graph-edges.outgoing_edges`

Habeas Codicem defines edges like `Impl` and `Test` that respectively identify
the code implementing and testing a requirement `R`. Its `EdgeLabel` type might
look like:

```rust,ignore
enum HabeasEdge {
    Impl,
    Test,
    // ...
}
```

When outgoing edges of type `Test` are requested from requirement `R`, HC looks
for `#[test]` functions tagged with `R`:

```rust,ignore
// {test}`R`
#[test]
fn test_something() {
    /* ... */
}
```
:::

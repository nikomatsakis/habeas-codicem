# Graph edges

[Graph nodes](graph.md) defines node identities and contents. Edges connect
those nodes by relationships chosen by the application. The SDK uses their
labels to look up related nodes.

## Label a relationship

:::{definition} edge
:phrase: labeled edge

A *labeled edge* is a directed relationship from one
{term}`requirements.sdk.graph.node` to another, with an application-defined
[`Graph::EdgeLabel`](../../rustdocs/habeas_sdk/graph/trait.Graph.html#associatedtype.EdgeLabel)
describing the relationship.
The targets of a node's outgoing edges with a given label are its successors
for that label. In Habeas Codicem, labels can identify a requirement's
implementations or tests.
:::

:::{req} label
The {term}`requirements.sdk.application_domain` uses
`habeas_sdk::domain::Graph::EdgeLabel` to describe relationships between its nodes:

```rust,ignore
pub trait Graph: Types + Send + Sync {
    /// The label describes the kind of relationship between two nodes.
    type EdgeLabel: Clone + Send + Sync;

    // ... additional graph members ...
}
```
:::

## Find a node's successors

:::{req} outgoing_edges
The `habeas_sdk::domain::Graph` trait includes a method for obtaining a node's
successors with a given label:

```rust,ignore
pub trait Graph: Types + Send + Sync {
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

## Fingerprint the selected targets

A [tracked edge query](graph/context-tools.md#selecting-a-nodes-outgoing-edges)
returns the target identities together with a
{term}`requirements.sdk.graph.observation_fingerprint` of their set of stable
{term}`requirements.sdk.graph.node_path`s. This makes the selected relationships
part of the recorded inputs, so adding or removing a target can make evidence
stale.

:::{req} snapshot
For a tracked label-specific edge query, the SDK constructs a
{term}`requirements.sdk.graph.content_snapshot` from the returned targets and
their target-set fingerprint, without another query.

It sorts and deduplicates the targets' `NodePath`s and hashes an unambiguous
encoding under a versioned scheme, including when the set is empty. Changing
order, duplicate edges, or another label's relationships leaves this fingerprint
unchanged. Reading target contents is a separate observation;
`NodeContents::fingerprint` describes those contents under
{cite}`requirements.sdk.graph.fingerprint_content.fingerprint`.

The returned vector preserves the graph's ordering and duplicates.
:::

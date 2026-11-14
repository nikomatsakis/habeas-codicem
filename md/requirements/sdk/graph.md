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
`habeas_sdk::graph` MUST expose the graph's associated types and content lookup operation:

```rust,ignore
pub trait Graph: Send + Sync {
    type NodeId: Clone + Eq + Send + Sync;
    type NodeContent: Send + Sync;
    type EdgeLabel: Clone + Send + Sync;
    type Error: Send;

    // Outgoing edges and judgment metadata are specified separately.
    fn node_content(&self, node: &Self::NodeId) -> Result<Self::NodeContent, Self::Error>;
}
```

The SDK MUST leave the domain meaning of node identities, edges, and contents
to the consumer.
:::


The SDK defines an edge's shape. The application supplies its label, including
any relationship metadata. Its source is implicit in the outgoing-edge lookup;
its target identifies another node whose contents are inputs to validation.

:::{req} edges
`habeas_sdk::graph` MUST expose the following edge structure:

```rust,ignore
#[derive(Clone, Debug)]
pub struct Edge<N, L> {
    pub label: L,
    pub target: N,
}
```

Graph edges MUST use `Edge<Self::NodeId, Self::EdgeLabel>`. The source MUST be
implicit in the node supplied to `outgoing_edges`; the target MUST be available
directly in `edge.target`. The graph MUST NOT require a separate endpoint
resolution operation. The consumer MUST define the edge label type and its
meaning, including any label metadata.
:::


For our expiration requirement, the outgoing edges select `check_token` with
an `Implements` label and `expired_token_is_rejected` with a `Tests` label.
The graph answers this lookup without enumerating unrelated nodes.

:::{req} outgoing_edges
`habeas_sdk::graph` MUST expose the following interface:

```rust,ignore
pub trait Graph: Send + Sync {
    // ...

    fn outgoing_edges(
        &self,
        node: &Self::NodeId,
    ) -> Result<Snapshot<Vec<Edge<Self::NodeId, Self::EdgeLabel>>>, Self::Error>;
}
```

The SDK MUST support node validation and reachable-node traversal using
caller-supplied starting nodes and per-node outgoing-edge lookup, without
requiring enumeration or materialization of all graph nodes or edges.
Every returned edge MUST be interpreted as originating at the requested node.
The returned snapshot MUST fingerprint the complete outgoing-edge selection,
including an empty selection, target identities, and the labels and
metadata supplied to validation.
:::


A `Snapshot` pairs a returned value with its fingerprint. The
[Fingerprinting chapter](graph/fingerprinting.md) explains how the SDK checks these
observations and how the application implements `Fingerprinting`.

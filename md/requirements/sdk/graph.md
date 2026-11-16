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


## Give nodes a stable path

`NodeId` can remain an application-defined enum or other structured type.
`Graph::node_path` gives the SDK a standard address for storage, links, and
diagnostics without requiring it to understand that structure. For example,
a requirement might have the path `["requirements", "auth", "expiration"]`,
while a function has `["source", "src/auth.rs", "fn[check_token]"]`.
These are logical segments, not filesystem path components.

:::{req} node_path
`habeas_sdk::graph` MUST expose the following path type and graph operation:

```rust,ignore
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NodePath {
    pub segments: Vec<String>,
}

pub trait Graph: Send + Sync {
    // ...
    fn node_path(&self, node: &Self::NodeId) -> NodePath;
}
```

Within a domain, equal node IDs MUST yield equal paths, and distinct node IDs
MUST yield distinct paths. The mapping MUST remain stable across runs and
changes to node contents. Path equality MUST compare the ordered sequence of
segments exactly, preserving empty segments, segment boundaries, and Unicode
text without normalization. An empty sequence MUST be a valid path.
The SDK MUST NOT require conversion from `NodePath` back to `NodeId`.
:::

For example, `["source", "src/auth.rs"]` and `["source", "src", "auth.rs"]`
are different paths. The [filesystem storage helper](filesystem-storage.md)
encodes these segments into safe filesystem components. The application still
supplies serialization for any node IDs stored inside an evidence record.


The SDK defines an edge's shape. The application supplies its label, including
any relationship metadata. Its source is implicit in the outgoing-edge lookup;
its target identifies another node whose contents can be selected as inputs to
validation.

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
including an empty selection, edge order and multiplicity, target identities,
and the labels and metadata available to validation.
:::


A `Snapshot` pairs a returned value with its fingerprint. The
[Fingerprinting chapter](graph/fingerprinting.md) explains how the SDK checks these
observations and how the application implements `Fingerprinting`.

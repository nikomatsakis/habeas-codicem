# Graph nodes

The application gives each graph node an identity, a stable path, and contents.
`Graph` identifies and reads nodes. The returned `NodeContents` values supply
their fingerprints and the text used in
[agent calls](graph/agent-calls.md#format-node-contents-for-the-agent).

## Identify a node

:::{definition} node
:phrase: graph node

A *graph node* is a thing identified in the application's graph, such as a
requirement, a function, a test, or an issue. The application's
[`Graph::NodeId`](../../rustdocs/habeas_sdk/graph/trait.Graph.html#associatedtype.NodeId)
identifies it; its contents and the criteria evaluated on it are separate from
that identity.
:::

:::{req} domain_types
The {term}`requirements.sdk.application_domain` implements
`habeas_sdk::domain::Graph` to define its node-identity type. `Graph` inherits its
error type from {cite}`requirements.sdk.errors`:

```rust,ignore
pub trait Graph: Types + Send + Sync {
    /// The *identifier* identifies a particular node in the graph.
    type NodeId: Clone + Eq + Send + Sync;

    // ... additional graph members ...
}
```
:::

:::{example} node-kinds
{cite}`requirements.sdk.graph.domain_types`

An application that reviews requirements and source code can distinguish both
kinds of node with one enum:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReviewNodeId {
    Requirement(Vec<String>),
    Source {
        file: String,
        selector: String,
    },
}
```

Its `Graph` implementation sets `type NodeId = ReviewNodeId;`. For example,
`ReviewNodeId::Requirement(vec!["sdk".into(), "errors".into()])` identifies a
requirement, while `ReviewNodeId::Source { file: "src/lib.rs".into(),
selector: "fn[validate]".into() }` identifies a function.
:::

## Give nodes a stable path

The application chooses its `NodeId` type. The SDK uses a
{term}`requirements.sdk.graph.node_path` to display node identities in diagnostics.

:::{definition} node_path
:phrase: node path

A *node path* is an ordered sequence of string segments describing a graph
node's identity.
In the SDK, [`NodePath`](../../rustdocs/habeas_sdk/graph/struct.NodePath.html)
represents this path.
[`Graph::node_path`](../../rustdocs/habeas_sdk/graph/trait.Graph.html#tymethod.node_path)
maps each application-defined
[`NodeId`](../../rustdocs/habeas_sdk/graph/trait.Graph.html#associatedtype.NodeId)
to a distinct path that stays stable across runs and changes to the node's
contents.
:::

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

:::{req} node_path_method
`habeas_sdk::domain::Graph::node_path` converts a node identity to its stable path:

```rust,ignore
pub trait Graph: Types + Send + Sync {
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

For the `ReviewNodeId` enum above, a requirement with the dotted name `a.b.c` is
represented by `ReviewNodeId::Requirement(vec!["a".into(), "b".into(), "c".into()])`.
Its `node_path` implementation maps that identity to
`["requirement", "a", "b", "c"]`.
:::

## Read node contents

:::{definition} node_contents
:phrase: node contents

*Node contents* are the application-defined data read from a
{term}`requirements.sdk.graph.node` as inputs when evaluating a
{term}`requirements.sdk.graph.validation.criterion`. An agent uses these inputs
to assess whether the criterion holds in the project. They can be source text,
parsed code, issue data, or another structured value. In the SDK,
[`Graph::NodeContent`](#habeas-req-requirements.sdk.graph.fingerprint_content)
represents this value and
[`Graph::node_content`](#habeas-req-requirements.sdk.graph.fingerprint_content)
obtains it. The value implements
[`NodeContents`](#habeas-req-requirements.sdk.graph.fingerprint_content)
to supply its fingerprint and agent text.
:::

:::{req} fingerprint_content
`habeas_sdk::domain::Graph` reads an
{term}`requirements.sdk.application_domain`'s nodes as values implementing
`habeas_sdk::domain::NodeContents`. Reading includes the loading and preparation
needed to fingerprint and format the returned value; failures return the
domain's error:

```rust,ignore
pub trait Graph: Types + Send + Sync {
    // ... node identities, paths, and edge members ...

    /// The application's representation of a node's contents.
    type NodeContent: NodeContents;

    /// Load and prepare this node's contents.
    fn node_content(&self, node: &Self::NodeId) -> Result<Self::NodeContent, Self::Error>;
}

pub trait NodeContents: Send + Sync {
    // ... fingerprint and format methods introduced below ...
}
```
:::

## Format node contents for an agent

:::{req} fingerprint_content.format
`habeas_sdk::domain::NodeContents::format` presents the retained
{term}`requirements.sdk.graph.node_contents` as text for an agent:

```rust,ignore
pub trait NodeContents: Send + Sync {
    /// Present these retained contents as text for an agent.
    /// Use the same observation described by fingerprint().
    /// Everything needed for presentation is retained in this value.
    fn format(&self) -> String;

    // ... fingerprint method introduced below ...
}
```

Formatting uses the retained value and completes without further resource reads
or fallible preparation.
:::

## Read content together with its fingerprint

:::{definition} observation_fingerprint
:phrase: fingerprint

A *fingerprint* is a digest together with the scheme identifying how it was
computed, represented by
[`Fingerprint`](../../rustdocs/habeas_sdk/graph/struct.Fingerprint.html).
It describes an observation under those rules; it does not identify the node or
say whether a criterion's answer is favorable.
Both the scheme and digest participate in comparisons.
:::

:::{definition} content_snapshot
:phrase: snapshot

A *snapshot* is the contents obtained by one observation, paired with the
{term}`requirements.sdk.graph.observation_fingerprint` of those
same contents. [`Snapshot<T>`](../../rustdocs/habeas_sdk/graph/struct.Snapshot.html)
represents that pair.
:::

:::{req} fingerprint
```rust,ignore
/// A digest and the rules used to compute it.
/// Tracking and freshness checks compare both fields as opaque values.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Fingerprint {
    pub scheme: String,
    pub value: String,
}
```

This type lives in `habeas_sdk::graph`. Each `NodeContents` implementation
supplies the rules used to describe its contents.
:::

:::{req} fingerprint_content.fingerprint
`habeas_sdk::domain::NodeContents::fingerprint` describes the
{term}`requirements.sdk.graph.observation_fingerprint` of the retained contents
read under
{cite}`requirements.sdk.graph.fingerprint_content`:

```rust,ignore
pub trait NodeContents: Send + Sync {
    /// Describe these retained contents without further resource reads
    /// or querying mutable external state.
    ///
    /// The same contents under the same rules give the same fingerprint.
    /// Retain everything needed to describe this observation, including
    /// its rules and their version. When the rules change, change the scheme.
    fn fingerprint(&self) -> Fingerprint;

    // ... format method introduced above ...
}
```
:::

:::{req} snapshot
```rust,ignore
/// Contents paired with the fingerprint of that same observation.
#[derive(Clone, Debug)]
pub struct Snapshot<T> {
    pub content: T,
    pub fingerprint: Fingerprint,
}
```

This type lives in `habeas_sdk::graph`. For node reads, the SDK constructs it from
the contents returned by `Graph::node_content` and their
`NodeContents::fingerprint` under
{cite}`requirements.sdk.graph.fingerprint_content.fingerprint`, without another read.
:::

To read `check_token`, the SDK calls `Graph::node_content`, fingerprints the
returned value, and packages both in a
{term}`requirements.sdk.graph.content_snapshot`.
The procedure receives the contents; the SDK retains the fingerprint so it can
check freshness later. Conceptually, the tracked context does this:

```rust,ignore
let content = graph.node_content(&check_token_id)?;
let fingerprint = content.fingerprint();
let snapshot = Snapshot { content, fingerprint };
```

The fingerprint describes the observation the procedure examines, even if the
underlying resource changes immediately afterward. Hashing a second read could
associate one observation's contents with another observation's fingerprint.

:::{example} structural-fingerprint
{cite}`requirements.sdk.graph.fingerprint`
{cite}`requirements.sdk.graph.fingerprint_content.fingerprint`
{cite}`requirements.sdk.graph.fingerprint_content.format`
{cite}`requirements.sdk.outlines.node_contents`
{cite}`requirements.references.structural_hash`

The [outline library](outlines.md) supplies contents with a structural
fingerprint and source text ready for the agent:

```rust,ignore
use habeas_sdk::domain::NodeContents;
use habeas_sdk_outline::Outliner;
use std::path::Path;

let outliner = Outliner::default();
let select = |source: &str| {
    let document = outliner.parse(Path::new("auth.rs"), source.into()).unwrap();
    document.select("fn[check_token]").unwrap()
};

let original = select("fn check_token() { allow(); }");
let reformatted = select("fn check_token() {\n    allow();\n}");
let changed = select("fn check_token() { deny(); }");

assert_eq!(original.fingerprint(), reformatted.fingerprint());
assert_ne!(original.fingerprint(), changed.fingerprint());
assert!(original.format().contains("fn check_token()"));
```
:::

## Use outlines as node contents

[`SelectedSource`](outlines.md#habeas-req-requirements.sdk.outlines.selection)
implements `NodeContents` through the SDK. An application can use it directly
as `Graph::NodeContent`, or include it in a content enum alongside issue data
and other node kinds. The selection retains its source snapshot and supplies
both its structural fingerprint and formatted excerpt.

| Selected contents | Fingerprinting rules |
| --- | --- |
| A Rust function such as `check_token` | The selected function's [Rust outline](sources/rust.md) hash. |
| A requirement written in Markdown | The selected directive's [Markdown outline](sources/markdown.md) hash, including its body. |
| Unstructured text | The [plain-text outline](sources/plain-text.md), with hashes for individual nonblank lines and the resource root. |

Markdown structural hashing ignores ordinary whitespace, so reflowing a
requirement paragraph preserves its hash while changing its words does not.
Plain-text hashing also ignores whitespace within a line, but its line-based
outline makes line boundaries significant.

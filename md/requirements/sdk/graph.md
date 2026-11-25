# Graph nodes

The application gives each graph node an identity, a stable path, and contents.
`Domain` identifies and reads nodes. The returned `NodeContents` values supply
their fingerprints and the text used in
[agent calls](graph/agent-calls.md#format-node-contents-for-the-agent).

## Identify a node

:::{definition} node
:phrase: graph node

A *graph node* is a thing identified in the application's graph, such as a
requirement, a function, a test, or an issue. `Domain::NodeData` identifies the same node across runs. For example, a requirement's qualified name can identify it as its text and implementation edges change.
:::

:::{req} domain_types
{cite}`requirements.sdk.domain`

Each application domain defines what identifies a node across runs. Changes to a node's contents or outgoing edges preserve its identity.
:::

:::{req} NodeData
{cite}`requirements.sdk.graph.domain_types`

`Domain::NodeData` is the application-defined type that identifies a graph node across runs. Two `NodeData` values compare equal exactly when they identify the same node.

```rust,ignore
pub trait Domain: Clone + Send + Sync {
    // ...

    /// Durable identity used to intern this node.
    type NodeData: Clone + Eq + Hash + Send + Sync;

    // ...
}
```
:::

:::{example} node-kinds
{cite}`requirements.sdk.graph.NodeData`

An application that reviews requirements and source code can distinguish both
kinds of node with one enum:

```rust
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ReviewNodeData {
    Requirement(Vec<String>),
    Source {
        file: String,
        selector: String,
    },
}
```

Its `Domain` implementation sets `type NodeData = ReviewNodeData;`. For example,
`ReviewNodeData::Requirement(vec!["sdk".into(), "errors".into()])` identifies a
requirement, while `ReviewNodeData::Source { file: "src/lib.rs".into(),
selector: "fn[validate]".into() }` identifies a function.
:::

:::{req} NodeId
`trust_but_verify::domain::NodeId<D>` is an opaque, `Copy` integer handle for a node interned in a `ValidationCache<D>`. Its meaning is scoped to that cache and its clones. The domain type distinguishes handles belonging to different domain types.

```rust,ignore
pub struct NodeId<D: Domain> { /* private fields */ }
```

`NodeId<D>` implements `Copy`, `Clone`, `Debug`, `Eq`, `PartialEq`, `Hash`, `Serialize`, `Deserialize`, and `JsonSchema` for every `D: Domain`. Its JSON representation is an unsigned integer.
:::

## Give nodes a stable path

Filesystem evidence storage uses the
{term}`requirements.sdk.graph.node_path` to construct the path to saved evidence.

:::{definition} node_path
:phrase: node path

A *node path* is an ordered sequence of string segments describing a graph
node's identity.
In the SDK, [`NodePath`](../../rustdocs/trust_but_verify/graph/struct.NodePath.html)
represents this path.
[`Domain::node_path`](../../rustdocs/trust_but_verify/domain/trait.Domain.html#tymethod.node_path)
maps each handle to the stable path for its node data. The path stays the same across runs and changes to the node's contents.
:::

:::{req} node_path_struct
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

`trust_but_verify::graph::NodePath` describes a node identity as an ordered sequence of path segments.

```rust,ignore
pub struct NodePath { /* private fields */ }

impl NodePath {
    pub fn new(segments: Vec<String>) -> Self;
    pub fn segments(&self) -> &[String];
}
```

Trait implementations

```rust,ignore
impl Clone for NodePath;
impl Debug for NodePath;
impl<'de> Deserialize<'de> for NodePath;
impl Hash for NodePath;
impl PartialEq for NodePath;
impl Serialize for NodePath;
impl Eq for NodePath;
```
:::

:::{req} node_path_method
{cite}`requirements.sdk.domain`

`trust_but_verify::domain::Domain::node_path` converts a node identity to its stable path:

```rust,ignore
pub trait Domain: Clone + Send + Sync {
    // ...

    /// Convert a `NodeId` into its `NodePath`.
    ///
    /// Equal node data yields the same path across caches and runs.
    /// Distinct node data yields distinct paths.
    fn node_path(&self, node: &NodeId<Self>) -> NodePath;

    // ...
}
```
:::

:::{example} hierarchical-paths
{cite}`requirements.sdk.graph.node_path_method`

For the `ReviewNodeData` enum above, a requirement with the dotted name `a.b.c` is
represented by `ReviewNodeData::Requirement(vec!["a".into(), "b".into(), "c".into()])`.
Its `node_path` implementation maps that identity to
`["requirement", "a", "b", "c"]`.
:::

## Read node contents

:::{definition} node_contents
:phrase: node contents

*Node contents* are the application-defined data read from a
{term}`requirements.sdk.graph.node` as inputs when evaluating a
{term}`requirements.sdk.graph.criteria.criterion`. An agent uses these inputs
to assess whether the criterion holds in the project. They can be source text,
parsed code, issue data, or another structured value. In the SDK,
[`Domain::NodeContent`](../../rustdocs/trust_but_verify/domain/trait.Domain.html#associatedtype.NodeContent)
represents this value and
[`Domain::node_content`](../../rustdocs/trust_but_verify/domain/trait.Domain.html#tymethod.node_content)
obtains it. The value implements
[`NodeContents`](../../rustdocs/trust_but_verify/domain/trait.NodeContents.html)
to supply its fingerprint and agent text.
:::

:::{req} node_content
{cite}`requirements.sdk.domain`

`trust_but_verify::domain::Domain` reads an
{term}`requirements.sdk.application_domain`'s nodes as values implementing
`trust_but_verify::domain::NodeContents`. Reading includes the loading and preparation
needed to fingerprint and format the returned value; failures return the
domain's error:

```rust,ignore
pub trait Domain: Clone + Send + Sync {
    // ...

    /// The application's representation of a node's contents.
    type NodeContent: NodeContents;

    /// Load and prepare this node's contents.
    fn node_content(&self, node: &NodeId<Self>) -> Result<Self::NodeContent, Self::Error>;

    // ...
}
```
:::

:::{req} NodeContents
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

`fingerprint` identifies the version of the contents. `format` renders that same value for the agent.

```rust,ignore
pub trait NodeContents: Send + Sync {
    fn fingerprint(&self) -> Fingerprint;
    fn format(&self) -> String;
}
```
:::

## Format node contents for an agent

:::{req} node_content.format
`trust_but_verify::domain::NodeContents::format` presents the retained
{term}`requirements.sdk.graph.node_contents` as text for an agent:

```rust,ignore
pub trait NodeContents: Send + Sync {
    // ...

    /// Present these retained contents as text for an agent.
    /// Use the same observation described by fingerprint().
    /// Everything needed for presentation is retained in this value.
    fn format(&self) -> String;

    // ...
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
[`Fingerprint`](../../rustdocs/trust_but_verify/graph/struct.Fingerprint.html).
It describes an observation under those rules; it does not identify the node or
say whether a criterion's answer is favorable.
Both the scheme and digest participate in comparisons.
:::

:::{definition} content_snapshot
:phrase: snapshot

A *snapshot* is the contents obtained by one observation, paired with the
{term}`requirements.sdk.graph.observation_fingerprint` of those
same contents. [`Snapshot<T>`](../../rustdocs/trust_but_verify/graph/struct.Snapshot.html)
represents that pair.
:::

:::{req} fingerprint
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

`Fingerprint` pairs a digest with the scheme used to compute it. Each `NodeContents`
implementation chooses the scheme. Freshness checks compare both fields as opaque values.

```rust,ignore
#[non_exhaustive]
pub struct Fingerprint {
    pub scheme: String,
    pub value: String,
}

impl Fingerprint {
    pub fn new(scheme: String, value: String) -> Self;
}
```

Trait implementations

```rust,ignore
impl Clone for Fingerprint;
impl Debug for Fingerprint;
impl<'de> Deserialize<'de> for Fingerprint;
impl PartialEq for Fingerprint;
impl Serialize for Fingerprint;
impl Eq for Fingerprint;
```
:::

:::{req} node_content.fingerprint
`trust_but_verify::domain::NodeContents::fingerprint` describes the
{term}`requirements.sdk.graph.observation_fingerprint` of the retained contents
read under
{cite}`requirements.sdk.graph.node_content`:

```rust,ignore
pub trait NodeContents: Send + Sync {
    // ...

    /// Describe these retained contents without further resource reads
    /// or querying mutable external state.
    ///
    /// The same contents under the same rules give the same fingerprint.
    /// Retain everything needed to describe this observation, including
    /// its rules and their version. When the rules change, change the scheme.
    fn fingerprint(&self) -> Fingerprint;

    // ...
}
```
:::

:::{req} snapshot
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

For node reads, the SDK constructs a `Snapshot` from
the contents returned by `Domain::node_content` and their
`NodeContents::fingerprint` under
{cite}`requirements.sdk.graph.node_content.fingerprint`, without another read.

```rust,ignore
#[non_exhaustive]
pub struct Snapshot<T> {
    pub content: T,
    pub fingerprint: Fingerprint,
}

impl<T> Snapshot<T> {
    pub fn new(content: T, fingerprint: Fingerprint) -> Self;
}
```

Trait implementations

```rust,ignore
impl<T: Clone> Clone for Snapshot<T>;
impl<T: Debug> Debug for Snapshot<T>;
```
:::

To read `check_token`, the SDK calls `Domain::node_content`, fingerprints the
returned value, and packages both in a
{term}`requirements.sdk.graph.content_snapshot`.
The procedure receives the contents; the SDK retains the fingerprint so it can
check freshness later. Conceptually, the tracked context does this:

```rust,ignore
let content = domain.node_content(&check_token_id)?;
let fingerprint = content.fingerprint();
let snapshot = Snapshot::new(content, fingerprint);
```

The fingerprint describes the observation the procedure examines, even if the
underlying resource changes immediately afterward. Hashing a second read could
associate one observation's contents with another observation's fingerprint.

:::{example} structural-fingerprint
{cite}`requirements.sdk.graph.fingerprint`
{cite}`requirements.sdk.graph.node_content.fingerprint`
{cite}`requirements.sdk.graph.node_content.format`
{cite}`requirements.sdk.outlines.node_contents`
{cite}`requirements.references.structural_hash`

The [outline library](outlines.md) supplies contents with a structural
fingerprint and source text ready for the agent:

```rust,ignore
use trust_but_verify::domain::NodeContents;
use trust_but_verify::outline::OutlineContent;

let cache = trust_but_verify::domain::ValidationCache::<AppDomain>::new();
let select = |source: &str| {
    OutlineContent::from_source(&cache, "auth.rs", source.into())
        .unwrap()
        .select("fn[check_token]")
        .unwrap()
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

[`OutlineContent`](outlines.md#habeas-req-requirements.sdk.outlines.selection)
implements `NodeContents`. An application can use it directly
as `Domain::NodeContent`, or include it in a content enum alongside issue data
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

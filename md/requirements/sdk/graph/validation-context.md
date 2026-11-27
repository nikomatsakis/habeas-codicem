# Validation context

:::{definition} validation_context
:phrase: validation context

A *validation context* gives a {term}`requirements.sdk.graph.criteria.criterion`
the tools to decide whether it is satisfied and records which inputs it uses
during a single execution. An agent called by the criterion shares that context.
:::

:::{req} ValidationContext
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

The SDK exposes the {term}`requirements.sdk.graph.validation-context.validation_context`
through `ValidationContext`. Criterion code can read graph nodes and source items, follow edges,
and call `ask` to dispatch an agent with a prompt. The agent can search project
files and use the same tracked reads as criterion code.

`verify` creates a context for one execution. Each tracked read adds
the input’s identity and a fingerprint of its contents to the dependency list.
`verify` returns these dependencies with the answer. `refresh_evidence` uses
them to decide whether to rerun the criterion. [Searching project sources](search.md) explains
how the agent chooses which search excerpts to record.

```rust,ignore
pub struct ValidationContext<'d, D: Domain> { /* private fields */ }
```
:::

:::{req} ValidationContext.new
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Construct a context for a source node with an empty dependency list. `verify` does this for each execution.

```rust,ignore
impl<'d, D: Domain> ValidationContext<'d, D> {
    pub fn new(graph: &'d D, node: NodeId<D>) -> Self;
}
```
:::

## Find a node, then read its contents

Suppose a test calls a helper whose code the agent needs to examine. A search
finds the file containing the helper. The file's outline lists its functions
and gives each one an identifier. The agent can then read the helper's complete
contents using that identifier.

If the agent reads the helper, that read records a dependency and the agent
can close the search with `used: []`. If the excerpt itself answers the
question, the agent can select its tag when closing the search. Either path
records the source observation on which the answer relies. [Searching project sources](search.md) walks through both choices.

## Access the criterion's source and inputs

Before reading additional inputs, criterion code can identify its source
and select related nodes through its validation context.

:::{req} ValidationContext.node_id
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Borrow the identity of the source node being checked.

```rust,ignore
impl<'d, D: Domain> ValidationContext<'d, D> {
    pub fn node_id(&self) -> &NodeId<D>;
}
```
:::

:::{req} ValidationContext.successors
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Select targets of a labeled edge from the current source node. Record the target set even when it is empty. Reading a selected target records its contents separately.

```rust,ignore
impl<'d, D: Domain> ValidationContext<'d, D> {
    pub fn successors(
        &self,
        label: &D::EdgeLabel,
    ) -> Result<Vec<NodeId<D>>, D::Error>
    where
        D::EdgeLabel: PartialEq;
}
```
:::

:::{req} root_inputs
`node_id` identifies the criterion’s source node. `successors` returns the targets
of its matching edges in order, including duplicates. Labels match by equality. A change to the recorded target set makes prior
evidence stale, including adding an edge to an empty selection. Target contents
become dependencies through tracked reads or agent insertions. An edge selects a
related source; assessments of that target remain separate.
:::

## Ask an agent

The [Agent calls](agent-calls.md) subchapter shows how to provide a prompt,
select the inputs, and await the agent’s typed answer.

:::{req} ValidationContext.ask
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Start an agent request that will decode its submitted answer as `R`. Its graph tools share the dependency list used by direct reads under {cite}`requirements.sdk.graph.validation-context.shared_context`.

```rust,ignore
impl<'d, D: Domain> ValidationContext<'d, D> {
    pub fn ask<R>(&self) -> Ask<'_, 'd, D, R, ContextPhase>
    where
        R: DeserializeOwned + JsonSchema + Send;
}
```
:::

## Agent tools call the same context as ordinary code

The SDK's graph tools call the same inspection operations as criterion code.
The shared context prepares reads and records observations.

:::{req} shared_context
During one call to `verify`, direct reads by the criterion and
reads served by the SDK's agent tools record inputs in the same
`ValidationContext`. For example, if criterion code reads a test and the agent
then reads its helper, both node fingerprints accompany the returned answer.
A subsequent verification starts with an empty dependency list.
:::

```mermaid
flowchart TD
    Code["Criterion code: cx.read(&amp;node_id)"] --> Context["Execution's ValidationContext"]
    Agent["Agent request"] --> Tools["SDK graph tools"]
    Tools --> Context
    Context --> Graph["Domain: read prepared NodeContents"]
    Context --> Trace["Execution's recorded inputs"]
```

Source discovery uses the project files and outline module
described in [Sources and inspection](../sources.md). The application maps
source addresses into its graph and resolves their contents. The agent passes the resulting node reference to `read_node` for a tracked read.

For each operation, `.fn` identifies the Rust method and `.mcp` gives the
agent tool's name, JSON arguments, result shape, and the description sent to
the model in Rig's `ToolDefinition`. `.behavior` describes what
the caller receives, and `.inputs` specifies which observations enter evidence.
In the signatures, `?` marks an optional
field and `T[]` is an array. A `next` field holds a continuation UUID issued by the current conversation.

## Passing node identities to an agent

:::{req} node_reference
The harness represents a `NodeId<D>` as an unsigned integer. Each built-in tool that accepts a node accepts the same top-level address fields, alongside its own arguments (such as an edge `label`). `NodeAddress` in tool signatures denotes these alternatives:

| Address fields | Resolution |
| --- | --- |
| `node` | Look up the assigned ID in the domain's cache. |
| `path`, optional `location` | Resolve the project-relative file within the domain's visible sources. `location` is its outline selector and defaults to the empty selector for the file root. Resolve the item, map its `OutlineData` through `Domain::source_node`, and intern the resulting identity. |
| `data` | Deserialize the JSON value as `D::NodeData` and intern it in the domain's cache. |

For domains using the agent harness, the tool schema describes `data` using the domain’s JSON schema for `D::NodeData`. The harness uses the same JSON representation when returning and accepting an identity.

Resolving an address is discovery. The requested operation determines which observations enter evidence. Invalid addresses, unavailable files, and unknown selectors produce tool errors before the requested operation runs.
:::

:::{req} node_reference.consistency
A tool MUST also accept `node` and `data` together. Look up the existing ID and compare its stored identity with the deserialized `data` using `D::NodeData` equality. Use that ID on a match. An unassigned ID, malformed data, or a mismatch produces a tool error, even if the supplied data would be valid on its own. A `path` address may include `location` and must be used separately from `node` and `data`.
:::

:::{example} file_node_arguments
{cite}`requirements.sdk.graph.validation-context.node_reference`
{cite}`requirements.sdk.graph.validation-context.node_reference.consistency`

Suppose `list_sources` returns `{"node":7,"data":{"file":"src/parser.rs"}}`. The agent can pass that entry unchanged to `read_node`. It can also pass `{"node":7}`, `{"data":{"file":"src/parser.rs"}}`, or `{"path":"src/parser.rs"}` to read the same root. On its first call it can read a method directly with `{"path":"src/parser.rs","location":"impl[Parser]/fn[parse]"}`. Supplying `{"node":7,"data":{"file":"src/other.rs"}}` yields an identity mismatch.
:::

## Reading the contents of a graph node

Rust callers pass a cached `NodeId<D>` to read its [contents](../graph.md#read-node-contents). The agent passes the node reference under {cite}`requirements.sdk.graph.validation-context.node_reference`.

:::{req} ValidationContext.read
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Read a graph node and record the fingerprint of the returned contents under {cite}`requirements.sdk.graph.validation-context.read_node.inputs`.

```rust,ignore
impl<'d, D: Domain> ValidationContext<'d, D> {
    pub fn read(&self, node: &NodeId<D>) -> Result<D::NodeContent, D::Error>;
}
```
:::

:::{req} ValidationContext.read_snapshot
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Read a node once and return both its contents and their fingerprint. Record the same fingerprint in the dependency list.

```rust,ignore
impl<'d, D: Domain> ValidationContext<'d, D> {
    pub fn read_snapshot(
        &self,
        node: &NodeId<D>,
    ) -> Result<Snapshot<D::NodeContent>, D::Error>;
}
```
:::

:::{req} read_node.fn
Criterion code uses {cite}`requirements.sdk.graph.validation-context.ValidationContext.read` for this operation.
:::

:::{req} read_node.mcp
Give `read_node` a node address under {cite}`requirements.sdk.graph.validation-context.node_reference`. The result contains the node’s text. If more
text remains, pass `next` to `get_next_page`.

```text
read_node(NodeAddress) -> {text: string, next?: UUID}
// For a source node the result has these additional fields:
{file: string, selector: string, children: {node: NodeId, file: string, selector: string, header?: string}[]}
```
:::

:::{req} read_node.mcp.description
> `Read a graph node by ID, source address, or node data. Returns text and, when more remains, a next UUID. A
> delivered page records its enclosing node.`
:::

:::{req} read_node.behavior
The Rust caller receives the node's complete `D::NodeContent` or the domain's read error.

The tool asks the domain to read the supplied node ID, including when an assigned ID first appears in the `read_node` call.
Each page’s `text` contains the next portion of the node’s formatted contents. Formatting follows
{cite}`requirements.sdk.graph.agent-calls.judge.rendering`.
:::

:::{req} read_node.inputs
A successful `ValidationContext::read` records the node's identity and the
fingerprint of the returned contents, under
{cite}`requirements.sdk.graph.evidence.node_inputs`. A failed read records no
successful observation. The evidence becomes stale when a later read of that
node has a different fingerprint; a failure to resolve it is reported by
{cite}`requirements.sdk.graph.evidence.checking_records`.

For a Rust caller, the SDK records the fingerprint when `read` returns the
contents. For an agent, the harness puts a tool result in the next request to
the model and records the fingerprint once the provider responds successfully.
That successful exchange is *delivery*. For example, suppose the tool reads a
function that returns `3`, and someone changes it to return `4` before the
provider responds. The model saw the version that returns `3`, so the SDK
records the fingerprint it computed from that version. Reading the function
again at this point would record a fingerprint for code the model never saw.
:::

:::{req} read_node.source_children
For a source node, `read_node` includes the project-relative `file`, its `selector`, and `children` in the result. Each child entry contains `{node, file, selector}` for one immediate outline child in source order. When the language defines a declaration header for the child, its entry also includes `header`, the text of that span under {cite}`requirements.sdk.outlines.source_spans` with credential redaction. A leaf has an empty child list. Render the selected item's own source under {cite}`requirements.sdk.outlines.node_contents`. Listing a child records no dependency on its contents or on the set of children. Reading that child records its own fingerprint.
:::

:::{example} reading_an_outline_parent
{cite}`requirements.sdk.graph.validation-context.read_node.source_children`

The file `parser.rs` contains:

```rust,ignore
impl Parser { fn parse(&self) -> bool { true } }
```

The agent calls `read_node({"path":"parser.rs","location":"impl[Parser]"})`. With node ID `2` assigned to `parse`, the tool returns:

```json
{
  "file": "parser.rs",
  "selector": "impl[Parser]",
  "text": "impl Parser { … }",
  "children": [{
    "node": 2,
    "file": "parser.rs",
    "selector": "impl[Parser]/fn[parse]",
    "header": "fn parse(&self) -> bool {"
  }]
}
```

Changing `true` to `false` preserves the fingerprint recorded for the impl. The agent can call `read_node({"node":2})` to see the method body and record the method's fingerprint.
:::

## Selecting a node's outgoing edges

Select relationships by their label. A criterion can use the returned node
identities to choose additional reads.

:::{req} ValidationContext.outgoing_edges
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Select targets from any supplied node by label, returning the target IDs and the recorded target-set fingerprint under {cite}`requirements.sdk.graph.validation-context.outgoing_edges.inputs`.

```rust,ignore
impl<'d, D: Domain> ValidationContext<'d, D> {
    pub fn outgoing_edges(
        &self,
        node: &NodeId<D>,
        label: D::EdgeLabel,
    ) -> Result<Snapshot<Vec<NodeId<D>>>, D::Error>
    where
        D::EdgeLabel: PartialEq;
}
```
:::

:::{req} outgoing_edges.fn
Criterion code uses {cite}`requirements.sdk.graph.validation-context.ValidationContext.outgoing_edges` for this operation.
:::

:::{req} outgoing_edges.mcp
Give `outgoing_edges` a node address and a label of the application’s
edge-label type. Each reference in `targets` can be used in a subsequent graph tool call.

```text
outgoing_edges({…NodeAddress, label: EdgeLabel}) -> {targets: NodeId[]}
```
:::

:::{req} outgoing_edges.mcp.description
> `Inspect a known node's outgoing targets for the requested label, making those
> targets available to read_node.`
:::

:::{req} outgoing_edges.behavior
The returned vector preserves order and duplicates. The fingerprint describes
the set of stable target paths under {cite}`requirements.sdk.graph.validation-context.snapshot`.

Follow edges with `label` from the addressed node. The application's
`D::EdgeLabel` supplies the JSON schema for `label`. Each entry in `targets` is a node reference under {cite}`requirements.sdk.graph.validation-context.node_reference` that the agent can pass directly to `read_node` or `outgoing_edges`.
:::

:::{req} outgoing_edges.inputs
A successful edge selection records the source identity, the requested label,
and the fingerprint of the returned target identity set. Adding or removing an
identity from that set changes it, including adding the first identity to an
empty set. Reordering targets or repeating a target does not change it.
Changes to a target's contents do not affect this dependency: reading those
contents records a separate node dependency. These observations follow
{cite}`requirements.sdk.graph.evidence.inputs` and
{cite}`requirements.sdk.graph.evidence.checking_records`.

`ValidationContext::outgoing_edges` records the selection when returning it.
The SDK agent records the prepared selection when delivered. A failed selection
records no successful observation.
:::

:::{req} snapshot
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

For a tracked label-specific edge query, the SDK constructs a
{term}`requirements.sdk.graph.content_snapshot` from the returned targets and
their target-set fingerprint, without another query.

The public `trust_but_verify::graph::edge_fingerprint(graph, targets)` helper
computes this `Fingerprint` from the domain and a slice of target node IDs.
It fingerprints the set of distinct stable paths obtained from `Domain::node_path` under a versioned
scheme, including when the set is empty. Changing
order, duplicate edges, or another label's relationships leaves this fingerprint
unchanged. Assigning different integers to the same targets in a fresh cache preserves the fingerprint. Reading target contents is a separate observation;
`NodeContents::fingerprint` describes those contents under
{cite}`requirements.sdk.graph.node_content.fingerprint`.

The returned vector preserves the graph's ordering and duplicates.

```rust,ignore
pub fn edge_fingerprint<D: Domain>(
    graph: &D,
    targets: &[NodeId<D>],
) -> Fingerprint;
```
:::

## Reading a source item

A source item has a file path and an outline selector, such as a function
within a file. Discovery identifies files through
[source files](../sources.md#identify-source-files) in the project directory.

:::{req} ValidationContext.read_source
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Resolve an outline item to its graph node, read the contents, and record the observation under {cite}`requirements.sdk.graph.validation-context.read_source.inputs`.

```rust,ignore
impl<D: Domain> ValidationContext<'_, D> where D::Error: From<SourceError> {
    pub fn read_source(
        &self,
        item: &OutlineData,
    ) -> Result<D::NodeContent, D::Error>;
}
```
:::

:::{req} read_source.fn
Criterion code uses {cite}`requirements.sdk.graph.validation-context.ValidationContext.read_source` for this operation.
:::

:::{req} read_source.behavior
The SDK interns the node data returned by `Domain::source_node` for the `OutlineData`. `Domain::node_content` returns that node’s contents. Mapping and read failures return errors. Agents use the node reference from discovery with `read_node`.
:::

:::{req} read_source.inputs
A successful source read records the mapped node's identity and the fingerprint
of the supplied contents under
{cite}`requirements.sdk.sources.provider_observations`. Direct tracked reads
record when returning the contents; agent reads record when delivering them.
Failures record no successful observation.

The dependency becomes stale when that node's fingerprint changes. The source
mapping cannot return an untracked successful read. Calling the standalone
outline parser directly records no graph dependency; a tracked node read must
enclose its use when its contents contribute to an assessment.
:::

## Listing source files

Use a [source scope](search.md#choosing-a-source-scope) to choose which files to list.

:::{req} list_sources.fn
```rust,ignore
impl Sources {
    // ...

    /// List file identities within this scope.
    pub async fn list(&self, scope: &SourceScope) -> Result<Vec<PathBuf>, SourceError>;

    // ...
}
```
:::

:::{req} list_sources.mcp
List visible file-root node references under {cite}`requirements.sdk.graph.validation-context.node_reference`.

```text
list_sources({}) -> {files: {node: NodeId, data: NodeData}[], next?: UUID}
```
:::

:::{req} list_sources.mcp.description
> `List available project file nodes.`
:::

:::{req} list_sources.behavior
Each file entry pairs its assigned root ID with the domain identity serialized as a JSON value. The whole entry is a valid node argument under {cite}`requirements.sdk.graph.validation-context.node_reference.consistency`. Follow `next` to retrieve further pages.
:::

:::{req} list_sources.inputs
The SDK's `list_sources` result is discovery and
records neither the returned identity set nor the files' contents as
evidence inputs. Adding or removing a listed file therefore does not by
itself invalidate the evidence. Subsequent tracked reads record their own
dependencies.
:::

## Inspecting a source outline

An outline identifies the items within a source, so a caller can select one
for a content read.

:::{req} outline_source.fn
```rust,ignore
impl Sources {
    // ...

    /// Outline one file snapshot.
    pub async fn parse(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<OutlineContent, SourceError>;

    // ...
}
```
:::

:::{req} outline_source.mcp
Pass a file-root address to obtain its selectable nodes. The tool accepts an address on the first call as well as one obtained from a listing.

```text
outline_source(NodeAddress) -> {node: NodeId, items: {node: NodeId, file: string, selector: string, kind: string}[], next?: UUID}
```
:::

:::{req} outline_source.mcp.description
> `Inspect the outline of a file node.`
:::

:::{req} outline_source.behavior
The outline uses the file snapshot retained by the domain’s cache. Each item includes its kind and a reference the agent can pass to `read_node`. Follow `next` to see more items.
:::

:::{req} outline_source.inputs
The SDK's `outline_source` result is discovery and records no evidence inputs. Changes to
that outline alone do not invalidate the evidence. A subsequent tracked read
of a selected item records that item's contents and fingerprint.
:::

## Continuing a paged result

Read each page as soon as it arrives. For example, suppose `list_sources` has
one more page of file nodes. This example shows one reference from each page.

```text
list_sources({})
→ {"files":[{"node":1,"data":{"file":"src/lib.rs"}}], "next":"71d27a0b-f65f-47e5-9aa2-750335b78897"}

get_next_page({"uuid":"71d27a0b-f65f-47e5-9aa2-750335b78897"})
→ {"files":[{"node":2,"data":{"file":"src/parser.rs"}}]}
```

The second page has no `next`, so the listing is complete. The same call
continues a node read, source read, outline, or search. A UUID from a node read
returns more `text`. A UUID from an outline returns more `items` for that file.

:::{req} pages.mcp
Pass the `next` UUID exactly as returned by the previous page. The UUID
identifies the prepared result and determines the shape of the returned object.

```text
get_next_page({uuid: UUID}) -> object
```
:::

:::{req} pages.mcp.description
> `Pass the UUID from next to read the next page in the same format. When next
> is absent, this is the last page.`
:::

:::{req} pages.behavior
Each page has the same result fields as the originating tool. Text reads return
successive portions of text. Listings, outlines, and searches return successive
arrays of whole entries in their original order. Only a page with a successor
includes `next`.

Reusing a UUID returns the same page from the original snapshot, including the
same successor UUID if more remains. A malformed UUID, an unknown UUID, or a UUID
from another conversation returns a tool error under
{cite}`requirements.sdk.graph.agent-harness.tool_agent.isolation`.
:::

:::{req} pages.inputs
A delivered read page records the dependencies retained by that page under
{cite}`requirements.sdk.graph.evidence.node_inputs`. Each continuation of a
node or mapped source read retains the original node identity and content
fingerprint; reading a later page does not observe a newer version. A change
to that node's fingerprint can invalidate the evidence even if it happens
between pages. Agent tools record on delivery. Discovery pages record no evidence
inputs, just like their initial result. Preparing an undelivered page records
no read.
:::

## Collect the inputs used by the criterion

:::{req} ValidationContext.dependencies
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Return a snapshot of the observations collected so far. Later reads continue to contribute to this execution.

```rust,ignore
impl<'d, D: Domain> ValidationContext<'d, D> {
    pub fn dependencies(&self) -> Vec<Dependency<NodeId<D>, D::EdgeLabel>>;
}
```
:::

:::{req} ValidationContext.finish
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Build a storage record from the supplied criterion version, serialized output, and the dependencies collected so far. Resolve their handles to durable node data under {cite}`requirements.sdk.graph.evidence.persisted_dependencies`. The caller can pass the record to its storage backend.

```rust,ignore
impl<'d, D: Domain> ValidationContext<'d, D> {
    pub fn finish(
        &self,
        version: CriterionVersion,
        output: impl Serialize,
    ) -> Result<Record<D::NodeData, D::EdgeLabel>, serde_json::Error>;
}
```
:::

## Stay inside the tracked interface

:::{req} ValidationContext.graph
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Borrow the application domain. Calls made directly on the domain bypass the context’s dependency tracking.

```rust,ignore
impl<'d, D: Domain> ValidationContext<'d, D> {
    pub fn graph(&self) -> &'d D;
}
```
:::

:::{req} tracked_inspection
Criteria and their agents should use the SDK's context inspection
operations for information on which a judgment depends.
The context records successful node-content observations in the enclosing
validation's trace. The SDK makes no recording claim for arbitrary filesystem,
process, or network I/O outside that interface. Discovery-only results do not
count as full reads of the nodes they locate.
:::

Application code performs source parsing or network access inside its domain's
node-content implementation. A tracked inspection read then records the node
value returned by that implementation.
[Remember what a procedure actually reads](evidence.md#remember-what-a-procedure-actually-reads)
describes the resulting dependency trace.

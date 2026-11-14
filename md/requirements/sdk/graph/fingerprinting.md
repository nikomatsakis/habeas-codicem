# Fingerprinting

Implement `Fingerprinting` to define which changes to a node's contents matter.
The SDK calls your method with the node identity and contents already read.
You return a `Fingerprint`: a digest together with the scheme that identifies
how it was computed.

```rust,ignore
pub trait Fingerprinting: Graph {
    fn fingerprint(
        &self,
        node: &Self::NodeId,
        content: &Self::NodeContent,
    ) -> Fingerprint;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fingerprint {
    pub scheme: String,
    pub value: String,
}
```

:::{req} fingerprint_content
`Fingerprinting` MUST provide the `fingerprint` method shown above.

The method MUST compute the fingerprint of the supplied contents without
re-reading the node or querying mutable external state. For the same node identity and contents
under the same fingerprinting rules, it MUST return the same fingerprint.
Changes to the interpretation of those rules MUST be distinguished by the
fingerprint's `scheme`. The graph MUST make the information needed to apply
its fingerprinting rules available in `NodeId`, `NodeContent`, or its configured rules.
:::

The method is infallible. Any parsing or preparation that can fail happens
during `Graph::node_content`; `NodeContent` can carry the prepared data needed
to compute the fingerprint successfully. A graph serving several kinds of
nodes can select the calculation using the node identity or a content enum.

A matching fingerprint establishes that the input is unchanged under those
rules. It does not establish that the node is valid: a current review can still
contain a negative judgment. The SDK uses these fingerprints to
[compare saved observations](evidence.md#compare-the-saved-observations).

## Fingerprint the structure, and version the rules

Habeas uses structural fingerprints: it parses a node's source text and hashes
its structure rather than its raw bytes. The SDK supplies source-analysis
utilities for doing the same. This allows formatting edits to leave evidence
current. An application wanting finer-grained change detection can instead hash
the raw bytes, so that even whitespace changes make the observation different.

The fingerprint records both the digest and the version of the rules used to
compute it. Suppose a language extension changes how documentation comments
contribute to structural hashes. Existing applications need a way to recognize
that their saved fingerprints were computed under the earlier rules. In the
current API, `scheme` identifies those rules and their version, while `value`
holds the digest; for example, `rust-structural-v1` and `rust-structural-v2` are
illustrative scheme names.

This is versioning of the fingerprint calculation for a node's contents. It
does not require a node to have several sources or simultaneous digests. A
change of scheme makes a saved observation stale even if the digest strings
happen to match. A library update that preserves the calculation need not change
the scheme. The current SDK detects the mismatch; it does not migrate old
fingerprints or automatically rerun a review.

:::{req} fingerprint
`habeas_sdk::graph` MUST define `Fingerprint` as shown above.

Fingerprint equality MUST compare both `scheme` and `value`. The SDK’s graph
tracking and freshness comparison MUST treat these two fields as opaque values.
The graph supplies the fingerprinting rules; it may use the SDK’s source
tooling to compute fingerprints.
:::

## Read content together with its fingerprint

To read `check_token`, the SDK calls `Graph::node_content`. It then passes the returned
contents and node identity to `Fingerprinting::fingerprint`, then packages the
contents and fingerprint in a
`Snapshot`. The procedure receives the contents; the SDK retains the fingerprint
so it can check freshness later. Conceptually, the tracked context does this:

```rust,ignore
let content = graph.node_content(&check_token_id)?;
let fingerprint = graph.fingerprint(&check_token_id, &content);
let snapshot = Snapshot { content, fingerprint };
```

The fingerprint method receives the node identity and the contents already
read. It does not fetch the node again. This makes the relationship explicit: the fingerprint describes
the same observation that the procedure examines, even if the underlying
resource changes immediately afterward.

`Graph::outgoing_edges` returns a snapshot directly because its edge selection
has a different type and fingerprinting rules from `Self::NodeContent`. Its
implementation computes the selection fingerprint from the returned edges; `fingerprint` is specifically for node contents.

:::{req} snapshot
`habeas_sdk::graph` MUST define `Snapshot` as follows:

```rust,ignore
#[derive(Clone, Debug)]
pub struct Snapshot<T> {
    pub content: T,
    pub fingerprint: Fingerprint,
}
```

For node reads, the SDK MUST construct the snapshot by calling
`Fingerprinting::fingerprint` with the node identity and contents returned by `Graph::node_content`, without
reading the node again to compute its fingerprint. Graph operations returning
snapshots directly MUST supply fingerprints of the results from those same
observations. The SDK MUST NOT require `T` to carry a filesystem path or source
span.
:::

## Reuse the outline helpers

For source-backed nodes, start with the SDK's
[language outlines](../sources.md#each-language-defines-its-outline).
`habeas_sdk::outline::parse` produces an `Outline`; each `OutlineNode` exposes
its `structural_hash`. Prepare the outline while reading the node, then carry
the selected item's hash in `NodeContent` so `fingerprint` can use it without
parsing or fetching the source again.

| Node contents | Fingerprinting helper |
| --- | --- |
| A Rust function such as `check_token` | The selected function's [Rust outline](../sources/rust.md) hash. |
| A requirement written in Markdown | The selected directive's [Markdown outline](../sources/markdown.md) hash, including its body. |
| Unstructured text | The [plain-text outline](../sources/plain-text.md), with hashes for individual nonblank lines and the resource root. |

Markdown structural hashing ignores ordinary whitespace, so reflowing a
requirement paragraph preserves its hash while changing its words does not.
Plain-text hashing also ignores whitespace within a line, but its line-based
outline makes line boundaries significant. Choose the rules that match the
meaning of the node; an application can instead hash the exact bytes when every
textual change matters. The SDK's underlying word-hashing primitive is private;
the public helpers currently expose hashes through outlines.

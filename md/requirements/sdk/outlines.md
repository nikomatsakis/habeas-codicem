# Source outlines

HC can identify a source node as a filename and selector, such as
`src/auth.rs` and `fn[check_token]`. Its node-contents implementation needs to
resolve that selector against the file snapshot it read.
`habeas-sdk-outline` supplies this operation independently of graph validation
and agent execution.

## Parse a source snapshot

The application obtains the source bytes. The outline library chooses their
language from the filename and builds a structured view of that snapshot.
HC can reuse the resulting document when resolving several items in the same
file.

:::{req} independent
The `habeas-sdk-outline` crate provides language selection, source outlining,
selector resolution, and source structural hashes independently of the
Habeas SDK graph, criteria, evidence storage, and agent execution.
Applications can use the crate directly.
:::

:::{req} parser
The outline crate exposes a parser that accepts a filename and supplied source
text. It builds an outline using only that supplied text.
Filename interpretation and supported-language defaults are defined by
{cite}`requirements.sdk.outlines.defaults`.

Selected public declarations are:

```rust,ignore
use std::{path::Path, sync::Arc};

// Defined in habeas_sdk_outline.
pub struct Outliner { /* private fields */ }

impl Outliner {
    /// Interpret this supplied source snapshot using its filename.
    pub fn parse(
        &self,
        filename: &Path,
        source: Arc<str>,
    ) -> Result<ParsedSource, OutlineError>;
}
```
:::

:::{req} snapshot
A parsed document retains the source text, selected language implementation,
and outline describing that same snapshot, including the scheme used for its
structural hashes under
{cite}`requirements.sdk.sources.language_provider.fingerprint_scheme`.
Its item spans, selected text, and structural hashes are derived from that
snapshot. A cached document keeps its outline and fingerprinting scheme paired
with those same bytes.
:::

For illustration, the resolved document can hold these values:

```rust,ignore
pub struct ParsedSource {
    pub source: Arc<str>,
    pub language: Arc<dyn Language>,
    pub outline: Outline,
    pub fingerprint_scheme: String,
}
```

A source provider's parse result associates this document with its `ResourceId`.

## Use the default languages

HC constructs the default parser and passes the filename along with the source
it has already obtained.

:::{req} defaults
`Outliner::default()` includes the supplied Rust, Python, TypeScript, TSX,
Markdown, CSS, TOML, and YAML language implementations. It chooses among them
using the filename's extension, case-insensitively, with the mappings described
in the [language chapters](sources.md#each-language-defines-its-outline).
An unmatched extension or a filename without an extension uses the
[plain-text outline](sources/plain-text.md).
:::

:::{req} errors
A failure to parse source using the selected language is an error. It is
distinct from a successful outline of an empty source and does not silently
switch the source to the plain-text fallback.
:::

This usage sketch resolves the contents of HC's source node:

```rust,ignore
let outliner = Outliner::default();
let source = hc.read_source_snapshot(&address.resource)?;
let document = outliner.parse(hc.filename(&address.resource), source)?;
let selected = document.select(&address.selector)?;
Ok(HcNodeContent::Source(selected))
```

[Sources and inspection](sources.md#connect-source-items-to-graph-nodes)
describes HC's mapping into the graph.

## Supply a language implementation

A language is a value implementing the shared interface under
{cite}`requirements.sdk.sources.language_provider`. Its implementation
interprets source text into outline items and structural hashes. The supplied
language implementations use their respective grammar and naming rules.

:::{req} additional_languages
The outline parser can register an application-supplied `Arc<dyn Language>`
alongside its default language set. The SDK's source operations use registered
languages through the same `Language` interface.

```rust,ignore
impl Outliner {
    /// Add a language implementation to this parser's registry.
    pub fn with_language(self, language: Arc<dyn Language>) -> Self;
}
```
:::

## Keep an address separate from a parsed document

An `OutlineId` identifies a resource and an item within it under
{cite}`requirements.sdk.sources.source_addresses`. It can be part of HC's
`NodeId` and can be saved in an evidence record.

:::{req} address_identity
`OutlineId` has a serializable, comparable resource address and selector.
The resource address and selector alone determine its identity.
Equivalent addresses compare equally when reconstructed in another process.
An empty selector addresses the resource
root under {cite}`requirements.references.outline_selectors`.
:::

The parser retains `Arc<dyn Language>` in the resolved document. The saved
address identifies the item to resolve again; its recorded fingerprint
identifies the observation whose freshness HC needs to check.

:::{req} selection
`ParsedSource` resolves an item selector against its retained outline and
returns an owned `SelectedSource` retaining the corresponding item and its
source snapshot. The selection stays usable after the parsed-document handle is
dropped; it can share the retained source storage. Resolution
follows {cite}`requirements.references.outline_selectors`, including its rule
that identical plain-text lines are interchangeable. A malformed selector, a
missing item, or an ambiguity not permitted by that rule returns an error.

```rust,ignore
impl ParsedSource {
    /// Resolve this selector within the retained source snapshot.
    pub fn select(&self, selector: &str) -> Result<SelectedSource, ResolveError>;
}

pub struct SelectedSource { /* retained source, selection, hash, and scheme */ }

impl SelectedSource {
    /// The selected text from the retained source snapshot.
    pub fn text(&self) -> &str;

    /// Structural digest computed for this item from that snapshot.
    pub fn structural_hash(&self) -> &str;

    /// The retained scheme identifying the structural-hash rules and version.
    pub fn fingerprint_scheme(&self) -> &str;
}
```
:::

## Return a selection as node contents

:::{req} node_contents
Habeas SDK implements `habeas_sdk::domain::NodeContents` for
`habeas_sdk_outline::SelectedSource`, making an outline selection usable
directly as `Domain::NodeContent` under
{cite}`requirements.sdk.graph.fingerprint_content`.

Its `fingerprint()` returns the selected item's retained structural hash and
scheme under {cite}`requirements.sdk.graph.fingerprint_content.fingerprint`.
Its `format()` returns the selected source text under
{cite}`requirements.sdk.graph.fingerprint_content.format`.
Both operations describe the source snapshot retained by the selection.
:::

# Source outlines

Use an outline to select a function, requirement, configuration entry, or other
item from a file. `OutlineId` identifies the item; `OutlineContent` loads its
contents and supplies the text and structural fingerprint used in a review.
The filename selects an `OutlineLang`, or the caller supplies one explicitly.

:::{req} independent
The `trust-but-verify` crate's `outline` module provides language selection,
source outlining, selector resolution, and source structural hashes.
Applications can use these operations directly on files and supplied source
snapshots.
:::

## Identify a file and item

:::{definition} outline
:phrase: source outline

A *source outline* is a tree of selectable items within one source snapshot.
Its root represents the whole file. Each item has a selector, a source span,
and a structural hash. The language determines which source constructs become
items and how they are named and nested.
:::

:::{definition} selector
:phrase: item selector

An *item selector* identifies an item within a
{term}`requirements.sdk.outlines.outline`, such as `mod[auth]/fn[check_token]`.
The empty selector identifies the file root. An `OutlineId` pairs the selector
with the file's path.
:::

:::{req} address_identity
`OutlineId` identifies a file and an item within it. Its path and selector
alone determine its identity; equivalent addresses compare equally when
reconstructed in another process. Constructing an address performs no file read.
Selectors follow {cite}`requirements.references.outline_selectors`.

```rust,ignore
use std::path::PathBuf;

// Defined in trust_but_verify::outline.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OutlineId {
    pub path: PathBuf,
    /// Item within the file; an empty selector selects its root.
    pub selector: String,
}
```
:::

```rust,ignore
let function_id = OutlineId {
    path: "src/auth.rs".into(),
    selector: "mod[auth]/fn[check_token]".into(),
};
```

## Load outline contents

:::{definition} content
:phrase: outline contents

*Outline contents* are the prepared contents of one selected outline item,
represented by `OutlineContent`. The value retains its source snapshot and the
language's fingerprinting rules, so its text and fingerprint describe the same
observation.
:::

:::{req} parser
`OutlineContent::from_path` reads a file and returns its root contents.
`from_id` reads the addressed file and selects the addressed item. Both infer
the language from the filename under
{cite}`requirements.sdk.outlines.defaults`.

```rust,ignore
use std::path::Path;

pub struct OutlineContent { /* private fields */ }

impl OutlineContent {
    /// Read and outline a file, selecting its root.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, OutlineError>;

    /// Read and outline the addressed file, then resolve its selector.
    pub fn from_id(id: &OutlineId) -> Result<Self, OutlineError>;
}
```
:::

```rust,ignore
let file = OutlineContent::from_path("src/auth.rs")?;
let function = OutlineContent::from_id(&function_id)?;
```

:::{req} parser.supplied_source
A caller that already has a file snapshot can construct its root
`OutlineContent` from the filename and supplied text. This constructor uses
only that text; the filename selects the language under
{cite}`requirements.sdk.outlines.defaults`.

```rust,ignore
use std::sync::Arc;

impl OutlineContent {
    /// Outline supplied text without reading the named file.
    pub fn from_source(
        filename: impl AsRef<Path>,
        source: Arc<str>,
    ) -> Result<Self, OutlineError>;
}
```
:::

:::{req} errors
File-read and parsing failures return errors, distinct from successful contents
for an empty file. Selecting a language commits to that language's parsing
rules; a parse failure is reported rather than interpreted as plain text.
A malformed selector, a missing item, or an ambiguity not permitted by
{cite}`requirements.references.outline_selectors` also returns an error.
:::

## Select another item from the same snapshot

:::{req} selection
`OutlineContent::select` resolves a selector from the file's root against the
retained outline. It returns another owned `OutlineContent` for that item,
sharing the original source snapshot and fingerprinting rules. Selection
performs no additional file read and remains usable after the original handle
is dropped. Resolution follows
{cite}`requirements.references.outline_selectors`, including its rule that
identical plain-text lines are interchangeable. Invalid, missing, or ambiguous
selections return an error under {cite}`requirements.sdk.outlines.errors`.

```rust,ignore
impl OutlineContent {
    /// Select an item from this retained file snapshot.
    pub fn select(&self, selector: &str) -> Result<Self, OutlineError>;

    /// Address of the selected item.
    pub fn id(&self) -> &OutlineId;

    /// Selected text from the retained snapshot, including attached source.
    pub fn text(&self) -> &str;
}
```
:::

```rust,ignore
let file = OutlineContent::from_path("src/auth.rs")?;
let function = file.select("mod[auth]/fn[check_token]")?;
let module = file.select("mod[auth]")?;
```

:::{req} snapshot
An `OutlineContent` retains the source text, outline, and selected language's
fingerprinting scheme describing that same snapshot. Its item spans, selected
text, and structural hashes are derived from those bytes. A cached value keeps
these parts together; later file edits do not change the loaded value.
:::

## Inspect selectable items

:::{req} items
`OutlineContent::outline` exposes the outline of its whole retained file,
including when the content value selects one item. The outline lists the file
root first, followed by its items in source preorder. Full selectors identify
items and their ancestry under {cite}`requirements.references.outline_selectors`.

```rust,ignore
use std::ops::Range;

pub struct Outline {
    pub items: Vec<OutlineItem>,
}

pub struct OutlineItem {
    pub selector: String,
    pub kind: String,
    pub span: Range<usize>,
    pub structural_hash: String,
}

impl OutlineContent {
    pub fn outline(&self) -> &Outline;
}
```

Each span uses byte offsets into the supplied source snapshot, with an exclusive
end and boundaries valid for slicing that text. It includes the item's attached
source under the language's rules. The root has an empty selector and span
`0..source.len()`.
:::

```rust,ignore
let file = OutlineContent::from_path("src/auth.rs")?;
for item in &file.outline().items {
    println!("{}: {}", item.kind, item.selector);
}
```

## Choose an outline language

:::{definition} language_definition
:phrase: outline language

An *outline language* is a cheaply cloneable implementation of `OutlineLang`.
It claims filename extensions and defines the selectable items, their names,
and their structural hashes. Each supplied language has its own implementation,
such as `languages::Rust` or `languages::Markdown`.
:::

:::{req} language_provider
`OutlineLang` defines the language interface. Each implementation supplies its
name, claimed extensions, and outlining operation. Cloning a language shares
its prepared configuration instead of rebuilding it or copying it in proportion
to its size. Its result uses the public items, spans, and hashes under
{cite}`requirements.sdk.outlines.items`.

```rust,ignore
pub trait OutlineLang: Clone + Send + Sync {
    /// Stable name of this language implementation.
    fn name(&self) -> &str;

    /// Claimed filename extensions, without their leading dots.
    fn extensions(&self) -> &[&str];

    /// Interpret supplied text into selectable items and structural hashes.
    fn outline(&self, source: &str) -> Result<Outline, OutlineError>;

    // ... fingerprint_scheme is introduced below ...
}
```
:::

:::{req} defaults
The filename-based constructors include Rust, Python, TypeScript, TSX,
Markdown, CSS, TOML, and YAML languages. Extension matching is
case-insensitive, using each implementation's claimed extensions.
The [language chapters](#language-specific-rules) define those mappings.
A filename without a claimed extension uses the
[plain-text outline](sources/plain-text.md): a file root with one child per
nonblank line.
:::

:::{req} additional_languages
The caller can supply an `OutlineLang` explicitly when loading a file or
outlining supplied text. These constructors use that language regardless of
the filename's extension and return root contents. The value retains that
language's fingerprinting rules under
{cite}`requirements.sdk.outlines.snapshot`.

```rust,ignore
impl OutlineContent {
    pub fn from_path_with<L: OutlineLang>(
        language: &L,
        path: impl AsRef<Path>,
    ) -> Result<Self, OutlineError>;

    pub fn from_source_with<L: OutlineLang>(
        language: &L,
        filename: impl AsRef<Path>,
        source: Arc<str>,
    ) -> Result<Self, OutlineError>;
}
```
:::

```rust,ignore
use trust_but_verify::outline::languages::Rust;

let language = Rust::default();
let file = OutlineContent::from_path_with(&language, "generated.txt")?;
let function = file.select("fn[check_token]")?;
```

## Fingerprint and format the selected contents

:::{req} language_provider.fingerprint_scheme
Each language supplies the scheme identifying its structural-hash rules and
their version. A change to those rules uses a different scheme; an update
preserving the calculation can retain the scheme. Loaded contents retain the
scheme used to compute their hashes.

```rust,ignore
pub trait OutlineLang: Clone + Send + Sync {
    /// Identify the structural-hash rules and their version.
    fn fingerprint_scheme(&self) -> &str;

    // ... other language methods ...
}

impl OutlineContent {
    /// The selected item's structural digest from this snapshot.
    pub fn structural_hash(&self) -> &str;

    /// The retained scheme identifying its hashing rules and version.
    pub fn fingerprint_scheme(&self) -> &str;
}
```
:::

:::{req} node_contents
`trust_but_verify::outline::OutlineContent` implements
`trust_but_verify::domain::NodeContents`, making it usable directly as
`Domain::NodeContent` under
{cite}`requirements.sdk.graph.node_content`.

Its `fingerprint()` returns the selected item's retained structural hash and
scheme under {cite}`requirements.sdk.graph.node_content.fingerprint`.
Its `format()` returns the selected source text under
{cite}`requirements.sdk.graph.node_content.format`.
Both operations describe the source snapshot retained by the value.
:::

```rust,ignore
use trust_but_verify::domain::NodeContents;

let contents = OutlineContent::from_id(&function_id)?;
let fingerprint = contents.fingerprint();
let prompt_text = contents.format();
```

HC can include `OutlineContent` in its node-content enum alongside issue data
and other application values. Its `Domain::node_content` loads the source item;
the SDK's tracked read records that returned value's fingerprint.
[Sources and inspection](sources.md#connect-source-items-to-graph-nodes)
explains the source-node mapping.

## Language-specific rules

Each language chapter defines its claimed extensions, selectable items,
selector naming, attachments, and structural-hashing rules:

| Language | Filename extensions | Rules |
| --- | --- | --- |
| Rust | `.rs` | [Rust outlines](sources/rust.md) |
| Python | `.py`, `.pyi` | [Python outlines](sources/python.md) |
| TypeScript and TSX | `.ts`, `.mts`, `.cts`, `.tsx` | [TypeScript and TSX outlines](sources/typescript.md) |
| Markdown | `.md`, `.markdown` | [Markdown outlines](sources/markdown.md) |
| CSS | `.css` | [CSS outlines](sources/css.md) |
| TOML | `.toml` | [TOML outlines](sources/toml.md) |
| YAML | `.yml`, `.yaml` | [YAML outlines](sources/yaml.md) |
| Plain text | Fallback | [Nonblank-line outlines](sources/plain-text.md) |

:::{req} language_provider.structural_search
An outline language can additionally supply the syntax capability used by the
SDK's [structural search](sources.md#providers-can-specialize-parsing-and-search).
Requesting structural search for a language without that capability returns an
unsupported-language error. Plain text supplies outlining only.
:::

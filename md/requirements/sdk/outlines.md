# Source outlines

Use an outline to select a function, requirement, configuration entry, or other
item from a file.
[`OutlineId`](../../rustdocs/trust_but_verify/outline/struct.OutlineId.html)
identifies the item;
[`OutlineContent`](../../rustdocs/trust_but_verify/outline/struct.OutlineContent.html) loads its
contents and supplies the text and structural fingerprint used in a review.
The filename selects an
[`OutlineLang`](../../rustdocs/trust_but_verify/outline/trait.OutlineLang.html), or the
caller supplies one explicitly.

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
The empty selector identifies the file root.
[`OutlineId`](../../rustdocs/trust_but_verify/outline/struct.OutlineId.html) pairs the
selector with the file's path.
:::

:::{req} address_identity
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

`OutlineId` identifies a file and an item within it. Its path and selector
alone determine its identity; equivalent addresses compare equally when
reconstructed in another process. Constructing an address performs no file read. `root(path)` selects the whole
file. Both `new(path, selector)` and the `selector` builder validate the selector
and return an error for invalid syntax.
Selectors follow {cite}`requirements.sdk.outlines.selector_syntax`.

```rust,ignore
#[non_exhaustive]
pub struct OutlineId {
    pub path: PathBuf,
    pub selector: String,
}

impl OutlineId {
    pub fn root(path: impl Into<PathBuf>) -> Self;
    pub fn selector(self, selector: String) -> Result<Self, OutlineError>;
    pub fn new(
        path: impl Into<PathBuf>,
        selector: String,
    ) -> Result<Self, OutlineError>;
}
```

Trait implementations

```rust,ignore
impl Clone for OutlineId;
impl Debug for OutlineId;
impl<'de> Deserialize<'de> for OutlineId;
impl PartialEq for OutlineId;
impl Serialize for OutlineId;
impl Eq for OutlineId;
```
:::

```rust,ignore
let function_id = OutlineId::root("src/auth.rs").selector("mod[auth]/fn[check_token]".into())?;
```

## Parse an outline selector

:::{req} selector_syntax
An outline selector MUST be a `/`-separated path of segments from the file's
root node to an outline node; the empty selector MUST select the root node. A
named node's segment MUST be `kind[name]`. An anonymous node's segment MUST be
`kind@hash`, where `hash` is the first twelve hexadecimal digits of its identity
hash under {cite}`requirements.references.structural_hash`. Siblings whose
segments would otherwise be identical MUST be disambiguated by `#N`, their
one-based position among those siblings in source order, except `line` nodes:
identical lines are interchangeable, so a `line` selector MUST resolve when any
line with that content exists in the file. In names, `%`, `[`, `]`, `/`, `#`,
`@`, and whitespace MUST be percent-encoded as UTF-8 bytes, so selectors are
reversible. Line numbers MUST NOT form part of a selector.
:::

:::{req} selector_parser
`trust_but_verify::outline::selector::parse_selector(&str)` returns
`Result<Vec<Segment>, SelectorError>`. It checks syntax without reading a file.
The empty selector yields an empty vector. Each `Segment` exposes `kind`,
`label`, and the optional one-based `ordinal`.

For `impl[Thing/Trait]/fn[run]#2`, the first label is `Label::Named` with the
decoded parts `Thing` and `Trait`. The second segment has kind `fn`, label
`run`, and ordinal `Some(2)`. `Label::Hash` holds an anonymous item's
`HASH_DIGITS` hexadecimal digits (12), normalized to lowercase. `SelectorError`
carries the offending `selector` and a diagnostic `message`.
:::

:::{req} parse_selector
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Parses the selector into segments in root-to-item order. An empty selector yields an empty vector. Invalid syntax returns a diagnostic.

```rust,ignore
pub fn parse_selector(selector: &str) -> Result<Vec<Segment>, SelectorError>;
```
:::

:::{req} Segment
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Describes one step from a selector’s root. `base` formats the kind and label. The optional one-based ordinal disambiguates siblings with the same base.

```rust,ignore
#[non_exhaustive]
pub struct Segment {
    pub kind: String,
    pub label: Label,
    pub ordinal: Option<usize>,
}

impl Segment {
    pub fn new(kind: String, label: Label) -> Self;
    pub fn ordinal(self, ordinal: usize) -> Self;
    pub fn base(kind: &str, label: &Label) -> String;
}
```

Trait implementations

```rust,ignore
impl Clone for Segment;
impl Debug for Segment;
impl Display for Segment;
impl PartialEq for Segment;
impl Eq for Segment;
```
:::

:::{req} Label
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

A named label carries decoded name parts. An anonymous label carries the hexadecimal identity hash.

```rust,ignore
pub enum Label {
    Named(Vec<String>),
    Hash(String),
}
```

Trait implementations

```rust,ignore
impl Clone for Label;
impl Debug for Label;
impl PartialEq for Label;
impl Eq for Label;
```
:::

:::{req} HASH_DIGITS
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Number of hexadecimal digits in the identity hash of an anonymous selector segment.

```rust,ignore
pub const HASH_DIGITS: usize = 12;
```
:::

:::{req} SelectorError
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Preserves the selector and a diagnostic explaining the syntax error.

```rust,ignore
#[non_exhaustive]
pub struct SelectorError {
    pub selector: String,
    pub message: String,
}

impl SelectorError {
    pub fn new(selector: &str, message: impl Into<String>) -> Self;
}
```

Trait implementations

```rust,ignore
impl Clone for SelectorError;
impl Debug for SelectorError;
impl Display for SelectorError;
impl Error for SelectorError;
impl PartialEq for SelectorError;
impl Eq for SelectorError;
```
:::

## Load outline contents

:::{definition} content
:phrase: outline contents

*Outline contents* are the prepared contents of one selected outline item,
represented by
[`OutlineContent`](../../rustdocs/trust_but_verify/outline/struct.OutlineContent.html).
The value retains its source snapshot and the
language's fingerprinting rules, so its text and fingerprint describe the same
observation.
:::

:::{req} parser
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

`OutlineContent::from_path` reads a file and returns its root contents.
`from_id` reads the addressed file and selects the addressed item. Both infer
the language from the filename under
{cite}`requirements.sdk.outlines.defaults`.

```rust,ignore
pub struct OutlineContent { /* private fields */ }

impl OutlineContent {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, OutlineError>;
    pub fn from_id(id: &OutlineId) -> Result<Self, OutlineError>;
    pub fn from_source(
        filename: impl AsRef<Path>,
        source: Arc<str>,
    ) -> Result<Self, OutlineError>;
    pub fn from_path_with<L: OutlineLang>(
        language: &L,
        path: impl AsRef<Path>,
    ) -> Result<Self, OutlineError>;
    pub fn from_source_with<L: OutlineLang>(
        language: &L,
        filename: impl AsRef<Path>,
        source: Arc<str>,
    ) -> Result<Self, OutlineError>;
    pub fn select(&self, selector: &str) -> Result<Self, OutlineError>;
    pub fn id(&self) -> &OutlineId;
    pub fn text(&self) -> &str;
    pub fn outline(&self) -> &Outline;
    pub fn structural_hash(&self) -> &str;
    pub fn fingerprint_scheme(&self) -> &str;
}
```

Trait implementations

```rust,ignore
impl Clone for OutlineContent;
impl Debug for OutlineContent;
impl NodeContents for OutlineContent;
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
The outline API makes a best effort to recover from parse errors.
File-read and unrecoverable parsing failures return errors, distinct from
successful contents for an empty file. Selecting a language commits to that
language's parsing rules.
A malformed selector, a missing item, or an ambiguity not permitted by
{cite}`requirements.sdk.outlines.selector_syntax` also returns an error.
:::

:::{req} OutlineError
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Carries a diagnostic and the line reported by the outline operation.

```rust,ignore
#[non_exhaustive]
pub struct OutlineError {
    pub line: usize,
    pub message: String,
}

impl OutlineError {
    pub fn new(line: usize, message: String) -> Self;
}
```

Trait implementations

```rust,ignore
impl Clone for OutlineError;
impl Debug for OutlineError;
impl Display for OutlineError;
impl Error for OutlineError;
impl PartialEq for OutlineError;
impl Eq for OutlineError;
```
:::

## Select another item from the same snapshot

:::{req} selection
`OutlineContent::select` resolves a selector from the file's root against the
retained outline. It returns another owned `OutlineContent` for that item,
sharing the original source snapshot and fingerprinting rules. Selection
performs no additional file read and remains usable after the original handle
is dropped. Resolution follows
{cite}`requirements.sdk.outlines.selector_syntax`, including its rule that
identical plain-text lines are interchangeable. Invalid, missing, or ambiguous
selections return an error under {cite}`requirements.sdk.outlines.errors`.

```rust,ignore
impl OutlineContent {
    /// Select an item from this retained file snapshot.
    pub fn select(&self, selector: &str) -> Result<Self, OutlineError>;

    /// Address of the selected item.
    pub fn id(&self) -> &OutlineId;

    /// Original selected text, including attached source, before redaction.
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
items and their ancestry under {cite}`requirements.sdk.outlines.selector_syntax`.

```rust,ignore
impl OutlineContent {
    pub fn outline(&self) -> &Outline;
}
```

The item's span includes its attached source under the language's rules. The
root has an empty selector and covers the entire source snapshot.
:::

:::{req} Span
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

`trust_but_verify::outline::Span` locates text by byte offsets from the start
of the original source snapshot, including when a caller selects a nested item.
`start` is inclusive and `end` is exclusive. A valid span satisfies
`start <= end <= source.len()` and both offsets are UTF-8 character boundaries.
For `source: &str`, `&source[span]` yields the same slice as
`&source[span.start..span.end]`, with the same bounds and boundary checks.

```rust,ignore
#[non_exhaustive]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self;
}
```

Trait implementations

```rust,ignore
impl Clone for Span;
impl Copy for Span;
impl Eq for Span;
impl PartialEq for Span;
impl Debug for Span;
impl Index<Span> for str {
    type Output = str;
}
```
:::

:::{req} Outline
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Lists the selectable items from a single parse, including the root for the whole file.
`diagnostic` holds the first syntax error reported during recovery, or `None` for a clean parse.
`Outline::new` initializes it to `None`.

```rust,ignore
#[non_exhaustive]
pub struct Outline {
    pub items: Vec<OutlineItem>,
    pub diagnostic: Option<OutlineError>,
}

impl Outline {
    pub fn new(items: Vec<OutlineItem>) -> Self;
}
```

Trait implementations

```rust,ignore
impl Clone for Outline;
impl Debug for Outline;
```
:::

:::{req} OutlineItem
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

`selector` addresses the item and `kind` describes it. `span` locates the whole item under {cite}`requirements.sdk.outlines.Span`.
`structural_hash` identifies the item’s outline-relevant contents.
`attachments` lists the source constructs assigned to this item under
{cite}`requirements.sdk.outlines.attachments`. `OutlineItem::new` initializes
the list to empty. `with_attachments` sets it to the supplied list.

```rust,ignore
#[non_exhaustive]
pub struct OutlineItem {
    pub selector: String,
    pub kind: String,
    pub span: Span,
    pub structural_hash: String,
    pub attachments: Vec<OutlineAttachment>,
}

impl OutlineItem {
    pub fn new(
        selector: String,
        kind: String,
        span: Span,
        structural_hash: String,
    ) -> Self;
    pub fn with_attachments(self, attachments: Vec<OutlineAttachment>) -> Self;
}
```

Trait implementations

```rust,ignore
impl Clone for OutlineItem;
impl Debug for OutlineItem;
```
:::

```rust,ignore
let file = OutlineContent::from_path("src/auth.rs")?;
for item in &file.outline().items {
    println!("{}: {}", item.kind, item.selector);
}
```

## Inspect attachments

:::{definition} attachment
:phrase: attachment

An *attachment* is a source construct that an outline language associates
with an outline item. A documentation comment on a Rust function and a
decorator on a Python method are examples.
:::

:::{req} attachments
The outline language assigns each {term}`requirements.sdk.outlines.attachment`
to one owning item, which may be the file root. Each item lists the
attachments it owns.
:::

:::{req} attachments.order
Each item's attachments appear in source order.
:::

:::{req} attachments.empty
An item with no attachments has an empty list. A language may return empty
lists for every item.
:::

:::{req} attachments.span
Each attachment's `span` is a half-open byte range in the original file
snapshot, even when the caller has selected a nested item. Slicing that
snapshot at the span yields the complete source construct, including its
delimiters. Repeated occurrences have their own spans.
:::

:::{req} attachments.comments
The bundled languages report each recognized source comment as `Comment` on
the item that owns it under {cite}`requirements.references.outline_comments`.
Each comment in a run has its own span and the run's owner. Consumers interpret
the comment text.
:::

:::{req} attachments.fingerprint
Each language's fingerprint rules determine which attachments
contribute to the owning item's structural hash.
:::

:::{req} OutlineAttachment
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

`kind` classifies the construct and `span` locates it under
{cite}`requirements.sdk.outlines.attachments.span`. Language providers can
construct an attachment from these two fields.

```rust,ignore
#[non_exhaustive]
pub struct OutlineAttachment {
    pub kind: AttachmentKind,
    pub span: Span,
}

impl OutlineAttachment {
    pub fn new(kind: AttachmentKind, span: Span) -> Self;
}
```

Trait implementations

```rust,ignore
impl Clone for OutlineAttachment;
impl Debug for OutlineAttachment;
```
:::

:::{req} AttachmentKind
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

`Comment` covers source comments, including documentation comments.
`Annotation` covers language constructs such as attributes and decorators
that annotate a declaration. Providers use the common kind when it applies
and `Other` with a language-defined name for additional kinds.

```rust,ignore
#[non_exhaustive]
pub enum AttachmentKind {
    Comment,
    Annotation,
    Other(String),
}
```

Trait implementations

```rust,ignore
impl Clone for AttachmentKind;
impl Debug for AttachmentKind;
impl PartialEq for AttachmentKind;
impl Eq for AttachmentKind;
```
:::

## Choose an outline language

:::{definition} language_definition
:phrase: outline language

An *outline language* is a cheaply cloneable implementation of
[`OutlineLang`](../../rustdocs/trust_but_verify/outline/trait.OutlineLang.html).
It claims filename extensions and defines the selectable items, their names,
and their structural hashes. Each supplied language has its own implementation,
such as `languages::Rust` or `languages::Markdown`.
:::

:::{req} language_provider
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

`OutlineLang` defines the language interface. Each implementation supplies its
name, claimed extensions, and outlining operation. Cloning a language shares
its prepared configuration instead of rebuilding it or copying it in proportion
to its size. Its result uses the public items, spans, and hashes under
{cite}`requirements.sdk.outlines.items`. Filename extensions omit the leading dot.

```rust,ignore
pub trait OutlineLang:
    Clone
    + Send
    + Sync {
    fn name(&self) -> &str;
    fn extensions(&self) -> &[&str];
    fn outline(&self, source: &str) -> Result<Outline, OutlineError>;
    fn fingerprint_scheme(&self) -> &str;

    fn search_structure(
        &self,
        _source: &str,
        _rule: &Map<String, Value>,
    ) -> Result<Vec<Span>, OutlineError> { ... }
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
    // ...

    /// Identify the structural-hash rules and their version.
    fn fingerprint_scheme(&self) -> &str;

    // ...
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
Its `format()` returns the selected source text with recognized credentials
redacted under {cite}`requirements.sdk.sources.redaction` and
{cite}`requirements.sdk.graph.node_content.format`.
Both operations describe the source snapshot retained by the value.
`text()` retains the original selected bytes for source inspection.
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
SDK's [structural search](sources.md#outline-and-search-the-available-files).
The operation returns matched byte ranges within the supplied source:

```rust,ignore
pub trait OutlineLang: Clone + Send + Sync {
    // ...

    fn search_structure(
        &self,
        source: &str,
        rule: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Vec<Span>, OutlineError>;

    // ...
}
```

The default implementation returns an unsupported-language error. Plain text
supplies outlining only.
:::

# Sources and inspection

Source inspection helps a criterion find files, select items from their
outlines, and read the items it needs. TBV supplies filesystem inspection
through `Sources` and its agent tools. The application selects the project
directory and maps selected source items to graph nodes.

## Identify source files

:::{definition} resource
:phrase: source resource

A *source resource* is a file in the project directory. Its project-relative
path identifies the file; an outline selector identifies an item within it.
:::

:::{req} source_addresses
The source tools identify project files with project-relative `PathBuf` values.

```rust,ignore
let file = std::path::PathBuf::from("src/auth.rs");
```

An item within that file uses the path and selector in `OutlineId` under
{cite}`requirements.sdk.outlines.address_identity`.
:::

## Select the project directory

:::{req} provider
[`Sources`](../../rustdocs/trust_but_verify/sources/struct.Sources.html) supplies
built-in filesystem listing, reading, outlining, text search, and structural
search. Construct it with the project directory:

```rust,ignore
use trust_but_verify::sources::Sources;

let sources = Sources::new(repository_root);
```
:::

:::{req} Sources
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Create a filesystem inspection rooted at a project directory. Listing, reading, outlining, and both kinds of search use that directory’s visibility rules.

```rust,ignore
pub struct Sources { /* private fields */ }

impl Sources {
    pub fn new(root: impl Into<PathBuf>) -> Self;
    pub async fn list(
        &self,
        scope: &SourceScope,
    ) -> Result<Vec<PathBuf>, SourceError>;
    pub async fn read(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<SourceFile, SourceError>;
    pub async fn parse(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<OutlineContent, SourceError>;
    pub async fn search_text(
        &self,
        query: &TextQuery,
    ) -> Result<TextMatches, SourceError>;
    pub async fn search_structure(
        &self,
        query: &StructuralQuery,
    ) -> Result<StructuralMatches, SourceError>;
}
```
:::

:::{req} provider.attachment
A validator for a source-capable domain uses the supplied project directory
for its fixed source inspection tools. Attach `Sources` once when constructing
the validator; the domain supplies the mapping under
{cite}`requirements.sdk.sources.source_nodes`.

```rust,ignore
use trust_but_verify::domain::SourceNodes;
use trust_but_verify::sources::Sources;
use trust_but_verify::graph::validator::Validator;

impl<D: SourceNodes + 'static> Validator<D>
where
    D::EdgeLabel: serde::de::DeserializeOwned + schemars::JsonSchema + PartialEq,
    D::Error: std::fmt::Display,
{
    // ...

    pub fn with_sources(self, sources: Sources) -> Self;

    // ...
}
```
:::

:::{req} local_provider
Source inspection uses the project directory and respects `.gitignore`,
including nested ignore files and their exceptions. Untracked files are
available unless ignored. The same scope applies to listing, reading,
outlining, text search, and structural search, including requests naming a
file or subdirectory directly.

Files outside the project directory and Git metadata are excluded. A direct
read of an excluded file returns an error.
:::

:::{req} provider.errors
Listing and reading failures return errors, distinct from successful empty
listings and successful reads of empty files.
:::

:::{req} SourceError
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Carries an I/O failure or a diagnostic for invalid source input, such as an unknown language or invalid UTF-8 in a request for text.

```rust,ignore
pub enum SourceError {
    Io(std::io::Error),
    Invalid(String),
}
```

Trait implementations

```rust,ignore
impl Debug for SourceError;
impl Display for SourceError;
impl Error for SourceError;
impl From<std::io::Error> for SourceError;
```
:::

## Read a project file

:::{req} read_file
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

`Sources::read` reads the bytes of one visible project-relative file. Its public
Rust signature is `pub async fn read(&self, path: impl AsRef<Path>) -> Result<SourceFile, SourceError>`.
`SourceFile::bytes` is an `Arc<[u8]>`. An empty file yields an empty byte slice.
`SourceFile::text()` borrows those bytes as `&str` when they are valid UTF-8 and
returns `SourceError` for invalid UTF-8. `SourceFile::language` is an
`Option<LanguageId>`. The built-in filesystem reader leaves it `None`.

```rust,ignore
#[non_exhaustive]
pub struct SourceFile {
    pub bytes: Arc<[u8]>,
    pub language: Option<LanguageId>,
}

impl SourceFile {
    pub fn new(bytes: Arc<[u8]>) -> Self;
    pub fn language(self, language: LanguageId) -> Self;
    pub fn text(&self) -> Result<&str, SourceError>;
}
```

Trait implementations

```rust,ignore
impl Clone for SourceFile;
impl Debug for SourceFile;
```
:::

## Identify a source language

:::{req} language_id
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

`LanguageId` is a public struct with a private string. Construct one from `&str`
or `String`, for example `LanguageId::from("rust")`. `as_str()` borrows the
original name. Serde represents the identifier as a string.

`StructuralQuery::language` takes a `LanguageId`. Construction preserves the
spelling. Structural search looks up the name among the configured languages
and returns an error for an unknown name.

```rust,ignore
pub struct LanguageId(/* private fields */);

impl LanguageId {
    pub fn as_str(&self) -> &str;
}
```

Trait implementations

```rust,ignore
impl Clone for LanguageId;
impl Debug for LanguageId;
impl<'de> Deserialize<'de> for LanguageId;
impl From<&str> for LanguageId;
impl From<String> for LanguageId;
impl Hash for LanguageId;
impl PartialEq for LanguageId;
impl Serialize for LanguageId;
impl Eq for LanguageId;
```
:::

## Outline and search the available files

:::{req} provider_defaults
TBV selects a compiled-in outline language from the file extension and uses
the plain-text outline for other files. Text search uses regular expressions.
Structural search uses the requested language's syntax capability under
{cite}`requirements.sdk.outlines.language_provider.structural_search`.
:::

:::{req} provider_defaults.composition
Outlining reads one file snapshot and parses its supplied bytes. Text search
lists files in scope and searches their bytes. Structural search lists files
in scope and parses their supplied snapshots. These operations share the
filesystem scope under {cite}`requirements.sdk.sources.local_provider`.
:::

:::{req} provider_results
Inspection preserves the requested path scope and the selected language for
outlining and structural search.
:::

:::{req} provider_results.parse_snapshot
A parse result retains the file identity, bytes, and outline prepared from
those bytes together. Selecting an item from that result uses the retained
snapshot under {cite}`requirements.sdk.outlines.snapshot`.
:::

:::{req} provider_results.search_completeness
Search results distinguish complete results from truncated or incomplete
ones. An operation failure returns an error, rather than a complete empty
result.
:::

## Connect source items to graph nodes

The application maps discovered source items to its graph. For HC,
`src/auth.rs` with selector `fn[check_token]` identifies a source node.

:::{req} source_nodes
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

An application supporting tracked source inspection maps `OutlineId` to its
graph's `NodeId`. The mapped node represents the requested path and selector;
an unsupported or invalid address returns an error. The SDK reads the mapped
node through the application's `Domain` interface.

```rust,ignore
pub trait SourceNodes: Domain {
    fn source_node(&self, item: &OutlineId) -> Result<Self::NodeId, Self::Error>;
}
```
:::

For a source node, HC's `Domain::node_content` resolves its `OutlineId` and
returns an `OutlineContent`. The returned value supplies its fingerprint and
agent text through `NodeContents` under
{cite}`requirements.sdk.outlines.node_contents`.

:::{req} provider_observations
The tracked source-read operation resolves the selected `OutlineId` through
the application's source-node mapping and reads that node through `Domain`.
Successful reads delivered to the caller record that node's identity and the
fingerprint of its supplied contents.

The graph's recording and freshness contracts apply under
{cite}`requirements.sdk.graph.evidence.node_inputs` and
{cite}`requirements.sdk.graph.evidence.checking_records`.
:::

## Inspection runs through the tracked context

:::{req} redaction
TBV automatically redacts recognized credentials in source excerpts delivered
by its inspection tools and in node text supplied to its agent requests.
The built-in rules replace recognized secret regions with redaction markers
while preserving line breaks. SDK-supplied outline languages use the same redacted
regions, so changing only a recognized credential does not change its
structural fingerprint.

Loaded source snapshots retain the original text for selection and spans;
redaction applies when preparing model-visible text and structural hashes.
Filesystem evidence storage preserves the supplied record. Applications supply
their own answer values and remain responsible for what those answers contain.
Habeas additionally recognizes explicit credentials in its configuration files
under {cite}`requirements.authoring.credentials`.
:::

Deterministic criterion code and SDK agent tools use the same inspection
operations. A direct tracked read records its returned input. An agent read
records the prepared input when it is delivered, including continuation
pages.

```mermaid
flowchart TD
    Agent["SDK agent tools"] --> Context["Tracked inspection context"]
    Code["Deterministic criterion"] --> Context
    Context --> Mapping["Application maps OutlineId to NodeId"]
    Mapping --> Domain["Application Domain loads the selected node"]
    Domain --> Trace["SDK records contents and fingerprint"]
```

Listing, outlining, and searching help choose nodes to read. A subsequent
tracked read records the selected node.
[Validation context](graph/validation-context.md) describes tracked reads and file discovery.
[Searching project sources](graph/search.md) explains how an agent can record
an excerpt it used directly.

[Source outlines](outlines.md) introduces `OutlineId`, `OutlineContent`, and
`OutlineLang`. Its [language chapters](outlines.md#language-specific-rules)
define the item kinds, selectors, and fingerprints for each language.

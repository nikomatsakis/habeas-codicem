# Sources and inspection

Suppose the agent reviewing our expiration requirement wants to inspect
`check_token` in `src/auth.rs`:

```rust
fn check_token(expires_at: u64, now: u64) -> bool {
    now < expires_at
}
```

The application can read that file, but the review needs the function rather
than an arbitrary range of lines. It may also need to find other calls to
`check_token`. The SDK's source machinery gives these operations a common home:
reading resources, identifying items within them, and searching their contents.

## Separate where content comes from from how it is interpreted

For this example, a local source provider supplies the bytes of `src/auth.rs`.
A registered Rust language definition supplies a tree-sitter grammar and rules
for identifying functions. The SDK's default parser combines them to produce
an outline containing `fn[check_token]`.

```mermaid
flowchart TD
    Read["Inspect check_token"] --> Provider["Source provider reads src/auth.rs"]
    Provider --> Bytes["Source bytes from one read"]
    Bytes --> Parse["SDK default parser"]
    Rust["Registered Rust grammar and outline rules"] --> Parse
    Parse --> Outline["Outline item: fn[check_token]"]
    Outline --> Contents["Function contents and fingerprint"]
```

These responsibilities generalize beyond a checkout. A provider could read a
Git revision, an editor buffer, generated content, or a database. The same
language definition can interpret Rust from any of those locations. Conversely,
one local provider can serve Rust, Python, and Markdown resources.

The graph has no mandatory source-node variant. Habeas can put
`Source(OutlineId)` in its node identity enum and delegate reads of that variant
to source inspection. Other graph nodes, such as GitHub issues, can be resolved
without passing through a parser at all.

## Identify something before reading it

A declaration and a source item each need an identity. An identity tells us
*which thing* to read; it does not tell us which version we will get. A function
can keep its identity while its body changes.

For source code, the SDK separates the resource from the item within it:

```rust,ignore
let function = OutlineId {
    resource: ResourceId {
        scheme: "repo".into(),
        location: "src/auth.rs".into(),
    },
    selector: "fn[check_token]".into(),
};
```

Here `repo` is a namespace interpreted by Habeas. The location identifies a
resource in that namespace; the selector identifies a function within it.
Another application could resolve `https` resources or use `generated` for the
output of a generator. Constructing an address does not fetch anything.

A graph node does not have to be source code. A requirement might be identified
by its name, and an imported review might be identified by a database key. The
application chooses its node identity type. None of these identities requires a
filename or source span. When we parse text, the resulting outline spans refer
to that particular text snapshot, wherever the text came from.

:::{req} source_addresses
```rust,ignore
// Public address types in habeas_sdk::resource.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceId {
    /// Namespace interpreted by the application, such as "repo".
    pub scheme: String,
    /// Address within that namespace, such as "src/auth.rs".
    pub location: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutlineId {
    pub resource: ResourceId,
    /// Item within the resource, such as "fn[check_token]".
    pub selector: String,
}
```

The application defines resource resolution, including any URI parsing or
resource fetching.
:::

## The application supplies a source provider

For a checkout, listing resources might return `src/auth.rs` and
`tests/auth.rs`; reading either resource returns its bytes. A database-backed
provider could expose the same logical resources without creating local files.
The application chooses the provider and the set of resources it exposes.

The `SourceProvider` interface starts with listing and reading. This sketch
shows their roles; names and types used only in the sketch are illustrative:

```rust,ignore
pub trait SourceProvider {
    type Error;

    /// Enumerate resources in the requested scope, including nonlocal resources.
    async fn list(
        &self,
        scope: &SourceScope,
    ) -> Result<Vec<ResourceId>, Self::Error>;

    /// Read one resource's bytes. An empty resource is a successful read.
    async fn read(
        &self,
        id: &ResourceId,
    ) -> Result<SourceFile, Self::Error>;

    // Parsing and search methods follow below.
}

pub struct SourceFile {
    pub bytes: Arc<[u8]>,
    /// Explicit language selection, or None to select by resource name.
    pub language: Option<LanguageId>,
}
```

A resource's optional language identifier selects a registered definition
explicitly. This is useful for generated content with no file extension. When
there is no explicit identifier, the registry can select a language using the
resource name. The source provider does not need to implement a grammar.

:::{req} provider
The SDK exposes a source-provider interface for enumerating resource identities
within a requested scope and reading resource bytes. Resources need no local
filesystem path.
:::

:::{req} provider.errors
Listing and reading failures are errors, distinct from successful empty
listings and successful reads of empty resources.
:::

The SDK also supplies a local-filesystem implementation. It uses the same
interface as an application-supplied provider, so choosing local storage does
not change the graph model or the inspection operations available to a review.

:::{req} local_provider
The SDK supplies a local-filesystem provider for listing and reading resources
within an application-selected scope.
:::

## Register the languages that interpret those resources

For `check_token`, the Rust grammar recognizes a `function_item`; the Rust
outline definition maps it to the kind `fn` and takes its name from the
function's name field. A Python definition maps `function_definition` to `def`.
The shared outline engine uses these mappings to name items and organize them
by their syntactic containment.

An application supplies a collection of these language definitions. The default
collection is convenient for Habeas; a different application can replace it or
register additional languages. In illustrative assembly code:

```rust,ignore
let sources = SourceAnalysis::new(
    LocalSources::new(repository_root),
    LanguageRegistry::new()
        .register(Rust)
        .register(Python),
);
```

`habeas-language` defines the existing provider trait and profile vocabulary.
Each `habeas-language-*` crate supplies a language implementation, and
`habeas-languages` assembles the default collection. A language provider defines
parsing and outline interpretation; a source provider supplies resources and
inspection operations. Neither defines what makes a requirement valid.

:::{req} language_provider
The language interface lives in `habeas-language`:

```rust,ignore
use tree_sitter::Language;

pub trait LanguageProvider: Sync {
    /// Rules mapping syntax to named outline items.
    fn profile(&'static self) -> &'static Profile;
    /// The tree-sitter grammar used to parse the resource.
    fn grammar(&self) -> fn() -> Language;
}
```
:::

`Profile` describes outline interpretation; `Language` is `tree_sitter::Language`.

## Providers can specialize parsing and search

Suppose a database provider already has an index of every call to `check_token`.
Reading every resource to repeat that search would discard the benefit of the
index. Parsing and search therefore belong on the source-provider interface,
with reusable SDK implementations as defaults.

Continuing the illustrative trait above:

```rust,ignore
async fn parse(
    &self,
    id: &ResourceId,
    languages: &LanguageRegistry,
) -> Result<ParsedSource, Self::Error> {
    defaults::parse(self, id, languages).await
}

async fn search_text(
    &self,
    query: &TextQuery,
) -> Result<TextMatches, Self::Error> {
    defaults::search_text(self, query).await
}

async fn search_structure(
    &self,
    query: &StructuralQuery,
    languages: &LanguageRegistry,
) -> Result<StructuralMatches, Self::Error> {
    defaults::search_structure(self, query, languages).await
}
```

The default text search enumerates resources and reads their contents. The
default parser reads a resource and applies its selected language definition.
The default structural search enumerates resources and calls the provider's
parsing operation. A provider that overrides parsing can therefore reuse the
shared structural-search implementation.

:::{req} provider_defaults
Source providers can override parsing, text search, and structural search.
The SDK supplies defaults for all three, so listing, reading, and a supplied
language registry suffice to support them.
:::

:::{req} provider_defaults.composition
The defaults compose through the provider's operations:

| Default operation | Uses |
| --- | --- |
| Parsing | Provider reading and the selected language definition |
| Text search | Provider listing and reading |
| Structural search | Provider listing and parsing, including any parsing override |
:::

An override changes how an operation is executed. A database index still needs
to answer the requested search over the requested scope; approximate index
matches can be candidates that the provider verifies. A cached parse must stay
paired with the source bytes it describes. Reading the file again to attach
newer bytes would produce an inconsistent result.

:::{req} provider_results
An override preserves the requested scope and semantics, including the selected
language definition for parsing and structural search.
:::

:::{req} provider_results.parse_snapshot
A parse result retains the source snapshot it describes. A cached parse stays
paired with those bytes, rather than bytes from a different source snapshot.
:::

:::{req} provider_results.search_completeness
Search results distinguish complete results from truncated or incomplete
ones. An operation failure is an error, rather than a complete empty result.
:::

## Inspection runs through the tracked context

The requirement-review agent asks a tool to read `check_token`. That tool calls
the inspection context, which dispatches to the configured source provider.
Deterministic validation code calls the same context directly. Both routes
observe the same source operations, regardless of whether the provider uses
SDK defaults or optimized implementations.

```mermaid
flowchart TD
    Agent["Agent tool adapter"] --> Context["Tracked inspection context"]
    Code["Deterministic procedure"] --> Context
    Context --> Provider["Configured source provider"]
    Provider --> Default["SDK default implementation"]
    Provider --> Custom["Provider override"]
    Default --> Result["Observation returned to context"]
    Custom --> Result
    Result --> Return["Supply result"]
    Return --> Record["Record delivered source-node reads"]
```

The graph's [input-recording contracts](graph/evidence.md#remember-what-a-procedure-actually-reads)
cover node reads and outgoing-edge selections. Providers execute operations;
the inspection context records source-node reads delivered to the procedure.
An override cannot bypass recording a delivered read merely because it answered
from a cache or database.

:::{req} provider_observations
The tracked inspection context records successful source-node reads delivered
to the procedure as assessment dependencies, whether the provider uses defaults
or overrides. Each fingerprint describes the contents delivered to the caller.
:::

The SDK search tools use the shared source-provider and parsing operations to
locate sources. They do not record search result sets, including empty results,
as assessment dependencies. This tool policy is fixed for now; a hook to
customize it is deferred. Provider overrides change how an operation executes,
not this dependency policy. A subsequent tracked source-node read records that
node as an input. Locating a function does not establish that its full contents
were examined.

## Each language defines its outline

For the example at the start, `fn[check_token]` selects the function independently
of its line number. An empty selector selects the resource root. Nested items
use slash-separated segments; reserved characters in names are percent-encoded.
Anonymous items use identity-hash prefixes and duplicate named siblings use
occurrence suffixes.

Node contents are the text supplied to a procedure. A structural fingerprint
captures which changes to that text matter for freshness. Shared structural
hashing ignores ordinary whitespace; language definitions specify comment
handling and the syntax attached to an item. The graph only compares the
resulting fingerprint, without interpreting these language rules.

Each page starts with a source example and its resulting selector, then explains
the language's outline definition:

- [Rust](sources/rust.md)
- [Python](sources/python.md)
- [TypeScript and TSX](sources/typescript.md)
- [Markdown](sources/markdown.md)
- [CSS](sources/css.md)
- [TOML](sources/toml.md)
- [YAML](sources/yaml.md)
- [Plain-text fallback](sources/plain-text.md)

Existing detailed selector and structural-hash requirements remain in
[References and Rust items](../references.md). The
[allocation plan](requirements-map.md) identifies how to separate their shared
rules, language definitions, and Habeas policy. The next chapter
[puts the SDK pieces together in Habeas](habeas.md).

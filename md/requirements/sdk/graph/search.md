# Searching project sources

Use `search_text` when you know the spelling, such as a diagnostic message.
Use `search_structure` when you know the shape of the code, such as a call
expression. Both tools accept the same [source scope](#choosing-a-source-scope).
Rust callers receive all matches found within the query limit in one return
value. The following agent signatures describe the agreed search protocol.
Use `next` with `get_next_page` to retrieve more matches. Use `search_id`
with `finish_search` to account for the excerpts on the page already received.

## Choosing a source scope

Use `SourceScope::all()` to search the whole project, `SourceScope::under("src")`
to search a directory, or `SourceScope::under("src/lib.rs")` to search one file.
The same scope can be passed to `Sources::list`, `TextQuery`, and `StructuralQuery`.

:::{req} source_scope
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

`all()` and `Default::default()` select all visible project files. `under(path)`
selects an existing project-relative file or directory when the operation runs.
Paths match whole components, so `under("src")` excludes a sibling named
`src-old`. Both constructors respect the project's visibility rules under
{cite}`requirements.sdk.sources.local_provider`.

```rust,ignore
pub struct SourceScope { /* private fields */ }

impl SourceScope {
    pub fn all() -> Self;
    pub fn under(path: impl Into<String>) -> Self;
}
```

Trait implementations

```rust,ignore
impl Clone for SourceScope;
impl Debug for SourceScope;
impl Default for SourceScope;
```
:::

In an agent tool call, omit `prefix` to search the whole project or pass a path
such as `"prefix": "src"` to narrow the scope.

## Searching source text

For a Rust call, construct a [`TextQuery`](#habeas-req-requirements.sdk.graph.search.TextQuery) with the
scope, regex, number of context lines on each side, and an optional match limit.
For example, if `src/config.rs` has a match on line 12 and `context_lines` is 1,
the [`TextMatch`](#habeas-req-requirements.sdk.graph.search.TextMatch) reports `line: 12` and its `excerpt`
contains lines 11 through 13 when all three exist. [`TextMatches`](#habeas-req-requirements.sdk.graph.search.TextMatches)
also reports whether the search found all matching lines.

:::{req} search_text.fn
```rust,ignore
impl Sources {
    // ...

    /// Search available project files.
    pub async fn search_text(&self, query: &TextQuery) -> Result<TextMatches, SourceError>;

    // ...
}
```
:::

:::{req} TextQuery
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

`TextQuery::new(pattern)` searches all visible files, with zero context lines and
no match limit. `scope`, `context_lines`, and `limit` override those defaults.
`pattern` is a regular expression. `context_lines` requests that many lines on either side of each matching line. Zero returns just the matching line. `limit` bounds matches across the scope. `None` requests all matches.

```rust,ignore
#[non_exhaustive]
pub struct TextQuery {
    pub scope: SourceScope,
    pub pattern: String,
    pub context_lines: usize,
    pub limit: Option<usize>,
}

impl TextQuery {
    pub fn new(pattern: String) -> Self;
    pub fn scope(self, scope: SourceScope) -> Self;
    pub fn context_lines(self, context_lines: usize) -> Self;
    pub fn limit(self, limit: usize) -> Self;
}
```
:::

:::{req} TextMatch
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

`path` is project-relative. `line` is the one-based number of the matching line, even when the excerpt starts earlier. `excerpt` includes the requested neighboring lines that exist in the file.

```rust,ignore
#[non_exhaustive]
pub struct TextMatch {
    pub path: PathBuf,
    pub line: usize,
    pub excerpt: String,
}

impl TextMatch {
    pub fn new(path: PathBuf, line: usize, excerpt: String) -> Self;
}
```
:::

:::{req} SearchResults
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

`matches` holds the results found. `complete` is true when the search exhausted the scope. Reaching the exact limit is compatible with a complete result if no further match exists.

```rust,ignore
#[non_exhaustive]
pub struct SearchResults<T> {
    pub matches: Vec<T>,
    pub complete: bool,
}

impl<T> SearchResults<T> {
    pub fn new(matches: Vec<T>, complete: bool) -> Self;
}
```
:::

:::{req} TextMatches
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

`Sources::search_text` returns the matching lines and excerpts in `matches`. The
[`SearchResults`](#habeas-req-requirements.sdk.graph.search.SearchResults) field
`complete` says whether the result includes every match in scope.

```rust,ignore
pub type TextMatches = SearchResults<TextMatch>;
```
:::

:::{req} search_text.mcp
Give `search_text` a regular expression. Use `prefix` to narrow the search,
`context_lines` to include surrounding lines, and `limit` to cap matches.
Use the returned `search_id` and match tags when calling `finish_search`.
For the `instructions` field, see {cite}`requirements.sdk.graph.search.first_search.instructions`.

```text
search_text({pattern: string, prefix?: string, context_lines?: integer, limit?: integer})
  -> {search_id: UUID, instructions?: string, next?: UUID, complete: boolean, matches: {tag: string, path: string, line: integer, excerpt: string}[]}
```
:::

:::{req} search_text.mcp.description
> `Find regular-expression matches in available files. Read selected nodes
> separately.`
:::

:::{req} search_text.behavior
`TextMatches` is `SearchResults<TextMatch>`. Each match contains a project-relative
file path, a line number, and the excerpt. `complete` reports whether the search
returned all matches under {cite}`requirements.sdk.sources.provider_results.search_completeness`.

The tool returns the page ID, completeness flag, and tagged regular-expression
matches with their paths, line numbers, and excerpts. `context_lines` adds surrounding lines to
each excerpt and defaults to zero. `limit` bounds the number of matches.
For example, `search_text({"pattern":"invalid selector","prefix":"src"})`
finds occurrences of that phrase under `src/`.
:::

:::{req} search_text.inputs
The agent accounts for each delivered `search_text` result under
{cite}`requirements.sdk.graph.search.finish_search.inputs`. Evidence can depend
on selected excerpts. The set of matching identities is untracked.
:::

## Searching source structure

A [`StructuralQuery`](#habeas-req-requirements.sdk.graph.search.StructuralQuery) supplies a scope, language,
ast-grep rule object, and optional match limit. In a [`StructuralMatch`](#habeas-req-requirements.sdk.graph.search.StructuralMatch),
`excerpt` is the matched source text and `node` addresses the outline item enclosing
that match. [`StructuralMatches`](#habeas-req-requirements.sdk.graph.search.StructuralMatches) carries the same
completeness flag as text search.

:::{req} search_structure.fn
```rust,ignore
impl Sources {
    // ...

    /// Search source structure.
    pub async fn search_structure(
        &self,
        query: &StructuralQuery,
    ) -> Result<StructuralMatches, SourceError>;

    // ...
}
```
:::

:::{req} StructuralQuery
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

`StructuralQuery::new(language, rule)` searches all visible files with no match
limit. `scope` and `limit` narrow the search. `language` selects the grammar, and `rule` holds the ast-grep rule object. `limit` bounds the number of matches across files. `None` leaves the count unbounded.

```rust,ignore
#[non_exhaustive]
pub struct StructuralQuery {
    pub scope: SourceScope,
    pub language: LanguageId,
    pub rule: Map<String, Value>,
    pub limit: Option<usize>,
}

impl StructuralQuery {
    pub fn new(language: LanguageId, rule: Map<String, Value>) -> Self;
    pub fn scope(self, scope: SourceScope) -> Self;
    pub fn limit(self, limit: usize) -> Self;
}
```
:::

:::{req} StructuralMatch
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

`node` identifies the enclosing outline item. `excerpt` holds the text of the matched byte range, which may be smaller than the enclosing item.

```rust,ignore
#[non_exhaustive]
pub struct StructuralMatch {
    pub node: OutlineId,
    pub excerpt: String,
}

impl StructuralMatch {
    pub fn new(node: OutlineId, excerpt: String) -> Self;
}
```
:::

:::{req} StructuralMatches
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

`Sources::search_structure` returns the enclosing outline addresses and matched
excerpts in `matches`. The [`SearchResults`](#habeas-req-requirements.sdk.graph.search.SearchResults)
field `complete` has the same meaning as for a text search.

```rust,ignore
pub type StructuralMatches = SearchResults<StructuralMatch>;
```
:::

:::{req} search_structure.mcp
Give `search_structure` the language and an ast-grep rule. Use `prefix` to
narrow the search and `limit` to cap matches. Each match carries a source
address for `read_source` and a tag for `finish_search`.
For the `instructions` field, see {cite}`requirements.sdk.graph.search.first_search.instructions`.

```text
search_structure({language: string, rule: object, prefix?: string, limit?: integer})
  -> {search_id: UUID, instructions?: string, next?: UUID, complete: boolean, matches: {tag: string, node: {path: string, selector: string}, excerpt: string}[]}
```
:::

:::{req} search_structure.mcp.description
> `Find structural matches in available files. Read selected nodes separately.`
:::

:::{req} search_structure.behavior
`StructuralMatches` is `SearchResults<StructuralMatch>`. Each match contains
an `OutlineId` and the excerpt. The `complete` flag has the same meaning as in
text search.

The tool returns the page ID, completeness flag, and tagged excerpts with the
source address of each item matching the ast-grep rule. For example,
`{"language":"rust","rule":{"rule":{"pattern":"call()"}}}` searches for calls
to `call` with no arguments. Use a match's `node` as the arguments to
`read_source` to inspect the enclosing item.
:::

:::{req} search_structure.inputs
The agent accounts for each delivered `search_structure` result under
{cite}`requirements.sdk.graph.search.finish_search.inputs`, including an empty
result. A separate tracked read of a matched item records its own dependency.
:::

## Accounting for search excerpts in an agent session

The MCP-style search tools give each delivered result page a `search_id`. The
agent uses it to tell the harness which excerpts contributed to its answer.
Direct Rust calls to `Sources::search_text` and `Sources::search_structure`
return matches without opening a session or recording evidence inputs.

:::{req} first_search.instructions
The first search result delivered in each agent conversation MUST include an
`instructions` string, even when `matches` is empty. Text and structural
searches share this rule. Failed searches leave the instructions for the first
successful delivery. Later searches and continuation pages omit the field.
The instructions explain when to call `finish_search` and which tags to include
in `used`, as described in {cite}`requirements.sdk.graph.search.finish_search.mcp.description`.
:::

:::{req} first_search.instructions.content
The instructions supplied under {cite}`requirements.sdk.graph.search.first_search.instructions` tell the agent to call `finish_search` for every delivered
page before submitting its answer. They explain how to select the tags of
excerpts used as evidence and how to pass `used: []` when none contributed.
Finishing a page records the selected inputs without fetching more source.
:::

If the excerpt tagged `a` supplies a fact used in the answer, pass `used: ["a"]`.
If the agent opens the enclosing function and bases its answer on that tracked
read, it can close the search page with `used: []`.

:::{req} finish_search.mcp
Pass the delivered page’s `search_id` and the tags of excerpts used as
evidence. Pass `used: []` when none contributed. The acknowledgment identifies
the page that was closed.

```text
finish_search({search_id: UUID, used: string[]}) -> {search_id: UUID, finished: true}
```
:::

:::{req} finish_search.mcp.description
> `Close a delivered search and record dependencies for snippets you used to
> justify your conclusion without an additional source read. search_id is the
> result's UUID and used is an array of its short entry tags, e.g.
> {"search_id":"11111111-1111-4111-8111-111111111111","used":["1"]}. Entries
> whose source you also read may be omitted; including them is harmless. Use
> used:[] when no entries qualify, including an empty search. Every delivered
> search must be finished before submission. The result is {search_id,
> finished:true}; no additional source is delivered.`
:::

:::{req} finish_search.behavior
Each text or structural search page delivered to the agent opens a session-local
`search_id` for accounting, even when there are no matches. Each match has a
short string `tag` unique within the page. The agent may fetch further pages
while earlier pages are open. Replaying a page preserves its ID, tags, and
completion state. Failed searches and undelivered pages create no obligation.

Before the harness accepts a submission, the agent MUST call `finish_search`
for every open page. `used` lists the tags of excerpts the agent relied on
directly and may be empty. A valid call closes the page and returns its ID and
`finished: true`. A later call for the same page can select additional tags.
An invalid call, including an unknown ID or tag, returns a correctable tool
error and preserves the page's completion state.
:::

:::{req} finish_search.inputs
For each selected tag, the harness records the node identity and fingerprint
from the snapshot that produced the delivered excerpt. Selecting a tag again
preserves the original dependency. Earlier selections survive later calls.
An empty `used` list adds no dependency. Unselected excerpts and the match set
add none either. An invalid call leaves all recorded inputs intact.
:::

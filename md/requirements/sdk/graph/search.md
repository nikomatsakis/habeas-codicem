# Searching project sources

Use `search_text` when you know the spelling, such as a diagnostic message.
Use `search_structure` when you know the shape of the code, such as a call
expression. Both tools accept the same [source scope](#choosing-a-source-scope).
Rust callers receive all matches found within the query limit in one return
value. [Source search results](#source-search-results) describes the occurrences and shows how the agent receives grouped excerpts.
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

Agent searches operate on the configured project sources.

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
Give `search_text` a regular expression and an optional match limit. Set `window_bytes` to choose how much source to show on each side of a hit under {cite}`requirements.sdk.graph.search.windows`.
Use the returned `search_id` and match tags when calling `finish_search`.
For the `instructions` field, see {cite}`requirements.sdk.graph.search.first_search.instructions`.

```text
search_text({pattern: string, window_bytes?: integer, limit?: integer})
  -> {search_id: UUID, instructions?: string, next?: UUID, complete: boolean, matches: {tag: string, node: NodeId, excerpt: string}[]}
```
:::

:::{req} search_text.mcp.description
> `Find regular-expression matches in available files. Read selected nodes
> separately.`
:::

:::{req} search_text.behavior
The harness groups matching occurrences under {cite}`requirements.sdk.graph.search.group_hits` and renders each excerpt under {cite}`requirements.sdk.graph.search.windows`. Each entry's `node` is a reference under {cite}`requirements.sdk.graph.validation-context.node_reference`. `complete` reports whether the search exhausted its scope.
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
    pub node: OutlineData,
    pub excerpt: String,
}

impl StructuralMatch {
    pub fn new(node: OutlineData, excerpt: String) -> Self;
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
Give `search_structure` the language, an ast-grep rule, and an optional match limit. Each match carries a node reference for `read_node` and a tag for `finish_search`.
For the `instructions` field, see {cite}`requirements.sdk.graph.search.first_search.instructions`.

```text
search_structure({language: string, rule: object, limit?: integer})
  -> {search_id: UUID, instructions?: string, next?: UUID, complete: boolean, matches: {tag: string, node: NodeId, excerpt: string}[]}
```
:::

:::{req} search_structure.mcp.description
> `Find structural matches in available files. Read selected nodes separately.`
:::

:::{req} search_structure.behavior
For each match the agent receives an excerpt and a reference to the enclosing node under {cite}`requirements.sdk.graph.validation-context.node_reference`. The agent can pass the reference to `read_node` to inspect that item. The `complete` flag has the same meaning as in text search.
:::

:::{req} search_structure.inputs
The agent accounts for each delivered `search_structure` result under
{cite}`requirements.sdk.graph.search.finish_search.inputs`, including an empty
result. A separate tracked read of a matched item records its own dependency.
:::

## Source search results

:::{definition} hit
:phrase: source search hit

A *source search hit* is one occurrence of a query in a retained file snapshot.
:::

### Address a hit

:::{req} source_identity
A domain that exposes project source uses outlines to address it. Its validation cache holds the optional `Sources` configuration. With no project source configured, the harness offers no project source tools and makes no source-to-node lookups. A graph-only domain can omit the mapping.
:::

:::{req} enclosing_item
For each {term}`requirements.sdk.graph.search.hit` in a text or structural search, locate the start of the match in the outline of the **same snapshot**. Select the innermost item containing that position. The item's project-relative file path and selector form its `OutlineData`. When no smaller item contains the position, use the file root (empty selector).
:::

:::{req} domain_node
The Rust search operation passes the enclosing `OutlineData` to `Domain::source_node`, then interns the returned node data to obtain `NodeId<D>`. A mapping failure is a search error.
:::

### What Rust receives

:::{req} occurrence
Return one result per occurrence, including multiple matches on the same line or in the same item. Each result contains the enclosing `NodeId<D>`, the exact matched text, and its file-relative `Span` in the retained UTF-8 source (zero-based byte offsets, end exclusive). `SourceProject::search_text` and `SourceProject::search_structure` return these hits. Rust callers can use the node ID to load the item's contents and choose their own presentation.

```rust,ignore
#[non_exhaustive]
pub struct SearchHit<N> {
    pub node: N,
    pub span: Span,
    pub text: String,
    /* private fields */
}

impl<N> SearchHit<N> {
    pub fn new(
        node: N,
        span: Span,
        item: OutlineContent,
    ) -> Result<Self, SourceError>;
}
```

Trait implementations

```rust,ignore
impl<N: Debug> Debug for SearchHit<N>;
```
:::

:::{example} same_line
{cite}`requirements.sdk.graph.search.occurrence`

A search for `hit` finds three occurrences on one line.

```rust
const A: &str = "éhit"; const B: &str = "hit hit";
```

Suppose the cache assigned handles `7` and `8` to `A` and `B`. The Rust caller receives these three occurrences, in source order. The `é` occupies two bytes.

| Node ID | File byte span | Matched text |
| --- | --- | --- |
| `7` | `19..22` | `hit` |
| `8` | `42..45` | `hit` |
| `8` | `46..49` | `hit` |
:::

### What the agent receives

:::{req} agent_search
The agent's `search_text` and `search_structure` tools call the corresponding public Rust search operation. For the same query they format the occurrences returned by that call, using its node IDs, match spans, and completeness flag. They build excerpts from the retained snapshot in which the Rust operation found the hits.
:::

:::{req} group_hits
Deliver one entry per enclosing item in a text search, even when the item has several hits. Group before pagination so an item's excerpt stays together. Each entry carries a node reference under {cite}`requirements.sdk.graph.validation-context.node_reference`, one excerpt, and the page-local tag used by `finish_search`. The tag accounts for use of the excerpt. The node reference lets the agent read the full item.
:::

:::{req} windows
The agent's `search_text` tool accepts a nonnegative `window_bytes`, defaulting to 128. For a hit with byte span `[start, end)`, retain the hit and up to `window_bytes` of source immediately before `start` and after `end`. Clip the window to the enclosing item's span. Move a context boundary that splits a UTF-8 character inward to the nearest character boundary. With `window_bytes: 0`, only the hit spans and the available header and trailer contribute source to the excerpt.

Compute windows for all hits in the item from the same snapshot. Where windows touch or overlap, emit the contiguous source once. This preserves each hit and the text between nearby hits, including whitespace and newlines.
:::

:::{example} grouped_constants
{cite}`requirements.sdk.graph.search.group_hits`
{cite}`requirements.sdk.graph.search.windows`

For the same `hit` query, the agent receives one excerpt per declaration. After the first-search instructions have been delivered, the complete page looks like this.

```json
{
  "search_id": "11111111-1111-4111-8111-111111111111",
  "complete": true,
  "matches": [
    {"tag": "1", "node": 7, "excerpt": "const A: &str = \"éhit\";"},
    {"tag": "2", "node": 8, "excerpt": "const B: &str = \"hit hit\";"}
  ]
}
```
:::

:::{req} agent_search_accounting
When it delivers a formatted page, the agent harness opens a [`finish_search`](#accounting-for-search-excerpts-in-an-agent-session) session for that page and associates each entry's tag with its node ID and the fingerprint from the snapshot used for the excerpt. Finishing the page records the selected dependencies using those saved observations. Direct Rust search calls return occurrences without opening a session or recording excerpt dependencies.
:::

:::{example} selecting_an_excerpt
{cite}`requirements.sdk.graph.search.agent_search_accounting`

If the agent relies on the excerpt for `B`, it calls `finish_search({"search_id":"11111111-1111-4111-8111-111111111111","used":["2"]})`. The harness records one dependency on `8` even though the Rust search found two hits there. If the agent instead calls `read_node({"node":8})` and relies only on that full read, the read records its own dependency and the agent finishes the search page with `used: []`.
:::

### Show the declaration around separated hits

:::{req} declaration_spans
An `OutlineItem` can supply a `header_span: Option<Span>` and a `trailer_span: Option<Span>` independently. Both use file-relative byte offsets within the item's span. Each language defines them for items with recognizable declaration syntax. In Rust, a function or braced `impl` header extends from the start of its declaration through the opening brace, and its trailer spans the closing brace. A `const` or `static` declaration can have a header through the `=` token. Either span can end within a line.
:::

:::{req} omissions
Render the union of the header, hit windows, and trailer in source order. Copy retained bytes verbatim and replace each nonempty omitted region of the enclosing item with the literal `...`, including a gap at the beginning or end. Insert the marker directly at the cut. When the retained spans cover the item, the excerpt is its original text.
:::

:::{example} separated_hits
{cite}`requirements.sdk.graph.search.declaration_spans`
{cite}`requirements.sdk.graph.search.omissions`
{cite}`requirements.sdk.graph.search.windows`

The agent calls `search_text({"pattern":"hit","window_bytes":4})` on this source.

```rust
# struct Runner;
# fn setup() {}
# fn hit() {}
# fn cleanup() {}
# fn finish() {}
impl Runner {
    fn run() {
        setup();
        hit(); hit();
        cleanup();
        hit();
        finish();
    }
}
```

The first two windows overlap. Together they include the space between the calls and the newline after the second semicolon. The third window is separated by omitted source. The agent receives one entry for `run`.

```json
{"tag":"1","node":9,"excerpt":"fn run() {...    hit(); hit();\n...    hit();\n...}"}
```

`read_node({"node":9})` reads the full function, including `cleanup();`.
:::

:::{example} utf8_window
{cite}`requirements.sdk.graph.search.windows`
{cite}`requirements.sdk.graph.search.omissions`

A file-root item contains `éhit猫` and has no header or trailer. With `window_bytes: 2`, the agent receives `{"tag":"1","node":10,"excerpt":"éhit..."}`. The two bytes before `hit` contain the entire `é`. The two bytes after it cover only part of `猫`, so the excerpt ends with the omission marker. With the default window, the excerpt is `éhit猫`.
:::

:::{example} file_root
{cite}`requirements.sdk.graph.search.enclosing_item`

A file contains only `needle` and its outline consists of the file root. Searching for `needle` returns `{"tag":"1","node":10,"excerpt":"needle"}`. Reading `10` reads the file-root node.
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

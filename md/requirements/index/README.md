# Book rendering

The book trusts the annotations and connects them into browsable documentation.
It presents recorded reviews alongside current source. Use `habeas status`
and `habeas author` to check those annotations and reviews.

:::{req} render_only
`habeas mdbook` reads annotations and stored reviews to render their connections.
It does not audit repository evidence, check review freshness or dependency
hashes, enforce reference coverage, or assess citation and site reviews.
It does not load saved execution measurements, run tests, collect coverage,
or invoke an assessment model. Source-input and stored-record read or parse
failures produce readable rendering diagnostics rather than a repository
validation verdict.
:::

:::{req} chapter
An mdBook chapter MAY contain one standalone `{requirements-index}` directive with an empty body, opened by at least three colons and closed by at least as many. A book MUST contain no more than one such directive. `habeas mdbook` MUST render a requirements index listing all requirements defined in rendered chapters, each linking to its evidence detail page under {cite}`requirements.index.detail_pages`. When a directive is present, it MUST replace the directive with the index table; otherwise, it MUST generate the chapter specified by {cite}`requirements.index.chapter.default`.
:::

:::{req} chapter.default
When a book contains no standalone `{requirements-index}` directive, `habeas mdbook` MUST append a new top-level chapter titled "Requirements index" at `requirements-index.md`, containing the index table, even when the book defines no requirements or examples. The generated chapter MUST have no source file or edit target. Its section number MUST be one greater than the largest numbered top-level chapter, or 1 when no top-level chapter is numbered.
:::

:::{req} detail_pages
`habeas mdbook` MUST generate one evidence detail
page for every requirement and named example defined in a rendered chapter.
Each page MUST be a subchapter of the index chapter, in definition order,
titled by the definition's full name, at a path derived from the index chapter's
path.
Books configured by {cite}`requirements.cli.setup` fold these subchapters in the
sidebar until a reader expands the index chapter.
:::

:::{req} state
The requirements index and its detail pages describe the last recorded
assessment as Recorded true, Recorded false, or Recorded FIXME. A missing
record is shown as Missing; an unreadable record or determination is shown as
Unreadable with a readable explanation. Recorded FIXME includes its recorded
acknowledgments.

These labels describe stored results, without claiming that the record is
current or that repository validation passes. `habeas status` and
`habeas author` check the annotations and evidence.
:::

:::{req} detail_format
Each evidence detail page displays its recorded state as a Shields.io static
badge with meaningful alt text and the current requirement or example body
as a Markdown block quote labeled by kind. It renders the recorded rationale
as Markdown in a Rationale section and the recorded acknowledgments for FIXME.

Inputs appear once, in the first applicable group: Implementation, Tests,
Referenced definitions, then Other sources. Implementation uses `{impl}`
connections and Tests uses `{test}` connections. Referenced definitions
contains definition inputs and requirement selections wholly covered by the
subject's citations; remaining inputs go to Other sources. Current annotations,
including anchored files, determine these connections
under {cite}`requirements.references.roles` and
{cite}`requirements.references.anchors`. Groups are expanded disclosures;
individual inputs are collapsed disclosures identifying their selectors.

An outline input shows its file, selector, kind, current inclusive line range,
and current source text in a fenced code block. A requirements input shows
its selection and each matched definition's full name, file, and inclusive
line range. Requirements and definition inputs show their recorded text.
The book presents current source alongside the recorded review without
comparing input hashes.

Code fences use Rust highlighting for `.rs` and language-aware highlighting
for Markdown, JSON, TOML, Java, JavaScript, TypeScript, and Python; TOML may use
INI highlighting when native TOML highlighting is unavailable.
:::

:::{req} declaration_sections
:impl: md/habeas.css
In authored chapters, requirements, examples, and named definitions render as
lightly framed, collapsible sections, expanded when the chapter opens. Each
header identifies the declaration and its kind. A disclosure chevron indicates
whether its body is expanded or collapsed; a subtle divider separates the
header from the expanded body. Collapsing a section hides its body while
keeping the header visible. The disclosure works with a mouse or keyboard.

The body retains its Markdown rendering, including code blocks and nested
declarations. Requirement and example headers retain the definition and
evidence links described by {cite}`requirements.index.navigation`; those
links navigate independently of the disclosure.
:::

:::{req} navigation
:impl: md/habeas.css
Each requirement and example definition badge MUST link to its own anchored definition on the current page. The badge MUST have a separate adjacent link containing an accessible right-arrow icon in an integrated segment matching the badge color linking to its evidence detail page. The badge and arrow MUST remain aligned in one non-wrapping row. Definitions MUST NOT be followed by a separate plaintext evidence link. Each detail page MUST link back to the definition's anchored badge and to the index. Reference badges MUST continue to link to the definition. These links MUST resolve when a requirement name contains `.md`, as in `requirements.cli.mdbook`.
:::

:::{req} source_links
When `output.html.git-repository-url` names a GitHub repository and `output.html.edit-url-template` uses that repository's `/edit/<branch>/{path}` form, each safely resolved source location displayed for an evidence input MUST link its repository-relative file and inclusive line range to `/blob/<branch>/<file>#L<start>-L<end>`. A source location MUST remain plain text when a safe URL cannot be derived. A generated detail page MUST use its evidence JSON file as the mdBook edit target when the file exists, or its requirement chapter when the evidence file is missing.
:::

:::{req} path_conflicts
The mdBook preprocessor MUST report an error when a generated requirements index or evidence detail page would use the path of an existing book chapter.
:::

:::{req} invalid_detail
An evidence detail page continues to show the current requirement or example
when its stored record is missing or unreadable. It shows readable placeholders
for missing rationale or inputs and the available raw contents of malformed
JSON. A source input that cannot be read or resolved shows a readable
diagnostic while the rest of the page remains browsable.
:::

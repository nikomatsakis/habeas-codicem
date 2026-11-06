# Book rendering

:::{req} chapter
An mdBook chapter MAY contain one standalone `{requirements-index}` directive with an empty body, opened by at least three colons and closed by at least as many. A book MUST contain no more than one such directive. When present, `habeas mdbook` MUST replace it with a table of all requirements defined in rendered chapters, each linking to its evidence detail page under {cite}`requirements.index.detail_pages`.
:::

:::{req} detail_pages
`habeas mdbook` MUST generate one evidence detail page for every requirement and
named example defined in a rendered chapter, whether or not the book contains a
requirements index. Each page MUST be a subchapter nested under the chapter that
defines it, in definition order, titled by the definition's full name, at a path
derived from that chapter's path. Books configured by {cite}`requirements.cli.setup`
fold these subchapters in the sidebar until a reader expands their chapter.
:::

:::{req} state
The requirements index MUST show each requirement's evidence state as Valid, Missing, Determination false, FIXME, or Invalid. Valid MUST mean its determination is true and its record passes the same evidence and reference coverage checks as `habeas::validate()`. A false or FIXME determination MUST have its distinct state when the record passes all other checks. The FIXME state MUST pass validation while retaining its distinct label. Malformed or stale evidence MUST remain Invalid regardless of its determination. Each detail page MUST show any validation findings.
:::

:::{req} detail_format
Each evidence detail page MUST display its state as a Shields.io static badge image with meaningful alt text and the current requirement or example body as a Markdown block quote labeled by kind. It MUST render the recorded rationale as Markdown in a Rationale section and display the recorded FIXME strings when the determination is FIXME. Inputs MUST be grouped under headings in this order: Implementation, for sources reached by an `{impl}` edge from the definition, including anchored files; Tests, for sources reached by a `{test}` edge, including tests, examples, and anchored files; Referenced definitions, for declarations the definition cites and their terminology; and Other sources, for the remaining inputs the assessment consulted. Edges MUST be determined from the current references under {cite}`requirements.references.targets` and {cite}`requirements.references.anchors`. Each group and each input MUST be a collapsible disclosure element, with groups expanded and inputs collapsed by default, so readers can move between groups and open individual sources. Each input MUST have its own subsection identifying its selector. A file-range or Rust-item input MUST show its resolved file and inclusive line range. A requirements input MUST show its selection and each matched definition's full name, file, and inclusive line range. Each subsection MUST show the exact recorded text in a fenced code block. Code fences MUST request Rust highlighting for `.rs` files and language-aware highlighting for Markdown, JSON, TOML, Java, JavaScript, TypeScript, and Python files; TOML MAY use INI highlighting when native TOML highlighting is unavailable.
:::

:::{req} navigation
Each requirement and example definition badge MUST link to its evidence detail page, and each rendered requirement and example MUST be followed by a "See evidence" link to that page. Each detail page MUST link back to the definition's anchored badge and, when an index exists, to the index. Reference badges MUST continue to link to the definition. These links MUST resolve when a requirement name contains `.md`, as in `requirements.cli.mdbook`.
:::

:::{req} source_links
When `output.html.git-repository-url` names a GitHub repository and `output.html.edit-url-template` uses that repository's `/edit/<branch>/{path}` form, each safely resolved source location displayed for an evidence input MUST link its repository-relative file and inclusive line range to `/blob/<branch>/<file>#L<start>-L<end>`. A source location MUST remain plain text when a safe URL cannot be derived. A generated detail page MUST use its evidence JSON file as the mdBook edit target when the file exists, or its requirement chapter when the evidence file is missing.
:::

:::{req} path_conflicts
The mdBook preprocessor MUST report an error when a generated evidence detail page would use the path of an existing book chapter.
:::

:::{req} invalid_detail
An evidence detail page MUST continue to show the current requirement or example and validation findings when evidence is missing or invalid. It MUST show readable placeholders for missing rationale or inputs and expose the contents of an unreadable JSON record.
:::

# Book rendering

:::{req} chapter
An mdBook chapter MAY contain one standalone `{requirements-index}` directive with an empty body, opened by at least three colons and closed by at least as many. A book MUST contain no more than one such directive. When present, `habeas mdbook` MUST replace it with a table of all requirements defined in rendered chapters and MUST add one evidence detail subchapter per requirement.
:::

:::{req} state
The requirements index MUST show each requirement's evidence state as Valid, Missing, Determination false, FIXME, or Invalid. Valid MUST mean its determination is true and its record passes the same evidence and reference coverage checks as `habeas::validate()`. A false or FIXME determination MUST have its distinct state when the record passes all other checks. The FIXME state MUST pass validation while retaining its distinct label. Malformed or stale evidence MUST remain Invalid regardless of its determination. Each detail page MUST show any validation findings.
:::

:::{req} detail_format
Each evidence detail page MUST display its state as a Shields.io static badge image with meaningful alt text and the current requirement or example body as a Markdown block quote labeled by kind. It MUST render the recorded rationale as Markdown in a Rationale section and display the recorded FIXME strings when the determination is FIXME. Each input MUST have its own subsection identifying its selector. A file-range or Rust-item input MUST show its resolved file and inclusive line range. A requirements input MUST show its glob and each matched definition's full name, file, and inclusive line range. Each subsection MUST show the exact recorded text in a fenced code block. Code fences MUST request Rust highlighting for `.rs` files and language-aware highlighting for Markdown, JSON, TOML, Java, JavaScript, TypeScript, and Python files; TOML MAY use INI highlighting when native TOML highlighting is unavailable.
:::

:::{req} navigation
When an index exists, each requirement definition badge MUST link to its evidence detail page. Each detail page MUST link back to the definition's anchored badge and to the index. Reference badges MUST continue to link to the definition. These links MUST resolve when a requirement name contains `.md`, as in `requirements.cli.mdbook`.
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

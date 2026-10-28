# Requirements

Capitalized requirement terms in this chapter use the meanings defined by BCP 14 ([RFC 2119](https://www.rfc-editor.org/rfc/rfc2119) and [RFC 8174](https://www.rfc-editor.org/rfc/rfc8174)).

:::{req} integration.validate
`habeas::validate()` MUST check the current Cargo and mdBook project and make its Cargo test fail with all validation errors when any requirement evidence is invalid.
:::

:::{req} ci.coverage
Every requirement defined by a fenced `{req}` directive in a chapter under the configured mdBook source directory MUST have an evidence file named `evidence/<full-requirement-name>.json`. The full name MUST use the chapter's path-derived namespace followed by the directive name; for example, a directive named `check` in `md/requirements.md` has the full name `requirements.check`. Habeas MUST apply this rule to its own requirements in this chapter.
:::

:::{req} ci.orphans
Every JSON file directly under `evidence/` MUST correspond to a declared requirement or named example. A JSON file at `evidence/rust/<rust_file>.json` MUST correspond to a first-party Rust source file containing an explicitly declared function or method. Habeas MUST report evidence JSON files in other locations under `evidence/` and Rust evidence files or entries whose source item no longer exists.
:::

:::{req} ci.schema
A requirement or example evidence file at `evidence/<full-name>.json` MUST be a JSON object with exactly `requirement` or `example`, `determination`, `rationale`, and `inputs`. The definition text and rationale MUST be nonempty strings, `determination` MUST be a boolean, and `inputs` MUST be an array. Every evidence input, including one in a Rust function attestation, MUST have exactly one of three forms: a file-range input with `file`, `line_start`, `line_end`, and `text`; a Rust-item input with `rust_file`, `rust_item`, and `text`; or a requirements input with `requirements`, `matches`, and `text`. File paths MUST be repository-relative, line numbers MUST be positive integers, `matches` MUST be an array of distinct full definition names, and `text` MUST be a nonempty array of strings.
:::

:::{req} ci.requirement_text
The `requirement` string in each evidence file MUST match all Markdown inside its fenced `{req}` directive, excluding leading and trailing whitespace. Habeas MUST report changed requirement text.
:::

:::{req} ci.determination
Every requirement or example evidence record and every Rust function attestation MUST have `determination: true` for `habeas::validate()` to succeed. When a determination is false, Habeas MUST report it together with its rationale so the remaining review is clear.
:::

:::{req} ci.inputs
For a file-range input, Habeas MUST verify that `line_start` and `line_end` identify an inclusive, ordered range in `file` and that `text` exactly matches the current raw lines in that range. Habeas MUST report missing files, invalid ranges, and changed lines.
:::

:::{req} ci.rust_item_inputs
For a Rust-item input, Habeas MUST parse `rust_file` with `syn` and resolve `rust_item` to exactly one item. It MUST determine an inclusive range of complete raw source lines containing that item, including attached doc comments, attributes, and leading citation comments. It MUST compare `text` with those lines without serializing the `syn` tree, and derive the range again during validation for display and reference coverage. Moving unchanged lines because of edits earlier in the file MUST NOT invalidate the input. Habeas MUST report a missing file, parse failure, invalid or ambiguous selector, or changed lines.
:::

:::{req} ci.requirements_inputs
For a requirements input, Habeas MUST match the glob in `requirements` against all defined requirements and named examples. It MUST recompute the matches during validation, sort their full names, and compare them with `matches`. It MUST take each match's complete raw fenced declaration, including its opening and closing lines, concatenate those declarations in full-name order with one blank line between them, and compare the resulting raw lines with `text`. It MUST retain every match's current source file and inclusive line range for display and reference coverage. A glob matching no definitions MUST be invalid. Adding, removing, renaming, or changing a matched definition MUST invalidate the stored input; moving an unchanged declaration within the book MUST NOT. Errors MUST identify the glob and affected definitions when available.
:::

:::{req} references.targets
Project text files MAY use the `{impl}`, `{cite}`, and `{test}` roles with a backtick-delimited full requirement or example name. Habeas MUST find these references throughout project source files and MUST fail with the source file and line if a name has no matching `{req}` or `{example}` definition. In Markdown, escaped roles and roles in code examples MUST remain literal. Evidence snapshots and generated build output MUST NOT be counted as additional source references.

```text
{impl}`requirements.references.targets`
{cite}`requirements.references.targets`
{test}`requirements.references.targets`
```
:::

:::{req} references.book_badges
In a book chapter, each valid `{impl}`, `{cite}`, or `{test}` reference MUST render as a Shields.io static badge image with meaningful alt text that displays its role and full name and links to the badge at the corresponding `{req}` or `{example}` definition. Links MUST work across chapters, including nested chapters. Each fenced `{req}` or `{example}` directive MUST render as a Shields.io definition badge image identifying its kind; text outside a directive MUST remain literal. Badge text MUST remain legible regardless of whether its link has been visited. These badges use the syntax defined by {cite}`requirements.references.targets`.
:::

:::{req} index.chapter
An mdBook chapter MAY contain one standalone `{requirements-index}` directive with an empty body, opened by at least three colons and closed by at least as many. A book MUST contain no more than one such directive. When present, `habeas mdbook` MUST replace it with a table of all requirements defined in rendered chapters and MUST add one evidence detail subchapter per requirement.
:::

:::{req} index.state
The requirements index MUST show each requirement's evidence state as Valid, Missing, Determination false, or Invalid. Valid MUST mean its record passes the same evidence and reference coverage checks as `habeas::validate()`; a false determination MUST be distinct if otherwise valid. Each detail page MUST show any validation findings.
:::

:::{req} index.detail_format
Each evidence detail page MUST display its state as a Shields.io static badge image with meaningful alt text and the current requirement or example body as a Markdown block quote labeled by kind. It MUST render the recorded rationale as Markdown in a Rationale section. Each input MUST have its own subsection identifying its selector. A file-range or Rust-item input MUST show its resolved file and inclusive line range. A requirements input MUST show its glob and each matched definition's full name, file, and inclusive line range. Each subsection MUST show the exact recorded text in a fenced code block. Code fences MUST request Rust highlighting for `.rs` files and language-aware highlighting for Markdown, JSON, TOML, Java, JavaScript, TypeScript, and Python files; TOML MAY use INI highlighting when native TOML highlighting is unavailable.
:::

:::{req} index.navigation
When an index exists, each requirement definition badge MUST link to its evidence detail page. Each detail page MUST link back to the definition's anchored badge and to the index. Reference badges MUST continue to link to the definition. These links MUST resolve when a requirement name contains `.md`, as in `requirements.cli.mdbook`.
:::

:::{req} index.source_links
When `output.html.git-repository-url` names a GitHub repository and `output.html.edit-url-template` uses that repository's `/edit/<branch>/{path}` form, each safely resolved source location displayed for an evidence input MUST link its repository-relative file and inclusive line range to `/blob/<branch>/<file>#L<start>-L<end>`. A source location MUST remain plain text when a safe URL cannot be derived. A generated detail page MUST use its evidence JSON file as the mdBook edit target when the file exists, or its requirement chapter when the evidence file is missing.
:::

:::{req} references.evidence_coverage
For every `{impl}`, `{cite}`, and `{test}` reference in a project source file, the evidence file for the referenced requirement or example MUST contain an input whose resolved source ranges cover the reference's file and line. Habeas MUST report every uncovered reference with its role, full name, source file, and line number.
:::

:::{req} references.rust_function_coverage
During repository validation, Habeas MUST parse every first-party Rust source file in the project. Every explicitly declared function or method, including trait methods and test helpers, MUST have a valid `{impl}`, `{cite}`, or `{test}` reference to a defined requirement or named example in a Rust comment attached to its declaration or within its body. A named example MUST cite a requirement. A reference in a string literal, on an enclosing module or impl block, or only in an evidence input MUST NOT count. Habeas MUST report every uncited function with its file, line, and name, and MUST report a Rust parse failure instead of silently skipping the file.
:::

:::{req} references.definition_globs
A definition glob MUST match a complete full name. `*` MUST match zero or more characters, including dots; all other valid name characters MUST match literally. An empty glob MUST be invalid. The same glob matching rule MUST be used by requirements evidence inputs and CLI definition filters. All three MUST match against both requirements and named examples.
:::

:::{req} references.rust_item_selectors
Habeas MUST identify a Rust item by the pair of its repository-relative `rust_file` and a `rust_item` selector derived from a `syn` parse of that file. The selector MUST describe syntactic ancestry within the file and MUST NOT resolve Rust modules across files or use line numbers as identity. Named items MUST use kind-and-name segments such as `fn[run]` and `struct[Packet]`. Impl segments MUST distinguish inherent from trait impls using their syntactic self type and optional trait path. Otherwise identical selectors MUST have deterministic source-order disambiguators. A `use` declaration MUST have a positional selector `use#N`, where N is its one-based position among sibling `use` declarations; its import target MUST NOT form part of the selector. Other items without a suitable name MUST have a kind and positional disambiguator. Selector spelling MUST be documented and reversible when names or types contain delimiter characters.
:::

:::{req} references.rust_function_attestations
For every function or method covered by `references.rust_function_coverage`, repository validation MUST require exactly one attestation in `evidence/rust/<rust_file>.json`. Each such file MUST be an object with exactly `rust_file` and `functions`, where `rust_file` matches its repository-relative source path and `functions` is an array. Each attestation MUST contain exactly `rust_item`, `citations`, `determination`, `rationale`, and `inputs`. Its `citations` MUST be the nonempty set of valid references owned by the function, each recorded by role and full requirement or example name. Its `inputs` MUST include one Rust-item input selecting the function and at least one requirements input; together, the requirements inputs MUST match every name in `citations`. Habeas MUST validate every input and fail for a missing, duplicate, orphaned, stale, or false attestation, a changed citation set, or an empty rationale. Each finding MUST identify the source file, current line when available, and item selector.
:::

:::{req} ci.errors
Habeas MUST continue validating after an error and report all validation errors it finds in one run. Each error MUST identify the affected evidence file or requirement and explain the failure.
:::

:::{req} ci.diagnostics
Every validation error MUST identify the failed check and give an agent enough detail to diagnose it. If the stored requirement text differs from the current chapter text, the error MUST name the requirement and show a compact line-oriented diff, with `-` for recorded lines and `+` for current lines. If recorded input text differs from the current file, the error MUST name the file and line range and show the same kind of diff. Other errors MUST state the specific missing or invalid value and the expected value when available. Diagnostics SHOULD be concise because agents are the primary readers.

```text
evidence/requirements.ci.inputs.json: requirement text changed (md/requirements.md:43)
- Habeas SHOULD check input lines.
+ Habeas MUST check input lines.
```
:::

:::{req} integration.self_test
The Habeas crate MUST have a Cargo integration test that invokes `habeas::validate()` against its own project, so its own requirements and evidence are checked recursively by `cargo test`.
:::

:::{req} cli.clap
The `habeas` CLI MUST use `clap` to define and parse its commands and options.
:::

:::{req} cli.mdbook
The `mdbook` subcommand MUST act as the mdBook preprocessor for HTML books, rendering requirement definitions and linked reference badges. This project's `book.toml` MUST configure `cargo habeas mdbook` as the `habeas` preprocessor, using a repository-local Cargo alias for `cargo run --release -p habeas --`. Consumer books MUST continue to invoke the installed `habeas mdbook` CLI directly.
:::

:::{req} cli.setup
Running `habeas setup` in an existing Cargo and mdBook project MUST add Habeas as a dev dependency, configure `habeas mdbook` as its preprocessor, add the badge stylesheet, and create `tests/habeas.rs` that calls `habeas::validate()`. Repeating setup MUST avoid duplicate configuration and MUST NOT overwrite a differing test or stylesheet. Setup MUST NOT install or modify agent skills.
:::

:::{req} cli.status
`habeas status [<glob>]` MUST consider requirements and named examples whose complete full names match the glob defined by `references.definition_globs`; without a glob it MUST consider every definition. It MUST list matching definitions in full-name order when their evidence is missing, has a false determination, or fails the same evidence and reference checks as `habeas::validate()`. `habeas status [<glob>] --all` MUST list every matching definition with its kind and evidence state. Both forms MUST show validation findings, including repository-wide Rust attestation findings, work from within a Cargo and mdBook project, and report invalid arguments or a supplied glob matching no definition clearly.
:::

:::{req} cli.status_table_alignment
When `habeas status` or `habeas status --all` displays a Markdown table, it MUST left-align and space-pad the Definition and Evidence columns so every row, including the header and separator row, has its `|` column delimiters in the same positions. Column widths MUST be determined by the widest displayed cell in each column.
:::

:::{req} cli.shared_status_rendering
`habeas status` and the mdBook requirements index MUST generate their Definition/Evidence Markdown tables through the same renderer. That renderer MUST calculate column widths, align the columns, and display the same evidence-state labels in both outputs. Callers MAY select different definitions and supply different kind-labeled name cells so mdBook can link to detail pages.
:::

:::{req} cli.info
`habeas info <glob>` MUST match complete requirement and named example names using `references.definition_globs`; a full name without `*` is an exact selection. It MUST print matches in full-name order as Markdown with the same kind-labeled definition, Validation findings, Rationale, and Inputs structure and evidence states as mdBook detail pages, including readable output for missing or invalid evidence. It MUST work without a requirements index directive and report an invalid glob or no matches clearly. Habeas's bundled agent guidance MUST tell agents they can use this command to inspect a requirement or named example.
:::

:::{req} cli.add_input
`habeas add-input` MUST accept exactly one evidence target: `--for-requirement <full-name>`, `--for-example <full-name>`, or `--for-rust-item <rust_file> <rust_item>` identifying an explicitly declared function or method. It MUST accept exactly one input mode: `--file-range`, `--rust-item`, or `--requirements`. It MUST use the same input schema and resolution rules as validation. Repeating a selector for a target MUST refresh its existing input rather than add a duplicate; an unchanged input MUST leave the record unchanged. The command MUST report every input it adds or refreshes. It MUST create a missing target record with a pending determination. Whenever it changes a record, it MUST leave `determination` false with a nonempty rationale indicating that review is pending.
:::

:::{req} cli.add_input_file_range
`habeas add-input --file-range <file>:<start>-<end>` MUST add or refresh one file-range input containing the current raw lines in the inclusive range. It MUST reject a missing file or invalid range.
:::

:::{req} cli.add_input_rust_item
`habeas add-input --rust-item <rust-file>:<line>` MUST parse the file with `syn` and add or refresh one Rust-item input for every explicit item declared on that line. An item with a declared name is declared on the line containing that name; an item without one is declared on the line containing its distinctive keyword or introducer, such as `use` or `impl`. Each input MUST store the repository-relative `rust_file`, the item's syntactic `rust_item` selector, and its complete raw source lines. The command MUST report every selected item and reject a line on which no item is declared.
:::

:::{req} cli.add_input_requirements
`habeas add-input --requirements <glob>` MUST add or refresh one requirements input containing the matched full names and their concatenated raw declaration sources. It MUST report the matched names in expansion order and reject a glob that matches no requirement or named example.
:::

:::{req} cli.function_attestation_guidance
Habeas's bundled agent skills and any output that requests or reports a function attestation MUST explain its purpose: after reviewing the function and applicable requirement and example texts, the agent is confirming that the function carries citations for every applicable requirement or named example. The agent MUST explain how it identified the relevant definitions, why its requirements inputs cover them, and how the cited texts account for the function's behavior, including interactions between requirements. Matching input snapshots and a matching citation list alone MUST NOT justify a true determination. A changed function or reviewed definition set MUST be reassessed before its determination is true.
:::

:::{req} cli.bootstrap_skill
Habeas MUST bundle a skill for deriving initial requirements from an existing codebase. The skill MUST direct agents to read the code and tests, map every first-party test to at least one requirement or example, identify and map every source unit implementing end-user-visible functionality or interactions to a requirement, and report gaps rather than claim complete coverage without an independent inventory review.
:::

:::{req} cli.requirements_review_skill
The bundled requirements skill MUST direct agents adding or changing user-visible behavior in a Habeas project to draft stable requirement IDs and exact text, obtain the user's agreement, and install the agreed requirements in the mdBook before writing implementation or test code. It MUST direct agents to review substantive changes to agreed obligations with the user. The bootstrap skill MUST require user review of requirements inferred from existing code before installing them. The evidence skill MUST allow truthful false determinations while implementation is pending.
:::

:::{req} cli.behavior_review_guidance
Habeas's bundled agent skills MUST direct agents reviewing code against requirements to inspect every end-user-observable result and branch, including errors and input handling, and verify both that the code fulfills each applicable requirement and that it introduces no additional end-user-observable behavior without an agreed requirement. Where requirements interact, agents MUST cite every applicable requirement at the relevant implementation point and explain how their texts jointly account for the behavior. A citation alone MUST NOT count as that explanation.
:::

:::{req} cli.internal_detail_guidance
Habeas's bundled agent skills MUST NOT offer a waiver for an uncited first-party Rust function. When a function serves only as an internal implementation detail and no existing requirement explains it, Habeas's bundled agent skills MUST direct agents to propose an explicit project requirement, such as `internal-detail`, obtain the user's agreement, cite it on the function, and record evidence explaining why each such function introduces no additional end-user-observable behavior. An internal-detail requirement MUST NOT justify an otherwise unspecified end-user-observable behavior.
:::

:::{req} install.symposium
The getting-started documentation MUST tell users with a Cargo and mdBook project to install and initialize Symposium, run `cargo add --dev habeas`, and run `cargo agents sync`, reviewing the prompt to enable Habeas's plugin. It MUST explain that the session-start hook advises the agent to run `habeas setup` when configuration is incomplete, how to refresh skills after updating Habeas, and how to preserve local edits when migrating from skills copied by older Habeas versions.
:::

:::{req} layout.project
The Habeas Codicem project MUST keep its mdBook Markdown in `md/` and Rust source in `src/`, and its own book MUST use the Habeas preprocessor.
:::

:::{req} layout.repository_links
The Habeas Codicem book MUST configure mdBook's Git repository link for `nikomatsakis/habeas-codicem` and its page edit links for the `main` branch.
:::

:::{req} integration.symposium
For a Cargo workspace that depends on `habeas`, interactive `cargo agents sync` MUST let the user enable Habeas's plugin. Once enabled, it MUST make the skills shipped with the selected Habeas version available to configured agents. After the selected version changes, `cargo agents sync` MUST provide the skills shipped with the new version.
:::

:::{req} integration.symposium_cli
The enabled Habeas Symposium plugin MUST request a global Cargo installation of the `habeas` CLI and use that installation as its session-start hook command, so the CLI is available when the hook advises an agent to run `habeas setup`.
:::

:::{req} integration.symposium_setup_hook
For an enabled Habeas plugin, a supported agent session started within a Cargo and mdBook project depending on `habeas` MUST receive a Symposium `SessionStart` hook. The hook MUST check whether the configuration and files required by `requirements.cli.setup` are present and compatible. If not, it MUST provide nonblocking agent context saying that Habeas Codicem is not fully configured and suggesting `habeas setup`. If setup is complete or no applicable project is found, it MUST provide no setup advisory. The hook MUST NOT modify project files or run setup itself.
:::

:::{req} integration.ci
GitHub Actions MUST run `cargo test --locked --workspace` and `mdbook test` for pull requests and pushes to `main`. After those checks pass on `main`, it MUST build the book and deploy the generated `book/` directory to GitHub Pages. The book MUST set `output.html.site-url` to `/habeas-codicem/` for project-site hosting.
:::

:::{req} integration.release
A dedicated GitHub Actions workflow MUST run release-plz on pushes to `main` to create release pull requests and publish approved versions. Publishing MUST require a merged release-plz pull request. The release job MUST request an OIDC token for crates.io trusted publishing, and both jobs MUST use a GitHub token from a repository secret so release pull requests can trigger the normal pull request checks.
:::

:::{req} ci.span
A `{req}` definition MUST use an opening fence of at least three colons immediately followed by `{req} <name>`. A closing fence MUST have at least as many colons as the opening fence. The requirement body MUST be all Markdown between those fence lines, including headings, examples, diagrams, and blank lines, and MUST NOT include text after the closing fence. Leading and trailing whitespace MUST be excluded when comparing the body with evidence.
:::

:::{req} ci.fence_lengths
A `{req}` opening fence MAY contain four or more colons. Habeas MUST accept opening fences of any length of at least three colons. A colon-only line with fewer colons than the opener MUST remain inside the requirement body, allowing a nested colon directive. A closing fence outside a fenced code example with at least as many colons as the opener MUST end the body.
:::

## Requirements found in the codebase audit

:::{req} integration.report_api
`habeas::validate_repository(root)` MUST return a report with the number of discovered requirements, the number of evidence files, and all validation errors for the supplied project. Ordinary invalid evidence MUST be reported in that report; an inaccessible project root MUST return an I/O error.
:::

:::{req} ci.book_source
Validation MUST read the mdBook source directory from `[book].src` in `book.toml`, using `src` when the setting is absent. It MUST report an unreadable or malformed book configuration and reject an empty, absolute, or parent-traversing source path.
:::

:::{req} ci.identifiers
Requirement and reference names MUST have nonempty dot-separated segments containing only ASCII letters, digits, underscores, or hyphens. An invalid `{req}` directive or role MUST remain literal and MUST NOT define or reference a requirement. For nested chapter paths, a leading `reqs/` and a final `index` or `README` segment MUST be omitted from the requirement namespace.
:::

:::{req} ci.definition_errors
Validation MUST report a requirement or example with a missing closing fence or an empty body. Full names of `{req}` and `{example}` definitions MUST be unique across both kinds. Habeas MUST report every duplicate full name, including requirement/requirement, example/example, and requirement/example pairs, and identify both the duplicate declaration and the first declaration.
:::

:::{req} ci.input_containment
Every project file named directly by an evidence input or resolved from its selector MUST resolve within the project root, including when its path contains symlinks. Validation MUST report a missing or unreadable file and a path that resolves outside the project.
:::

:::{req} references.markdown_literals
In Markdown, escaped roles and role-looking text in inline code, fenced code, or raw HTML MUST remain literal and MUST NOT count as references. A `{req}` directive inside a fenced code example and a legacy inline `{req}` role MUST NOT define a requirement.
:::

:::{req} references.safe_badges
The book preprocessor MUST escape requirement names and link targets when inserting badge HTML. It MUST encode badge label and message text in Shields.io URL path components so punctuation cannot change the badge URL structure.
:::

:::{req} index.path_conflicts
The mdBook preprocessor MUST report an error when a generated evidence detail page would use the path of an existing book chapter.
:::

:::{req} index.invalid_detail
An evidence detail page MUST continue to show the current requirement or example and validation findings when evidence is missing or invalid. It MUST show readable placeholders for missing rationale or inputs and expose the contents of an unreadable JSON record.
:::

:::{req} cli.invocation
`habeas mdbook supports html` MUST succeed, and a support query for another renderer MUST fail. An unknown command or invalid setup option combination MUST fail with a usage message. In particular, `habeas agents` and `habeas setup --install-skills` MUST fail with a usage message.
:::

:::{req} cli.setup_project
`habeas setup` MUST find an ancestor containing both `Cargo.toml` and `book.toml`, require a nonempty relative mdBook source path without parent components and a `SUMMARY.md` at that path, and report an error when these prerequisites are absent or invalid.
:::

:::{req} cli.setup_css
`habeas setup` MUST preserve existing entries in `output.html.additional-css` while adding the Habeas stylesheet at most once. It MUST report an error if `additional-css` is not an array.
:::

:::{req} cli.setup_local_path
When given `--path <crate-dir>` and no existing Habeas dev dependency, `habeas setup` MUST add that local crate as the dev dependency. Repeating setup MUST NOT add a duplicate Habeas dependency.
:::

## Examples

:::{req} examples.definition
An mdBook chapter MAY define an example with an opening fence of at least three colons immediately followed by `{example} <name>` and a closing colon-only fence of at least the opening length. The full name MUST use the same chapter-derived namespace and valid-name rules as `{req}`. The body MUST include all Markdown between the fences, excluding surrounding whitespace, and MUST describe a concrete case with expected behavior. An example MUST contain at least one valid `{cite}` reference to a requirement it illustrates. An example directive inside a fenced code block MUST remain literal.
:::

:::{req} examples.evidence
Every defined example MUST have an `evidence/<full-example-name>.json` file whose `example` string exactly matches the current example body. A true determination MUST have at least one `{test}` reference to the example, covered by an evidence input. The agent MUST identify the tests in the rationale and explain how their setup and assertions cover every interesting aspect and expected result of the example. Habeas MUST apply the same missing-evidence, false-determination, input validation, and reference-coverage checks used for requirements. It MUST report a true determination with no test reference and changed example text with a compact line-oriented diff.
:::

:::{req} examples.index
When a `{requirements-index}` chapter exists, it MUST list examples as well as requirements, identify each entry's kind, show each example's evidence state, and add one evidence detail subchapter per example. An example definition badge MUST link to its detail page. The detail page MUST link back to the example's anchored definition and to the index; references to the example MUST link to its definition. An example detail page MUST show validation findings when its evidence is missing or invalid.
:::

## Rust item and definition selector examples

:::{example} references.selector_nested
In `src/lib.rs`, `fn outer() { fn inner() {} }` gives the selectors `fn[outer]` and `fn[outer]/fn[inner]`. {cite}`requirements.references.rust_item_selectors`
:::

:::{example} references.selector_impl_and_trait
In `src/lib.rs`, a method `run` in `impl Thing` has selector `impl[Thing]/fn[run]`; a method `run` in `impl Trait for Thing` has selector `impl[Thing/Trait]/fn[run]`; and a method `read` declared in `trait Read` has selector `trait[Read]/fn[read]`. {cite}`requirements.references.rust_item_selectors`
:::

:::{example} references.selector_inline_and_foreign
A function `run` inside an inline `mod local { ... }` in `src/lib.rs` has selector `mod[local]/fn[run]`. A function defined in a separate `src/local.rs` file has a selector within that file, without reconstructing the path of a `mod local;` declaration elsewhere. A foreign function `puts` in an `extern "C"` block has selector `extern[C]/fn[puts]`. {cite}`requirements.references.rust_item_selectors`
:::

:::{example} references.selector_duplicate_names
Two sibling declarations named `run` in one file, guarded by different `#[cfg]` attributes, have selectors `fn[run]#1` and `fn[run]#2` in source order. Their source line numbers are not part of either selector. {cite}`requirements.references.rust_item_selectors`
:::

:::{example} references.selector_use
For consecutive sibling declarations `use alpha::A;` and `use beta::B;`, the selectors are `use#1` and `use#2`. Changing an import target leaves that declaration's positional selector the same and changes its raw evidence input. A named declaration such as `struct Packet;` instead has selector `struct[Packet]`. {cite}`requirements.references.rust_item_selectors` {cite}`requirements.ci.rust_item_inputs`
:::

:::{example} references.selector_raw_lines
For a function preceded by a doc comment, an attribute, and an attached citation comment, a Rust-item input starts at the earliest attached line and ends at the function's final line. Its `text` contains those complete raw source lines. Inserting lines before them changes the displayed range without changing the input. {cite}`requirements.ci.rust_item_inputs`
:::

:::{example} references.selector_same_line
For `impl Thing { fn run() {} }` on one line, `habeas add-input --for-requirement requirements.foo --rust-item src/lib.rs:1` adds separate inputs for `impl[Thing]` and `impl[Thing]/fn[run]`. Both contain the complete raw line. {cite}`requirements.cli.add_input_rust_item`
:::

:::{example} references.definition_glob_cases
Given a requirement `foo.a.bar` and a named example `foo.a.b.bar`, `foo.*.bar` matches both, in that order. A requirements input stores both full names and their complete raw fenced declarations. Adding another matching definition invalidates the existing input. {cite}`requirements.references.definition_globs` {cite}`requirements.ci.requirements_inputs`
:::

:::{example} references.attestation_reassessment
A function attestation has a Rust-item input selecting its function and a requirements input matching the definitions reviewed for it. Changing the function's raw lines or adding a definition matched by the requirements glob invalidates the attestation until the agent reviews it again. {cite}`requirements.references.rust_function_attestations`
:::

:::{example} cli.definition_glob_search
Given a requirement `foo.ready` and a named example `foo.case`, `habeas info 'foo.*'` displays both, and `habeas status 'foo.*' --all` lists both with their kinds and evidence states. {cite}`requirements.cli.info` {cite}`requirements.cli.status`
:::

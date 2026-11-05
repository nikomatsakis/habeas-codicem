# Command-line interface

:::{req} clap
The `habeas` CLI MUST use `clap` to define and parse its commands and options.
:::

:::{req} mdbook
The `mdbook` subcommand MUST act as the mdBook preprocessor for HTML books, rendering requirement definitions and linked reference badges. This project's `book.toml` MUST configure `cargo habeas mdbook` as the `habeas` preprocessor, using a repository-local Cargo alias for `cargo run --release -p habeas --`. Consumer books MUST continue to invoke the installed `habeas mdbook` CLI directly.
:::

:::{req} setup
Running `habeas setup` in an existing Cargo and mdBook project MUST add Habeas as a dev dependency, configure `habeas mdbook` as its preprocessor, add the badge stylesheet, and create `tests/habeas.rs` that calls `habeas::validate()`. Repeating setup MUST avoid duplicate configuration and MUST NOT overwrite a differing test or stylesheet. Setup MUST NOT install or modify agent skills.
:::

:::{req} status
`habeas status [<glob>] [--all] [--verbose]` MUST consider requirements and named examples whose complete full names match the glob defined by `references.definition_globs`; without a glob it MUST consider every definition. By default, it MUST list matching definitions in full-name order when their evidence is missing, has a false determination, or fails the same evidence and reference checks as `habeas::validate()`. It MUST summarize FIXME judgments under {cite}`requirements.cli.fixme_summary`, with accepted FIXME subjects listed individually only when `--verbose` or `--all` is selected. `--all` MUST list every matching definition with its kind and evidence state. Every form MUST show validation findings, including repository-wide Rust attestation findings, work from within a Cargo and mdBook project, and report invalid arguments or a supplied glob matching no definition clearly.
:::

:::{req} fixme_summary
`habeas status` MUST group the selected requirement and example judgments marked
FIXME by issue reference and report how many distinct judgments each issue
affects. It MUST recognize `#123` and `owner/repo#123` within recorded FIXME
strings, including `FIXME(...)` and explanatory text. A GitHub issue URL such as
`https://github.com/owner/repo/issues/123` MUST identify the same issue as
`owner/repo#123`. Issue numbers MUST be positive, complete numeric tokens with
leading zeros normalized; repository spelling MUST remain case-sensitive, and
unqualified and qualified references MUST remain distinct.

A judgment MUST count at most once for each issue, even when that issue occurs
repeatedly or in several FIXME strings. A judgment naming several issues MUST
count in each corresponding group. Judgments with no recognized issue in any
FIXME string MUST be counted once in a separate "FIXME without an issue" group.
Issue groups MUST be ordered by decreasing affected-judgment count, then by
normalized reference; the group without an issue MUST follow them. A supplied
glob MUST restrict these counts to the selected subjects.

`--verbose` MUST list the full name and kind of every judgment in each issue
group and in the group without an issue, along with its original FIXME strings.
The summary MUST include valid FIXME determinations even when other evidence
checks fail; grouping MUST NOT hide their validation findings. Malformed
determinations MUST remain validation errors under {cite}`requirements.ci.outcomes`.
:::

:::{req} status_table_alignment
When `habeas status` or `habeas status --all` displays a Markdown table, it MUST left-align and space-pad the Definition and Evidence columns so every row, including the header and separator row, has its `|` column delimiters in the same positions. Column widths MUST be determined by the widest displayed cell in each column.
:::

:::{req} shared_status_rendering
`habeas status` and the mdBook requirements index MUST generate their Definition/Evidence Markdown tables through the same renderer. That renderer MUST calculate column widths, align the columns, and display the same evidence-state labels in both outputs. Callers MAY select different definitions and supply different kind-labeled name cells so mdBook can link to detail pages.
:::

:::{req} info
`habeas info <glob>` MUST match complete requirement and named example names using `references.definition_globs`; a full name without `*` is an exact selection. It MUST print matches in full-name order as Markdown with the same kind-labeled definition, Validation findings, Rationale, and Inputs structure and evidence states as mdBook detail pages, including readable output for missing or invalid evidence. It MUST work without a requirements index directive and report an invalid glob or no matches clearly. Habeas's bundled agent guidance MUST tell agents they can use this command to inspect a requirement or named example.
:::

:::{req} add_input
`habeas add-input` MUST accept exactly one evidence target: `--for-requirement <full-name>`, `--for-example <full-name>`, or `--for-rust-item <rust_file> <rust_item>` identifying an explicitly declared function or method. It MUST accept exactly one input mode: `--file-range`, `--rust-item`, or `--requirements`. It MUST use the same input schema and resolution rules as validation. Repeating a selector for a target MUST refresh its existing input rather than add a duplicate; an unchanged input MUST leave the record unchanged. The command MUST report every input it adds or refreshes. It MUST create a missing target record with a pending determination. Whenever it changes a record, it MUST leave `determination` false with a nonempty rationale indicating that review is pending.
:::

:::{req} add_input_file_range
`habeas add-input --file-range <file>:<start>-<end>` MUST add or refresh one file-range input containing the current raw lines in the inclusive range. It MUST reject a missing file or invalid range.
:::

:::{req} add_input_rust_item
`habeas add-input --rust-item <rust-file>:<line>` MUST parse the file with `syn` and add or refresh one Rust-item input for every explicit item declared on that line. An item with a declared name is declared on the line containing that name; an item without one is declared on the line containing its distinctive keyword or introducer, such as `use` or `impl`. Each input MUST store the repository-relative `rust_file`, the item's syntactic `rust_item` selector, and its complete raw source lines. The command MUST report every selected item and reject a line on which no item is declared.
:::

:::{req} add_input_requirements
`habeas add-input --requirements <glob>` MUST add or refresh one requirements input containing the matched full names and their concatenated raw declaration sources. It MUST report the matched names in expansion order and reject a glob that matches no requirement or named example.
:::

:::{req} function_attestation_guidance
Habeas's bundled agent skills and any output that requests or reports a function attestation MUST explain its purpose: the configured model reviews the function and applicable requirement and example texts to judge whether the function cites every applicable requirement or named example. They MUST direct the supervising agent to run `habeas author --all` for that assessment. The model's rationale MUST explain how it identified the relevant definitions, why its requirements inputs cover them, and how the cited texts account for the function's behavior, including interactions between requirements. Matching input snapshots and a matching citation list alone MUST NOT justify a true determination. A changed function or reviewed definition set MUST be reassessed by the harness before its determination is true. {cite}`requirements.authoring.function_attestations`
:::

:::{req} bootstrap_skill
Habeas MUST bundle a skill for deriving initial requirements from an existing codebase. The skill MUST direct agents to read the code and tests, map every first-party test to at least one requirement or example, identify and map every source unit implementing end-user-visible functionality or interactions to a requirement, and report gaps rather than claim complete coverage without an independent inventory review.
:::

:::{req} requirements_review_skill
The bundled requirements skill MUST direct agents adding or changing user-visible behavior in a Habeas project to draft stable requirement IDs and exact text, obtain the user's agreement, and install the agreed requirements in the mdBook before writing implementation or test code. It MUST direct agents to review substantive changes to agreed obligations with the user. The bootstrap skill MUST require user review of requirements inferred from existing code before installing them. The evidence skill MUST accept missing evidence or truthful false judgments from the harness while implementation is pending; it MUST NOT direct the supervising agent to write placeholder evidence itself.
:::

:::{req} behavior_review_guidance
Habeas's bundled agent skills MUST direct agents reviewing code against requirements to inspect every end-user-observable result and branch, including errors and input handling, and verify both that the code fulfills each applicable requirement and that it introduces no additional end-user-observable behavior without an agreed requirement. They MUST explain the requirement and example review criteria in {cite}`requirements.ci.review_criteria`, including test adequacy and justified absence of tests for requirements, and the three outcomes and documented-gap convention in {cite}`requirements.ci.outcomes`. Where requirements interact, agents MUST cite every applicable requirement at the relevant implementation point and explain how their texts jointly account for the behavior. A citation alone MUST NOT count as that explanation.
:::

:::{req} internal_detail_guidance
Habeas's bundled agent skills MUST NOT offer a waiver for an uncited first-party Rust function. When a function serves only as an internal implementation detail and no existing requirement explains it, Habeas's bundled agent skills MUST direct agents to propose an explicit project requirement, such as `internal-detail`, obtain the user's agreement, cite it on the function, and run `habeas author --all` for the harness to assess its evidence. That assessment MUST account for whether the cited requirement explains the function's behavior. An internal-detail requirement MUST NOT justify an otherwise unspecified end-user-observable behavior.
:::

:::{req} invocation
`habeas mdbook supports html` MUST succeed, and a support query for another renderer MUST fail. An unknown command or invalid setup option combination MUST fail with a usage message. In particular, `habeas agents` and `habeas setup --install-skills` MUST fail with a usage message.
:::

:::{req} setup_project
`habeas setup` MUST find an ancestor containing both `Cargo.toml` and `book.toml`, require a nonempty relative mdBook source path without parent components and a `SUMMARY.md` at that path, and report an error when these prerequisites are absent or invalid.
:::

:::{req} setup_css
`habeas setup` MUST preserve existing entries in `output.html.additional-css` while adding the Habeas stylesheet at most once. It MUST report an error if `additional-css` is not an array.
:::

:::{req} setup_local_path
When given `--path <crate-dir>` and no existing Habeas dev dependency, `habeas setup` MUST add that local crate as the dev dependency. Repeating setup MUST NOT add a duplicate Habeas dependency.
:::

:::{example} definition_glob_search
Given a requirement `foo.ready` and a named example `foo.case`, `habeas info 'foo.*'` displays both, and `habeas status 'foo.*' --all` lists both with their kinds and evidence states. {cite}`requirements.cli.info` {cite}`requirements.cli.status`
:::

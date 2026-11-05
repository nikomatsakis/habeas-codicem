---
name: habeas-evidence
description: Record and repair Habeas evidence for requirements, examples, source references, and Cargo validation failures.
---

# Habeas evidence

Run `habeas status [<glob>]` to find requirements and named examples whose evidence needs attention, or add `--all` to see every matching definition. Run `habeas info <glob>` to read definitions, evidence states, rationales, inputs, and validation findings as Markdown. Globs match whole names; `*` matches any number of characters, including dots.

For each requirement or example, maintain `evidence/<full-name>.json` with exactly `determination`, `rationale`, `inputs`, and a text field matching the directive: `requirement` for `{req}` or `example` for `{example}`. Copy the current Markdown between the directive's opening and closing fences into that text field, excluding leading and trailing whitespace. Assess the named subject using its cited requirements and definitions as context. Record one of three determinations:

- `true`: the implementation and tests support the whole subject under the review criteria below.
- `false`: an unacknowledged implementation or testing gap remains, or the available information does not support a judgment. Explain the difference between observed noncompliance and missing information in the rationale.
- `{"FIXME": ["#125", "rust-lang/rust#123"]}`: source comments or tests explicitly acknowledge the uncovered aspects, with no additional unacknowledged gap. The array contains at least one nonempty string identifying those gaps by issue reference or other source content. The convention is `FIXME(#125)` or `FIXME(rust-lang/rust#123)`, but references may also appear elsewhere in the inputs. Do not require that exact spelling or invent an issue number. Explain each gap and its source location in the rationale, and capture the comments or tests and any referenced source content as evidence inputs.

CI accepts true and FIXME judgments for requirements and examples when all other evidence checks pass. FIXME remains visible in status and the book; it does not excuse missing, stale, malformed, or uncovered evidence. Function attestations still require true.

An approved requirement or example may be installed before its implementation exists. Record it with `determination: false` and a rationale describing the pending work; an empty `inputs` array is valid until there are lines to cite. Expect validation to fail during this interval. Once code and tests satisfy the approved definition, inspect them, snapshot their exact lines, and only then change the determination to `true`. Review a substantive change to an approved obligation with the user before changing its requirement text.

Each input has one of four forms (the definition form is described below):

- File range: `file`, 1-based inclusive `line_start` and `line_end`, and `text` containing the exact raw lines.
- Rust item: repository-relative `rust_file`, syntactic `rust_item` selector, and `text` containing the complete raw lines, including attached comments, doc comments, and attributes.
- Requirements: `requirements` glob, sorted `matches` of full requirement and named example names, and `text` containing the matched raw fenced declarations in that order, with one blank line between them.

Use `habeas add-input` to capture or refresh an input:

```sh
habeas add-input --for-requirement requirements.ci.inputs --file-range src/evidence.rs:1-20
habeas add-input --for-example reference.cli.unknown_command --rust-item tests/cli.rs:26
habeas add-input --for-rust-item src/evidence.rs 'fn[validate_repository]' --requirements 'requirements.ci.*'
```

The `--rust-item` input mode selects every Rust item declared on the given line. A named item is declared where its name appears; an unnamed item is declared where its keyword appears. The target flag `--for-rust-item` takes a source path and a selector. An unchanged selector is left untouched; adding or refreshing an input sets the determination to false for review. Every `{impl}`, `{cite}`, or `{test}` reference to a definition must fall within one of that definition's resolved input ranges.

For every first-party Rust function or method, maintain exactly one entry in `evidence/rust/<rust_file>.json`. The file contains `rust_file` and a `functions` array. Each entry contains `rust_item`, `citations` as objects with `role` and `name`, `determination`, `rationale`, and `inputs`. Give it one Rust-item input selecting its function and at least one requirements input whose matches include all its citations. The attestation confirms that the function carries citations for **every applicable** requirement or named example. Inspect the whole function, search the relevant definitions, and explain how you identified the applicable set, why the input globs cover it, and how the cited texts jointly account for its behavior. A matching snapshot and citation list alone do not justify `determination: true`. Reassess after the function or selected definitions change.

Function-attestation determinations remain boolean and judge citation completeness, not whether the requirements are satisfied.

For a requirement, assess whether the cited code implements it as stated and whether its tests cover the interesting cases in the design space. In the rationale, explain the implementation and testing support. If there are no tests, judge whether the logic is simple enough not to need them or testing is impractical, and explain why. This exception applies to requirements, not examples.

For an example, assess whether tests cover its described scenario and expected outcomes. A true determination requires at least one relevant `{test}` reference covered by an evidence input. In the rationale, name the tests and explain how their setup and assertions cover every interesting aspect and expected result. Inspect the tests and their results; a reference alone does not establish adequate coverage. Use FIXME for explicitly documented coverage gaps under the criteria above, and false for other missing coverage.

When assessing one subject, inspect every end-user-observable result and branch relevant to it, including errors and input handling. Use related requirements as context without making separate judgments about their complete implementation. In a broader code review, confirm that the code fulfills each applicable requirement and introduces no additional observable behavior without an agreed requirement. When requirements interact, cite every applicable requirement at the relevant implementation point and explain how their texts jointly account for the behavior. A citation alone does not prove the behavior is specified.

Every explicit function or method in first-party Rust sources needs its own requirement or example reference, including trait methods and test helpers. Each example cites a requirement. Do not waive an uncited function because it is an internal helper. If no behavior-specific requirement explains it, propose an explicit project requirement such as `internal-detail` for the user's agreement, cite it, and give evidence for each such function explaining why it introduces no additional end-user-observable behavior. Do not use internal-detail to justify unspecified observable behavior.

Run `cargo test --workspace` after changing requirements, examples, source references, or evidence. Read the full set of Habeas errors: missing and orphan records, malformed fields, false determinations, stale definition text, changed input lines, unknown references, uncovered reference lines, uncited Rust functions, and stale or missing function attestations.

Do not make a failing check green by inventing input text or changing a determination without examining the cited files.

Named definitions use a fourth input form: `definition` (one full definition name)
and `text` (the complete raw fenced declaration, including `:phrase:`). They have
no standalone determination. `habeas add-input` automatically captures definitions
referenced with `{term}` by the subject text or reviewed inputs, including transitive
dependencies. Cycles are allowed; retain one input per definition. Changes to a
phrase or body require reassessment. Requirements globs select requirements and
examples, while their term dependencies are captured separately.

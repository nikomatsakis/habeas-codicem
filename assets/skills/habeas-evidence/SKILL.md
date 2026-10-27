---
name: habeas-evidence
description: Record and repair Habeas evidence for requirements, examples, source references, and Cargo validation failures.
---

# Habeas evidence

Run `habeas status` to find requirements whose evidence needs attention, or `habeas status --all` to see every requirement. Run `habeas info <full-requirement-name>` to read the current requirement, evidence state, rationale, inputs, and validation findings as Markdown. The selector can also be a regular expression over full requirement names.

For each requirement or example, maintain `evidence/<full-name>.json` with exactly `determination`, `rationale`, `inputs`, and a text field matching the directive: `requirement` for `{req}` or `example` for `{example}`. Copy the current Markdown between the directive's opening and closing fences into that text field, excluding leading and trailing whitespace. Set `determination` to `true` only after checking the behavior; otherwise use `false` and explain the remaining work in `rationale`.

An approved requirement or example may be installed before its implementation exists. Record it with `determination: false` and a rationale describing the pending work; an empty `inputs` array is valid until there are lines to cite. Expect validation to fail during this interval. Once code and tests satisfy the approved definition, inspect them, snapshot their exact lines, and only then change the determination to `true`. Review a substantive change to an approved obligation with the user before changing its requirement text.

Each input names a repository-relative `file`, a 1-based inclusive `line_start` and `line_end`, and a `text` array containing the exact current text of those lines. Cite the lines that support the determination. Every `{impl}`, `{cite}`, or `{test}` reference to the definition must fall within one of its input ranges.

For an example, cite at least one relevant test with `{test}`. In the rationale, name the tests and explain how their setup and assertions cover every interesting aspect and expected result. Inspect the tests and their results; a reference alone does not establish adequate coverage. Keep the determination false while coverage is missing.

Run `cargo test --workspace` after changing requirements, examples, source references, or evidence. Read the full set of Habeas errors: missing and orphan records, malformed fields, false determinations, stale definition text, changed input lines, unknown references, and uncovered reference lines.

Do not make a failing check green by inventing input text or changing a determination without examining the cited files.

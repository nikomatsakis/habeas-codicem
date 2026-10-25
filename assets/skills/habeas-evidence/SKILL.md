---
name: habeas-evidence
description: Record and repair Habeas evidence for requirements, source references, and Cargo validation failures.
---

# Habeas evidence

Run `habeas status` to find requirements whose evidence needs attention, or `habeas status --all` to see every requirement. Run `habeas info <full-requirement-name>` to read the current requirement, evidence state, rationale, inputs, and validation findings as Markdown. The selector can also be a regular expression over full requirement names.

For each requirement, maintain `evidence/<full-name>.json` with exactly `requirement`, `determination`, `rationale`, and `inputs`. Copy the current Markdown between the `{req}` directive's opening and closing fences into `requirement`, excluding leading and trailing whitespace. Set `determination` to `true` only after checking the implementation; otherwise use `false` and explain the remaining work in `rationale`.

An approved requirement may be installed before its implementation exists. Record it with `determination: false` and a rationale describing the pending work; an empty `inputs` array is valid until there are lines to cite. Expect validation to fail during this interval. Once code and tests satisfy the approved requirement, inspect them, snapshot their exact lines, and only then change the determination to `true`. Review a substantive change to the approved obligation with the user before changing its requirement text.

Each input names a repository-relative `file`, a 1-based inclusive `line_start` and `line_end`, and a `text` array containing the exact current text of those lines. Cite the lines that support the determination. Every `{impl}`, `{cite}`, or `{test}` reference to the requirement must fall within one of its input ranges.

Run `cargo test --workspace` after changing requirements, source references, or evidence. Read the full set of Habeas errors: missing and orphan records, malformed fields, false determinations, stale requirement text, changed input lines, unknown references, and uncovered reference lines.

Do not make a failing check green by inventing input text or changing a determination without examining the cited files.

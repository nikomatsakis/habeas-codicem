# CI and evidence validation

:::{req} coverage
Every requirement defined by a fenced `{req}` directive in a chapter under the configured mdBook source directory MUST have an evidence file named `evidence/<full-requirement-name>.json`. The full name MUST use the chapter's path-derived namespace followed by the directive name; for example, a directive named `check` in `md/requirements.md` has the full name `requirements.check`. Habeas MUST apply this rule to its own requirements in this chapter.
:::

:::{req} orphans
Every JSON file directly under `evidence/` MUST correspond to a declared requirement or named example. A JSON file at `evidence/rust/<rust_file>.json` MUST correspond to a first-party Rust source file containing an explicitly declared function or method. Habeas MUST report evidence JSON files in other locations under `evidence/` and Rust evidence files or entries whose source item no longer exists.
:::

:::{req} schema
A requirement or example evidence file at `evidence/<full-name>.json` MUST be a JSON object with exactly `requirement` or `example`, `determination`, `rationale`, and `inputs`. The definition text and rationale MUST be nonempty strings, `determination` MUST use one of the outcomes defined by {cite}`requirements.ci.outcomes`, and `inputs` MUST be an array. Every evidence input, including one in a Rust function attestation, MUST have exactly one of four forms: a file-range input with `file`, `line_start`, `line_end`, and `text`; a Rust-item input with `rust_file`, `rust_item`, and `text`; a requirements input with `requirements`, `matches`, and `text`; or a definition input with `definition` and `text`. File paths MUST be repository-relative, line numbers MUST be positive integers, `matches` MUST be an array of distinct full definition names, and `text` MUST be a nonempty array of strings.
:::

:::{req} requirement_text
The `requirement` string in each evidence file MUST match all Markdown inside its fenced `{req}` directive, excluding leading and trailing whitespace. Habeas MUST report changed requirement text.
:::

:::{req} determination
For requirement and example evidence, both `true` and `FIXME` MUST satisfy the determination check in `habeas::validate()`. A false determination MUST fail validation and be reported with its rationale. Every Rust function attestation MUST have `determination: true`; a false attestation MUST be reported with its rationale. All other evidence checks MUST still pass: FIXME MUST NOT excuse malformed, missing, or stale evidence or uncovered references. Validation MUST check the recorded outcome, schema, input freshness, and reference coverage without inference or reassessing whether a source comment adequately acknowledges a gap.
:::

:::{req} review_criteria
The reviewer MUST assess the named subject using its text, referenced requirements,
and terminology definitions. For a requirement, the reviewer MUST assess whether
the cited code implements every stated obligation and whether its tests cover the
interesting cases in the design space, including relevant boundaries and failure
cases. When there are no tests, the reviewer MUST assess whether the logic is
simple enough to need no test or a test is impractical to author, and explain that
judgment in the rationale.

For an example, the reviewer MUST assess whether tests exist whose setup and
assertions cover its scenario and expected outcomes. The rationale MUST identify
the relevant sources and explain the implementation and testing judgments for a
requirement, or the test coverage for an example. Referenced requirements provide
context; they MUST NOT become additional subjects to assess in full.
:::

:::{req} outcomes
A requirement or example judgment MUST use one of these outcomes, applying
{cite}`requirements.ci.review_criteria`:

- `true`: the sources support the subject and its testing obligations.
- `false`: the sources do not support the subject, or information is missing,
  and the gaps are not all explicitly acknowledged in source comments or tests.
- `FIXME`: some aspects remain unmet or untested, and source comments or tests
  explicitly acknowledge those gaps. The reviewer MUST explain the gaps and
  their source locations in the rationale and record their issue references or
  other identifying content from the inputs.

The JSON `determination` MUST be `true`, `false`, or an object containing exactly
`"FIXME"` with a nonempty array of nonempty strings, such as
`{"FIXME": ["#125", "rust-lang/rust#123"]}`. The expected source convention is
`FIXME(issue-number)`, for example `FIXME(#125)` or
`FIXME(rust-lang/rust#123)`. References MAY appear elsewhere in the comment or
test; the reviewer MUST recognize explicit acknowledgments without requiring
that spelling. When no issue reference is present, the strings MUST identify the
documented gap using its source content. A FIXME judgment MUST be grounded in
recorded evidence inputs; a bare issue reference without an acknowledged gap
MUST NOT suffice.
:::

See [FIXME examples](#fixme-examples) for concrete review judgments and CI results.

:::{req} inputs
For a file-range input, Habeas MUST verify that `line_start` and `line_end` identify an inclusive, ordered range in `file` and that `text` exactly matches the current raw lines in that range. Habeas MUST report missing files, invalid ranges, and changed lines.
:::

:::{req} rust_item_inputs
For a Rust-item input, Habeas MUST parse `rust_file` with `syn` and resolve `rust_item` to exactly one item. It MUST determine an inclusive range of complete raw source lines containing that item, including attached doc comments, attributes, and leading citation comments. It MUST compare `text` with those lines without serializing the `syn` tree, and derive the range again during validation for display and reference coverage. Moving unchanged lines because of edits earlier in the file MUST NOT invalidate the input. Habeas MUST report a missing file, parse failure, invalid or ambiguous selector, or changed lines.
:::

:::{req} requirements_inputs
For a requirements input, Habeas MUST match the glob in `requirements` against all defined requirements and named examples. It MUST recompute the matches during validation, sort their full names, and compare them with `matches`. It MUST take each match's complete raw fenced declaration, including its opening and closing lines, concatenate those declarations in full-name order with one blank line between them, and compare the resulting raw lines with `text`. It MUST retain every match's current source file and inclusive line range for display and reference coverage. A glob matching no definitions MUST be invalid. Adding, removing, renaming, or changing a matched definition MUST invalidate the stored input; moving an unchanged declaration within the book MUST NOT. Errors MUST identify the glob and affected definitions when available.
:::

:::{req} errors
Habeas MUST continue validating after an error and report all validation errors it finds in one run. Each error MUST identify the affected evidence file or requirement and explain the failure.
:::

:::{req} diagnostics
Every validation error MUST identify the failed check and give an agent enough detail to diagnose it. If the stored requirement text differs from the current chapter text, the error MUST name the requirement and show a compact line-oriented diff, with `-` for recorded lines and `+` for current lines. If recorded input text differs from the current file, the error MUST name the file and line range and show the same kind of diff. Other errors MUST state the specific missing or invalid value and the expected value when available. Diagnostics SHOULD be concise because agents are the primary readers.

```text
evidence/requirements.ci.inputs.json: requirement text changed (md/requirements.md:43)
- Habeas SHOULD check input lines.
+ Habeas MUST check input lines.
```
:::

:::{req} span
A `{req}` definition MUST use an opening fence of at least three colons immediately followed by `{req} <name>`. A closing fence MUST have at least as many colons as the opening fence. The requirement body MUST be all Markdown between those fence lines, including headings, examples, diagrams, and blank lines, and MUST NOT include text after the closing fence. Leading and trailing whitespace MUST be excluded when comparing the body with evidence.
:::

:::{req} fence_lengths
A `{req}` opening fence MAY contain four or more colons. Habeas MUST accept opening fences of any length of at least three colons. A colon-only line with fewer colons than the opener MUST remain inside the requirement body, allowing a nested colon directive. A closing fence outside a fenced code example with at least as many colons as the opener MUST end the body.
:::

:::{req} book_source
Validation MUST read the mdBook source directory from `[book].src` in `book.toml`, using `src` when the setting is absent. It MUST report an unreadable or malformed book configuration and reject an empty, absolute, or parent-traversing source path.
:::

:::{req} identifiers
Requirement and reference names MUST have nonempty dot-separated segments containing only ASCII letters, digits, underscores, or hyphens. An invalid `{req}` directive or role MUST remain literal and MUST NOT define or reference a requirement. For nested chapter paths, a leading `reqs/` and a final `index` or `README` segment MUST be omitted from the requirement namespace.
:::

:::{req} definition_errors
Validation MUST report a requirement or example with a missing closing fence or an empty body. Full names of `{req}` and `{example}` definitions MUST be unique across requirements, examples, and named definitions. Habeas MUST report every duplicate full name, including collisions involving named definitions, and identify both the duplicate declaration and the first declaration.
:::

:::{req} input_containment
Every project file named directly by an evidence input or resolved from its selector MUST resolve within the project root, including when its path contains symlinks. Validation MUST report a missing or unreadable file and a path that resolves outside the project.
:::

## Existing evidence inputs used by authoring

:::{req} authoring_inputs
When `habeas author-evidence` assesses a subject with existing evidence, the
current resolution of each existing input selector MUST contribute to the
mandatory input set defined by {cite}`requirements.authoring.mandatory_inputs`.
The contribution MUST use the current source text selected by that input, rather
than its old recorded text. Reading these selectors MUST NOT modify the existing
evidence. The previous determination and rationale MUST NOT be supplied as
source evidence.
:::

## FIXME examples

### An acknowledged implementation gap

:::{example} fixme_implementation_gap
A queue requirement says insertion into a full queue returns `Full`, including
when its capacity is zero. The implementation handles positive capacities but
contains this comment:

```text
// FIXME(#125): capacity-zero insertion does not return Full yet.
```

The reviewer finds the remaining behavior implemented and adequately tested.
The judgment is `{"FIXME": ["#125"]}`, with a rationale explaining the zero-capacity
gap and locating the comment. The recorded inputs include the comment and reviewed
sources. CI accepts the evidence when all other checks pass.
{cite}`requirements.ci.outcomes`
{cite}`requirements.ci.determination`
:::

### Missing tests and references outside the marker

:::{example} fixme_example_test_gaps
A named example describes recovery after a timeout and a partial write. Its test
source explicitly records the two missing cases:

```text
// FIXME(#125): retry-after-timeout coverage is missing.
// Partial-write recovery is also untested; tracked in rust-lang/rust#123.
```

The reviewer finds no other gaps and records
`{"FIXME": ["#125", "rust-lang/rust#123"]}`. The rationale identifies both missing
tests, and the inputs include these comments. The second reference is recognized
even though it is outside `FIXME(...)`. CI accepts this documented incomplete
coverage; the example does not receive a `true` judgment.
{cite}`requirements.ci.outcomes`
{cite}`requirements.ci.determination`
{cite}`requirements.examples.evidence`
:::

### An acknowledged gap without an issue number

:::{example} fixme_without_issue
The implementation and tests satisfy a requirement except for a platform-specific
test gap recorded in the test source:

```text
// FIXME: cancellation races are not tested on Windows.
```

The reviewer records
`{"FIXME": ["cancellation races are not tested on Windows"]}`, using the comment's
content without inventing an issue number. The rationale explains the gap and
locates the comment in the recorded inputs. CI accepts the evidence when all
other checks pass.
{cite}`requirements.ci.outcomes`
{cite}`requirements.ci.determination`
:::

### An additional, unacknowledged gap

:::{example} fixme_with_unacknowledged_gap
The queue implementation acknowledges its zero-capacity bug with `FIXME(#125)`.
The reviewer also finds that insertion overwrites an existing entry when a
positive-capacity queue is full, contrary to the requirement. No source comment
or test acknowledges that second gap. The judgment is `false`, with a rationale
describing both gaps and their source locations. The existing FIXME does not
account for the additional defect. CI rejects the false determination.
{cite}`requirements.ci.outcomes`
{cite}`requirements.ci.determination`
:::

### Recorded FIXME evidence becomes stale

:::{example} fixme_stale_input
A current evidence record has `{"FIXME": ["#125"]}` and passes CI. A source comment
within one of its recorded file-range inputs then changes. CI rejects the record
because the recorded text no longer matches, even though its determination is
still FIXME. Validation reports the changed input without running a model or
reassessing the documented gap.
{cite}`requirements.ci.determination`
{cite}`requirements.ci.inputs`
:::

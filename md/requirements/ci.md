# CI and evidence validation

:::{req} coverage
Repository validation MUST check that every requirement defined by a fenced `{req}` directive in a chapter under the configured mdBook source directory has an evidence file named `evidence/<full-requirement-name>.json`, and MUST report each requirement whose evidence file is missing. The full name MUST use the chapter's path-derived namespace followed by the directive name; for example, a directive named `check` in `md/requirements.md` has the full name `requirements.check`.
:::

:::{req} orphans
Every JSON file directly under `evidence/` MUST correspond to a declared requirement or named example. Habeas MUST report evidence JSON files that do not, and evidence JSON files in any other location under `evidence/`.
:::

:::{req} schema
A requirement or example evidence file at `evidence/<full-name>.json` MUST be a JSON object with exactly `requirement` or `example`, `determination`, `rationale`, and `inputs`. The definition text and rationale MUST be nonempty strings, `determination` MUST use one of the outcomes defined by {cite}`requirements.ci.outcomes`, and `inputs` MUST be an array. Every evidence input MUST have exactly one of three forms: an outline input with `file`, `item`, and `hash`; a requirements input with `requirements`, `matches`, and `text`; or a definition input with `definition` and `text`. File paths MUST resolve to permitted inputs under {cite}`requirements.ci.input_containment`. Project-local paths SHOULD be recorded relative to the project for portability; absolute and parent-relative paths MAY be used when their resolved targets are permitted. `matches` MUST be an array of distinct full definition names, `hash` MUST be a lowercase hexadecimal SHA-256 digest, and `text` MUST be a nonempty array of strings. Evidence MUST NOT store source text for outline inputs.
:::

:::{req} requirement_text
The `requirement` string in each evidence file MUST match all Markdown inside its fenced `{req}` directive, excluding leading and trailing whitespace. Habeas MUST report changed requirement text.
:::

:::{req} determination
For requirement and example evidence, both `true` and `FIXME` MUST satisfy the determination check in `habeas::validate()`. A false determination MUST fail validation and be reported with its rationale. Each definition's determination MUST be checked on its own; a false determination MUST NOT fail the definitions it implements, tests, or cites. All other evidence checks MUST still pass: FIXME MUST NOT excuse malformed, missing, or stale evidence or uncovered references. Validation MUST check the recorded outcome, schema, input freshness, and reference coverage without inference or reassessing whether a source comment adequately acknowledges a gap.
:::

:::{req} review_criteria
The reviewer MUST assess the named subject using its text, referenced requirements,
and terminology definitions. For a requirement, the reviewer MUST assess whether
the code marked with `{impl}` implements every stated obligation completely and
exactly as stated, and whether its
tests cover the interesting cases in the design space, including relevant
boundaries and failure cases. The reviewer MUST also assess whether the
implementation is marked completely: code that implements part of the
requirement under {cite}`requirements.references.targets` without an `{impl}`
reference to it, including code that refers to it only with `{cite}`, MUST be
treated as a gap. Behavior in the `{impl}` code that the requirement does not
specify MUST be accounted for by another definition that the same code
implements; otherwise it MUST be treated as a gap. When another requirement implements part of the subject, the
reviewer MUST assess whether its text carries out that part, and MUST NOT
reassess its implementation, tests, or evidence; that requirement is assessed
separately. When there are no tests, the reviewer MUST assess whether the logic is
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


:::{req} outline_inputs
For an outline input, Habeas MUST parse `file` with its language's grammar under
{cite}`requirements.references.outlines` and resolve `item` to exactly one
outline node under {cite}`requirements.references.outline_selectors`. It MUST
compare `hash` with the node's current structural hash under
{cite}`requirements.references.structural_hash`. It MUST derive the node's
inclusive line range, including attached comments and attributes, during
validation for display and reference coverage. Moving the node, editing other
nodes, or editing comments its profile does not hash MUST NOT invalidate the
input. Habeas MUST report a missing file, unsupported language, parse failure,
invalid or ambiguous selector, or changed hash.
:::

:::{req} requirements_inputs
For a requirements input, `requirements` MUST be a {term}`requirements.references.definition_selection`, and Habeas MUST match it against all defined requirements and named examples. It MUST recompute the matches during validation, sort their full names, and compare them with `matches`. It MUST take each match's complete raw fenced declaration, including its opening and closing lines, concatenate those declarations in full-name order with one blank line between them, and compare the resulting raw lines with `text`. It MUST retain every match's current source file and inclusive line range for display and reference coverage. A selection matching no definitions MUST be invalid. Adding, removing, renaming, or changing a matched definition MUST invalidate the stored input; moving an unchanged declaration within the book MUST NOT. Errors MUST identify the selection and affected definitions when available.
:::

:::{definition} validation_error
:phrase: validation error

One message in a repository validation report, describing a single failed check.
It begins with the affected evidence file, requirement, or source location, names
the failed check, and states the specific missing, invalid, or changed value,
with the expected value when available. It is concise because agents are its
primary readers.
:::

:::{req} errors
Repository validation MUST continue after a failed check and report every failed
check it finds in one run, each as a {term}`requirements.ci.validation_error`.
This is a convention for every validation error; the requirement for each check
states which conditions that check reports. A file that cannot be parsed, such as
a malformed `habeas.toml`, `book.toml`, or evidence file, is one failed check:
its error MAY report only the first parse or decoding problem in that file, and
checks that do not depend on its contents MUST still run.
:::

:::{req} diagnostics
When stored requirement text differs from the current chapter text, the
{term}`requirements.ci.validation_error` MUST name the requirement and show a
compact line-oriented diff, with `-` for recorded lines and `+` for current lines.
When an outline input's hash differs from its node's current hash, the validation
error MUST name the file, the selector, and the node's current line range.

```text
evidence/requirements.ci.outline_inputs.json: requirement text changed (md/requirements.md:43)
- Habeas SHOULD check input lines.
+ Habeas MUST check input lines.
```
:::

:::{req} span
A `{req}` definition MUST use an opening fence of at least three colons immediately followed by `{req} <name>`. A closing fence MUST have at least as many colons as the opening fence and nothing else on its line except whitespace. As in CommonMark fences, an opening or closing fence MAY be indented by up to three spaces; a line indented by four or more spaces MUST NOT be a fence. The requirement body MUST be all Markdown between those fence lines, including headings, examples, diagrams, and blank lines, and MUST NOT include text after the closing fence. Leading and trailing whitespace MUST be excluded when comparing the body with evidence.
:::

:::{req} fence_lengths
A `{req}` opening fence MAY contain four or more colons. Habeas MUST accept opening fences of any length of at least three colons. A colon-only line with fewer colons than the opener MUST remain inside the requirement body, allowing a nested colon directive. A closing fence outside a fenced code example with at least as many colons as the opener MUST end the body.
:::

:::{req} book_source
Validation MUST read the mdBook source directory from `[book].src` in `book.toml`, using `src` when the setting is absent. It MUST report an unreadable or malformed book configuration and reject an empty, absolute, or parent-traversing source path.
:::

:::{req} identifiers
Requirement and reference names MUST have nonempty dot-separated segments containing only ASCII letters, digits, underscores, or hyphens. A `{req}` directive whose full name is invalid, including one whose chapter path gives an invalid namespace, and a role with an invalid name MUST remain literal and MUST NOT define or reference a requirement. For nested chapter paths, a leading `reqs/` and a final `index` or `README` segment MUST be omitted from the requirement namespace.
:::

:::{req} definition_errors
Validation MUST report a requirement or example with a missing closing fence or an empty body. Full names of `{req}` and `{example}` definitions MUST be unique across requirements, examples, and named definitions. Habeas MUST report every duplicate full name, including collisions involving named definitions, and identify both the duplicate declaration and the first declaration.
:::

:::{req} input_containment
Every file named directly by an evidence input or resolved from its selector MUST resolve within the project root unless permitted by workspace path configuration or discovered Cargo generated-output policy. Symlink targets MUST follow the same policy. Validation MUST enforce explicit workspace disallow entries and report missing, unreadable, or excluded files. It MUST check existing generated inputs without running a build. {cite}`requirements.configuration.evidence_paths` {cite}`requirements.authoring.generated_inputs`
:::

## Existing evidence inputs used by authoring

:::{req} authoring_inputs
When `habeas author` assesses a subject with existing evidence, that evidence
MUST only determine whether the subject needs assessment. Its inputs,
determination, and rationale MUST NOT contribute to the mandatory input set
defined by {cite}`requirements.authoring.mandatory_inputs` or otherwise be
supplied to the model. A new assessment's evidence MUST record only the inputs
supplied to it, so sources that an earlier assessment read but that no longer
exist or are no longer relevant cannot block or shape reassessment.
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
A current evidence record has `{"FIXME": ["#125"]}` and passes CI. The code in one
of its recorded outline inputs then changes. CI rejects the record because the
input's hash no longer matches, even though its determination is still FIXME. Validation reports the changed input without running a model or
reassessing the documented gap.
{cite}`requirements.ci.determination`
{cite}`requirements.ci.outline_inputs`
:::

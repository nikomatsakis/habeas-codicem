# CI and evidence validation

:::{req} coverage
Repository validation MUST check that every requirement defined by a fenced `{req}` directive in a chapter under the configured mdBook source directory has an evidence file named `evidence/<full-requirement-name>.json`, and MUST report each requirement whose evidence file is missing. The full name MUST use the chapter's path-derived namespace followed by the directive name; for example, a directive named `check` in `md/requirements.md` has the full name `requirements.check`.
:::

:::{req} orphans
Every JSON file directly under `evidence/` MUST correspond to a declared requirement or named example. Habeas MUST report evidence JSON files that do not, and evidence JSON files in any other location under `evidence/` except the review directories defined by {cite}`requirements.references.citation_review` and {cite}`requirements.references.site_review`.
:::

:::{req} schema
A requirement or example evidence file at `evidence/<full-name>.json` MUST be a JSON object with exactly `requirement` or `example`, `determination`, `rationale`, `references`, and `inputs`. `references` MUST be the reference-set digest defined by {cite}`requirements.ci.reference_set`. The definition text and rationale MUST be nonempty strings, `determination` MUST use one of the outcomes defined by {cite}`requirements.ci.outcomes`, and `inputs` MUST be an array. Every evidence input MUST have exactly one of three forms: an outline input with `file`, `item`, and `hash`; a requirements input with `requirements`, `matches`, and `text`; or a definition input with `definition` and `text`. File paths MUST resolve to permitted inputs under {cite}`requirements.ci.input_containment`. Project-local paths SHOULD be recorded relative to the project for portability; absolute and parent-relative paths MAY be used when their resolved targets are permitted. `matches` MUST be an array of distinct full definition names, `hash` MUST be a lowercase hexadecimal SHA-256 digest, and `text` MUST be a nonempty array of strings. Evidence MUST NOT store source text for outline inputs.
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
implementation is reachable: code that carries out part of the requirement
under {cite}`requirements.references.roles` and that no item marked `{impl}` for
it reaches under {cite}`requirements.references.procedure`, including code that
refers to it only with `{cite}`, MUST be treated as a gap. Behavior in the `{impl}` code that the requirement does not
specify MUST be accounted for by another definition that the same code
implements; otherwise it MUST be treated as a gap. When another requirement implements part of the subject, the
reviewer MUST assess whether its text carries out that part, and MUST NOT
reassess its implementation, tests, or evidence; that requirement is assessed
separately. When there are no tests, the reviewer MUST assess whether the logic is
simple enough to need no test or a test is impractical to author, and explain that
judgment in the rationale.

The reviewer MUST apply these rules in every assessment. MUST and MUST NOT state
obligations; where the reviewer judges whether source code meets the text, an unmet SHOULD MUST be treated as a gap unless the subject's text or a source comment gives a reason it does not apply; MAY imposes nothing to
check; examples and sample output in the text illustrate the obligations its
prose states and bind their exact form only where the text says so. When the subject's
text admits two readings its wording supports equally and they lead to different conclusions, such as whether code conforms or which role a citation calls for, the ambiguity MUST be treated as a gap and the rationale MUST name
the readings. A test that verifies the subject without a `{test}` reference to it
MUST be treated as a marking gap. A gap MUST be
reported only for a specific obligation and a specific source item the reviewer
has read; the reviewer MUST look for unreached implementations among the sources
that reference the subject and by targeted search for what its obligations
produce, not by surveying unrelated code. Style, performance, logging, how
helpers are factored, robustness the text does not require, and silently ignoring input the text does not mention MUST NOT be treated as gaps, and the reviewer MUST NOT report bugs or issues outside the
subject's text.

For a distributed requirement under {cite}`requirements.references.distributed`,
the reviewer MUST also assess the requirement and its queries. Each obligation
MUST be either one that each site upholds on its own, checked by its review under
{cite}`requirements.references.site_review`, or one that particular code carries
out, assessed as above through its `{impl}` and `{test}` references; an
obligation neither covers MUST be treated as a gap. The reviewer MUST judge whether
each query's conditions are adequate and adequately tested: the site queries
MUST select every place the text's description of a site covers and no place
outside the requirement's stated scope, and each query's `match` and `no_match`
snippets MUST test its conditions as tests of code would, judged as such; a
condition without a snippet of its own MUST NOT by itself be treated as a gap. Because queries are heuristics, the reviewer MUST
judge them by a preponderance of the evidence, asking whether it is more likely
than not that the site queries find the places the requirement applies: a
specific place in the sources that a site query misses or wrongly selects MUST be
treated as a gap, and a form that does not occur in the sources MUST NOT.
Obligations that decide which places are sites, such as a naming rule, MUST be
checked by the reviewer's search of the sources. Violation queries direct site
reviews to likely problems; the reviewer SHOULD note constructs they miss, but
their incompleteness MUST NOT by itself make the determination false. Any other
gap in the queries or snippets MUST make the determination false, even when the
code is correct. The reviewer SHOULD run each site query once, scan its matches,
and search briefly for the construct the text describes, and MUST NOT review each
site or treat a site without an `{impl}` reference as a gap.

For an example, the reviewer MUST assess whether tests exist whose setup and
assertions cover its scenario and expected outcomes. The rationale MUST identify
the relevant sources and explain the implementation and testing judgments for a
requirement, or the test coverage for an example. Referenced requirements provide
context; they MUST NOT become additional subjects to assess in full.
:::

:::{req} outcomes
Every assessment judgment MUST use one of these outcomes, applying
{cite}`requirements.ci.review_criteria`:

- `true`: the sources support the subject and its testing obligations.
- `false`: the sources do not support the subject, or information is missing,
  and the gaps are not all acknowledged by `{fixme}` references to it.
- `FIXME`: some aspects remain unmet or untested, and `{fixme}` references to
  the subject acknowledge every one of those gaps. The reviewer MUST explain the
  gaps and their source locations in the rationale and record their issue
  references or other identifying content from the inputs.

The JSON `determination` MUST be `true`, `false`, or an object containing exactly
`"FIXME"` with a nonempty array of nonempty strings, such as
`{"FIXME": ["example/queue#125", "rust-lang/rust#123"]}`. A gap MUST count as
acknowledged only by a `{fixme}` reference under
{cite}`requirements.references.roles` whose body describes that gap and that
references the assessed definition, or, for a site review, the distributed requirement from within that site; a citation review MUST NOT accept a `{fixme}`; a plain
`FIXME` comment, another marker such as `TODO`, or prose MUST NOT. The strings
MUST list each distinct issue reference the acknowledging `{fixme}` references
name in their option lines, as written after `:github:`, and, for a `{fixme}`
naming no issue, its body's words, without comment delimiters, joined by single
spaces. A `{fixme}` reference describing a gap that no longer exists MUST itself
be treated as a gap. A `{fixme}` naming a GitHub issue that
{cite}`requirements.authoring.github_reads` reports closed MUST NOT count as
acknowledging its gap. A FIXME judgment MUST be grounded in recorded evidence
inputs.
:::

See [FIXME examples](#fixme-examples) for concrete review judgments and CI results.


:::{req} reference_set
A definition's reference set MUST consist of its current incoming `{impl}`, `{test}`, and `{fixme}` edges, except a `{fixme}` inside a site of the definition under {cite}`requirements.references.site_review`, under {cite}`requirements.references.roles` and
{cite}`requirements.references.anchors`: for each edge, its role and the file and
outline selector of the node the reference belongs to under
{cite}`requirements.references.outline_comments`, or of the anchored file or
node, and, for a fenced reference in a source outside the book, the words of
its body under the word rule of {cite}`requirements.references.structural_hash`.
`{cite}` references MUST NOT contribute. The reference-set digest MUST be
the SHA-256 digest of an unambiguous encoding of those entries in sorted order,
so it does not depend on line numbers or on the order references are found.
Authoring MUST record the digest of the reference set it supplied, and
validation MUST recompute it and report a changed reference set as stale
evidence. Adding or removing an `{impl}`, `{test}`, or `{fixme}` reference or anchor,
changing its role, editing its commentary, or moving it to a different node
MUST therefore make the evidence stale even when no recorded input changed; moving a reference within
the node it belongs to MUST NOT.
:::

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

:::::{req} errors
Repository validation MUST continue after a failed check and report every failed
check it finds in one run, each as a {term}`requirements.ci.validation_error`.
This is a convention for every validation error; the requirement for each check
states which conditions that check reports. A file that cannot be parsed, such as
a malformed `habeas.toml`, `book.toml`, or evidence file, is one failed check:
its error MAY report only the first parse or decoding problem in that file, and
checks that do not depend on its contents MUST still run.

The code that sequences the repository's checks carries out the first paragraph.
A repository-validation function that collects validation errors MUST hold them
in a parameter or local variable named `errors`, and a function with such a
variable is a site below; authoring code that collects errors only to abort is
not one. After one of its checks fails, a site MUST still run every later check
that does not depend on the failed check's result, including later checks in the
same loop iteration, and MUST NOT report one failure twice.

::::{sites} error_accumulators
```yaml
language: rust
paths: [src, crates/habeas-sdk/src]
exclude: [src/authoring/inputs.rs, src/authoring/tools.rs]
rule:
  kind: function_item
  has:
    stopBy: { kind: function_item }
    any:
      - kind: parameter
        has: { field: pattern, regex: "^errors$" }
      - kind: let_declaration
        has: { field: pattern, regex: "^errors$" }
  not:
    inside:
      stopBy: end
      kind: mod_item
      has: { field: name, regex: "^tests$" }
match:
  - "fn check(value: &str, errors: &mut Vec<String>) {}"
  - |
    fn run() {
        let mut errors: BTreeSet<String> = BTreeSet::new();
        check("", &mut errors);
    }
no_match:
  - "fn check(value: &str, discovery_errors: &mut Vec<String>) {}"
  - |
    fn run() {
        let mut problems = Vec::new();
        check("", &mut problems);
    }
  - |
    mod tests {
        fn check(errors: &mut Vec<String>) {}
    }
```
::::

::::{violations} early_exits
```yaml
language: rust
paths: [src, crates/habeas-sdk/src]
within: error_accumulators
rule:
  any:
    - kind: try_expression
    - kind: return_expression
    - kind: continue_expression
    - kind: break_expression
match:
  - |
    fn check(v: &Value, errors: &mut Vec<String>) -> Option<u8> {
        let n = v.as_u64()?;
        if v.is_null() { errors.push("null".into()); }
        Some(n as u8)
    }
  - |
    fn check(items: &[Value], errors: &mut Vec<String>) {
        for item in items {
            if !item.is_object() { errors.push("object".into()); continue; }
            if item.is_null() { errors.push("null".into()); }
        }
    }
  - |
    fn check(v: &Value, errors: &mut Vec<String>) {
        if !v.is_object() { errors.push("object".into()); return; }
        if v.is_null() { errors.push("null".into()); }
    }
  - |
    fn check(items: &[Value], errors: &mut Vec<String>) {
        for item in items {
            if !item.is_object() { errors.push("object".into()); break; }
        }
        if items.is_empty() { errors.push("empty".into()); }
    }
no_match:
  - |
    fn parse(v: &Value) -> Option<u8> {
        let n = v.as_u64()?;
        Some(n as u8)
    }
```
::::
:::::

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
contains this reference:

```text
// :::{fixme} requirements.queue.full
// :github: example/queue#125
// Capacity-zero insertion does not return Full yet.
// :::
```

The reviewer finds the remaining behavior implemented and adequately tested.
The judgment is `{"FIXME": ["example/queue#125"]}`, with a rationale explaining the zero-capacity
gap and locating the comment. The recorded inputs include the comment and reviewed
sources. CI accepts the evidence when all other checks pass.
{cite}`requirements.ci.outcomes`
{cite}`requirements.ci.determination`
:::

### Missing tests tracked by two issues

:::{example} fixme_example_test_gaps
A named example describes recovery after a timeout and a partial write. Its test
source records the two missing cases:

```text
// :::{fixme} requirements.io.recovery_example
// :github: example/io#125
// :github: rust-lang/rust#123
// Retry-after-timeout and partial-write recovery are untested.
// :::
```

The reviewer finds no other gaps and records
`{"FIXME": ["example/io#125", "rust-lang/rust#123"]}`. The rationale identifies
both missing tests, and the inputs include the reference. CI accepts this documented incomplete
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
// :::{fixme} requirements.tasks.cancel
// Cancellation races are not tested on Windows.
// :::
```

The reviewer records
`{"FIXME": ["Cancellation races are not tested on Windows."]}`, using the
reference's body without inventing an issue number. The rationale explains the gap and
locates the comment in the recorded inputs. CI accepts the evidence when all
other checks pass.
{cite}`requirements.ci.outcomes`
{cite}`requirements.ci.determination`
:::

### An additional, unacknowledged gap

:::{example} fixme_with_unacknowledged_gap
The queue implementation acknowledges its zero-capacity bug with a `{fixme}`
reference naming `example/queue#125`.
The reviewer also finds that insertion overwrites an existing entry when a
positive-capacity queue is full, contrary to the requirement. No `{fixme}`
reference acknowledges that second gap. The judgment is `false`, with a rationale
describing both gaps and their source locations. The existing FIXME does not
account for the additional defect. CI rejects the false determination.
{cite}`requirements.ci.outcomes`
{cite}`requirements.ci.determination`
:::

### The tracking issue closes

:::{example} fixme_closed_issue
A current evidence record has `{"FIXME": ["example/queue#125"]}`, acknowledged by
a `{fixme}` naming `example/queue#125`. The issue is then closed, though the
zero-capacity gap remains. `habeas status` reports the record as stale and lists
the closed issue, exactly as if an input had changed, and `habeas author --all`
reassesses it. The reviewer sees the issue's state as closed, so the `{fixme}` no
longer acknowledges the gap, and the judgment is `false`. Repository validation,
which does not contact GitHub, still accepts the old record until it is
reassessed.
{cite}`requirements.ci.outcomes`
{cite}`requirements.authoring.github_reads`
{cite}`requirements.cli.fixme_issues`
:::

### Recorded FIXME evidence becomes stale

:::{example} fixme_stale_input
A current evidence record has `{"FIXME": ["example/queue#125"]}` and passes CI. The code in one
of its recorded outline inputs then changes. CI rejects the record because the
input's hash no longer matches, even though its determination is still FIXME. Validation reports the changed input without running a model or
reassessing the documented gap.
{cite}`requirements.ci.determination`
{cite}`requirements.ci.outline_inputs`
:::

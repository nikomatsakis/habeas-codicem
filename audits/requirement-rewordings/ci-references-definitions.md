# Requirement rewordings: ci, references, definitions

Rewordings made while turning false evidence records in
`md/requirements/ci.md`, `md/requirements/references.md`, and
`md/requirements/definitions.md` into true ones. Each entry records the old and
new text, the reason, and the evidence rationale that prompted it. No intended
obligation was weakened; where code contradicted a requirement, the code was
fixed instead (see the end of this log).

## requirements.references.targets

Old (last sentence):

> Evidence snapshots and generated build output MUST NOT be counted as additional source references.

New:

> Evidence snapshots and generated build output MUST NOT be counted as additional source references: discovery MUST skip `.git` and `target` directories at any depth and the project-root `book` and `evidence` directories.

Reason: the requirement did not say which directories hold snapshots and build
output, and the code skipped them only at the project root, so a nested crate's
`target/` was counted. The code now skips `.git` and `target` at any depth, as
Rust item coverage already did, and the text states the exact rule so the
reviewer can check it and the test pins it.

Prompted by: `evidence/requirements.references.targets.json`, gap 3 ("The
`.git`, `target`, `book` and `evidence` exclusions apply only at the project
root ... The requirement does not state this either way.").

## requirements.references.rust_item_coverage

Old (first sentence):

> During repository validation, Habeas MUST parse every first-party Rust source file in the project.

New:

> During repository validation, Habeas MUST parse every first-party Rust source file in the project: every `.rs` file except those under `.git` or `target` directories at any depth or under the project-root `book`, `evidence`, or `vendor` directories.

Reason: "first-party" was undefined, and the scan's directory exclusions were
sensible but unstated, so the reviewer read them as silent skipping. The new text
defines first-party sources by the exclusions the code applies; the test now
covers each exclusion and shows that `src/vendor/` and `src/book/` are scanned.

Prompted by: `evidence/requirements.references.rust_item_coverage.json`, gap 2
("Undocumented directory skipping ... The nested-`target` rule is neither
specified nor tested").

## requirements.references.outline_comments

Old:

> A comment that follows source on the same line MUST form a run by itself

New:

> A comment that follows other source on the same line, including another comment, MUST form a run by itself

Reason: the first sentence already treats comments as source ("no other source
between them"), but the reviewer found it ambiguous whether an earlier comment on
the same line counts. The engine treats it as source, so in `/* a */ /* b */`
the second comment is a trailing run. The text now says so, and a test pins it.

Prompted by: `evidence/requirements.references.outline_comments.json`, gap 3
("Untested, ambiguous same-line case ... The requirement doesn't say whether
that counts as 'source'.").

## requirements.references.markdown_literals

Old:

> In Markdown, escaped roles and role-looking text in inline code, fenced code, or raw HTML MUST remain literal and MUST NOT count as references.

New:

> In Markdown, escaped roles and role-looking text in inline code, fenced code, or raw HTML MUST remain literal and MUST NOT count as references. Raw HTML means an HTML block, an HTML comment, or an inline HTML tag including its attributes; text between inline tags is ordinary Markdown.

Reason: the reviewer asked whether `<span>{impl}...</span>` falls under "raw
HTML". In CommonMark, text between inline tags is ordinary Markdown, not raw
HTML, and the implementation protects only the HTML itself. The new sentence
makes the boundary precise; a new test covers HTML blocks, HTML comments,
attributes, and text between inline tags.

Prompted by: `evidence/requirements.references.markdown_literals.json`, gap 2
("Whether that falls under 'raw HTML' is ambiguous, and no test pins the
intended behaviour.").

## requirements.references.book_badges

Old (sentence on legibility):

> Badge text MUST remain legible regardless of whether its link has been visited.

New:

> Badge text MUST remain legible regardless of whether its link has been visited. A badge link MUST show a visible indicator when it has keyboard focus and MAY brighten on hover.

Reason: the anchored stylesheet `md/habeas.css` gives badge links a focus
outline and a hover brightness change, and no definition specified either. Both
are deliberate accessibility and affordance choices, so the requirement now
states them. The other unspecified behaviour the reviewer found, the intro-step
styling, was not badge behaviour. It was moved out of `md/habeas.css` into this
book's own `md/intro.css`, so it no longer ships to user projects.

Prompted by: `evidence/requirements.references.book_badges.json` ("The
stylesheet anchored as its `{impl}` contains behavior the requirement does not
specify").

## requirements.references.definition_glob_cases (example)

Old:

> Given a requirement `foo.a.bar` and a named example `foo.a.b.bar`, `foo.*.bar` matches both, in that order.

New:

> Given a requirement `foo.a.bar` and a named example `foo.a.b.bar`, `foo.*.bar` matches both, in full-name order: `foo.a.b.bar` before `foo.a.bar`.

Reason: `requirements.ci.requirements_inputs` sorts matches by full name, and
`foo.a.b.bar` sorts before `foo.a.bar` because `.` precedes `a`. "In that order"
contradicted the requirement this example illustrates. While writing the
example's test, I found that the old wording could not be satisfied.

Prompted by: `evidence/requirements.references.definition_glob_cases.json` (the
example lacked a test of its ordering, stored declarations and invalidation).
The contradiction surfaced while writing that test.

## requirements.ci.span

Old:

> A closing fence MUST have at least as many colons as the opening fence.

New:

> A closing fence MUST have at least as many colons as the opening fence and nothing else on its line except whitespace. As in CommonMark fences, an opening or closing fence MAY be indented by up to three spaces; a line indented by four or more spaces MUST NOT be a fence.

Reason: `colon_fence` accepts up to three spaces of indentation and rejects four
or more, following CommonMark's fence rule. It also allows trailing whitespace,
but no other text, after a closing fence. Neither rule was specified. The new
text states both, and new tests cover indentation, trailing whitespace, and a
`:::foo` line that does not close the body.

Prompted by: `evidence/requirements.ci.span.json`, gap 1 ("Unspecified
behaviour in `{impl}` code ... accepts up to three leading spaces") and gap 2
(the `:::foo` case).

## requirements.ci.errors

Old (end of requirement):

> This is a convention for every validation error; the requirement for each check states which conditions that check reports.

New:

> This is a convention for every validation error; the requirement for each check states which conditions that check reports. A file that cannot be parsed, such as a malformed `habeas.toml`, `book.toml`, or evidence file, is one failed check: its error MAY report only the first parse or decoding problem in that file, and checks that do not depend on its contents MUST still run.

Reason: the reviewer noted that a `habeas.toml` with several invalid fields
yields one message. Parsing stops at the first syntax or decoding error by
nature, and the intended rule is that the parse is one failed check whose
failure does not stop the others. The new sentence states that rule; a test now
shows that invalid `habeas.toml`, `book.toml` and evidence JSON are each
reported alongside later checks. The other gap the reviewer found, that term
dependency resolution stopped at the first missing term, was a real defect and
was fixed in code.

Prompted by: `evidence/requirements.ci.errors.json`, gap 2 ("Config loading
reports only the first failure") and gap 4 (missing tests).

## requirements.ci.identifiers

Old:

> An invalid `{req}` directive or role MUST remain literal and MUST NOT define or reference a requirement.

New:

> A `{req}` directive whose full name is invalid, including one whose chapter path gives an invalid namespace, and a role with an invalid name MUST remain literal and MUST NOT define or reference a requirement.

Reason: "invalid directive" did not say whether validity concerns the local name
or the full name. Validation already rejected invalid full names, but the book
preprocessor still defined and badged them. The code was fixed to leave such
directives literal, and the text now states the full-name rule explicitly.

Prompted by: `evidence/requirements.ci.identifiers.json`, gap 1 ("Invalid full
names are still defined in the book").

## Code fixes made instead of rewording

Code was changed instead of the requirement in these cases:

- `requirements.ci.identifiers`: the book preprocessor now leaves directives with
  invalid full names literal. `reqs/index.md` now drops both segments, as the
  text says, giving an empty namespace.
- `requirements.ci.errors`: term dependency resolution reports every
  unresolvable term and still checks the definition inputs of the resolvable
  ones.
- `requirements.ci.diagnostics`: the changed-text error now uses the documented
  `requirement text changed (<file>:<line>)` form.
- `requirements.ci.requirements_inputs`: errors now name the glob and the
  still-matched definitions whose text changed.
- `requirements.references.targets`: reference discovery skips `.git` and
  `target` at any depth.

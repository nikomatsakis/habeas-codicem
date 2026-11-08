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

## requirements.references.outlines

Old:

> Habeas MUST parse Rust, Python, TypeScript, CSS, Markdown, TOML, and YAML sources with pinned tree-sitter grammars, selected by file extension.

New:

> Habeas MUST parse Rust, Python, TypeScript, CSS, Markdown, TOML, and YAML sources with pinned tree-sitter grammars, selected by file extension compared case-insensitively. A source with any syntax error MUST fail to parse, reporting the line of its first error.

Reason: `profile_for_path` compares extensions case-insensitively, and
`check_errors` rejects any tree containing an error node and reports the first
error's line. Both are intended, but no definition specified them. New tests
cover both. In the same change, the line-query helpers the reviewer flagged
(`declared_on_line`, `node_lines`, `innermost_at_line`, `display_name`) were
marked with the requirements that specify their behaviour (`cli.add_input_item`,
`ci.outline_inputs`, `outline_comments`, `authoring.outline_tool`,
`rust_item_coverage`). `check_errors` was also marked for `ci.outline_inputs`.

Prompted by: the second reassessment of `evidence/requirements.references.outlines.json`,
gaps 1 and 2. This record was not reassessed again: it used its two
reassessments, so its current evidence is false and stale.

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

### Second rewording: which nodes are comments

Old (opening):

> In every supported language, comments on consecutive lines ...

New:

> Each documented profile defines which grammar nodes are comments; the Markdown profile's comments are HTML comment blocks, so an inline HTML comment within a paragraph is text of its section. In every supported language, comments on consecutive lines ...

Reason: the Markdown profile uses the block grammar and treats only HTML comment
blocks as comments, as `md/reference.md` documents. The requirement never said
that each profile decides what a comment is. The reviewer therefore read "in
every supported language" as requiring inline `<!-- -->` comments after
paragraph text to be trailing comment runs. The new sentence states the
intended, documented behaviour, and a new test pins it.

Prompted by: the reassessed `evidence/requirements.references.outline_comments.json`,
gap 2 ("Markdown recognises only whole `html_block` comments ... An inline
`<!-- -->` that follows text on a line is not treated as a comment").

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

## requirements.ci.coverage (commit 3010f22)

**Old:** "Every requirement defined by a fenced `{req}` directive … MUST have an evidence file named `evidence/<full-requirement-name>.json`. … Habeas MUST apply this rule to its own requirements in this chapter."

**New:** "Repository validation MUST check that every requirement defined by a fenced `{req}` directive … has an evidence file named `evidence/<full-requirement-name>.json`, and MUST report each requirement whose evidence file is missing. …" (the "apply this rule to its own requirements" sentence was removed).

**Reason:** the old text described repository state, so reviewers tried to list `evidence/`, which authoring excludes, and the assessment aborted. The obligation is now on the validation code that enforces the rule. Requested by the user.

## requirements.references.rust_item_coverage — accept habeas:none

**Old:** "…MUST have a valid `\{impl}`, `\{cite}`, or `\{test}` reference to a defined requirement or named example in a Rust comment…"

**New:** "…MUST have a valid `\{impl}`, `\{cite}`, or `\{test}` reference to a defined requirement or named example, or a valid citation of the built-in `habeas:none` under …no_requirement, in a Rust comment…"

**Reason:** step 4 of the reference procedure (commit 8153b92, agreed with the user) directs items that no definition fits to cite `habeas:none`; the coverage rule still required a project definition. The implementation already accepts it.

## requirements.references.citation_review

Old (sentence on mandatory inputs):

> Mandatory inputs MUST be the item's outline node, each cited declaration or built-in text, and the citations' commentary.

New:

> Mandatory inputs, under \{cite}`requirements.authoring.mandatory_inputs`, MUST be the item's outline node, each cited declaration or built-in text, and the citations' commentary.

Reason: `requirements.authoring.mandatory_inputs` says a requirement that
contributes mandatory inputs MUST cite it. This requirement contributes a
citation review's inputs but did not cite it. The citation adds no obligation
and removes none; `AssessmentSources::prepare_citation` and `builtin_input` are
now also marked as implementing `mandatory_inputs`, with a test of the citation
path's deduplication and abort on an unresolvable input.

Prompted by: `evidence/requirements.authoring.mandatory_inputs.json` ("A
contributing requirement does not cite this one. ... It cites
`citation_prompt`, `targets` and `outcomes`, but not `mandatory_inputs`.").

## requirements.references.book_badges (stylesheet layout)

Old (sentence on focus and hover):

> A badge link MUST show a visible indicator when it has keyboard focus and MAY brighten on hover.

New:

> A badge link MUST show a visible indicator when it has keyboard focus and MAY brighten on hover. Badges MAY be laid out inline with the surrounding text, a fenced reference's badge and body MAY be visually set apart from the surrounding text as one block, and a built-in's unlinked badge MAY show a help cursor to signal its hover text.

Reason: the anchored stylesheet `md/habeas.css` lays badges out inline
(`.habeas-req-badge`, `.habeas-shield`), borders and pads each fenced reference
(`.habeas-fenced-ref`), and gives built-in badges `cursor: help`
(`.habeas-builtin`). These are deliberate presentation choices that no
definition stated. The requirement now permits exactly these, so the stylesheet
has no unspecified behaviour; every obligation is unchanged. A new test
(`badge_stylesheet_keeps_visited_links_legible_and_shows_focus`) checks the
visited, focus, and hover rules and that the stylesheet styles only these
classes.

Prompted by: `evidence/requirements.references.book_badges.json` ("No test
covers the CSS obligations." and "The anchored `{impl}` stylesheet contains
behaviour the requirement does not specify. ... The fenced-reference border and
padding are end-user-visible and are neither specified nor covered by another
requirement.").

## `requirements.references.citation_review`: single-review invocation

Old:

> Authoring with `--all` MUST include citation reviews.

New:

> Authoring with `--all` MUST include citation reviews, and `habeas author citations:<file>#<selector>` MUST author the review of the one item it names, where omitting `#<selector>` names the file's root node.

Reason: `AuthoringSubject::parse` in `src/authoring/mod.rs` accepts
`citations:<file>#<selector>` as a single authoring subject and treats an
omitted selector as the root node. `md/reference.md` documents this form, and
the unit tests in `src/authoring/mod.rs` exercise it (`citations:src/lib.rs#fn[helper]`,
`citations:notes.txt`). It is intended behaviour that no requirement stated:
`requirements.authoring.invocation` covers only requirements and named
examples. The requirement now states it; no existing obligation changed.

Prompted by: `evidence/requirements.references.citation_review.json` ("The
single-subject command form `habeas author citations:<file>#<selector>` is not
specified by this requirement or by `requirements.authoring.invocation`. ...
This user-visible CLI behavior is therefore not backed by any requirement
text.").

## requirements.ci.errors (marking scope)

Old (end of requirement):

> ... its error MAY report only the first parse or decoding problem in that file, and checks that do not depend on its contents MUST still run.

New (sentences appended):

> Code carries out this convention when it decides how validation proceeds after a failed check: it continues to later checks, items, or files, substitutes a fallback so later checks still run, or omits a second report of a failure another check already reports. Code that checks one field or value and reports its failures implements that check's requirement, not this convention.

Reason: the requirement is a convention for every validation error, and
\{cite}`requirements.references.targets` says such a convention is implemented
by the source that carries it out, but did not say which helpers along the way
count. Existing `{impl}` marks covered some report-and-continue helpers
(`book_source_dir`, `DiscoverySources::load`, `files_with_extension`) but not
others, so a reviewer found the marking inconsistent. The new sentences give the
test the marks now follow. Under it, `discovered_files`, `citations::validate`,
`validate_input`, `validate_outline_input`, `validate_resolved_input`,
`validate_committed_credentials` and `DiscoverySources::walk` were marked; leaf
field checks (`check_keys`, `string_field`, `array_field`, `text_field`) were
not. `parse_citation` was changed to check every field instead of stopping at
the first malformed one. A new test,
`git_index_failure_is_reported_once_and_later_checks_still_run`, covers the
Git-index failure path.

Prompted by: `evidence/requirements.ci.errors.json` ("`discovered_files` ...
is marked only `references.targets`, not `ci.errors` ... The `ci.errors` marks
are applied inconsistently across the code that implements the convention ...
No test for the Git-index failure path.").

## requirements.references.structural_hash (comment groups)

Old:

> For an included comment, only its text MUST be hashed, not its delimiter tokens such as `//`, `///`, `//!`, `/*`, `*/`, or a leading `*`, so reflowing it across more or fewer lines does not change the hash.

New:

> For an included comment, only its text and its profile's comment group MUST be hashed, not its delimiter tokens such as `//`, `///`, `//!`, `/*`, `*/`, or a leading `*`; the Rust profile's groups are inner and outer doc comments. The words of consecutive included comments of one group MUST be hashed as one ordered list, so reflowing them across more or fewer lines or comments does not change the hash, while moving text between groups does.

Reason: `Encoder::prose` tags each run of comment words with a group, and
`rust::doc_prose` picks the inner or outer group with
`rust::is_inner_doc_comment`. Without the group, moving words from a nested
item's `///` comment into its module's adjacent `//!` comment would leave the
module's hash unchanged, so the split is intended, but the requirement did not
state it. `is_inner_doc_comment` is now marked for this requirement, and the new
test `inner_and_outer_doc_comments_hash_in_separate_groups` pins the behaviour.
The summary in `md/reference.md` was updated to match. In the same change, tests
were added for comment exclusion in the Python, TypeScript, CSS, Markdown, TOML
and YAML profiles, for Python decorator attachment, and for whitespace in
plain-text `line` hashes.

Prompted by: `evidence/requirements.references.structural_hash.json` ("`rust::is_inner_doc_comment` ... carries only an `outline_comments` impl marker, not `structural_hash` ... The `inner_doc_comment` / `outer_doc_comment` group split also goes beyond what the requirement states.").

## requirements.ci.errors (several checks in one unit)

Old (last sentence):

> Code that checks one field or value and reports its failures implements that check's requirement, not this convention.

New:

> Code that checks one field or value and reports its failures implements that check's requirement, not this convention; code that runs several checks on one file, record, or input and continues after one of them fails carries out the convention.

Reason: after the previous rewording, the reassessment read `validate_input`,
`validate_outline_input` and `validate_resolved_input` as field-level checks
and called their `{impl}` marks over-marking, though each runs several checks on
one input and continues after a failure. The added clause settles which side of
the line they fall on. In the same change, `evidence_input_ranges`,
`terms::direct_references`, `parse_citation` and `DiscoverySources::files`
were marked, because each skips or keeps an item so that a failure is reported
once by its own check while checking continues. The `ci.errors` mark on
`DefinitionKind::noun`, which only names a definition kind, was removed.

Prompted by: the reassessment of `evidence/requirements.ci.errors.json`
("Over-marking of field-level checks. `validate_input`, `validate_outline_input`
and `validate_resolved_input` check individual fields ... Unmarked code that
decides how validation proceeds ... Questionable marking. `DefinitionKind::noun`").

Follow-up (no text change): the second reassessment found three more functions
that omit a second report of a failure another check makes and lacked the mark:
`citations::citation_items`, `fenced::source_fences` and
`evidence::reference_set_digest`. They are now marked `{impl}` for
`requirements.ci.errors`. The "omits a second report" clause makes every
graceful-degradation site an implementing site, so later reviews may keep
finding unmarked ones; narrowing which code carries out this convention is a
substantive change to review with the user.

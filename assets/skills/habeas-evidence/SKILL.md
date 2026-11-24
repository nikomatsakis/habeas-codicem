---
name: habeas-evidence
description: Use when Habeas evidence needs creating or refreshing, or when `habeas coverage` or `habeas status` reports Habeas findings such as missing, stale, or false evidence, uncovered `{test}` lines, unmarked tests or Cargo examples, FIXME judgments, citation or site reviews, or disagreeing assessments, even if the user only says "the habeas test is failing". Covers running `habeas author` and fixing what it flags. If evidence went stale because code or requirements moved, use habeas-refactor first.
---

# Habeas evidence

## Workflow

Run commands from the project root, the directory with `book.toml` and
`Cargo.toml`.

- [ ] 1. `git add` new source files. Authoring and reference discovery read only
      files in the Git index; unstaged edits to tracked files are read.
- [ ] 2. `habeas status` lists what needs attention; `habeas info <name>` shows
      one definition's judgment, rationale, and findings. Fix what the findings
      name (see "Fixing findings").
- [ ] 3. `habeas author --all` assesses every definition whose evidence is
      missing, stale, or invalid and skips current ones; `-v` prints each false
      judgment's rationale. `habeas author <name>` does one.
- [ ] 4. `habeas coverage`, then `habeas coverage-check`. While findings remain
      that you can fix, return to step 2.
- [ ] 5. Check the "Disagreements" section of `habeas status`. Validation
      accepts a `true` or FIXME whose rationale records a disagreement, so only
      this shows it; see "Disagreeing assessments".
- [ ] 6. Stop and report each finding that needs the user: a new or changed
      requirement, a gap to defer, a blocker, missing information needed to
      decide test adequacy, or a missing record or false judgment whose rationale
      names only implementation that is still pending, which is expected.
      Don't keep iterating on those, and never write placeholder evidence.
      Don't wait for answers before step 7.
- [ ] 7. Commit every change under `evidence/` (new, updated, and deleted
      records) with the source and requirement changes they assess; CI
      validates only committed evidence.
- [ ] 8. Next: for findings that need code or tests, or requirement text that
      must change, return to the habeas-requirements skill.

## Testing and coverage findings

Follow the habeas skill's "Testing and coverage artifacts": default to
`habeas coverage` for unit, integration and focused runs through `cargo nextest`;
run doctests separately with `cargo test --doc --workspace`, and never stage or
commit disposable coverage artifacts in the Cargo target directory.

For post-suite `habeas coverage-check` failures, first distinguish missing or
stale measurement from actual coverage gaps. Recollect measurement failures;
no explanation can waive them. Add tests for coverage gaps, or give a
substantive explanation through a standard `habeas:coverage` citation and author
its independent citation review. Neither `habeas:util` nor `{fixme}` excuses
execution coverage. Use `habeas test` for full verification after the changes.
The habeas skill's "Testing and coverage artifacts" gives the per-file target
and function obligations.
For CI integration, recommend the separate collection and check steps in the
Habeas [CI example](../habeas/references/ci.md); collection alone does not enforce
the policy.
{impl}`requirements.test-coverage.check.guidance`

## Judgments

`habeas author` records, for each requirement and example, a model's judgment
of whether the reviewed tests adequately check it, or, for structural assessment
of a requirement with implementation roots and no test edges, whether its stated
structure is supported by those implementations; `habeas coverage` validates
those records without calling a model. A judgment is one of three outcomes:

- `true`: the reviewed tests adequately check the definition, or its
  implementations support its structural obligations, with no gap under the
  assessment's criteria. Runtime obligations still require test evidence.
- `false`: there is a gap that no `{fixme}` reference acknowledges; the
  rationale names it.
- FIXME: `{fixme}` references acknowledge every remaining gap; validation
  accepts it while each named issue is open.

The intended requirement review checks structure and behavior independently.
The current choice of one assessment, including its incomplete handling of
mixed requirements and absent edge kinds, is the accepted gap tracked by issue
#51. Keep its fenced `{fixme}` acknowledgments until the independent checks
and their regression tests are implemented.

## Rules

- Never write or edit evidence JSON, determinations, rationales, or inputs
  yourself, and never hand that to another agent. Evidence means something only
  as an independent assessment that the harness records. If authoring cannot
  run, see Gotchas; when the fix needs the user, such as missing credentials,
  stop and report the blocker.
- Never re-run an assessment to get a different result.
  Results vary between runs, so a lucky pass would not stay true. Fix
  the source, tests, references, or requirement text instead. Use `--force`
  only when something the hash does not see changed: a fix in an ordinary
  comment, a newly staged file, or a reassessment the user asked for.
- Don't read evidence files into your context. Use `habeas status` and
  `habeas info`; to survey many rationales, ask a subagent to read them and
  return short findings.
- A change to agreed requirement text, even a rewording, goes through habeas-requirements steps 3 to 5 (subagent review, the user's agreement, commit) first. Then give the reason, and the rationale that
  prompted it, in the message of the commit that makes it, describing the change
  relative to the prior commit rather than the drafts that led to it.
- Validation is deterministic. Run `habeas author` yourself; never add it to CI.

## Fixing findings

| Finding | Action |
| --- | --- |
| missing or stale evidence | `habeas author --all` |
| false judgment | read the rationale with `habeas info <name>`, fix the gap it names, then `habeas author --all` |
| uncovered `{test}` line | check the reference names the right definition, then `habeas author <full-name>`; fixing the reference makes the record stale, so no `--force` is needed |
| rationale identifies an unmarked standalone test | add `{test}` for the requirement or example it verifies; a reached test helper needs no reference of its own |
| unmarked test or Cargo example | see "Missing test and example references" |
| a real gap the user agrees to defer, a FIXME judgment, or a `{fixme}` issue that closed | read [references/fixme.md](references/fixme.md) |
| false citation or site review | read [references/reviews.md](references/reviews.md) |
| orphaned citation or site review | `habeas author --all` deletes it |
| orphaned evidence for a renamed requirement or example | `habeas refactor --map <old>=<new>` (habeas-refactor), which keeps the evidence |
| orphaned evidence for a removed requirement or example | `git rm` the file the finding names (removing an orphan is not editing evidence), then `habeas author --all` |
| untracked file with references | `git add` it |

A false rationale names one of the gaps listed in "What the review checks" in
the habeas-requirements skill. Behavior with no requirement needs one agreed
with the user. Each determination stands alone: a false requirement does not
fail the definitions it implements, tests, or cites, so fix each on its own.

### Missing test and example references

A test function with `#[test]` or another attribute ending in `test` needs a
`{test}` reference. A Cargo example needs to `{cite}` a named example in the
book. Add the reference for the behavior it checks or illustrates.

Ordinary functions, types, trait methods, and test helpers need no individual
book connection. Keep meaningful `{impl}` roots and optional explanatory
citations. Choose references by "Choosing references" in the habeas-requirements
skill. Recorded reads track evidence freshness, without Git-history grace
periods or unreferenced-item warnings. Execution coverage is checked separately
by `habeas coverage-check`.

## Disagreeing assessments

The goal is that independent assessments of the same subject reach the same
determination, because the requirement, code, and tests leave no gap a careful
reviewer could find and no reading a careful reviewer could take differently.
Assessments that disagree are findings against that goal.

By default (`confirm` policy), `habeas author` checks each `true` or FIXME
result with a second assessment and a verifier, and notes any disagreement at
the end of the recorded rationale. A determination that changes on
reassessment, though the subject and sources have not changed in substance, is
the same kind of finding.

1. Read both assessments: for a recorded disagreement, the rationale and its
   note (`habeas info`); for a changed determination, the new rationale and the
   previous one, which only Git has, such as by asking a subagent to summarize
   `git show HEAD:evidence/<full-name>.json`. Decide whether one found a real
   gap or the requirement text admits more than one reading.
2. Fix the source or tests, or tighten the requirement text through habeas-requirements steps 3 to 5 (subagent review, the user's agreement, commit).

## Gotchas

- What makes evidence stale:

  | Change | Evidence |
  | --- | --- |
  | moving a node within its file without changing its selector (such as `impl[Thing]/fn[run]`), or editing Rust `//` comments | stays current |
  | moving a node to another file or outline node | stale or invalid; use habeas-refactor |
  | renaming a node | stale; reassess |
  | editing code, doc comments, or docstrings | stale |
  | adding, removing, or changing the role of an `{impl}`, `{test}`, or `{fixme}`, or editing its commentary | stale |
  | editing an item with no reference | every record that read it is stale |

  Make related edits to shared helpers together before authoring.
- After a fix the hash does not see, such as one only in an ordinary comment,
  run `habeas author --force <full-name>`.
- A false rationale saying the reviewer could not read a file you created means
  the file is untracked: `git add` it and run `habeas author --force <name>`.
  The inputs changed, so this is not a re-run for a different result.
- If authoring fails on configuration, credentials, throttling, or a denied
  mandatory input, read
  [references/authoring-failures.md](references/authoring-failures.md).

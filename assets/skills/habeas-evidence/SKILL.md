---
name: habeas-evidence
description: Use when Habeas evidence needs creating or refreshing, or when `cargo test` or `habeas status` reports Habeas findings such as missing, stale, or false evidence, uncovered `{impl}` or `{test}` lines, uncovered Rust items, coverage-loss reviews, FIXME judgments, citation or site reviews, or disagreeing assessments, even if the user only says "the habeas test is failing". Covers running `habeas author` and fixing what it flags. If evidence went stale because code or requirements moved, use habeas-refactor first.
---

# Habeas evidence

`habeas author` records, for each requirement and example, a model's judgment
of whether the code and tests satisfy it; `cargo test --workspace` validates
those records without calling a model.

## Workflow

Run commands from the project root, the directory with `book.toml` and
`Cargo.toml`.

- [ ] 1. `git add` new source files. Authoring and reference discovery read only
      files in the Git index; unstaged edits to tracked files are read.
- [ ] 2. `habeas status` lists what needs attention; `habeas info <name>` shows
      one definition's judgment, rationale, and findings. Fix what the findings
      name (see "Fixing findings"), except uncovered Rust items: leave those
      until after step 3, because an assessment that reads an item covers it.
- [ ] 3. `habeas author --all` assesses every definition whose evidence is
      missing, stale, or invalid and skips current ones; `-v` prints each false
      judgment's rationale. `habeas author <name>` does one.
- [ ] 4. `cargo test --workspace`. Handle uncovered items as "Uncovered Rust
      items" says. While findings remain that you can fix, return to step 2.
- [ ] 5. Check the "Disagreements" section of `habeas status`. Validation
      accepts a `true` or FIXME whose rationale records a disagreement, so only
      this shows it; see "Disagreeing assessments".
- [ ] 6. Stop and report each finding that needs the user: a new or changed
      requirement, a gap to defer, a blocker, or a missing record or false
      judgment whose rationale names only implementation that is still pending,
      which is expected. Don't keep
      iterating on those, and never write placeholder evidence. Don't wait for
      answers before step 7.
- [ ] 7. Commit every change under `evidence/` (new, updated, and deleted
      records) with the source and requirement changes they assess; CI
      validates only committed evidence.
- [ ] 8. Next: for findings that need code or tests, or requirement text that
      must change, return to the habeas-requirements skill.

## Rules

- Never write or edit evidence JSON, determinations, rationales, or inputs
  yourself, and never hand that to another agent. Evidence means something only
  as an independent assessment that the harness records. If authoring cannot
  run, see Gotchas; when the fix needs the user, such as missing credentials,
  stop and report the blocker.
- Never re-run an assessment to get a different result or to get an item read.
  Results vary between runs, so a lucky pass or read would not stay true. Fix
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
| uncovered `{impl}` or `{test}` line | check the reference names the right definition, then `habeas author <full-name>`; fixing the reference makes the record stale, so no `--force` is needed |
| rationale names code the reviewer found only by searching | add `{impl}` to the item it names, making it a root; don't restructure the code or add `{cite}` to make it reachable |
| uncovered Rust item | see "Uncovered Rust items" |
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

### Uncovered Rust items

An item is covered by its own reference or by a current record or review that
read it. `habeas author` runs a coverage-loss review when a record it replaces
read the item and the new one does not; `habeas status` shows it beside the
item. Before acting on a `not needed` finding, check from its rationale and the
code whether the earlier read was mistaken. Then, for each item still uncovered
after `habeas author --all`:

| Situation | Action |
| --- | --- |
| coverage-loss review says `root` | add `{impl}` of that definition to the item |
| coverage-loss review says `not needed`, and nothing reaches the item | report it to the user as dead code; don't delete it unasked |
| coverage-loss review says `not needed`, and the item serves other code | add `` // {cite}`habeas:util` `` |
| the earlier read was mistaken, or no coverage-loss review | choose references by "Choosing references" in the habeas-requirements skill |

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
  | editing an item with no reference | every record that read it is stale, and the item is uncovered until they are reassessed |

  Make related edits to shared helpers together before authoring.
- After a fix the hash does not see, such as one only in an ordinary comment,
  run `habeas author --force <full-name>`.
- A false rationale saying the reviewer could not read a file you created means
  the file is untracked: `git add` it and run `habeas author --force <name>`.
  The inputs changed, so this is not a re-run for a different result.
- If authoring fails on configuration, credentials, throttling, or a denied
  mandatory input, read
  [references/authoring-failures.md](references/authoring-failures.md).

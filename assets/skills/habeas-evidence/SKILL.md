---
name: habeas-evidence
description: Use when Habeas evidence needs creating or refreshing, or when `cargo test` or `habeas status` reports Habeas findings such as missing, stale, or false evidence, uncovered `{impl}` or `{test}` lines, uncovered Rust items, coverage-loss reviews, FIXME judgments, citation or site reviews, or disagreeing assessments, even if the user only says "the habeas test is failing". Covers running `habeas author` and fixing the code, tests, or references it flags. To design new or changed requirements before coding, use habeas-requirements; to derive an initial requirement set when most of a codebase has no references, use habeas-bootstrap-requirements.
---

# Habeas evidence

Habeas records, for each requirement and example in a project's mdBook, a
model's judgment of whether the code and tests satisfy it. `habeas author`
produces these records; `cargo test --workspace` validates them without calling
a model.

## Workflow

Run commands from the project root, the directory with `book.toml` and
`Cargo.toml`.

- [ ] 1. `git add` the new source files you created. Untracked files are
      invisible to authoring and reference discovery, and `habeas status` lists
      untracked files that contain references; unstaged edits to tracked files
      are read.
- [ ] 2. `habeas status [<selection>]` lists definitions needing attention;
      `--all` includes accepted ones, and `--verbose` lists the judgments in each
      FIXME issue group. `habeas info <selection>` shows one definition's text,
      judgment, rationale, inputs, and validation findings. A selection is a full
      name or a glob in which `*` matches any characters, including dots.
- [ ] 3. Fix what the findings name (see "Fixing findings"). Leave uncovered
      Rust items until after step 4: an assessment that reads an item covers
      it.
- [ ] 4. `habeas author --all` assesses every definition whose evidence is
      missing, stale, or invalid, keeps current records, and reports how many it
      skipped as current; `-v` prints each false judgment's rationale.
      `habeas author <full-name>` does one.
- [ ] 5. `cargo test --workspace`. Handle items still uncovered as "Uncovered
      Rust items" says. If findings remain that you can fix, return to
      step 2. Also check `habeas status` for recorded disagreements (see
      "Disagreeing assessments"). Stop and report to the user each finding that needs their
      decision, such as a new or changed requirement, a gap to defer, or a
      blocker, and each missing evidence record or false judgment expected because
      implementation is pending; don't keep iterating on those. When you commit, include every change under
      `evidence/` (new, updated, and deleted records) with the source and
      requirement changes they assess; CI validates only the committed
      evidence.

## Rules

- Never write or edit evidence JSON, determinations, rationales, or inputs
  yourself, and never hand that to another agent. Evidence means something only
  as an independent assessment that the harness records. If authoring cannot
  run, follow the matching Gotcha below; when the fix needs the user, such as
  missing credentials or configuration, stop and report the blocker.
- Never re-run an assessment to get a different result or to get an item read,
  or change a judgment to make validation pass. Fix the source, tests,
  references, or requirement text.
- Don't read evidence files into your context. Use `habeas status` and
  `habeas info`; to survey many rationales at once, ask a subagent to read them
  and return short findings.
- Review every change to an agreed requirement's text with the user first, as
  the habeas-requirements skill describes, even a rewording for precision.
- When the user agrees to reword a requirement for precision, whether a false
  rationale, a site review, or a disagreement prompted it, record the old text, new text,
  reason, and the rationale that prompted the change in the project's audit
  log, such as `audits/requirement-rewordings/`; if the project has none, ask
  the user before creating one. Escape quoted roles there, as in `\{cite}`, so
  the log adds no references.
- Validation is deterministic. When it tells you to run `habeas author`, run it
  yourself; never add `habeas author` to CI.

## Fixing findings

| Finding | Action |
| --- | --- |
| missing or stale evidence | `habeas author --all` |
| false judgment | read its rationale with `habeas info <name>`, fix the gap it names, then `habeas author --all` |
| uncovered `{impl}` or `{test}` line | check the reference names the right definition, then re-author that definition |
| uncovered Rust item | see "Uncovered Rust items" below |
| orphaned citation or site review | `habeas author --all` deletes it |
| orphaned requirement or example evidence | its definition was renamed or removed; `git rm` the file the finding names (removing an orphan is not editing evidence), then `habeas author --all` |
| untracked file with references | `git add` it |

A false rationale points to the gaps that skill's "What the review checks"
lists. Behavior with no requirement needs one agreed with the user. When a
rationale names code the reviewer found only by searching, add `{impl}` to the
item it names, which makes that item a root; don't restructure the code or add
`{cite}` to make it reachable. Each
determination stands alone: a false requirement does not fail the definitions it
implements, tests, or cites, so fix each failing definition on its own.

### Uncovered Rust items

An item is covered by its own reference or by a current record or review that
read it (habeas-requirements, "Gotchas"). For each item `habeas status` still
reports uncovered after `habeas author --all`:

1. If `habeas status` shows a coverage-loss review beside the item, act on
   each finding.
   `habeas author` runs this review, an independent assessment, when a record
   it replaces read the item and the new one does not.
   - `root`: add `{impl}` of that definition to the item.
   - `not needed`: before changing anything, determine from its rationale and
     the code whether the item is dead code or the earlier read was mistaken.
     Report dead code to the user instead of deleting it unasked; otherwise
     continue with step 2.
2. Otherwise, add references by the procedure in the habeas-requirements skill,
   citing `habeas:none` in fenced form with a justification only when nothing
   applies: often `{cite}`, when the item should be re-checked as the
   definition changes.

Whether an assessment reads an item varies between runs, so never re-run or
`--force` authoring just to get an item read; it would not stay covered.

### FIXME

A FIXME judgment, such as `{"FIXME": ["rust-lang/rust#123"]}`, means `{fixme}`
references to the definition acknowledge every remaining gap. Validation
accepts it like `true` when all other checks pass. When a gap is real but the
user agrees to defer it, add a fenced `{fixme}` reference to the definition
near the code or test it concerns, naming the issue that tracks it:

```text
// :::{fixme} requirements.cli.status
// :github: rust-lang/rust#123
// Selections with globs are not tested.
// :::
```

The reference makes the definition's evidence stale, so the next
`habeas author --all` reassesses it. A plain `FIXME` or `TODO` comment does not
count. Never invent issue numbers, and never add a `{fixme}` just to get past a
false determination. A `{fixme}` counts only while each issue it names is open: when one closes, `habeas status` lists it and treats the record as stale, and `habeas author --all` reassesses it, so fix the gap or name an open issue. An unnamed `:::{fixme}` with only `:github:` lines ties any place, including a requirement's text, to an issue: when that issue closes, `habeas status` reports the containing item or requirement as failing, so revisit it.

### Citation reviews

Every source item outside the book with a `{cite}` gets a citation review. It
checks that each citation is sensible and should not be `{impl}` or `{test}`, and
that each `habeas:none` justification holds. Fix a false review by
changing the references or the item.

### Site reviews

A distributed requirement (habeas-requirements,
references/distributed-requirements.md) gets a site review for each site,
authored by `habeas author --all` or, for one requirement,
`habeas author sites:<full-name>`. A site the requirement does not apply to is
judged true, so a false review means the reviewer found the requirement applies
and is broken:

1. If it does apply, fix the code at the site.
2. If the site should not be covered, narrow the site query or tighten the text
   describing sites; if the obligation's text admits a reading under which the
   site fails, tighten that text. Query and text changes edit the requirement;
   review them with the user.

Don't re-run a false site review for a different result.

If `habeas info <full-name>` reports many sites the requirement does not apply
to, the site query selects too much.

## Disagreeing assessments

The goal is that independent assessments of the same subject reach the same
determination, because the requirement, code, and tests leave no gap a careful
reviewer could find and no reading a careful reviewer could take differently.
Assessments that disagree are findings against that goal.

By default (`confirm` policy), `habeas author` checks each `true` or FIXME
result with a second independent assessment and a verifier; the recorded
rationale ends with a note on any disagreement, shown by `habeas info`. A
determination that changes on reassessment, though the subject and sources have
not changed in substance, is the same kind of finding. Validation accepts a recorded `true` or FIXME whose
rationale notes a disagreement, so look for these in the "Disagreements"
section of `habeas status` after each `habeas author` run.

1. Read both assessments: for a recorded disagreement, the rationale and its
   note on the other assessment (`habeas info`); for a determination that
   changed on reassessment, the new rationale and the previous one, which only
   Git has, such as by asking a subagent to summarize
   `git show HEAD:evidence/<full-name>.json`. Decide whether one assessment
   found a real gap or the requirement text admits more than one reading.
2. Fix the source or tests, or tighten the requirement text, reviewing
   substantive changes with the user. Don't re-run for a different result.
3. If you reword the requirement, record it in the audit log as the Rules say.

## Gotchas

- Evidence identifies each source by an outline node, such as `fn[run]` or
  `impl[Thing]/fn[run]`, and a structural hash of its code. Moving a node within its file without changing its selector, or editing
  comments the language does not hash, such as Rust `//` comments, keeps
  evidence current; moving it to another file or renaming it
  changes its selector and makes the evidence stale or invalid. Changing code, doc comments,
  or docstrings makes it stale, and so does adding, removing, or changing the
  role of an `{impl}`, `{test}`, or `{fixme}` reference, or editing its
  fenced commentary. Editing an item without a reference makes every record
  that read it stale, and the item stays uncovered until they are reassessed;
  make related edits to shared helpers together before authoring, and fix an
  item that is uncovered and whose readers are stale with `habeas author --all`.
- `habeas author --all` skips current records. After a fix the hash does not
  see, such as one made only in an ordinary comment, run
  `habeas author --force <full-name>`; never use `--force` to retry unchanged
  sources.
- If authoring fails on configuration, credentials, throttling, or a denied
  read, read [references/authoring-failures.md](references/authoring-failures.md).

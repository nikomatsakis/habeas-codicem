---
name: habeas-evidence
description: Use when Habeas evidence needs creating or refreshing, or when `cargo test` or `habeas status` reports Habeas findings such as missing, stale, or false evidence, uncovered `{impl}` or `{test}` lines, unreferenced Rust items, FIXME judgments, citation or site reviews, or disagreeing assessments, even if the user only says "the habeas test is failing". Covers running `habeas author` and fixing the code, tests, or references it flags. To design new or changed requirements before coding, use habeas-requirements; to derive an initial requirement set when most of a codebase is unreferenced, use habeas-bootstrap-requirements.
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
- [ ] 3. Fix what the findings name (see "Fixing findings").
- [ ] 4. `habeas author --all` assesses every definition whose evidence is
      missing, stale, or invalid, keeps current records, and reports how many it
      skipped as current; `-v` prints each false judgment's rationale.
      `habeas author <full-name>` does one.
- [ ] 5. `cargo test --workspace`. If findings remain that you can fix, return to
      step 2. Stop and report to the user each finding that needs their
      decision, such as a new or changed requirement, a gap to defer, or a
      blocker, and each false judgment expected because implementation is
      pending; don't keep iterating on those.

## Rules

- Never write or edit evidence JSON, determinations, rationales, or inputs
  yourself, and never hand that to another agent. Evidence means something only
  as an independent assessment that the harness records. If authoring cannot
  run, because configuration or credentials are missing, a read is denied, or the
  provider fails, stop and report the blocker.
- Never re-run an assessment to get a different result, or change a judgment to
  make validation pass. Fix the source, tests, references, or requirement text.
- Don't read evidence files into your context. Use `habeas status` and
  `habeas info`; to survey many rationales at once, ask a subagent to read them
  and return short findings.
- Review a change to an agreed requirement's text with the user first, as the
  habeas-requirements skill describes.
- Validation is deterministic. When it tells you to run `habeas author`, run it
  yourself; never add `habeas author` to CI.

## Fixing findings

| Finding | Action |
| --- | --- |
| missing or stale evidence | `habeas author --all` |
| false judgment | read its rationale with `habeas info <name>`, fix the gap it names, then `habeas author --all` |
| uncovered `{impl}` or `{test}` line | check the reference names the right definition, then re-author that definition |
| unreferenced Rust item | add references by the procedure in the habeas-requirements skill |
| orphaned citation or site review | the item or query changed; `habeas author --all` |
| untracked file with references | `git add` it |

A false rationale points to the gaps that skill's "What the review checks"
lists. Behavior with no requirement needs one agreed with the user. Each
determination stands alone: a false requirement does not fail the definitions it
implements, tests, or cites, so fix each failing definition on its own.

### FIXME

A FIXME judgment, such as `{"FIXME": ["#125"]}`, means a `FIXME` comment or test
acknowledges every remaining gap, conventionally `FIXME(#125)` or
`FIXME(rust-lang/rust#123)`; a `TODO` does not count. Validation accepts it like
`true` when all other checks pass. When a gap is real but the user agrees to
defer it, add a `FIXME(#N)` comment or test citing a real issue; comments
containing `FIXME` are hashed, so the evidence becomes stale and the next
`habeas author --all` reassesses it. Never invent issue numbers, and never add a
`FIXME` just to get past a false determination.

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
2. If the reviewer misread whether it applies, tighten the requirement text or
   narrow the site query so the site's status is unambiguous. Query and text
   changes edit the requirement; review them with the user.

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
not changed in substance, is the same kind of finding.

1. Read the rationale and its disagreement note, and decide whether one
   assessment found a real gap or the requirement text admits more than one
   reading.
2. Fix the source or tests, or tighten the requirement text, reviewing
   substantive changes with the user. Don't re-run for a different result.
3. When you reword a requirement for precision, record the old text, new text,
   reason, and the rationale that prompted the change in the project's audit
   log, such as `audits/requirement-rewordings/`; if the project has none, ask
   the user before creating one. Escape quoted roles there, as in `\{cite}`, so
   the log adds no references.

## Gotchas

- Evidence identifies each source by an outline node, such as `fn[run]` or
  `impl[Thing]/fn[run]`, and a structural hash of its code. Moving a node, or
  editing comments the language does not hash, such as Rust `//` comments
  without `FIXME`, keeps evidence current. Changing code, doc comments,
  docstrings, or `FIXME` comments makes it stale, and so does adding, removing,
  or changing the role of an `{impl}` or `{test}` reference, or editing its
  fenced commentary.
- If authoring fails on configuration, credentials, or throttling, read
  [references/configuration.md](references/configuration.md). If it reports a
  denied read, read [references/file-access.md](references/file-access.md).

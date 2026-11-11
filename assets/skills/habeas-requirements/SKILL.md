---
name: habeas-requirements
description: Use during design sessions and before changing code, tests, or requirement text in a project that tracks requirements with Habeas, even when the user only asks for a feature, bug fix, CLI flag, test, dependency bump, or wording tweak and never says "requirement". Drafts or edits requirements with the user (or confirms none change), has subagents review them, gets the user's agreement, then implements and adds `{impl}`/`{test}`/`{cite}` references. Not for moving code or requirements between files or outline nodes; use habeas-refactor. For validation findings, use habeas-evidence.
---

# Habeas requirements

Every end-user-observable result of the code, including errors and input
handling, must follow from a requirement the user agreed to. The habeas skill
has the directive and reference syntax.

## Workflow

- [ ] 1. Check the starting point. Run `git status` on the book's source
      directory. If it has uncommitted edits you did not make in this task, ask
      the user whether they belong to it. Read the relevant requirements, code,
      and tests; `habeas info <selection>` prints a requirement's text and state.
      If the change adds, alters, or removes no observable behavior (a comment typo, a
      dependency bump, a speed-up), or an existing requirement already specifies
      it (most bug fixes), tell the user which requirement covers it, or that
      none is needed, and go to step 6.
- [ ] 2. Draft. Write drafts from the request straight into the chapter as
      uncommitted, unstaged edits, with no draft marker; Git tracks what is in
      progress. For a new chapter, add it to `SUMMARY.md` and `git add -N` it so
      reviewers and Habeas see it. Ask the user only what the request can't
      answer; the review in step 3 finds the rest. Drafts may be high-level,
      compound, or incomplete. Cover each observable outcome and failure case
      of the change without committing to an implementation. Reuse existing
      requirements by full name, and propose a new one only for behavior none
      covers; one general requirement, such as "invalid arguments MUST be
      reported with usage", can cover many branches. Note the open product
      choices; reviewers get them in step 3.
- [ ] 3. Review. Sort the new and changed requirements into groups that one
      reviewer should see together: requirements that cite each other or
      describe one feature. For each group, start a read-only subagent, all in
      parallel. Tell it to read this skill's `references/review-prompt.md` and
      the habeas skill's `SKILL.md` by absolute path, and give it the group's
      full names and a brief: what the user asked for, the decisions already
      made, and the open choices already noted, which it should not re-raise.
      If you can't start subagents, review each group yourself against the
      prompt, one group at a time. Apply the revised text the reviews return,
      and reconcile reviews that clash, such as two that split the same
      requirement differently; a clash whose resolution changes what is
      promised becomes a design question.
- [ ] 4. Present each group to the user: its exact text, including any `{sites}`
      and `{violations}` queries, and the design questions the review returned,
      each with its options and your recommendation. Show text that was already
      agreed or that the user supplied as a diff against that text, then a
      "Mechanical:" list of the changes. Show the edits reviewers made outside
      their group the same way, and list added `{example}`s, which each need a
      test. Don't raise mechanical changes to new text as questions. Apply the
      user's decisions. If a decision adds an obligation no reviewer saw,
      review that group again.
      **Gate:** no implementation or test code, and no commit, until the user
      has agreed to, rejected, or deferred every group. Exact text the user supplied, or explicit agreement
      earlier in the conversation, counts; a general request to build the
      feature does not. If you cannot ask the user in this session, stop,
      leave the drafts uncommitted, and report them with the design
      questions.
- [ ] 5. Commit. Revert drafts the user rejected or deferred (`git restore -p`)
      and list them in your report. Check with `git diff` that the book holds
      exactly the agreed text, then commit it, all groups together, before any
      code; leave out unrelated edits found in step 1. For a change to agreed
      text, the message gives the reason and the rationale that prompted it,
      relative to the prior commit (habeas-evidence, "Rules"). This commit fails
      validation until evidence is authored; that is expected, as only the pull
      request as a whole must pass. Report the full names. In a design session
      with no code to write, stop here.
- [ ] 6. Implement and test. Add references as "Choosing references" says, then
      check the work against "What the review checks". When a requirement was
      split, move the `{impl}` and `{test}` of each split-off part to its new
      name.
- [ ] 7. Next, follow the habeas-evidence skill to author evidence and fix what
      it reports. When assessments disagree, follow its "Disagreeing
      assessments".

Every change to agreed text, including a rewording for precision or removing a
requirement, goes through steps 3 to 5 before it is committed. If
implementation shows that agreed text must change, stop work that depends on it and take the changed requirements
through steps 3 to 5 first. Behavior no requirement covers goes back to step 2.
Renaming or moving a committed requirement is a habeas-refactor task, agreed
with the user and committed before its text changes. Restructuring code in
place, including renaming an item, needs no requirement review, but references
move with the code and the evidence is reassessed; moving code to another file
or outline node is a habeas-refactor task.

## Choosing references

The reviewer starts from a requirement's `{impl}` and `{test}` items and follows
what they *reach*: what an item calls, the types it uses, and the trait methods
it calls, transitively. Callers don't count. An item *carries out* part of a
requirement when it makes a decision, performs a check, produces an output, or
handles a case the requirement specifies; a type does when the requirement
constrains its fields, variants, serialized form, or validation. A *root* is an
item that carries out part of a requirement and that no other `{impl}` item for
it reaches. Roots are per requirement: a helper reached from one requirement's
root may be a root of another.

For each requirement or example an item could reference (an item may carry
several):

1. A test designed to verify it gets `{test}`. Helpers the test calls need
   nothing; setup a test happens to exercise does not count.
2. Otherwise a root gets `{impl}`. Leave what it reaches unmarked.
3. Otherwise, code that should be re-checked when the requirement changes gets
   `{cite}`.
4. Otherwise, if the item is still uncovered after `habeas author` has run
   (see Gotchas), and it serves other code and carries out no requirement
   itself, it gets
   `` // {cite}`habeas:util` ``. That includes code a library calls, such as a
   trait implementation it requires.

Edge cases:

- Code that only checks consistency with a requirement, such as recognizing a
  form it defines to decide whether to run other behavior, gets `{cite}`. A
  dispatcher that only routes to the implementation gets `{cite}` of what it
  dispatches to, or nothing.
- A requirement that refines another's obligation can `{impl}` it.
- A site of a distributed requirement needs no reference to it, but a site that
  is a root of another requirement gets `{impl}` of that one.
- If a behavior follows only from several requirements together, mark each with
  `{impl}` where it applies and add fenced commentary saying how they combine.
  For example, `^C` quits because one requirement maps `^C` to `q` and another
  says q stops managed forwards; a reference alone would not say why `^C` is
  accepted.
- For internal code with an invariant worth checking, such as a cache that must
  not change results, propose a requirement stating the invariant and
  `{impl}` it. Don't propose grouping requirements that state no checkable
  obligation, such as one covering an area's internal helpers.
- An item that neither carries out a requirement nor serves other code needs a
  requirement or is not needed: report it to the user with a proposed
  requirement, or as dead code if nothing reaches it. Don't delete it unasked.
- Don't remove an existing `{impl}` from code a root reaches unless the user
  asks: the user may want it as a root, and removing it changes what the
  assessment reads.

`{cite}` needs no coverage and implements nothing; a citation review checks that
each one should not be `{impl}` or `{test}`. To defer a gap the user agrees to,
use `{fixme}` as the habeas-evidence skill's `references/fixme.md` describes.

## What the review checks

Before running `habeas author`, check the change against what each
requirement's assessment treats as a gap:

- an obligation the `{impl}` code does not implement completely and exactly as
  stated;
- code that carries out part of the requirement but is not reachable from its
  `{impl}` items, including code that only cites it, and a test that verifies it
  without `{test}`;
- observable behavior in `{impl}` code that no requirement implemented there
  specifies;
- obligations, boundaries, and failure cases no test runs and asserts. A test
  may be omitted only for code with no logic of its own, or code that needs
  network services, real time, or process-wide state no project test sets up;
  temporary files don't qualify, and an obligation that a value stays
  unchanged, distinct, or present always needs a test;
- an unexplained unmet SHOULD, and text with two readings that the code meets
  under only one.

An example's review checks that tests' setup and assertions cover its scenario
and each expected outcome. One acknowledged gap does not excuse others.

## Gotchas

- In Rust, every explicit function or method, including trait methods and test
  helpers, and every struct, enum, and union must be covered: by
  its own reference, or by a current assessment that read it or an enclosing
  item. A reference on an enclosing module or `impl` is not the item's own.
- Search results only locate code; they cover nothing.
- Reads exist only after `habeas author`, so author before treating an item as
  uncovered, and before giving it `habeas:util`. For an item still uncovered
  after that, see "Uncovered Rust items" in the habeas-evidence skill.

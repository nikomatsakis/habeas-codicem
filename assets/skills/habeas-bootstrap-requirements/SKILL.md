---
name: habeas-bootstrap-requirements
description: Use when adopting Habeas in an existing codebase, when validation reports unreferenced items across most of a project's code, or when the user asks for a requirements coverage audit. Inventories tests, Rust items, and user-visible behavior; proposes an initial requirement set for the user's approval; tags every test and item; and drives `habeas author --all` and validation to full coverage. For a single behavior change, use habeas-requirements; to refresh evidence or fix a few validation failures, use habeas-evidence.
---

# Bootstrap requirements

Habeas traces requirements, written in a project's mdBook, to the code and tests
that implement and verify them. Bootstrapping derives requirements from code
that already exists, so every requirement starts as a proposal: observed
behavior may be intended, a bug, or incidental.

Read the habeas-requirements skill first; it defines the directive syntax, the
reference procedure, and what the review checks. The goal is accurate
references, not a coverage number.

## Workflow

Steps 1 to 3 run once. Repeat steps 4 to 7 for each area, such as one command
or module, then do steps 8 and 9. In a large codebase, give each area's reading
to a subagent to keep it out of your context.

- [ ] 1. Check the setup. If the project has no `habeas.toml`, ask before
      running `habeas setup`. Confirm authoring works by running
      `habeas author <full-name>` on one definition; if it fails, see the
      habeas-evidence skill. Use one chapter file per area, such as
      `md/requirements/cli.md` for `requirements.cli.*`; add each to
      `SUMMARY.md` and `git add` it.
- [ ] 2. Read the existing requirements, documentation, public entry points,
      implementation, and tests. Follow behavior through the code rather than
      relying on test names or top-level dispatch.
- [ ] 3. Build the inventories and the gap list (below).
- [ ] 4. Propose requirements for the area (below), reusing existing ones by
      full name and proposing any changes to their text in the same review.
      Review each proposal with the user in turn, as the habeas-requirements
      skill describes. You may present an area's proposals together as a
      numbered list, each with its own concerns and suggested revisions, if the
      user decides on each; record every decision before moving on. A general
      request to bootstrap is not agreement to specific text.
      **Gate:** install a requirement only after the user agrees to it. Items
      the user has not decided stay on the gap list.
- [ ] 5. Install the agreed requirements in the area's chapter.
- [ ] 6. Tag the area's tests and items (below).
- [ ] 7. Run `cargo test --workspace`, which makes no model calls, and fix the
      reference and coverage errors it reports. Then run `habeas status`, tell
      the user how many definitions need authoring, and run
      `habeas author --all`; re-check one with `habeas author <full-name>`.
      Follow the habeas-evidence skill for the remaining findings until
      validation passes or every remaining finding is on the gap list with a
      reason.
- [ ] 8. Have a fresh subagent recount both inventories from the source and tests
      without your lists, and reconcile any differences.
- [ ] 9. Report (below).

## Inventories

Keep the inventories and the gap list in the conversation or in a file outside
the repository; a staged file listing names in role syntax would add
references.

- **Tests:** every first-party runnable test or test case, including integration
  tests and parameterized cases. Record the behavior each test checks; fill in
  requirement names once they are installed.
- **Items and behavior:** in Rust projects, every function, method (including
  trait methods and test helpers), struct, enum, and union. Validation's report
  of unreferenced Rust items, from `cargo test --workspace`, is the source of
  truth for this list. Add by hand the observable surface: public APIs,
  commands, handlers, UI actions, configuration, outputs, errors, and distinct
  observable branches. One tagged entry point does not cover deeper code.
- **Gap list:** each item or behavior you cannot yet map, with its location, the
  reason, and the open question.

Exclude third-party code. Exclude checked-in generated code by disallowing its
path under `[evidence]` in `habeas.toml`, with the user's agreement.

## Proposing requirements

Write one requirement per obligation a user could rely on; one requirement can
cover a command's branches when one sentence states them. For tests of internal
invariants, write a requirement for the invariant they check rather than
inventing a user-facing promise. Label each proposal and act on the label:

- **Intended:** state the observed behavior.
- **Possible bug:** ask the user what behavior is intended and write the
  requirement for that. Don't change the code during bootstrap unless the user
  asks; the resulting false judgment stays on the gap list, or gets a
  `FIXME(#N)` if the user defers it.
- **Incidental:** the user chooses between a requirement, often with MAY, and
  removing the behavior. For example, a command that also accepts an
  undocumented alias.

When a convention applies on its own at many sites, such as how every
validation function continues after a failed check, propose a distributed
requirement instead of marking each site `{impl}`; read the
habeas-requirements skill's
[references/distributed-requirements.md](../habeas-requirements/references/distributed-requirements.md).
Each site still needs its own reference to what it implements.

## Tagging

Tag each test with `{test}` for the definitions it is designed to verify, not
those it exercises incidentally. For a parameterized test, put the reference on
the case source or the generating declaration; don't let one file-level tag
stand in for unrelated tests.

Give every inventoried item its references by the procedure in the
habeas-requirements skill. Put `{impl}` only on code that makes the decision or
produces the output a requirement specifies, not on callers that pass the call
along. Use `habeas:none` only for an item that implements no observable
behavior; citation reviews check this, and many `habeas:none` citations in a
bootstrap suggest mis-tagging.

```rust
// {impl}`requirements.cli.unknown_command`
fn dispatch(args: &[String]) -> Result<()> { /* … */ }

#[test] // {test}`requirements.cli.unknown_command`
fn unknown_command_fails_with_usage() { /* … */ }
```

## Report

```text
Tests mapped: <mapped>/<total>
Items mapped: <mapped>/<total>  (habeas:none: <count>, from `habeas status --all`)
Independent recount: <agrees | differences and how reconciled>
Gaps:
- <item or behavior> (<file>:<line>): <reason>; <open question>
```

Claim 100% only when both inventories have no unmapped items, the independent
recount agrees, and every observable branch has been reviewed. Don't silently
exclude difficult cases.

## Gotchas

- Never write evidence yourself or ask another agent to, even over a long
  session; only `habeas author` produces it.
- `git add` new files, including new chapter files: references in untracked
  files don't count.

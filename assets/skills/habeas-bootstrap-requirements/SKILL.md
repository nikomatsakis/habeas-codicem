---
name: habeas-bootstrap-requirements
description: Use when adopting Habeas in an existing codebase, when validation reports unreferenced items across most of a project's code, or when the user asks for a requirements coverage audit. Inventories tests, Rust items, and user-visible behavior; proposes an initial requirement set for the user's approval; tags every test and item; and drives `habeas author --all` and validation to full coverage. For a single behavior change, use habeas-requirements; to refresh evidence or fix a few validation failures, use habeas-evidence.
---

# Bootstrap requirements

Habeas traces requirements, written in a project's mdBook, to the code and tests
that implement and verify them. Bootstrapping derives requirements from code
that already exists, so every requirement starts as a proposal: observed
behavior may be intended, a bug, or incidental.

## Workflow

Steps 1 to 3 run once. Repeat steps 4 to 7 for each area, such as one command
or module, then do steps 8 and 9. In a large codebase, give each area's reading
to a subagent to keep it out of your context.

- [ ] 1. Read the habeas-requirements skill; it defines the directive syntax,
      the reference procedure, and the review criteria. Aim for accurate
      references, not a coverage number. If the project has no
      `tests/habeas.rs` calling `habeas::validate()`, Habeas is not set up: ask
      before running `habeas setup`. If a requirement already exists, confirm
      authoring works with `habeas author --stdout <full-name>`, which always
      assesses and writes no evidence. If it fails, see the habeas-evidence
      skill.
- [ ] 2. Read the existing requirements, documentation, public entry points,
      implementation, and tests. Follow behavior through the code rather than
      relying on test names or top-level dispatch.
- [ ] 3. Build the inventories and the gap list (below).
- [ ] 4. Propose requirements for the area (below), reusing existing ones by
      full name and proposing any changes to their text. Review them with the
      user in turn as habeas-requirements step 3 describes; a general request to
      bootstrap is not agreement to specific text.
      **Gate:** drafts may be uncommitted edits to the chapter, as in
      habeas-requirements step 2, but don't commit a requirement or tag code
      against it until the user agrees to its text. Remove undecided drafts
      from the chapter and keep them on the gap list.
- [ ] 5. Install the agreed requirements in the area's chapter, one file per
      area, such as `md/requirements/cli.md` for `requirements.cli.*`; add a
      new chapter to `SUMMARY.md` and `git add` it.
- [ ] 6. Tag the area's tests and items (below). In the first area, if step 1
      had no requirement to try, run `habeas author <full-name>` on one of the
      area's requirements to confirm authoring works before going on.
- [ ] 7. Run `cargo test --workspace`, which makes no model calls, and fix the
      reference and coverage errors in this area's files. Unreferenced items in
      areas not yet bootstrapped wait for their own pass; never give them
      placeholder references. Tell the user how many definitions
      `habeas status` says need authoring, then follow the habeas-evidence
      workflow for this area's definitions until each passes or is on the gap
      list with a reason. Full validation can pass only after the last area.
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
- **Items and behavior:** in Rust projects, every item `cargo test --workspace`
  reports as unreferenced. Add by hand the observable surface: public APIs,
  commands, handlers, UI actions, configuration, outputs, errors, and distinct
  observable branches. One tagged entry point does not cover deeper code.
- **Gap list:** each item or behavior you cannot yet map, with its location, the
  reason, and the open question.

Exclude third-party and checked-in generated code. Rust item coverage already
skips the project-root `vendor` directory; for any other such path, add it to
`[evidence] disallow` in `habeas.toml` with the user's agreement, which also
hides it from authoring.

## Proposing requirements

Write one requirement per obligation a user could rely on; one requirement can
cover a command's branches when one sentence states them. For tests of internal
invariants, write a requirement for the invariant they check rather than
inventing a user-facing promise. Label each proposal and act on the label:

- **Intended:** state the observed behavior.
- **Possible bug:** ask the user what behavior is intended and write the
  requirement for that. Don't change the code during bootstrap unless the user
  asks; the resulting false judgment stays on the gap list, or gets a
  a
  `{fixme}` reference if the user defers it (see habeas-evidence).
- **Incidental:** the user chooses between a requirement, often with MAY, and
  removing the behavior. For example, a command that also accepts an
  undocumented alias.

When a convention applies on its own at many sites, such as how every
validation function continues after a failed check, propose a distributed
requirement instead of marking each site `{impl}`; read the
habeas-requirements skill's
[references/distributed-requirements.md](../habeas-requirements/references/distributed-requirements.md).

## Tagging

Give every inventoried test and item its references by the procedure in the
habeas-requirements skill. Every test must end up with a `{test}` reference to a requirement or example,
attached to the test as the habeas-requirements skill describes; a test whose behavior no agreed requirement states goes on the gap
list, never `habeas:none`. For a parameterized test, put the reference on the
case source or the generating declaration; one file-level tag must not stand in
for unrelated tests. Many `habeas:none` citations on other items suggest
mis-tagging.

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
  session (habeas-evidence explains why).
- `git add` new chapter and source files; references in untracked files don't
  count.

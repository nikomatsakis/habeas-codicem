---
name: habeas
description: Use at the start of any work in a project that tracks requirements with Habeas (a `habeas` dev-dependency, a `habeas.toml`, `{req}` directives in its mdBook, or `{impl}`/`{test}`/`{cite}` references in code) to pick the task skill (habeas-requirements for design and any change, habeas-refactor for moves, habeas-evidence for validation findings, habeas-bootstrap-requirements for adoption). Also the reference for directive and reference syntax, `habeas status` and `habeas info`, and gotchas common to every Habeas task.
---

# Habeas

Habeas traces requirements, written in a project's mdBook, to the code and tests
that implement and verify them. `habeas author` has a model assess each
requirement and records the result as evidence under `evidence/`;
`cargo test --workspace` validates that evidence without calling a model.

## Workflow

- [ ] 1. Pick the skill for the task:

      | Task | Skill |
      | --- | --- |
      | A design session, or any change to behavior, code, tests, or requirement text, including renaming code in place | habeas-requirements |
      | Moving code, tests, or requirements to another file, chapter, or outline node, or renaming a requirement, without changing them | habeas-refactor |
      | Creating or refreshing evidence; `cargo test` or `habeas status` reports Habeas findings | habeas-evidence |
      | Adopting Habeas in an existing codebase, or most of the code has no references | habeas-bootstrap-requirements |

      If Habeas is not configured yet (no `tests/habeas.rs` calling
      `habeas::validate()`), ask the user before running `habeas setup`; if
      setup is not completed, do no Habeas requirements work in the project.
      A new project with little code starts with habeas-requirements, not
      habeas-bootstrap-requirements, even though most of its code has no
      references.

- [ ] 2. Follow that skill; each one names the next. A change that both moves
      and edits code, tests, or requirements is the move first
      (habeas-refactor), then the edit under the skill the table gives for it.

Run commands from the project root, the directory with `book.toml` and
`Cargo.toml`.

## Inspecting

- `habeas status [<selection>]` lists definitions needing attention.
- `habeas info <selection>` prints requirement and example text, evidence state,
  rationale, inputs, and validation findings.

A selection is a full name or a glob in which `*` matches any characters,
including dots: `habeas info 'requirements.cli.*'`. Don't read `evidence/` files
into your context; use these commands.

## Requirements, examples, and definitions

Requirements live in the book's source directory (`[book].src` in `book.toml`,
`src` by default). A requirement is a fenced `{req}` directive. Its full name is
the chapter path relative to that directory, without `.md`, with `/` changed to
`.`, then the directive name. With `[book] src = "md"`, in `md/requirements.md`:

```text
:::{req} forwarding.quit
Pressing q MUST stop managed forwards.
:::
```

defines `requirements.forwarding.quit`. State obligations with BCP 14 terms
(MUST, SHOULD, MAY). Everything between the fences is the body.

An `{example}` is a concrete case, named and fenced the same way. Its body must
reference a requirement it illustrates: `{test}` when that requirement's review
should check the case, `{cite}` when the requirement is context for the
example's own review. A test must reference the example with `{test}` for the
example to be judged true.

```text
:::{example} forwarding.quit_two
With two forwards running, pressing q stops both. {test}`requirements.forwarding.quit`
:::
```

A `{definition}` defines shared vocabulary: `:::{definition} name`, one
`:phrase: display text` option, and an explanatory body. Refer to it with
`` {term}`full.name` ``. It has no evidence and is never the target of `{impl}`,
`{test}`, or `{cite}`. Names are unique across all three kinds.

For an obligation that applies separately at each of many sites, such as a
convention every validation function follows, read
[references/distributed-requirements.md](references/distributed-requirements.md)
before drafting it, and whenever you edit a `{req}` that contains `{sites}` or
`{violations}`.

## References

Write a reference with a full name in a comment on the relevant source line:
`` {impl}`requirements.forwarding.quit` ``, `` {test}`…` ``, or `` {cite}`…` ``.
`{impl}` marks code that carries out a requirement, `{test}` a test that
verifies it, and `{cite}` related code to re-check when it changes; the
habeas-requirements skill says which to use. `{fixme}` acknowledges a deferred
gap (the habeas-evidence skill's `references/fixme.md`).

Fence a reference to attach commentary, keeping both fences in one run of
consecutive comment lines:

```text
// :::{impl} requirements.forwarding.quit
// Stops forwards in creation order, so later ones never outlive earlier ones.
// :::
```

If a file that implements or tests a requirement cannot carry a comment, such as
a shipped skill or a stylesheet copied into other projects, or a requirement
lives in a nested chapter, read
[references/syntax-details.md](references/syntax-details.md).

## Gotchas

- A directive whose name has a segment (between dots) with characters other
  than ASCII letters, digits, `_`, and `-` stays literal text and silently
  defines nothing.
- A new chapter renders only once it is in `SUMMARY.md`.
- A `{req}` whose body contains a nested colon directive, including a fenced
  reference, needs a longer outer fence such as `::::{req}`; otherwise the inner
  `:::` closes it.
- Reference discovery and authoring read only files in the Git index. `git add`
  every new source file; for a new chapter that is still a draft, `git add -N`
  records it without staging its content.
- A reference inside a string literal does not count, so test fixtures can hold
  role text.
- Moving or renaming a requirement changes its full name, which evidence,
  references, and links use. Agree it with the user, then use habeas-refactor.
- Editing a vocabulary definition makes the evidence of everything that uses it
  stale.
- When assessments of the same subject disagree, follow "Disagreeing
  assessments" in the habeas-evidence skill.
- Never write or edit evidence yourself, or ask another agent to: evidence means
  something only as an independent assessment (habeas-evidence, "Rules").

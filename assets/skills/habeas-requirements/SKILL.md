---
name: habeas-requirements
description: Use before adding, changing, or moving code, tests, or requirements in a project that tracks requirements with Habeas (a `habeas.toml`, `{req}` directives in its mdBook, or `{impl}`/`{test}`/`{cite}` references in code), even when the user only asks for a feature, bug fix, refactor, or CLI change and never says "requirement". Covers agreeing requirements and examples with the user, writing `{req}`, `{example}`, and `{definition}` directives, and choosing `{impl}`, `{test}`, `{cite}`, or `habeas:none` references. To run the authoring harness or fix evidence and validation failures, use habeas-evidence; to build an initial requirement set for an existing codebase, use habeas-bootstrap-requirements.
---

# Habeas requirements

Habeas traces requirements, written in a project's mdBook, to the code and tests
that implement and verify them. `habeas author` has a model assess each one;
`cargo test --workspace` checks the results.

## Workflow

- [ ] 1. Read the relevant requirements, code, and tests. `habeas info <selection>`
      prints requirement and example texts, evidence states, rationales, and
      validation findings; a selection is a full name or a glob in which `*`
      matches any characters, including dots, as in `habeas info 'requirements.cli.*'`.
- [ ] 2. Draft: list the existing requirements to reuse by full name, and the
      smallest set of new or changed obligations, with their chapter, full
      names, and exact fenced text. Write the drafts into the chapter as
      uncommitted edits and quote them in the conversation. Cover important
      outcomes and failure cases without committing to an implementation. List
      open product choices separately.
- [ ] 3. Review each new or changed requirement with the user in turn: present
      its exact text, and for a distributed requirement its queries; point out
      ambiguity, obligations that are hard to test or that nothing would
      enforce, and conflicts with other requirements; propose specific
      improvements to accept, revise, or reject; apply the decisions before
      moving on. You may present several at once, each with its own
      recommendations, if the user decides on each.
      **Gate:** no implementation or test code, and no commit of the drafts, until
      the user agrees. Exact text the user supplied, or explicit agreement earlier
      in the conversation, counts; a general request to build the feature does not.
      If you cannot ask the user in this session, stop here and report the drafts
      and open choices.
- [ ] 4. Make sure the book contains exactly the approved text and report the
      full names.
- [ ] 5. Implement, add tests, and add references (below).
- [ ] 6. Follow the habeas-evidence workflow to author evidence and fix the
      gaps it reports.

If implementation shows that an approved obligation must change substantively,
review the revised text with the user before coding the change. Log a precision
rewording of an agreed requirement as the habeas-evidence Rules say. A bug fix whose
behavior an existing requirement already specifies needs no new requirement:
confirm the requirement covers it, then fix the code and check its tests and
references. A refactor that changes no behavior needs no review, but references
must move with the code.

## Rules

- Never write evidence JSON, rationales, determinations, or inputs yourself, and
  never ask another agent to; habeas-evidence explains why.
- Every end-user-observable result, including errors and input handling, must
  follow from some agreed requirement. One general requirement, such as "invalid
  arguments MUST be reported with usage", can cover many branches; propose a new
  requirement only for behavior no existing one covers, and check that the code
  adds no observable behavior beyond the requirements.
- If a behavior follows only from several requirements together, mark each with
  `{impl}` where it applies and add fenced commentary explaining how they
  combine. For example, `^C` quits because one requirement maps `^C` to `q` and
  another says pressing q stops managed forwards; a reference alone would not
  say why `^C` is accepted.
- Keep full names stable once agreed: evidence, references, and links use them.
  Moving a requirement to another chapter or renaming it changes its full name:
  agree it with the user, update every reference, link, and anchor to it, and
  handle its old evidence as the orphaned-evidence row in habeas-evidence's
  "Fixing findings" says.

## Writing requirements

Put requirements in the book's Markdown source directory: `[book].src` in
`book.toml`, `src` by default. A requirement is a fenced `{req}` directive: an
opening line of three or more colons followed by `{req} name`, the body, and a
closing line of at least as many colons. Its full name is the chapter path
relative to that directory, without `.md`, with `/` changed to `.`, followed by
the directive name. With `[book] src = "md"`, in `md/requirements.md`:

```text
:::{req} forwarding.quit
Pressing q MUST stop managed forwards.
:::
```

defines `requirements.forwarding.quit`. Name segments use only ASCII letters,
digits, `_`, and `-`; a directive with an invalid name stays literal text and
silently defines nothing. Add a new chapter to `SUMMARY.md` so it renders.
Everything between the fences is the body, including headings and diagrams.
State obligations with BCP 14 terms (MUST, SHOULD, MAY). Give the outer directive a longer fence, such as `::::{req}`, when
its body contains a nested colon directive, including a fenced reference;
otherwise the inner closing `:::` ends the directive.

Keep each requirement as small as it can be while still standing on its own: one
obligation, or a few that only make sense together, that a reviewer can judge
from its text and the requirements it cites. Split a requirement whose parts
could change, be implemented, or fail independently, and move shared detail,
such as a template or a format, into its own requirement that the others cite.
A small requirement costs less to assess, and editing it makes only its own
evidence stale. Draft new requirements this way; when you change an existing
requirement that is larger, propose splitting it as part of the change.

Write a concrete case as an `{example}` directive, named and fenced the same
way. Its body must reference at least one requirement it illustrates: `{test}`
when the requirement's review should check the case, `{cite}` when the
requirement is context for the example's own review, or both. Tests must
reference the example with `{test}` for it to be judged true.

```text
:::{example} forwarding.quit_two
With two forwards running, pressing q stops both. {test}`requirements.forwarding.quit`
:::
```

Define shared vocabulary with `:::{definition} name`, one nonempty
`:phrase: display text` option, and an explanatory body; refer to it with the
`{term}` role, which renders the phrase as a plain link. A vocabulary definition
has no evidence of its own and is never the target of `{impl}`, `{test}`, or
`{cite}`. Names are
unique across requirements, examples, and vocabulary definitions.

If an obligation applies separately at each of many sites, such as a convention
every validation function follows, or you edit a `{req}` that contains `{sites}`
or `{violations}`, read
[references/distributed-requirements.md](references/distributed-requirements.md)
before drafting.

## Choosing references

Write each reference with a full name, in a comment on the relevant source line
or on the book line: `` {impl}`requirements.forwarding.quit` ``, `` {test}`…` ``,
or `` {cite}`…` ``. Choose
each item's references by this procedure, applied to every requirement or
example it could reference; an item may carry several:

1. If the item is a test or test helper designed to verify the requirement or
   example, use `{test}`. Incidental setup that a test happens to exercise does
   not count; a test that verifies one without `{test}` is a gap.
2. Otherwise, if fulfilling it depends on the item, use `{impl}`. Code qualifies
   when it makes a decision, performs a check, produces an output, or handles a
   case the requirement specifies; a type qualifies when the requirement
   constrains its fields, variants, serialized form, or validation. Code that only checks it is consistent with
   a requirement, such as recognizing a form the requirement defines to decide
   whether to run other behavior, gets `{cite}`, not `{impl}`. Calling code
   that implements it does not count, so a dispatcher that only routes to the
   implementation gets `{cite}` of what it dispatches to, or `habeas:none`. A
   requirement that refines another's obligation can `{impl}` it. A site of a
   distributed requirement needs no reference to that requirement; its site
   review covers it, but it still needs references for anything else it
   implements.
3. Otherwise, if the item should be re-checked or kept in sync when the
   requirement or example changes, use `{cite}`.
4. If nothing gives the item a reference, cite the built-in `habeas:none` in
   fenced form, with a body justifying why the item exists:

   ```text
   // :::{cite} habeas:none
   // Joins the outline builders' line splitting; no requirement concerns how.
   // :::
   ```

For internal code with an invariant worth checking, such as a cache that must
not change results, propose a requirement stating the invariant and implement it
with `{impl}`. Don't propose grouping requirements that state no checkable
obligation, such as one covering an area's internal helpers. An item citing
`habeas:none` must not implement observable behavior that no requirement
specifies.

`{impl}` and `{test}` make the item an input to the assessment, and its evidence
must cover them. `{cite}` needs no coverage and does not count as implementing
anything; citation reviews (habeas-evidence) check it should not be `{impl}` or
`{test}`.

Any reference may be fenced to attach commentary: `:::{impl} name`, the
commentary, and a closing `:::`. In a source comment, keep both fences in one run
of consecutive comment lines.

```text
// :::{impl} requirements.forwarding.quit
// Stops forwards in creation order, so later ones never outlive earlier ones.
// :::
```

To acknowledge a gap the user agrees to defer, use a `{fixme}` reference, as
the habeas-evidence skill describes. If a file that implements or tests a
requirement cannot carry a reference, such as a shipped skill or a stylesheet
copied into other projects, or a name, reference, or vocabulary edit behaves
unexpectedly, read [references/syntax-details.md](references/syntax-details.md).

## What the review checks

The authoring harness judges each requirement `true`, `false`, or FIXME, an
acknowledged, deferred gap (see "FIXME" in the habeas-evidence skill). A
requirement's review treats these as gaps:

- an obligation the code marked `{impl}` does not implement completely and
  exactly as stated;
- code that implements part of the requirement without `{impl}`, including code
  that only cites it, and a test that verifies it without `{test}`;
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
and each expected outcome. One acknowledged gap does not excuse others. When
assessments of the same subject disagree, follow "Disagreeing assessments" in
the habeas-evidence skill.

## Gotchas

- In Rust, every explicit function or method, including trait methods and test
  helpers, and every struct, enum, and union needs its own reference in an
  attached comment or its body. A reference on an enclosing module or `impl`
  does not cover it, and there is no waiver.

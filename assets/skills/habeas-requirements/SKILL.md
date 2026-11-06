---
name: habeas-requirements
description: Design and vet requirements and examples for user-visible behavior changes in a Habeas mdBook project before coding; maintain definitions and references.
---

# Habeas requirements

## New or changed behavior

When a user requests new or changed user-visible behavior in a Habeas project, first read the relevant requirements, code, and tests. Identify requirements that already express the intended behavior. For gaps, draft the smallest set of observable obligations, including important outcomes and failure cases, without committing to an implementation.

Run `habeas info <selection>` to read current requirement and named example texts, evidence states, rationales, inputs, and validation findings as Markdown. A selection is a full name, which selects exactly, or a glob where `*` matches any number of characters, including dots, when you need to inspect a group.

Show the user the full IDs and text of existing requirements to reuse, plus the proposed chapter location, full IDs, and exact fenced text of any additions or changes. State open product choices separately. Ask the user to agree to or revise this requirement set. User-provided exact requirement text or explicit agreement earlier in the conversation counts as approval; a general request to build the feature does not. Keep drafts out of the mdBook while review is pending, and do not write implementation or test code yet.

After agreement, install the approved requirements in the mdBook and confirm their names. Then implement the behavior and add tests and traceability references. Run `habeas author --all` to create or refresh evidence through the configured model. The supervising agent must not compose evidence JSON, rationales, determinations, or source snapshots itself or delegate manual evidence writing to another agent. If implementation reveals a substantive change to an approved obligation, review and install the revised requirement before coding that change. Missing or false evidence is expected while implementation is incomplete.

## Reviewing code against requirements

The authoring harness assesses whether the code marked `{impl}` for a requirement implements every obligation completely and exactly as stated, and whether tests cover interesting design cases. It also treats as gaps any code that implements part of the requirement without `{impl}`, including code that only cites it, and any behavior in `{impl}` code that no definition implemented there specifies. When another requirement implements part of the subject, it checks only that the text carries out that part. For absent tests, it assesses whether simplicity or impracticality justifies their absence. For an example, it assesses whether test setup and assertions cover the scenario and expected outcomes. Use these criteria while implementing and fixing reported gaps; leave evidence judgments and rationales to the harness.

Requirement and example judgments have three outcomes: `true`, `false`, or `{"FIXME": ["#125"]}`. The harness uses FIXME when source comments or tests explicitly acknowledge the uncovered aspects, recording their issue references or other identifying content. A documented gap does not excuse other, unacknowledged gaps. Use the `habeas-evidence` skill for the authoring workflow.

Inspect every end-user-observable result and branch, including errors and input handling. Check that the code fulfills each applicable requirement and introduces no additional observable behavior without an agreed requirement. When requirements interact, mark every requirement implemented at the relevant point with `{impl}` and explain how their texts jointly account for the behavior. A reference alone does not explain why a behavior, such as accepting `^C`, is specified.

Every explicit function or method in first-party Rust sources needs its own requirement or example reference in an attached Rust comment or within its body, including trait methods and test helpers. A reference on an enclosing module or `impl` does not cover its functions, and there is no uncited-function waiver. If no existing requirement explains an internal helper, propose a requirement scoped to that area of internal code, such as `internal.config_parsing`, rather than one catch-all, get the user's agreement, and mark the helper with `{impl}`. That requirement's review checks that it explains each implementing function. Do not use an internal-detail requirement to justify otherwise unspecified observable behavior.

## Authoring format

Put requirements in the book's Markdown source directory. Define each with a fenced `{req}` directive: the opening line is three or more colons followed by `{req} name`, and the closing line has at least as many colons. Derive the full name from the chapter path relative to that directory: remove `.md`, change `/` to `.`, and append the directive name. For nested chapters, omit a leading `reqs/` and a trailing `/index` or `/README`.

```text
:::{req} forwarding.quit
Pressing q MUST stop managed forwards.
:::
```

Everything between the fences is the requirement body, including headings and diagrams. Use a longer outer colon fence when the body contains a nested colon directive. Use BCP 14 terms when stating obligations.

Write a concrete case as a separate `{example}` directive with the same chapter-qualified naming and fence rules. Reference at least one requirement it illustrates from the example body: `{test}` when the example is a case the requirement's review should check, `{cite}` when the requirement is context for the example's own review, or both. A true example determination needs test references and an evidence rationale explaining how the tests cover its interesting aspects. Keep names unique across requirements and examples.

Use fully qualified names for implementation, citation, and test references. Place them on the relevant source or book line:

```text
{impl}`requirements.forwarding.quit`
{cite}`requirements.forwarding.quit`
{test}`requirements.forwarding.quit`
```

Choose the role by what the source does:

- `{impl}`: it implements part of the definition, making a decision, performing a check, producing an output, or handling a case the definition specifies. Calling code that implements the definition, or producing a result a caller uses to meet it, does not count. A requirement that refines another's obligation can `{impl}` it. A convention for every output of a kind, such as every validation error, is implemented by the code that carries it out as a whole.
- `{test}`: it is designed to verify behavior the definition specifies, through a test's scenario and assertions or an example's concrete case. Incidental setup does not count.
- `{cite}`: it relies on, supports, or refers to the definition without implementing or testing it.

`{impl}` and `{test}` make the source an input to the definition's assessment and must be covered by its evidence. `{cite}` makes the cited definition context for the citing requirement or example, needs no coverage, and does not count toward the cited requirement's implementation.

When a file that implements or tests a requirement cannot or should not carry a reference itself, such as a shipped skill, a stylesheet copied into other projects, or a README, anchor it from the declaration with `:impl: <path>` or `:test: <path>` option lines directly after the opening fence (examples accept only `:test:`). An anchor covers the whole file, or with `:impl: <path>#<selector>` one outline node of it, such as `src/lib.rs#fn[run]` or a Markdown `section[Heading]`. Do not anchor a file when a more precise reference is available, such as a comment attached to the Rust item that implements or tests the requirement.

When a requirement or example changes, run `habeas author --all`, then the Cargo tests. Address reported implementation, test, or reference gaps and rerun the harness. Validation itself checks stored evidence without inference.

## Shared vocabulary

Use `:::{definition} name` with one nonempty `:phrase: display text` option before
its explanatory body to define shared vocabulary. Reference its chapter-qualified
name with the `{term}` role; the book renders the phrase as a plain link, without
a badge. Names are unique across requirements, examples, and definitions. A
named definition has no evidence record of its own and cannot substitute for a
requirement citation. Dependent evidence snapshots its complete declaration;
review changes to phrases and bodies because they invalidate that evidence.

---
name: habeas-requirements
description: Design and vet requirements and examples for user-visible behavior changes in a Habeas mdBook project before coding; maintain definitions and references.
---

# Habeas requirements

## New or changed behavior

When a user requests new or changed user-visible behavior in a Habeas project, first read the relevant requirements, code, and tests. Identify requirements that already express the intended behavior. For gaps, draft the smallest set of observable obligations, including important outcomes and failure cases, without committing to an implementation.

Run `habeas info <full-requirement-name>` to read a requirement's current text, evidence state, rationale, inputs, and validation findings as Markdown. The selector also accepts a regular expression over full requirement names when you need to inspect a group.

Show the user the full IDs and text of existing requirements to reuse, plus the proposed chapter location, full IDs, and exact fenced text of any additions or changes. State open product choices separately. Ask the user to agree to or revise this requirement set. User-provided exact requirement text or explicit agreement earlier in the conversation counts as approval; a general request to build the feature does not. Keep drafts out of the mdBook while review is pending, and do not write implementation or test code yet.

After agreement, install the approved requirements in the mdBook and confirm their names. Then implement the behavior, add tests and traceability references, and maintain evidence. If implementation reveals a substantive change to an approved obligation, review and install the revised requirement before coding that change. Use the evidence skill for pending determinations while implementation is incomplete.

## Reviewing code against requirements

Inspect every end-user-observable result and branch, including errors and input handling. Check that the code fulfills each applicable requirement and introduces no additional observable behavior without an agreed requirement. When requirements interact, cite every applicable requirement at the relevant implementation point and explain how their texts jointly account for the behavior. A citation alone does not explain why a behavior, such as accepting `^C`, is specified.

Every explicit function or method in first-party Rust sources needs its own requirement or example reference in an attached Rust comment or within its body, including trait methods and test helpers. Each example cites a requirement. A reference on an enclosing module or `impl` does not cover its functions, and there is no uncited-function waiver. If no existing requirement explains an internal helper, propose a project requirement such as `internal-detail` for the user's agreement, cite it on the function, and record evidence explaining why each such function adds no end-user-observable behavior beyond the requirements it supports. Do not use an internal-detail requirement to justify otherwise unspecified observable behavior.

## Authoring format

Put requirements in the book's Markdown source directory. Define each with a fenced `{req}` directive: the opening line is three or more colons followed by `{req} name`, and the closing line has at least as many colons. Derive the full name from the chapter path relative to that directory: remove `.md`, change `/` to `.`, and append the directive name. For nested chapters, omit a leading `reqs/` and a trailing `/index` or `/README`.

```text
:::{req} forwarding.quit
Pressing q MUST stop managed forwards.
:::
```

Everything between the fences is the requirement body, including headings and diagrams. Use a longer outer colon fence when the body contains a nested colon directive. Use BCP 14 terms when stating obligations.

Write a concrete case as a separate `{example}` directive with the same chapter-qualified naming and fence rules. Cite at least one requirement it illustrates from the example body. A true example determination needs test references and an evidence rationale explaining how the tests cover its interesting aspects. Keep names unique across requirements and examples.

Use fully qualified names for implementation, citation, and test references. Place them on the relevant source or book line:

```text
{impl}`requirements.forwarding.quit`
{cite}`requirements.forwarding.quit`
{test}`requirements.forwarding.quit`
```

When a requirement or example changes, update its `evidence/<full-name>.json` record. Run the Cargo tests to see all Habeas validation findings.

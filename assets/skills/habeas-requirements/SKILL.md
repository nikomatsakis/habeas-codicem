---
name: habeas-requirements
description: Design and vet requirements for new features or user-visible behavior changes in a Habeas mdBook project before coding; also maintain requirements and references.
---

# Habeas requirements

## New or changed behavior

When a user requests new or changed user-visible behavior in a Habeas project, first read the relevant requirements, code, and tests. Identify requirements that already express the intended behavior. For gaps, draft the smallest set of observable obligations, including important outcomes and failure cases, without committing to an implementation.

Show the user the full IDs and text of existing requirements to reuse, plus the proposed chapter location, full IDs, and exact fenced text of any additions or changes. State open product choices separately. Ask the user to agree to or revise this requirement set. User-provided exact requirement text or explicit agreement earlier in the conversation counts as approval; a general request to build the feature does not. Keep drafts out of the mdBook while review is pending, and do not write implementation or test code yet.

After agreement, install the approved requirements in the mdBook and confirm their names. Then implement the behavior, add tests and traceability references, and maintain evidence. If implementation reveals a substantive change to an approved obligation, review and install the revised requirement before coding that change. Use the evidence skill for pending determinations while implementation is incomplete.

## Authoring format

Put requirements in the book's Markdown source directory. Define each with a fenced `{req}` directive: the opening line is three or more colons followed by `{req} name`, and the closing line has at least as many colons. Derive the full name from the chapter path relative to that directory: remove `.md`, change `/` to `.`, and append the directive name. For nested chapters, omit a leading `reqs/` and a trailing `/index` or `/README`.

```text
:::{req} forwarding.quit
Pressing q MUST stop managed forwards.
:::
```

Everything between the fences is the requirement body, including headings, examples, and diagrams. Use a longer outer colon fence when the body contains a nested colon directive. Use BCP 14 terms when stating obligations.

Use fully qualified names for implementation, citation, and test references. Place them on the relevant source or book line:

```text
{impl}`requirements.forwarding.quit`
{cite}`requirements.forwarding.quit`
{test}`requirements.forwarding.quit`
```

When a requirement changes, update its `evidence/<full-name>.json` record. Run the Cargo tests to see all Habeas validation findings.

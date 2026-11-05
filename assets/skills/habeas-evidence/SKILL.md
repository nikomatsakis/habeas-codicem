---
name: habeas-evidence
description: Use the Habeas authoring harness to create or refresh requirement evidence and Rust function attestations, and address Cargo validation failures.
---

# Habeas evidence

Run the authoring harness from the Cargo and mdBook project:

```sh
habeas author --all
```

It fills in missing, stale, invalid, or false evidence for requirements, named
examples, and first-party Rust functions. Current accepted records, including
FIXME judgments, are preserved. Each assessment runs in its own model conversation.
The harness records delivered source inputs and writes the model's judgment and
rationale. The supervising agent must not compose or refresh evidence JSON,
determinations, rationales, or source snapshots itself, or delegate that work to
an ad hoc evidence-writing agent.

In a Git worktree, stage new source files with `git add` before authoring.
Unstaged edits to tracked files are readable. The harness excludes untracked
files, generated `evidence/` content, and paths outside the project.

Use `--config <name>` to select a saved authoring configuration. Configuration and
credentials stay outside the project. Bedrock defaults to concurrent assessments;
Ollama defaults to one at a time. Each named configuration's positive `concurrency`
setting overrides its default. If the provider reports throttling, lower that
setting and rerun the command. Do not substitute manual evidence creation when
configuration or provider access is missing; report the blocker.

Use `habeas status [<glob>]` to find requirements and examples needing attention,
or `--all` to include accepted records. Status groups FIXME judgments by issue;
use `--verbose` to list the judgments and original FIXME entries in each group.
Use `habeas info <glob>` to inspect their
judgments, inputs, and validation findings. Globs match whole names; `*` matches
any number of characters, including dots. For a focused reassessment, run
`habeas author <full-name>`; add `--stdout` to inspect its JSON without changing
the evidence file.

Address findings by fixing implementation, tests, or source citations. Review a
substantive change to an approved requirement with the user before changing its
text. Every first-party Rust function or method needs its own applicable
requirement or example citations, including trait methods and test helpers; an
enclosing module citation does not cover its functions. When no existing
requirement explains behavior, propose an appropriate requirement rather than
waiving the function or inventing evidence.
Missing evidence or a truthful false judgment is expected while implementation
is pending; do not write placeholder evidence yourself.

The harness assesses requirements against their implementation and testing of
interesting design cases. It may justify absent tests for simple logic or where
testing is impractical. Examples require tests covering the scenario and expected
outcomes; an accepted true judgment needs a covered `{test}` reference. Function
attestations assess whether all applicable requirements and examples are cited,
including ones the function does not already cite.

Requirement and example judgments are `true`, `false`, or `{"FIXME": ["#125"]}`.
FIXME means source comments or tests explicitly acknowledge every remaining gap;
the entries identify issues or other source content. The convention is
`FIXME(#125)` or `FIXME(rust-lang/rust#123)`, but other source locations are allowed.
The harness records these references; do not invent issue numbers or change a
judgment to make validation pass. CI accepts true and FIXME only when all schema,
freshness, and coverage checks also pass. Function attestations remain boolean
and must be true to validate.

After source or requirement fixes, rerun `habeas author --all`, then
`cargo test --workspace`. Authoring preserves completed writes when another
assessment fails, so rerunning can continue the work. A completed false judgment
still fails validation and needs its reported gaps addressed. Ordinary validation
checks stored evidence without inference; its instruction to run authoring is
for the agent, not permission for CI to call a model automatically.

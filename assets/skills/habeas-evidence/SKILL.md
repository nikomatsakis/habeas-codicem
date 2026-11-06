---
name: habeas-evidence
description: Use the Habeas authoring harness to create or refresh requirement and example evidence, and address Cargo validation failures.
---

# Habeas evidence

Run the authoring harness from the Cargo and mdBook project:

```sh
habeas author --all
```

It fills in missing, stale, or invalid evidence for requirements and named
examples. Current records are preserved, including FIXME judgments and false
judgments whose subject and inputs are unchanged; rerun one explicitly with
`habeas author <full-name>`. Each assessment runs in its own model conversation.
The harness records delivered source inputs and writes the model's judgment and
rationale. The supervising agent must not compose or refresh evidence JSON,
determinations, rationales, or source inputs itself, or delegate that work to
an ad hoc evidence-writing agent.

The model reads sources with the `habeas` tool, lists a file's selectable nodes
with the `outline` tool, and searches with `ripgrep`. Evidence identifies each
source by an outline node of its file, such as `fn[run]` or `impl[Thing]/fn[run]`,
and that node's structural hash; it stores no source text or line numbers. Moving
code or editing comments that the language profile does not hash, such as
ordinary Rust `//` comments, keeps the evidence current; changing the node's code
or doc comments makes it stale and calls for reassessment. `habeas add-input
--item <file>:<line>` adds every node declared on a line to a record pending
review, but authoring remains the way to produce judgments.

Monitor summaries, outcomes, and validation diagnostics without loading generated
evidence bodies into the supervising agent's context. If a rationale needs
investigation, delegate that reading and request a concise implementation finding
rather than the evidence text.

In a Git worktree, stage new source files with `git add` before authoring.
Unstaged edits to tracked files are readable. The default exclusions are
untracked files, generated `evidence/` content, and paths outside the project;
existing Cargo build-script output directories are allowed when discoverable.
The nearest enclosing `habeas.toml` can set `[evidence]` `allow` and `disallow`
paths (literal files or directory subtrees relative to that file). Allow overrides
the defaults and disallow wins. A denied explicit read aborts that assessment;
other batch assessments continue, and the command ultimately reports failure.
The driving human or agent must decide whether to stage the file or change policy.
Do not bypass the error by writing evidence yourself.

Use `--config <name>` to select a saved authoring configuration. Configuration and
credentials can come from the user file or nearest `habeas.toml`; prefer profiles
and environment references over literal credentials in workspace files.
Bedrock defaults to concurrent assessments;
Ollama defaults to one at a time. Each named configuration's positive `concurrency`
setting overrides its default. If the provider reports throttling, lower that
setting and rerun the command. Do not substitute manual evidence creation when
configuration or provider access is missing; report the blocker.

Use `habeas status [<selection>]` to find requirements and examples needing attention,
or `--all` to include accepted records. Status groups FIXME judgments by issue;
use `--verbose` to list the judgments and original FIXME entries in each group.
Use `habeas info <selection>` to inspect their
judgments, inputs, and validation findings. A selection is a full name, which
selects exactly, or a glob matching whole names, where `*` matches any number of
characters, including dots. For a focused reassessment, run
`habeas author <full-name>`; add `--stdout` to inspect its JSON without changing
the evidence file.

Address findings by fixing implementation, tests, or source references. Review a
substantive change to an approved requirement with the user before changing its
text. Every first-party Rust function or method needs its own requirement or
example reference, including trait methods and test helpers; an enclosing module
reference does not cover its functions. Mark code `{impl}` where it implements
part of a definition and tests `{test}` where they are designed to verify it; use
`{cite}` only for related code that does neither. A requirement can `{impl}`
another requirement it refines, and an example can `{test}` or `{cite}` a
requirement. When no existing
requirement explains behavior, propose an appropriate requirement rather than
waiving the function or inventing evidence.
Missing evidence or a truthful false judgment is expected while implementation
is pending; do not write placeholder evidence yourself.

The harness assesses each requirement against the code marked `{impl}` for it
and checks that its `{test}` sources cover interesting design cases. It may
justify absent tests for simple logic or where testing is impractical. The same
review checks that the implementation is marked completely: code that implements
part of the requirement without `{impl}`, including code that only cites it, is a
gap, and so is behavior in `{impl}` code that no definition implemented there
specifies. Examples require tests covering the scenario and expected outcomes; an
accepted true judgment needs a covered `{test}` reference. `{impl}` and `{test}`
lines must be covered by the referenced definition's evidence; `{cite}` lines
need no coverage.

Requirement and example judgments are `true`, `false`, or `{"FIXME": ["#125"]}`.
FIXME means source comments or tests explicitly acknowledge every remaining gap;
the entries identify issues or other source content. The convention is
`FIXME(#125)` or `FIXME(rust-lang/rust#123)`, but other source locations are allowed.
The harness records these references; do not invent issue numbers or change a
judgment to make validation pass. CI accepts true and FIXME only when all schema,
freshness, and coverage checks also pass. Each determination stands alone: a
false judgment does not fail the definitions its subject implements, tests, or
cites.

After source or requirement fixes, rerun `habeas author --all`, then
`cargo test --workspace`. Authoring preserves completed writes when another
assessment fails, so rerunning can continue the work. A completed false judgment
still fails validation and needs its reported gaps addressed. Ordinary validation
checks stored evidence without inference; its instruction to run authoring is
for the agent, not permission for CI to call a model automatically.

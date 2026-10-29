# Habeas requirements proposal

## Overview

Use this SOP to create or revise a requirements-only mdBook pull request for an
`autofix` issue. The pull request is the owner's review space. Its definitions
are proposals until an owner approves the exact head commit.

## Parameters

- **context_json** (required): The controller-provided JSON object appended to the prompt, containing the issue number, title, body, selected owner feedback, explicitly linked comments, and expected Git commit.

**Constraints for parameter acquisition:**
- If all required parameters are already provided, You MUST proceed to the Steps.
- If any required parameters are missing, You MUST ask for them before proceeding.
- When asking for parameters, You MUST request all parameters in a single prompt.
- When asking for parameters, You MUST use the exact parameter names as defined.

## Steps

### 1. Understand the requested behavior

Read the issue, selected feedback, relevant mdBook chapters, source, and tests.
Use `cargo habeas info` in this repository, or `habeas info` where installed,
to inspect related existing definitions.

**Constraints:**
- You MUST treat `context_json` as the complete GitHub conversation input.
- You MUST NOT fetch GitHub comments or use GitHub APIs, because the controller
  has deliberately excluded comments that the owner did not authorize.
- You MUST distinguish existing requirements to reuse from missing behavior.
- You SHOULD keep requirements observable and avoid prescribing a design unless
  the issue requires one.

### 2. Write the proposal

Edit the mdBook chapter or chapters to state the smallest coherent set of
requirements and concrete examples. The owner has chosen to review draft
definitions in a PR; this is the review location for this workflow.

**Constraints:**
- You MUST edit only Markdown under the configured mdBook source directory and
  matching JSON records under `evidence/`.
- You MUST use stable, unique `{req}` and `{example}` IDs and cite the
  requirement illustrated by each example.
- You MUST give new or changed definitions truthful evidence records with
  `determination: false` while the behavior is pending.
- You MUST NOT write implementation or tests, because the owner has not
  approved the proposed obligations yet.
- If selected feedback only asks a question, You MAY leave files unchanged and
  answer it in the final message.

### 3. Check the proposal

Check the changed paths and render the book. Inspect `cargo habeas status` (or
`habeas status` where installed) for proposed definitions.

**Constraints:**
- You MUST run `git diff --check` and `mdbook build`.
- You MUST run a Habeas status command when available and explain expected
  pending determinations in the final message.
- You MUST fix malformed definitions or evidence before finishing.
- You MUST NOT set a pending determination to true just to make validation pass, because that would claim unverified implementation.

### 4. Report for review

Finish with a concise PR summary.

**Constraints:**
- You MUST list the proposed full IDs, important examples, open choices, and
  checks run.
- You MUST say when no files changed, so the controller can post an answer on
  the existing PR.

## Examples

With an `autofix` issue asking for a new CLI flag, add a requirement for the
observable flag behavior, an example for a representative invocation, and
pending evidence records. A request for changes updates that same proposal PR.

## Troubleshooting

If `mdbook build` is unavailable, report the missing tool and leave the
proposal reviewable. If existing requirements conflict with the issue, explain
the conflict as an open choice instead of silently rewriting them.

# Implement approved Habeas requirements

## Overview

Use this SOP after a repository owner approves the current head of a Habeas
requirements proposal PR. Implement exactly that revision and prepare a
separate code PR containing the approved definitions, code, tests, and evidence.

## Parameters

- **context_json** (required): The controller-provided JSON object appended to the prompt, containing the issue, selected owner feedback, exact approved requirements commit, and previous implementation PR head when revising.

**Constraints for parameter acquisition:**
- If all required parameters are already provided, You MUST proceed to the Steps.
- If any required parameters are missing, You MUST ask for them before proceeding.
- When asking for parameters, You MUST request all parameters in a single prompt.
- When asking for parameters, You MUST use the exact parameter names as defined.

## Steps

### 1. Read the approved specification

Read the requirements and examples at the checked-out commit, then inspect
relevant source, tests, and evidence. Use `cargo habeas info` in this repository
or `habeas info` where installed.

**Constraints:**
- You MUST use the checked-out requirements commit as the approved baseline.
- You MUST treat `context_json` as the complete GitHub conversation input.
- You MUST NOT fetch GitHub comments or use GitHub APIs, because the controller
  has deliberately excluded comments that the owner did not authorize.
- You MUST identify which obligations already have supporting behavior.

### 2. Implement and test

Make the smallest changes that satisfy the approved definitions and selected
owner feedback. Add or update tests for the concrete examples.

**Constraints:**
- You MUST add appropriate `{impl}` and `{test}` references to the relevant
  lines and maintain Habeas evidence with exact current inputs.
- You MUST leave `determination: false` when evidence is incomplete.
- You MUST NOT change the substance of an approved requirement or example, because that would bypass the owner's review of the specification.
- You MUST NOT edit `md/`, because the publisher preserves the approved
  mdBook proposal exactly in this implementation stage.
- If an approved obligation needs to change, You MUST stop and explain the
  needed revision in the final message for a new requirements review.
- You MUST NOT edit `.github/` or `agent-sops/`, because workflow code needs
  separate review from an issue-driven implementation.

### 3. Validate the implementation

Run the project checks and inspect any failures.

**Constraints:**
- You MUST run `git diff --check`, `cargo test --locked --workspace`,
  `mdbook test`, and `mdbook build`.
- You MUST inspect Habeas status with `cargo habeas status` or `habeas status`
  when available.
- You MUST fix failures caused by your changes before finishing.
- You MUST report checks that could not run or failures that remain.

### 4. Report the result

Write a concise summary for the code PR.

**Constraints:**
- You MUST state what changed, which approved requirements and examples it
  satisfies, which checks ran, and any remaining limitations.
- You MUST say when no files changed, so the controller can post an answer on
  the existing PR.

## Examples

If the approved proposal defines a CLI flag and an example invocation, add
the flag, test that invocation, cite the requirement from the implementation,
cite the example from the test, and update both evidence records.

## Troubleshooting

If implementation reveals an ambiguous or incorrect obligation, stop and
describe the exact text needing owner review. If a test failure is unrelated
to the patch, record its command and output in the final message.

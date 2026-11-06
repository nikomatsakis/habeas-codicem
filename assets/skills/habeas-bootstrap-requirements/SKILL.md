---
name: habeas-bootstrap-requirements
description: Build an initial Habeas requirement set from an existing codebase by tracing its tests and user-visible behavior; use for requirements bootstrapping or coverage audits.
---

# Bootstrap requirements

Read the project's existing requirements, documentation, public entry points, implementation, and tests before writing new requirements. Follow behavior through the code rather than relying only on test names or top-level dispatch.

Keep two working inventories:

- Every first-party runnable test or test case, including integration tests, doctests, and parameterized cases where present. Record the requirement or example IDs each test checks.
- Every explicit function or method in first-party Rust sources, including trait declarations, test helpers, and internal helpers. Record its file location and the requirement or example IDs it references, with each reference's role. Also inventory relevant public APIs, commands, handlers, UI actions, configuration, outputs, errors, and distinct observable branches. Do not count one tagged entry point as coverage for deeper code.

Exclude generated and third-party code from the inventories. If a candidate looks user-facing but its purpose is unclear, keep it on the gap list until resolved.

Treat requirements inferred from existing code as proposals. Show the user their proposed full IDs and exact text, along with uncertainty about whether observed behavior is intended or a bug. Obtain agreement on the proposals before installing them in the mdBook; prior explicit agreement to the same requirements counts.

Create or reuse the agreed, stable, chapter-qualified `{req}` requirements in the mdBook. State observable obligations with BCP 14 terms. Group tests that check the same behavior under one requirement; several tests MAY share an ID. For tests of internal invariants, write a requirement for the invariant they actually check rather than inventing a user-facing promise. Distinguish intended behavior from a possible bug or incidental implementation detail.

Put at least one `{test}` reference with a full requirement or example name on each test definition or individual case, naming the definitions the test is designed to verify rather than ones it exercises incidentally. A parameterized test can carry a reference at its case source or generating declaration when that is the only editable source. Add multiple references when a test checks multiple definitions. Do not use one file-level tag to stand in for unrelated tests.

Add a requirement or example reference in a Rust comment attached to each inventoried function or method or within its body. Use `{impl}` where the function itself implements part of a definition: it makes a decision, performs a check, produces an output, or handles a case the definition specifies. Calling code that implements a definition does not count. Use `{cite}` only for code that relies on or refers to a definition without implementing or testing it; the cited requirement's review treats cite-only implementing code as a gap. An example must reference a requirement with `{test}` or `{cite}`, so an example reference still traces the function to a requirement. A reference on an enclosing module or `impl` does not cover its functions. There is no waiver for an uncited function. If no behavior-specific requirement explains an internal helper, propose an explicit requirement scoped to that area of internal code, such as `internal.config_parsing`, rather than one catch-all, get the user's agreement, mark the helper with `{impl}`, and run `habeas author --all` so the harness assesses that requirement's evidence; that review checks that the requirement explains each implementing function. Do not use an internal-detail requirement to justify otherwise unspecified end-user-observable behavior.

Run `habeas author --all` to create requirement and example evidence. Each assessment has its own model conversation. Each requirement's review receives the sources marked `{impl}` or `{test}` for it and can look up every source that references it, so it can find implementing code that is unmarked or only cited. It records delivered source inputs and writes judgments and rationales. The supervising agent must not compose or refresh evidence JSON, determinations, rationales, or snapshots itself, or delegate manual evidence writing to another agent. If authoring configuration or credentials are unavailable, report the blocker instead of substituting hand-written evidence.

Review every end-user-observable result and branch, including errors and input handling, when proposing requirements and adding references. Verify that the code fulfills each applicable requirement and does not introduce additional observable behavior without an agreed requirement. When requirements interact, mark each one implemented at a meaningful implementation point with `{impl}`. For example, a `^C` shortcut needs a requirement that explains why it is accepted. A tag alone does not establish that the behavior matches the requirement. Use the bundled `habeas-requirements` and `habeas-evidence` guidance for directive syntax and the authoring workflow.

The harness assesses whether the code marked `{impl}` implements each requirement completely and exactly, whether that marking is complete, with no unspecified behavior in the marked code, and whether tests cover interesting cases, allowing a rationale for absent tests when logic is simple or testing is impractical. For examples, it checks the scenario, test setup, and expected assertions; a true judgment needs a covered `{test}` reference. Its outcomes are true for supported subjects, false for unacknowledged gaps or insufficient information, and `{"FIXME": ["#125"]}` for gaps explicitly acknowledged in source comments or tests. A FIXME does not excuse additional unacknowledged gaps.

For example, source comments can carry these references:

```text
// {test}`requirements.foo`
// {impl}`requirements.foo`
```

After authoring, run the project tests and Habeas validation. Address reported gaps by fixing source, tests, references, or agreed requirement text, then rerun `habeas author --all` and the tests. Validation checks evidence without invoking models. Have both inventories independently reviewed against the source and tests before claiming complete coverage. Report test-tag coverage and function coverage as mapped counts over total counts. Claim 100% only when both inventories have no unmapped items and every observable branch has been reviewed. List remaining gaps with file locations and reasons; do not invent evidence or silently exclude difficult cases.

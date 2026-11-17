# Author code for approved Habeas requirements

Apply the shared one-PR protocol. Start in **🛠 Author code**.

1. Read the approved requirements/examples at `context_json.requirementsSha`
   and the current checkout at `context_json.baseSha`. Inspect relevant source,
   tests, evidence, discussion history, and commits. Use `cargo habeas info` in
   this repository or `habeas info` where installed. Identify obligations already
   satisfied and the changes needed for the selected owner feedback.
2. Implement the approved behavior and test its concrete examples. Mark code that
   implements part of a requirement with `{impl}` and tests that verify it with
   `{test}`, using `{cite}` only for related code that does neither. Then run
   `cargo habeas author --all` (or installed `habeas author --all`) to assess
   evidence. Leave
   evidence JSON, judgments, rationales, and input snapshots to that harness.
   Address reported gaps in source, references, or tests and rerun authoring.
   Report unavailable model configuration or provider access as a blocker.
   Do not edit `md/`
   while authoring code, except to add, remove, or update `:impl:`/`:test:`
   anchor option lines (for example, when moving an anchored file). If an obligation needs revision, follow the shared
   transition protocol: restore this run's code changes, propose requirements
   in a separate requirements-only commit, and wait for approval on this PR.
3. For code changes, run `git diff --check`, `cargo habeas coverage -- --locked --workspace`,
   followed only on success by `cargo habeas coverage-check`,
   `cargo test --doc --workspace`, `mdbook test`, and `mdbook build`. Inspect `cargo habeas status` (or installed
   `habeas status`). Fix failures caused by your changes. Report checks that
   could not run or unrelated failures that remain. For a requirements
   transition, run the proposal checks described in the shared protocol.
4. Write `.autofix-output.json` with the final mode and intent. For code, describe
   the requirements and examples being satisfied by full ID; for a requirements
   proposal, quote and link the relevant user sources. Finish with the active
   badge, changes, validation, limitations, and next review action.

Use `cargo habeas coverage -- <nextest arguments>` for every Rust unit and
integration test run, including focused or filtered runs. It executes parallel
processes through `cargo nextest` and retains coverage for each process. Coverage
profiles, reports, and source snapshots under the Cargo target directory are
disposable build output: never stage or commit them to Git.
Run doctests separately with `cargo test --doc --workspace`.

For full local verification, `cargo habeas test` collects workspace coverage and
then enforces the coverage policy. Recommend separate collection and
`habeas coverage-check` steps in project CI, as shown in the bundled Habeas
[CI example](../assets/skills/habeas/references/ci.md). Collection alone does
not enforce the coverage policy.
{impl}`requirements.test-coverage.check.guidance`

# Author code for approved Habeas requirements

Apply the shared one-PR protocol. Start in **🛠 Author code**.

1. Read the approved requirements/examples at `context_json.requirementsSha`
   and the current checkout at `context_json.baseSha`. Inspect relevant source,
   tests, evidence, discussion history, and commits. Use `cargo habeas info` in
   this repository or `habeas info` where installed. Identify obligations already
   satisfied and the changes needed for the selected owner feedback.
2. Implement the approved behavior and test its concrete examples. Add `{impl}`
   and `{test}` references and maintain evidence with exact current inputs.
   Keep `determination: false` when evidence is incomplete. Do not edit `md/`
   while authoring code. If an obligation needs revision, follow the shared
   transition protocol: restore this run's code changes, propose requirements
   in a separate requirements-only commit, and wait for approval on this PR.
3. For code changes, run `git diff --check`, `cargo test --locked --workspace`,
   `mdbook test`, and `mdbook build`. Inspect `cargo habeas status` (or installed
   `habeas status`). Fix failures caused by your changes. Report checks that
   could not run or unrelated failures that remain. For a requirements
   transition, run the proposal checks described in the shared protocol.
4. Write `.autofix-output.json` with the final mode and intent. For code, describe
   the requirements and examples being satisfied by full ID; for a requirements
   proposal, quote and link the relevant user sources. Finish with the active
   badge, changes, validation, limitations, and next review action.

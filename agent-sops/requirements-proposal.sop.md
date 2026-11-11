# Adjust Habeas requirements

Apply the shared one-PR protocol. Start in **📝 Adjust requirements**.

1. Read the issue, triggering owner feedback, discussion history, and relevant
   commits, mdBook chapters, source, and tests. Use `cargo habeas info` in this
   repository or installed `habeas info`. Reuse existing requirements where
   appropriate. Distinguish user requests from your proposed interpretations.
2. Write the smallest coherent proposal using only Markdown under `md/`.
   Use stable, unique `{req}` and
   `{example}` IDs; reference the requirements illustrated by each example with
   `{test}` or `{cite}`. Keep
   requirements observable and avoid prescribing a design unless requested.
   Leave changed or new definitions' evidence pending; missing or stale evidence
   is expected at this stage. Do not write placeholder judgments or snapshots.
   After approval, the implementation workflow uses `habeas author --all` to
   assess evidence with the configured model. Preserve earlier implementation commits. Do not write code
   or tests until the current proposal receives owner approval. If feedback only
   asks a question, you may leave files unchanged and answer in the final message.
3. Run `git diff --check`, `mdbook build`, and `cargo habeas status` (or installed
   `habeas status`). Fix malformed definitions and report unavailable
   checks. Pending determinations are expected; do not claim success merely to
   make validation pass. Explain conflicting obligations as open choices.
4. Write `.autofix-output.json` with `stage: "requirements"`, the intent behind
   the changes, and exact user quotes with GitHub source links. Finish with the
   requirements badge, proposed full IDs, examples, open choices, and checks.
   Explain that approval of this PR's current head resumes coding on this PR.

# Habeas Codicem

Habeas connects mdBook requirements and examples to exact source evidence. A Cargo test checks definition coverage, determinations, cited lines, `{impl}`, `{cite}`, and `{test}` references, and uncited Rust functions.

## Install

Install Symposium and select the agents you use:

```sh
cargo install symposium
cargo agents init
```

## Add Habeas to a project

Run these commands from a Cargo project that already has `book.toml` and a mdBook `SUMMARY.md`:

```sh
cargo add --dev habeas
cargo agents sync
```

Run `cargo agents sync` in your terminal and choose **Enable** when it offers Habeas's plugin. The plugin provides Habeas's bundled skills. Its session-start hook checks the project and advises the agent to run `habeas setup` when configuration is incomplete.

To configure the project before starting an agent session, or if the hook is unavailable in your agent, run `cargo install habeas` and `habeas setup` yourself. For development from a checkout, use `cargo install --path .`.

`habeas setup` adds Habeas as a dev dependency, configures `[preprocessor.habeas]` to run `habeas mdbook`, adds the badge stylesheet, and creates `tests/habeas.rs`. Setup can be run again without duplicating configuration or overwriting differing files.

Add a `{test}` citation to an appropriate project requirement or named example for the generated test, and cite the rest of your Rust functions before expecting validation to pass.

After setup, run `cargo test --workspace` to validate requirements and evidence.

Run `cargo agents sync` after updating the Habeas dependency to refresh the bundled skills.

If Habeas previously copied skills into `.agents/skills/habeas-*`, review them for local edits before the first Symposium sync. Save any edits under different names or elsewhere, remove those old copies, and then sync so Symposium can manage them.

During local development, `habeas setup --path /path/to/habeas-codicem` uses a local crate in place of the registry dependency.

Run `mdbook build` to render requirement badges and linked references. The badge images load from Shields.io when the book is viewed. Run `cargo test --workspace` for validation; there is no separate CI command.

Use `habeas status` to list requirements with missing, false, or invalid evidence, including changed requirement text or input lines. Use `habeas status --all` to list every requirement. To inspect the current requirement, validation findings, rationale, and inputs as Markdown, use an exact full name or a regular expression:

```sh
habeas info requirements.ci.inputs
habeas info '^requirements\.cli\.'
```

The bundled agent skills describe `habeas info` as a way to look up requirements while working.

For a requirements index, add a chapter to `SUMMARY.md` containing `:::{requirements-index}` on one line and `:::` on the next. The preprocessor lists evidence states there and adds a detail subchapter for each requirement and example.

A requirement states general behavior. An example uses its own directive to show a concrete case and cites the requirement it illustrates. In `md/requirements.md`:

```text
:::{req} forwarding.quit
Pressing q MUST stop managed forwards.
:::

:::{example} forwarding.two_active
With two managed forwards active, pressing q stops both. {cite}`requirements.forwarding.quit`
:::
```

Tests can target the example with `{test}` followed by its full name, `requirements.forwarding.two_active`. The agent must explain in `evidence/requirements.forwarding.two_active.json` how those tests cover both active forwards and the result of pressing q. See [How it works](md/reference.md#examples).

## Experimental GitHub autofix workflow

The [Habeas autofix workflow](.github/workflows/habeas-autofix.yml) keeps the
whole review cycle for an `autofix` issue in one PR and one branch:

1. An owner opens an issue with the label or adds the label to an existing issue.
   The agent opens a PR with proposed requirements, examples, and pending evidence.
2. In **📝 Adjust requirements** mode, owner comments and reviews refine the
   proposal. An **Approve** review of the current head enables coding.
3. In **🛠 Author code** mode, the agent implements the approved requirements,
   tests their examples, and completes evidence. Owner feedback triggers revisions.
   Code commits preserve requirements approval.
4. If implementation reveals that an obligation needs revision, the agent returns
   to **📝 Adjust requirements**, explains the reason, and publishes a
   requirements-only proposal on the same PR. Approval of that new head resumes
   coding. Repeat as needed; merging stays manual.

Every commit belongs to exactly one mode. A run publishes at most one commit;
if it switches to requirements, it restores its uncommitted code changes before
proposing the revision and preserves earlier committed work. Requirements
commits change only `md/` Markdown and matching evidence JSON. Code commits
cannot change `md/`. Every agent message carries its active mode badge.

Commit messages record intent. Requirements commits explain why the behavior
needs to change, quote relevant user feedback, and link the originating GitHub
comments/reviews (or issue body). Agent interpretations and implementation
findings must be distinguished from user requests. Code commits describe the
requirements and examples being implemented by full ID and link the exact
approved requirements revision. The publisher validates quoted sources and
constructs the commit message from the agent's structured output.

The agent can inspect the issue and PR discussion history, including prior
proposals, reviews, inline comments, agent summaries, source URLs, and authors.
The checkout includes Git history for inspecting earlier intent and changes.
Only an account owner (personal repository) or active organization owner can
trigger runs or approve requirements. Other contributors' comments are context,
not instructions or approval. An owner may also link same-repository comments
outside this issue/PR to include their context.

The agent job has no GitHub write credential and runs without network access.
A separate publisher rechecks the head, approval, and discussion snapshot,
rejects mixed-mode patches, and pushes with a lease. Mode and the approved
requirements SHA are stored in the published commit, so state advances atomically
with the code. Dismissing the requirements approval blocks further code runs.
The GitHub App creates PRs so ordinary PR checks run.

This experiment needs a GitHub App installed with **Contents, Issues, and Pull
requests: read and write** permissions. For an organization, also grant
**Members: read**. Set `HABEAS_APP_CLIENT_ID` and the secrets
`HABEAS_APP_PRIVATE_KEY` and `OPENAI_API_KEY`. The App slug becomes the bot's
identity. Add the `autofix` label and trigger it as an owner. The workflow must
be on the default branch for issue and review events to run it.

Requirements commits may fail the full `habeas::validate()` test because pending
evidence honestly has `determination: false`. Proposal runs check book rendering;
code runs must pass the regular Cargo and mdBook checks. The workflow does not
merge automatically or create a second implementation PR.

## Developing and publishing the book

The repository-local `cargo habeas` alias runs `cargo run --release -p habeas --`; only this book invokes its preprocessor with `cargo habeas mdbook`. Consumer books configured by `habeas setup` continue to invoke the installed `habeas mdbook` CLI.

GitHub Actions runs the Cargo and mdBook tests on pull requests and pushes to `main`. Passing pushes build the book and deploy it to GitHub Pages. Set the repository's Pages source to **GitHub Actions** in Settings → Pages.

## Releasing

The dedicated release-plz workflow runs on pushes to `main`. When the packaged crate changes, it opens a release PR with the next version and changelog; merging that PR publishes `habeas` and creates a GitHub release. `release-plz.toml` sets `release_always = false`, so ordinary pushes do not publish.

The checked-in `.github/workflows/release-plz.yml` was generated by `release-plz init` with trusted publishing and a GitHub personal access token. Its checkout uses `persist-credentials: true` and the same token. Running `release-plz init` again rewrites the workflow, so review its diff before committing it.

The crates.io trusted publisher should use owner `nikomatsakis`, repository `habeas-codicem`, workflow filename `release-plz.yml`, and no environment. The GitHub token needs **Contents** and **Pull requests** read/write access only to this repository; `init` stores it as the `RELEASE_PLZ_TOKEN` secret. GitHub Actions must be allowed to create pull requests.

Review each generated release PR before merging it. The normal pull-request CI runs on it.

## License

Habeas Codicem is licensed under either of [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE), at your option.

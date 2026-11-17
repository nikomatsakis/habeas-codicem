# Getting started

{impl}`requirements.install.symposium`
{impl}`requirements.install.symposium.skill_hook`

Start with a Cargo project that already has an mdBook (`book.toml` and a `SUMMARY.md` in the book source directory).

Install [Symposium](https://github.com/symposium-dev/symposium) and select your agents once on this machine:

```sh
cargo install symposium
cargo agents init
```

From the project directory, add Habeas and sync your agent setup:

```sh
cargo add --dev habeas
cargo agents sync
```

Run `cargo agents sync` in your terminal and choose **Enable** when Symposium offers Habeas's plugin. Its default **Ask me later** choice leaves the plugin disabled.

When an agent starts a session in this project, Habeas's hook tells it that the project tracks requirements with Habeas and to load the `habeas` skill before starting any work. The hook also checks setup: if configuration is missing or incompatible, it advises the agent to run `habeas setup`. The hook does not change project files itself.

To configure the project before starting an agent session, or if your agent is not running Symposium hooks, install the CLI and configure the project yourself:

```sh
cargo install habeas
habeas setup
```

`habeas setup` adds Habeas as a Cargo dev dependency, configures mdBook to use the installed `habeas mdbook` preprocessor, copies the badge stylesheet, creates `tests/habeas.rs`, configures the per-process collector in `.config/nextest.toml` at the Cargo workspace root, and enables the automatically generated coverage report under the requirements index. That test calls `habeas::validate()`, so normal Cargo tests check your requirements and evidence.

Once the plugin is enabled, `cargo agents sync` provides Habeas's skills to your configured agents. Run it again after updating the Habeas dependency to refresh the skills.

If you previously ran `habeas agents` or `habeas setup --install-skills`, inspect the resulting `.agents/skills/habeas-*` directories for local edits before your first Symposium sync. Save customized files under different names or elsewhere, remove those old copies, and run `cargo agents sync` to receive Symposium-managed skills.

Use the `habeas-bootstrap-requirements` skill to derive an initial set of requirements from an existing codebase and its tests. Setup does not write requirements or evidence for you. Reference a project requirement or named example on the generated `tests/habeas.rs` test and on every other first-party Rust function, including internal helpers: `{impl}` where the function implements part of a definition, `{test}` where it is designed to verify one, and `{cite}` only for related code that does neither.

Configure an authoring model using the [Bedrock or Ollama examples](./reference.md#authoring-evidence-with-a-model), then run:

```sh
habeas author --all
habeas test
cargo test --doc --workspace
```

The authoring harness assesses each requirement and example in a separate model
conversation. The supervising agent fixes reported gaps and reruns the command;
it does not compose evidence itself. CI checks the saved evidence without inference.
See [How it works](./reference.md) for the evidence index and local-checkout workflow.

Install `cargo-nextest` (>= 0.9.116) and the active toolchain's LLVM tools with
`cargo install cargo-nextest --locked` and `rustup component add llvm-tools-preview`.
Use `habeas coverage -- <nextest arguments>` for filtered runs as well as full
suite runs. Raw profiles, per-process reports and captured sources stay under
the Cargo target directory (`target/habeas/coverage/` by default), are never
committed, and disappear with `cargo clean`.

For full verification, `habeas test` collects workspace coverage and then
enforces the coverage policy. In CI, run these as separate steps, with the
second command running only after the first succeeds:

```sh
habeas coverage -- --locked --workspace
habeas coverage-check
```

Collection alone does not enforce coverage. Add both steps to the project's
workflow after installing its Habeas CLI and coverage prerequisites. Setup
configures the collector and static validation test; it does not install a CI
workflow. The bundled skills include a CI example, delivered with
`cargo agents sync` after updating Habeas.

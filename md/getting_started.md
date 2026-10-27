# Getting started

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

Habeas's hook checks setup when an agent starts a session in this project. If configuration is missing or incompatible, it advises the agent to run `habeas setup`. The hook does not change project files itself.

To configure the project before starting an agent session, or if your agent is not running Symposium hooks, install the CLI and configure the project yourself:

```sh
cargo install habeas
habeas setup
```

`habeas setup` adds Habeas as a Cargo dev dependency, configures mdBook to use the installed `habeas mdbook` preprocessor, copies the badge stylesheet, and creates `tests/habeas.rs`. That test calls `habeas::validate()`, so normal Cargo tests check your requirements and evidence.

Once the plugin is enabled, `cargo agents sync` provides Habeas's skills to your configured agents. Run it again after updating the Habeas dependency to refresh the skills.

If you previously ran `habeas agents` or `habeas setup --install-skills`, inspect the resulting `.agents/skills/habeas-*` directories for local edits before your first Symposium sync. Save customized files under different names or elsewhere, remove those old copies, and run `cargo agents sync` to receive Symposium-managed skills.

Use the `habeas-bootstrap-requirements` skill to derive an initial set of requirements from an existing codebase and its tests. Setup does not write requirements or evidence for you. See [How it works](./reference.md) for those steps, the evidence index, and instructions for using a local checkout.

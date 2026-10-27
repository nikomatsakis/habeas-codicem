# Getting started

Start with a Cargo project that already has an mdBook (`book.toml` and a `SUMMARY.md` in the book source directory).

Install [Symposium](https://github.com/symposium-dev/symposium) and select your agents once on this machine:

```sh
cargo install symposium
cargo agents init
```

From the project directory, add Habeas and enable its Symposium plugin:

```sh
cargo add --dev habeas
cargo agents use habeas
```

Symposium 0.5.1 does not yet install the CLI when enabling Habeas's plugin. Until that works, install it separately and configure the project:

```sh
cargo install habeas
habeas setup
```

`habeas setup` adds Habeas as a Cargo dev dependency, configures mdBook to use the installed `habeas mdbook` preprocessor, copies the badge stylesheet, and creates `tests/habeas.rs`. That test calls `habeas::validate()`, so normal Cargo tests check your requirements and evidence.

`cargo agents use habeas` enables the plugin for this workspace and syncs Habeas's skills into your configured agents. Run `cargo agents sync` after updating the Habeas dependency to refresh the skills. Use `cargo agents use habeas --global` if you want the plugin enabled in every workspace; the crate still needs to be a dependency for its skills to apply.

If you previously ran `habeas agents` or `habeas setup --install-skills`, inspect the resulting `.agents/skills/habeas-*` directories for local edits before your first Symposium sync. Save customized files under different names or elsewhere, remove those old copies, and run `cargo agents sync` to receive Symposium-managed skills.

Use the `habeas-bootstrap-requirements` skill to derive an initial set of requirements from an existing codebase and its tests. Setup does not write requirements or evidence for you. See [How it works](./reference.md) for those steps, the evidence index, and instructions for using a local checkout.

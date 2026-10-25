# Getting started

Start with a Cargo project that already has an mdBook (`book.toml` and a `SUMMARY.md` in the book source directory).

## With Symposium

For best results, install [Symposium](https://github.com/symposium-dev/symposium), then choose your agents once on this machine:

```sh
cargo install symposium
cargo agents init
```

From the project directory, enable Habeas and sync its skills:

```sh
cargo agents use habeas
cargo agents sync
```

Use `cargo agents use --global habeas` instead if you want the plugin enabled in every workspace. This `--global` flag controls where the plugin is enabled. Habeas's `Symposium.toml` separately requests that the CLI be installed in Cargo's global bin directory, where it is available on your `PATH`.

In Symposium 0.5.1, `use` and `sync` install the skills but do not install a standalone CLI merely because it appears in `[[installations]]`. Install the CLI, then configure this project:

```sh
cargo install habeas
habeas setup
```

## Without Symposium

If you prefer to install the CLI directly, use these two commands:

```sh
cargo install habeas
habeas setup --install-skills
```

`habeas setup` adds `habeas` as a Cargo dev dependency, configures mdBook to use the installed `habeas mdbook` preprocessor, copies the badge stylesheet, and creates `tests/habeas.rs`. That test calls `habeas::validate()`, so normal Cargo tests check your requirements and evidence. The `--install-skills` flag also copies three skills into `.agents/skills/` for bootstrapping requirements, authoring them, and recording evidence; you can move them afterward.

Use the `habeas-bootstrap-requirements` skill to derive an initial set of requirements from an existing codebase and its tests. Setup does not write requirements or evidence for you. See [How it works](./reference.md) for those steps, the evidence index, and instructions for using a local checkout.

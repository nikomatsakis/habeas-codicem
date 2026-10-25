# Habeas Codicem

Habeas connects mdBook requirements to exact source evidence. A Cargo test checks requirement coverage, determinations, cited lines, and `{impl}`, `{cite}`, and `{test}` references.

## Install

After publication on crates.io, install the CLI with:

```sh
cargo install habeas
```

For development from a checkout, use `cargo install --path .`.

The Symposium manifest declares a global CLI installation for users who run `cargo agents use habeas`.

## Add Habeas to a project

Run these commands from a Cargo project that already has `book.toml` and a mdBook `SUMMARY.md`:

```sh
habeas setup --install-skills
cargo test --workspace
```

`habeas setup` adds Habeas as a dev dependency, configures `[preprocessor.habeas]` to run `habeas mdbook`, adds the badge stylesheet, and creates `tests/habeas.rs`. The `--install-skills` flag also copies three bundled skills into `.agents/skills/` for bootstrapping requirements, authoring them, and recording evidence; you can move them afterward. Setup can be run again without duplicating configuration or overwriting differing files.

With Symposium, run `cargo agents sync` in a project that depends on Habeas and approve its plugin to receive these same skills from `assets/skills`. The `habeas agents` command remains available to install just the skills.

During local development, `habeas setup --path /path/to/habeas-codicem --install-skills` uses a local crate in place of the registry dependency and installs the skills.

Run `mdbook build` to render requirement badges and linked references. The badge images load from Shields.io when the book is viewed. Run `cargo test --workspace` for validation; there is no separate CI command.

For a requirements index, add a chapter to `SUMMARY.md` containing `:::{requirements-index}` on one line and `:::` on the next. The preprocessor lists evidence states there and adds a detail subchapter for each requirement.

A requirement uses a colon-fenced directive. Its body includes all Markdown up to the matching closing fence:

```text
:::{req} forwarding.quit
Pressing q MUST stop managed forwards.

## Example
An example or diagram can live here.
:::
```

## Developing and publishing the book

The repository-local `cargo habeas` alias runs `cargo run --release -p habeas --`; only this book invokes its preprocessor with `cargo habeas mdbook`. Consumer books configured by `habeas setup` continue to invoke the installed `habeas mdbook` CLI.

GitHub Actions runs the Cargo and mdBook tests on pull requests and pushes to `main`. Passing pushes build the book and deploy it to GitHub Pages. Set the repository's Pages source to **GitHub Actions** in Settings → Pages.

## License

Habeas Codicem is licensed under either of [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE), at your option.

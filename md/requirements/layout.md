# Project layout

:::{req} project
The Habeas Codicem project MUST keep its mdBook Markdown in `md/`, the `habeas` crate's Rust source in `src/`, and the `trust-but-verify` crate's Rust source in `crates/trust-but-verify/src/`. Its own book MUST use the Habeas preprocessor as specified by {cite}`requirements.cli.mdbook`.
:::

:::{req} project.workspace
The repository is a Cargo workspace with exactly two first-party packages:
`habeas` at the repository root and `trust-but-verify` in
`crates/trust-but-verify/`.
:::

:::{req} repository_links
The Habeas Codicem book MUST configure mdBook's Git repository link for `nikomatsakis/habeas-codicem` and its page edit links for the `main` branch.
:::

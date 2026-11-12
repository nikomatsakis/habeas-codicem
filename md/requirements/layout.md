# Project layout

:::{req} project
The Habeas Codicem project MUST keep its mdBook Markdown in `md/`, the `habeas` crate's Rust source in `src/`, and the `habeas-sdk` crate's Rust source in `crates/habeas-sdk/src/`. Its own book MUST use the Habeas preprocessor as specified by {cite}`requirements.cli.mdbook`.
:::

:::{req} project.workspace
The repository MUST be a Cargo workspace containing the `habeas` package at the repository root and the `habeas-sdk` package in `crates/habeas-sdk/`.
:::

:::{req} repository_links
The Habeas Codicem book MUST configure mdBook's Git repository link for `nikomatsakis/habeas-codicem` and its page edit links for the `main` branch.
:::

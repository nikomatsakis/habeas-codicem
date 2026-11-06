# Requirement rewordings: cli, configuration, integration, layout, index

Rewordings made while turning false evidence records into true ones. Each entry gives the requirement, its old and new text, the reason, and the evidence rationale that prompted it. Requirement text is quoted in code blocks so that its references stay literal.

## `requirements.integration.ci`

Old text:

```text
GitHub Actions MUST run `cargo test --locked --workspace` and `mdbook test` for pull requests and pushes to `main`. After those checks pass on `main`, it MUST build the book and deploy the generated `book/` directory to GitHub Pages. The book MUST set `output.html.site-url` to `/habeas-codicem/` for project-site hosting.
```

New text:

```text
A GitHub Actions CI workflow MUST run, for pull requests and pushes to `main`, these checks: `cargo test --locked --workspace`, `mdbook test`, the autofix controller tests with `node --test .github/scripts/autofix.test.mjs`, and, in a separate job, the Symposium session-start hook smoke test `tests/symposium_hook_smoke.py`. It MUST also build the book, and on a push to `main`, only after every check job passes, it MUST deploy the generated `book/` directory to GitHub Pages. The book MUST set `output.html.site-url` to `/habeas-codicem/` for project-site hosting.
```

Reason: The workflow `.github/workflows/ci.yml` also runs the autofix controller tests (`node --test`) and a separate `symposium-hook` job, and the deploy job depends on both. These are sensible, intended checks, but the old text named only the two Cargo/mdBook checks, so the reviewer had to treat them as unspecified behaviour in the marked workflow. The new text states every check the workflow runs, that the book is built on every run, and that deployment on `main` waits for every check job. The original obligations (both checks on PRs and pushes to `main`, deploy `book/` to Pages after checks pass on `main`, `site-url`) are unchanged.

Prompted by: evidence/requirements.integration.ci.json gap 3: "Behavior the requirement does not specify. The CI workflow also runs the autofix controller tests (`node --test`) and a separate `symposium-hook` job. Deployment depends on both, and neither is mentioned in this requirement."

## `requirements.integration.symposium_setup_hook`

Old text:

```text
For an enabled Habeas plugin, a supported agent session started within a Cargo and mdBook project depending on `habeas` MUST receive a Symposium `SessionStart` hook. The hook MUST check whether the configuration and files required by `requirements.cli.setup` are present and compatible. If not, it MUST provide nonblocking agent context saying that Habeas Codicem is not fully configured and suggesting `habeas setup`. If setup is complete or no applicable project is found, it MUST provide no setup advisory. The hook MUST NOT modify project files or run setup itself.
```

New text:

```text
For an enabled Habeas plugin, a supported agent session started within a Cargo and mdBook project whose manifest lists `habeas` under `[dev-dependencies]`, as {cite}`requirements.cli.setup` adds it, MUST receive a Symposium `SessionStart` hook. The hook MUST check whether the configuration and files required by `requirements.cli.setup` are present and compatible. If not, it MUST provide nonblocking agent context saying that Habeas Codicem is not fully configured and suggesting `habeas setup`. If setup is complete or no applicable project is found, it MUST provide no setup advisory. The hook MUST NOT modify project files or run setup itself.
```

Reason: The hook decides that a project "depends on `habeas`" only when its manifest has a `[dev-dependencies]` entry for `habeas`; a regular or workspace dependency yields no advisory. That is the intended rule: `habeas setup` adds Habeas as a dev dependency, and a project using Habeas as an ordinary library is not a project whose requirements Habeas validates. The new text states the dev-dependency condition precisely and cites `requirements.cli.setup` as its source, instead of the ambiguous "depending on `habeas`". The other obligations are unchanged; the separate gap that the completeness check ignored `output.html.fold.enable` was fixed in code, not by rewording.

Prompted by: evidence/requirements.integration.symposium_setup_hook.json gap 3: "Possible narrowing, lower severity. 'Depending on `habeas`' is checked only as a `[dev-dependencies]` entry, so a regular or workspace dependency yields no advisory. This is defensible because setup adds a dev dependency, but it is not stated in the requirement."

## `requirements.index.detail_format`

Old text:

```text
Each evidence detail page MUST display its state as a Shields.io static badge image with meaningful alt text and the current requirement or example body as a Markdown block quote labeled by kind. It MUST render the recorded rationale as Markdown in a Rationale section and display the recorded FIXME strings when the determination is FIXME. Inputs MUST be grouped under headings in this order: Implementation, for sources reached by an `{impl}` edge from the definition, including anchored files; Tests, for sources reached by a `{test}` edge, including tests, examples, and anchored files; Referenced definitions, for declarations the definition cites and their terminology; and Other sources, for the remaining inputs the assessment consulted. Edges MUST be determined from the current references under {cite}`requirements.references.targets` and {cite}`requirements.references.anchors`. Each group and each input MUST be a collapsible disclosure element, with groups expanded and inputs collapsed by default, so readers can move between groups and open individual sources. Each input MUST have its own subsection identifying its selector. An outline input MUST show its file, selector, kind, and current inclusive line range. A requirements input MUST show its selection and each matched definition's full name, file, and inclusive line range. Each outline input subsection MUST show the node's current source text in a fenced code block and say when its hash no longer matches; a requirements or definition input subsection MUST show its recorded text. Code fences MUST request Rust highlighting for `.rs` files and language-aware highlighting for Markdown, JSON, TOML, Java, JavaScript, TypeScript, and Python files; TOML MAY use INI highlighting when native TOML highlighting is unavailable.
```

New text:

```text
Each evidence detail page MUST display its state as a Shields.io static badge image with meaningful alt text and the current requirement or example body as a Markdown block quote labeled by kind. It MUST render the recorded rationale as Markdown in a Rationale section and display the recorded FIXME strings when the determination is FIXME. Inputs MUST be grouped under headings in this order, each input appearing once, in the first group that applies to it: Implementation, for sources reached by an `{impl}` edge from the definition, including anchored files; Tests, for sources reached by a `{test}` edge, including tests, examples, and anchored files; Referenced definitions, for declarations the definition cites and their terminology; and Other sources, for the remaining inputs the assessment consulted. Edges MUST be determined from the current references under {cite}`requirements.references.targets` and {cite}`requirements.references.anchors`. Each group and each input MUST be a collapsible disclosure element, with groups expanded and inputs collapsed by default, so readers can move between groups and open individual sources. Each input MUST have its own subsection identifying its selector. An outline input MUST show its file, selector, kind, and current inclusive line range. A requirements input MUST show its selection and each matched definition's full name, file, and inclusive line range. Each outline input subsection MUST show the node's current source text in a fenced code block and say when its hash no longer matches; a requirements or definition input subsection MUST show its recorded text. Code fences MUST request Rust highlighting for `.rs` files and language-aware highlighting for Markdown, JSON, TOML, Java, JavaScript, TypeScript, and Python files; TOML MAY use INI highlighting when native TOML highlighting is unavailable.
```

Reason: The grouping rule listed four groups by edge kind but did not say what happens to a source reached by more than one kind of edge, such as a function marked both impl and test. The renderer places each input once, in the first applicable group (Implementation before Tests before Referenced definitions before Other sources), which keeps every input as a single disclosure and avoids duplicate input numbering. The new text states that each input appears once, in the first group that applies to it. A test now covers a dual-edge source. Nothing else in the requirement changed.

Prompted by: evidence/requirements.index.detail_format.json gap 3: "Dual edges are not specified. `DefinitionEdges::group` returns at the first matching edge, so a source that is both an impl and a test target appears only under Implementation. ... This case is neither specified nor tested."

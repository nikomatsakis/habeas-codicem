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

## `requirements.integration.release`

Old text:

```text
A dedicated GitHub Actions workflow MUST run release-plz on pushes to `main` to create release pull requests and publish approved versions. Publishing MUST require a merged release-plz pull request. The release job MUST request an OIDC token for crates.io trusted publishing, and both jobs MUST use a GitHub token from a repository secret so release pull requests can trigger the normal pull request checks.
```

New text:

```text
A dedicated GitHub Actions workflow MUST run release-plz on pushes to `main` to create release pull requests and publish approved versions. Both jobs MUST run only when the repository owner is `nikomatsakis`, so forks do not attempt releases. Release pull request runs for the same ref MUST NOT run concurrently; a later run MUST wait for an earlier one rather than cancel it. Publishing MUST require a merged release-plz pull request. The release job MUST request an OIDC token for crates.io trusted publishing, and both jobs MUST use a GitHub token from a repository secret so release pull requests can trigger the normal pull request checks.
```

Reason: Both release-plz jobs in `.github/workflows/release-plz.yml` carry an `if: github.repository_owner == 'nikomatsakis'` guard, and the release-PR job has a `concurrency` group keyed on the ref with `cancel-in-progress: false`. These are deliberate: forks must not try to publish or open release PRs, and overlapping release-PR runs must queue rather than race or cancel each other. The old text did not mention either, so the reviewer treated them as unspecified behaviour in the marked workflow. The new text states both. The original obligations are unchanged, and a test now checks the guard and the concurrency setting.

Prompted by: evidence/requirements.integration.release.json, "Minor, unspecified behavior": "The jobs have an `if: github.repository_owner == 'nikomatsakis'` guard. The PR job has a `concurrency` group. Neither is specified by this requirement or by another definition."

## `requirements.configuration.workspace_structure`

(Made by the configuration sub-task.)

Old text:

```text
`habeas.toml` MUST serialize WorkspaceConfig as UTF-8 TOML with `version = 1`.
Its supported feature tables are `[authoring]` and `[evidence]`; either MAY be
absent. Unknown common keys, unsupported versions, invalid field types, and
invalid TOML MUST produce a diagnostic identifying the file and setting.
Features MUST define their own data structures and loading rules under
{cite}`requirements.configuration.feature_sections`. A file containing only
`version = 1` MUST be valid. Habeas settings MUST NOT require a `[habeas]` table
inside `book.toml`.
```

New text:

```text
`habeas.toml` MUST serialize WorkspaceConfig as UTF-8 TOML with `version = 1`.
Its supported feature tables are `[authoring]` and `[evidence]`; either MAY be
absent. Unknown keys, unsupported versions, and invalid field types MUST produce
a diagnostic identifying the file and setting. Invalid TOML, including content
that is not UTF-8, MUST produce a diagnostic identifying the file, with the line
and column of a syntax error.
Features MUST define their own data structures and loading rules under
{cite}`requirements.configuration.feature_sections`. A file containing only
`version = 1` MUST be valid. Habeas settings MUST NOT require a `[habeas]` table
inside `book.toml`.
```

Reason: A TOML syntax error or a file that is not UTF-8 has no setting to name, so the old text asked for something that cannot exist in those cases. The new text requires the line and column for syntax errors and the setting wherever one exists, and extends the unknown-key rule from common keys to all keys, which the code already enforces. The code was also fixed so serde decoding errors name the setting path (for example `evidence.disallow`). No intended obligation is weakened.

Prompted by: evidence/requirements.configuration.workspace_structure.json (reassessment): "Imprecise setting in two diagnostics: An invalid-UTF-8 read error and a TOML syntax error name the file but give an empty or no setting", alongside the original "Setting identification" gap, which was fixed in code.

## `requirements.configuration.evidence_paths`

(Made by the configuration sub-task.)

Old text:

```text
The workspace `[evidence]` table MUST serialize EvidenceConfig. `allow` and
`disallow` MUST be arrays of nonempty literal paths, each naming a file or a
directory subtree. Relative paths MUST resolve from the selected `habeas.toml`
directory; absolute paths MUST be supported. These paths are not glob patterns.

For authoring reads, a matching disallow MUST reject the path; otherwise a
matching allow MUST permit it; otherwise the default source-access rules MUST
apply. Explicit allow MUST override every default, including Git tracking,
project containment, generated-evidence exclusion, and automatic generated-output
rules. Disallow MUST win even when an allow or generated-output rule also matches.
Both the requested path and its resolved symlink target MUST be checked for
disallow, so an allowed alias cannot expose a disallowed target.

The same explicit disallow rules and external-path exceptions MUST apply when
resolving recorded evidence inputs, including ordinary validation and
`add-input`. Validation MUST remain independent of Git index membership: it
checks the recorded inputs' existence and contents. Loading path policy MUST NOT
resolve credentials or validate unused authoring settings.
```

New text:

```text
The workspace `[evidence]` table MUST serialize EvidenceConfig. `allow` and
`disallow` MUST be arrays of nonempty literal paths, each naming a file or a
directory subtree. Relative paths MUST resolve from the selected `habeas.toml`
directory; absolute paths MUST be supported. These paths are not glob patterns.
An omitted `allow` or `disallow` MUST mean an empty array, and an absent
`[evidence]` table MUST mean both are empty, so only the default source-access
rules apply. No operation requires the section. The table has no user-file
counterpart, and these rules have no invocation options. This section refines
{impl}`requirements.configuration.feature_sections` for the evidence feature.

For authoring reads, a matching disallow MUST reject the path; otherwise a
matching allow MUST permit it; otherwise the default source-access rules MUST
apply. Explicit allow MUST override every default, including Git tracking,
project containment, generated-evidence exclusion, and automatic generated-output
rules. Disallow MUST win even when an allow or generated-output rule also matches.
Both the requested path and its resolved symlink target MUST be checked for
disallow, so an allowed alias cannot expose a disallowed target.

The same explicit disallow rules and external-path exceptions MUST apply when
resolving recorded evidence inputs, including ordinary validation and
`add-input`. Validation MUST remain independent of Git index membership: it
checks the recorded inputs' existence and contents. Loading path policy MUST NOT
resolve credentials or validate unused authoring settings.
```

Reason: Refinement, not weakening. `requirements.configuration.feature_sections` requires each feature to state its absent-section behaviour, which operations require the section, and how user, project, and invocation settings combine. The evidence feature only implied these in a schema comment. The added sentences state what the code already does (`#[serde(default)]`, `unwrap_or_default`) and mark this requirement as refining feature_sections for the evidence feature. Obligations are added, none removed.

Prompted by: evidence/requirements.configuration.feature_sections.json: "No document or code is marked impl for `feature_sections` ... The `[evidence]` feature is a second feature section ... but it is not tied to `feature_sections` either."

## `requirements.cli.add_input_item`

Old text:

```text
A named node is
declared on the line containing its name; an anonymous node is declared on the
line where it starts.
```

New text:

```text
A named node is
declared on the line where its name begins in the source. An anonymous node, or
a named node whose name is not written in the source, such as an extern block
without an ABI string, is declared on the line where its own syntax starts,
excluding attached comments and attributes.
```

Reason: Precision. A name such as `impl[Wrapper/From<T>]` can span lines when a header wraps, so "the line containing its name" was ambiguous; the code now declares custom-named nodes (impls, extern blocks, Markdown sections, CSS rules) where the name begins, and tests it. "Where it starts" was ambiguous for an anonymous node with attached attributes, whose line range starts at the attribute; the code declares it where its own syntax starts, which is the line a user points at. An extern block named `Rust` by default has no name in the source and falls under the same start rule. No obligation is removed.

Prompted by: evidence/requirements.cli.add_input_item.json: "Named nodes with custom naming are declared at the node's start, not at their name. ... Anonymous nodes use the grammar node's start, not the node's start including attached siblings ... It is arguable which line \"where it starts\" means, but the code and the requirement text are not clearly aligned."

## `requirements.integration.live_bedrock_test`

Old text:

```text
`HABEAS_LIVE_CONFIG`, when set, MUST select a named authoring configuration for
the test. Otherwise the test MUST use the project's or user's selected
configuration when it selects Bedrock, falling back to the first named Bedrock
configuration in name order.
```

New text:

```text
`HABEAS_LIVE_CONFIG`, when set, MUST select a named authoring configuration for
the test. Otherwise the test MUST use the configuration that authoring would
select for the checkout under {cite}`requirements.authoring.configuration_precedence`
when it is a Bedrock configuration, falling back to the first Bedrock
configuration in name order among the user and workspace configurations. The
fixture project MUST resolve the selected name against the same workspace
authoring configurations as the checkout.
```

Reason: Precision, resolving the ambiguity the reviewer raised in favour of the stronger reading. "The project's or user's selected configuration" did not say whether the workspace `habeas.toml` counted. The test now follows the product's precedence (user project selection, workspace default, user default), considers workspace configurations when falling back, and copies the checkout's workspace `[authoring]` table into the fixture project so `--config` resolves the same record. The new text states that behaviour; it adds obligations and removes none.

Prompted by: evidence/requirements.integration.live_bedrock_test.json: "Project configuration may be ignored. The product's own precedence ... is the user's project selection, then the workspace `habeas.toml` default, then the user default. `selected_config` calls `resolve_authoring_from`, which passes no workspace configuration. ... The requirement text is ambiguous here, and neither the code nor any comment settles the question."

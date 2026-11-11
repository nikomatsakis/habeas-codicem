# Project integration

:::{req} validate
`habeas::validate()` MUST check the current Cargo and mdBook project and make its Cargo test fail with all validation errors when any requirement evidence is invalid.
Its failure message MUST instruct the agent to run `habeas author --all`, address
the reported gaps, and rerun tests. Validation MUST remain deterministic and MUST
NOT invoke authoring itself. {cite}`requirements.authoring.batch`
{cite}`requirements.authoring.deterministic_validation`
:::

:::{req} self_test
The Habeas crate MUST have a Cargo integration test that invokes `habeas::validate()` against its own project, so its own requirements and evidence are checked recursively by `cargo test`.
:::

:::{req} live_bedrock_test
The Habeas crate MUST have a Cargo integration test that exercises evidence
authoring against a real configured Bedrock model when AWS credentials can be
loaded. It MUST skip before reading configuration or resolving credentials when
the `CI` environment variable is present. Outside CI, it MUST fail when no
Bedrock authoring configuration exists, and MUST skip with a visible warning when
the AWS SDK cannot load credentials. Skip warnings MUST be visible under ordinary
`cargo test` output.

`HABEAS_LIVE_CONFIG`, when set, MUST select a named authoring configuration for
the test. Otherwise the test MUST use the configuration that authoring would
select for the checkout under {cite}`requirements.authoring.configuration_precedence`
when it is a Bedrock configuration, falling back to the first Bedrock
configuration in name order among the user and workspace configurations. The
fixture project MUST resolve the selected name against the same workspace
authoring configurations as the checkout. Malformed configuration and invalid explicit
selections MUST fail the test. The selected profile, region, model, and options
MUST come from that configuration; credential resolution MUST use the AWS SDK.

Once credentials load, model-request and authoring failures MUST fail the test
rather than be treated as skips. The test MUST exercise an additional source read
and judgment submission using a temporary project and stdout evidence output,
without changing the checkout's evidence. It MUST retain ordinary usage history
and check the returned evidence and recorded usage. Deterministic tests MUST
verify the CI and missing-credential skip paths and the missing-configuration
failure without calling a model.
{cite}`requirements.authoring.provider`
{cite}`requirements.authoring.source_tools`
{cite}`requirements.authoring.submission`
{cite}`requirements.authoring.persistence`
{cite}`requirements.authoring.usage_storage`
:::

:::{req} symposium
For a Cargo workspace that depends on `habeas`, interactive `cargo agents sync` MUST let the user enable Habeas's plugin. Once enabled, it MUST make the skills shipped with the selected Habeas version available to configured agents. After the selected version changes, `cargo agents sync` MUST provide the skills shipped with the new version.
:::

:::{req} symposium_cli
The enabled Habeas Symposium plugin MUST request a global Cargo installation of the `habeas` CLI and use that installation as its session-start hook command, so the CLI is available when the hook advises an agent to run `habeas setup`.
:::

:::{req} symposium_setup_hook
For an enabled Habeas plugin, a supported agent session started within a Cargo and mdBook project whose manifest lists `habeas` under `[dev-dependencies]`, as {cite}`requirements.cli.setup` adds it, MUST receive a Symposium `SessionStart` hook. The hook MUST check whether the configuration and files required by `requirements.cli.setup` are present and compatible. If not, it MUST provide nonblocking agent context saying that Habeas Codicem is not fully configured and suggesting `habeas setup`. If setup is complete or no applicable project is found, it MUST provide no setup advisory. The hook MUST NOT modify project files or run setup itself.
:::

:::{req} symposium_skill_hook
For an enabled Habeas plugin, a supported agent session started within a Cargo and mdBook project whose manifest lists `habeas` under `[dev-dependencies]`, as {cite}`requirements.cli.setup` adds it, MUST receive nonblocking agent context from the `SessionStart` hook of {cite}`requirements.integration.symposium_setup_hook`. That context MUST contain exactly one sentence that points to the skill. The sentence says the project uses Habeas and tells the agent to load the `habeas` skill of {cite}`requirements.cli.overview_skill` before starting any work in the project.
:::

:::{example} symposium_skill_hook.configured
In a project where `habeas setup` has completed, the whole session-start agent context is one sentence such as "This project tracks requirements with Habeas; load the `habeas` skill before starting any work." {test}`requirements.integration.symposium_skill_hook`
:::

:::{req} symposium_skill_hook.setup_state
When the setup advisory of {cite}`requirements.integration.symposium_setup_hook` also applies, the hook MUST provide the sentence of {cite}`requirements.integration.symposium_skill_hook` and the advisory together in one agent context, with the sentence first.
:::

:::{example} symposium_skill_hook.incomplete_setup
In a project that lists `habeas` under `[dev-dependencies]` but has no `habeas.css` in its book source directory, the agent context starts with the skill sentence, followed by the advisory saying Habeas Codicem is not fully configured and suggesting `habeas setup`. {test}`requirements.integration.symposium_skill_hook.setup_state`
:::

:::{req} symposium_skill_hook.not_applicable
When a session starts outside every Cargo and mdBook project, or within one whose manifest does not list `habeas` under `[dev-dependencies]`, the hook MUST provide no agent context.
:::

:::{example} symposium_skill_hook.runtime_dependency_only
A Cargo and mdBook project that lists `habeas` only under `[dependencies]` gets no session-start agent context, neither the skill sentence nor a setup advisory. {test}`requirements.integration.symposium_skill_hook.not_applicable`
:::

:::{req} ci
:impl: .github/workflows/ci.yml#pair[jobs]
A GitHub Actions CI workflow MUST run, for pull requests and pushes to `main`, these checks: `cargo test --locked --workspace`, `mdbook test`, the autofix controller tests with `node --test .github/scripts/autofix.test.mjs`, and, in a separate job, the Symposium session-start hook smoke test `tests/symposium_hook_smoke.py`. It MUST also build the book, and on a push to `main`, only after every check job passes, it MUST deploy the generated `book/` directory to GitHub Pages. The book MUST set `output.html.site-url` to `/habeas-codicem/` for project-site hosting.
:::

:::{req} release
:impl: .github/workflows/release-plz.yml#pair[jobs]
A dedicated GitHub Actions workflow MUST run release-plz on pushes to `main` to create release pull requests and publish approved versions. Both jobs MUST run only when the repository owner is `nikomatsakis`, so forks do not attempt releases. Release pull request runs for the same ref MUST NOT run concurrently; a later run MUST wait for an earlier one rather than cancel it. Publishing MUST require a merged release-plz pull request. The release job MUST request an OIDC token for crates.io trusted publishing, and both jobs MUST use a GitHub token from a repository secret so release pull requests can trigger the normal pull request checks.
:::

:::{req} report_api
`habeas::validate_repository(root)` MUST return a report with the number of discovered requirements, the number of evidence files, and all validation errors for the supplied project. Ordinary invalid evidence MUST be reported in that report; an inaccessible project root MUST return an I/O error.
:::

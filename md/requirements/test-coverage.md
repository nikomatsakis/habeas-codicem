# Test execution coverage

Execution coverage records which instrumented Rust code ran in test processes.
The evidence-input coverage of
{cite}`requirements.references.coverage.report` records which code requirement
assessments read; execution coverage does not replace those assessments.

The nextest integration uses nextest-owned configuration in
`.config/nextest.toml`. It adds no settings to `habeas.toml`; that file's existing
`[coverage]` table continues to control evidence-input coverage history.

:::{req} guidance
:impl: assets/skills/habeas/SKILL.md
:impl: assets/skills/habeas-evidence/SKILL.md
:impl: assets/skills/habeas-bootstrap-requirements/SKILL.md
:impl: agent-sops/approved-implementation.sop.md
Habeas's bundled agent instructions MUST direct agents in Habeas-enabled Cargo
projects to use the coverage workflow of
{cite}`requirements.test-coverage.run` by default for Rust unit and integration
test runs, including focused or filtered runs. That workflow MUST execute tests
through `cargo nextest`.
:::

:::{req} guidance.doctests
:impl: assets/skills/habeas/SKILL.md
:impl: assets/skills/habeas-evidence/SKILL.md
:impl: assets/skills/habeas-bootstrap-requirements/SKILL.md
:impl: agent-sops/approved-implementation.sop.md
Habeas's bundled agent instructions MUST direct agents to run Cargo doctests
separately with `cargo test --doc --workspace`.
:::

:::{req} run
`habeas coverage [-- <nextest arguments>]` MUST run LLVM-instrumented Rust tests
through `cargo nextest run`, forwarding the supplied arguments and preserving
nextest's configured test concurrency, filtering, and retries. With no supplied
arguments it MUST select the workspace.
:::

:::{req} run.configuration
The coverage workflow MUST verify that the effective nextest profile and
configuration selected for the invocation route every selected test attempt
through Habeas's per-process collector, accounting for profile inheritance and
wrapper-rule precedence. If that cannot be established, it MUST fail before
executing tests with a diagnostic identifying the conflicting profile or
setting, rather than silently collecting incomplete coverage.
:::

:::{req} processes
Each coverage run MUST retain LLVM coverage profiles independently for each
test process, identifying its nextest binary, test name, and attempt. Concurrent
tests, tests with the same name in different binaries, retries, and separate
runs MUST NOT overwrite or combine one another's stored profiles. Profiles from
instrumented child processes MUST remain separate and associated with the test
attempt that spawned them.
:::

:::{req} processes.reports
Habeas MUST support deriving both a report for an individual recorded process
and a combined report without discarding the separate profiles of
{cite}`requirements.test-coverage.processes`. Retained run data MUST remain
sufficient to display those reports using the matching captured source after
the project is rebuilt or its source changes.
:::

:::{req} storage
Coverage runs MUST be stored as disposable build artifacts under
`habeas/coverage/` in the project's Cargo target directory
(`target/habeas/coverage/` with default Cargo settings), retaining earlier runs
until those artifacts are removed. Each run record MUST identify the run,
capture time, command, and test selection, and, when available, the source
revision and whether the source had uncommitted changes.
:::

:::{req} guidance.artifacts
:impl: assets/skills/habeas/SKILL.md
:impl: assets/skills/habeas-evidence/SKILL.md
:impl: assets/skills/habeas-bootstrap-requirements/SKILL.md
:impl: agent-sops/approved-implementation.sop.md
Habeas's bundled agent instructions MUST tell agents that coverage artifacts
under {cite}`requirements.test-coverage.storage`, including profiles, reports,
and captured source snapshots, are disposable build output and MUST NOT be
staged or committed to Git.
:::

:::{req} storage.provenance
Each coverage record MUST identify its instrumented build configuration and
capture enough source-input identity to determine whether its coverage remains
compatible with the project's current source. Source identity MUST account for
uncommitted content changes rather than rely only on the Git revision.

Source identity MUST include the paths and contents of the workspace's
first-party Rust sources and the recorded source and build inputs that affect
the instrumented program, including manifests and the dependency lockfile when
present. Adding, removing, or changing an included input MUST invalidate the
workspace's collected coverage for the combined view. Freshness checking MUST
NOT require rebuilding the project.
:::

:::{req} storage.configuration
Each coverage record MUST identify the recorded toolchain, compilation target,
Cargo build profile, enabled features, and instrumentation-affecting compiler
settings. These settings MUST determine build-configuration compatibility for
the combined view. A nextest scheduling profile alone MUST NOT make otherwise
compatible build configurations incompatible.
:::

:::{req} storage.outcomes
Each coverage run MUST record the outcome of every test attempt that started.
It MUST distinguish successful, failed, and interrupted runs and identify
attempts with missing or unreadable coverage rather than treating them as zero
execution. An empty test selection MUST be identified as empty; nextest's
success or failure for that selection MUST remain the test execution result.
:::

:::{req} failures
The coverage workflow MUST return a failing exit status when test execution or
coverage collection fails, preserve any profiles already collected, and report
the failure. Collecting coverage MUST NOT turn a failing test run into a
successful command.
:::

:::{req} failures.prerequisites
When required nextest or LLVM coverage tooling is unavailable or has an
unsupported version, the coverage workflow MUST identify the prerequisite and
explain how to obtain a supported version.
:::

:::{req} index
A book MAY contain one standalone `{coverage-index}` directive with an empty
body, opened by at least three colons and closed by at least as many. The
preprocessor MUST reject duplicate, unclosed, or nonempty directives outside
Markdown code examples. When the directive is present, `habeas mdbook` MUST
replace it with the report of {cite}`requirements.test-coverage.index.summary`.
:::

:::{req} index.selection
The coverage index MUST combine the most recent recorded test attempt for each
nextest binary and test name among the retained runs. "Most recent" MUST refer
to when the attempt started, including failed or interrupted attempts. A newer
filtered run MUST update only the tests it executes, leaving other compatible
test records available to the combined view. A newer attempt with missing or
unreadable coverage MUST be shown as unavailable rather than replaced with an
older successful attempt. Results from incompatible instrumented build
configurations MUST NOT be combined under
{cite}`requirements.test-coverage.storage.configuration`.

The main combined view MUST use the instrumented build configuration of the
most recently started recorded test attempt. Latest-per-test selection MUST
occur within that configuration. Records from other configurations MUST be
identified as excluded historical coverage. Configuration selection MUST use
recorded metadata without building or executing tests.
:::

:::{req} index.freshness
Only records whose captured source inputs match the current project source and
whose build configurations are compatible MAY contribute execution counts to
the combined view of {cite}`requirements.test-coverage.index.selection`.
Records invalidated by a source change, or whose compatibility cannot be
established, MUST be identified as stale or unavailable and excluded from the
combined coverage totals. Their retained per-process reports MUST remain
inspectable using their captured source until the build artifacts are removed.
:::

:::{req} index.summary
The coverage index MUST display the combined view selected under
{cite}`requirements.test-coverage.index.selection` and
{cite}`requirements.test-coverage.index.freshness`. It MUST identify the
contributing runs and show each selected test's binary, test name, attempt,
capture time, command, selection, and outcome, including missing or stale
coverage. It MUST show source revisions and uncommitted-source state when
available rather than present records from several runs as one run.

For first-party Rust files under
{cite}`requirements.references.rust_item_coverage`, the summary MUST compute
line coverage from the union of instrumented file-and-line locations in the
selected fresh records, including their associated child-process records.
Each location MUST count once in the denominator and count as covered when any
selected record has a positive execution count for it. Percentages MUST be
computed from those union counts rather than averaged across process reports.
A view with no instrumented lines MUST show coverage as unavailable. The index
MUST link to {cite}`requirements.test-coverage.index.details`, visibly identify
incomplete coverage, and MUST NOT imply that every current test has coverage
merely because all available records are valid.
:::

:::{req} index.details
For the records displayed by {cite}`requirements.test-coverage.index.summary`,
`habeas mdbook` MUST generate a detail subchapter under the index chapter for
each recorded nextest test process, identifying its binary, test name, attempt,
test-attempt outcome, and originating run. Each report MUST show its instrumented
first-party Rust source lines and execution counts, distinguishing executed
from unexecuted lines, using the source captured for that run, and link back to
the index.
Coverage from associated child processes MUST remain distinguishable from
coverage recorded by the nextest test process itself.
:::

:::{req} index.paths
Generated coverage-report paths MUST be distinct and MUST be rejected when they
conflict with an existing book chapter.
:::

:::{req} index.unavailable
When no coverage run has been recorded, the coverage index MUST show a readable
message explaining how to collect one. Missing, unreadable, or incomplete run
data MUST produce a visible diagnostic in the index or affected process report
without preventing the rest of the book from rendering.
:::

:::{req} index.render_only
Building the book MUST NOT run tests or collect coverage.
:::

:::{req} setup
In addition to {cite}`requirements.cli.setup`, `habeas setup` MUST add a chapter
documenting the coverage collection command and containing `{coverage-index}`
to the book's `SUMMARY.md`, unless a rendered chapter already contains that
directive. Repeating setup MUST avoid duplicate chapters. It MUST preserve
existing nextest settings and differing chapter contents, reporting a
conflicting chapter path for review rather than overwriting it.
:::

:::{req} setup.configuration
`habeas setup` MUST configure native nextest settings in
`.config/nextest.toml` at the Cargo workspace root so the coverage workflow of
{cite}`requirements.test-coverage.run` routes each executed test attempt through
the per-process collector of {cite}`requirements.test-coverage.processes`.
Those settings MUST use nextest's own configuration format.
Repeating setup MUST avoid duplicate configuration, preserve unrelated nextest
settings, and report conflicting settings for review rather than overwrite them.
:::

:::{req} setup.validation
Repository validation, including the test calling `habeas::validate()` under
{cite}`requirements.integration.validate`, MUST check that the saved nextest
integration required by
{cite}`requirements.test-coverage.setup.configuration` is present and valid
for the coverage workflow's default profile, accounting for profile inheritance
and wrapper-rule precedence. A missing, unreadable, malformed, or incompatible
integration MUST be a validation error naming the affected file or setting and
directing the agent to repair it with `habeas setup`. The check MUST inspect the
collector settings and selection rules rather than only the file's existence.
Nextest's unrelated settings MUST retain their native meaning.
:::

:::{req} setup.validation.static
The setup validation of {cite}`requirements.test-coverage.setup.validation`
MUST NOT run tests, collect coverage, or require an existing completed coverage
run. It MUST work during the first instrumented test run, before that run's
profiles and reports have been written.
:::

:::{req} setup.bootstrap
:impl: assets/skills/habeas-bootstrap-requirements/SKILL.md
The `habeas-bootstrap-requirements` skill MUST direct agents to check the
coverage setup under {cite}`requirements.test-coverage.setup.validation` and
help establish or repair it with `habeas setup` before bootstrapping requirement
evidence. It MUST direct agents to report conflicting configuration for review
rather than overwrite project-specific nextest settings.
:::

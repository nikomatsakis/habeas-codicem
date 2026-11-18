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

:::{req} processes.progress
During report conversion, on a terminal enabled under
{cite}`requirements.test-coverage.processes.progress.terminal`, Habeas MUST
display updating progress with elapsed time and completed and total
test-attempt counts. The total MUST count the recorded attempts selected for
conversion. An attempt MUST advance the completed count only after its
conversion has finished or reported a collection failure, including processing
its associated child-process profiles. These counts MUST measure conversion
work regardless of the attempt's test outcome.
:::

:::{req} processes.progress.preparation
During coverage-data preparation, on a terminal enabled under
{cite}`requirements.test-coverage.processes.progress.terminal`, Habeas MUST
display updating activity and identify the current preparation stage.
:::

:::{req} processes.progress.terminal
Habeas coverage progress MUST be displayed when stderr is an interactive
terminal and `TERM` is not `dumb`. `NO_COLOR` MUST disable color without
disabling progress. When stderr is redirected or `TERM=dumb`, Habeas coverage
progress MUST NOT emit animation or terminal control sequences.

Temporary progress displays MUST be cleared when their stage ends, including
empty work and early collection failures. Final diagnostics and the coverage
artifact location MUST remain readable. Progress reporting MUST NOT change
coverage results, retained profiles, or command exit status.
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
`habeas mdbook` MUST automatically generate exactly one execution-coverage
subchapter under the requirements index of
{cite}`requirements.index.chapter`, whether that requirements index is
explicitly placed or automatically generated.
:::

:::{req} index.navigation
The requirements index MUST link to its execution-coverage subchapter under
{cite}`requirements.test-coverage.index`.
:::

:::{req} index.source
`habeas mdbook` MUST NOT require a separate authored coverage chapter or
`{coverage-index}` directive, or use such a directive to control generation
of the subchapter under {cite}`requirements.test-coverage.index`.
The generated coverage subchapter MUST have no authored source file or edit
target.
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
combined coverage totals. Their per-process reports and matching captured source
MUST remain retained in the build artifacts until those artifacts are removed.
:::

:::{req} index.summary
The coverage index MUST display the combined view selected under
{cite}`requirements.test-coverage.index.selection` and
{cite}`requirements.test-coverage.index.freshness`. It MUST show the number of
selected test attempts and summarize coverage excluded because it is stale,
unavailable, or from a historical build configuration. Detailed test identities,
run provenance, and per-test execution data MUST remain in the saved coverage
artifacts rather than being included in the generated book.

For first-party Rust files under
{cite}`requirements.references.rust_item_coverage`, the summary MUST compute
line coverage from the union of instrumented file-and-line locations in the
selected fresh records, including their associated child-process records.
Each location MUST count once in the denominator and count as covered when any
selected record has a positive execution count for it. Percentages MUST be
computed from those union counts rather than averaged across process reports.
A view with no instrumented lines MUST show coverage as unavailable. The index
MUST visibly identify incomplete coverage and MUST NOT imply that every current
test has coverage merely because all available records are valid.
:::

:::{req} index.file_map
A file coverage map MUST identify each file by its workspace-relative path and
show its covered and instrumented line counts and percentage. These counts MUST
use the union rule of {cite}`requirements.test-coverage.index.summary`,
restricted to the file and the records represented by that map, including
associated child processes. The map MUST visually distinguish coverage levels
while also displaying percentages as text. The percentage MUST describe the
map's recorded instrumented lines, rather than all physical source lines.

A listed file with instrumented lines but no positive execution counts MUST
show 0%. A listed file with no recorded instrumented lines in otherwise usable
coverage MUST show "not measured" without a percentage. Coverage whose
completeness cannot be established MUST be identified as unavailable rather
than as 0% or "not measured".
:::

:::{req} index.file_map.table
Each file coverage map MUST present its listed files in a compact table, with
one row per file and columns for its workspace-relative path and coverage
percentage or measurement status. Covered and instrumented line counts MUST
remain available in each measured file's row or expanded details.
The counts, statuses, and visual distinction of coverage levels MUST follow
{cite}`requirements.test-coverage.index.file_map`.
:::

:::{req} index.file_map.directories
The table of {cite}`requirements.test-coverage.index.file_map.table` MUST group
its listed files by their directory hierarchy. Each non-root directory
containing a listed file, directly or through descendants, MUST have a group.
Each group MUST be independently expandable and collapsible.

A collapsed group MUST show its workspace-relative path and the summaries
required by {cite}`requirements.test-coverage.index.file_map.minimum` and
{cite}`requirements.test-coverage.index.file_map.minimum.statuses`, while
hiding its file rows and descendant groups.
:::

:::{req} index.file_map.directories.initial
Directory groups under
{cite}`requirements.test-coverage.index.file_map.directories` MUST be expanded
initially in the combined gaps overview. File rows at the workspace root MUST
remain visible.
:::

:::{req} index.file_map.directories.data
Expanding or collapsing a directory group under
{cite}`requirements.test-coverage.index.file_map.directories` MUST NOT change
the files, coverage counts, or measurement statuses represented by the map.
:::

:::{req} index.file_map.minimum
Each directory group under
{cite}`requirements.test-coverage.index.file_map.directories` MUST display the
minimum measured file coverage percentage within its directory, including
descendant directories.

For the combined gaps overview, the minimum MUST consider all current
first-party Rust files represented by the map within that directory,
including files hidden because they have no unsatisfied obligations.
The minimum MUST use the file coverage fractions of
{cite}`requirements.test-coverage.index.file_map` before rounding for display,
regardless of which groups are expanded. Its label MUST identify it as the
minimum measured file coverage. Not-measured and unavailable files MUST NOT
contribute a percentage. A group with no measured files MUST show no numerical
minimum.
:::

:::{req} index.file_map.minimum.statuses
Each directory group under
{cite}`requirements.test-coverage.index.file_map.directories` MUST display
the number of its listed files, including descendants, that are not measured
whenever that count is nonzero. The status MUST follow
{cite}`requirements.test-coverage.index.file_map`. The count MUST describe
only files listed by the map, regardless of which groups are expanded.
:::

:::{example} index.file_map.minimum.descendants
A map lists `src/a.rs` with 1 of 2 instrumented lines covered,
`src/nested/b.rs` with 1 of 4 covered, and `src/nested/c.rs` as not measured.
Both the `src` and `src/nested` groups show a minimum measured file coverage
of 25% and one not-measured file, including while collapsed.
{test}`requirements.test-coverage.index.file_map.minimum`
{test}`requirements.test-coverage.index.file_map.minimum.statuses`
:::

:::{req} index.file_map.notes
Each listed first-party Rust file in the combined file coverage
map MUST provide access to a file-notes view identifying its
workspace-relative path.
:::

:::{req} index.file_map.notes.justifications
The view of {cite}`requirements.test-coverage.index.file_map.notes` MUST show
each `habeas:coverage` citation attached to the file root or an explicitly
declared function or method. Each entry MUST identify its source item and
source location and show the fenced citation commentary and Rust doc comments
supplying its tracked justification under
{cite}`requirements.test-coverage.check.exceptions.anchor`.
When no tracked justification is present, the entry MUST say so.

File-root citations MUST be labelled as file-threshold exception proposals;
function and method citations MUST be labelled as function exception proposals,
following {cite}`requirements.test-coverage.check.exceptions.anchor`.
:::

:::{req} index.file_map.notes.current
The view of {cite}`requirements.test-coverage.index.file_map.notes` MUST use
the current workspace source at book-render time and explicitly identify its
contents as current workspace source notes.
:::

:::{req} index.file_map.notes.availability
The view of {cite}`requirements.test-coverage.index.file_map.notes` MUST show
a readable statement when successfully read and parsed current source contains
no citations within the scope of
{cite}`requirements.test-coverage.index.file_map.notes.justifications`.

Missing, unreadable, or unparseable current source MUST instead produce a
visible diagnostic, without preventing the rest of the book from rendering.
These failures MUST NOT be presented as an absence of coverage explanations.
:::

:::{req} index.file_map.notes.reviews
Each citation entry under
{cite}`requirements.test-coverage.index.file_map.notes.justifications` MUST
show whether its source item's citation review is current and accepted under
{cite}`requirements.test-coverage.check.exceptions.reviews`.

Missing, unreadable, stale, invalid, or rejected reviews MUST NOT be presented
as accepted. The view MUST distinguish a missing review from a present review
that is not accepted and expose the validation findings explaining
non-acceptance.
:::

:::{req} index.file_map.notes.render_only
Rendering the view of
{cite}`requirements.test-coverage.index.file_map.notes` MUST NOT invoke an
assessment model. It MUST follow
{cite}`requirements.test-coverage.index.render_only`.
:::

:::{req} index.file_map.combined
The coverage index MUST present a gaps-focused file coverage map representing
all current first-party Rust files under
{cite}`requirements.references.rust_item_coverage` as its primary overview.
Its primary file rows MUST list only files with unsatisfied coverage
obligations under {cite}`requirements.test-coverage.check.threshold` or
{cite}`requirements.test-coverage.check.functions`. A file meeting its line
target MUST still be listed when it contains an unsatisfied function obligation.

Its coverage counts MUST use only records eligible for the combined view under
{cite}`requirements.test-coverage.index.selection` and
{cite}`requirements.test-coverage.index.freshness`, following
{cite}`requirements.test-coverage.index.file_map`.
:::

:::{req} index.file_map.combined.measurement
The combined gaps overview MUST visibly identify coverage excluded because it
is stale, missing, or unreadable. Measurement failures under
{cite}`requirements.test-coverage.check.measurement` MUST be reported separately
from uncovered file and function obligations. They MUST NOT be presented as
zero execution or as absence of individual function measurement in otherwise
usable coverage.

When the coverage check reports a measurement failure under
{cite}`requirements.test-coverage.check.measurement`, the combined overview
MUST show coverage as unavailable and omit file rows and directory groups.
Measurement findings MUST remain inspectable under
{cite}`requirements.test-coverage.index.gaps`. When no usable selected coverage
exists, the overview MUST retain the collection guidance of
{cite}`requirements.test-coverage.index.unavailable`. Numerical coverage MUST
NOT be fabricated from missing or incomplete data.
:::

:::{req} index.file_map.combined.details
The combined coverage overview MUST restrict its expandable details to coverage
gaps, explanation status, and coverage-check findings. It MUST NOT include a
test-run provenance table or embed per-test coverage data.
:::

:::{req} index.gaps
The combined coverage overview MUST identify whether the post-suite check of
{cite}`requirements.test-coverage.check` would pass or fail using the same
saved coverage and current exception evidence. Findings that cause the check
to fail MUST remain inspectable, including measurement, source discovery,
source parsing, and exception-review findings.
:::

:::{req} index.gaps.summary
The combined coverage overview MUST identify the numbers of unsatisfied file
and function obligations and group those obligations by source file.
Each directory summary MUST identify the number of unsatisfied obligations
within that directory and its descendants, regardless of which groups are
expanded.

When measurement or source discovery or parsing prevents establishing those
counts, the affected counts MUST be identified as unavailable or incomplete
rather than presented as complete counts. An empty list of established
unsatisfied obligations MUST NOT imply that the check passes when
{cite}`requirements.test-coverage.index.gaps` reports other failures.
:::

:::{req} index.gaps.details
In the combined gaps overview, clicking a listed file MUST expand or collapse
its uncovered obligations in place. Each expanded file MUST show each
obligation's source item and source location, measurement, and explanation
status.
:::

:::{req} index.gaps.details.initial
Per-file uncovered obligations under
{cite}`requirements.test-coverage.index.gaps.details` MUST be collapsed
initially.
:::

:::{req} index.gaps.explanations
Each unsatisfied obligation under
{cite}`requirements.test-coverage.index.gaps.summary` MUST distinguish a
missing `habeas:coverage` proposal from a present proposal whose review is
missing or is not current and accepted. When a present proposal contains no
tracked justification, the view MUST say so.

Present justification text and review validation findings MUST remain
inspectable following
{cite}`requirements.test-coverage.index.file_map.notes.justifications` and
{cite}`requirements.test-coverage.index.file_map.notes.reviews`.
:::

:::{req} index.gaps.accepted
Accepted exceptions actually used to satisfy current file or function
coverage obligations MUST be shown separately in an initially collapsed
documented-gaps section. Each entry MUST identify its source item and source
location, its uncovered measurement, its tracked justification, and its current
accepted review state, using current source notes under
{cite}`requirements.test-coverage.index.file_map.notes.current`.

Accepted exceptions MUST NOT increase reported execution-coverage percentages.
Citations on items whose measured coverage already satisfies their obligations
MUST NOT be listed as active gaps.
:::

:::{req} index.file_map.test
The coverage index MUST NOT offer per-test file coverage maps or a test
selector. Its file coverage map MUST focus on insufficient combined coverage
under {cite}`requirements.test-coverage.index.file_map.combined`.
:::

:::{req} index.details
`habeas mdbook` MUST NOT generate per-test coverage subchapters or captured-source
pages under the execution-coverage chapter of
{cite}`requirements.test-coverage.index`. File-notes pages supporting coverage
gap explanations under {cite}`requirements.test-coverage.index.file_map.notes`
MUST remain available.
:::

:::{req} index.paths
Generated coverage-index and coverage-report paths MUST be distinct.
The preprocessor MUST reject a generated coverage path that conflicts with
an existing book chapter, including another generated chapter.
:::

:::{req} index.unavailable
When no coverage run has been recorded, the coverage index MUST show a readable
message explaining how to collect one. Missing, unreadable, or incomplete run
data MUST produce a visible diagnostic in the index
without preventing the rest of the book from rendering.
:::

:::{req} index.render_only
Building the book MUST NOT run tests or collect coverage.
:::

:::{req} setup
In addition to {cite}`requirements.cli.setup`, `habeas setup` MUST enable the
automatic coverage subchapter of {cite}`requirements.test-coverage.index`
without creating a separate coverage source chapter, adding a coverage
entry to `SUMMARY.md`, or requiring a coverage source directive.
Setup MUST preserve existing authored chapter contents and `SUMMARY.md`.
:::

:::{req} index.guidance
The generated coverage subchapter MUST make coverage-collection and
post-suite-checking guidance available under
{cite}`requirements.test-coverage.check.guidance`, without requiring
separate authored content.
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

## Post-suite coverage check

Stricter coverage policy for security-sensitive code, including how projects
identify that code, is deferred to
[issue #47](https://github.com/nikomatsakis/habeas-codicem/issues/47).

:::{req} check
`habeas coverage-check` MUST check saved execution coverage against
{cite}`requirements.test-coverage.check.threshold` and
{cite}`requirements.test-coverage.check.functions`, returning success only when
every applicable obligation is satisfied or has accepted exception evidence
under {cite}`requirements.test-coverage.check.exceptions`. The command MUST NOT
build the project, execute tests, collect coverage, or invoke an assessment model.
:::

:::{req} test
`habeas test` MUST run the full-workspace instrumented nextest workflow of
{cite}`requirements.test-coverage.run` and, only after that workflow succeeds,
run the post-suite check of {cite}`requirements.test-coverage.check`. It MUST
return a failing exit status if test execution, coverage collection, or the
post-suite check fails. It MUST NOT author exception evidence or invoke an
assessment model.
:::

:::{req} check.scope
The coverage check MUST consider every explicitly declared function or method
with a body in the first-party Rust sources of
{cite}`requirements.references.rust_item_coverage`, including test functions,
test helpers, and default trait methods. Declarations without bodies MUST NOT
create execution-coverage obligations. Source discovery or parsing failures
MUST cause the check to fail rather than silently omit functions.

A function whose measurement cannot be established MUST be reported as not
measured rather than silently excluded. Functions excluded by the selected
build configuration require exception evidence under
{cite}`requirements.test-coverage.check.exceptions`.
:::

:::{req} check.threshold
Each first-party Rust file under
{cite}`requirements.references.rust_item_coverage` MUST have at least 80%
execution coverage of its instrumented lines, or current accepted exception
evidence explaining why the file falls below that target under
{cite}`requirements.test-coverage.check.exceptions`. Counts MUST use the
file-and-line union rule of {cite}`requirements.test-coverage.index.summary`;
a line counts as executed when any eligible test process or its associated
child processes has a positive execution count for it.

The check MUST use the combined selection and freshness rules of
{cite}`requirements.test-coverage.index.selection` and
{cite}`requirements.test-coverage.index.freshness`, restricted to the active
build configuration. It MUST compare exact counts rather than rounded displayed
percentages, average file percentages, or impose an 80% target within each
function. A file whose executable code is not measured MUST require exception
evidence rather than pass with a fabricated percentage. A file established to
contain no executable coverage obligations MUST be reported as such and MUST
NOT require a threshold exception.
Absence of recorded instrumented lines alone MUST NOT establish that a file
contains no executable coverage obligations.
:::

:::{req} check.functions
Every function under {cite}`requirements.test-coverage.check.scope` without
recorded execution attributable to it MUST have current accepted exception
evidence under {cite}`requirements.test-coverage.check.exceptions`, even when
its containing file meets the target of
{cite}`requirements.test-coverage.check.threshold`.

Execution attributable only to another function, including a nested function
or closure, MUST NOT establish coverage of the enclosing function merely
because their source locations overlap. Having no recorded lines alone MUST
NOT establish that a function has no executable coverage obligations.
:::

:::{req} check.measurement
The coverage check MUST fail when no usable coverage has been collected, or when
a selected latest attempt has missing, unreadable, or stale coverage. It MUST
distinguish these measurement failures from measured lines with zero execution
and from individual functions absent from otherwise usable coverage. Exception
evidence MUST NOT excuse measurement failures or permit substitution of an
older attempt. Excluded historical runs and incompatible configurations MUST
NOT by themselves cause failure.

Coverage whose format does not support the function attribution required by
{cite}`requirements.test-coverage.check.functions` MUST be rejected with guidance
to recollect coverage. When missing or corrupt metadata prevents establishing
the latest-per-test selection or active configuration, the check MUST fail
rather than silently use remaining records. Historical data MAY be disregarded
without failure only when its exclusion can be established.
:::

:::{req} check.exceptions
An unmet file threshold or an unexecuted or not-measured function MAY satisfy
the coverage policy only
through current, accepted, independently assessed evidence explaining why its
coverage is incomplete. An exception MUST be attached to the source file or
function whose incomplete coverage it justifies. A file-threshold exception
MUST NOT automatically excuse untested functions.
An exception MUST NOT excuse another source item's incomplete coverage.
Changes to the source item, justification, or other inputs
of its citation review MUST invalidate the affected exception under
{cite}`requirements.references.citation_review`.

A source comment or unassessed justification alone MUST NOT count as accepted
exception evidence. Function and gap identities MUST remain portable between
equivalent local and CI checkouts rather than depend on checkout paths,
run identifiers, timestamps, or process identifiers.
:::

:::{req} check.exceptions.anchor
Habeas MUST provide the built-in `habeas:coverage` under
{cite}`requirements.references.builtin_definitions`, with this text:

> Cite this on a source file or explicitly declared function to propose a
> coverage exception. Explain in prose why the item's incomplete execution
> coverage is acceptable. Put the explanation in fenced citation commentary
> or Rust doc comments included in the source item's assessed input. An ordinary
> non-doc comment alone does not supply a tracked justification. The reason
> must be substantive; difficulty alone is not a justification.
> On a file's root item, the exception excuses only
> that file's 80% instrumented-line threshold, including absent measurement
> of its executable code. On an explicit function with a body, it excuses
> that function's recorded zero execution or absence from otherwise usable
> measurement, without excusing the file's threshold.
> The explanation must agree with the current source and any callers or
> configuration assumptions on which it relies. Only a current accepted
> independent assessment permits the exception. A file exception does not
> excuse unexecuted or not-measured functions in that file.
> The citation applies to its source item without conditional build matching
> and may remain when measured coverage satisfies the item's obligations.

Only `{cite}` MAY reference this built-in, inline or fenced using the standard
reference forms of {cite}`requirements.references.roles`. The built-in MUST be
available in every project without a project requirement, site query, or
exception-specific configuration.
:::

:::{req} check.exceptions.reviews
Exception proposals MUST be `{cite}` references to `habeas:coverage`
under {cite}`requirements.test-coverage.check.exceptions.anchor`. Each proposal
MUST have a current, accepted citation review of its source item under
{cite}`requirements.references.citation_review`. The reviewer MUST assess
whether the justification adequately explains the proposed exception, not merely
whether the citation has the right role. An acknowledgment of missing coverage
without an adequate justification MUST NOT authorize an exception.

File-threshold proposals MUST belong to the source file's root item. Proposals
concerning unexecuted or not-measured functions MUST belong to their explicitly
declared function's item. One accepted function proposal MUST cover both zero execution
and absence from otherwise usable measurement, without separate exception kinds.
A proposal in a nested function MUST NOT authorize exceptions
for its enclosing function, and an enclosing function's proposal MUST NOT
authorize exceptions for nested functions.
:::

:::{req} check.exceptions.binding
The coverage check MUST use the citation's source attachment and current
accepted review to identify an exception. It MUST NOT require structured
exception metadata, match exceptions to targets, profiles, or feature sets, or
interpret build conditions in prose as machine-enforced restrictions.
A citation with a current accepted review MUST remain eligible across build
configurations even when its prose describes a condition specific to one
configuration. Satisfying the item's measured coverage obligations MUST NOT
by itself invalidate the citation or make its presence a coverage-check error.
:::

:::{req} check.exceptions.author
The `habeas author` workflow MUST support authoring and refreshing exception
evidence under {cite}`requirements.test-coverage.check.exceptions`. Assessment
inputs MUST include the current file or function source, the built-in's fixed
text, and its citation commentary under
{cite}`requirements.references.citation_review`.

Justifications and their assessment evidence MUST be retainable in Git
independently of the disposable coverage artifacts under
{cite}`requirements.test-coverage.storage`. Raw profiles, coverage reports, and
captured source snapshots MUST NOT be required in Git to validate an exception
against freshly collected coverage.
:::

:::{req} check.exceptions.setup
Setup-generated coverage documentation and bundled agent instructions MUST
explain how to write a standard `habeas:coverage` citation with a prose
justification under
{cite}`requirements.test-coverage.check.exceptions.anchor` and author or refresh
its citation review using `habeas author`. The instructions MUST explain that
the anchor requires no project declaration or exception-specific configuration,
that only current accepted citation evidence permits an exception, and that
ordinary partially uncovered functions need none when their file meets its
target.
:::

:::{req} check.report
The coverage check MUST show each file's measured coverage and threshold result,
identify each unsatisfied function by source file and function, list uncovered
locations or absence of measurement, and explain missing, stale, or rejected
exception evidence. It MUST report accepted
exceptions separately from executed coverage; accepting an exception MUST NOT
increase any displayed execution-coverage percentage.
:::

:::{req} check.guidance
Habeas's bundled agent instructions and setup-generated coverage documentation
MUST recommend running `habeas coverage-check` after a successful full-workspace
instrumented nextest run using `habeas coverage -- --workspace`, as a separate
CI step. They MUST document `habeas test` as the command running collection and
post-suite checking in order under {cite}`requirements.test-coverage.test`.

The static validation test MUST NOT require current collected execution
coverage or validate exception applicability against current runtime gaps.
Those checks belong to the post-suite coverage command. Coverage checking
MUST NOT depend on running the static validation test last.
:::

:::{req} setup.bootstrap
:impl: assets/skills/habeas-bootstrap-requirements/SKILL.md
The `habeas-bootstrap-requirements` skill MUST direct agents to check the
coverage setup under {cite}`requirements.test-coverage.setup.validation` and
help establish or repair it with `habeas setup` before bootstrapping requirement
evidence. It MUST direct agents to report conflicting configuration for review
rather than overwrite project-specific nextest settings.
:::

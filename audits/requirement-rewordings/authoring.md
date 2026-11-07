# Requirement rewordings: `requirements.authoring.*`

Each entry records a change to `md/requirements/authoring.md` made while
resolving false evidence, with the evidence rationale that prompted it.

## `requirements.authoring.batch` — interruption semantics

**Old text** (after the read-denial sentence, nothing about interruption):

> An access-policy denial MUST abort only the affected assessment under
> \{cite}`requirements.authoring.read_denial`.

**New text:**

> An access-policy denial MUST abort only the affected assessment under
> \{cite}`requirements.authoring.read_denial`. Interruption MUST instead stop the
> whole batch: queued assessments MUST NOT start, running assessments MUST be
> cancelled, and an assessment whose evidence has not been written when the
> interruption arrives MUST NOT write it. Evidence already written MUST be
> retained, and the command MUST report the number of assessments not started as
> a batch failure.

**Reason.** On Ctrl-C, `author_all_with_options` and `cancel_on_error` in
`src/authoring/batch.rs` stop launching queued work, cancel running
assessments, discard judgments that arrive after the stop signal without
writing them, keep evidence already written, and report "N assessments not
started" as a `batch` failure. This was intended behavior, and
`interrupted_terminal_progress_keeps_unstarted_work_incomplete` already tested
it. No requirement stated it, though: `requirements.authoring.progress` only
says an interrupted batch keeps its actual progress. The new sentence records
the existing behavior as an obligation, so the `{impl}` code no longer has
unspecified behavior. It also states the contrast with read denials, which
abort only one assessment. No existing obligation was weakened.

**Prompting rationale:** `evidence/requirements.authoring.batch.json`, gap 1:
"Cancellation behavior is not specified by this requirement … this stop and
discard semantic has no clear specifying definition."

## `requirements.authoring.batch` — read and scan failures

**Old text:**

> Failure to discover declarations or parse project sources MUST report an error,
> not a successful empty batch.

**New text:**

> Failure to discover declarations or to read, scan, or parse project sources MUST report an error,
> not a successful empty batch.

**Reason.** The rationale found that unreadable reference sources and
directories ("cannot scan references", "cannot read reference source" in
`collect_references_with_progress`) did not abort a batch. The code was fixed:
`inspect_repository_with_progress` now treats them as discovery errors, and
`unreadable_reference_sources_fail_the_batch_before_inference` tests this. The
old wording said only "discover … or parse", which left it unclear whether read
and scan failures were covered. Naming them makes the obligation that the fix
implements explicit, and the obligation is stronger, not weaker.

**Prompting rationale:** `evidence/requirements.authoring.batch.json`, gap 2:
"Discovery and parse-failure handling is incomplete … A batch would proceed
despite them, although the requirement says failure to discover or parse
project sources must report an error and not succeed."

## `requirements.authoring.read_denial` — requests outside the project

**Old text:** the requirement ended with:

> Malformed tool arguments and ordinary read failures MUST retain their existing
> recoverable tool-error behavior.

**New text:** one paragraph appended:

> A dispatched model request naming a path outside the project root, before
> symlinks are resolved, that no workspace allow rule or generated-output rule
> permits MUST be treated as an ordinary read failure rather than an abort: it
> MUST return an explanatory tool error, add no evidence input, and let the
> assessment continue. An explicit workspace disallow matching such a path MUST
> still abort. A mandatory input naming such a path, and a request inside the
> project whose symlink target leaves it, MUST still abort.

**Reason.** Aborting on a denied read exists so that the operator fixes
project policy, either by staging an untracked file or by adding a workspace
allow or disallow rule. When a model reaches outside the project, for example
into `~/.cargo/registry/src` to check a dependency's behavior, it is exploring.
Neither the operator nor the model can fix that by staging a file, and aborting
throws away the whole assessment. This made
`requirements.authoring.provider_backoff` repeatedly impossible to assess. The
cases that do concern policy still abort: explicit disallows, in-project
exclusions (untracked files, `evidence/`, credential-bearing workspace
configuration), in-project symlinks whose targets leave the project, and
mandatory inputs. The change is implemented in `src/authoring/access.rs`
(`OutsideProject`) and `tools::dispatch`, and tested by
`unpermitted_requests_outside_the_project_are_recoverable_tool_errors` and
`mandatory_inputs_outside_the_project_still_abort`.

**Prompting rationale:** `requirements.authoring.provider_backoff` assessments
aborted with a read_denial error after the model asked to read
`/root/.cargo/registry/src`, which is outside the project.

## `requirements.authoring.source_tools` — consistency with read_denial

**Old text:**

> Access-policy denials MUST abort;
> other failed reads MUST return an explanatory tool error.

**New text:**

> Access-policy denials MUST abort
> except where \{cite}`requirements.authoring.read_denial` makes them ordinary read
> failures; other failed reads MUST return an explanatory tool error.

**Reason.** Keeps this requirement consistent with the read_denial exception
above, so the two requirements do not contradict each other.

**Prompting rationale:** the same provider_backoff read_denial abort.

## `requirements.authoring.outline_tool`, `file_discovery`, `text_search` — defer to read_denial

**Old text:**
- outline_tool: "Explicitly requesting an excluded file MUST abort under
  \{cite}`requirements.authoring.read_denial`."
- file_discovery: "…unless access was denied, which MUST abort under
  \{cite}`requirements.authoring.read_denial`."
- text_search: "…explicitly requesting an excluded path MUST abort under
  \{cite}`requirements.authoring.read_denial`."

**New text:**
- outline_tool: "…MUST be handled as a denial under
  \{cite}`requirements.authoring.read_denial`."
- file_discovery: "…which MUST be handled under
  \{cite}`requirements.authoring.read_denial`."
- text_search: "…MUST be handled as a denial under
  \{cite}`requirements.authoring.read_denial`."

**Reason.** Each of these restated "abort", which would contradict the new
read_denial exception for unpermitted paths outside the project. They now defer
to read_denial, which keeps the abort for every in-project and policy-relevant
denial.

**Prompting rationale:** the same provider_backoff read_denial abort.

## `requirements.authoring.initial_prompt` — carry all of review_criteria

**Old text:**

> Its review guidance MUST follow \{cite}`requirements.ci.review_criteria`,

> completely and exactly as stated, and that tests cover the interesting points in the design space.

> specifies it. If there are no tests,

> a Markdown rationale with source locations, including any acknowledged gaps.

**New text:**

> Its review guidance MUST follow \{impl}`requirements.ci.review_criteria`,

> completely and exactly as stated, and that tests cover the interesting points in
> the design space, including relevant boundaries and failure cases.

> specifies it. When another requirement implements part of the subject, assess
> whether its text carries out that part; do not reassess its implementation,
> tests, or evidence. If there are no tests,

> a Markdown rationale with source locations, including any acknowledged gaps; for
> a requirement, explain the implementation and testing judgments.

**Reason.** The template is the text that carries out `review_criteria` for the
model, so the requirement implements part of it rather than only citing it. Three
obligations of `review_criteria` were missing from the template: test coverage of
relevant boundaries and failure cases, the rule for parts implemented by another
requirement, and explaining the implementation and testing judgments. The
template now states each of them. `initial_prompt_carries_the_review_criteria`
in `src/authoring/mod.rs` asserts the new phrases.

**Prompting rationale:** `evidence/requirements.ci.review_criteria.json`: "The
prompt template has no such instruction [for another requirement implementing part
of the subject]", "The requirement says tests must cover 'relevant boundaries and
failure cases'. The template says only 'interesting points in the design space'",
"The requirement says the rationale must explain 'the implementation and testing
judgments' … The template only asks to 'explain your judgment … with source
locations'", and "`requirements.authoring.initial_prompt` only `{cite}`s
`review_criteria`".

## "Editing the file" — mark and complete the configuration examples

**Old text** (prose after `requirements.authoring.configuration_documentation`):

> directory readable while excluding its private subtree:

> Set `concurrency` alongside `backend`
> and `model` to override the provider's default number of simultaneous assessments:
>
> ```toml
> [authoring.configs.cloud]
> backend = "bedrock"
> model = "MODEL_ID"
> concurrency = 4
> ```

**New text:**

> directory readable while excluding its private subtree:
> \{impl}`requirements.configuration.documentation`
> \{impl}`requirements.authoring.configuration_documentation`

> Set `concurrency` alongside `backend`
> and `model` to override the provider's default number of simultaneous assessments.
> This complete user `config.toml` sets it for the `cloud` configuration; the same
> table can appear in a workspace `habeas.toml`:
>
> ```toml
> version = 1
>
> [authoring]
> default = "cloud"
>
> [authoring.configs.cloud]
> backend = "bedrock"
> model = "MODEL_ID"
> concurrency = 4
> ```

**Reason.** The "Editing the file" section documents the authoring configuration,
including the only explicit-credential example, but was not marked as
implementing the documentation requirements; the reference is in the section, so
it covers all of its examples. The `concurrency` example was a fragment with no
`version = 1` and no file named. It is now a complete, named file.
`documented_configuration_examples_are_complete_valid_files` in
`tests/configuration.rs` now also parses every TOML example in this chapter and
checks that the literal-credential example uses only uppercase placeholders.

**Prompting rationale:** `evidence/requirements.configuration.documentation.json`:
"The only example of explicit credential fields … is in
`md/requirements/authoring.md`, 'Editing the file'. That section carries only a
`{cite}` of this requirement", "The test excludes `md/requirements/authoring.md`,
so that document's TOML examples are unchecked", and "`md/requirements/authoring.md`
also has a `concurrency` fragment that is not a complete file. It starts at
`[authoring.configs.cloud]`, has no `version = 1`, and does not name the file to
edit."

## `requirements.authoring.configuration` — refine feature_sections

**Old text:**

> Authoring MUST use the `[authoring]` sections of the shared user configuration
> and the selected workspace's `habeas.toml`.
> \{cite}`requirements.configuration.user_location`
> \{cite}`requirements.configuration.structure`
> \{cite}`requirements.configuration.feature_sections`
> It MUST support named configurations, …

**New text:**

> Authoring MUST use the `[authoring]` sections of the shared user configuration
> and the selected workspace's `habeas.toml`, refining
> \{impl}`requirements.configuration.feature_sections` for the authoring feature.
> \{cite}`requirements.configuration.user_location`
> \{cite}`requirements.configuration.structure`
> Assessing a subject with `habeas author`, singly or in a batch that needs at
> least one assessment, MUST require an `[authoring]` section in at least one of
> these files; either section MAY be absent. No other operation requires the
> section, including a batch with no assessments needed and the operations in
> \{cite}`requirements.authoring.deterministic_validation`.
> It MUST support named configurations, …

**Reason.** The authoring configuration requirements define the authoring
feature's sections, types, defaults, validation, and precedence, which is what
`feature_sections` asks each feature to specify, so this requirement refines it.
`feature_sections` also requires each feature to state which operations require
its section; for authoring this was only implied by the missing-configuration
error. Only `resolve_authoring`, called by single-subject authoring and by a
batch with work to do, requires the section; the batch requirement and
`deterministic_validation` already exempt the other operations.

**Prompting rationale:** `evidence/requirements.configuration.feature_sections.json`:
"The authoring spec in `md/requirements/authoring.md` … only `{cite}`s
`feature_sections`, once, in `configuration`. It has no `{impl}` or `{test}`
relationship to it", and "The authoring spec never states which operations
require the `[authoring]` section. The requirement obliges each feature to say
this explicitly."

## `requirements.authoring.concurrency` — throttling is retried, not failed

**Old text:**

> Provider errors, including throttling, MUST be reported through
> the ordinary assessment-failure path.

**New text:**

> Transient provider failures, including throttling, MUST be
> retried within the affected assessment under
> \{cite}`requirements.authoring.provider_backoff`; every other provider error MUST
> be reported through the ordinary assessment-failure path.

**Reason.** The old sentence contradicted `provider_backoff`, which requires
throttling and other transient failures to be retried with no attempt limit and
not reported as terminal assessment errors while recovery is in progress. The
intended obligation of this sentence was that provider errors in a batch are
handled per assessment rather than aborting the batch; the new text keeps that
for permanent errors and defers transient ones to the backoff requirement.

**Prompting rationale:** `evidence/requirements.authoring.concurrency.json`:
"`ProviderBackoff::next_delay` … treats HTTP 429 and `ThrottlingException` as
transient and retries them … So a throttled assessment never ends in the
failure path … The two requirements contradict each other".

## `requirements.authoring.provider_backoff` — retry temporary unavailability and unanswered requests (commit 69997e4)

**Old:** "When a provider reports transient throttling through HTTP 429 or a Bedrock `ThrottlingException`, the harness MUST wait and retry the same completion request, retaining…"

**New:** "When a provider reports a transient failure, the harness MUST wait and retry the same completion request. Transient failures are throttling, reported through HTTP 429 or a Bedrock `ThrottlingException`; temporary unavailability, reported through HTTP 500, 502, 503, or 504 or a Bedrock `ServiceUnavailableException`, `InternalServerException`, or `ModelNotReadyException`; and a request with no response after ten minutes, which the harness MUST abandon. The retry MUST retain…" The backoff window now doubles "after each consecutive transient failure".

**Reason:** a full authoring run lost four assessments to Bedrock `503 Service Unavailable` and one waited about four hours on a request that never returned. The obligation is broadened, not weakened. Requested by the user.

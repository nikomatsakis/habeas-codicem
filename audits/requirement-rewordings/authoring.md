# Requirement rewordings: `requirements.authoring.*`

Each entry records a change to `md/requirements/authoring.md` made while
resolving false evidence, with the evidence rationale that prompted it.

## `requirements.authoring.batch` — interruption semantics

**Old text** (after the read-denial sentence, nothing about interruption):

> An access-policy denial MUST abort only the affected assessment under
> {cite}`requirements.authoring.read_denial`.

**New text:**

> An access-policy denial MUST abort only the affected assessment under
> {cite}`requirements.authoring.read_denial`. Interruption MUST instead stop the
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
> except where {cite}`requirements.authoring.read_denial` makes them ordinary read
> failures; other failed reads MUST return an explanatory tool error.

**Reason.** Keeps this requirement consistent with the read_denial exception
above, so the two requirements do not contradict each other.

**Prompting rationale:** the same provider_backoff read_denial abort.

## `requirements.authoring.outline_tool`, `file_discovery`, `text_search` — defer to read_denial

**Old text:**
- outline_tool: "Explicitly requesting an excluded file MUST abort under
  {cite}`requirements.authoring.read_denial`."
- file_discovery: "…unless access was denied, which MUST abort under
  {cite}`requirements.authoring.read_denial`."
- text_search: "…explicitly requesting an excluded path MUST abort under
  {cite}`requirements.authoring.read_denial`."

**New text:**
- outline_tool: "…MUST be handled as a denial under
  {cite}`requirements.authoring.read_denial`."
- file_discovery: "…which MUST be handled under
  {cite}`requirements.authoring.read_denial`."
- text_search: "…MUST be handled as a denial under
  {cite}`requirements.authoring.read_denial`."

**Reason.** Each of these restated "abort", which would contradict the new
read_denial exception for unpermitted paths outside the project. They now defer
to read_denial, which keeps the abort for every in-project and policy-relevant
denial.

**Prompting rationale:** the same provider_backoff read_denial abort.

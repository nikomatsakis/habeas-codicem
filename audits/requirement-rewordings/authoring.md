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

# Refactoring

Moving a requirement to another chapter, or source code to another file, changes
the addresses that evidence records but not what any assessment reviewed.
`habeas refactor` rewrites those addresses after the move, so evidence whose
content is unchanged stays current without reassessment. It covers moves only:
renaming a function or changing any text the evidence recorded still needs
reassessment.

## Invocation

:::{req} command
`habeas refactor --map <old>=<new> [--map <old>=<new>]... [--dry-run]` MUST
update the repository after the user has already moved the mapped items. It
MUST NOT move or edit code or directives itself, except as
{cite}`requirements.refactor.reference_rewrite` specifies. It MUST list each
record it changed and each record a mapping affects that it left unchanged,
with the reason, and MUST succeed when every mapping is valid.
:::

:::{req} dry_run
With `--dry-run`, `habeas refactor` MUST report every change it would make,
in the same form as {cite}`requirements.refactor.command`, and MUST write
nothing.
:::

:::{req} mappings
Each `--map` MUST have one of these forms:

- a file or directory path, which maps that file, or every file under that
  directory, to the same relative path under the new side;
- `<file>#<selector>`, which maps one outline node, and its outline
  descendants, to the node at the new file and selector;
- a definition name, which maps that full name, and every full name beginning
  with it followed by `.`, to the same suffix under the new name.

A mapping whose old side names a recorded definition, or a prefix of one
followed by `.`, MUST be a definition-name mapping.
:::

:::{req} invalid_mappings
`habeas refactor` MUST fail with a usage error, before changing anything, for a
malformed mapping, a mapping whose old side could be read as both a path and a
definition name, two mappings with overlapping old sides, or a mapping whose
new side does not exist.
:::

:::{req} reference_rewrite
For a definition-name mapping, `habeas refactor` MUST rewrite each `{impl}`,
`{test}`, `{cite}`, `{fixme}`, and `{term}` reference that names an old name,
in the book and in the sources reference discovery considers. For a path or
node mapping, it MUST rewrite each `:impl:` or `:test:` anchor that names an
old file or node. It MUST change nothing else in those files.
:::

## Verifying that only addresses changed

:::{definition} mapped_back
The *mapped-back* form of current text or a current address is the text or
address with each new side of the mappings replaced by its old side.
:::

:::{req} outline_inputs
An outline input MUST be rewritten only if the mapped node exists and its
structural hash, computed under
{cite}`requirements.references.structural_hash` over the
{term}`requirements.refactor.mapped_back` text of the node, equals the recorded
hash.
:::

:::{req} definition_inputs
A requirements or definition input MUST be rewritten only if the
{term}`requirements.refactor.mapped_back` current text of each definition it
records equals the recorded text and, for a requirements input, the mapped
selection's current matches are exactly the recorded matches with the mapping
applied.
:::

:::{req} subject_text
A record's `requirement` or `example` field MUST be rewritten only if the
{term}`requirements.refactor.mapped_back` current body of its subject equals
the recorded body.
:::

:::{req} reference_digest
A record's `references` digest MUST be rewritten only if the
{term}`requirements.refactor.mapped_back` current reference set of its subject,
under {cite}`requirements.ci.reference_set`, has the recorded digest.
:::

:::{req} whole_record
A record MUST be changed only if every rewrite a mapping calls for in it
verifies under {cite}`requirements.refactor.outline_inputs`,
{cite}`requirements.refactor.definition_inputs`,
{cite}`requirements.refactor.subject_text`, and
{cite}`requirements.refactor.reference_digest`; otherwise the whole record MUST
be left unchanged and reported with the check that failed. Records that no
mapping affects MUST be left byte-identical.
:::

:::{req} equivalence
After `habeas refactor` succeeds, each record it changed MUST pass or fail
validation exactly as it did before the move.
:::

## Rewriting records

:::{req} record_files
For each changed record, `habeas refactor` MUST move its evidence file,
citation review file under {cite}`requirements.references.citation_review`, or
site review file under {cite}`requirements.references.site_review` to the path
its mapped subject, file, and selector determine. It MUST NOT overwrite an
existing file; it MUST leave that record unchanged and report the conflict.
:::

:::{req} record_fields
For each changed record, `habeas refactor` MUST rewrite every recorded file,
selector, definition name, definition selection, and match the mappings cover,
including the `citations`, `requirement`, `file`, and `item` fields of citation
and site reviews, and MUST record the current text, hash, and reference-set
digest where it rewrote the corresponding field.
:::

## Rewriting rationales

A rationale is free Markdown that validation never checks, so these rewrites
only keep it readable. Only exact backtick-delimited code spans are rewritten,
the form {cite}`requirements.authoring.judgment_rules` asks for.

:::{req} rationale.names
In the rationale of each changed record, `habeas refactor` MUST rewrite each
code span that is exactly an old definition name, or a name beneath one, to its
new name.
:::

:::{req} rationale.paths
In the rationale of each changed record, `habeas refactor` MUST rewrite each
code span that is exactly a mapped file path, or a path beneath a mapped
directory, to its new path.
:::

:::{req} rationale.addresses
In the rationale of each changed record, `habeas refactor` MUST rewrite each
code span that is exactly `<file>#<selector>` for a mapped node, or for a node
in a mapped file, to the node's new address.
:::

:::{req} rationale.untouched
`habeas refactor` MUST NOT change rationale text other than the spans
{cite}`requirements.refactor.rationale.names`,
{cite}`requirements.refactor.rationale.paths`, and
{cite}`requirements.refactor.rationale.addresses` cover. For each changed
record, it MUST list the other code spans in the rationale that contain an old
side of a path or node mapping, so the user can review them.
:::

## Examples

:::{example} moved_function
`fn[load]` moves unchanged from `src/config.rs` to `src/config/load.rs`, and
nothing else changes. `habeas refactor --map 'src/config.rs#fn[load]=src/config/load.rs#fn[load]'`
rewrites the outline inputs that read it, recomputes the reference-set digests
of the requirements it implements, rewrites `` `src/config.rs#fn[load]` `` in
their rationales, and validation then passes without calling a model.
{cite}`requirements.refactor.outline_inputs`
{cite}`requirements.refactor.reference_digest`
{cite}`requirements.refactor.rationale.addresses`
{cite}`requirements.refactor.equivalence`
:::

:::{example} moved_requirement
`requirements.cli.check` moves to the `core` chapter, becoming
`requirements.core.check`; the `{impl}`, `{test}`, and `{cite}` references to
it still use the old name. `habeas refactor --map requirements.cli.check=requirements.core.check`
rewrites those references, renames its evidence file, rewrites the requirements
inputs of records that read it, and leaves unchanged a record whose input
selects `requirements.cli.*`, because that selection no longer matches what it
recorded. {cite}`requirements.refactor.reference_rewrite`
{cite}`requirements.refactor.record_files`
{cite}`requirements.refactor.definition_inputs`
:::

:::{example} edited_while_moving
`fn[load]` moves to `src/config/load.rs` and its body is also edited. The
mapped node's hash differs from the recorded one, so `habeas refactor` leaves
every record that read it unchanged and reports each one; those records stay
stale until `habeas author` reassesses them.
{cite}`requirements.refactor.outline_inputs`
{cite}`requirements.refactor.whole_record`
:::

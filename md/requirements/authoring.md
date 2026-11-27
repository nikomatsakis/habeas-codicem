# Authoring evidence

Habeas gives a model a requirement or example and its mandatory source inputs.
The model can discover files, request more inputs, and submit one judgment. The
harness records the inputs and writes the judgment into the existing evidence
format. For a failed definition assessment, file-output authoring can apply
missing reference markers and reassess subjects whose inputs changed. Rig provides the connection to the user's configured model.

:::{req} invocation
`habeas author <full-name> [--config <name>] [--missing | --force] [--stdout] [-v|--verbose]`
MUST initially select one existing requirement or named example in the current
project under {cite}`requirements.authoring.refresh`. Subsequent assessments
follow {cite}`requirements.authoring.reassessment`. It MUST report the
selected subject, backend, model, evidence destination, and usage-record path on
completion. The evidence destination MUST identify the evidence-file path by
default or stdout when `--stdout` is selected. In stdout mode, the completion
report and diagnostics MUST go to stderr. `author-evidence` MUST remain an alias
for the single-subject command.
:::

:::{req} batch
:impl: assets/skills/habeas/SKILL.md
:impl: assets/skills/habeas-requirements/SKILL.md
:impl: assets/skills/habeas-evidence/SKILL.md
:impl: assets/skills/habeas-bootstrap-requirements/SKILL.md
`habeas author --all [--config <name>] [--missing | --force] [-v|--verbose]` MUST author
the evidence of every current requirement and named example, together with the
reviews {cite}`requirements.references.citation_review` and
{cite}`requirements.references.site_review` include, under
{cite}`requirements.authoring.refresh`.
`--all` MUST be mutually exclusive with a subject name and `--stdout`. It MUST
also delete each orphaned citation review under
{cite}`requirements.references.citation_review` and each orphaned site review
under {cite}`requirements.references.site_review`, and report each deletion; the
deletion MUST NOT require model configuration or credentials.
Failure to discover declarations or to read, scan, or parse project sources MUST report an error,
not a successful empty batch.

Each subject MUST receive an independent assessment with its own prompt, inputs,
judgment, and usage record. The command MUST report aggregate completion and each
authoring error under {cite}`requirements.authoring.reporting`, and retain
successful writes when another assessment fails. It MUST return a nonzero
exit status if any assessment fails. False judgments MUST still be written as
completed assessments; they remain failures under ordinary evidence validation.
A denied mandatory input MUST abort only the affected assessment under
{cite}`requirements.authoring.read_denial.mandatory`. Interruption MUST instead stop the
whole batch: queued assessments MUST NOT start, running assessments MUST be
cancelled, and an assessment whose evidence has not been written when the
interruption is observed MUST NOT begin committing it. After preparing a
temporary evidence file, the harness MUST check interruption again before its
atomic replacement; a replacement already in progress MAY finish. Evidence already written MUST be
retained, and the command MUST report the number of assessments not started as
a batch failure.

The bundled Habeas skills MUST direct the supervising agent to run this command
to create or refresh evidence. They MUST leave judgments, rationales, and input
snapshots to the authoring harness; the supervising agent addresses outstanding manual findings by
changing requirements, source citations, implementation, or tests.
{cite}`requirements.authoring.concurrency`
{cite}`requirements.authoring.usage_storage`
{cite}`requirements.authoring.persistence`
:::

:::{req} refresh
Every `habeas author` form MUST accept `--missing` and `--force`, which are
mutually exclusive; `--missing` MUST be the default. Under `--missing`, the
command MUST initially assess only the selected subjects whose evidence is missing, stale,
or invalid, and MUST preserve every current record without invoking a model,
including accepted FIXME judgments and current false judgments; a current false
judgment is reassessed only when its subject or inputs change or `--force` is
given. Under `--force`, the command MUST assess every selected subject. `--stdout`
MUST imply `--force`, because it writes no evidence. The command MUST report the
number of selected subjects it skipped as current on initial selection.
After that selection, {cite}`requirements.authoring.reassessment` governs each round. When no assessment is needed,
the command MUST succeed without resolving model configuration or credentials.
:::

:::{req} confirmation
Under `confirm`, a first `false` MUST finish the round for that subject. After
`true` or FIXME, the harness MUST obtain exactly one independent second assessment
of the same subject, with the same mandatory inputs and source state. Each
assessment may discover additional inputs and has its own usage record under
{cite}`requirements.authoring.usage_storage`.
:::

:::{req} confirmation.selection
When `confirm` obtains two assessments, the harness MUST select `false` over
FIXME and FIXME over `true` without a further model conversation. A tie selects
the first assessment with one exception. In a site review, `applies` records
whether the distributed requirement governs the source item under review
({cite}`requirements.authoring.site_prompt`). If the judgments tie and only one
assessor says the requirement governs that item, select that assessment.
The selected assessment’s judgment, corrections or acknowledgments, and
explanation MUST be recorded, supplemented by the disagreement report when
required under {cite}`requirements.authoring.confirmation.disagreement`.
:::

:::{req} confirmation.disagreement
When two assessments differ in judgment kind or site applicability, the evidence
rationale MUST report both decisions, their explanations, and which decision was
recorded under {cite}`requirements.authoring.confirmation.selection`.
{cite}`requirements.cli.flip_guidance`
:::

:::{req} habeas_background
Every assessment prompt MUST replace `{{habeas_background}}` with the first text
below. Requirement, example and citation prompts MUST replace `{{reference_roles}}`
with the second. The site prompt and distributed task MUST replace
`{{distributed_background}}` with the third.

```text
Habeas traces a project's requirements, written in its mdBook, to the source
code and tests that implement and verify them.

- A requirement is a {req} directive in the book that states obligations with
  MUST, MUST NOT, SHOULD, and MAY. A named example is an {example} directive
  describing a concrete case of one or more requirements. Both have
  dot-separated full names, such as requirements.cli.status. A vocabulary
  definition, a {definition} directive, defines a term that other text uses with
  {term}; it is never the target of a reference.
- Source code marks the requirements and examples it implements, tests, or
  relies on with {impl}, {test}, and {cite} references, such as
  {impl}`requirements.cli.status`, and acknowledges known gaps in them with
  {fixme} references.
- Sources are identified by outline nodes: a file's functions, types, sections,
  and similar items, named by selectors such as fn[run] or impl[Thing]/fn[run].
  <root> is the whole file.
```

```text
The reference roles:

- {impl}: the source implements part of the requirement or example. Source code
  implements part of one when it makes a decision, performs a check, produces an
  output, or handles a case it specifies, or when it constrains a type's fields,
  variants, serialized form, or validation. Calling code that implements it, or
  producing a value a caller uses to meet it, does not by itself count. A check
  counts only if the text states it, or the item could not meet one of the
  text's obligations without it; then returning its outcome counts even when a
  caller acts on it. Code that checks it is consistent with a requirement, such
  as recognizing a form the requirement defines in order to decide whether to
  run other behavior, relies on that requirement ({cite}) rather than
  implementing it. A requirement stating a convention for every output of a
  kind, such as every validation error, is implemented by the code that carries
  out the convention as a whole, not by each item that produces one such output.
  Only a root needs the reference: an item that no other item marked {impl}
  for the same requirement reaches. Code a reviewer reaches by following a
  marked item, through what it calls, the types it uses, or the implementations
  of trait methods it calls, needs none; nor does a test helper reached from a
  test marked {test}. A site of a
  distributed requirement needs no reference to it for being a site.
- {test}: the source is a test or test helper designed to verify behavior the
  requirement or example specifies.
- {cite}: the source relies on or refers to the requirement or example, and
  should be re-checked or kept in sync when it changes, without implementing or
  testing it. A {cite} needs no coverage; each item with citations has its own
  citation review.
- {fixme}: the source acknowledges a known gap in fulfilling the requirement or
  example. It is always fenced; its body describes the gap and may begin with
  lines such as :github: rust-lang/rust#123 naming issues that track it.

A declaration can also name a whole file or one outline node it is implemented
or tested by, with an :impl: or :test: option line directly after its opening
fence; that counts as an {impl} or {test} reference from that source. A
reference written as a fence, :::{role} name, then commentary, then :::,
attaches commentary.
```

```text
A distributed requirement states obligations that each of many sites upholds on
its own. It contains site queries, which select its sites, and violation
queries, which select constructs likely to break those obligations. Each site is
the innermost outline node containing a site-query match.
```
:::

:::{req} judgment_rules
Every assessment prompt MUST replace `{{judgment_rules}}` with this text, so all
assessments judge by the same rules:

```text
- Derive obligations from the supplied subject and contextual text. Base
  conclusions about tests or other source behavior only on source code you have
  received in mandatory inputs, node reads, or text and structural search results. Close every delivered search with
  finish_search, recording the tags whose snippets you relied on without an
  additional source read. Supplied acknowledgments establish what a {fixme}
  reference says and which issues it names; they do not establish that its
  claimed gap is real. Listings and references locate items;
  read any item whose code your conclusion relies on unless a search snippet
  provides the information you need. Comments state intent; check behavior and
  assertions in the code itself.
- Judge only against the text your task names. Report a gap only where your task
  or these rules define one, naming the obligation and the relevant sources you
  have reviewed. For a missing test, identify the obligation or scenario after
  looking where your task directs; do not invent a source item for absent code.
  Do not report other bugs or issues you notice; edge cases the text does not name are
  not gaps.
- A SHOULD imposes an obligation unless the text or a source comment gives a
  reason it does not apply. In a test-adequacy assessment, apply the testing
  criteria to each applicable SHOULD, and explain any exception in your rationale.
  MAY imposes nothing to check. Examples and sample output inside the text
  illustrate the obligations its prose states; they bind their exact form only
  where the text says the output takes that form.
- If the text admits two readings that its wording supports equally, not merely
  one it fails to exclude, and they lead to different conclusions, such as
  whether code conforms or which role a citation calls for, that ambiguity is a
  gap. Name the readings in your rationale.
- Unless the subject explicitly requires them, these are not gaps: style,
  performance, logging, how helpers are factored, robustness, and handling input
  the text does not mention.
- A gap is acknowledged only by a {fixme} reference, in the supplied
  acknowledgments, in the inputs, or in a source you read, to the requirement
  or example your task judges against, or,
  in a site review, to the distributed requirement within the site, whose body
  describes that same gap, and whose named issues are open; a {fixme} naming a
  closed issue no longer acknowledges its gap. A plain FIXME or TODO comment is
  not an acknowledgment. Judge an acknowledgment only within your task's scope.
  An acknowledgment of an implementation bug alone does not establish a
  test-adequacy gap or acknowledge a different missing-test or assertion gap.
  Do not inspect implementation correctness merely to validate an out-of-scope
  acknowledgment. In a FIXME determination or an acknowledgments list,
  give each distinct issue its :github: option names once, such as
  "rust-lang/rust#123", and, for a {fixme} naming no issue, its body's words
  joined by single spaces. A {fixme} describing a gap that your review establishes
  no longer exists within its task's scope is itself a gap.
- Give source locations in your Markdown rationale. For Failed, put the reasons
  in the individual corrections. Include any acknowledged gaps as context in
  those explanations when reporting a mixed judgment.
- Give a source location as its file and outline selector, in backticks: `src/config.rs#fn[load]`, or
  `src/config.rs` for a whole file. Name requirements, examples, and
  definitions by full name in backticks. Name functions, types, and tests in
  backticks exactly as they are written. Do not give line numbers: they change
  whenever nearby code changes, while a selector stays correct.
```
:::

:::{req} citation_prompt
A citation review under {cite}`requirements.references.citation_review` MUST use
the SDK’s graph and project source tools for exploration under {cite}`requirements.sdk.graph.validation-context.read_node.behavior` and {cite}`requirements.sdk.graph.search.finish_search.behavior`, and the SDK’s `submit_result({result})` protocol under {cite}`requirements.sdk.graph.agent-harness.submission`. The result contains a boolean determination and a nonempty rationale. Citation assessments vet the rationale under
{cite}`requirements.authoring.line_numbers.citation`. A citation assessment uses this
prompt, replacing `{{item_file}}`, `{{item_selector}}` (with `<root>` for the
root node), `{{review_kind}}` (`source_item` for ordinary source citations or
`book_commentary` for a reviewed book passage), `{{citations}}` (the item's
`{cite}` references as JSON with role, name, and commentary), `{{habeas_background}}` and `{{reference_roles}}` under
{cite}`requirements.authoring.habeas_background`, `{{judgment_rules}}` under
{cite}`requirements.authoring.judgment_rules`, and `{{mandatory_inputs}}`, the
first block as its system prompt and the second as its subject message under
{cite}`requirements.authoring.prompt_parts`:

```text
# Task

Review one source item's citations. Your determination should be the one any
careful reviewer would reach from the same text and sources.

# Background: Habeas terms

{{habeas_background}}

{{reference_roles}}

# Your task

The mandatory inputs at the end contain the source item and the declarations
or fixed built-in text of its citations.

Assess each listed citation using the rules below. For citations of project
requirements and examples outside book commentary, decide whether the item
implements part of a cited requirement, not whether it does so correctly. Do not
look for requirements or examples the item does not cite, except as needed to
assess a built-in's stated citation criteria.

1. For `book_commentary` citing project requirements or examples, check whether
   the passage's claims agree with and are supported by the cited declarations.
   Contradicting or unsupported claims are gaps. Explanatory prose
   uses {cite}; it does not need {impl} or {test} merely because it describes
   behavior. For other citations of project requirements and examples, apply
   the reference procedure in order: if the item is a
   test designed to verify the cited requirement or example, it calls for
   {test}; otherwise, if it is a root of the cited definition's
   implementation as the reference roles describe, it calls for {impl};
   otherwise the citation is right if a change to the cited text could
   require changing the item, as when the item relies on behavior the text
   specifies or must agree with a form or value it defines; read the item's
   callers only when the item's own code does not settle this. A citation for
   which this calls for a different role, or for no reference at all, is a gap.
2. A citation of a requirement or example the item also references with {impl}
   or {test} is redundant and is a gap.
3. Treat commentary as context. For a built-in citation, check its permitted
   reference roles and forms and assess any citation-review criteria stated in
   its fixed text using the source and commentary.
   Read other sources when a stated reason relies on them.
   Record a gap when the stated criteria are not met.

# How to judge

{{judgment_rules}}

# Submitting

Choose true when your task finds no gap, and false otherwise. A citation review
MUST NOT submit a FIXME determination. Call submit_result with determination and rationale inside result.
A rejected submission does not complete the assessment. A judgment written in your
reply text is not accepted.
```

```text
# Subject

Item under review: {{item_file}} {{item_selector}}
Review kind: {{review_kind}}
Citations to check: {{citations}}
Judge against: the declarations or fixed built-in text it cites.

# Mandatory inputs

{{mandatory_inputs}}
```
:::

:::{req} site_prompt
A site review under {cite}`requirements.references.site_review` MUST use the
same tools and exactly-once submission rule as other assessments, except that
`submit_result` MUST also require an `applies: Boolean` field, recorded as the
review's `applies`; a submission with `applies` false and a determination other
than `true` MUST be invalid. It MUST use this prompt, replacing
`{{requirement_name}}`, `{{site_file}}`, `{{site_selector}}`, `{{matches}}` (a
JSON array giving, for each query match at the site, the query's kind, `sites`
or `violations`, its name, and the selector, under
{cite}`requirements.references.outline_selectors`, of the innermost outline node
containing the match, but no line numbers, and for a violation match its
matched source text),
`{{habeas_background}}` under {cite}`requirements.authoring.habeas_background`,
`{{distributed_background}}` under
{cite}`requirements.authoring.habeas_background`, `{{judgment_rules}}` under
{cite}`requirements.authoring.judgment_rules`, and
`{{mandatory_inputs}}`, the first block as its system prompt and the second as
its subject message under {cite}`requirements.authoring.prompt_parts`:

```text
# Task

Review one site of a distributed requirement. Your determination should be the
one any careful reviewer would reach from the same text and sources.

# Background: Habeas terms

{{habeas_background}}

{{distributed_background}}

# Your task

The mandatory inputs at the end contain the site, the requirement, and the
declarations it cites.

The requirement may also state obligations that particular code carries out;
those, and the requirement's tests, are assessed with the requirement, not here.
A site needs no {impl} or {test} reference and no tests of its own. Do not
assess the site against other requirements, and do not assess the queries, the
site's callers, or other sites.

1. Decide applies from the requirement's text describing its sites, not from
   which query matched; a violation match alone does not make the requirement
   apply. Read the site's callers only when its own code does not settle whether
   the text describes it. Set applies to true when the best-supported reading of
   that text, or one its wording supports equally well, governs the site.
   Otherwise set applies to false and determination to true, and explain in your
   rationale why the requirement does not apply.
2. If applies is true, judge the site: each obligation the requirement states
   for its sites that the site does not meet is a gap. Violation matches are
   leads, not verdicts: judge each one listed above, saying whether it breaks
   the requirement here and why, but a match may be harmless and the site may
   break the requirement without one.
3. Follow code the site calls only as far as needed to decide the site's
   behavior under the requirement, and do not judge that code on its own
   account.

# How to judge

{{judgment_rules}}

# Submitting

Choose true when your task finds no gap, {"FIXME": ["..."]} when every gap is
acknowledged, and false otherwise. You MUST end by calling submit_result
with applies, determination, and rationale inside result until one submission is accepted.
A rejected submission does not complete the assessment. A judgment written in
your reply text is not accepted.
```

```text
# Subject

Site under review: {{site_file}} {{site_selector}}
Requirement: {{requirement_name}}
Matches: {{matches}}
Judge against: the obligations the requirement states for each of its sites.

# Mandatory inputs

{{mandatory_inputs}}
```
:::

:::{req} progress
When stderr is an interactive terminal, `habeas author --all` MUST display an
updating progress bar with elapsed time, percentage, finished and total assessment
counts, active and queued counts, and separate counts for `true`, `false`, `FIXME`,
and authoring errors. The total MUST include each scheduled assessment once, including newly queued
reassessments. A subject assessed twice contributes two completed assessments. A finished assessment
MUST advance progress whether it writes a judgment or reports an authoring error.
Progress measures completed work, not satisfaction of requirements.

Inspection MUST also show a progress bar for each phase with a known amount of
work, including evidence files, reference source files and coverage, Rust
function coverage, and assessment selection. The display MUST label the current phase
and show its completed and total checks; its percentage measures that phase's
checks, not estimated time or overall inspection completion. Discovery and other
phases whose totals are not yet known MUST use an indeterminate activity display.
Inspection progress MUST NOT change validation results or invoke inference.
Enabled reports MUST remain readable alongside progress. An
interrupted batch MUST retain its actual progress without counting unstarted
assessments as finished. Empty batches and early errors MUST clear the temporary
display. When stderr is redirected or `TERM=dumb`, the command MUST retain its
plain line-oriented reports without terminal control sequences. `NO_COLOR` MUST
disable color without disabling progress on a supported terminal. The existing
stdout completion summary and exit-status rules MUST remain unchanged.
{cite}`requirements.authoring.batch`
{cite}`requirements.authoring.reporting`
:::

:::{req} reporting
By default, authoring MUST suppress per-subject `true` judgment lines and show
each `false` judgment's subject and outcome on stderr, on both terminals and
redirected output. Existing progress counters and completion summaries MUST remain
available; FIXME notices and authoring errors MUST remain visible. Single-subject
authoring MUST retain its subject, backend, model, evidence destination, and
usage-record metadata.

`-v` and `--verbose` MUST additionally show each false judgment's recorded Markdown
rationale on stderr; rationales MUST be omitted by default. This applies to
single-subject and batch authoring. It MUST
NOT print true rationales or append recorded
input snapshots. Reporting MUST use the completed assessment's existing result,
without additional inference. In `--stdout` mode, stdout MUST remain only the
evidence JSON; verbose rationale and other diagnostics MUST go to stderr.
For each definition with corrections, stderr MUST identify additions applied,
markers already present, outstanding manual instructions, and each unapplied
action with its reason, even at default verbosity. If a subject remains negative
with no changed inputs, the report MUST say why it paused. In `--stdout` mode,
report proposed additions as proposals. Verbosity MUST NOT change persistence,
validation, or exit-status rules.
{cite}`requirements.authoring.invocation` {cite}`requirements.authoring.batch`
:::

:::{req} provider_backoff
When a provider reports a transient failure, the harness MUST wait and retry
the same completion request. Transient failures are throttling, reported
through HTTP 429 or a Bedrock `ThrottlingException`; temporary unavailability,
reported through HTTP 500, 502, 503, or 504 or a Bedrock
`ServiceUnavailableException`, `InternalServerException`, or
`ModelNotReadyException`; and a request with no response after ten minutes,
which the harness MUST abandon. The retry MUST retain its prompt, history,
tools, and pending inputs. It MUST NOT
restart the assessment or repeat already completed tool calls. A permanent
input/context-length error, authentication failure, or invalid request MUST
remain an error; mentioning tokens in an error message alone MUST NOT trigger
retries.

The backoff window MUST start at one second and double after each consecutive
transient failure, up to sixty seconds. Each delay MUST be randomly selected
between half and all of that window to spread parallel retries. An available
provider `Retry-After` value, in seconds or HTTP-date form, MUST be honored as a
minimum delay even when longer than the window. A successful response MUST reset
backoff for the next completion request, and interruption MUST cancel both the
wait and further retries.

Each attempt MUST count toward the assessment totals under {cite}`requirements.authoring.usage_requests`.
Hidden SDK retries MUST remain disabled so the harness can account for attempts.
Transient retries MUST NOT be reported as finished judgments or terminal
assessment errors while recovery is still in progress. Pending inputs MUST be
recorded as evidence only after the request succeeds.
{cite}`requirements.authoring.provider` {cite}`requirements.authoring.delivered_inputs`
:::

:::{req} provider_backoff.limit
The harness MUST stop retrying a completion request under
{cite}`requirements.authoring.provider_backoff` when the next attempt would
start more than one minute after the first of its consecutive transient
failures, including when a provider `Retry-After` delay would pass that point.
It MUST then fail the assessment with an error naming the number of attempts
and the last failure's HTTP status and provider error code, when available.
:::

## Model tools

All review kinds use TBV’s configured agent and `submit_result`. The agent can read graph nodes, inspect edges, list and search project sources, finish searches, and retrieve additional pages. Habeas registers `lookup_requirements`, `run_named_query`, and `github` for operations specific to its domain.

:::{req} tool_descriptions.common_case
The descriptions of `lookup_requirements`, `run_named_query`, and `github` MUST explain what the tool does, when to use it rather than another tool, each
argument's meaning and form, with a short example where the form is not
obvious, and the form of a successful result.
:::

:::{req} tool_descriptions.special_cases
`lookup_requirements`, `run_named_query`, and `github` SHOULD each explain an invalid request or failed lookup in the returned error. Each description SHOULD focus on the successful operation.
:::

:::{req} redaction_notice
Tool results MUST use the SDK’s `[REDACTED:<kind>]` marker for credential-shaped text under {cite}`requirements.authoring.redaction`.
:::

:::{req} no_line_numbers
For each project source location Habeas supplies, the address MUST consist of the project-relative file and outline selector under {cite}`requirements.references.outline_selectors`. This applies to the source metadata in mandatory inputs, acknowledgments, site matches and citation reviews, and to locations in Habeas tool diagnostics. These addresses MUST omit line and column coordinates. Quoted source retains its text, and an argument error may quote the agent's input.
:::

The SDK specifies the response formats for its graph and source tools, including {cite}`requirements.sdk.graph.validation-context.read_node.source_children` and {cite}`requirements.sdk.graph.search.agent_search`.

### Read graph nodes

:::{req} source_tools
All assessments MUST use the SDK graph and source tools. A read uses the node address under {cite}`requirements.sdk.graph.validation-context.read_node.mcp`. A delivered read records that node’s fingerprint under {cite}`requirements.sdk.graph.validation-context.read_node.inputs`. The source project follows {cite}`requirements.sdk.sources.local_provider`.
:::

:::{req} read_denial
Habeas configures the SDK source tools with the project directory. The files available for agent discovery and search follow {cite}`requirements.sdk.sources.local_provider`. An explicit tool read of a file excluded by that provider MUST return a tool error naming the path and reason, add no evidence input, and allow the assessment to continue. Malformed tool
arguments and ordinary read failures MUST also return explanatory tool errors.
:::

:::{req} read_denial.mandatory
When source-access policy denies a mandatory input, the assessment MUST abort
before any model request and report the denied path and reason to its caller. The aborted assessment MUST NOT write evidence or emit evidence JSON, and
its usage record MUST identify the failure. In a batch, other assessments MUST
continue, including queued assessments, and their successful writes MUST be
retained; the command MUST return a nonzero exit status after the batch
finishes.
:::

:::{req} generated_inputs
When validating a saved evidence input in a Cargo build-script output directory,
Habeas MUST accept existing output discovered from Cargo metadata and its
build-script output records, subject to {cite}`requirements.configuration.evidence_paths`.
It MUST check freshness using the recorded selector and MUST NOT run a build.
:::

### Discover source nodes

The SDK supplies {cite}`requirements.sdk.graph.validation-context.list_sources.behavior`, {cite}`requirements.sdk.graph.validation-context.outline_source.behavior`, {cite}`requirements.sdk.graph.search.search_text.mcp`, and {cite}`requirements.sdk.graph.search.search_structure.mcp`. Agents follow a result with `read_node` and can traverse its direct children under {cite}`requirements.sdk.graph.validation-context.read_node.source_children`. For a result with a continuation, use {cite}`requirements.sdk.graph.validation-context.pages.mcp`.

:::{req} structural_search
`run_named_query` MUST accept a declared `requirement/query` name and return the matching source node IDs and their data. Agents read the relevant nodes with `read_node`.
:::

### Record the excerpts used in a review

Text and structural searches use the SDK's page IDs, match tags, and first-page instructions under {cite}`requirements.sdk.graph.search.first_search.instructions`. The agent closes each delivered page with {cite}`requirements.sdk.graph.search.finish_search.mcp`. The SDK retains the selected observations and requires all pages to be closed before submission under {cite}`requirements.sdk.graph.search.finish_search.behavior`.

:::{req} finish_search.inputs
Habeas MUST store the source dependencies selected by `finish_search` under {cite}`requirements.sdk.graph.search.finish_search.inputs` as outline inputs under {cite}`requirements.ci.outline_inputs`. Each input identifies the file, selector, and structural hash from the snapshot that supplied the selected excerpt.
:::

:::{req} submission.searches.line_numbers
The SDK checks for outstanding searches before invoking Habeas's rationale vetting. A submission rejected for an outstanding search MUST leave the rationale correction attempt available under {cite}`requirements.authoring.line_numbers.rejection`.
:::

:::{example} search_accounting.helper_freshness
A marked test calls `assert_refusal`. A search excerpt of that helper supplies
the refusal and byte-preservation assertions used to justify the judgment.
The model finishes the search with the helper's tag without reading it with
`read_node`. Saved evidence MUST include the helper's outline input.

Replacing the helper with a no-op MUST make that evidence stale, even when
the marked test source is unchanged.
{test}`requirements.authoring.finish_search.inputs`
{test}`requirements.sdk.graph.search.finish_search.inputs`
:::

### Read GitHub issues: `github`

:::{req} github_reads
The harness MUST expose a `github` tool with this interface:

```text
github({ issue: String }) -> Issue

Issue { reference: String, state: "open" | "closed", title: String, body: String }
```

`issue` MUST have the form `<owner>/<repo>#<number>`. The tool MUST read the issue
or pull request through the GitHub API, authenticating with the `GITHUB_TOKEN`
environment variable when that is set, and MUST return its body redacted under
{cite}`requirements.authoring.redaction`. GitHub reads cannot be reproduced
later, so their results MUST NOT be recorded as evidence inputs and MUST NOT
make evidence stale. Every GitHub read by Habeas, including this tool's, the
lookups below, and those of {cite}`requirements.cli.fixme_issues`, MUST go
through one cache under the state root of
{cite}`requirements.configuration.state_location`, keyed by issue, and MUST
reuse a cached result less than one hour old instead of contacting GitHub. A malformed reference, a missing issue, or a failed request
MUST return an explanatory tool error.

When the mandatory inputs contain a `{fixme}` reference under
{cite}`requirements.references.roles`, or the initial request includes one under
{cite}`requirements.authoring.fixme_inputs`, the harness MUST look up the state of
each issue its `:github:` lines name in the same way and MUST supply each state
with the corresponding source input or acknowledgment as `open` or `closed`;
when GitHub cannot be reached, the issue MUST be assumed open. These states MUST
NOT be recorded either. Instead, `habeas status` and `habeas author` MUST treat a recorded FIXME determination as stale, exactly as if one of its inputs had changed, when an issue that one of its acknowledging `{fixme}` references names, or for a site review the `{fixme}` references inside the site, is closed or does not exist; repository validation, which never contacts GitHub, MUST NOT.

The harness MAY take the GitHub API base URL from `HABEAS_GITHUB_API_URL`,
accepting only `http://127.0.0.1:<port>`, so that tests can use a local server.
:::

### Look up requirements and follow references

:::{req} reference_index
`lookup_requirements` MUST accept an optional requirement-name glob and return matching requirement and example node IDs and their data. With no selection it lists all requirements and named examples. The agent can inspect implementation, citation, test, and acknowledgment targets through `outgoing_edges`. Reading the returned node through `read_node` records its contents.
:::

### Submit the result

:::{req} submission
Each assessment MUST use the SDK’s `submit_result({"result": ...})` tool under {cite}`requirements.sdk.graph.agent-harness.submission.mcp`. Requirement and named example assessments MUST submit one variant of a Rust enum:

- `Success { rationale }` records `true`.
- `Failed { corrections }` records `false` and MUST include a nonempty list under {cite}`requirements.authoring.corrections`.
- `AcknowledgedGap { entries, rationale }` records `{"FIXME": entries}` when every real gap is acknowledged under {cite}`requirements.ci.outcomes`.

Each rationale MUST be nonempty Markdown. Citation reviews submit a boolean `determination` and a rationale. Site reviews submit `determination`, `rationale`, and `applies` under {cite}`requirements.authoring.site_prompt`. The SDK handles submission under {cite}`requirements.sdk.graph.agent-harness.submission.acceptance`. Failure before acceptance MUST preserve existing evidence.
:::

### Applying corrections and reassessing subjects

:::{req} corrections
Each correction in a `Failed` submission MUST be either
`AddReference { role, file, selector, rationale }` or `Manual { instruction, rationale }`.
An `AddReference` proposes a missing `test` or `impl` reference from an existing
item to the assessed definition. Its file MUST be project-relative and its
selector MUST identify that item. A `Manual` instruction MUST be nonempty and
state what remains to be done.
:::

:::{req} corrections.rationale
Each correction MUST carry a nonempty Markdown rationale identifying the
obligation and explaining why the observed evidence warrants that change.
For `AddReference`, explain how the item serves the proposed role. For `Manual`,
explain the gap the instruction would close.
:::

:::{req} corrections.coverage
The corrections in a `Failed` submission MUST cover every unacknowledged gap.
When a marker and a change to code or tests are both needed, the assessment
MUST report both corrections. A manual instruction does not acknowledge a gap
under {cite}`requirements.ci.outcomes`.
:::

:::{req} corrections.apply
For each round of file-output authoring, Habeas MUST assess the queued subjects
against the same source state, using the concurrency under
{cite}`requirements.authoring.concurrency`. It MUST collect the round's findings
before applying any source edits. It MUST use only corrections from the final
negative result selected under {cite}`requirements.authoring.policy` and
{cite}`requirements.authoring.adaptive`. Every valid addition MUST be applied,
including additions from subjects that also have manual or inapplicable corrections.

Habeas MUST group additions by file and deduplicate identical definition, role,
file, and selector tuples. If reference discovery already associates the same role and definition with
the selected item, including through an anchor under
{cite}`requirements.references.anchors`, that action is satisfied without a write. To insert a marker, Habeas MUST
resolve a unique item in a writable source within the project under
{cite}`requirements.references.discovery` and match its current
fingerprint to the one observed for that exact item in the proposing assessment.
A delivered item input or read, or an excerpt recorded as used under
{cite}`requirements.authoring.finish_search.inputs`, qualifies. A file listing
or an address alone is insufficient. Check actions for the same file against
the same pre-edit contents. The resulting file MUST attach each new marker to
the intended item under {cite}`requirements.references.outline_comments`, preserve
other content, and avoid overwriting intervening edits. An interruption MUST stop
new source writes and reassessments under {cite}`requirements.authoring.batch`.
:::

:::{req} reassessment
After applying a round's additions, Habeas MUST check freshness against the
edited sources. It MUST reassess a subject whose corrections are all satisfied
when the writes changed its assessment inputs. It MUST also reassess any other
subject whose previously current evidence those writes made stale, including
definitions, citation reviews, and site reviews outside the initial selection.
Independent reassessments MUST run under {cite}`requirements.authoring.concurrency`.
Each reassessment MUST use fresh inputs and the selected assessment policy.
Only a fresh successful assessment establishes `true`.

A subject with a remaining manual instruction or an action that could not be applied MUST pause for
the rest of the invocation, even if another edit changes its inputs. Other
subjects MUST continue. An already-present marker or stale selector alone MUST
cause no reassessment. If every requested marker already exists and the inputs
are unchanged, the negative judgment stands. Repeat while eligible subjects
remain, with no limit on rounds or automated additions. With `--stdout`, Habeas
MUST report the named subject's assessment and proposed corrections without
editing sources or starting a correction-driven reassessment.
:::

## Initial prompt

### Mandatory inputs

:::{req} line_numbers.detection
A rationale MUST be treated as citing line numbers when it contains a file path
followed by `:` and a digit, the word `line` or `lines` in any case followed by
a number, or a parenthesized number range such as `(12–30)` or `(12-30)`.
:::

:::{req} line_numbers.rejection
For requirement, example, and site reviews, when the only submission in a response
under {cite}`requirements.authoring.submission` is otherwise valid but any of its
rationales cites line numbers under
{cite}`requirements.authoring.line_numbers.detection`, and it is the
assessment's first such submission, and it passes the outstanding-search gate
under {cite}`requirements.authoring.submission.searches.line_numbers`, the
harness MUST reject it instead of
accepting it and continue the conversation. The tool result MUST quote the
matched text, identify the affected rationale (including its correction position
for `Failed`), repeat the location guidance of
{cite}`requirements.authoring.judgment_rules`, and give a fresh random UUID,
saying the model may submit once more, either without line numbers or with
the offending rationale set to exactly that UUID to keep the rejected submission as it was.
A rejection MUST be counted in usage like any other turn.
:::

:::{req} line_numbers.resubmission
After a rejection under {cite}`requirements.authoring.line_numbers.rejection`,
a submission whose previously flagged rationale is exactly the UUID that rejection gave MUST
resolve to the rejected submission unchanged, ignoring the new submission's
other fields; acceptance remains subject to
{cite}`requirements.authoring.submission.searches.line_numbers`. After passing that gate, any other rationale that cites line numbers MUST fail
the assessment, so
an assessment rejects at most one submission. The UUID MUST NOT be recorded in
evidence.
:::

:::{req} line_numbers.citation
For each citation assessment, Habeas MUST vet
`answer.rationale` with {cite}`requirements.sdk.graph.agent-calls.vet_answer`.
If the rationale contains no source line citation as defined by
{cite}`requirements.authoring.line_numbers.detection`, the line-number callback
MUST return `Ok(VetResult::Accept)`. The first time a rationale cites a line
number, the callback MUST return `Ok(VetResult::CorrectAndTryAgain(message))`.
The message MUST quote the offending text and ask for the file and outline
selector under {cite}`requirements.authoring.judgment_rules`. If a later
submission still cites a line number, the callback MUST end the review by returning a domain error under
{cite}`requirements.sdk.graph.agent-calls.vet_answer`.
:::

:::{req} mandatory_inputs
The harness MUST construct each assessment's mandatory input set by combining
the contributions specified by the requirements applicable to that subject and
its inputs. A requirement that contributes inputs MUST cite this requirement and
define which sources it contributes and when. The harness MUST include every
applicable contribution, deduplicate identical selections, and report
unresolvable mandatory inputs before inference. The complete set MUST be supplied
in the initial model request.
A file selected as a mandatory input MUST be visible under
{cite}`requirements.sdk.sources.local_provider`. A denied selection MUST abort
under {cite}`requirements.authoring.read_denial.mandatory`.
:::

The contribution rules live with [requirements](../requirements.md#requirements-as-authoring-inputs),
[examples](examples.md#examples-as-authoring-inputs),
[references](references.md#references-as-authoring-inputs), and
[terminology](definitions.md#terminology-as-authoring-inputs). Existing
evidence is [not carried forward](ci.md#existing-evidence-inputs-used-by-authoring).

:::{req} fixme_inputs
For a requirement or example assessment, the initial request MUST include every
valid named `{fixme}` reference to the subject found under
{cite}`requirements.references.discovery`, except a reference inside one of
the subject's sites under {cite}`requirements.references.site_review`, in this
form:

```text
Acknowledgment {
    file: String,
    item: String,
    body: String,
    issues: Array<{ reference: String, state: "open" | "closed" }>,
}
```

`file` and `item` MUST locate the containing source by project-relative path and
outline selector, using `<root>` for a whole file. `body` MUST contain the
reference's complete fenced commentary without its fences or source-comment
delimiters, including its option lines, redacted under
{cite}`requirements.authoring.redaction`. `issues` MUST give every distinct issue
its `:github:` lines name, in their order, with state supplied under
{cite}`requirements.authoring.github_reads`. Acknowledgments MUST be ordered by
file and position in the file and MUST carry no line numbers or containing
implementation source.

The locations and bodies MUST be protected against staleness by the reference-set
digest under {cite}`requirements.ci.reference_set`; the acknowledgment MUST NOT
create an outline input merely to cover its containing source. Actual reads of
that source MUST still be recorded under
{cite}`requirements.authoring.delivered_inputs`. Sources MUST satisfy
{cite}`requirements.authoring.source_tools`; an inaccessible acknowledgment
MUST be reported before inference.

Terminology definitions referenced by an acknowledgment's body MUST contribute
their complete, transitive declarations to the mandatory input set under
{cite}`requirements.definitions.authoring_inputs` and MUST be recorded as
definition inputs. Only the acknowledgment's body MUST contribute these
dependencies; its containing implementation source MUST NOT be supplied merely
to discover them.
:::

### Structural assessment

Some requirements describe the program's structure: a public signature, a
borrowed context, or delegation through a particular interface. These can be
checked by reading the implementations. Behavioral properties are checked by
tests. A requirement can have both kinds of property; each check applies
independently.

:::{req} structural_review
For every non-distributed requirement, the assessment MUST identify its stated
structural properties, including signatures, type and field relationships,
ownership, and required delegation through interfaces. With incoming `{impl}`
references or `:impl:` anchors, it MUST check that the supplied implementation
roots support every such property under
{cite}`requirements.authoring.structural_prompt`. Without those edges, it MUST
establish that the requirement has no structural properties needing
implementation evidence; an unmet structural obligation MUST be a gap.
Incoming `{test}` references or `:test:` anchors MUST NOT skip this check.
References and anchors are those considered under
{cite}`requirements.references.discovery` and {cite}`requirements.references.anchors`. Obligations
delegated to refining declarations follow
{cite}`requirements.ci.review_criteria`. Examples, distributed requirements,
citation reviews, and site reviews retain their own assessment tasks.
:::

:::{req} behavioral_review
For every non-distributed requirement, the assessment MUST identify its stated
behavioral properties and check their test adequacy under
{cite}`requirements.ci.review_criteria`. With incoming `{test}` references or
`:test:` anchors, it MUST check that the supplied tests cover every behavioral
property assessed here. Without those edges, it MUST establish that there are
no behavioral properties needing test evidence, explaining any established
testing exemption or obligation delegated to a refining declaration under
{cite}`requirements.authoring.testing_bar` and
{cite}`requirements.ci.review_criteria`; otherwise the missing test evidence
MUST be a gap. Incoming `{impl}` references or `:impl:` anchors MUST NOT skip
this check. Considered references and anchors follow
{cite}`requirements.references.discovery` and {cite}`requirements.references.anchors`.
:::

The current harness selects structural assessment only for requirements with
supported non-Markdown implementation roots and no test edges. Requirements with test edges receive
test-adequacy assessment, including mixed requirements. Issue #51 tracks the
independent checks specified above, including explicit checks when either kind
of edge is absent. The implementation carries issue-linked `{fixme}`
acknowledgments of this temporary gap.

:::{req} structural_inputs
For every non-distributed requirement's structural check under
{cite}`requirements.authoring.structural_review`, every direct implementation
reference or `:impl:` anchor outside a declaration MUST contribute its source
selection to the initial mandatory inputs under
{cite}`requirements.authoring.mandatory_inputs`, including when the requirement
also has incoming `{test}` references or `:test:` anchors. An in-source reference selects
its enclosing outline node, or the whole file when it has no language outline;
an anchor selects its named file or node. Source access, exclusions, errors,
and deduplication follow the existing mandatory-input rules. Refining
declarations continue to contribute their complete text. Implementation
helpers are requested when needed, rather than supplied recursively.
:::

:::{req} structural_prompt
A structural assessment MUST use the same tools, acknowledgments, judgment
rules, confirmation policy, and submission contract as a test-adequacy
assessment, with these system and subject blocks under
{cite}`requirements.authoring.prompt_parts`. Placeholders MUST be filled as in
{cite}`requirements.authoring.initial_prompt`.

```text
# Task

Assess whether a requirement's stated structural obligations are fulfilled by
its implementations. Your determination should be the one any careful reviewer
would reach from the same text and sources.

# Background: Habeas terms

{{habeas_background}}

{{reference_roles}}

# Your task

The mandatory inputs contain the requirement, its cited declarations and
terminology, its direct implementation sources, and declarations that refine
it. Each {fixme} acknowledgment includes the state of the issues it names.

1. Derive the stated structural obligations, including signatures, type and
   field relationships, ownership, and required delegation through interfaces.
   Do not substitute inferred runtime behavior for a structural obligation.
2. Inspect the supplied implementation roots. Follow relevant calls, types,
   trait implementations, and callbacks; request the helper source needed to
   establish each obligation. Comments or reference edges alone are not proof.
3. Map each obligation to the concrete source that fulfills it. Identify
   obligations delegated to a refining declaration separately: assess that
   declaration's text, not its implementation or tests. Cited declarations
   supply context and do not become additional subjects to assess in full.
4. A missing or conflicting structural element is a gap. Observable runtime
   outcomes, boundaries, and failure cases require the independent behavioral
   assessment; do not certify them from implementation alone. Missing or
   inadequate test evidence is a testing gap. Discover relevant tests before
   reporting missing evidence; a test designed to check the subject without
   a {test} reference is a marking gap.
5. Report ambiguities and unavailable information separately. A successful
   structural check means every structural obligation assessed here is
   supported. The requirement's judgment must also account for its independent
   behavioral check. Do not assess unspecified behavior or general
   implementation correctness.

# How to judge

{{judgment_rules}}

# Submitting

Submit Success when you find no gap, AcknowledgedGap with entries when every
real gap is acknowledged, and Failed with corrections otherwise. For a missing
{test} or {impl} marker, give the role, file and selector in AddReference and
explain how that item supports the subject. For Manual, give the instruction
and explain what gap it closes. Include both when both apply. Cover every
unacknowledged gap. A manual instruction does not acknowledge a gap.
Call submit_result with the chosen variant inside result until it is accepted.
Set result.status to "success", "acknowledged_gap", or "failed".
For a failed result, set each correction's kind to "add_reference" or "manual".
A rejected submission does not complete the assessment. A judgment written in your
reply text is not accepted.
```

```text
# Subject

Subject: requirement {{subject_name}}
Judge against: the subject's text, using structural assessment.

# Acknowledgments

{{acknowledgments}}

# Mandatory inputs

{{mandatory_inputs}}
```
:::

### Prompt template

:::{req} initial_prompt
Except for structural assessments under
{cite}`requirements.authoring.structural_prompt`, the initial request MUST provide
the SDK and domain tool definitions above and the
following prompt, the first block as its system prompt and the second as its
subject message under {cite}`requirements.authoring.prompt_parts`. The harness MUST replace `{{subject_kind}}` with `requirement`
or `example`, `{{subject_name}}` with the full subject name,
`{{habeas_background}}`, `{{reference_roles}}`, and `{{distributed_background}}`
under {cite}`requirements.authoring.habeas_background`, `{{judgment_rules}}` under
{cite}`requirements.authoring.judgment_rules`, `{{acknowledgments}}` with the JSON
array of {cite}`requirements.authoring.fixme_inputs`, and `{{mandatory_inputs}}` with the
addresses and supplemental metadata for the mandatory inputs as a JSON array. The SDK appends the corresponding node contents to the subject message. It MUST replace `{{subject_task}}`, for a requirement, with the text of
{cite}`requirements.authoring.requirement_task` followed by that of
{cite}`requirements.authoring.testing_bar`, and for an example, with the text of
{cite}`requirements.authoring.example_task`. It MUST replace
`{{distributed_task}}` with the filled text of
{cite}`requirements.authoring.distributed_task` for a distributed requirement,
and with nothing otherwise. The template MUST otherwise
be used as written. Their review guidance MUST follow
{impl}`requirements.ci.review_criteria`, {cite}`requirements.examples.evidence`,
and {cite}`requirements.ci.outcomes`.

```text
# Task

Assess whether a project's tests adequately check one {{subject_kind}}.
Your determination should be the one any careful reviewer would reach from the
same text and sources.

# Background: Habeas terms

{{habeas_background}}

{{reference_roles}}

# Your task

The mandatory inputs at the end contain the subject, the requirements and
examples it cites, the vocabulary definitions it uses, and the source text of
its {test} items and :test: anchors. Requirement and example declarations that
refine the subject through {impl} are also included. Implementation source is
not included merely because it carries an {impl} reference or :impl: anchor.
Use outgoing_edges to locate it and read_node to read its contents when needed.
Each {fixme} acknowledgment includes the state of the issues it names.

Cited requirements and examples are context: when the subject requires behavior
under or following a cited requirement, check that the subject's tests cover
applying it in the subject's case. Do not reassess the cited requirement's
implementation or tests.

When another requirement's text carries an {impl} reference to the subject,
assess only whether that text carries out the obligations it covers; do not
reassess its implementation or tests, and those obligations need no test here.

Judge test adequacy, rather than whether the current implementation is correct.
Derive expected behavior from the requirement and examples, even if you request
implementation source. A true judgment means the reviewed tests adequately
check the subject under the criteria below. You do not need to find a defect
when those criteria are met.

{{subject_task}}

# How to judge

{{judgment_rules}}

# Submitting

Submit Success when you find no gap, AcknowledgedGap with entries when every
real gap is acknowledged, and Failed with corrections otherwise. For a missing
{test} or {impl} marker, give the role, file and selector in AddReference and
explain how that item supports the subject. For Manual, give the instruction
and explain what gap it closes. Include both when both apply. Cover every
unacknowledged gap. A manual instruction does not acknowledge a gap.
Call submit_result with the chosen variant inside result until it is accepted.
Set result.status to "success", "acknowledged_gap", or "failed".
For a failed result, set each correction's kind to "add_reference" or "manual".
A rejected submission does not complete the assessment. A judgment written in your
reply text is not accepted.
```

```text
# Subject

Subject: {{subject_kind}} {{subject_name}}
Judge against: the subject's text.

{{distributed_task}}

# Acknowledgments

{{acknowledgments}}

# Mandatory inputs

{{mandatory_inputs}}
```
:::

:::{req} requirement_task
The requirement task of {cite}`requirements.authoring.initial_prompt` MUST be
this text, the steps a requirement's review follows to assess test adequacy:

```text
1. Derive testable obligations and expected outcomes from each MUST, MUST NOT,
   and SHOULD clause. Separate independent conditions and properties while
   preserving alternatives the text permits. Identify ambiguities separately.
2. Identify relevant input classes, boundaries, condition combinations, and
   state transitions from the requirement. Use only distinctions that affect
   an obligation; do not invent required behavior for unspecified inputs.
3. Read the supplied tests and request helpers or fixtures needed to understand
   their inputs and assertions. Use targeted discovery to look for relevant
   tests before reporting a missing scenario.
4. Map each obligation assessed here to concrete test scenarios and assertions
   in your rationale. Identify obligations delegated to a refining requirement
   or a distributed site's review, and any established testing exemptions,
   separately. Check both whether the expected result is correct under the
   requirement and whether the assertions enforce it. Execution without a
   relevant assertion does not establish adequacy; assertions in helpers,
   snapshots, and parameterized or property tests can do so.
5. For each proposed adequacy gap, describe plausible requirement-violating
   behavior that the reviewed tests would accept. Cite the requirement clause
   and explain why the tested inputs or assertions would miss it. A wrong
   expected result is a gap too: explain how the assertion contradicts the
   requirement rather than claiming it proves a missing scenario.
   Explain the requirement-relevant distinction the counterexample exposes.
   Another untested value in an already represented input class is not a gap
   without explaining why it could expose different required behavior.
6. Check whether another reviewed test or helper already rules out that
   behavior. Report unresolved questions separately from established gaps.
   If information needed to decide adequacy remains unavailable, choose false
   and explain what is missing without inventing a behavioral gap.
   A relevant test without a {test} reference to the subject is a marking gap;
   a helper reached from a marked test needs no reference of its own. Do not
   confuse marking gaps with inadequate assertions.
7. Request implementation source only to resolve a specific question, stating
   that question in your rationale. Derive expected behavior from the
   requirement, not the implementation. Do not assess implementation
   correctness, implementation reachability, or unspecified implementation
   behavior as additional tasks.
```
:::

:::{req} testing_bar
The requirement task MUST continue with these criteria, which set when a
requirement's obligations need tests:

```text
Testing criteria:
   - Every obligation assessed here, and every boundary or failure case the text
     names for that obligation, needs
     a test that exercises the required behavior and asserts its outcome.
   - Distinct modes and scenarios the text governs, such as verbose and
     default modes, each need a relevant test. Implementation branches for
     cases the text does not distinguish need no test of their own.
   - An untested obligation is a gap unless the code carrying it out has no
     conditional and no transformation of its own, such as passing a value
     through unchanged, or running it would need network services, real elapsed
     time, or process-wide state such as environment variables or the current
     directory, and no test in the project sets that up; say which, and why.
     Request source if needed to establish a claimed exemption. Files and
     directories a test can create do not qualify.
   - That exemption does not cover an obligation that itself states that a value
     stays unchanged, distinct, or present; such an obligation needs a test.
```
:::

:::{req} distributed_task
For a distributed requirement under {cite}`requirements.references.distributed`,
the subject message of {cite}`requirements.authoring.initial_prompt` MUST
include this text, which continues the requirement task to assess the
requirement's queries:

```text
{{distributed_background}} Each query is an ast-grep rule with match and
no_match snippets.

This subject is a distributed requirement. Its sites need no {impl} reference
and each has its own review, so do not vet them. Also assess the requirement and
its queries:

8. Each obligation must be either one that each site upholds on its own, or one
   that particular code carries out, whose tests you assess under 1 to 7 and the
   testing criteria. An obligation that
   neither covers is a gap.
9. The site queries must select every place the text's description of a site
   covers, and no place outside the requirement's stated scope. Run each site
   query once with run_named_query by its name, <subject>/<query>, scan its matches,
   and search briefly for the construct the text describes to find places the
   queries miss. Obligations that decide which places are sites, such as how a
   variable is named, are checked by this search.
10. Judge whether each query's conditions are adequately tested by its
   snippets, as you would judge tests of code: its match snippets should show
   what it selects, and its no_match snippets nearby code it must not select.
   A condition without a snippet of its own is not by itself a gap. This
   applies to violation queries too.

Queries are heuristics, so judge them by a preponderance of the evidence: is it
more likely than not that the site queries find the places the requirement
applies? A specific place in the sources that a site query misses, or selects
outside the requirement's scope, is a gap; a form that does not occur in the
sources is not. A violation query that misses constructs is not a gap; note what
it misses.
```
:::

:::{req} example_task
For a named example, the task of {cite}`requirements.authoring.initial_prompt`
MUST be this text:

```text
The subject is a named example. First check that it describes a concrete case
with expected behavior; a missing concrete case or expected behavior is a gap.
Read its supplied tests and request helpers or
fixtures needed to understand their setup and assertions. Check that they cover
its scenario and assert each expected outcome it states, with expected results
derived from the example and its referenced requirements. Say which inputs and
assertions check each outcome. For a proposed gap, describe requirement-violating
behavior those tests would accept, or an assertion whose expected result
contradicts the example. Check whether another reviewed test or helper resolves
the gap, and report unresolved questions separately. Request implementation
source only for a specific question, stating it in your rationale.
If information needed to decide adequacy remains unavailable, choose false and
explain what is missing without inventing a behavioral gap.
A true judgment needs at least one {test} reference to the example in the
mandatory inputs; choose true when the tests adequately check it.
```
:::

## Configuration

:::{req} provider
The authoring harness MUST use Rig and support Bedrock, Ollama, and OpenAI. Users MUST be
able to select a backend and model through configuration without changing the
assessment tools, prompt, or evidence format.
:::

:::{req} configuration
Authoring MUST use the `[authoring]` sections of the shared user configuration
and the selected workspace's `habeas.toml`, refining
{impl}`requirements.configuration.feature_sections` for the authoring feature.
{cite}`requirements.configuration.user_location`
{cite}`requirements.configuration.structure`
Assessing a subject with `habeas author`, singly or in a batch that needs at
least one assessment, MUST require an `[authoring]` section in at least one of
these files; either section MAY be absent. No other operation requires the
section, including a batch with no assessments needed and the operations in
{cite}`requirements.authoring.deterministic_validation`.
It MUST support named configurations, workspace defaults, and a user-local
selection for each project. Credential-source references SHOULD be used;
explicit credentials MUST be supported under {cite}`requirements.authoring.credentials`.
{cite}`requirements.configuration.workspace_location`
{cite}`requirements.configuration.workspace_structure`
:::

### Serialized configuration data model

The shared `UserConfig.authoring` field contains `UserAuthoringConfig`.
`Optional<T>` means a field may be absent; `Map<K, V>` is a table keyed by `K`.

:::{req} configuration_structure
{impl}`requirements.configuration.feature_sections`
Habeas MUST define its authoring configuration using these data structures:

```text
UserAuthoringConfig {
    default: Optional<ConfigName>,
    configs: Map<ConfigName, AuthoringConfig>,
    projects: Map<CanonicalProjectRoot, ProjectSelection>,
}

WorkspaceAuthoringConfig {
    default: Optional<ConfigName>,
    configs: Map<ConfigName, AuthoringConfig>,  // omitted means empty
}

ProjectSelection {
    config: ConfigName,
}

AuthoringConfig {
    backend: BackendConfig,                   // tagged union, flattened in TOML
    concurrency: Optional<PositiveInteger>,   // simultaneous assessments
    policy: Optional<AssessmentPolicy>,       // default: Once for OpenAI; Confirm otherwise
}

AssessmentPolicy = Confirm | Once             // TOML: "confirm" or "once"

ModelRef {
    backend: BackendKind,                     // Bedrock, Ollama, or Openai
    model: ModelId,
}
```

`BackendConfig` uses the backend tags and fields in the TBV
[backend settings and credentials](sdk/graph/agent-harness.md#backend-settings-and-credentials)
under {cite}`requirements.sdk.graph.agent-harness.agent_config.settings`.
Habeas flattens those fields into each named authoring configuration.

`ModelRef` MUST be derived from the selected backend configuration and MUST
preserve the exact model identifier. It MUST identify the model without replacing
connection settings such as AWS profile, region, or Ollama endpoint.
:::

:::{req} configuration_serialization
{impl}`requirements.configuration.feature_sections`
The user `[authoring]` table MUST serialize UserAuthoringConfig, and the workspace
table MUST serialize WorkspaceAuthoringConfig. Each
`[authoring.configs.<name>]` table MUST serialize one AuthoringConfig, with
`backend = "bedrock"`, `backend = "ollama"`, or `backend = "openai"` and that variant's fields in the same
table. Backend-specific inference options MUST appear in its `options` subtable.
Within a present user authoring section, `configs` MUST be present but MAY be empty;
workspace `configs` MAY be omitted, allowing a workspace default to select a
named user configuration.
omitted `projects` and `options` tables MUST deserialize as empty maps.
Serialization MUST follow the common rules in
{cite}`requirements.configuration.serialization`.
:::

:::{req} openai_defaults
{impl}`requirements.configuration.feature_sections`
An OpenAI configuration MAY omit `model`; the effective model MUST then be
`gpt-6.1-sol`. When `options.reasoning.effort` is omitted, the request MUST use
`low` effort, preserving other reasoning options. When `options.max_output_tokens`
is omitted, the request MUST use an output-token limit of 16,384. These option
defaults MUST apply even when the model is overridden. An explicit model,
reasoning effort, or output-token limit MUST override its corresponding default.
Defaults MUST NOT be written into omitted configuration fields on serialization.
An explicitly empty model MUST still be rejected.
:::

:::{req} adaptive
Adaptive is experimental and requires explicit opt-in. For the OpenAI backend,
`model = "adaptive"` MUST select models locally rather
than send `adaptive` to the API. An assessment with an applicable existing
FIXME reference MUST go directly to `gpt-6.1-sol`. Otherwise it MUST begin with
`gpt-5.4-mini`: a true judgment is accepted, while false, FIXME, or a failed
conversation/submission MUST trigger one fresh Sol assessment. Sol's result
MUST be final for that assessment, including a false judgment or failure.
Provider errors, cancellation, mandatory-input failures, connection failures,
and usage-storage failures MUST NOT trigger model escalation.

Applicable FIXMEs MUST follow the subject's existing scope rules: requirement
references outside its own distributed sites, or the reviewed site's own FIXMEs.
Citation reviews have no applicable acknowledgments. Matching a literal word
inside unrelated text MUST NOT trigger pre-escalation.

Both tiers MUST inherit the selected credentials, endpoint, and inference
options. Explicit concrete model IDs MUST bypass adaptive routing. Each actual
model attempt MUST have a separate usage record with its real model name;
the adaptive record MUST link its attempts and record any routing reason.
Escalation MUST NOT persist intermediate mini evidence or pass mini's rationale
to Sol. Under explicit `confirm`, each independent assessment MUST use adaptive
routing. The harness MUST select the outcome under {cite}`requirements.authoring.confirmation.selection`. Adaptive routing MUST NOT perform sampled audits.
:::

:::{req} policy
Every `habeas author` form MUST assess its subjects under an assessment policy,
`confirm` or `once`. The policy MUST come from `--policy <name>` when given,
otherwise from the selected configuration's `policy`, and otherwise MUST be
`once` for OpenAI and `confirm` for other backends. Under `confirm`, each subject MUST be assessed as
{cite}`requirements.authoring.confirmation` specifies. Under `once`, each subject
MUST receive a single assessment per round (including model escalation under
{cite}`requirements.authoring.adaptive`), whose result is recorded; it uses fewer tokens
and less time, such as with a slow local model. An unknown policy name MUST be
rejected as invalid configuration or arguments. The completion report MUST name
the policy used, and each usage record MUST identify it.
:::

:::{req} concurrency
Each named authoring configuration MUST accept an optional positive integer
`concurrency` in its configuration table. When omitted, the maximum number of
simultaneous assessments MUST default to the number of available logical CPUs
for Bedrock and OpenAI, falling back to one when that count is unavailable, and to one for
Ollama. A configured value MUST override the default for each backend; zero
MUST be rejected as invalid configuration.

Batch authoring MUST use asynchronous tasks on a runtime with a fixed worker
thread pool. The number of assessments running at once MUST NOT exceed the
selected concurrency. Each assessment MUST retain its own model conversation and
usage record. Transient provider failures, including throttling, MUST be
retried within the affected assessment under
{cite}`requirements.authoring.provider_backoff`; every other provider error MUST
be reported through the ordinary assessment-failure path.
:::

:::{req} configuration_storage
{impl}`requirements.configuration.feature_sections`
Configuration names MUST match `[A-Za-z0-9][A-Za-z0-9_-]*` and be case-sensitive.
Project-selection keys MUST be canonical absolute project-root paths. Model IDs
MUST be nonempty opaque strings, preserving tags and ARNs. `api_key_env` MUST
name a nonempty environment variable without `=` or NUL.

Authoring MUST validate its fields under {cite}`requirements.configuration.loading`.
The `options` map MUST accept backend-defined inference settings rather than a
closed Habeas list; the adapter MUST pass them through Rig's provider parameter
mechanism and report rejection by Rig or the provider. These options MUST NOT
replace the selected model, the assessment prompt, or tool definitions.
:::

:::{req} configuration_precedence
{impl}`requirements.configuration.feature_sections`
`--config <name>` MUST select a configuration for that invocation. Otherwise,
authoring MUST use the user's selection for the canonical project root, then the
workspace default, then the user's default. Named workspace configurations MUST
override same-named user configurations as complete records. Backend, model,
connection settings, and options MUST come from the selected record as a unit;
fields MUST NOT be merged between records. Workspace-only authoring MUST work
without a user configuration file. Missing configuration or an unknown selection
MUST identify the relevant configuration file and point to documented examples.

An explicit Bedrock AWS profile or region MUST override the corresponding SDK
default; omitted values MUST use standard AWS SDK resolution. Ollama MUST use
its configured endpoint or `http://localhost:11434`. If `api_key_env` is set, its
named variable MUST supply the credential; a missing or empty value MUST produce
an error before a model request. OpenAI MUST use its configured endpoint or
`https://api.openai.com/v1`, and MUST resolve `OPENAI_API_KEY` when neither
`api_key` nor `api_key_env` is supplied.
:::

:::{req} redaction
Habeas MUST apply the SDK's built-in credential rules to source text supplied
in mandatory inputs, tool reads, and search results, and to source text shown
in the book and `habeas info`. The same rules apply to configuration files.

Each match of a built-in `secretshape` rule MUST be replaced with
`[REDACTED:<rule name>]`, preserving every line break. The match MUST extend
through adjacent regions flagged by the entropy heuristic when only whitespace
separates them. Entropy alone MUST NOT trigger a replacement.

Outlines MUST use the original source for structure and spans. Structural
hashes MUST use the redacted text for each matched region as specified in
{cite}`requirements.sdk.sources.redaction`. The `secretshape` version MUST be
pinned exactly because changes to its rules can affect evidence hashes.
:::

:::{req} credentials
Bedrock configurations MAY supply `credentials` containing nonempty
`access_key_id`, `secret_access_key`, and optional nonempty `session_token`.
Ollama and OpenAI configurations MAY supply a nonempty `api_key`. Habeas MUST reject a
Bedrock record combining explicit credentials with `aws_profile`, and an Ollama or OpenAI
record combining `api_key` with `api_key_env`.

Explicit credentials MUST be passed directly to the selected provider without
changing process environment variables or writing credential files. Saved
explicit values MUST round-trip; credentials resolved from profiles or the
environment MUST NOT be serialized back. Debug output, configuration diagnostics,
and usage records MUST omit configured credential values. The harness MUST omit
provider credentials when constructing initial prompts. TOML
diagnostics MUST NOT echo credential-containing source lines. A configuration
file read as source, including as a prompt input, MUST follow {cite}`requirements.authoring.redaction`.
Validation MUST report a workspace `habeas.toml` present in the Git index that
contains explicit credentials, naming the file and the credential fields and
recommending credential-source references, because committed credentials leak.

Documentation MUST discourage literal credentials in workspace files and
recommend AWS profiles or environment-variable references. Examples MUST use
placeholders and explain that workspace files may be committed or read as source
when visible under the project’s `.gitignore` rules.
:::

:::{req} output_limit
When a Bedrock configuration sets no `maxTokens` option, each provider request
MUST ask for at most 16,384 output tokens. A configured `maxTokens` MUST be used
as given.

When a response stops because it reached a known output limit, the harness MUST
continue the same conversation once, adding a user message with an error result
for each tool call in the cut-off response, followed by this text with `<limit>`
replaced by the limit:

```text
The response was cut off at <limit> tokens. Continue with a shorter response and finish with submit_result.
```

A second response that reaches the limit, or a response that stops at an
output limit that is not known, MUST fail the assessment as an incomplete
response. Where the provider reports whether a response stopped at the output
limit or because the conversation exceeded the model's context window, the
harness SHOULD continue only the former and fail the assessment on the latter;
where it does not, the stop MUST be treated as reaching the output limit.
:::

:::{req} prompt_parts
Each assessment conversation MUST send its prompt as a system prompt followed by
one user message, the subject message, filled from the first and second text
blocks of the prompt requirement for the assessment's kind. The kinds are
test-adequacy requirement, structural requirement, example, citation review, site
review. A system prompt MUST be the same for
every assessment of its kind, so every placeholder whose value depends on the
subject MUST be in the subject message.
:::

:::{req} cache_checkpoints
For backends that accept explicit cache checkpoints, when the backend and model
support prompt caching, every provider request MUST place one cache checkpoint
after the system prompt of {cite}`requirements.authoring.prompt_parts` and
another at the end of its last message, so each request reuses the tool
definitions and system prompt that earlier assessments of its kind sent, and
the conversation its own earlier requests sent. Cache checkpoints MUST NOT
change the content of a request.

For OpenAI, which caches matching prompt prefixes automatically, the harness
MUST keep the system prompt and tool definitions unchanged across requests in
one conversation and replay prior conversation content in its original order.
It MUST NOT insert checkpoint blocks.
:::

:::{req} cache_warmup
So that concurrent assessments find their system prompt cached under
{cite}`requirements.authoring.cache_checkpoints`, a batch MUST start one
assessment of each kind it contains before any other of that kind, and start the
others once its first conversation with a provider attempt finishes, including a failed conversation.
:::

:::{req} rig_configuration_mapping
Habeas MUST resolve its saved configuration before constructing Rig provider
clients and completion models. The Habeas data structures MUST remain the saved
format. Rig model references MAY be reused internally without discarding the
connection settings they do not represent.

The Bedrock adapter MUST pass the chosen AWS profile and region through AWS SDK
configuration, or use explicitly configured credentials. The Ollama adapter MUST use the chosen server, exact model ID,
and optional authentication; the server MAY be local or remote. The OpenAI adapter MUST use the Responses API, selected endpoint, exact model
ID, and resolved API key. All adapters
MUST supply the same tools and initial prompt.
:::

### Editing the file

:::{req} configuration_documentation
Authoring documentation MUST follow {cite}`requirements.configuration.documentation`
and include complete TOML examples for Bedrock, Ollama, and OpenAI, with all placeholders
identified. It MUST show how to choose a default, use a remote Ollama server,
select a configuration for a project or invocation, and edit or remove selections
by hand. It MUST explain backend-specific options and credential-source references.
:::

Create the user file at the [shared configuration location](configuration.md#locations-and-scope).
For workspace settings, create `habeas.toml` in the directory whose descendants
should use them. The nearest enclosing file wins. This complete workspace `habeas.toml`
selects the user's saved `cloud` configuration and configures saved-evidence validation for a generated
source directory and its private subtree.
{impl}`requirements.configuration.documentation`
{impl}`requirements.authoring.configuration_documentation`

```toml
version = 1

[authoring]
default = "cloud"

[evidence]
allow = ["generated"]
disallow = ["generated/private"]
```

Replace `generated` and `generated/private` with your own paths.
Workspace files can also contain `[authoring.configs.<name>]` tables using the
same backend structures shown below. Those records override same-named user
records as a whole. A user's explicit project selection takes precedence over a
workspace default; `--config` takes precedence over both. Remove the workspace
default or configuration table to fall back to the user settings.

For Ollama, this is a complete minimal user `config.toml`; it can also serve as
a workspace `habeas.toml`. Replace `MODEL_TAG` with a model identifier available
on the local server:

```toml
version = 1

[authoring]
default = "local"

[authoring.configs.local]
backend = "ollama"
model = "MODEL_TAG"
```

For Bedrock, this is a complete minimal user `config.toml`; it can also serve as
a workspace `habeas.toml`. Replace `MODEL_ID` with an available
model or inference-profile identifier, `research` with your AWS profile name,
and `us-east-1` with your region:

```toml
version = 1

[authoring]
default = "cloud"

[authoring.configs.cloud]
backend = "bedrock"
model = "MODEL_ID"
aws_profile = "research"
region = "us-east-1"
```

This complete user `config.toml` combines Bedrock, local Ollama, and remote
Ollama. Its `[authoring.projects]` table is valid only in the user file, never
in a workspace file. Replace
`MODEL_ID` with an available Bedrock model or inference-profile identifier,
`research` with your AWS profile name, `us-east-1` with your region, `MODEL_TAG`
with an Ollama model identifier, `workstation` in the URL with your server, and
`/absolute/path/to/project` with the canonical path to your checkout.

```toml
version = 1

[authoring]
default = "cloud"

[authoring.configs.cloud]
backend = "bedrock"
model = "MODEL_ID"
aws_profile = "research"
region = "us-east-1"

[authoring.configs.local]
backend = "ollama"
model = "MODEL_TAG"

[authoring.configs.workstation]
backend = "ollama"
model = "MODEL_TAG"
endpoint = "http://workstation:11434"

[authoring.projects."/absolute/path/to/project"]
config = "workstation"
```

Edit `authoring.default` to change the user default, or the project entry's
`config` to change that checkout's selection. Delete the corresponding field or
project table to remove a selection. To remove a named configuration, delete its
table and update any selections naming it. Preserve other feature sections when
editing an existing file.

Run `habeas author --all` to fill in evidence needing attention, or
`habeas author <full-name>` to author one requirement or example; add `--force`
to reassess current evidence. Add `--config local`
to override the saved selection for one run. Set `concurrency` alongside `backend`
and `model` to override the provider's default number of simultaneous assessments.
This complete user `config.toml` sets it for the `cloud` configuration; the same
table can appear in a workspace `habeas.toml`:

```toml
version = 1

[authoring]
default = "cloud"

[authoring.configs.cloud]
backend = "bedrock"
model = "MODEL_ID"
concurrency = 4
```

Here `MODEL_ID` is your available model identifier and four is an illustrative
choice, not the default. Bedrock and OpenAI default to the available logical CPU count;
Ollama defaults to one. Lower concurrency if the provider reports throttling;
rerunning `habeas author --all` preserves current accepted results and retries
records that still need attention.

Backend options belong under
`[authoring.configs.<name>.options]`. For an authenticated Ollama server, put
`api_key_env = "OLLAMA_API_KEY"` in its configuration table and set that variable
outside the file. Omit `aws_profile` or `region` to use AWS SDK defaults; omit an
Ollama endpoint to use localhost.

For OpenAI, set `OPENAI_API_KEY` to your API key outside the file and use this
complete user `config.toml` with a credential-source reference. The default
model is `gpt-6.1-sol`, with low reasoning effort and an output-token limit
of 16,384. Model and inference settings remain overridable.

```toml
version = 1

[authoring]
default = "openai"

[authoring.configs.openai]
backend = "openai"
api_key_env = "OPENAI_API_KEY"
```

You may omit `api_key_env` to use `OPENAI_API_KEY` implicitly, or name a different
environment variable. To keep the key in your user file instead, replace
`api_key_env` with `api_key = "OPENAI_API_KEY_VALUE"`, substituting your key for
`OPENAI_API_KEY_VALUE`. OpenAI uses
the Responses API at `https://api.openai.com/v1`; `endpoint` overrides that base URL.

To opt into experimental adaptive routing, set `model = "adaptive"`. It starts
with GPT-5.4-mini and escalates false/FIXME judgments or failed submissions to
Sol; applicable existing FIXMEs go directly to Sol. It accepts mini's `true`
judgments without a Sol audit and missed known gaps in calibration. Adaptive
is a harness model, not an API model ID. Its prompt and routing need further
evaluation before use as a default.

To override the defaults, add `model = "gpt-6-luna"` to the profile and/or
`reasoning = { effort = "medium" }` under its `options` table. Set
`max_output_tokens = 8192` in that same options table to override the output
limit. The `low` effort and 16,384-token defaults also apply to other OpenAI
models; override them with values supported by the selected model. Inference
options are Responses API parameters.

OpenAI defaults to `policy = "once"`: one assessment per subject in each round, including
adaptive escalation if needed. Explicit `policy = "confirm"` independently
reassesses true or FIXME results once and records the less favorable outcome. Other backends continue to default to `confirm`. `concurrency = 1` assesses batch subjects
one at a time; if omitted, OpenAI defaults to the available logical CPU count.
These settings are optional and independent of model selection.

For a model comparison, create one named configuration per model, keep the
policy, output limit, and subject set fixed, and run each on a separate copy
of the same evidence baseline. Existing accepted evidence can otherwise skip
assessment. Compare correctness against your reviewed determinations and
rationales, tool failures, completion rate, elapsed time, and tokens in the
usage records. Start with one concurrent assessment and a small subject set
before increasing the experiment size. Specify each comparison model explicitly.

Literal credentials are supported but not recommended, especially in a
workspace file that might be committed. Prefer AWS profiles (`aws_profile`) or
environment-variable references (`api_key_env`).
Workspace files may be committed or read as source when visible under the project’s `.gitignore` rules.
If needed, this complete `habeas.toml` illustrates the fields;
replace every uppercase placeholder with your own value and keep the resulting
file out of version control. Omit `session_token` for credentials without one.

```toml
version = 1

[authoring]
default = "cloud"

[authoring.configs.cloud]
backend = "bedrock"
model = "MODEL_ID"
region = "AWS_REGION"

[authoring.configs.cloud.credentials]
access_key_id = "AWS_ACCESS_KEY_ID"
secret_access_key = "AWS_SECRET_ACCESS_KEY"
session_token = "AWS_SESSION_TOKEN"

[authoring.configs.local]
backend = "ollama"
model = "MODEL_TAG"
api_key = "OLLAMA_API_KEY_VALUE"
```

Do not combine `credentials` with `aws_profile`, or `api_key` with `api_key_env`.
An agent reading a configuration file can see literal values that the SDK’s
built-in credential rules do not recognize. Validation reports a committed workspace file that
contains literal credentials; keep such files out of version control or use
credential sources instead.
{impl}`requirements.authoring.credentials`

## Recording results

:::{req} delivered_inputs
The harness MUST record the sources it supplies to the model, including
mandatory inputs, additional reads, and selected search dependencies under
{cite}`requirements.authoring.finish_search.inputs`, using the evidence input formats
defined by {cite}`requirements.ci.outline_inputs`,
{cite}`requirements.ci.requirements_inputs`, and
{cite}`requirements.definitions.inputs`. The harness MUST create these records
itself so ordinary evidence validation can check them later. For citation assessments, additional node reads and selected excerpts are recorded as SDK dependencies under {cite}`requirements.sdk.graph.search.finish_search.inputs` and {cite}`requirements.sdk.graph.validation-context.read_node.inputs`.
:::

:::{req} persistence
After policy selection in each round, the harness MUST write the subject text,
mapped determination, rationale, and recorded inputs under {cite}`requirements.ci.schema`.
Each of the three outcomes counts as a completed assessment. Its validation
status follows {cite}`requirements.ci.determination`.
:::

:::{req} persistence.destination
By default, the harness MUST write the subject's evidence file, replacing earlier
evidence on each completed reassessment. With `--stdout`, it MUST emit one
evidence JSON object followed by a newline and leave evidence files and
directories untouched.
:::

:::{req} persistence.corrections
Versioned evidence for a failed definition assessment MUST retain the selected
correction list under {cite}`requirements.sdk.habeas.evidence_format`. The stored
evidence rationale MUST render each action or instruction with its own rationale,
in submission order, without another model call.
:::

:::{req} persistence.fingerprints
Applying a source edit MUST preserve the input fingerprints of every recorded
assessment. A completed reassessment records its own observed inputs under
{cite}`requirements.authoring.delivered_inputs`.
:::

### Usage history

:::{req} usage_storage
Each assessment MUST have a separate JSON usage record under
`<state-root>/authoring/usage/<run-id>.json` using
{cite}`requirements.configuration.state_location`.
It MUST identify its format version, run, project root, subject kind and full
name, start time, backend and model when known, end time when finished, outcome,
accepted determination kind (`true`, `false`, or `FIXME`) when present, and
whether evidence was written. It MUST record the selected output destination in
`evidence_output` as `file` or `stdout`; a successful write to either destination
MUST set `evidence_written` to true. Usage recording MUST remain outside the
project in stdout mode. FIXME entry text MUST NOT be stored in usage records.
Failed, cancelled, or unfinished
runs MUST remain distinguishable from completed judgments. Records MUST persist
for later review without storing source text, prompts, responses, or rationale.
:::

:::{req} usage_inputs
Each assessment’s usage record MUST report its validator’s `prepared_inputs` and `delivered_inputs` under {cite}`requirements.sdk.graph.running-criteria.UsageStats`. It MUST also report the invocation count and total result bytes for each invoked tool under {cite}`requirements.sdk.graph.running-criteria.ToolUsage.calls` and {cite}`requirements.sdk.graph.running-criteria.ToolUsage.result_bytes`.
:::

:::{example} file_discovery_usage
{cite}`requirements.authoring.usage_inputs`
{cite}`requirements.sdk.graph.validation-context.list_sources.behavior`

`list_sources` returns the name `src/parser.rs`. The assessment counts one `list_sources` invocation and the bytes of the whole listing. Showing the filename leaves `prepared_inputs` and `delivered_inputs` unchanged. If the agent then reads a function in `src/parser.rs`, the function’s contents become an evidence input when delivered to the model.
:::

:::{req} usage_requests.failures
Each assessment’s usage record MUST include its total failed provider attempts and retries, including failures recovered by a successful retry. A terminal failure MUST retain the accumulated counts.
:::

:::{req} usage_requests
HC MUST use one {term}`requirements.sdk.graph.running-criteria.validator` per assessment. Its usage record MUST report that validator’s attempt and token totals under {cite}`requirements.sdk.graph.running-criteria.Validator.usage` and its failure and retry counts under {cite}`requirements.authoring.usage_requests.failures`. The record MUST distinguish partial or unavailable token measurements from complete totals and preserve the available cache and reasoning breakdowns. The assessment MUST also report elapsed time.
:::

### CI without inference

:::{req} deterministic_validation
Ordinary evidence validation, status inspection, and book rendering MUST continue
to check evidence using the existing rules without invoking models, contacting
providers, or requiring authoring configuration or credentials. Missing or stale
evidence MUST remain a validation failure rather than trigger authoring.
:::

## Examples

:::{example} complete_initial_bundle
A requirement has an implementation reference in one function, a test reference
in another, and a terminology reference. The initial prompt contains the complete
requirement, the test function's source text, and the required terminology
definition as evidence inputs. The implementation reference alone does not add
that function's source to the initial prompt. Following the requirement’s `Implements` edges identifies that function. Reading it with `read_node` supplies its source and records its fingerprint.
{cite}`requirements.authoring_input`
{cite}`requirements.references.authoring_inputs`
{cite}`requirements.definitions.authoring_inputs`
{cite}`requirements.authoring.mandatory_inputs`
{cite}`requirements.authoring.initial_prompt`
:::

:::{example} additional_read
The model calls `list_sources` to discover `src/lib.rs`, then `search_text` to find a matching function. The search returns its node address and a source excerpt. Reading the function with `read_node` records the function as a dependency. The agent closes the search with `finish_search` and an empty `used` list because it relied on the full node read.
{cite}`requirements.sdk.graph.validation-context.list_sources.behavior`
{cite}`requirements.sdk.graph.search.search_text.mcp`
{cite}`requirements.authoring.source_tools`
{cite}`requirements.authoring.delivered_inputs`
:::

:::{example} negative_assessment
The model reads additional sources and calls `submit_result` once with
`Failed` with one `Manual` correction. Its instruction asks for an assertion
that the returned page ID matches the requested ID. Its rationale explains that
the current test checks only that a page is returned, so returning the wrong
page would pass. Habeas saves the negative judgment, corrections,
and observed inputs, reports the instruction, and pauses the subject. Ordinary
evidence validation reports the false determination.
{cite}`requirements.authoring.submission`
{cite}`requirements.authoring.delivered_inputs`
{cite}`requirements.authoring.persistence.corrections`
:::

:::{example} successive_references
An assessment finds an existing paging test and proposes its missing `{test}`
reference. Habeas applies it. The fresh assessment sees the newly linked test
and identifies a second valid reference on another item. Habeas applies that
reference and reassesses again. Only the third assessment, which finds no gap,
writes `true`. If it instead requests only the marker that is already present,
Habeas retains its negative judgment and reports why it stopped.
{cite}`requirements.authoring.reassessment`
:::

:::{example} acknowledged_gap
The tests omit a boundary case explicitly required by a requirement. A fenced
`{fixme}` reference to that requirement describes the missing test and names
`:github: example/app#125`. The model receives that acknowledgment and its open
issue state without the containing implementation source. It finds no other gaps
and calls `submit_result` once with
`AcknowledgedGap` with `entries` equal to `["example/app#125"]` and a rationale locating the
acknowledgment and explaining the untested boundary. The harness saves that
judgment, the source inputs it supplied, and the reference-set digest covering
the acknowledgment. CI accepts
the FIXME judgment when all other evidence checks pass.
{cite}`requirements.ci.outcomes`
{cite}`requirements.ci.determination`
{cite}`requirements.authoring.submission`
{cite}`requirements.authoring.delivered_inputs`
{cite}`requirements.authoring.persistence`
:::

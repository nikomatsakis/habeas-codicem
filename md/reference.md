# How it works

Habeas reads requirements from an mdBook and evidence from JSON files in the same repository. `cargo test --workspace` checks the evidence; `mdbook build` renders links to it.

## Requirements

Write a colon-fenced `{req}` directive in a book chapter. For example, in `md/requirements.md`:

```text
:::{req} check
The CLI MUST report invalid arguments.
:::
```

The chapter path is part of the requirement's name, so this one is `requirements.check`. Everything between the opening and closing fences is its body, including headings and other Markdown.

## Examples

An example is a separate named definition for a concrete case. State the situation, action, and expected result, and reference the requirement it illustrates: `{cite}` makes the requirement context for the example's own assessment, while `{test}` makes the example part of the requirement's assessment. An example may use both. This book contains a working example:

:::{example} cli.unknown_command
Running `habeas unknown` fails with a nonzero exit status and prints a usage message. {cite}`requirements.cli.invocation`
:::

Its full name is `reference.cli.unknown_command`. The `unknown_command_fails_with_usage` integration test targets it with `{test}` followed by the backtick-delimited full name. The authoring harness records `evidence/reference.cli.unknown_command.json`, including the example's exact body, test source lines, and a rationale explaining how the test covers the case. A passing validation check confirms the references are current; the rationale explains why the tests are adequate.

## Evidence

Run `habeas author --all` to create and refresh evidence for requirements and
examples. The harness writes `evidence/requirements.check.json`
for the requirement above and `evidence/reference.cli.unknown_command.json` for the
example. Each requirement or example evidence file records four fields:

| Field | Meaning |
| --- | --- |
| `requirement` or `example` | The complete body, copied from the corresponding directive. Use the field matching its kind. |
| `determination` | `true`, `false`, or `{"FIXME": ["#125"]}` for explicitly documented gaps. |
| `rationale` | A Markdown explanation of that judgment. |
| `inputs` | Source snapshots supporting it. Inputs may select a file range, Rust item, or definition selection. |

For a requirement, the model reviews whether the code marked `{impl}` implements its
stated obligations completely and exactly, and whether tests cover the interesting
design cases. It also checks that the implementation is marked completely: code
that implements part of the requirement without `{impl}`, including code that only
cites it, is a gap, as is behavior in the `{impl}` code that neither this nor
another definition implemented there specifies. When another requirement
implements part of the subject, the review checks only that its text carries out
that part; that requirement is assessed separately. If there are no tests,
its rationale explains why the logic is simple enough to need none or testing is impractical.
For an example, the model reviews whether tests cover its scenario and expected outcomes;
a true judgment requires a covered `{test}` reference. The assessment concerns
the named subject, using referenced requirements and definitions as context.
{cite}`requirements.ci.review_criteria` {cite}`requirements.examples.evidence`

The model chooses `true` when that review supports the subject and FIXME when
source comments or tests explicitly acknowledge the remaining implementation or
testing gaps; otherwise it chooses `false` for gaps or missing information.
The FIXME array records the acknowledged gaps' issue references or other
identifying source content, with each gap and source location explained in the
rationale. The expected comment
convention is `FIXME(#125)` or `FIXME(rust-lang/rust#123)`, but the reference may
appear elsewhere in the comment or test. The harness captures those source inputs.
{cite}`requirements.ci.outcomes`

An input has exactly one of these forms. `text` always stores complete raw lines, so comments remain available to the reviewer:

```json
{"file":"src/main.rs","line_start":1,"line_end":3,"text":["..."]}
{"rust_file":"src/main.rs","rust_item":"fn[main]","text":["..."]}
{"requirements":"requirements.cli.*","matches":["requirements.cli.info"],"text":["..."]}
```

`habeas author --all` captures source text and delegates judgments to the configured
model. The supervising agent fixes reported gaps and reruns the command; it does
not write evidence by hand. The lower-level `habeas add-input` command remains
available for selecting an additional input. A changed input leaves its
determination false until the authoring harness reassesses it.
{cite}`requirements.authoring.batch` {cite}`requirements.cli.add_input` {cite}`requirements.ci.schema`

```sh
habeas add-input --for-requirement requirements.ci.inputs --file-range src/evidence.rs:1-20
habeas add-input --for-example reference.cli.unknown_command --rust-item tests/cli.rs:26
habeas add-input --for-requirement requirements.ci.inputs --requirements 'requirements.ci.*'
```

The `--rust-item <file>:<line>` mode selects all explicit Rust items declared on that line. A named item is declared where its name appears; an unnamed item is declared where its keyword appears. Rust-item snapshots include attached comments, doc comments, and attributes. Moving unchanged lines earlier or later in the file does not stale the snapshot. A requirements input takes a definition selection, either a full name or a glob where `*` matches any characters including dots. It includes requirements and named examples, records every matched name, and stores the full raw fenced declarations in sorted name order. Changes to a matched declaration or the match set invalidate the input. {cite}`requirements.cli.add_input_file_range` {cite}`requirements.cli.add_input_rust_item` {cite}`requirements.cli.add_input_requirements` {cite}`requirements.ci.rust_item_inputs` {cite}`requirements.ci.requirements_inputs`

Habeas checks that every requirement and example has evidence, that every evidence file names a definition, and that recorded text and input lines still match the repository. An example with a true determination also needs a `{test}` reference. Each determination is checked on its own: a false judgment does not fail the definitions its subject implements, tests, or cites. {cite}`requirements.ci.determination`

## Authoring evidence with a model

`habeas author --all` fills in missing, stale, or invalid evidence for
requirements and named examples. It preserves current records, including FIXME
judgments and current false judgments. A false judgment is reassessed when its
subject or inputs change, or when you name it with `habeas author <full-name>`.
Each assessment
gets its own conversation and usage record. Successful results are kept if
another assessment fails; the command reports failures and exits unsuccessfully.
A completed false judgment is saved and remains a validation failure.
{cite}`requirements.authoring.batch`

In a terminal, inspection shows a progress bar for each phase once its amount of
work is known, with a phase label and completed/total checks. Discovery uses a
spinner until a total is available. During authoring, the bar shows elapsed time, finished/total,
active and queued assessments, and separate `true`, `false`, `FIXME`, and error
counts. Progress measures work finished; a false judgment still fails validation.
The bar appears on stderr, leaving the stdout summary unchanged. Redirected
stderr keeps the plain reports; `TERM=dumb` also uses plain output. `NO_COLOR`
keeps the progress display without color. {cite}`requirements.authoring.progress`

Completed `true` assessments do not print individual completion lines. Each
`false` judgment prints its subject and outcome on stderr so it can be followed
up. FIXME notices, authoring errors, and the aggregate summary remain visible.
Add `-v` or `--verbose` to include each false judgment's stored rationale,
without input snapshots or additional inference:

```sh
habeas author --all --config cloud -v
habeas author requirements.ci.inputs --verbose
```

This also works with `--stdout`: stdout remains evidence JSON and the verbose
rationale goes to stderr. {cite}`requirements.authoring.reporting`

Transient provider token/request throttling is retried automatically with
exponential backoff and jitter, honoring `Retry-After` when the provider exposes
it. Every attempt is counted in usage. Retries have no attempt limit; Ctrl-C
interrupts waiting. Requests that exceed a model's input/context limit still fail
because waiting cannot make that request fit. {cite}`requirements.authoring.provider_backoff`

`habeas author <full-name>` assesses one requirement or example, records the
source inputs delivered to the model, and writes its judgment to the usual
evidence file. `author-evidence` remains an alias. The model can read sources
with `habeas`, list files, search them with ripgrep (built in; no `rg` executable
is needed), and use `references` to list definition names or every `{impl}`,
`{cite}`, and `{test}` site for a definition selection. Listings, searches, and
reference lookups are not recorded as evidence.
Choose a model that supports tool calls, either on Amazon Bedrock or an accessible
Ollama server. The server can run on another machine; Habeas does not need a GPU.
{cite}`requirements.authoring.invocation` {cite}`requirements.authoring.provider`
{cite}`requirements.authoring.text_search`
{cite}`requirements.authoring.reference_index`

In a Git worktree, stage new source files with `git add` before authoring. The
model can read unstaged edits to tracked files. By default, reads exclude untracked files,
generated `evidence/` content, and paths outside the project, including symlink
aliases. Standalone projects use the same containment rules without requiring Git.
An explicitly requested excluded path aborts that assessment and reports an error
to the caller; it is not sent back to the model to work around. In a batch, other
assessments continue and successful writes are retained. The command exits
unsuccessfully after the batch finishes.
{cite}`requirements.authoring.source_tools`
{cite}`requirements.authoring.read_denial`

The nearest enclosing `habeas.toml` supplies workspace settings. Relative paths
in this file use its directory as their base. This complete example selects a
user configuration named `cloud` and allows a generated directory except for one
subtree:

```toml
version = 1

[authoring]
default = "cloud"

[evidence]
allow = ["generated"]
disallow = ["generated/private"]
```

An allow overrides all defaults; disallow wins if both match. Paths name literal
files or directory subtrees, not globs. Absolute paths can allow external inputs.
Workspace files can also define `[authoring.configs.<name>]` records, which
replace same-named user records as a whole. No ancestor workspace files are
merged. Selection order is `--config`, the user's per-project selection, workspace
default, then user default.
{cite}`requirements.configuration.workspace_location`
{cite}`requirements.configuration.evidence_paths`
{cite}`requirements.authoring.configuration_precedence`

Existing Cargo build-script output directories are automatically allowed when
discoverable from the current build layout, including external build directories.
No build is run to find or recreate them. Root listings expose discovered output
directories, and workspace disallow rules still win. For an undiscovered layout,
add its output directory to `allow`. CI must generate the recorded paths and
contents before validating their evidence.
{cite}`requirements.authoring.generated_inputs`

Create and edit `$XDG_CONFIG_HOME/habeas/config.toml`, or
`$HOME/.config/habeas/config.toml` when XDG_CONFIG_HOME is absent, empty, or
relative. On Windows, use `%APPDATA%/habeas/config.toml`. This file is user-local
and is not part of the project. Here is a complete configuration with two choices:
{cite}`requirements.configuration.user_location`
{impl}`requirements.authoring.configuration_documentation`

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
```

Replace `MODEL_ID` with your available Bedrock model or inference-profile ID,
`research` with your configured AWS profile, `us-east-1` with your region, and
`MODEL_TAG` with a model installed on your Ollama server. Omit `aws_profile` and
`region` to use standard AWS SDK resolution. Ollama defaults to
`http://localhost:11434`; add `endpoint = "http://workstation:11434"` to its table
for a remote server, replacing `workstation` with that server's host name. For
authentication, add `api_key_env = "OLLAMA_API_KEY"` and set that variable outside
the file. Profiles and environment references are preferred. Literal Bedrock
`credentials` or Ollama `api_key` values are supported but not recommended in
workspace files; see the [credential examples](requirements/authoring.md#editing-the-file).
{cite}`requirements.authoring.configuration_precedence`
{cite}`requirements.authoring.credentials`

Run from the project:

```sh
habeas author --all
habeas author --all --config local
```

For a single requirement or example, replace `requirements.check` with its full name:

```sh
habeas author requirements.check
habeas author requirements.check --config local
habeas info requirements.check
```

Use `--stdout` to emit the evidence JSON followed by a newline without creating
or changing evidence files or directories. The completion summary, usage-record
path, and diagnostics go to stderr, so stdout can be redirected or piped:

```sh
habeas author requirements.check --stdout > /tmp/assessment.json
```

All three judgments (`true`, `false`, and FIXME) produce evidence JSON. An
assessment that fails before accepting a valid judgment emits no evidence JSON.
`--stdout` is available for one named subject; it cannot be combined with `--all`.
Usage is still recorded outside the project, with `evidence_output` identifying
the selected `file` or `stdout` destination and `evidence_written` reporting
whether the write succeeded.
{cite}`requirements.authoring.invocation`
{cite}`requirements.authoring.submission`
{cite}`requirements.authoring.persistence`
{cite}`requirements.authoring.usage_storage`

Change `authoring.default` to switch the default, or add a per-project selection
using its canonical absolute path (replace `/absolute/path/to/project`):

```toml
[authoring.projects."/absolute/path/to/project"]
config = "local"
```

The invocation's `--config` wins over that project selection, which wins over the
default. Remove the project table or default field to remove a selection. Delete
a configuration's table to remove it, updating selections that named it. Provider
inference options go under `[authoring.configs.<name>.options]`; supported keys
depend on the provider, and cannot replace the model, prompt, or tools. See the
[configuration reference](requirements/authoring.md#configuration) for the full
format. {cite}`requirements.authoring.configuration_storage`
{impl}`requirements.authoring.configuration_documentation`

Bedrock runs assessments concurrently by default, using the available logical CPU
count. Ollama runs one at a time. Override either default with a positive
`concurrency` value alongside `backend` and `model` in its configuration table:

```toml
[authoring.configs.cloud]
backend = "bedrock"
model = "MODEL_ID"
concurrency = 4
```

Four is an example, not a fixed default. Authoring uses asynchronous tasks on a
fixed worker thread pool and limits simultaneous assessments to this value.
Lower it if Bedrock reports throttling, then rerun `habeas author --all` to work
on records that still need attention. {cite}`requirements.authoring.concurrency`

For Bedrock, put the Converse names `temperature`, `maxTokens`, `topP`, and
`stopSequences` directly in that options table. Habeas maps them to Converse's
`inferenceConfig`; other keys become model-specific `additionalModelRequestFields`.
Use these flat keys without an `inferenceConfig` wrapper. For example:

```toml
[authoring.configs.cloud.options]
temperature = 0.25
maxTokens = 321
topP = 0.75
stopSequences = ["END"]
```

These values are illustrative choices, not Habeas defaults. Omit any setting to
leave it to the provider. For Ollama, native options such as `num_ctx` and `seed`
go in the same options table; Rig places them in Ollama's native request.
{cite}`requirements.authoring.configuration_storage`
{cite}`requirements.authoring.rig_configuration_mapping`

The command reports the evidence destination and usage-record path. Usage
metadata is kept under `$XDG_STATE_HOME/habeas/authoring/usage`, falling back to
`$HOME/.local/state/habeas/authoring/usage`; Windows uses
`%LOCALAPPDATA%/habeas/state/authoring/usage`. Ordinary CI validation, status, and
book rendering remain deterministic and do not require model configuration or
credentials. {cite}`requirements.authoring.usage_storage`
{cite}`requirements.authoring.deterministic_validation`

### Live Bedrock tests

`cargo test` includes a live Bedrock integration test. It skips with a visible
warning in CI or when the AWS SDK cannot load credentials. Outside CI, a missing
Bedrock authoring configuration fails the test. A skip returns successfully, so
Cargo displays `ok` rather than `ignored`. Once credentials load, provider and
authoring errors fail the test. Local runs with credentials make billable model
requests and retain their usage records in the usual user state directory.

Run just these tests with:

```sh
cargo test --test live_bedrock
```

The live test uses the project's or user's selected Bedrock configuration,
falling back to the first named Bedrock configuration in name order. To select
one explicitly:

```sh
HABEAS_LIVE_CONFIG=sonnet cargo test --test live_bedrock
```

The test uses a temporary project and stdout evidence output, leaving the
checkout's evidence unchanged. Any set `CI` environment variable disables live
testing before configuration or credential lookup, even if credentials are
available. For example, `CI=1 cargo test` runs the suite without live inference.
{cite}`requirements.integration.live_bedrock_test`

## References

Source files and book chapters can mark lines as implementing, citing, or testing a requirement or example:

```text
{impl}`requirements.check`
{cite}`requirements.check`
{test}`requirements.check`
{test}`reference.cli.unknown_command`
```

Each reference must name an existing requirement or example and is a directed edge between its source and that definition. Choose the role by what the source does:

- `{impl}`: the source implements part of the definition. It makes a decision, performs a check, produces an output, or handles a case the definition specifies. Calling code that implements a definition, or producing a result a caller uses to meet it, is not implementing it. A requirement that refines another requirement's obligation can `{impl}` it. A convention for every output of a kind, such as every validation error, is implemented by the code that carries it out as a whole.
- `{test}`: the source is designed to verify behavior the definition specifies, either a test through its scenario and assertions, or a named example through its concrete case.
- `{cite}`: the source relies on, supports, or refers to the definition without implementing or testing it.

Only requirements and examples are assessed; code is reviewed as part of the definitions it implements or tests. `{impl}` and `{test}` point from the definition to the source, so the source becomes a mandatory input to the definition's assessment, and that definition's evidence must cover the reference line. `{cite}` points from the source to the definition: a citing requirement or example receives the cited declaration as context, and the citing site needs no evidence coverage. Marking only `{cite}` on code that implements part of a requirement leaves a gap in that requirement's review. In the book, references and definitions become linked badges.
{cite}`requirements.references.targets` {cite}`requirements.references.evidence_coverage` {cite}`requirements.references.authoring_inputs`

For Rust, Habeas parses first-party source files and requires every explicit function or method, including trait methods and test helpers, to have a requirement or named example reference in an attached comment or its body. Each named example references a requirement. A reference on an enclosing `impl` or module, in a string literal, or only in an evidence input does not cover the function. A Rust source file that cannot be parsed is reported as an error. For a helper with no behavior-specific requirement, agree on an explicit internal-detail requirement scoped to one area, such as `internal.config_parsing`, rather than one catch-all, and mark the helper with `{impl}`. That requirement's review checks that it explains each implementing function's behavior; it cannot justify otherwise unspecified user-visible behavior. {cite}`requirements.references.rust_function_coverage` {cite}`requirements.cli.internal_detail_guidance`

Rust-item selectors use syntactic ancestry within one repository-relative file. Examples include `mod[local]/fn[run]`, `impl[Thing]/fn[run]`, `impl[Thing/Trait]/fn[run]`, `trait[Trait]/fn[read]`, `fn[outer]/fn[inner]`, and positional `use#1`. Duplicate sibling names gain `#1`, `#2`, and so on. Delimiter characters and whitespace within names or impl types use percent-encoded UTF-8 bytes, so spelling can be recovered without resolving modules across files. {cite}`requirements.references.rust_item_selectors`

## Checking and viewing

The test created by `habeas setup` calls `habeas::validate()`. Run `cargo test --workspace` to find missing evidence, false determinations, changed lines, uncovered `{impl}` and `{test}` references, and unreferenced Rust functions. Validation accepts true and FIXME judgments when all other evidence checks pass. FIXME remains visible in status and the book with its recorded references and rationale. These checks are deterministic: they verify the saved evidence without inference or reassessing the reviewer's judgment about documented gaps. The check reports all the errors it finds, with diffs for changed text. Run `mdbook build` to render the book. {cite}`requirements.ci.determination`

Run `habeas status [<selection>]` from within the project to list requirements and named
examples needing attention and their validation findings. FIXME judgments are
summarized by issue, for example:

```text
- #123 — affects 22 judgments
- #45 — affects 3 judgments
- 16 judgments marked FIXME without an issue
```

Each judgment counts once per issue, even if it repeats that reference. A judgment
with several issues counts in each group; the final group counts judgments with
no recognized issue reference. References may appear in explanatory text or
`FIXME(...)`. Qualified references such as `rust-lang/rust#123` and GitHub issue
URLs are also recognized. Groups with the most affected judgments come first.
{cite}`requirements.cli.fixme_summary`

Use `--verbose` to list the actual judgments in each group, with their full names,
kinds, and recorded FIXME strings. `--all` shows every matching definition and its
evidence state. Stale or invalid FIXME evidence still reports its validation
findings. Use `habeas info <selection>` to print matching detail pages as Markdown,
including current texts, rationales, inputs, FIXME references, and findings. Both
commands take a definition selection: a full name selects exactly that definition,
and in a glob `*` can span any number of characters, including dots. {cite}`requirements.cli.status` {cite}`requirements.cli.info`
{cite}`requirements.references.definition_globs`

```sh
habeas info requirements.ci.inputs
habeas info 'requirements.cli.*'
habeas status 'requirements.cli.*' --all
habeas status --verbose
```

Agent skills installed by Habeas point to `habeas info` for looking up definition details.

To add an evidence index, list a chapter in `SUMMARY.md` and put this directive in it:

```text
:::{requirements-index}
:::
```

The build fills that chapter with requirement and example states and adds an evidence page under it for each definition.

## Using a local checkout

To use a checkout instead of the registry package, install the CLI from that checkout and point setup at it:

```sh
cargo install --path /path/to/habeas-codicem
habeas setup --path /path/to/habeas-codicem
```

Run `habeas setup` in the consuming Cargo and mdBook project. Its book uses the installed `habeas mdbook` CLI; the repository-local `cargo habeas` alias is only for Habeas's own book.

## Named definitions

Use a definition for shared vocabulary. It has a stable chapter-qualified name,
a short display phrase, and an explanatory body:

```text
:::{definition} first_parent_history
:phrase: first-parent history

The sequence obtained by starting at a Git commit and repeatedly
following its first parent until reaching a root commit.
:::
```

In `md/glossary.md`, this declares `glossary.first_parent_history`.
Write `{term}` followed by that name in backticks to insert an ordinary link
whose text is “first-parent history”, without a badge. The phrase is plain text.
Definitions do not have their own evidence records or determinations.

Evidence includes each referenced definition as a separate input:

```json
{"definition":"glossary.first_parent_history","text":[":::{definition} first_parent_history",":phrase: first-parent history","","The sequence obtained by starting at a Git commit and repeatedly","following its first parent until reaching a root commit.",":::"]}
```

`habeas add-input` automatically captures definitions referenced by the target's
requirement or example text and its reviewed inputs. It follows references between
definitions until no new definitions are found; cycles are allowed, and each
definition appears once. A requirements input's selection still selects only
requirements and examples. Their referenced definitions are captured regardless of name.

Changing a definition's phrase or body invalidates its snapshots. Refreshing inputs
leaves the evidence pending reassessment. Validation reports missing or stale
snapshots without modifying them. A term link does not replace a function's required
requirement or example reference.

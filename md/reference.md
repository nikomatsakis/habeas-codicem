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

An example is a separate named definition for a concrete case. State the situation, action, and expected result, and cite the requirement it illustrates. This book contains a working example:

:::{example} cli.unknown_command
Running `habeas unknown` fails with a nonzero exit status and prints a usage message. {cite}`requirements.cli.invocation`
:::

Its full name is `reference.cli.unknown_command`. The `unknown_command_fails_with_usage` integration test targets it with `{test}` followed by the backtick-delimited full name. The authoring harness records `evidence/reference.cli.unknown_command.json`, including the example's exact body, test source lines, and a rationale explaining how the test covers the case. A passing validation check confirms the citations are current; the rationale explains why the tests are adequate.

## Evidence

Run `habeas author --all` to create and refresh evidence for requirements, examples,
and Rust function attestations. The harness writes `evidence/requirements.check.json`
for the requirement above and `evidence/reference.cli.unknown_command.json` for the
example. Each requirement or example evidence file records four fields:

| Field | Meaning |
| --- | --- |
| `requirement` or `example` | The complete body, copied from the corresponding directive. Use the field matching its kind. |
| `determination` | `true`, `false`, or `{"FIXME": ["#125"]}` for explicitly documented gaps. |
| `rationale` | A Markdown explanation of that judgment. |
| `inputs` | Source snapshots supporting it. Inputs may select a file range, Rust item, or glob of definitions. |

For a requirement, the model reviews whether the cited code implements its stated obligations
and whether tests cover the interesting design cases. If there are no tests,
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
habeas add-input --for-rust-item src/evidence.rs 'fn[validate_repository]' --requirements 'requirements.ci.*'
```

The `--rust-item <file>:<line>` mode selects all explicit Rust items declared on that line. A named item is declared where its name appears; an unnamed item is declared where its keyword appears. Rust-item snapshots include attached comments, doc comments, and attributes. Moving unchanged lines earlier or later in the file does not stale the snapshot. A requirements glob includes requirements and named examples, records every matched name, and stores the full raw fenced declarations in sorted name order. Changes to a matched declaration or the match set invalidate the input. {cite}`requirements.cli.add_input_file_range` {cite}`requirements.cli.add_input_rust_item` {cite}`requirements.cli.add_input_requirements` {cite}`requirements.ci.rust_item_inputs` {cite}`requirements.ci.requirements_inputs`

Habeas checks that every requirement and example has evidence, that every evidence file names a definition, and that recorded text and input lines still match the repository. An example with a true determination also needs a `{test}` reference.

## Authoring evidence with a model

`habeas author --all` fills in missing or stale evidence and reassesses invalid or
false records. It includes requirements, examples, and Rust function attestations,
preserving current accepted evidence, including FIXME judgments. Each assessment
gets its own conversation and usage record. Successful results are kept if
another assessment fails; the command reports failures and exits unsuccessfully.
A completed false judgment is saved and remains a validation failure.
{cite}`requirements.authoring.batch` {cite}`requirements.authoring.function_attestations`

`habeas author <full-name>` assesses one requirement or example, records the
source inputs delivered to the model, and writes its judgment to the usual
evidence file. `author-evidence` remains an alias. Install `rg` (ripgrep) on your PATH for the model's search tool.
Choose a model that supports tool calls, either on Amazon Bedrock or an accessible
Ollama server. The server can run on another machine; Habeas does not need a GPU.
{cite}`requirements.authoring.invocation` {cite}`requirements.authoring.provider`
{cite}`requirements.authoring.text_search`

Create and edit `$XDG_CONFIG_HOME/habeas/config.toml`, or
`$HOME/.config/habeas/config.toml` when XDG_CONFIG_HOME is absent, empty, or
relative. On Windows, use `%APPDATA%/habeas/config.toml`. This file is user-local
and is not part of the project. Here is a complete configuration with two choices:
{cite}`requirements.configuration.user_location`
{cite}`requirements.authoring.configuration_documentation`

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
the file. Credentials belong to AWS's credential sources or the named environment
variable. {cite}`requirements.authoring.configuration_precedence`

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
{cite}`requirements.authoring.configuration_documentation`

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

Each reference must name an existing requirement or example. Its evidence must include an input covering the line with the reference. In the book, references and definitions become linked badges.

For Rust, Habeas parses first-party source files and requires every explicit function or method, including trait methods and test helpers, to have a requirement or named example reference in an attached comment or its body. Each named example cites a requirement. A reference on an enclosing `impl` or module, in a string literal, or only in an evidence input does not cover the function. A Rust source file that cannot be parsed is reported as an error. For a helper with no behavior-specific requirement, agree on an explicit project requirement such as `internal-detail` and provide evidence explaining why it adds no otherwise unspecified user-visible behavior.

Rust-item selectors use syntactic ancestry within one repository-relative file. Examples include `mod[local]/fn[run]`, `impl[Thing]/fn[run]`, `impl[Thing/Trait]/fn[run]`, `trait[Trait]/fn[read]`, `fn[outer]/fn[inner]`, and positional `use#1`. Duplicate sibling names gain `#1`, `#2`, and so on. Delimiter characters and whitespace within names or impl types use percent-encoded UTF-8 bytes, so spelling can be recovered without resolving modules across files. {cite}`requirements.references.rust_item_selectors`

Each Rust function also needs an attestation in `evidence/rust/<rust_file>.json`, with a `rust_file` field and a `functions` array. Every entry has `rust_item`, a `citations` array of `{ "role": "impl", "name": "requirements.some_name" }` objects, a boolean `determination`, `rationale`, and `inputs`. This determination assesses citation coverage; FIXME applies to requirement and example judgments. It needs one Rust-item input for that function and at least one requirements input covering every cited definition. The agent must review the whole function and relevant definitions, then explain why its citations include *every applicable* requirement or named example and how those texts account for its behavior together. A matching snapshot alone is insufficient. {cite}`requirements.references.rust_function_attestations` {cite}`requirements.cli.function_attestation_guidance`

## Checking and viewing

The test created by `habeas setup` calls `habeas::validate()`. Run `cargo test --workspace` to find missing evidence, false determinations, changed lines, uncovered references, and uncited Rust functions. Validation accepts true and FIXME judgments for requirements and examples when all other evidence checks pass; Rust function attestations still require true. FIXME remains visible in status and the book with its recorded references and rationale. These checks are deterministic: they verify the saved evidence without inference or reassessing the reviewer's judgment about documented gaps. The check reports all the errors it finds, with diffs for changed text. Run `mdbook build` to render the book. {cite}`requirements.ci.determination`

Run `habeas status [<glob>]` from within the project to list requirements and named examples needing attention and their validation findings, including FIXME judgments and their recorded references. `--all` shows every matching definition and its evidence state. Use `habeas info <glob>` to print matching detail pages as Markdown, including current texts, rationales, inputs, FIXME references, and findings. Both commands match whole full names; `*` can span any number of characters, including dots. {cite}`requirements.cli.status` {cite}`requirements.cli.info` {cite}`requirements.references.definition_globs`

```sh
habeas info requirements.ci.inputs
habeas info 'requirements.cli.*'
habeas status 'requirements.cli.*' --all
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
definition appears once. Existing requirements globs still select only requirements
and examples. Their referenced definitions are captured regardless of name.

Changing a definition's phrase or body invalidates its snapshots. Refreshing inputs
leaves the evidence pending reassessment. Validation reports missing or stale
snapshots without modifying them. A term link does not replace a function's required
requirement or example citations.

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
example. Each requirement or example evidence file records five fields:

| Field | Meaning |
| --- | --- |
| `requirement` or `example` | The complete body, copied from the corresponding directive. Use the field matching its kind. |
| `determination` | `true`, `false`, or `{"FIXME": ["example/queue#125"]}` for gaps acknowledged by `{fixme}` references. |
| `rationale` | A Markdown explanation of that judgment. |
| `references` | A SHA-256 digest of the definition's reference set: each incoming `{impl}` or `{test}` reference's role, file, and the outline node it belongs to, with the words of a fenced reference's commentary in a source outside the book, plus its `:impl:` and `:test:` anchors. `{cite}` references do not contribute. Adding, removing, or re-roling a reference or anchor, editing its commentary, or moving a reference to another node makes the evidence stale even when no input changed; moving one within its node does not. {cite}`requirements.ci.reference_set` |
| `inputs` | Sources supporting it. Inputs select an outline node of a file, a definition selection, or a terminology definition. |

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
fenced `{fixme}` references to the subject acknowledge every remaining
implementation or testing gap; otherwise it chooses `false` for gaps or missing
information. A plain `FIXME` or `TODO` comment acknowledges nothing. A `{fixme}`
names the definition, may begin with `:github: <owner>/<repo>#<number>` lines
naming the issues that track the gap, and describes the gap in its body:

```text
// :::{fixme} requirements.queue.full
// :github: example/queue#125
// Capacity-zero insertion does not return Full yet.
// :::
```

The FIXME array lists each issue the acknowledging `{fixme}` references name, as
written after `:github:`, and, for one naming no issue, its body's words joined
by single spaces, with each gap and source location explained in the rationale.
Validation reports a FIXME string that no current `{fixme}` to the subject
could have produced. The harness captures those source inputs, and editing a
`{fixme}` makes the evidence stale. A named `{fixme}` belongs in sources outside
the book; an unnamed `:::{fixme}` with only `:github:` lines may tie any place,
including a requirement's text, to the issues it names.
{cite}`requirements.ci.outcomes`

An input has exactly one of these forms. A source file input names one node of
the file's outline by its selector and records the node's structural hash; it
stores no source text and no line numbers. Requirements and definition inputs
store the complete raw declaration lines:

```json
{"file":"src/main.rs","item":"fn[main]","hash":"<64 lowercase hex digits>"}
{"requirements":"requirements.cli.*","matches":["requirements.cli.info"],"text":["..."]}
{"definition":"glossary.command","text":["..."]}
```

The empty `item` selects the whole file. Moving a node, editing other nodes, or
editing comments its language profile does not hash, such as ordinary Rust `//`
comments, leaves the input valid; changing its code, or a Rust doc comment,
changes the hash and makes the input stale. The validation error names the file,
the selector, and the node's current line range. Detail pages and `habeas info`
show each node's kind, current line range, and current source text.
{cite}`requirements.ci.outline_inputs` {cite}`requirements.references.structural_hash`

`habeas author --all` captures source text and delegates judgments to the configured
model. The supervising agent fixes reported gaps and reruns the command; it does
not write evidence by hand. A changed input leaves its determination false until
the authoring harness reassesses it.
{cite}`requirements.authoring.batch` {cite}`requirements.ci.schema`

A requirements input takes a definition selection, either a full name or a glob where `*` matches any characters including dots. It includes requirements and named examples, records every matched name, and stores the full raw fenced declarations in sorted name order. Changes to a matched declaration or the match set invalidate the input. {cite}`requirements.ci.requirements_inputs`

Habeas checks that every requirement and example has evidence, that every evidence file names a definition, that recorded requirement and definition text still matches, and that every outline input's hash still matches its node. An example with a true determination also needs a `{test}` reference. Each determination is checked on its own: a false judgment does not fail the definitions its subject implements, tests, or cites. {cite}`requirements.ci.determination`

## Authoring evidence with a model

`habeas author --all` fills in missing, stale, or invalid evidence for
requirements and named examples, their citation reviews, and site reviews, and
deletes orphaned citation and site reviews. Every `habeas author` form works this
way by default (`--missing`): it preserves current records without contacting a
model, including FIXME judgments and current false judgments, and reports how
many selected subjects it skipped as current. When nothing needs assessment, it
succeeds without reading model configuration or credentials. A false judgment is
reassessed when its subject or inputs change. Add `--force` to reassess every
selected subject, current or not; `--stdout` implies `--force`, and
`--missing` and `--force` cannot be combined.
{cite}`requirements.authoring.refresh`

```sh
habeas author --all --force
habeas author requirements.check --force
```

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
habeas author requirements.ci.outline_inputs --verbose
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
with `habeas`, list files, list a file's selectable nodes with `outline`, search
files with ripgrep (built in; no `rg` executable is needed), and use `references`
to list definition names or every `{impl}`, `{cite}`, and `{test}` site for a
definition selection. A `habeas` read names an outline node directly or asks for
a line range; a line range is returned as the innermost outline nodes covering
those lines, and those nodes are recorded as outline inputs. Searches only locate
code: each ripgrep or ast-grep match is returned as the innermost outline node
containing it with the matching lines and their context, and the model reads with
`habeas` any node it relies on. Searches, listings, outlines, and reference
lookups are not recorded as evidence.
Choose a model that supports tool calls, either on Amazon Bedrock or an accessible
Ollama server. The server can run on another machine; Habeas does not need a GPU.
{cite}`requirements.authoring.invocation` {cite}`requirements.authoring.provider`
{cite}`requirements.authoring.text_search`
{cite}`requirements.authoring.outline_tool`
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

Source text sent to the model, and source text shown on book detail pages and by
`habeas info`, is redacted. In a workspace `habeas.toml` or your user
configuration file, the values of Bedrock `credentials` fields and Ollama
`api_key` are replaced by name, whatever they contain. In every file, text
matching one of the built-in credential rules of the
[`secretshape`](https://docs.rs/secretshape) crate, such as a provider-prefixed
token or a PEM private key header, is replaced too, extended through adjacent
whitespace-separated text that looks random, such as the body of a private key.
Random-looking text alone, such as a hex digest, is never redacted. Each
replacement reads `[REDACTED:<kind>]`, naming the field or rule, and keeps the
replaced text's line breaks, so line numbers do not change. Outlines are built from
the original file, and structural hashes use the redacted text, so evidence does
not depend on a credential's value. Habeas pins its `secretshape` version;
upgrading it is a deliberate change that may alter redaction and hashes.
{cite}`requirements.authoring.redaction`

Habeas reads two configuration files, and you create and edit both by hand with a
text editor; there is no configuration-management command, and Habeas never
creates, repairs, or rewrites them. The user file, described below, holds
settings that apply across projects. A workspace file named `habeas.toml` holds
settings for the directory containing it and its descendants. Both are UTF-8 TOML.
Each must start with the schema version `version = 1`; any other version is
rejected. Apart from `version`, the top level may contain only feature tables:
`[authoring]` in either file, and `[evidence]` in `habeas.toml`. Unknown keys,
wrongly typed values, and invalid TOML are reported with the file and setting.
A file containing only `version = 1` is valid and configures nothing. The
[common data structures](requirements/configuration.md#common-data-structures)
describe this schema in full. To remove an optional setting or selection, delete
its key or table; omitted settings use their documented defaults.
{impl}`requirements.configuration.documentation`

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
Replace `generated` and `generated/private` with your own paths. Add or delete
entries in either array to change the policy. An omitted `allow` or `disallow`
means an empty list; delete the whole `[evidence]` table to restore the default
source-access rules, or the `[authoring]` table to use only user settings.
{impl}`requirements.configuration.documentation`
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
{impl}`requirements.configuration.documentation`
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
the file. This complete user `config.toml` uses a remote, authenticated Ollama server;
replace `MODEL_TAG`, `workstation`, and `OLLAMA_API_KEY` as above:

```toml
version = 1

[authoring]
default = "local"

[authoring.configs.local]
backend = "ollama"
model = "MODEL_TAG"
endpoint = "http://workstation:11434"
api_key_env = "OLLAMA_API_KEY"
```

Delete `endpoint` to return to the local default, or `api_key_env` to send no key.
Profiles and environment references are preferred. Literal Bedrock
`credentials` or Ollama `api_key` values are supported but not recommended in
workspace files; see the [credential examples](requirements/authoring.md#editing-the-file).
The model may read a configuration file that contains them, with the values
redacted. Validation reports a workspace `habeas.toml` in the Git index that
contains literal credentials, naming the file and its credential settings,
because committed credentials leak; keep such a file untracked or use credential
sources instead.
{cite}`requirements.authoring.configuration_precedence`
{impl}`requirements.authoring.credentials`

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

Change `authoring.default` in the user file to switch the default, or add a
per-project selection using its canonical absolute path. In this complete user
`config.toml`, replace `/absolute/path/to/project` with that path and `MODEL_TAG` as above:

```toml
version = 1

[authoring.configs.local]
backend = "ollama"
model = "MODEL_TAG"

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
{impl}`requirements.configuration.documentation`

Bedrock runs assessments concurrently by default, using the available logical CPU
count. Ollama runs one at a time. Override either default with a positive
`concurrency` value alongside `backend` and `model` in its configuration table.
In this complete user `config.toml`, replace `MODEL_ID` as above; delete `concurrency` to
restore the default:

```toml
version = 1

[authoring.configs.cloud]
backend = "bedrock"
model = "MODEL_ID"
concurrency = 4
```

Four is an example, not a fixed default. Authoring uses asynchronous tasks on a
fixed worker thread pool and limits simultaneous assessments to this value.
Lower it if Bedrock reports throttling, then rerun `habeas author --all` to work
on records that still need attention. {cite}`requirements.authoring.concurrency`
{impl}`requirements.configuration.documentation`

For Bedrock, put the Converse names `temperature`, `maxTokens`, `topP`, and
`stopSequences` directly in that options table. Habeas maps them to Converse's
`inferenceConfig`; other keys become model-specific `additionalModelRequestFields`.
Use these flat keys without an `inferenceConfig` wrapper. For example, in this
complete user `config.toml` (replace `MODEL_ID` as above):

```toml
version = 1

[authoring.configs.cloud]
backend = "bedrock"
model = "MODEL_ID"

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
{impl}`requirements.configuration.documentation`

The command reports the evidence destination and usage-record path. Usage
metadata is kept under `$XDG_STATE_HOME/habeas/authoring/usage`, falling back to
`$HOME/.local/state/habeas/authoring/usage`; Windows uses
`%LOCALAPPDATA%/habeas/state/authoring/usage`. Ordinary CI validation, status, and
book rendering remain deterministic and do not require model configuration or
credentials. {cite}`requirements.authoring.usage_storage`
{cite}`requirements.authoring.deterministic_validation`

`habeas author` and `habeas status` read GitHub issues, authenticating with
`GITHUB_TOKEN` when it is set, through a cache in the `github` directory of the
same state root: `$XDG_STATE_HOME/habeas/github`, falling back to
`$HOME/.local/state/habeas/github`, or `%LOCALAPPDATA%/habeas/state/github` on
Windows. Each issue `<owner>/<repo>#<number>` is one JSON file,
`<owner>+<repo>+<number>.json`, recording the `issue`, the GitHub `api` it was
read from, the Unix time it was `fetched` in seconds, and its `state` (`open`,
`closed`, or `missing`) and, for an issue that exists, its redacted `title` and
`body`. An entry less than an hour old is reused; a failed read is not cached.
Deleting the directory only forces fresh reads. When the state root cannot be
determined or lies inside the project repository, issues are read without a
cache, so nothing is written there.
{cite}`requirements.authoring.github_reads`
{impl}`requirements.configuration.state_location`

### Assessment policy

Every `habeas author` form assesses its subjects under an assessment policy,
`confirm` or `once`. `--policy <name>` selects it for one run; otherwise the
selected configuration's `policy` applies, and otherwise `confirm`. This
complete user `config.toml` sets `once` for a `local` configuration; replace
`MODEL_TAG` with the exact Ollama model tag, and the same table can appear in a
workspace `habeas.toml`:

```toml
version = 1

[authoring.configs.local]
backend = "ollama"
model = "MODEL_TAG"
policy = "once"
```

Under `confirm`, each `true` or FIXME result is checked with an independent
second assessment. When both assessments reach the same kind of outcome, the
first is recorded; site reviews that differ only in `applies` disagree. When
they disagree, a third conversation verifies the less favorable one, where
`false` is less favorable than FIXME and FIXME than `true`: the verifier
receives the subject, its mandatory inputs, that assessment's rationale, and the
source tools, and answers with `submit_verification`, saying whether its
claimed gaps are real and whether `{fixme}` references acknowledge them. The
recorded rationale then ends with a confirmation note stating that the
assessments disagreed and what the verification concluded, and `habeas status`
lists the record under "Disagreements". Each conversation has its own usage
record, whose subject names the second assessment or the verification.

Under `once`, each subject gets a single assessment, which uses fewer tokens and
less time, such as with a slow local model. The completion report names the
policy used, and each usage record identifies it in `policy`. An unknown policy
name is rejected. {cite}`requirements.authoring.policy`

```sh
habeas author --all --policy once
habeas author requirements.check --policy confirm
```

A disagreement is a finding, not noise to retry away: the requirement, code, or
tests leave a gap one reviewer found or a reading the reviewers took
differently. Close the gap, or tighten the requirement text, rather than
rerunning for a different determination.
{cite}`requirements.authoring.confirmation` {cite}`requirements.cli.flip_guidance`

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

Each reference must name an existing requirement or example, or a built-in definition, and is a directed edge between its source and that definition. Choose the role by what the source does:

- `{impl}`: the source implements part of the definition. It makes a decision, performs a check, produces an output, or handles a case the definition specifies. Calling code that implements a definition, or producing a result a caller uses to meet it, is not implementing it. A requirement that refines another requirement's obligation can `{impl}` it. A convention for every output of a kind, such as every validation error, is implemented by the code that carries it out as a whole.
- `{test}`: the source is designed to verify behavior the definition specifies, either a test through its scenario and assertions, or a named example through its concrete case.
- `{cite}`: the source relies on, supports, or refers to the definition without implementing or testing it.

Only requirements and examples are assessed; code is reviewed as part of the definitions it implements or tests. `{impl}` and `{test}` point from the definition to the source, so the source becomes a mandatory input to the definition's assessment, and that definition's evidence must cover the reference line. `{cite}` points from the source to the definition: a citing requirement or example receives the cited declaration as context, and the citing site needs no evidence coverage. Marking only `{cite}` on code that implements part of a requirement leaves a gap in that requirement's review. In the book, references and definitions become linked badges.
{cite}`requirements.references.roles` {cite}`requirements.references.evidence_coverage` {cite}`requirements.references.authoring_inputs`

Habeas discovers references in the same sources authoring may read. In a Git worktree, these are the files in the Git index, including staged new files; outside Git, every project file. Either way, the project-root `book` and `evidence` directories, `.git` and `target` directories at any depth, and paths your `[evidence]` configuration disallows are skipped. References in an untracked file are not validated and do not count toward any definition until you stage the file with `git add`; `habeas status` lists untracked files that contain references as a reminder.
{cite}`requirements.references.discovery` {cite}`requirements.cli.status`

### Choosing references

For each definition a source item could reference, choose by this procedure. An item may carry several references.

1. If the item is a test or test helper designed to verify the definition, use `{test}`.
2. Otherwise, if fulfilling the definition depends on the item, use `{impl}`. For code, this means it makes a decision, performs a check, produces an output, or handles a case the definition specifies. For a type, it means the definition constrains its shape: its fields, variants, serialized form, or validation.
3. Otherwise, if the item should be re-checked or kept in sync when the definition changes, use `{cite}`.
4. If the item has no reference after applying steps 1 to 3 to every definition, cite the built-in `habeas:none`, justifying why the item exists.

For internal code, state any invariant worth checking, such as a cache that must not change results, as an ordinary requirement and implement it. Otherwise cite `habeas:none`. Do not write grouping requirements that state no checkable obligation just to give internal helpers something to reference.
{cite}`requirements.references.procedure` {cite}`requirements.cli.internal_detail_guidance`

### Fenced references and commentary

Any reference can instead be written in fenced form to attach commentary: an opening line of three or more colons followed by the role and the name, the commentary, and a closing line of at least as many colons. The edge is the same as the inline form's. In Markdown, the commentary is Markdown, and the book renders the badge followed by it:

```text
:::{impl} requirements.check
Reports every invalid argument, not only the first.
:::
```

In a source comment, Habeas recognizes the fence lines after removing each line's comment delimiters, such as `//`, `///`, `#`, or a block comment's leading `*`. Both fences must lie in one comment run: consecutive comment lines with no other source between them. A fence that does not close within its comment run, or before the end of a Markdown or plain-text file, is a validation error. The reference belongs to the node its comment run belongs to.

```rust
// :::{cite} requirements.check
// The parser here must keep accepting what the check reports as valid.
// :::
fn parse_arguments() {}
```

A fenced reference inside a `{req}` or `{example}` closes at the first colon-only line with as many colons, which would also close a directive opened with the same number. Give the outer directive a longer fence:

```text
::::{req} check
The CLI MUST report invalid arguments.

:::{cite} requirements.cli.invocation
Usage errors use the same report.
:::
::::
```

For an `{impl}` or `{test}` reference in a source outside the book, the words of its commentary are part of the definition's reference set, so editing them makes that definition's evidence stale; reflowing them does not. In the book, commentary is part of the declaration text instead.
{cite}`requirements.references.roles` {cite}`requirements.ci.reference_set` {cite}`requirements.references.book_badges`

### Built-in definitions

Built-in definitions are named `habeas:<name>` and are available in every project without a declaration. Each has fixed text and permits only certain roles. Built-ins have no evidence files and do not appear in the requirements index or `habeas status`. Project definitions cannot use names beginning with `habeas:`. In the book, a reference to a built-in renders as a badge without a link; hovering over it shows the built-in's text.

There is one built-in, `habeas:none`. Its text is:

> Cite this only when no requirement or example applies to the item under the
> reference procedure. The citation MUST use the fenced form, and its body MUST
> justify why the item exists. An item with an invariant worth checking, such as
> a cache that must not change results, needs a requirement stating that
> invariant instead. An item citing this MUST NOT implement end-user-observable
> behavior that no requirement specifies.

Only `{cite}` may reference it, and only in fenced form whose body, without comment delimiters and other references, justifies the item:

```rust
// :::{cite} habeas:none
// Splits lines for the outline builders; no requirement concerns how.
// :::
fn split_lines(text: &str) -> Vec<&str> {
    text.lines().collect()
}
```

A justified `habeas:none` citation counts as a reference for Rust item coverage. `habeas status --all` lists every `habeas:none` citation with its item and justification.
{cite}`requirements.references.builtin_definitions` {cite}`requirements.references.no_requirement`

### Citation reviews

Every source item outside the book that carries a `{cite}` reference, inline or fenced, needs a citation review. The item is the outline node its reference belongs to, or the whole file for a plain-text source. The review assesses whether each citation is sensible, meaning the item should be re-checked or kept in sync when the cited definition changes, and whether the procedure above calls for `{impl}` or `{test}` instead. For `habeas:none`, it assesses whether the justification is true, whether any definition applies, and whether the item implements end-user-observable behavior that no requirement specifies. A citation in a requirement or example declaration is reviewed as part of that definition's own assessment instead.

The review of an item at `src/lib.rs` with selector `impl[Thing]/fn[run]` is recorded at `evidence/citations/src/lib.rs/impl[Thing]%2Ffn[run].json`: the file's project-relative path, then the selector with every `/` and `%` percent-encoded, or `_root` for the root node. It has these fields:

| Field | Meaning |
| --- | --- |
| `file`, `item` | The item's project-relative path and outline selector. |
| `citations` | Its `{cite}` references in source order, each with `role`, `name`, and `commentary`, which is empty for an inline reference. |
| `determination`, `rationale` | The judgment and its Markdown explanation, with the same outcomes as other evidence. |
| `inputs` | The sources supporting it, including the item's outline node and each cited declaration. |

Validation reports a missing, false, or orphaned review, and a stale one: its item's hash changed, a cited declaration changed, or the citations or the words of their commentary changed. `habeas author --all` authors reviews that need attention, and `habeas author citations:<file>#<selector>` names one, such as `habeas author citations:src/lib.rs#fn[run]`; omit the selector for the root node.
{cite}`requirements.references.citation_review` {cite}`requirements.authoring.citation_prompt`

### Anchors

Some sources cannot or should not carry a reference themselves, such as a shipped agent skill. A requirement can anchor such a file with `:impl:` or `:test:` option lines immediately after its opening fence; an example can use `:test:`. Each anchor is an edge to the complete file, or with `path#selector` to one outline node in it, such as `src/lib.rs#fn[run]`. The requirement's evidence must contain an outline input selecting the anchored node or one of its ancestors (the empty selector selects the whole file), so changing it makes the evidence stale. Option lines are not part of the requirement text. Prefer a precise in-source reference whenever one is possible.
{cite}`requirements.references.anchors`

```text
:::{req} skill
:impl: skills/habeas/SKILL.md
The bundled skill MUST explain how to author evidence.
:::
```

### Rust items

For Rust, Habeas parses first-party source files, which are the `.rs` files among the sources reference discovery considers, except under the project-root `vendor` directory. It requires every explicit function or method, including trait methods and test helpers, to have a requirement or named example reference in an attached comment or its body. Each named example references a requirement. A reference on an enclosing `impl` or module, in a string literal, or only in an evidence input does not cover the function. A Rust source file that cannot be parsed is reported as an error. For a helper no requirement or example applies to, cite `habeas:none` in fenced form with a justification, as described under [Built-in definitions](#built-in-definitions). {cite}`requirements.references.rust_item_coverage` {cite}`requirements.cli.internal_detail_guidance`

Rust selectors use syntactic ancestry within one repository-relative file, under the Rust outline profile below. Examples include `mod[local]/fn[run]`, `impl[Thing]/fn[run]`, `impl[Thing/Trait]/fn[run]`, `trait[Trait]/fn[read]`, `extern[C]/fn[puts]`, and `fn[outer]/fn[inner]`. An impl is named by its self type and optional trait as written, so `impl<T> Trait for Wrapper<T>` is `impl[Wrapper<T>/Trait]`. `use` declarations and item-position macro invocations are anonymous, such as `use@3f2a9c01b7de`. Modules are never resolved across files. {cite}`requirements.references.rust_item_selectors`

## Outlines

Habeas reads sources as outlines: trees of nodes chosen by a per-language profile from a pinned tree-sitter grammar. The file extension picks the profile; every other text file uses the plain-text profile. Each outline has a root node for the whole file. Grammar nodes a profile does not map are transparent, so their outline descendants appear under the nearest outline ancestor. Source in no mapped node, such as a top-level statement no profile maps or Markdown front matter, belongs to the root; profiles have no fallback kinds. Comments are never outline nodes. {cite}`requirements.references.outlines`

### Selectors

A selector is a `/`-separated path of segments from the root; the empty selector is the root. A named node's segment is `kind[name]`; an anonymous node's is `kind@hash`, the first twelve hex digits of its identity hash. Siblings whose segments would otherwise be identical gain `#1`, `#2`, and so on in source order, except `line` nodes: identical lines are interchangeable and a `line` selector resolves to any line with that content. In names, `%`, `[`, `]`, `/`, `#`, `@`, and whitespace are percent-encoded as UTF-8 bytes. A name made of parts, such as an impl's type and trait, joins the encoded parts with a literal `/` inside the brackets. Line numbers are never part of a selector. {cite}`requirements.references.outline_selectors`

### Hashes

A node's structural hash is the SHA-256 digest of a length-prefixed encoding of the kinds of its named grammar nodes, node boundaries, and, in source order, its text pieces, including siblings the profile attaches to it. A text piece is a leaf token's text or text inside a node that no child token covers, such as TOML string contents, YAML block scalars, or Markdown inline text. In every language, each text piece is hashed as its ordered list of whitespace-separated words, each encoded separately: whitespace is never hashed, so `"a  b"` and `"a b"` hash alike but `"ab"` differs, and reflowing prose does not change the hash. Comments the profile does not hash are excluded; for a hashed comment only its text and its comment group are hashed, without delimiters such as `//`, `///`, `//!`, `/*`, `*/`, or a leading `*`, so reflowing a doc comment across lines keeps the hash. Rust's groups are inner and outer doc comments, so moving words from an item's `///` comment into an adjacent `//!` comment changes the enclosing node's hash. A `line` node's hash uses the same word rule over the line's text. The identity hash is computed the same way but also skips the node's outline descendants, with their attached siblings, so editing a nested function does not change its parent's selector. {cite}`requirements.references.structural_hash`

### Comments

Each profile defines which grammar nodes are comments; in Markdown these are HTML comment blocks, so an inline HTML comment within a paragraph is text of its section. Comments on consecutive lines with no other source between them form one run. A comment that follows other source on the same line, including another comment, is a run by itself and belongs to the innermost node containing that line. Any other run belongs to the next outline node that starts after it within the smallest node containing it; when that container has no such node, the run belongs to the container, which is the root at the top level. A profile can bind a comment kind to its container instead, so its runs belong to the smallest node containing them: the Rust profile binds inner doc comments (`//!`, `/*!`), so a file-level inner doc comment belongs to the root. A node's line range runs from its earliest attached sibling or owned comment to its last line or owned comment. {cite}`requirements.references.outline_comments`

### Outline profiles

| Profile | Extensions | Kind | Grammar nodes | Name |
| --- | --- | --- | --- | --- |
| Rust | `.rs` | `fn` | `function_item`, `function_signature_item` | `name` |
| | | `struct`, `enum`, `union`, `trait`, `mod` | `struct_item`, `enum_item`, `union_item`, `trait_item`, `mod_item` | `name` |
| | | `impl` | `impl_item` | `type` field, then `/` and the `trait` field when present |
| | | `macro` | `macro_definition` | `name` |
| | | `macro` | `macro_invocation` in item position | anonymous |
| | | `const`, `static` | `const_item`, `static_item` | `name` |
| | | `type` | `type_item`, `associated_type` | `name` |
| | | `extern` | `foreign_mod_item` | ABI string, `Rust` when absent |
| | | `extern-crate` | `extern_crate_declaration` | `name` |
| | | `use` | `use_declaration` | anonymous |
| Python | `.py`, `.pyi` | `def` | `function_definition` | `name` |
| | | `class` | `class_definition` | `name` |
| TypeScript | `.ts`, `.mts`, `.cts`, `.tsx` | `function` | `function_declaration`, `generator_function_declaration`, `function_signature` | `name` |
| | | `class` | `class_declaration`, `abstract_class_declaration` | `name` |
| | | `method` | `method_definition`, `method_signature`, `abstract_method_signature` | `name` |
| | | `interface`, `type`, `enum` | `interface_declaration`, `type_alias_declaration`, `enum_declaration` | `name` |
| | | `namespace` | `internal_module`, `module` | `name` |
| CSS | `.css` | `rule` | `rule_set` | selector list, whitespace collapsed |
| | | `media`, `supports` | `media_statement`, `supports_statement` | query text, whitespace collapsed |
| | | `keyframes` | `keyframes_statement` | keyframes name |
| Markdown | `.md`, `.markdown` | `section` | `section` | heading text, whitespace collapsed; anonymous before the first heading |
| TOML | `.toml` | `table` | `table`, `table_array_element` | key as written |
| | | `pair` | `pair` | key as written |
| YAML | `.yml`, `.yaml` | `pair` | `block_mapping_pair`, `flow_pair` | `key` as written |
| | | `item` | `block_sequence_item` | anonymous |
| Plain text | any other | `line` | each non-blank line | anonymous |

Profile details:

- Rust: attributes and outer doc comments before an item attach to it. Doc comments, outer and inner, are hashed; other comments are not.
- Python: decorators attach to the decorated `def` or `class`. Docstrings are ordinary string statements and are hashed.
- TypeScript: `.tsx` files use the TSX grammar with the same table. An `export` wrapper is transparent.
- CSS: `/* */` comments are not hashed.
- Markdown: sections nest by heading level. An HTML comment block (`<!-- ... -->`) is a comment and is not hashed.
- TOML and YAML: `#` comments are not hashed. Quoted keys keep their quotes in names. In YAML, a comment before a key belongs to that key, even though the grammar places it at the end of the previous key's node; in TOML, a comment after a table's last pair stays in that table.

Grammar versions are pinned exactly in `Cargo.toml`. Upgrading a grammar or changing a profile can change selectors and hashes, so it is a deliberate change. {cite}`requirements.references.outlines`

## Checking and viewing

The test created by `habeas setup` calls `habeas::validate()`. Run `cargo test --workspace` to find missing evidence, false determinations, changed lines, changed reference sets, uncovered `{impl}`, `{test}`, and `{fixme}` references, misplaced or malformed `{fixme}` references, missing or stale citation reviews, and unreferenced Rust functions. Validation accepts true and FIXME judgments when all other evidence checks pass. FIXME remains visible in status and the book with its recorded references and rationale. These checks are deterministic: they verify the saved evidence without inference or reassessing the reviewer's judgment about documented gaps. The check reports all the errors it finds, with diffs for changed text. Run `mdbook build` to render the book. {cite}`requirements.ci.determination`

Run `habeas status [<selection>]` from within the project to list requirements and named
examples needing attention and their validation findings. FIXME judgments are
summarized by issue, for example:

```text
- rust-lang/rust#123 — affects 22 judgments
- example/queue#45 — affects 3 judgments
- 16 judgments marked FIXME without an issue
```

Each judgment counts once per issue, even if it repeats that reference. A judgment
with several issues counts in each group; the final group counts judgments with
no issue reference. Only a FIXME string that is exactly `owner/repo#N` names an
issue, compared exactly; any other string, such as a gap description, a bare
`#N`, or an issue URL, counts as a gap without an issue. Groups with the most
affected judgments come first.
{cite}`requirements.cli.fixme_summary`

Every form also lists, in a "Disagreements" section, each selected
definition, citation review, and site review whose recorded rationale states
that its assessments disagreed under the `confirm` policy. Validation accepts
these records, so the section is where they stay visible for review. With a
selection, a citation review is listed when one of its citations names a
selected definition, and a site review when its requirement is selected.
{cite}`requirements.cli.status`

Use `--verbose` to list the actual judgments in each group, with their full names,
kinds, and recorded FIXME strings. `--all` shows every matching definition and its
evidence state, and lists every `habeas:none` citation with its item and
justification. Stale or invalid FIXME evidence still reports its validation
findings. Use `habeas info <selection>` to print matching detail pages as Markdown,
including current texts, rationales, inputs, FIXME references, and findings. Both
commands take a definition selection: a full name selects exactly that definition,
and in a glob `*` can span any number of characters, including dots. {cite}`requirements.cli.status` {cite}`requirements.cli.info`
{cite}`requirements.references.definition_globs`

```sh
habeas info requirements.ci.outline_inputs
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

The build fills that chapter with a table of requirement and example states linking to their evidence pages. With or without an index, the build adds an evidence page for every requirement and example, nested under the chapter that defines it; each definition badge and its "See evidence" link open that page. `habeas setup` enables `output.html.fold` so these pages stay folded in the sidebar until a reader expands their chapter. {cite}`requirements.index.detail_pages`

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

The authoring harness automatically captures definitions referenced by the
subject's requirement or example text and its reviewed inputs. It follows references between
definitions until no new definitions are found; cycles are allowed, and each
definition appears once. A requirements input's selection still selects only
requirements and examples. Their referenced definitions are captured regardless of name.

Changing a definition's phrase or body invalidates its snapshots. Validation reports missing or stale
snapshots without modifying them. A term link does not replace a function's required
requirement or example reference.

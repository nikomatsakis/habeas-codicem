# Authoring evidence

Habeas gives a model a requirement, example, or Rust function and its mandatory source inputs.
The model can discover files, request more inputs, and submit one judgment. The
harness records the inputs and writes the judgment into the existing evidence
format. Rig provides the connection to the user's configured model.

:::{req} invocation
`habeas author <full-name> [--config <name>] [--stdout] [-v|--verbose]` MUST assess one
existing requirement or named example in the current project. It MUST report the
selected subject, backend, model, evidence destination, and usage-record path on
completion. The evidence destination MUST identify the evidence-file path by
default or stdout when `--stdout` is selected. In stdout mode, the completion
report and diagnostics MUST go to stderr. `author-evidence` MUST remain an alias
for the single-subject command.
:::

:::{req} batch
`habeas author --all [--config <name>] [-v|--verbose]` MUST author every current requirement,
named example, and first-party Rust function whose evidence or attestation is
missing, stale, invalid, or false. It MUST preserve accepted, current records,
including accepted FIXME judgments, without invoking a model for those records.
`--all` MUST be mutually exclusive with a subject name and `--stdout`.
Failure to discover declarations or parse project sources MUST report an error,
not a successful empty batch. A batch with no assessments needed MUST succeed
without resolving model configuration or credentials.

Each subject MUST receive an independent assessment with its own prompt, inputs,
judgment, and usage record. The command MUST report aggregate completion and each
authoring error under {cite}`requirements.authoring.reporting`, and retain
successful writes when another assessment fails. It MUST return a nonzero
exit status if any assessment fails. False judgments MUST still be written as
completed assessments; they remain failures under ordinary evidence validation.
An access-policy denial MUST abort only the affected assessment under
{cite}`requirements.authoring.read_denial`.
Concurrent function assessments in one source file MUST be merged into that
file's attestation record without losing other function entries. Assessments
reading that record MUST see a complete version while results are saved.

The bundled Habeas skills MUST direct the supervising agent to run this command
to create or refresh evidence. They MUST leave judgments, rationales, and input
snapshots to the authoring harness; the supervising agent addresses findings by
changing requirements, source citations, implementation, or tests.
{cite}`requirements.authoring.function_attestations`
{cite}`requirements.authoring.concurrency`
{cite}`requirements.authoring.usage_storage`
{cite}`requirements.authoring.persistence`
:::

:::{req} progress
When stderr is an interactive terminal, `habeas author --all` MUST display an
updating progress bar with elapsed time, percentage, finished and total assessment
counts, active and queued counts, and separate counts for `true`, `false`, `FIXME`,
and authoring errors. The total MUST count only subjects selected for assessment;
current accepted records remain in the unchanged count. A finished assessment
MUST advance progress whether it writes a judgment or reports an authoring error.
Progress measures completed work, not satisfaction of requirements.

Inspection MUST also show a progress bar for each phase with a known amount of
work, including evidence files, reference source files and coverage, Rust
attestations, and assessment selection. The display MUST label the current phase
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
single-subject and batch authoring, including Rust function attestations. It MUST
NOT print true rationales or append recorded
input snapshots. Reporting MUST use the completed assessment's existing result,
without additional inference. In `--stdout` mode, stdout MUST remain only the
evidence JSON; verbose rationale and other diagnostics MUST go to stderr.
Verbosity MUST NOT change persistence, validation, or exit-status rules.
{cite}`requirements.authoring.invocation` {cite}`requirements.authoring.batch`
:::

:::{req} provider_backoff
When a provider reports transient throttling through HTTP 429 or a Bedrock
`ThrottlingException`, the harness MUST wait and retry the same completion
request, retaining its prompt, history, tools, and pending inputs. It MUST NOT
restart the assessment or repeat already completed tool calls. A permanent
input/context-length error, authentication failure, or invalid request MUST
remain an error; mentioning tokens in an error message alone MUST NOT trigger
retries.

The backoff window MUST start at one second and double after each consecutive
throttled attempt, up to sixty seconds. Each delay MUST be randomly selected
between half and all of that window to spread parallel retries. An available
provider `Retry-After` value, in seconds or HTTP-date form, MUST be honored as a
minimum delay even when longer than the window. A successful response MUST reset
backoff for the next completion request. There MUST be no retry-attempt limit;
interruption MUST cancel both the wait and further retries.

Each attempt MUST be recorded under {cite}`requirements.authoring.usage_requests`.
Hidden SDK retries MUST remain disabled so the harness can account for attempts.
Transient retries MUST NOT be reported as finished judgments or terminal
assessment errors while recovery is still in progress. Pending inputs MUST be
recorded as evidence only after the request succeeds.
{cite}`requirements.authoring.provider` {cite}`requirements.authoring.delivered_inputs`
:::

## Model tools

The model receives four tools: `habeas`, `list_files`, `ripgrep`, and `submit_judgment`.
The following notation defines their arguments and results; names in uppercase
are data types, and optional fields show their defaults.

### Request evidence inputs: `habeas`

:::{req} source_tools
The harness MUST expose a `habeas` tool accepting exactly one of these request
forms. Each form MUST correspond to one existing evidence-input kind:

| Request fields | Evidence input returned | Definition |
| --- | --- | --- |
| `file: String`, `line_start: Integer`, optional `line_end: Integer` | File-range input | {cite}`requirements.ci.inputs` |
| `rust_file: String`, `rust_item: String` | Rust-item input | {cite}`requirements.ci.rust_item_inputs` |
| `requirements: String` | Requirements input, including its matching names | {cite}`requirements.ci.requirements_inputs` |
| `definition: String` | Definition input | {cite}`requirements.definitions.inputs` |

Rust file and selector arguments MUST use the syntax defined by
{cite}`requirements.references.rust_item_selectors`. Reads MUST use the current
project working tree, including unstaged changes to tracked files and staged new
files. By default, source inputs MUST stay within the project root after
resolving symlinks; in a Git worktree, both the requested file and its resolved
target MUST be present in the Git index. Projects outside Git MUST use the same
containment rules without requiring an index. Failure to read an existing
repository's index MUST be reported as an error.
The project-root `evidence/` directory and an active workspace configuration
containing literal credentials MUST be excluded from model reads by default,
including symlink aliases. Generated build outputs MUST follow
{cite}`requirements.authoring.generated_inputs`. Workspace path overrides MUST
follow {cite}`requirements.configuration.evidence_paths`, including explicit
permission for otherwise excluded inputs. A denied mandatory input or explicit
read MUST abort under {cite}`requirements.authoring.read_denial`.

File line numbers MUST be one-based and inclusive. Omitting `line_end` MUST read
through EOF; the returned evidence input MUST contain the actual numeric ending
line. The model-facing tool description MUST include the argument conventions
from the cited requirements, including Rust selector syntax, requirement globs,
and full definition names, with a short request example for each form.

A successful call MUST return the requested evidence input, populated by the
harness with the selected source text, using the existing evidence schema.
{cite}`requirements.ci.schema`
Required terminology dependencies MUST be returned alongside it as additional
definition inputs under {cite}`requirements.definitions.authoring_inputs`.
The result MUST have this shape:

```text
HabeasResult {
    input: EvidenceInput,
    definitions: Array<DefinitionInput>,
}
```

`definitions` MUST be empty when no additional terminology is required. The
harness MUST record inputs supplied to the model in the assessment's evidence.
A failed read MUST add no evidence input. Access-policy denials MUST abort;
other failed reads MUST return an explanatory tool error.
:::

:::{req} read_denial
When a mandatory input or dispatched model read is denied by source-access
policy, the assessment MUST abort and report the denied path and reason to its caller.
It MUST NOT return the denial to the model as a recoverable tool result or make
another model request for that assessment. An untracked-file diagnostic MUST
explain that the caller can stage the file with `git add` or configure a workspace
allow rule; the harness MUST NOT stage or permit the file itself.

The aborted assessment MUST NOT write evidence or emit evidence JSON. Its usage
record MUST identify the failure and any failed tool call. In a batch, other
assessments MUST continue, including queued assessments, and their successful
writes MUST be retained. The command MUST return a nonzero exit status after
the batch finishes. Discovery and search MAY omit denied candidates without
aborting, but explicitly requesting a denied file or directory MUST abort the
affected assessment.
Malformed tool arguments and ordinary read failures MUST retain their existing
recoverable tool-error behavior.
:::

:::{req} generated_inputs
Habeas MUST permit existing Cargo build-script output directories as an exception
to default Git tracking and containment restrictions. It MUST discover the Cargo
build directory using metadata and inspect existing build-script output records,
without running a build or executing build scripts. It MUST NOT treat its own
inherited `OUT_DIR` environment variable as the workspace's output directory.
Only discovered output directories and their contents MUST receive this default
permission; unrelated build artifacts MUST remain subject to ordinary rules.
Workspace disallow entries MUST override this permission.

Discovery MUST tolerate missing builds or unavailable metadata; callers MUST be
able to allow undiscovered outputs explicitly using `habeas.toml`. The model's
root directory listing MUST expose available generated-output directories,
including external ones. Generated inputs MUST use the existing file-range and
Rust-item formats and receive ordinary freshness checks. Validation MUST NOT
invoke a build to materialize an input.
:::

Cargo has one `OUT_DIR` per build-script execution. Discovery uses existing
`root-output` records beneath the Cargo build directory; this is Cargo's current
internal layout, so an explicit allow is the fallback for an undiscovered layout.
Generated paths are recorded as ordinary file selectors. CI must build the
matching paths and contents before checking that evidence.

### Discover files: `list_files`

:::{req} file_discovery
The harness MUST expose a `list_files` tool with this interface:

```text
list_files({ path: String = "." }) -> Array<DirectoryEntry>

DirectoryEntry {
    path: String,
    kind: "file" | "directory",
}
```

The tool MUST list the immediate children of the requested directory, returning
project-relative paths where possible and absolute paths for permitted external
files. Its root listing MUST also expose available generated-output directories
and configured external allow paths. The model can list a returned
directory in another call to explore further. A failed listing MUST return an
explanatory tool error unless access was denied, which MUST abort under
{cite}`requirements.authoring.read_denial`. Directory listings MUST NOT become evidence inputs.
Listings MUST omit files excluded by {cite}`requirements.authoring.source_tools`.
In a Git worktree, directories MUST have at least one eligible source
descendant to be listed unless explicitly allowed or discovered as generated
output. Explicitly requesting an excluded path MUST return an
authoring error under {cite}`requirements.authoring.read_denial`.
:::

### Search file contents: `ripgrep`

:::{req} text_search
The harness MUST expose a `ripgrep` tool with this interface:

```text
ripgrep({ pattern: String, path: String = ".", context: Integer = 5 }) -> SearchResult

SearchResult {
    inputs: Array<FileRangeInput>,
    definitions: Array<DefinitionInput>,
}
```

The tool MUST search the requested permitted file or directory using
ripgrep's regular-expression syntax and default case-sensitive search and ignore
rules. Explicitly allowed paths and discovered generated-output directories MUST
be searched even when ignored or hidden; workspace disallow rules still apply.
Its model-facing description MUST explain those conventions. `context`
MUST be nonnegative and specify how many lines before and after each matching
line to include, defaulting to five on each side. Windows MUST stop at file
boundaries, and overlapping windows in the same file MUST be merged. Each
resulting window MUST be returned as a file-range input with its source path,
inclusive line range, and complete text under {cite}`requirements.ci.inputs`.
Setting `context` to zero MUST return only matching lines.
Search results MUST exclude files disallowed by
{cite}`requirements.authoring.source_tools`; explicitly requesting an excluded
path MUST abort under {cite}`requirements.authoring.read_denial`.
Required terminology MUST be included under
{cite}`requirements.definitions.authoring_inputs`. Returned source inputs supplied
to the model MUST be recorded under {cite}`requirements.authoring.delivered_inputs`.
No matches MUST return empty arrays; invalid arguments or a failed search MUST
return an explanatory tool error.
:::

### Submit the result: `submit_judgment`

:::{req} submission
The harness MUST expose a `submit_judgment` tool accepting exactly these fields:

```text
submit_judgment({ determination: Judgment, rationale: String })

Judgment = Boolean | { FIXME: Array<String> }
```

The judgment MUST use the three outcomes and nonempty FIXME entries defined by
{cite}`requirements.ci.outcomes`.
`rationale` MUST be nonempty Markdown. This tool call MUST supply the final
assessment result. The model MUST invoke it exactly once over the entire
assessment. If the model finishes without invoking it, or invokes it more than
once, the harness MUST report an error and leave existing evidence unchanged.
The harness MUST check the complete model response for multiple submissions
before accepting one. Invalid submission arguments MUST also fail the assessment.
A valid single submission MUST finish the assessment; there is no submission
repair or resubmission loop. An assessment that fails before accepting a valid
judgment MUST emit no evidence JSON to stdout.
:::

## Initial prompt

### Mandatory inputs

:::{req} mandatory_inputs
The harness MUST construct each assessment's mandatory input set by combining
the contributions specified by the requirements applicable to that subject and
its inputs. A requirement that contributes inputs MUST cite this requirement and
define which sources it contributes and when. The harness MUST include every
applicable contribution, deduplicate identical selections, and report
unresolvable mandatory inputs before inference. The complete set MUST be supplied
in the initial model request.
Generated output under project-root `evidence/`, including symlink aliases, MUST
be omitted from previous input selections when the default exclusion applies.
An explicit workspace allow MUST permit those selections; an explicit disallow
MUST abort under {cite}`requirements.authoring.read_denial`.
:::

The contribution rules live with [requirements](../requirements.md#requirements-as-authoring-inputs),
[examples](examples.md#examples-as-authoring-inputs),
[references](references.md#references-as-authoring-inputs),
[terminology](definitions.md#terminology-as-authoring-inputs), and
[existing evidence inputs](ci.md#existing-evidence-inputs-used-by-authoring).

### Prompt template

:::{req} initial_prompt
The initial request MUST provide the four tool definitions above and the
following prompt. The harness MUST replace `{{subject_kind}}` with `requirement`
or `example`, `{{subject_name}}` with the full subject name, and
`{{mandatory_inputs}}` with the complete mandatory inputs as a JSON array using
the existing evidence-input formats. The template MUST otherwise be used as
written. Its review guidance MUST follow {cite}`requirements.ci.review_criteria`,
{cite}`requirements.examples.evidence`, and {cite}`requirements.ci.outcomes`.

```text
Bring your expert judgment to the task of assessing whether a project's sources satisfy a requirement or example.

Subject: {{subject_kind}} {{subject_name}}

Read the subject and mandatory inputs below. Use habeas to request any additional
source information you need. Use list_files to discover files and directories,
and ripgrep to search file contents.
The harness records source inputs for you; directory listings are not evidence.

Assess only the named subject, using cited requirements and definitions as context.
For a requirement, check that the cited code implements it as stated and that
tests cover the interesting points in the design space. If there are no tests,
assess whether simplicity or impracticality justifies their absence, and explain.
For an example, assess whether test setup and assertions cover its scenario and
expected outcomes. A true judgment needs at least one {test} reference to the
example in the supplied inputs. Explain the coverage.

Choose true when the subject meets these criteria. Choose {"FIXME": ["..."]} when
the remaining gaps are explicitly acknowledged in source comments or tests;
list their issue references or other identifying content. The usual convention
is FIXME(#125) or FIXME(rust-lang/rust#123), but references may occur elsewhere.
Choose false for other gaps or insufficient evidence. Explain your judgment in
a Markdown rationale with source locations, including any acknowledged gaps.

When you are ready, call submit_judgment exactly once with determination and
rationale. That tool call is your final answer.

Mandatory inputs:
{{mandatory_inputs}}
```
:::

### Rust function attestations

:::{req} function_attestations
The harness MUST author Rust function attestations using the schema and citation
completeness criteria in {cite}`requirements.references.rust_function_attestations`.
It MUST supply the complete function, its current citations, all requirement and
example declarations selected by `*`, and their required terminology as mandatory
inputs under {cite}`requirements.authoring.mandatory_inputs`. Existing attestation
input selectors MUST also contribute their current resolutions, subject to the
generated-output exclusion above. The harness MUST
derive the recorded citations from source and record the inputs it supplies;
the model MUST supply only the boolean determination and rationale. FIXME MUST
be rejected for a function attestation.

The same four tools and exactly-once submission rule MUST apply. The initial
prompt MUST use the following template, replacing `{{subject_name}}` with the
repository-relative Rust file and selector, `{{citations}}` with the current
citation objects as JSON, and `{{mandatory_inputs}}` with the mandatory input
array. {cite}`requirements.authoring.submission`
{cite}`requirements.authoring.delivered_inputs`

```text
Bring your expert judgment to the task of assessing a Rust function's citation completeness.

Subject: {{subject_name}}
Current citations: {{citations}}

Read the whole function and the requirement and example declarations below.
Look beyond the citations already present for every applicable requirement or
example, including obligations governing observable branches and error cases.
Assess citation completeness, not whether those requirements are implemented.

Use habeas for additional source inputs, list_files to discover files, and
ripgrep to search their contents. The harness records source inputs for you;
directory listings are not evidence.

Choose true only when the function cites every applicable requirement or
example. Choose false for missing citations or insufficient information.
Explain how you identified the applicable set and how the cited declarations
account for the function's behavior, with source locations, and why the reviewed
requirements inputs cover that set. The determination
must be boolean; FIXME is not a function-attestation outcome.

Call submit_judgment exactly once with determination and a nonempty Markdown
rationale. That tool call is your final answer.

Mandatory inputs:
{{mandatory_inputs}}
```
:::

## Configuration

:::{req} provider
The authoring harness MUST use Rig and support Bedrock and Ollama. Users MUST be
able to select a backend and model through configuration without changing the
assessment tools, prompt, or evidence format.
:::

:::{req} configuration
Authoring MUST use the `[authoring]` sections of the shared user configuration
and the selected workspace's `habeas.toml`.
{cite}`requirements.configuration.user_location`
{cite}`requirements.configuration.structure`
{cite}`requirements.configuration.feature_sections`
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
}

BackendConfig = Bedrock(BedrockConfig) | Ollama(OllamaConfig)

BedrockConfig {
    model: ModelId,                           // model ID or inference-profile ID/ARN
    aws_profile: Optional<String>,
    region: Optional<String>,
    credentials: Optional<BedrockCredentials>,
    options: Map<String, OptionValue>,
}

BedrockCredentials {
    access_key_id: SecretString,
    secret_access_key: SecretString,
    session_token: Optional<SecretString>,
}

OllamaConfig {
    model: ModelId,                           // exact identifier, including tag
    endpoint: Optional<String>,               // default: http://localhost:11434
    api_key_env: Optional<EnvironmentVariableName>,
    api_key: Optional<SecretString>,
    options: Map<String, OptionValue>,
}

OptionValue = String | Boolean | Integer | FiniteFloat
            | Array<OptionValue> | Map<String, OptionValue>

SecretString = String                         // serialized normally; redacted in Debug

ModelRef {
    backend: BackendKind,                     // Bedrock or Ollama
    model: ModelId,
}
```

`ModelRef` MUST be derived from the selected backend configuration and MUST
preserve the exact model identifier. It MUST identify the model without replacing
connection settings such as AWS profile, region, or Ollama endpoint.
:::

:::{req} configuration_serialization
The user `[authoring]` table MUST serialize UserAuthoringConfig, and the workspace
table MUST serialize WorkspaceAuthoringConfig. Each
`[authoring.configs.<name>]` table MUST serialize one AuthoringConfig, with
`backend = "bedrock"` or `backend = "ollama"` and that variant's fields in the same
table. Backend-specific inference options MUST appear in its `options` subtable.
Within a present user authoring section, `configs` MUST be present but MAY be empty;
workspace `configs` MAY be omitted, allowing a workspace default to select a
named user configuration.
omitted `projects` and `options` tables MUST deserialize as empty maps.
Serialization MUST follow the common rules in
{cite}`requirements.configuration.serialization`.
:::

:::{req} concurrency
Each named authoring configuration MUST accept an optional positive integer
`concurrency` in its configuration table. When omitted, the maximum number of
simultaneous assessments MUST default to the number of available logical CPUs
for Bedrock, falling back to one when that count is unavailable, and to one for
Ollama. A configured value MUST override the default for either backend; zero
MUST be rejected as invalid configuration.

Batch authoring MUST use asynchronous tasks on a runtime with a fixed worker
thread pool. The number of assessments running at once MUST NOT exceed the
selected concurrency. Each assessment MUST retain its own model conversation and
usage record. Provider errors, including throttling, MUST be reported through
the ordinary assessment-failure path.
:::

:::{req} configuration_storage
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
an error before a model request.
:::

:::{req} credentials
Bedrock configurations MAY supply `credentials` containing nonempty
`access_key_id`, `secret_access_key`, and optional nonempty `session_token`.
Ollama configurations MAY supply a nonempty `api_key`. Habeas MUST reject a
Bedrock record combining explicit credentials with `aws_profile`, and an Ollama
record combining `api_key` with `api_key_env`.

Explicit credentials MUST be passed directly to the selected provider without
changing process environment variables or writing credential files. Saved
explicit values MUST round-trip; credentials resolved from profiles or the
environment MUST NOT be serialized back. Debug output, configuration diagnostics,
usage records, and initial prompts MUST NOT include credential values. TOML
diagnostics MUST NOT echo credential-containing source lines. An active workspace
file containing explicit credentials MUST be excluded from model source reads by
default under {cite}`requirements.authoring.source_tools`; explicit workspace path
overrides still apply.

Documentation MUST discourage literal credentials in workspace files and
recommend AWS profiles or environment-variable references. Examples MUST use
placeholders and explain that workspace files may be committed or read as source
when explicitly allowed.
:::

:::{req} rig_configuration_mapping
Habeas MUST resolve its saved configuration before constructing Rig provider
clients and completion models. The Habeas data structures MUST remain the saved
format. Rig model references MAY be reused internally without discarding the
connection settings they do not represent.

The Bedrock adapter MUST pass the chosen AWS profile and region through AWS SDK
configuration, or use explicitly configured credentials. The Ollama adapter MUST use the chosen server, exact model ID,
and optional authentication; the server MAY be local or remote. Both adapters
MUST supply the same tools and initial prompt.
:::

### Editing the file

:::{req} configuration_documentation
Authoring documentation MUST follow {cite}`requirements.configuration.documentation`
and include complete TOML examples for Bedrock and Ollama, with all placeholders
identified. It MUST show how to choose a default, use a remote Ollama server,
select a configuration for a project or invocation, and edit or remove selections
by hand. It MUST explain backend-specific options and credential-source references.
:::

Create the user file at the [shared configuration location](configuration.md#locations-and-scope).
For workspace settings, create `habeas.toml` in the directory whose descendants
should use them. The nearest enclosing file wins. This complete workspace file
selects the user's saved `cloud` configuration and makes a generated source
directory readable while excluding its private subtree:

```toml
version = 1

[authoring]
default = "cloud"

[evidence]
allow = ["generated"]
disallow = ["generated/private"]
```

Workspace files can also contain `[authoring.configs.<name>]` tables using the
same backend structures shown below. Those records override same-named user
records as a whole. A user's explicit project selection takes precedence over a
workspace default; `--config` takes precedence over both. Remove the workspace
default or configuration table to fall back to the user settings.

For Ollama, this is a complete minimal file; replace `MODEL_TAG` with a model
identifier available on the local server:

```toml
version = 1

[authoring]
default = "local"

[authoring.configs.local]
backend = "ollama"
model = "MODEL_TAG"
```

For Bedrock, this is a complete minimal file. Replace `MODEL_ID` with an available
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

This complete file combines Bedrock, local Ollama, and remote Ollama. Replace
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
`habeas author <full-name>` to assess one requirement or example. Add `--config local`
to override the saved selection for one run. Set `concurrency` alongside `backend`
and `model` to override the provider's default number of simultaneous assessments:

```toml
[authoring.configs.cloud]
backend = "bedrock"
model = "MODEL_ID"
concurrency = 4
```

Here `MODEL_ID` is your available model identifier and four is an illustrative
choice, not the default. Bedrock defaults to the available logical CPU count;
Ollama defaults to one. Lower concurrency if the provider reports throttling;
rerunning `habeas author --all` preserves current accepted results and retries
records that still need attention.

Backend options belong under
`[authoring.configs.<name>.options]`. For an authenticated Ollama server, put
`api_key_env = "OLLAMA_API_KEY"` in its configuration table and set that variable
outside the file. Omit `aws_profile` or `region` to use AWS SDK defaults; omit an
Ollama endpoint to use localhost.

Literal credentials are supported but not recommended, especially in a
workspace file that might be committed. Prefer the profile and environment
references above. If needed, this complete `habeas.toml` illustrates the fields;
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
The active workspace file is excluded from model source reads when it contains
literal credentials, unless you explicitly allow it. Such an allow exposes its
contents to the model and records them as evidence inputs.

## Recording results

:::{req} delivered_inputs
The harness MUST record the source text it supplies to the model, including
mandatory inputs and additional reads, using the existing evidence input formats
defined by {cite}`requirements.ci.inputs`,
{cite}`requirements.ci.rust_item_inputs`,
{cite}`requirements.ci.requirements_inputs`, and
{cite}`requirements.definitions.inputs`. The harness MUST create these records
itself so ordinary evidence validation can check them later.
:::

:::{req} persistence
After one valid judgment, the harness MUST write the subject text, submitted
determination and rationale, and recorded inputs using the existing evidence
schema. {cite}`requirements.ci.schema`
By default, it MUST write the subject's evidence file. With `--stdout`, it MUST
emit a single evidence JSON object followed by a newline to stdout and MUST NOT
create or change evidence files or directories. All three outcomes MUST be
written to the selected destination as completed assessments; their validation
status MUST follow {cite}`requirements.ci.determination`.
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
Usage MUST record mandatory input counts and sizes by evidence kind, in UTF-8
bytes and lines, and the same information for additional inputs requested through
`habeas` or `ripgrep`. A requirements input MUST also record its number of matched declarations.
Usage MUST distinguish unique inputs from repeated reads, successful from
failed reads, and prepared inputs from those actually sent to the model.
File-discovery calls MUST count as tool calls without recording directory
listings as evidence or source inputs.
:::

:::{req} usage_requests
Usage MUST record each provider request's outcome and available input/output
token counts, including retries, with cached or reasoning-token counts when
provided. Missing measurements MUST be recorded as unavailable, not zero.
Run totals MUST include request and tool-call counts by tool, elapsed time, and
available token usage, identifying incomplete totals without double counting
provider counters that overlap.
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
A requirement has an implementation citation in one function, a test citation in
another, and a terminology reference. The initial prompt contains the complete
requirement, both functions, and the required definition as evidence inputs.
{cite}`requirements.authoring_input`
{cite}`requirements.references.authoring_inputs`
{cite}`requirements.definitions.authoring_inputs`
{cite}`requirements.authoring.mandatory_inputs`
{cite}`requirements.authoring.initial_prompt`
:::

:::{example} additional_read
The model calls `list_files` to find a directory, then `ripgrep` to find matching
lines in a Rust file. The listing creates no evidence input; the search returns
and records file-range inputs including five context lines on each side, where
available. A `habeas` request with that file's `rust_file` and
a `rust_item` selector returns and records the complete Rust item, along with any
required terminology. A file-range request omitting `line_end` reads through EOF
and records the actual ending line without requiring the model to know it.
{cite}`requirements.authoring.file_discovery`
{cite}`requirements.authoring.text_search`
{cite}`requirements.authoring.source_tools`
{cite}`requirements.authoring.delivered_inputs`
:::

:::{example} submission_count
A model finishes with prose but never calls `submit_judgment`: the assessment
fails without writing evidence. A response with two `submit_judgment` calls also
fails, even when their payloads are identical. A single call with invalid
arguments fails without requesting a replacement submission.
{cite}`requirements.authoring.submission`
:::

:::{example} negative_assessment
The model reads additional sources and calls `submit_judgment` once with
`determination: false` and a rationale explaining the discrepancy. The harness
saves that judgment with all source inputs it supplied. The authoring operation
completes; ordinary evidence validation reports the false determination.
{cite}`requirements.authoring.submission`
{cite}`requirements.authoring.delivered_inputs`
{cite}`requirements.authoring.persistence`
:::

:::{example} acknowledged_gap
The cited code implements a requirement except for a boundary case explicitly
documented in a source comment as `FIXME(#125)`. The model finds no other gaps
and calls `submit_judgment` once with `determination: {"FIXME": ["#125"]}` and a
rationale locating the comment and explaining the boundary case. The harness
saves that judgment and the source inputs, including the comment. CI accepts
the FIXME judgment when all other evidence checks pass.
{cite}`requirements.ci.outcomes`
{cite}`requirements.ci.determination`
{cite}`requirements.authoring.submission`
{cite}`requirements.authoring.delivered_inputs`
{cite}`requirements.authoring.persistence`
:::

# Configuration

Habeas uses a shared user configuration file and workspace settings in `habeas.toml`.
This chapter defines their locations, common structure, and loading rules. Each
configurable feature defines its own section, data structures, defaults, and
resolution rules, and cites these shared requirements.
Persistent activity history uses the user state directory defined here.

Users create and edit the files with a text editor. A configuration-management
command is not needed.

## Locations and scope

:::{req} user_location
Habeas MUST locate its per-user configuration at
`$XDG_CONFIG_HOME/habeas/config.toml` on Unix when XDG_CONFIG_HOME is an absolute,
nonempty path, otherwise at `$HOME/.config/habeas/config.toml`. On Windows it MUST
use `%APPDATA%/habeas/config.toml`. `HOME` and `APPDATA` MUST likewise be
absolute, nonempty paths to be used. If no usable base directory can be
determined, an operation requiring user configuration MUST fail with an
explanatory diagnostic naming the variable.

The same user configuration MUST be usable across projects. Its location MUST
NOT be selected relative to the current project or changed by project settings.
User-local preferences for a project MAY remain in the user configuration;
workspace settings MUST use `habeas.toml` under
{cite}`requirements.configuration.workspace_location`.
:::

:::{req} workspace_location
Habeas MUST search from its invocation directory upward for the nearest enclosing
`habeas.toml`. It MUST use that file alone, without merging ancestor workspace
files. A library operation given a project root MUST search from that root unless
its current directory is inside the project, in which case it MUST start there.
The directory containing the selected file defines the base for its relative
paths; it does not replace the Cargo and mdBook project root. A missing file MUST
mean absent workspace settings. Discovery MUST report unreadable candidate files
rather than silently selecting an ancestor. Loading MUST NOT create the file.
:::

### Persistent state

Usage history belongs in the state directory described by the
[XDG Base Directory Specification](https://specifications.freedesktop.org/basedir/latest/#variables).

:::{req} state_location
Habeas MUST store persistent per-user activity state under
`$XDG_STATE_HOME/habeas` on Unix when XDG_STATE_HOME is an absolute, nonempty path,
otherwise under `$HOME/.local/state/habeas`. On Windows it MUST use
`%LOCALAPPDATA%/habeas/state`. `HOME` and `LOCALAPPDATA` MUST likewise be
absolute, nonempty paths to be used. Features MUST use a named subdirectory under that
state root and document their record formats and locations.

State MUST persist across invocations and MUST NOT be stored in the project
repository, the configuration file, or a disposable cache directory. Habeas MUST
create the required directories when recording state. If it cannot determine or
write the state location, the operation requiring that state MUST report the
failure and MUST NOT claim that its history was recorded.
:::

## Common data structures

The following schema notation describes the serialized data. `Optional<T>` means
the field may be absent. Feature fields use the types defined by that feature's
requirements; the current feature is [evidence authoring](authoring.md).

```text
UserConfig {
    version: Integer,                         // exactly 1
    authoring: Optional<UserAuthoringConfig>,  // defined by evidence authoring
}

WorkspaceConfig {
    version: Integer,                              // exactly 1
    authoring: Optional<WorkspaceAuthoringConfig>,  // defined by evidence authoring
    evidence: Optional<EvidenceConfig>,
    coverage: Optional<CoverageConfig>,
}

EvidenceConfig {
    allow: Array<Path>,                            // omitted means empty
    disallow: Array<Path>,                         // omitted means empty
}

CoverageConfig {
    reauthorings: Optional<Integer>,               // positive; omitted means 3
}
```

`UserConfig` is the root of the user file. `WorkspaceConfig` is the root of
`habeas.toml`; `book.toml` remains mdBook configuration. Adding a configurable
feature adds a named, typed optional field to the
applicable structure. These are explicitly defined fields, not an unrestricted
map of arbitrary settings.
{impl}`requirements.configuration.documentation`

:::{req} structure
The user configuration file MUST be UTF-8 TOML serializing UserConfig. It MUST
contain `version = 1` and MAY contain the documented feature tables. The common
root MUST contain only the schema version and those feature tables; feature
settings MUST NOT occupy unscoped root keys. A user file containing only
`version = 1` MUST be valid common configuration.

Workspace configuration MUST follow {cite}`requirements.configuration.workspace_structure`.
The absence of an optional feature section MUST be distinguishable
from that feature's explicit settings and interpreted according to its own
requirements.
:::

:::{req} workspace_structure
`habeas.toml` MUST serialize WorkspaceConfig as UTF-8 TOML with `version = 1`.
Its supported feature tables are `[authoring]`, `[coverage]`, and
`[evidence]`; each MAY be absent. `[coverage]` has one optional key,
`reauthorings`, under {cite}`requirements.references.coverage.history`; it has
no user-file counterpart or invocation option, and no operation requires it. Unknown keys, unsupported versions, and invalid field types MUST produce
a diagnostic identifying the file and setting. Invalid TOML, including content
that is not UTF-8, MUST produce a diagnostic identifying the file, with the line
and column of a syntax error.
Features MUST define their own data structures and loading rules under
{cite}`requirements.configuration.feature_sections`. A file containing only
`version = 1` MUST be valid. Habeas settings MUST NOT require a `[habeas]` table
inside `book.toml`.
:::

:::{req} evidence_paths
The workspace `[evidence]` table MUST serialize EvidenceConfig. `allow` and
`disallow` MUST be arrays of nonempty literal paths, each naming a file or a
directory subtree. Relative paths MUST resolve from the selected `habeas.toml`
directory; absolute paths MUST be supported. These paths are not glob patterns.
An omitted `allow` or `disallow` MUST mean an empty array, and an absent
`[evidence]` table MUST mean both are empty, so only the default source-access
rules apply. No operation requires the section. The table has no user-file
counterpart, and these rules have no invocation options. This section refines
{impl}`requirements.configuration.feature_sections` for the evidence feature.

For authoring reads, a matching disallow MUST reject the path; otherwise a
matching allow MUST permit it; otherwise the default source-access rules MUST
apply. Explicit allow MUST override every default, including Git tracking,
project containment, generated-evidence exclusion, and automatic generated-output
rules. Disallow MUST win even when an allow or generated-output rule also matches.
Both the requested path and its resolved symlink target MUST be checked for
disallow, so an allowed alias cannot expose a disallowed target.

The same explicit disallow rules and external-path exceptions MUST apply when
resolving recorded evidence inputs, including ordinary validation. Validation MUST
remain independent of Git index membership: it
checks the recorded inputs' existence and contents. Loading path policy MUST NOT
resolve credentials or validate unused authoring settings.
:::

For example, this is a complete user `config.toml` with no feature settings:

```toml
version = 1
```

Evidence authoring adds `[authoring]` and its subtables to both files. Complete
examples and the definitions of their authoring structures appear in
[Authoring evidence](authoring.md#serialized-configuration-data-model).

A complete workspace `habeas.toml` can select a model configuration saved in the
user file and permit generated sources while excluding one subtree:

```toml
version = 1

[authoring]
default = "cloud"

[evidence]
allow = ["generated"]
disallow = ["generated/private"]
```

Replace `generated` and `generated/private` with your own paths.
`generated/private` is excluded even though its parent is allowed. An allow is an
exception to the defaults, not an exclusive list of every readable path.
Edit either array to change the policy, or delete the `[evidence]` table to
restore the default source-access rules.
{impl}`requirements.configuration.documentation`

## Feature sections

:::{req} feature_sections
Each configurable feature MUST define the name and typed data structure of its
user section and any supported project section, citing these common configuration
requirements. The feature MUST specify its supported keys, value types, defaults,
validation rules, and the precedence or combination rules for user settings,
project settings, and invocation options. It MUST specify the behavior of an
absent section and which operations require that section.

Feature-specific fields MUST stay within the feature's section. A feature MUST
NOT reinterpret another feature's settings or change the shared file locations
or schema version. A feature MAY define a provider-owned options map, but MUST
document that boundary and how those options are validated.
:::

:::{req} serialization
Saved configuration MUST remain distinct from resolved runtime configuration.
Serializing and deserializing a valid configuration MUST preserve its semantic
values, including explicit versus omitted settings. Optional fields MAY be
omitted on serialization; defaults and environment-derived values MUST NOT be
materialized into omitted fields. TOML comments and formatting need not
round-trip.

Loading configuration or running a configured feature MUST NOT rewrite the user
or project configuration as a side effect. Resolved credentials and live runtime
objects MUST NOT enter the serialized data structures. Explicitly supplied
credential values are permitted only in the fields documented by the authoring
feature; credentials resolved from a profile or environment MUST NOT be written
back to configuration.
:::

## Loading and diagnostics

:::{req} loading
An operation MUST load user configuration only when its requirements call for
user settings. A missing user file MUST represent absent user configuration;
whether the operation can proceed MUST be defined by the feature. Loading MUST
NOT create a file, repair malformed input, or supply an undocumented selection.

When loading configuration, Habeas MUST report unreadable files, invalid TOML,
unsupported user-file versions, unknown common keys, and invalid common field
types. It MUST validate the feature sections used by that operation and report
invalid feature settings with the file and setting identified. Unknown keys in
an explicitly documented provider-owned options map MUST follow that feature's
validation rules.

An operation MUST NOT require or semantically validate configuration for a
feature it does not use. This isolation MUST NOT suppress TOML syntax errors in
a shared file that the operation otherwise needs to parse. In particular,
ordinary validation still requires a parseable `book.toml` while remaining
independent of authoring settings and the user configuration file. It MUST apply
the selected workspace's evidence path policy without loading its model clients
or credentials.
:::

## Manual editing and documentation

:::{req} documentation
Habeas MUST document the user and project configuration locations, the common
file structure and schema version, and how to create and edit the files by hand.
Each configurable feature MUST document its fields, defaults, resolution rules,
and credential-source references, with complete, copyable examples identifying
all placeholders and the file to edit. Documentation MUST explain how to change
or remove optional settings and selections.

Configuration MUST NOT require a configuration-management command or generated
state beyond the documented files. Primary examples MUST use credential-source
references; examples explaining explicit credential fields MUST use clearly
identified placeholders, never real secrets.
:::

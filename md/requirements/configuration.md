# Configuration

Habeas uses a shared user configuration file and project settings in `book.toml`.
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
use `%APPDATA%/habeas/config.toml`. If the required base directory cannot be
determined, an operation requiring user configuration MUST fail with an
explanatory diagnostic.

The same user configuration MUST be usable across projects. Its location MUST
NOT be selected relative to the current project or changed by project settings.
User-local preferences for a project MUST remain in the user configuration;
shared project settings MUST remain in the project's `book.toml`.
:::

### Persistent state

Usage history belongs in the state directory described by the
[XDG Base Directory Specification](https://specifications.freedesktop.org/basedir/latest/#variables).

:::{req} state_location
Habeas MUST store persistent per-user activity state under
`$XDG_STATE_HOME/habeas` on Unix when XDG_STATE_HOME is an absolute, nonempty path,
otherwise under `$HOME/.local/state/habeas`. On Windows it MUST use
`%LOCALAPPDATA%/habeas/state`. Features MUST use a named subdirectory under that
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

ProjectConfig {
    // No shared project feature settings are currently defined.
}
```

`UserConfig` is the root of the user file. `ProjectConfig` is the `[habeas]` table
within `book.toml`; the rest of the book configuration retains its existing
meaning. Adding a configurable feature adds a named, typed optional field to the
applicable structure. These are explicitly defined fields, not an unrestricted
map of arbitrary settings.

:::{req} structure
The user configuration file MUST be UTF-8 TOML serializing UserConfig. It MUST
contain `version = 1` and MAY contain the documented feature tables. The common
root MUST contain only the schema version and those feature tables; feature
settings MUST NOT occupy unscoped root keys. A user file containing only
`version = 1` MUST be valid common configuration.

Shared project configuration MUST serialize ProjectConfig under `[habeas]` in
`book.toml`, with each feature in `[habeas.<feature>]`. The `[habeas]` table and
its feature tables MAY be absent. The user-file version MUST NOT be required in
`book.toml`. The absence of an optional feature section MUST be distinguishable
from that feature's explicit settings and interpreted according to its own
requirements.
:::

For example, this is a complete user file with no feature settings:

```toml
version = 1
```

Evidence authoring adds `[authoring]` and its subtables to the user file. It
defines no shared project settings. Complete examples and the definition of
`UserAuthoringConfig` appear in
[Authoring evidence](authoring.md#serialized-configuration-data-model).

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
objects MUST NOT enter the serialized data structures.
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
independent of authoring settings and the user configuration file.
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
state beyond the documented files. Examples MUST use credential-source
references rather than secrets.
:::

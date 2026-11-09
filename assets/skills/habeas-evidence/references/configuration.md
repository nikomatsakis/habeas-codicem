# Authoring configuration

`habeas author --config <name>` selects a saved authoring configuration. Saved
configurations live in the user configuration file or the nearest enclosing
`habeas.toml`. Prefer named profiles and environment references over literal
credentials in workspace files.

Bedrock runs assessments concurrently by default; Ollama runs one at a time. A
configuration's positive `concurrency` setting overrides its default. If the
provider reports throttling, lower `concurrency` and rerun the command; the
harness keeps evidence already written.

Don't edit the user's configuration file or `habeas.toml` without asking. If
credentials are missing or expired, ask the user to provide or refresh them. If
configuration or provider access is missing, report the blocker. Never work
around it by writing evidence yourself.

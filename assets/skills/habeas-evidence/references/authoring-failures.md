# Authoring failures

## Configuration, credentials, and throttling

`habeas author --config <name>` selects a saved authoring configuration. Saved
configurations live in the user configuration file or the nearest enclosing
`habeas.toml`. Prefer named profiles and environment references over literal
credentials in workspace files.

Bedrock runs assessments concurrently by default; Ollama runs one at a time. A
configuration's positive `concurrency` setting, set only in the configuration
file, overrides its default. The harness retries throttling itself; if a run
still fails on throttling, ask the user to lower `concurrency`, then rerun the
command. The harness keeps evidence already written.

Don't edit the user's configuration file or `habeas.toml` without asking. If
credentials are missing or expired, ask the user to provide or refresh them. If
configuration or provider access is missing, report the blocker.

The `once` policy (`--policy once` or a configuration's `policy`) skips the
second assessment to save time and tokens; use it only if the user asks.

## Denied reads

Authoring uses the SDK’s project source rules. Untracked files are readable
unless a project `.gitignore` excludes them. Git metadata and files outside
the project are excluded, including external symlink targets. A failed tool
read returns an error to the reviewer. An unavailable mandatory input aborts
that assessment before inference and the other assessments in a batch continue.

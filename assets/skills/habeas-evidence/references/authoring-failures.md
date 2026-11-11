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

Authoring reads only permitted sources. By default it excludes untracked files,
generated `evidence/` content, and paths outside the project; existing Cargo
build-script output directories are allowed when discoverable.

The nearest enclosing `habeas.toml` can set `[evidence]` `allow` and `disallow`
paths: literal files or directory subtrees, relative to that file. Allow
overrides the defaults, and disallow wins over allow.

A denied model read, such as an untracked file, `evidence/`, a path an
`[evidence] disallow` rule matches, a path outside the project, or build output,
is an ordinary tool error: the reviewer is told the path and why, and the
assessment continues (see Gotchas in SKILL.md). Only a denied mandatory input aborts
the assessment, before any model request; other assessments in the batch
continue, and the command reports failure. If the denied file is one you
created, stage it and rerun. Otherwise ask the user whether to stage it or
change the `[evidence]` policy.

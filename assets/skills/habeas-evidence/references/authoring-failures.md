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

A model request for a path outside the project root, or inside the book's
build output (`book`, or `book.toml`'s `[build] build-dir`) or the project-root
`target` directory, is an ordinary read failure: the reviewer gets a tool error
and the assessment continues, so it needs no action. Other denied explicit
reads abort that assessment: an untracked file, `evidence/`, a path an
`[evidence] disallow` rule matches, a mandatory input outside the project, or a
path whose symlink target leaves it. Other assessments in the batch continue,
and the command reports failure. If the denied file is one you created, stage
it and rerun. Otherwise ask the user whether to stage it or change the
`[evidence]` policy.

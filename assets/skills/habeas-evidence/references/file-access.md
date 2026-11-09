# File access during authoring

Authoring reads only permitted sources. By default it excludes untracked files,
generated `evidence/` content, and paths outside the project; existing Cargo
build-script output directories are allowed when discoverable.

The nearest enclosing `habeas.toml` can set `[evidence]` `allow` and `disallow`
paths: literal files or directory subtrees, relative to that file. Allow
overrides the defaults, and disallow wins over allow.

A denied explicit read aborts that assessment; other assessments in the batch
continue, and the command reports failure. If the denied file is one you
created, stage it and rerun. Otherwise ask the user whether to stage it or
change the `[evidence]` policy.

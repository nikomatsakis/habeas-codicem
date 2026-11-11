# FIXME judgments

A FIXME judgment, such as `{"FIXME": ["rust-lang/rust#123"]}`, means `{fixme}`
references to the definition acknowledge every remaining gap. Validation
accepts it like `true` when all other checks pass.

When a gap is real and the user agrees to defer it, add a fenced `{fixme}`
reference to the definition near the code or test it concerns, naming the issue
that tracks it:

```text
// :::{fixme} requirements.cli.status
// :github: rust-lang/rust#123
// Selections with globs are not tested.
// :::
```

The reference makes the definition's evidence stale, so the next
`habeas author --all` reassesses it. A plain `FIXME` or `TODO` comment does not
count. Never invent issue numbers, and never add a `{fixme}` just to get past a
false determination.

A `{fixme}` counts only while each issue it names is open. When one closes,
`habeas status` lists it and treats the record as stale: fix the gap, or name
another open issue that tracks it.

An unnamed `:::{fixme}` with only `:github:` lines ties any place, including a
requirement's text, to an issue. When that issue closes, `habeas status`
reports the containing item or requirement as failing, so revisit it.

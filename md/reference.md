# How it works

Habeas reads requirements from an mdBook and evidence from JSON files in the same repository. `cargo test --workspace` checks the evidence; `mdbook build` renders links to it.

## Requirements

Write a colon-fenced `{req}` directive in a book chapter. For example, in `md/requirements.md`:

```text
:::{req} check
The CLI MUST report invalid arguments.
:::
```

The chapter path is part of the requirement's name, so this one is `requirements.check`. Everything between the opening and closing fences is its body, including any Markdown examples or headings.

## Evidence

Add `evidence/requirements.check.json` for that requirement. Each evidence file records four fields:

| Field | Meaning |
| --- | --- |
| `requirement` | The complete requirement body, copied from the directive. |
| `determination` | `true` when you judge the requirement met. Validation fails if it is `false`. |
| `rationale` | A Markdown explanation of that judgment. |
| `inputs` | Repository file excerpts supporting it. Each input has a `file`, inclusive `line_start` and `line_end`, and `text` containing the exact lines. |

Habeas checks that every requirement has evidence, that every evidence file names a requirement, and that recorded text and input lines still match the repository.

## References

Source files and book chapters can mark lines as implementing, citing, or testing a requirement:

```text
{impl}`requirements.check`
{cite}`requirements.check`
{test}`requirements.check`
```

Each reference must name an existing requirement. Its evidence must include an input covering the line with the reference. In the book, these references and requirement definitions become linked badges.

## Checking and viewing

The test created by `habeas setup` calls `habeas::validate()`. Run `cargo test --workspace` to find missing evidence, false determinations, changed lines, and uncovered references. The check reports all the errors it finds, with diffs for changed text. Run `mdbook build` to render the book.

To add an evidence index, list a chapter in `SUMMARY.md` and put this directive in it:

```text
:::{requirements-index}
:::
```

The build fills that chapter with requirement states and adds an evidence page under it for each requirement.

## Using a local checkout

To use a checkout instead of the registry package, install the CLI from that checkout and point setup at it:

```sh
cargo install --path /path/to/habeas-codicem
habeas setup --path /path/to/habeas-codicem
```

Run `habeas setup` in the consuming Cargo and mdBook project. Its book uses the installed `habeas mdbook` CLI; the repository-local `cargo habeas` alias is only for Habeas's own book.

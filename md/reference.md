# How it works

Habeas reads requirements from an mdBook and evidence from JSON files in the same repository. `cargo test --workspace` checks the evidence; `mdbook build` renders links to it.

## Requirements

Write a colon-fenced `{req}` directive in a book chapter. For example, in `md/requirements.md`:

```text
:::{req} check
The CLI MUST report invalid arguments.
:::
```

The chapter path is part of the requirement's name, so this one is `requirements.check`. Everything between the opening and closing fences is its body, including headings and other Markdown.

## Examples

An example is a separate named definition for a concrete case. State the situation, action, and expected result, and cite the requirement it illustrates. This book contains a working example:

:::{example} cli.unknown_command
Running `habeas unknown` fails with a nonzero exit status and prints a usage message. {cite}`requirements.cli.invocation`
:::

Its full name is `reference.cli.unknown_command`. The `unknown_command_fails_with_usage` integration test targets it with `{test}` followed by the backtick-delimited full name. The agent records `evidence/reference.cli.unknown_command.json`, including the example's exact body, test source lines, and a rationale explaining how the test covers the case. A passing validation check confirms the citations are current; the rationale explains why the tests are adequate.

## Evidence

Add `evidence/requirements.check.json` for a requirement or `evidence/reference.cli.unknown_command.json` for the example above. Each evidence file records four fields:

| Field | Meaning |
| --- | --- |
| `requirement` or `example` | The complete body, copied from the corresponding directive. Use the field matching its kind. |
| `determination` | `true` when you judge the requirement or example met. Validation fails if it is `false`. |
| `rationale` | A Markdown explanation of that judgment. |
| `inputs` | Source snapshots supporting it. Inputs may select a file range, Rust item, or glob of definitions. |

An input has exactly one of these forms. `text` always stores complete raw lines, so comments remain available to the reviewer:

```json
{"file":"src/main.rs","line_start":1,"line_end":3,"text":["..."]}
{"rust_file":"src/main.rs","rust_item":"fn[main]","text":["..."]}
{"requirements":"requirements.cli.*","matches":["requirements.cli.info"],"text":["..."]}
```

Use `habeas add-input` to capture current source text instead of assembling the JSON by hand. A changed input leaves its determination false until an agent reassesses it. {cite}`requirements.cli.add_input` {cite}`requirements.ci.schema`

```sh
habeas add-input --for-requirement requirements.ci.inputs --file-range src/evidence.rs:1-20
habeas add-input --for-example reference.cli.unknown_command --rust-item tests/cli.rs:26
habeas add-input --for-rust-item src/evidence.rs 'fn[validate_repository]' --requirements 'requirements.ci.*'
```

The `--rust-item <file>:<line>` mode selects all explicit Rust items declared on that line. A named item is declared where its name appears; an unnamed item is declared where its keyword appears. Rust-item snapshots include attached comments, doc comments, and attributes. Moving unchanged lines earlier or later in the file does not stale the snapshot. A requirements glob includes requirements and named examples, records every matched name, and stores the full raw fenced declarations in sorted name order. Changes to a matched declaration or the match set invalidate the input. {cite}`requirements.cli.add_input_file_range` {cite}`requirements.cli.add_input_rust_item` {cite}`requirements.cli.add_input_requirements` {cite}`requirements.ci.rust_item_inputs` {cite}`requirements.ci.requirements_inputs`

Habeas checks that every requirement and example has evidence, that every evidence file names a definition, and that recorded text and input lines still match the repository. An example with a true determination also needs a `{test}` reference.

## References

Source files and book chapters can mark lines as implementing, citing, or testing a requirement or example:

```text
{impl}`requirements.check`
{cite}`requirements.check`
{test}`requirements.check`
{test}`reference.cli.unknown_command`
```

Each reference must name an existing requirement or example. Its evidence must include an input covering the line with the reference. In the book, references and definitions become linked badges.

For Rust, Habeas parses first-party source files and requires every explicit function or method, including trait methods and test helpers, to have a requirement or named example reference in an attached comment or its body. Each named example cites a requirement. A reference on an enclosing `impl` or module, in a string literal, or only in an evidence input does not cover the function. A Rust source file that cannot be parsed is reported as an error. For a helper with no behavior-specific requirement, agree on an explicit project requirement such as `internal-detail` and provide evidence explaining why it adds no otherwise unspecified user-visible behavior.

Rust-item selectors use syntactic ancestry within one repository-relative file. Examples include `mod[local]/fn[run]`, `impl[Thing]/fn[run]`, `impl[Thing/Trait]/fn[run]`, `trait[Trait]/fn[read]`, `fn[outer]/fn[inner]`, and positional `use#1`. Duplicate sibling names gain `#1`, `#2`, and so on. Delimiter characters and whitespace within names or impl types use percent-encoded UTF-8 bytes, so spelling can be recovered without resolving modules across files. {cite}`requirements.references.rust_item_selectors`

Each Rust function also needs an attestation in `evidence/rust/<rust_file>.json`, with a `rust_file` field and a `functions` array. Every entry has `rust_item`, a `citations` array of `{ "role": "impl", "name": "requirements.some_name" }` objects, `determination`, `rationale`, and `inputs`. It needs one Rust-item input for that function and at least one requirements input covering every cited definition. The agent must review the whole function and relevant definitions, then explain why its citations include *every applicable* requirement or named example and how those texts account for its behavior together. A matching snapshot alone is insufficient. {cite}`requirements.references.rust_function_attestations` {cite}`requirements.cli.function_attestation_guidance`

## Checking and viewing

The test created by `habeas setup` calls `habeas::validate()`. Run `cargo test --workspace` to find missing evidence, false determinations, changed lines, uncovered references, and uncited Rust functions. The check reports all the errors it finds, with diffs for changed text. Run `mdbook build` to render the book.

Run `habeas status [<glob>]` from within the project to list requirements and named examples needing attention and their validation findings. `--all` shows every matching definition and its evidence state. Use `habeas info <glob>` to print matching detail pages as Markdown, including current texts, rationales, inputs, and findings. Both commands match whole full names; `*` can span any number of characters, including dots. {cite}`requirements.cli.status` {cite}`requirements.cli.info` {cite}`requirements.references.definition_globs`

```sh
habeas info requirements.ci.inputs
habeas info 'requirements.cli.*'
habeas status 'requirements.cli.*' --all
```

Agent skills installed by Habeas point to `habeas info` for looking up definition details.

To add an evidence index, list a chapter in `SUMMARY.md` and put this directive in it:

```text
:::{requirements-index}
:::
```

The build fills that chapter with requirement and example states and adds an evidence page under it for each definition.

## Using a local checkout

To use a checkout instead of the registry package, install the CLI from that checkout and point setup at it:

```sh
cargo install --path /path/to/habeas-codicem
habeas setup --path /path/to/habeas-codicem
```

Run `habeas setup` in the consuming Cargo and mdBook project. Its book uses the installed `habeas mdbook` CLI; the repository-local `cargo habeas` alias is only for Habeas's own book.

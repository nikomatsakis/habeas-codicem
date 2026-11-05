# References and Rust items

:::{req} targets
Project text files MAY use the `{impl}`, `{cite}`, and `{test}` roles with a backtick-delimited full requirement or example name. Habeas MUST find these references throughout project source files and MUST fail with the source file and line if a name has no matching `{req}` or `{example}` definition. In Markdown, escaped roles and roles in code examples MUST remain literal. Evidence snapshots and generated build output MUST NOT be counted as additional source references.

```text
{impl}`requirements.references.targets`
{cite}`requirements.references.targets`
{test}`requirements.references.targets`
```
:::

:::{req} book_badges
In a book chapter, each valid `{impl}`, `{cite}`, or `{test}` reference MUST render as a Shields.io static badge image with meaningful alt text that displays its role and full name and links to the badge at the corresponding `{req}` or `{example}` definition. Links MUST work across chapters, including nested chapters. Each fenced `{req}` or `{example}` directive MUST render as a Shields.io definition badge image identifying its kind; text outside a directive MUST remain literal. Badge text MUST remain legible regardless of whether its link has been visited. These badges use the syntax defined by {cite}`requirements.references.targets`.
:::

:::{req} evidence_coverage
For every `{impl}`, `{cite}`, and `{test}` reference in a project source file, the evidence file for the referenced requirement or example MUST contain an input whose resolved source ranges cover the reference's file and line. Habeas MUST report every uncovered reference with its role, full name, source file, and line number.
:::

:::{req} rust_function_coverage
During repository validation, Habeas MUST parse every first-party Rust source file in the project. Every explicitly declared function or method, including trait methods and test helpers, MUST have a valid `{impl}`, `{cite}`, or `{test}` reference to a defined requirement or named example in a Rust comment attached to its declaration or within its body. A named example MUST cite a requirement. A reference in a string literal, on an enclosing module or impl block, or only in an evidence input MUST NOT count. Habeas MUST report every uncited function with its file, line, and name, and MUST report a Rust parse failure instead of silently skipping the file.
:::

:::{req} definition_globs
A definition glob MUST match a complete full name. `*` MUST match zero or more characters, including dots; all other valid name characters MUST match literally. An empty glob MUST be invalid. The same glob matching rule MUST be used by requirements evidence inputs and CLI definition filters. All three MUST match against both requirements and named examples.
:::

:::{req} rust_item_selectors
Habeas MUST identify a Rust item by the pair of its `rust_file`, following the path rules in {cite}`requirements.ci.schema`, and a `rust_item` selector derived from a `syn` parse of that file. The selector MUST describe syntactic ancestry within the file and MUST NOT resolve Rust modules across files or use line numbers as identity. Named items MUST use kind-and-name segments such as `fn[run]` and `struct[Packet]`. Impl segments MUST distinguish inherent from trait impls using their syntactic self type and optional trait path. Otherwise identical selectors MUST have deterministic source-order disambiguators. A `use` declaration MUST have a positional selector `use#N`, where N is its one-based position among sibling `use` declarations; its import target MUST NOT form part of the selector. Other items without a suitable name MUST have a kind and positional disambiguator. Selector spelling MUST be documented and reversible when names or types contain delimiter characters.
:::

:::{req} rust_function_attestations
For every function or method covered by `references.rust_function_coverage`, repository validation MUST require exactly one attestation in `evidence/rust/<rust_file>.json`. Each such file MUST be an object with exactly `rust_file` and `functions`, where `rust_file` matches its repository-relative source path and `functions` is an array. Each attestation MUST contain exactly `rust_item`, `citations`, `determination`, `rationale`, and `inputs`. Its `determination` MUST remain a boolean assessing citation coverage; the FIXME outcome applies to requirement and example judgments. Its `citations` MUST be the nonempty set of valid references owned by the function, each recorded by role and full requirement or example name. Its `inputs` MUST include one Rust-item input selecting the function and at least one requirements input; together, the requirements inputs MUST match every name in `citations`. Habeas MUST validate every input and fail for a missing, duplicate, orphaned, stale, or false attestation, a changed citation set, or an empty rationale. Each finding MUST identify the source file, current line when available, and item selector.
:::

:::{req} markdown_literals
In Markdown, escaped roles and role-looking text in inline code, fenced code, or raw HTML MUST remain literal and MUST NOT count as references. A `{req}` directive inside a fenced code example and a legacy inline `{req}` role MUST NOT define a requirement.
:::

:::{req} safe_badges
The book preprocessor MUST escape requirement names and link targets when inserting badge HTML. It MUST encode badge label and message text in Shields.io URL path components so punctuation cannot change the badge URL structure.
:::

:::{example} selector_nested
In `src/lib.rs`, `fn outer() { fn inner() {} }` gives the selectors `fn[outer]` and `fn[outer]/fn[inner]`. {cite}`requirements.references.rust_item_selectors`
:::

:::{example} selector_impl_and_trait
In `src/lib.rs`, a method `run` in `impl Thing` has selector `impl[Thing]/fn[run]`; a method `run` in `impl Trait for Thing` has selector `impl[Thing/Trait]/fn[run]`; and a method `read` declared in `trait Read` has selector `trait[Read]/fn[read]`. {cite}`requirements.references.rust_item_selectors`
:::

:::{example} selector_inline_and_foreign
A function `run` inside an inline `mod local { ... }` in `src/lib.rs` has selector `mod[local]/fn[run]`. A function defined in a separate `src/local.rs` file has a selector within that file, without reconstructing the path of a `mod local;` declaration elsewhere. A foreign function `puts` in an `extern "C"` block has selector `extern[C]/fn[puts]`. {cite}`requirements.references.rust_item_selectors`
:::

:::{example} selector_duplicate_names
Two sibling declarations named `run` in one file, guarded by different `#[cfg]` attributes, have selectors `fn[run]#1` and `fn[run]#2` in source order. Their source line numbers are not part of either selector. {cite}`requirements.references.rust_item_selectors`
:::

:::{example} selector_use
For consecutive sibling declarations `use alpha::A;` and `use beta::B;`, the selectors are `use#1` and `use#2`. Changing an import target leaves that declaration's positional selector the same and changes its raw evidence input. A named declaration such as `struct Packet;` instead has selector `struct[Packet]`. {cite}`requirements.references.rust_item_selectors` {cite}`requirements.ci.rust_item_inputs`
:::

:::{example} selector_raw_lines
For a function preceded by a doc comment, an attribute, and an attached citation comment, a Rust-item input starts at the earliest attached line and ends at the function's final line. Its `text` contains those complete raw source lines. Inserting lines before them changes the displayed range without changing the input. {cite}`requirements.ci.rust_item_inputs`
:::

:::{example} selector_same_line
For `impl Thing { fn run() {} }` on one line, `habeas add-input --for-requirement requirements.foo --rust-item src/lib.rs:1` adds separate inputs for `impl[Thing]` and `impl[Thing]/fn[run]`. Both contain the complete raw line. {cite}`requirements.cli.add_input_rust_item`
:::

:::{example} definition_glob_cases
Given a requirement `foo.a.bar` and a named example `foo.a.b.bar`, `foo.*.bar` matches both, in that order. A requirements input stores both full names and their complete raw fenced declarations. Adding another matching definition invalidates the existing input. {cite}`requirements.references.definition_globs` {cite}`requirements.ci.requirements_inputs`
:::

:::{example} attestation_reassessment
A function attestation has a Rust-item input selecting its function and a requirements input matching the definitions reviewed for it. Changing the function's raw lines or adding a definition matched by the requirements glob invalidates the attestation until the agent reviews it again. {cite}`requirements.references.rust_function_attestations`
:::

## References as authoring inputs

:::{req} authoring_inputs
When authoring evidence, every requirement or example directly referenced by the
assessed subject MUST contribute its complete fenced declaration to the
mandatory input set defined by {cite}`requirements.authoring.mandatory_inputs`.

Every source containing a valid implementation, citation, or test reference to
the assessed subject MUST also contribute to that set. For a reference in a Rust
item, the contribution MUST include the complete smallest enclosing item with
its attached comments and attributes. For a reference in a Markdown requirement,
example, or terminology declaration, it MUST include that complete declaration.
For other references, it MUST include the complete containing source file.
Literal references, evidence copies, and generated output MUST remain excluded
under the existing reference-discovery rules.

These contribution rules MUST apply to references directly from or to the
assessed subject. Including another declaration MUST NOT by itself recursively
expand that declaration's incoming or outgoing requirement and example references.
Citation links MUST NOT be described as proof that all substantively relevant
sources have been found.
:::

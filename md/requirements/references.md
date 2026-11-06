# References and Rust items

:::{req} targets
Project text files MAY use the `{impl}`, `{cite}`, and `{test}` roles with a backtick-delimited full requirement or example name. Each reference is a directed edge between its source and the named definition. A source is a project source item or a requirement or example declaration. `{cite}` MUST mean that the source relies on, supports, or refers to the definition, and is an edge from the source to the definition. `{impl}` MUST mean that the source implements part of the definition: it makes a decision, performs a check, produces an output, or handles a case that the definition specifies. A requirement MAY implement part of another requirement by refining one of its obligations. Calling code that implements a definition, or producing a result a caller uses to meet it, MUST NOT by itself count as implementing it. A requirement stating a convention for every output of a kind, such as every validation error, is implemented by the source that carries out the convention as a whole, such as the step that collects and reports those outputs. `{test}` MUST mean that the source is designed to verify behavior the definition specifies: a test through its scenario and the assertions about that scenario's outcome, or a named example through the concrete case it describes. `{impl}` and `{test}` are edges from the definition to the source. Only requirements and examples are assessed; code is reviewed as part of the definitions it implements or tests. Habeas MUST find these references throughout project source files and MUST fail with the source file and line if a name has no matching `{req}` or `{example}` definition. In Markdown, escaped roles and roles in code examples MUST remain literal. Evidence snapshots and generated build output MUST NOT be counted as additional source references.

```text
{impl}`requirements.references.targets`
{cite}`requirements.references.targets`
{test}`requirements.references.targets`
```
:::

:::{req} anchors
:impl: assets/skills/habeas-requirements/SKILL.md
A `{req}` declaration MAY list `:impl:` and `:test:` options, and an `{example}`
declaration MAY list `:test:` options, immediately after its opening fence and
before its body, one project file path per option line. Each anchor MUST be an `{impl}` or `{test}` edge, under
{cite}`requirements.references.targets`, from the definition to the complete
anchored file, for files that cannot or should not carry a reference themselves.
Paths MUST follow the path rules in {cite}`requirements.ci.schema`. Option lines
MUST NOT form part of the definition's text for evidence comparison or for
requirements inputs, and MUST NOT render as body text.

An anchored file MUST be covered by the definition's evidence under
{cite}`requirements.references.evidence_coverage`, by an input containing the
complete file, so changing the file makes that evidence stale. A path that does
not resolve to a readable, permitted file MUST be a validation error naming the
definition and the path; authoring MUST report it before inference, because the
definition cannot be assessed without it. Any other option line in a `{req}` or
`{example}` declaration MUST be a validation error.

Agents SHOULD NOT anchor a file when a more precise reference is available, such
as a reference attached to the Rust item that implements or tests the
definition. Habeas's bundled agent skills MUST give this direction.
:::

:::{req} book_badges
:impl: md/habeas.css
In a book chapter, each valid `{impl}`, `{cite}`, or `{test}` reference MUST render as a Shields.io static badge image with meaningful alt text that displays its role and full name and links to the badge at the corresponding `{req}` or `{example}` definition. Links MUST work across chapters, including nested chapters. Each fenced `{req}` or `{example}` directive MUST render as a Shields.io definition badge image identifying its kind; text outside a directive MUST remain literal. Badge text MUST remain legible regardless of whether its link has been visited. These badges use the syntax defined by {cite}`requirements.references.targets`.
:::

:::{req} evidence_coverage
For every `{impl}` and `{test}` reference in a project source file, including one in a requirement or example declaration, the evidence file for the referenced requirement or example MUST contain an input whose resolved source ranges cover the reference's file and line. A `{cite}` reference MUST NOT require coverage; it is discoverable through {cite}`requirements.authoring.reference_index`. Habeas MUST report every uncovered reference with its role, full name, source file, and line number.
:::

:::{req} rust_item_coverage
During repository validation, Habeas MUST parse every first-party Rust source file in the project. Every explicitly declared function or method, including trait methods and test helpers, and every struct, enum, and union MUST have a valid `{impl}`, `{cite}`, or `{test}` reference to a defined requirement or named example in a Rust comment attached to it or within its body. A comment immediately preceding an item's declaration or its attributes is attached to that item. A comment within an item's body, including one before a field, variant, or attribute inside it, belongs to the smallest enclosing item. A named example MUST reference a requirement under {cite}`requirements.examples.definition`. A reference in a string literal, on an enclosing module or impl block, or only in an evidence input MUST NOT count for the items it encloses. Habeas MUST report every unreferenced item with its file, line, kind, and name, and MUST report a Rust parse failure instead of silently skipping the file.
:::

:::{req} definition_globs
A definition glob MUST match a complete full name. `*` MUST match zero or more characters, including dots; all other valid name characters MUST match literally. An empty glob MUST be invalid. Every definition selection MUST use this matching rule, including requirements evidence inputs, CLI definition filters, and the authoring `references` tool, and MUST match against both requirements and named examples.
:::

:::{definition} definition_selection
:phrase: definition selection

A full requirement or example name, or a definition glob, identifying the
requirements and named examples it matches under
{cite}`requirements.references.definition_globs`. A name without `*` selects
exactly that definition.
:::

:::{req} rust_item_selectors
Habeas MUST identify a Rust item by the pair of its `rust_file`, following the path rules in {cite}`requirements.ci.schema`, and a `rust_item` selector derived from a `syn` parse of that file. The selector MUST describe syntactic ancestry within the file and MUST NOT resolve Rust modules across files or use line numbers as identity. Named items MUST use kind-and-name segments such as `fn[run]` and `struct[Packet]`. Impl segments MUST distinguish inherent from trait impls using their syntactic self type and optional trait path. Otherwise identical selectors MUST have deterministic source-order disambiguators. A `use` declaration MUST have a positional selector `use#N`, where N is its one-based position among sibling `use` declarations; its import target MUST NOT form part of the selector. Other items without a suitable name MUST have a kind and positional disambiguator. Selector spelling MUST be documented and reversible when names or types contain delimiter characters.
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


## References as authoring inputs

:::{req} authoring_inputs
When authoring evidence, the edges defined by
{cite}`requirements.references.targets` MUST determine the reference
contributions to the mandatory input set defined by
{cite}`requirements.authoring.mandatory_inputs`. The target of each edge leaving
the assessed subject MUST contribute; sources with only edges into the subject
MUST NOT contribute and remain discoverable under
{cite}`requirements.authoring.reference_index`. A `{cite}` made by the subject
contributes the cited definition's complete fenced declaration. An anchor under
{cite}`requirements.references.anchors` contributes the complete anchored file.
An in-source `{impl}` or `{test}` reference to the subject contributes the source
containing it: for a reference in a Rust item, the complete smallest enclosing item with its attached
comments and attributes; for a reference in a Markdown requirement, example, or
terminology declaration, that complete declaration; for other references, the
complete containing source file. Literal references, evidence copies, and
generated output MUST remain excluded under the existing reference-discovery
rules.

These contribution rules MUST apply only to edges directly leaving the assessed
subject. Including another declaration or source MUST NOT by itself recursively
expand that source's own edges. Reference links MUST NOT be described as proof
that all substantively relevant sources have been found.
:::

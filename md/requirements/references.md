# References and Rust items

:::{req} targets
Project text files MAY use the `{impl}`, `{cite}`, and `{test}` roles with a backtick-delimited full requirement or example name, or with a built-in name under {cite}`requirements.references.builtin_definitions`. A reference MAY instead be written in fenced form, `:::{role} name` on one line, a body, and a closing fence, following the fence rules of {cite}`requirements.ci.span` and {cite}`requirements.ci.fence_lengths`; the fenced form is the same edge with attached commentary, and the inline form carries none. In a source comment, fence lines MUST be recognized after removing each line's comment delimiters within one comment run under {cite}`requirements.references.outline_comments`, and a fence that does not close within its comment run MUST be a validation error. Each reference is a directed edge between its source and the named definition. A source is a project source item or a requirement or example declaration. `{cite}` MUST mean that the source relies on, supports, or refers to the definition, and is an edge from the source to the definition. `{impl}` MUST mean that the source implements part of the definition: it makes a decision, performs a check, produces an output, or handles a case that the definition specifies. A requirement MAY implement part of another requirement by refining one of its obligations. Calling code that implements a definition, or producing a result a caller uses to meet it, MUST NOT by itself count as implementing it. A requirement stating a convention for every output of a kind, such as every validation error, is implemented by the source that carries out the convention as a whole, such as the step that collects and reports those outputs. `{test}` MUST mean that the source is designed to verify behavior the definition specifies: a test through its scenario and the assertions about that scenario's outcome, or a named example through the concrete case it describes. `{impl}` and `{test}` are edges from the definition to the source. Only requirements and examples are assessed; code is reviewed as part of the definitions it implements or tests. Habeas MUST find these references throughout project source files and MUST fail with the source file and line if a name has no matching `{req}` or `{example}` definition. In Markdown, escaped roles and roles in code examples MUST remain literal. Evidence snapshots and generated build output MUST NOT be counted as additional source references: discovery MUST skip `.git` and `target` directories at any depth and the project-root `book` and `evidence` directories.

A source item MUST choose its references for each candidate definition by this
procedure, and MAY carry several:

1. If the item is a test or test helper designed to verify the definition, it
   MUST use `{test}`.
2. Otherwise, if fulfilling the definition depends on the item, it MUST use
   `{impl}`. For code, this means it makes a decision, performs a check,
   produces an output, or handles a case the definition specifies. For a type,
   this means the definition constrains its shape: fields, variants, serialized
   form, or validation.
3. Otherwise, if the item should be re-checked or kept in sync when the
   definition changes, it MUST use `{cite}`.
4. If the item carries no reference after applying steps 1 to 3 to every
   definition, it MUST cite the built-in `habeas:none` under
   {cite}`requirements.references.no_requirement`, justifying why the item
   exists.

```text
{impl}`requirements.references.targets`
{cite}`requirements.references.targets`
{test}`requirements.references.targets`
```
:::

:::{req} builtin_definitions
Habeas MUST provide built-in definitions named `habeas:<name>`, available in
every project without a declaration. Each built-in MUST have fixed text, which
states its obligations and is documented in Habeas's reference documentation, and
MUST state which reference roles may use it. References MUST resolve built-in
names alongside project definitions. So that every book stands alone, the book
MUST render a reference to a built-in as a badge without a link whose hover text
gives the built-in's full text. Built-ins MUST NOT require or permit evidence
files and MUST NOT appear in the requirements index, `habeas status`, or the
missing-evidence check of {cite}`requirements.ci.coverage`. A project definition
MUST NOT use a full name beginning with `habeas:`; validation MUST report one,
and the `:` character already makes such names invalid under
{cite}`requirements.ci.identifiers`. Validation MUST report a reference to a
built-in in a role the built-in does not permit, or that breaks the built-in's
stated form.
:::

:::{req} no_requirement
Habeas MUST provide the built-in `habeas:none` under
{cite}`requirements.references.builtin_definitions`, with this text:

> Cite this only when no requirement or example applies to the item under the
> reference procedure. The citation MUST use the fenced form, and its body MUST
> justify why the item exists. An item with an invariant worth checking, such as
> a cache that must not change results, needs a requirement stating that
> invariant instead. An item citing this MUST NOT implement end-user-observable
> behavior that no requirement specifies.

Only `{cite}` MAY reference it. Validation MUST report an inline citation of it
or a fenced citation whose body, without comment delimiters and other
references, is empty. Whether the justification holds MUST be assessed under
{cite}`requirements.references.citation_review`. `habeas status --all` MUST list
each `habeas:none` citation with its item and justification.
:::

:::{req} citation_review
Every source item outside the book that carries at least one `{cite}` reference
MUST have a citation review: an assessment of that item's citations, recorded at
`evidence/citations/<file>/<selector>.json`, where `<file>` is the item's
project-relative path, `<selector>` is its outline selector with every `/` and
`%` percent-encoded, and the root node uses `_root`. The review MUST use the
prompt defined by {cite}`requirements.authoring.citation_prompt`. The reviewer MUST assess, for each citation,
whether the item should be re-checked or kept in sync when the cited definition
changes, and whether the reference procedure of
{cite}`requirements.references.targets` calls for `{impl}` or `{test}` instead;
for `habeas:none`, whether its justification is true, no definition applies, and
the item implements no unspecified end-user-observable behavior. Mandatory inputs,
under {cite}`requirements.authoring.mandatory_inputs`, MUST be the item's outline
node, each cited declaration or built-in text, and the citations' commentary. The record MUST contain the item's file and selector,
its citations by role, name, and commentary, `determination`, `rationale`, and
`inputs`, with determinations under {cite}`requirements.ci.outcomes`. Validation
MUST require a current, accepted review for every such item and report stale,
missing, or orphaned reviews; a review MUST become stale when the item's hash,
a cited declaration, or its citations or their commentary change. Authoring with
`--all` MUST include citation reviews. A citation in a requirement or example
declaration is reviewed as part of that definition's own assessment instead.
:::

:::{req} anchors
:impl: assets/skills/habeas-requirements/SKILL.md
A `{req}` declaration MAY list `:impl:` and `:test:` options, and an `{example}`
declaration MAY list `:test:` options, immediately after its opening fence and
before its body, one per option line, naming a project file path or
`path#selector` for one outline node under
{cite}`requirements.references.outline_selectors`. Each anchor MUST be an `{impl}` or `{test}` edge, under
{cite}`requirements.references.targets`, from the definition to the anchored file or
node, for files that cannot or should not carry a reference themselves.
Paths MUST follow the path rules in {cite}`requirements.ci.schema`. Option lines
MUST NOT form part of the definition's text for evidence comparison or for
requirements inputs, and MUST NOT render as body text.

An anchored file or node MUST be covered by the definition's evidence under
{cite}`requirements.references.evidence_coverage`, by an input selecting it, so changing the file makes that evidence stale. A path or selector that
does not resolve to a readable, permitted file or node MUST be a validation error naming the
definition and the path; authoring MUST report it before inference, because the
definition cannot be assessed without it. Any other option line in a `{req}` or
`{example}` declaration MUST be a validation error.

Agents SHOULD NOT anchor a file when a more precise reference is available, such
as a reference attached to the Rust item that implements or tests the
definition. Habeas's bundled agent skills MUST give this direction.
:::

:::{req} book_badges
:impl: md/habeas.css
In a book chapter, each valid `{impl}`, `{cite}`, or `{test}` reference, inline or fenced, MUST render as a Shields.io static badge image with meaningful alt text that displays its role and full name and links to the badge at the corresponding `{req}` or `{example}` definition. Links MUST work across chapters, including nested chapters. A fenced reference MUST render its badge followed by its body as Markdown. Each fenced `{req}` or `{example}` directive MUST render as a Shields.io definition badge image identifying its kind; text outside a directive MUST remain literal. Badge text MUST remain legible regardless of whether its link has been visited. A badge link MUST show a visible indicator when it has keyboard focus and MAY brighten on hover. Badges MAY be laid out inline with the surrounding text, a fenced reference's badge and body MAY be visually set apart from the surrounding text as one block, and a built-in's unlinked badge MAY show a help cursor to signal its hover text. These badges use the syntax defined by {cite}`requirements.references.targets`.
:::

:::{req} evidence_coverage
For every `{impl}` and `{test}` reference in a project source file, including one in a requirement or example declaration, the evidence file for the referenced requirement or example MUST contain an input whose resolved source ranges cover the reference's file and line. A `{cite}` reference MUST NOT require coverage; it is discoverable through {cite}`requirements.authoring.reference_index`. Habeas MUST report every uncovered reference with its role, full name, source file, and line number.
:::

:::{req} rust_item_coverage
During repository validation, Habeas MUST parse every first-party Rust source file in the project: every `.rs` file except those under `.git` or `target` directories at any depth or under the project-root `book`, `evidence`, or `vendor` directories. Every explicitly declared function or method, including trait methods and test helpers, and every struct, enum, and union MUST have a valid `{impl}`, `{cite}`, or `{test}` reference to a defined requirement or named example, or a valid citation of the built-in `habeas:none` under {cite}`requirements.references.no_requirement`, in a Rust comment attached to it or within its body. Comments belong to items under {cite}`requirements.references.outline_comments`; a comment before a field, variant, or attribute inside an item belongs to that item. A named example MUST reference a requirement under {cite}`requirements.examples.definition`. A reference in a string literal, on an enclosing module or impl block, or only in an evidence input MUST NOT count for the items it encloses. Habeas MUST report every unreferenced item with its file, line, kind, and name, and MUST report a Rust parse failure instead of silently skipping the file.
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
The Rust outline profile under {cite}`requirements.references.outlines` MUST
identify items by segments describing their syntactic ancestry within the file,
such as `fn[run]`, `struct[Packet]`, and `mod[local]/fn[run]`, and MUST NOT
resolve Rust modules across files. Functions, methods, structs, enums, unions,
traits, impls, modules, macros, constants, statics, type aliases, and extern
blocks MUST be outline nodes. Impl segments MUST be named by their syntactic self
type and optional trait path, as `impl[Thing]` and `impl[Thing/Trait]`. `use`
declarations and other items without a suitable name MUST be anonymous. Selector
spelling MUST be documented.
:::

## Outlines

:::{req} outlines
Habeas MUST parse Rust, Python, TypeScript, CSS, Markdown, TOML, and YAML
sources with pinned tree-sitter grammars, selected by file extension compared
case-insensitively. A source with any syntax error MUST fail to parse, reporting
the line of its first error. Every
other text file MUST use a plain-text profile in which each line that is not
blank is an anonymous outline node of kind `line`. Each supported language MUST
have an outline profile that maps grammar node kinds to short outline kind names
and, for each mapped kind, states whether a node is named, and by which fields
or children, or anonymous. Only mapped kinds MUST appear in the outline; other
nodes MUST be transparent, so their outline descendants appear under the
nearest outline ancestor. A profile MAY attach grammar siblings to an outline
node, such as Rust attributes and doc comments preceding an item, so the outline
tree need not match the grammar tree exactly; attached siblings are part of that
node for its line range and hash. Every file's outline MUST have a root node
for the whole file, independent of language. Source in no other outline node,
such as top-level statements no profile maps, MUST belong to the root node;
profiles MUST NOT invent fallback kinds for it. Comments MUST NOT be outline nodes. Profiles MUST be
documented. Upgrading a grammar or profile MAY change outlines and MUST be
treated as a deliberate change.
:::

:::{req} outline_selectors
An outline selector MUST be a `/`-separated path of segments from the file's
root node to an outline node; the empty selector MUST select the root node. A
named node's segment MUST be `kind[name]`. An anonymous node's segment MUST be
`kind@hash`, where `hash` is the first twelve hexadecimal digits of its identity
hash under {cite}`requirements.references.structural_hash`. Siblings whose
segments would otherwise be identical MUST be disambiguated by `#N`, their
one-based position among those siblings in source order, except `line` nodes:
identical lines are interchangeable, so a `line` selector MUST resolve when any
line with that content exists in the file. In names, `%`, `[`, `]`, `/`, `#`,
`@`, and whitespace MUST be percent-encoded as UTF-8 bytes, so selectors are
reversible. Line numbers MUST NOT form part of a selector.
:::

:::{req} structural_hash
A node's structural hash MUST be the SHA-256 digest of an unambiguous encoding of
its outline-relevant structure, including siblings its profile attaches to it:
the kinds of its named grammar nodes and, in source order, its text pieces. A
text piece MUST be the text of a leaf token, or text inside a node that no child
token covers. Each text piece MUST be hashed as its ordered list of words, where
words are separated by whitespace, the whitespace itself, including leading and
trailing whitespace, is never hashed, and each word is encoded separately, so
`ab cd` and `abcd` differ. The same rule MUST apply in every language. Comments
MUST be excluded, except comment kinds the profile includes; the Rust profile
MUST include doc comments. For an included comment, only its text MUST be hashed,
not its delimiter tokens such as `//`, `///`, `//!`, `/*`, `*/`, or a leading `*`,
so reflowing it across more or fewer lines does not change the hash. A `line`
node's hash MUST be computed by the same word rule over the line's text. A node's
identity hash MUST be computed the same way but MUST also exclude its outline
descendants and their attached siblings, so editing a descendant does not change
its ancestors' selectors.
:::

:::{req} outline_comments
Each documented profile defines which grammar nodes are comments; the Markdown
profile's comments are HTML comment blocks, so an inline HTML comment within a
paragraph is text of its section. In every supported language, comments on consecutive lines with no other source
between them MUST form one comment run. A comment that follows other source on
the same line, including another comment, MUST form a run by itself and MUST belong to the innermost outline node
containing that line. Any other run MUST belong to the next outline node that
starts after it within the smallest node containing it; when that container has
no such node, the run MUST belong to the container, which is the root node at top
level. A profile MAY bind a comment kind to its container
instead: such a run MUST belong to the smallest node containing it. The Rust
profile MUST bind inner doc comments (`//!`, `/*!`) to their container, so a
file-level inner doc comment belongs to the root node. A reference in a comment MUST count for the node its run belongs to under
{cite}`requirements.references.rust_item_coverage` and
{cite}`requirements.references.authoring_inputs`.
:::


:::{req} markdown_literals
In Markdown, escaped roles and role-looking text in inline code, fenced code, or raw HTML MUST remain literal and MUST NOT count as references. Raw HTML means an HTML block, an HTML comment, or an inline HTML tag including its attributes; text between inline tags is ordinary Markdown. A `{req}` directive inside a fenced code example and a legacy inline `{req}` role MUST NOT define a requirement.
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
For consecutive sibling declarations `use alpha::A;` and `use beta::B;`, each selector is an anonymous `use@<hash>` segment derived from that declaration's structure. Inserting a declaration before them leaves both selectors unchanged; changing an import target gives that declaration a new selector. A named declaration such as `struct Packet;` instead has selector `struct[Packet]`. {cite}`requirements.references.rust_item_selectors` {cite}`requirements.references.outline_selectors`
:::

:::{example} selector_raw_lines
For a function preceded by a doc comment, an attribute, and an attached citation comment, an outline input's displayed range starts at the earliest attached line and ends at the function's final line. Inserting lines before the function changes the displayed range without changing the input. Editing the citation comment leaves its hash unchanged; editing the doc comment changes it. {cite}`requirements.ci.outline_inputs` {cite}`requirements.references.structural_hash`
:::

:::{example} selector_same_line
For `impl Thing { fn run() {} }` on one line, `habeas add-input --for-requirement requirements.foo --item src/lib.rs:1` adds separate inputs for `impl[Thing]` and `impl[Thing]/fn[run]`, each with its own structural hash. {cite}`requirements.cli.add_input_item`
:::

:::{example} definition_glob_cases
Given a requirement `foo.a.bar` and a named example `foo.a.b.bar`, `foo.*.bar` matches both, in full-name order: `foo.a.b.bar` before `foo.a.bar`. A requirements input stores both full names and their complete raw fenced declarations. Adding another matching definition invalidates the existing input. {cite}`requirements.references.definition_globs` {cite}`requirements.ci.requirements_inputs`
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
containing it: for a reference in a supported language, the smallest enclosing
outline node under {cite}`requirements.references.outline_comments`; for a reference in a Markdown requirement, example, or
terminology declaration, that complete declaration; for other references, the
complete containing source file. Literal references, evidence copies, and
generated output MUST remain excluded under the existing reference-discovery
rules.

These contribution rules MUST apply only to edges directly leaving the assessed
subject. Including another declaration or source MUST NOT by itself recursively
expand that source's own edges. Reference links MUST NOT be described as proof
that all substantively relevant sources have been found.
:::

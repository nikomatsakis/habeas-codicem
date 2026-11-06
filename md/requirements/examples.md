# Named examples

:::{req} definition
An mdBook chapter MAY define an example with an opening fence of at least three colons immediately followed by `{example} <name>` and a closing colon-only fence of at least the opening length. The full name MUST use the same chapter-derived namespace and valid-name rules as `{req}`. The body MUST include all Markdown between the fences, excluding surrounding whitespace, and MUST describe a concrete case with expected behavior. An example MUST contain at least one valid `{cite}` or `{test}` reference to a requirement it illustrates; `{cite}` makes the requirement an input to the example's assessment, and `{test}` makes the example an input to the requirement's assessment, under {cite}`requirements.references.targets`. An example MAY use both. An example directive inside a fenced code block MUST remain literal.
:::

:::{req} evidence
Every defined example MUST have an `evidence/<full-example-name>.json` file whose `example` string exactly matches the current example body. A true determination MUST have at least one `{test}` reference to the example, covered by an evidence input. The reviewer MUST assess its tests under {cite}`requirements.ci.review_criteria` and use the outcomes defined by {cite}`requirements.ci.outcomes`; the no-test justification for a requirement MUST NOT establish a true example judgment. Habeas MUST apply the same missing-evidence, determination, input validation, and reference-coverage checks used for requirements. It MUST report a true determination with no test reference and changed example text with a compact line-oriented diff.
:::

:::{req} index
When a `{requirements-index}` chapter exists, it MUST list examples as well as requirements, identify each entry's kind, and show each example's evidence state. Example detail pages and their links MUST follow {cite}`requirements.index.detail_pages` and {cite}`requirements.index.navigation`; references to an example MUST link to its definition.
:::

## Examples as authoring inputs

:::{req} authoring_inputs
When authoring evidence for a named example, its complete fenced declaration
MUST be included in the mandatory input set defined by
{cite}`requirements.authoring.mandatory_inputs`.
:::

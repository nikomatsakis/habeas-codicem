# Named definitions

:::{req} declaration
An mdBook chapter MAY declare a named definition using a fenced `{definition}` directive. Definitions MUST use the same chapter-qualified names, identifier rules, fence rules, and nonempty-body rules as requirements and examples. Names MUST be unique across all three kinds.

Each definition MUST declare exactly one nonempty `:phrase:` option before its explanatory body. The phrase MUST be interpreted as plain text. Definitions MUST NOT require their own evidence records or determinations.
:::

:::{req} references
The `{term}` role followed by a backtick-delimited full definition name MUST reference a named definition. In rendered Markdown, the reference MUST display the definition's phrase as an ordinary hyperlink to its anchored declaration, without a badge. The phrase MUST be escaped appropriately for rendering.

Missing targets and targets of another kind MUST be reported. Escaped references and references in Markdown code examples MUST remain literal. A term reference MUST NOT satisfy a requirement citation or a Rust function's requirement-coverage obligation.
:::

:::{req} inputs
Evidence inputs MAY have a definition form containing exactly `definition` and `text`. `definition` MUST identify one named definition; `text` MUST contain its complete raw fenced declaration as source lines, including its phrase option and explanatory body.

Validation MUST reject missing definitions and changed snapshot text, identifying the definition and showing a compact diff when available. Moving an unchanged definition within its chapter MUST NOT invalidate its snapshot. Evidence displays MUST show the definition name, current source location, and recorded text.
:::

:::{req} dependencies
Requirement and example evidence MUST include a definition input for every term referenced by its subject text or reviewed inputs.

Habeas MUST repeatedly include definitions referenced by required definitions until the set reaches a fixed point. Each required definition MUST have exactly one definition input per evidence record. Cyclic references MUST be permitted and MUST NOT cause repeated expansion or nontermination.

Validation MUST report missing required definition inputs and MUST NOT silently refresh snapshots.
:::

:::{req} selection
A {term}`requirements.references.definition_selection` in a requirements input MUST continue to select requirements and named examples, excluding named definitions. Definitions referenced by selected texts MUST be captured as separate definition inputs, regardless of whether their names match the selection. Sharing a selection's matching name prefix alone MUST NOT make a definition a dependency.
:::

:::{req} capture
When `habeas add-input` creates or refreshes evidence, it MUST also capture or refresh the definition inputs required by the resulting record, including dependencies of its subject text and all reviewed inputs.

Repeated references MUST NOT create duplicate definition inputs. Any change to the record MUST leave its determination false pending reassessment. If no inputs or other record contents change, the record MUST remain unchanged.
:::

:::{example} phrase_rendering
A definition named `glossary.first_parent_history` declares the phrase "first-parent history". A term reference to that name renders the phrase as an ordinary hyperlink to the declaration, without displaying the identifier or a badge. {cite}`requirements.definitions.references`
:::

:::{example} transitive_cycle
Requirement R references definition A. A references B, and B references A. R's evidence contains one definition input for A and one for B. Dependency collection terminates. Changing B's phrase or body invalidates R's snapshot of B. {cite}`requirements.definitions.dependencies` {cite}`requirements.definitions.inputs`
:::

:::{example} glob_dependency
A requirements input selects `requirements.cli.*`. A selected requirement references `glossary.command`, which is captured as a separate definition input despite not matching that selection. An unreferenced definition named `requirements.cli.unrelated` is not selected merely because its name matches the selection. {cite}`requirements.definitions.selection`
:::

:::{example} reassessment
An evidence record has determination true and a current definition input. After the definition changes, validation reports stale evidence without modifying the record. Running `habeas add-input` for that record refreshes its required definition input and leaves determination false pending review. {cite}`requirements.definitions.inputs` {cite}`requirements.definitions.capture`
:::

## Terminology as authoring inputs

:::{req} authoring_inputs
When preparing the mandatory input set defined by
{cite}`requirements.authoring.mandatory_inputs`, every terminology definition
referenced by the subject or any contributed source MUST contribute its complete
fenced declaration, including its phrase option. Definitions referenced by those
definitions MUST also be included transitively, without duplicates or
nontermination on cycles.

When an additional source read introduces a terminology dependency, the same
complete, transitive definition set MUST be supplied to the model and recorded
before accepting a submission.
:::

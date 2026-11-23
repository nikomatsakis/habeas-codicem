# Plain-text outlines

Use plain-text outlines to select individual lines from a file whose extension
is not claimed by a supplied language.

## Use the fallback language

:::{req} language
`languages::PlainText` implements `OutlineLang` under
{cite}`requirements.sdk.outlines.language_provider`. Its default value is
cheaply cloneable and claims no extensions. The filename-based constructors
select it when no supplied language claims the extension, including filenames
without an extension, under {cite}`requirements.sdk.outlines.defaults`.
:::

## Select a nonblank line

:::{req} items
A plain-text outline has a root for the whole file and one anonymous `line`
child per nonblank line, in source order. Blank lines create no child nodes.
Each line's selector is `line@<identity-hash-prefix>` under
{cite}`requirements.references.outline_selectors`. The prefix derives from its
contents rather than its line number. Identical lines are interchangeable for
selection.
:::

For this file, the root has two children:

```text
Tokens expire.

Reject expired tokens.
```

A review can select “Reject expired tokens.” independently of “Tokens expire.”
Adding an unrelated line preserves the existing lines' selectors.

## Fingerprint line contents

:::{req} fingerprints
Each line's structural and identity hashes are the SHA-256 digest of its
whitespace-separated words, using the unambiguous encoding under
{cite}`requirements.references.structural_hash`. Whitespace within a line does
not affect its hash; word boundaries remain significant.

The root hash encodes the nonblank lines in source order, including their line
boundaries. Inserting blank lines preserves it. Reordering distinct nonblank
lines or joining them changes the root's structural encoding.
The scheme identifies these rules and their version under
{cite}`requirements.sdk.outlines.language_provider.fingerprint_scheme`.
:::

Plain text supports outlining and text search. Structural search reports an
unsupported-language error under
{cite}`requirements.sdk.outlines.language_provider.structural_search`.

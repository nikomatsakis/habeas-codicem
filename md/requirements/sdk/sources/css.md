# CSS outlines

Select CSS source items by their declared names and nesting. The outline
supplies their source text and structural fingerprints.

## Choose the language

:::{req} language
`languages::Css` implements `OutlineLang` under
{cite}`requirements.sdk.outlines.language_provider`. Its default value is
cheaply cloneable and claims `.css`. Filename-based constructors select it under
{cite}`requirements.sdk.outlines.defaults`.
:::

## Select source items

:::{req} items
The CSS outline exposes the following items beneath its file root:

| Construct | Kind | Name |
| --- | --- | --- |
| Rule set | `rule` | Whitespace-normalized selector list |
| Media or supports statement | `media` or `supports` | Whitespace-normalized prelude |
| Keyframes | `keyframes` | Animation name |

Nested rules appear beneath their nearest enclosing outline item.

Item selectors, percent encoding, anonymous identities, and duplicate-name
suffixes follow {cite}`requirements.references.outline_selectors`.
:::

This source contains `rule[.token]`:

```css
.token { color: red; }
```

## Fingerprint the contents

:::{req} fingerprints
Structural hashes use SHA-256 over the unambiguous structural encoding and
whitespace-separated words under {cite}`requirements.references.structural_hash`.
The fingerprint scheme identifies these rules and their version under
{cite}`requirements.sdk.outlines.language_provider.fingerprint_scheme`.

CSS comments are excluded from structural hashes.
:::

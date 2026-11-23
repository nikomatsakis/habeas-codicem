# Python outlines

Select Python source items by their declared names and nesting. The outline
supplies their source text and structural fingerprints.

## Choose the language

:::{req} language
`languages::Python` implements `OutlineLang` under
{cite}`requirements.sdk.outlines.language_provider`. Its default value is
cheaply cloneable and claims `.py` and `.pyi`.
Filename-based constructors select it under
{cite}`requirements.sdk.outlines.defaults`.
:::

## Select source items

:::{req} items
The Python outline exposes the following items beneath its file root:

| Construct | Kind | Name |
| --- | --- | --- |
| Functions and methods | `def` | Declared name |
| Classes | `class` | Declared name |

Nested definitions appear beneath their nearest enclosing outline item.
Decorators attach to the definition they decorate and are included in its
selected source and structural hash.

Item selectors, percent encoding, anonymous identities, and duplicate-name
suffixes follow {cite}`requirements.references.outline_selectors`.
:::

This source contains `class[Tokens]/def[check_token]`:

```python
class Tokens:
    def check_token(self):
        return True
```

## Fingerprint the contents

:::{req} fingerprints
Structural hashes use SHA-256 over the unambiguous structural encoding and
whitespace-separated words under {cite}`requirements.references.structural_hash`.
The fingerprint scheme identifies these rules and their version under
{cite}`requirements.sdk.outlines.language_provider.fingerprint_scheme`.

Ordinary comments are excluded. Docstrings are source expressions and
participate in hashes along with the definition's other expressions.
:::

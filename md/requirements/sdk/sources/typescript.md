# TypeScript and TSX outlines

Select TypeScript and TSX source items by their declared names and nesting.
The outline supplies their source text and structural fingerprints.

## Choose the language

:::{req} language
`languages::TypeScript` and `languages::Tsx` implement `OutlineLang` under
{cite}`requirements.sdk.outlines.language_provider`. Their default values are
cheaply cloneable language configurations. They claim `.ts`, `.mts`, and `.cts`
for TypeScript, and `.tsx` for TSX;
filename-based constructors select them under
{cite}`requirements.sdk.outlines.defaults`.
:::

## Select source items

:::{req} items
The TypeScript and TSX outlines expose the following items beneath their file roots:

| Construct | Kind | Name |
| --- | --- | --- |
| Functions, generators, and function signatures | `function` | Declared name |
| Classes and abstract classes | `class` | Declared name |
| Methods and method signatures | `method` | Declared name |
| Interfaces | `interface` | Declared name |
| Type aliases | `type` | Declared name |
| Enums | `enum` | Declared name |
| Namespaces and modules | `namespace` | Declared name |

TypeScript and TSX use the same outline kinds. Nested items appear beneath
their nearest enclosing outline item.

Item selectors, percent encoding, anonymous identities, and duplicate-name
suffixes follow {cite}`requirements.references.outline_selectors`.
:::

This source contains `function[checkToken]`:

```typescript
function checkToken(): boolean { return true; }
```

## Fingerprint the contents

:::{req} fingerprints
Structural hashes use SHA-256 over the unambiguous structural encoding and
whitespace-separated words under {cite}`requirements.references.structural_hash`.
The fingerprint scheme identifies these rules and their version under
{cite}`requirements.sdk.outlines.language_provider.fingerprint_scheme`.

Comments, including documentation comments, are excluded from structural
hashes. The remaining source structure and text participate in hashing.
:::

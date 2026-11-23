# YAML outlines

Select YAML source items by their declared names and nesting. The outline
supplies their source text and structural fingerprints.

## Choose the language

:::{req} language
`languages::Yaml` implements `OutlineLang` under
{cite}`requirements.sdk.outlines.language_provider`. Its default value is
cheaply cloneable and claims `.yml` and `.yaml`.
Filename-based constructors select it under
{cite}`requirements.sdk.outlines.defaults`.
:::

## Select source items

:::{req} items
The YAML outline exposes the following items beneath its file root:

| Construct | Kind | Name |
| --- | --- | --- |
| Block and flow mapping entries | `pair` | Written key |
| Block sequence entries | `item` | Anonymous |

Nested entries appear beneath their nearest enclosing outline item.
Selected source ends at the last non-comment token. Comment ownership follows
{cite}`requirements.references.outline_comments`.

Item selectors, percent encoding, anonymous identities, and duplicate-name
suffixes follow {cite}`requirements.references.outline_selectors`.
:::

This source contains `pair[auth]/pair[expires]`:

```yaml
auth:
  expires: true
```

## Fingerprint the contents

:::{req} fingerprints
Structural hashes use SHA-256 over the unambiguous structural encoding and
whitespace-separated words under {cite}`requirements.references.structural_hash`.
The fingerprint scheme identifies these rules and their version under
{cite}`requirements.sdk.outlines.language_provider.fingerprint_scheme`.

Comments are excluded from structural hashes.
:::

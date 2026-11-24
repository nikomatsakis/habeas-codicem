# TOML outlines

Select TOML source items by their declared names and nesting. The outline
supplies their source text and structural fingerprints.

## Choose the language

:::{req} language
`languages::Toml` implements `OutlineLang` under
{cite}`requirements.sdk.outlines.language_provider`. Its default value is
cheaply cloneable and claims `.toml`. Filename-based constructors select it under
{cite}`requirements.sdk.outlines.defaults`.
:::

:::{req} Toml
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

The `Toml` implementation of `OutlineLang` supplies the extension, selection, and fingerprint rules described in this chapter.

```rust,ignore
pub struct Toml;
```

Trait implementations

```rust,ignore
impl Clone for Toml;
impl Debug for Toml;
impl Default for Toml;
impl OutlineLang for Toml;
impl Copy for Toml;
```
:::

## Select source items

:::{req} items
The TOML outline exposes the following items beneath its file root:

| Construct | Kind | Name |
| --- | --- | --- |
| Tables and array tables | `table` | Written header key |
| Key/value entries | `pair` | Written key |

Entries appear beneath their nearest containing outline item, including
entries nested in inline tables. Names preserve bare, quoted, and dotted keys
as written, rather than normalizing them to semantic values.

Item selectors, percent encoding, anonymous identities, and duplicate-name
suffixes follow {cite}`requirements.references.outline_selectors`.
:::

This source contains `table[auth]/pair[expires]`:

```toml
[auth]
expires = true
```

## Fingerprint the contents

:::{req} fingerprints
Structural hashes use SHA-256 over the unambiguous structural encoding and
whitespace-separated words under {cite}`requirements.references.structural_hash`.
The fingerprint scheme identifies these rules and their version under
{cite}`requirements.sdk.outlines.language_provider.fingerprint_scheme`.

Comments are excluded from structural hashes.
:::

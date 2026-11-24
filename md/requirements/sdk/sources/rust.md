# Rust outlines

Select Rust source items by their declared names and nesting. The outline
supplies their source text and structural fingerprints.

## Choose the language

:::{req} language
`languages::Rust` implements `OutlineLang` under
{cite}`requirements.sdk.outlines.language_provider`. Its default value is
cheaply cloneable and claims `.rs`. Filename-based constructors select it under
{cite}`requirements.sdk.outlines.defaults`.
:::

:::{req} Rust
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

The `Rust` implementation of `OutlineLang` supplies the extension, selection, and fingerprint rules described in this chapter.

```rust,ignore
pub struct Rust;
```

Trait implementations

```rust,ignore
impl Clone for Rust;
impl Debug for Rust;
impl Default for Rust;
impl OutlineLang for Rust;
impl Copy for Rust;
```
:::

## Select source items

:::{req} items
The Rust outline exposes the following items beneath its file root:

| Construct | Kind | Name |
| --- | --- | --- |
| Functions, methods, and function signatures | `fn` | Declared name |
| Structs, enums, unions, and traits | `struct`, `enum`, `union`, `trait` | Declared name |
| Implementations | `impl` | Written self type and optional trait path |
| Modules | `mod` | Declared name |
| Macro definitions | `macro` | Declared name |
| Macro invocations in item position | `macro` | Anonymous |
| Constants and statics | `const`, `static` | Declared name |
| Type aliases and associated types | `type` | Declared name |
| Foreign blocks | `extern` | ABI string, or `Rust` when absent |
| Extern-crate declarations | `extern-crate` | Declared name |
| Imports | `use` | Anonymous |

An implementation of `Trait` for `Thing` uses `impl[Thing/Trait]`;
an inherent implementation uses `impl[Thing]`. Nesting follows source
containment within the file. Outer attributes and outer documentation comments
attach to the following item; inner documentation comments belong to their
containing item or file root.

Item selectors, percent encoding, anonymous identities, and duplicate-name
suffixes follow {cite}`requirements.references.outline_selectors`.
:::

This source contains `mod[auth]/fn[check_token]`:

```rust
mod auth {
    /// Validate the token.
    pub fn check_token() -> bool { true }
}
```

## Inspect attachments

:::{req} attachments
Rust reports line and block comments, including outer and inner documentation
comments, as `Comment`. An outer attribute (`#[…]`) attached to an outline
item is an `Annotation` on that item. These are the two kinds Rust reports.
For example, a function preceded by `/// Verify the token.` and `#[inline]`
owns a `Comment` followed by an `Annotation`. A file-level `//!` comment
belongs to the file root.
:::

## Fingerprint the contents

:::{req} fingerprints
Structural hashes use SHA-256 over the unambiguous structural encoding and
whitespace-separated words under {cite}`requirements.references.structural_hash`.
The fingerprint scheme identifies these rules and their version under
{cite}`requirements.sdk.outlines.language_provider.fingerprint_scheme`.

Documentation comments participate in structural hashes. Ordinary comments
are excluded. Included comment prose is grouped as inner or outer documentation;
delimiters are excluded, and consecutive comments of one group hash as one word
sequence. Reflowing that prose preserves its hash; changing its words changes it.
Attributes attached to an item participate in that item's hash.
:::

```rust,ignore
use trust_but_verify::domain::NodeContents;
use trust_but_verify::outline::OutlineContent;

let select = |source: &str| {
    OutlineContent::from_source("auth.rs", source.into())?
        .select("fn[check_token]")
};

let original = select("fn check_token() { allow(); }")?;
let formatted = select("fn check_token() {\n    allow();\n}")?;
let changed = select("fn check_token() { deny(); }")?;

assert_eq!(original.fingerprint(), formatted.fingerprint());
assert_ne!(original.fingerprint(), changed.fingerprint());
```

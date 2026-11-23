# Rust outline definition

Consider this source:

```rust
fn check_token() -> bool { true }
```

The default `habeas-sdk-outline` parser includes this language implementation
and selects it for `.rs` files.

The {term}`requirements.sdk.outlines.selector` `fn[check_token]` identifies the item in this example.
The resource address identifies the file separately from this selector.

## Outline rules

The function's declared name becomes `fn[check_token]`. Other mapped items
include structs, enums, unions, traits, implementations, modules, macros,
constants, statics, types, foreign blocks, imports, and extern-crate declarations.
Nesting follows syntax within a file; it does not follow modules into other files.

## Comments and fingerprints

Attributes and outer documentation comments attach to items. Documentation
comments participate in fingerprints; ordinary comments do not. Inner
documentation comments belong to the enclosing container.

The [shared source machinery](../sources.md) defines selector encoding, duplicate
names, and the distinction between node contents and structural fingerprints.

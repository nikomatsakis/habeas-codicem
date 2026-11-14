# Rust outline definition

Consider this source:

```rust
fn check_token() -> bool { true }
```

The `habeas-language-rust` crate supplies this outline definition. The default
registry selects it for `rs` files.

The selector `fn[check_token]` identifies the item in this example.
The resource address identifies the file separately from this selector.

## Outline rules

Functions use `fn`; other mapped items include structs, enums, unions, traits, implementations, modules, macros, constants, statics, types, foreign blocks, imports, and extern-crate declarations. Nesting follows the syntax within a file; it does not follow modules into other files.

## Comments and fingerprints

Attributes and outer documentation comments attach to items. Documentation comments participate in fingerprints; ordinary comments do not. Inner documentation comments belong to the enclosing container.

The [shared source machinery](../sources.md) defines selector encoding, duplicate
names, and the distinction between node contents and structural fingerprints.
These rules describe the current provider; existing normative contracts remain
in [References and Rust items](../../references.md) pending the
[requirements reorganization](../requirements-map.md).

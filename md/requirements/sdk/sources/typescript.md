# TypeScript and TSX outline definition

Consider this source:

```typescript
function checkToken(): boolean { return true; }
```

The `habeas-language-typescript` crate supplies this outline definition. The default
registry selects it for `ts, mts, cts; tsx` files.

The selector `function[checkToken]` identifies the item in this example.
The resource address identifies the file separately from this selector.

## Outline rules

Functions, generators, and function signatures use `function`; classes use `class`; methods and method signatures use `method`. Interfaces, type aliases, enums, and namespaces have corresponding outline kinds. TSX uses a separate grammar and provider with the same outline rules.

## Comments and fingerprints

Comments, including documentation comments, do not participate in structural fingerprints.

The [shared source machinery](../sources.md) defines selector encoding, duplicate
names, and the distinction between node contents and structural fingerprints.
These rules describe the current provider; existing normative contracts remain
in [References and Rust items](../../references.md) pending the
[requirements reorganization](../requirements-map.md).

# TypeScript and TSX outline definition

Consider this source:

```typescript
function checkToken(): boolean { return true; }
```

The `habeas-language-typescript` crate supplies this outline definition. The default
registry selects TypeScript for `.ts`, `.mts`, and `.cts` files and TSX for `.tsx`.

The {term}`requirements.sdk.sources.selector` `function[checkToken]` identifies the item in this example.
The resource address identifies the file separately from this selector.

## Outline rules

The declared function name supplies `function[checkToken]`. Generators and
function signatures also use `function`; classes use `class`, and methods and
method signatures use `method`. Interfaces, type aliases, enums, and namespaces
have corresponding kinds. TSX has its own grammar and provider with the same
outline rules.

## Comments and fingerprints

Comments, including documentation comments, do not participate in structural
fingerprints.

The [shared source machinery](../sources.md) defines selector encoding, duplicate
names, and the distinction between node contents and structural fingerprints.
These rules describe the current provider. Their existing requirements live
in [References and Rust items](../../references.md) pending the
[requirements reorganization](../requirements-map.md).

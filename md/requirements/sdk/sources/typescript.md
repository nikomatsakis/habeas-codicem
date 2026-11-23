# TypeScript and TSX outline definition

Consider this source:

```typescript
function checkToken(): boolean { return true; }
```

The default `habeas-sdk-outline` parser includes this language implementation
and selects TypeScript for `.ts`, `.mts`, and `.cts` files and TSX for `.tsx`.

The {term}`requirements.sdk.sources.selector` `function[checkToken]` identifies the item in this example.
The resource address identifies the file separately from this selector.

## Outline rules

The declared function name supplies `function[checkToken]`. Generators and
function signatures also use `function`; classes use `class`, and methods and
method signatures use `method`. Interfaces, type aliases, enums, and namespaces
have corresponding kinds. TSX uses the same outline rules.

## Comments and fingerprints

Comments, including documentation comments, do not participate in structural
fingerprints.

The [shared source machinery](../sources.md) defines selector encoding, duplicate
names, and the distinction between node contents and structural fingerprints.

# TOML outline definition

Consider this source:

```toml
[auth]
expires = true
```

The `habeas-language-toml` crate supplies this outline definition. The default
registry selects it for `toml` files.

The selector `table[auth]/pair[expires]` identifies the nested entry in this example.
The resource address identifies the file separately from this selector.

## Outline rules

Tables and array tables use `table`. Key/value entries use `pair`. Names preserve the written bare, quoted, or dotted key rather than normalizing keys to their semantic values.

## Comments and fingerprints

Comments are excluded from structural fingerprints.

The [shared source machinery](../sources.md) defines selector encoding, duplicate
names, and the distinction between node contents and structural fingerprints.
These rules describe the current provider; existing normative contracts remain
in [References and Rust items](../../references.md) pending the
[requirements reorganization](../requirements-map.md).

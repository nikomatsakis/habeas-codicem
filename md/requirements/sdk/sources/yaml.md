# YAML outline definition

Consider this source:

```yaml
auth:
  expires: true
```

The `habeas-language-yaml` crate supplies this outline definition. The default
registry selects it for `.yml` and `.yaml` files.

The selector `pair[auth]/pair[expires]` identifies the nested entry in this example.
The resource address identifies the file separately from this selector.

## Outline rules

The written keys supply `pair[auth]` and its child `pair[expires]`. Block sequence
entries have anonymous `item` nodes. The engine looks through unmapped syntax
to find these outline nodes.

## Comments and fingerprints

Comments are excluded from structural fingerprints. A trailing comment is
outside the preceding outline item.

The [shared source machinery](../sources.md) defines selector encoding, duplicate
names, and the distinction between node contents and structural fingerprints.
These rules describe the current provider. Their existing requirements live
in [References and Rust items](../../references.md) pending the
[requirements reorganization](../requirements-map.md).

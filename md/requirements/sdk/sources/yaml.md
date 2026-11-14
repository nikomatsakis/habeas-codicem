# YAML outline definition

Consider this source:

```yaml
auth:
  expires: true
```

The `habeas-language-yaml` crate supplies this outline definition. The default
registry selects it for `yml, yaml` files.

The selector `pair[auth]/pair[expires]` identifies the nested entry in this example.
The resource address identifies the file separately from this selector.

## Outline rules

Mapping pairs use `pair`, named by their written key. Block sequence entries use anonymous `item` nodes. Unmapped syntax is traversed to find these outline nodes.

## Comments and fingerprints

Comments are excluded from structural fingerprints. Trailing comments are treated as outside the preceding outline item.

The [shared source machinery](../sources.md) defines selector encoding, duplicate
names, and the distinction between node contents and structural fingerprints.
These rules describe the current provider; existing normative contracts remain
in [References and Rust items](../../references.md) pending the
[requirements reorganization](../requirements-map.md).

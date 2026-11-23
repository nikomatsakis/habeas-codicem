# TOML outline definition

Consider this source:

```toml
[auth]
expires = true
```

The default `habeas-sdk-outline` parser includes this language implementation
and selects it for `.toml` files.

The {term}`requirements.sdk.outlines.selector` `table[auth]/pair[expires]` identifies the nested entry in this example.
The resource address identifies the file separately from this selector.

## Outline rules

The table header supplies `table[auth]`, and its `expires` entry supplies the
child `pair[expires]`. Array tables also use `table`. Names preserve the written
bare, quoted, or dotted key rather than normalizing keys to their semantic values.

## Comments and fingerprints

Comments are excluded from structural fingerprints.

The [shared source machinery](../sources.md) defines selector encoding, duplicate
names, and the distinction between node contents and structural fingerprints.

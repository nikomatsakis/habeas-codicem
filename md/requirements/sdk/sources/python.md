# Python outline definition

Consider this source:

```python
def check_token():
    return True
```

The `habeas-language-python` crate supplies this outline definition. The default
registry selects it for `.py` and `.pyi` files.

The selector `def[check_token]` identifies the item in this example.
The resource address identifies the file separately from this selector.

## Outline rules

The function's declared name becomes `def[check_token]`. Class definitions use
`class` with their declared name. Both take names from the grammar's name field,
and decorators attach to the definitions they decorate.

## Comments and fingerprints

Ordinary comments do not participate in structural fingerprints. Docstrings
are source expressions, so they remain part of the parsed content.

The [shared source machinery](../sources.md) defines selector encoding, duplicate
names, and the distinction between node contents and structural fingerprints.
These rules describe the current provider. Their existing requirements live
in [References and Rust items](../../references.md) pending the
[requirements reorganization](../requirements-map.md).

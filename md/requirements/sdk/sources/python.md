# Python outline definition

Consider this source:

```python
def check_token():
    return True
```

The `habeas-language-python` crate supplies this outline definition. The default
registry selects it for `py, pyi` files.

The selector `def[check_token]` identifies the item in this example.
The resource address identifies the file separately from this selector.

## Outline rules

Function definitions use `def` and class definitions use `class`. Their names come from the grammar’s name field. Decorators attach to their definitions.

## Comments and fingerprints

Ordinary comments do not participate in structural fingerprints. Docstrings are source expressions, so they remain part of the parsed content.

The [shared source machinery](../sources.md) defines selector encoding, duplicate
names, and the distinction between node contents and structural fingerprints.
These rules describe the current provider; existing normative contracts remain
in [References and Rust items](../../references.md) pending the
[requirements reorganization](../requirements-map.md).

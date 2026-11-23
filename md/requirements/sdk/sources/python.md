# Python outline definition

Consider this source:

```python
def check_token():
    return True
```

The default `habeas-sdk-outline` parser includes this language implementation
and selects it for `.py` and `.pyi` files.

The {term}`requirements.sdk.sources.selector` `def[check_token]` identifies the item in this example.
The resource address identifies the file separately from this selector.

## Outline rules

The function's declared name becomes `def[check_token]`. Class definitions use
`class` with their declared name. Decorators attach to the definitions they
decorate.

## Comments and fingerprints

Ordinary comments do not participate in structural fingerprints. Docstrings
are source expressions, so they remain part of the parsed content.

The [shared source machinery](../sources.md) defines selector encoding, duplicate
names, and the distinction between node contents and structural fingerprints.

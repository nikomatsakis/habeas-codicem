# CSS outline definition

Consider this source:

```css
.token { color: red; }
```

The `habeas-language-css` crate supplies this outline definition. The default
registry selects it for `css` files.

The selector `rule[.token]` identifies the item in this example.
The resource address identifies the file separately from this selector.

## Outline rules

Rule sets use `rule`, named by their normalized selectors. Media and supports statements use `media` and `supports`, named by their normalized preludes. Keyframes use `keyframes`, named by the animation name.

## Comments and fingerprints

CSS comments are excluded from structural fingerprints.

The [shared source machinery](../sources.md) defines selector encoding, duplicate
names, and the distinction between node contents and structural fingerprints.
These rules describe the current provider; existing normative contracts remain
in [References and Rust items](../../references.md) pending the
[requirements reorganization](../requirements-map.md).

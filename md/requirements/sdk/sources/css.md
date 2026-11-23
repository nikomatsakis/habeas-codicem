# CSS outline definition

Consider this source:

```css
.token { color: red; }
```

The default `habeas-sdk-outline` parser includes this language implementation
and selects it for `css` files.

The {term}`requirements.sdk.sources.selector` `rule[.token]` identifies the item in this example.
The resource address identifies the file separately from this selector.

## Outline rules

The selector's kind describes the CSS construct; its name comes from the
selector or prelude:

| Construct | Outline kind | Name |
| --- | --- | --- |
| Rule set | `rule` | Normalized selector, such as `.token` |
| Media or supports statement | `media` or `supports` | Normalized prelude |
| Keyframes | `keyframes` | Animation name |

## Comments and fingerprints

CSS comments are excluded from structural fingerprints.

The [shared source machinery](../sources.md) defines selector encoding, duplicate
names, and the distinction between node contents and structural fingerprints.

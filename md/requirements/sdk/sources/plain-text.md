# Plain-text fallback

When no filename extension matches a language, the default outline parser
selects its plain-text implementation and outlines nonblank lines. For example:

```text
Tokens expire.
Reject expired tokens.
```

Each nonblank line is an anonymous `line` item. In this example, a review can
select “Reject expired tokens.” independently of “Tokens expire.” Its
{term}`requirements.sdk.outlines.selector`
is `line@<identity-hash-prefix>`, with the prefix derived from contents rather
than a line number. Identical lines are interchangeable for selection.

This fallback is a `Language` implementation in `habeas-sdk-outline`.
The default parser selects it automatically when the
filename has no matching language extension.

The [shared outline model](../sources.md) explains how selectors become source
node addresses. A plain-text node can supply contents and be validated just like
any other node. Structural search is unsupported for plain text.

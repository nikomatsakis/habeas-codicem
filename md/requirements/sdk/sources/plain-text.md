# Plain-text fallback

When no language profile applies, the SDK can outline nonblank lines. For example:

```text
Tokens expire.
Reject expired tokens.
```

Each nonblank line is an anonymous `line` item, addressed as
`line@<identity-hash-prefix>`. The prefix is derived from its content, not its
line number. Identical lines are interchangeable for selection.

This fallback lives in the SDK's outline module. It has no tree-sitter grammar
and is not an implementation of the grammar-bearing `LanguageProvider` trait.
Applications can choose it when resolving a resource with no matching provider.

The [shared outline model](../sources.md) explains how selectors become source
node addresses. A plain-text node can supply contents and be validated just like
any other node; it does not offer grammar-based structural search.

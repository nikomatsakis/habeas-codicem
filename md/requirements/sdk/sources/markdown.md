# Markdown outline definition

Consider this source, which combines a paragraph, a directive, and a literal
code example:

`````markdown
# Authentication

Tokens expire.

::::{note}
Reject expired tokens.

:::{tip}
Check the boundary too.
:::
::::

```text
hi
```
`````

The section is named `section[Authentication]`. Its paragraph and fenced blocks
need their own outline identities so that a review can select one explanation
without selecting the entire section. Paragraphs are anonymous children, as are
the fenced blocks in this proposal. Each uses the shared content-derived
identity scheme rather than a line number.

The proposed outline has this shape; the letters stand in for hash prefixes:

```text
section[Authentication]
  paragraph@a                 Tokens expire.
  block@b                     ::::{note} ... ::::
    paragraph@c               Reject expired tokens.
    block@d                   :::{tip} ... :::
      paragraph@e             Check the boundary too.
  block@f                     ```text ... ```
```

The `habeas-language-markdown` crate currently outlines sections only. The
paragraph and block contracts below describe the extension being reviewed;
they are not yet implemented. The default registry selects this language for
`.md` and `.markdown` resources.

## Paragraphs are selectable inputs

A paragraph such as “Tokens expire.” has no declared name. Its identity is
computed using the shared anonymous-item rules. Paragraphs also occur within
lists, quotations, and directive bodies. Their outline parent is the nearest
enclosing outline container; syntax that has no outline node is transparent.

:::{req} paragraphs
The Markdown outline MUST expose each prose paragraph as an anonymous
`paragraph` node under its nearest enclosing outline container, including
paragraphs before the first heading and inside list items, quotations, and
Markdown directive bodies. Paragraphs MUST use the shared anonymous-selector
rules under {cite}`requirements.references.outline_selectors`. Heading text
and literal code-block contents MUST NOT create paragraph nodes.
:::

## Fences create containers

The outer note in the example uses four colons so that it can contain a
three-colon tip. The inner closing fence ends the tip without ending the note.
This follows MyST's convention of using longer outer fences for nesting.
[MyST syntax overview](https://mystmd.org/guide/syntax-overview).

A literal code block is also an outline node, but its contents are code rather
than Markdown children. Thus `hi` in the example belongs to the code block;
it does not create a paragraph node. A code example containing `:::{req}` must
remain an example, rather than introduce a nested requirement container.

:::{req} fenced_blocks
The Markdown outline MUST expose colon-fenced directive containers and fenced
code blocks as anonymous `block` nodes using the shared anonymous-selector
rules under {cite}`requirements.references.outline_selectors`. A block MUST
include its opening fence, body, and closing fence when present, and MUST be
nested under its nearest enclosing outline container.
:::

:::{req} fenced_blocks.contents
Markdown content in colon-fenced directive bodies MUST be outlined
recursively. Contents of ordinary fenced code blocks MUST remain literal
and MUST NOT create nested paragraph or directive nodes.
:::

Colon fences use at least three colons, including longer runs such as `::::::`.
A closing line contains only a colon run and optional whitespace, with at least
as many colons as its opener. Backtick and tilde code fences retain their
CommonMark delimiter rules. In particular, shorter delimiter runs inside a code
block cannot end it, and apparent directives inside it remain literal.
[MyST colon-fence rules](https://myst-parser.readthedocs.io/en/stable/syntax/optional.html#code-fences-using-colons).

:::{req} fence_precedence
Markdown outline parsing MUST recognize colon directive fences of three or
more colons and support nested directives with shorter inner fences. A
colon-only closing fence MUST have at least as many colons as the opener it
closes. Ordinary backtick and tilde code fences MUST follow CommonMark fence
rules and shield their contents from colon-directive recognition. Fence-like
text in inline code or raw HTML MUST NOT introduce outline directive blocks.
:::

These outline boundaries supply the containers needed for citation ownership.
An inline citation in ordinary prose belongs to its paragraph; inside a
requirement or other directive it belongs to the nearest enclosing directive
block. Choosing that citation owner is Habeas policy, separate from exposing
paragraphs and blocks in the Markdown outline.

## Sections and comments

Sections use `section`, named from their heading text. ATX and setext headings
are supported. Content before a heading can form an anonymous section.

HTML comment blocks are recognized as comments and excluded from structural
fingerprints. Inline HTML comments within paragraph text are not classified as
comment blocks.

The [shared source machinery](../sources.md) defines selector encoding, duplicate
names, and the distinction between node contents and structural fingerprints.
Existing normative contracts for sections and comment handling remain in
[References and Rust items](../../references.md) pending the
[requirements reorganization](../requirements-map.md).

## Fence behavior to settle before implementation

For an unclosed colon fence, the proposed behavior is to retain a block through
the end of its enclosing parsing context. Habeas can still diagnose a missing
requirement terminator independently. The alternative is to reject the outline
entirely; this choice is not yet part of the normative contract.

This proposal uses MyST's colon nesting convention, rather than implementing
all MyST directive semantics. Backtick fences, including those whose info string
starts with `{note}` or another directive name, would remain literal blocks in
this increment. Interpreting backtick directives would be a separate extension.

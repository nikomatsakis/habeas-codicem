# Markdown outlines

Select Markdown sections, paragraphs, directives, and code blocks. Each item
has its own outline identity, source text, and structural fingerprint.

## Choose Markdown

:::{req} language
`languages::Markdown` implements `OutlineLang` under
{cite}`requirements.sdk.outlines.language_provider`. Its default value is
cheaply cloneable and claims `.md` and `.markdown`.
Filename-based constructors select it under
{cite}`requirements.sdk.outlines.defaults`.
:::

:::{req} Markdown
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

The `Markdown` implementation of `OutlineLang` supplies the extension, selection, and fingerprint rules described in this chapter.

```rust,ignore
pub struct Markdown;
```

Trait implementations

```rust,ignore
impl Clone for Markdown;
impl Debug for Markdown;
impl Default for Markdown;
impl OutlineLang for Markdown;
impl Copy for Markdown;
```
:::

## Select a section

:::{req} sections
Sections use `section`, named from whitespace-normalized heading text. ATX and
setext headings are supported. Content before the first heading can form an
anonymous section. Section nesting follows heading levels; other outline items
appear beneath their nearest enclosing container. Selectors follow
{cite}`requirements.references.outline_selectors`.
:::

## Select a paragraph

:::{req} paragraphs
A paragraph has no declared name. Each prose paragraph has an anonymous
`paragraph` node under its nearest
enclosing outline container. This includes paragraphs before the first heading
and inside list items, quotations, and directive bodies. Their selectors follow
{cite}`requirements.references.outline_selectors`. Heading text and literal
code-block contents create no paragraph nodes. Syntax that has no outline node
is transparent when finding the enclosing container.
:::

## Select a fenced block

:::{req} fenced_blocks
Colon-fenced directives and fenced code blocks have anonymous `block` nodes
with selectors under {cite}`requirements.references.outline_selectors`.
A block contains its opening fence, body, and closing fence when present.
Its parent is the nearest enclosing outline container.
An unclosed colon fence retains its block through the end of the enclosing
file or directive body, excluding the enclosing directive's closing fence.
:::

:::{req} fenced_blocks.contents
Colon-fenced directive bodies are outlined recursively as Markdown. Ordinary
fenced code-block contents remain literal, creating no nested paragraph or
directive nodes. Backtick fences with a directive name such as `{note}` in
their info string are literal code blocks.
:::

:::{req} fence_precedence
Colon directive fences use three or more colons and support nested directives
with shorter inner fences. A colon-only closing fence has at least as many
colons as its opener, followed only by optional whitespace.
Backtick and tilde code fences follow CommonMark rules and shield their contents
from colon-directive recognition. Fence-like text in inline code or raw HTML
introduces no outline directive blocks.
:::

For this source, the four-colon note contains a three-colon tip. The inner
closing fence ends the tip without ending the note. The final code block
contains literal text:

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

The outline has this shape; the letters stand in for hash prefixes:

```text
section[Authentication]
  paragraph@a                 Tokens expire.
  block@b                     ::::{note} ... ::::
    paragraph@c               Reject expired tokens.
    block@d                   :::{tip} ... :::
      paragraph@e             Check the boundary too.
  block@f                     ```text ... ```
```

## Inspect attachments

:::{req} attachments
Markdown reports only `Comment` attachments. Each HTML comment block whose
trimmed text starts with `<!--` and ends with `-->` is one attachment.
An inline HTML comment within a paragraph remains paragraph text, and
comment delimiters inside fenced code remain code.
:::

## Fingerprint the contents

:::{req} fingerprints
Structural hashes use SHA-256 over the unambiguous structural encoding and
whitespace-separated words under {cite}`requirements.references.structural_hash`.
The fingerprint scheme identifies these rules and their version under
{cite}`requirements.sdk.outlines.language_provider.fingerprint_scheme`.

HTML comment blocks are excluded from structural hashes. Inline HTML comments
within paragraph text remain ordinary paragraph text. Paragraph and directive
prose uses word-based hashing. Reflowing whitespace within the same paragraph,
while preserving its words and surrounding outline structure, preserves the
hash. Changing its words changes the structural encoding.
:::

# Syntax details

## Full names in nested chapters

A requirement's full name comes from its chapter path relative to the book's
source directory. In nested chapters, omit a leading `reqs/` and a trailing
`/index` or `/README`: `md/reqs/cli/index.md` gives `cli.<name>`.

## Anchors

When a file that implements or tests a requirement cannot carry a reference,
such as a shipped skill or a stylesheet copied into other projects, anchor it
from the declaration with `:impl: <path>` or `:test: <path>` option lines
directly after the opening fence (examples accept only `:test:`). Use
`:impl: <path>#<selector>` for one outline node, an item such as a function or
a Markdown section that a selector names, such as
`:impl: skills/foo/SKILL.md#section[Workflow]`; `habeas info` shows each input's
selector. Don't anchor a file whose items can carry a reference themselves.

# Distributed requirements

Some obligations apply on their own at each of many sites, such as a convention
every validation function follows. Marking every site `{impl}` spreads the
requirement across the codebase and asks one review to judge them all. A
distributed requirement instead declares queries that find its sites, and
Habeas reviews each site separately.

## Queries

A requirement becomes distributed by containing query directives, each with one
fenced `yaml` block and a name unique within the requirement:

- `:::{sites} <name>` selects the places the obligation applies.
- `:::{violations} <name>` selects constructs likely to break it, such as `?`,
  `return`, `continue`, or `break` in a function that must keep going after a
  failed check. Each needs `within: <site query>`, naming a site query of the same
  requirement; only matches inside that query's sites count. Violation queries
  point site reviews at likely problems; each site's review still judges the
  whole site, and a violation query that misses a construct does not by itself
  make the requirement false.

Each body has these keys: `language` (`rust`, `python`, `typescript`, `tsx`,
`css`, `markdown`, `toml`, or `yaml`); optional `paths` and `exclude`, lists of
project-relative files or directories; an ast-grep `rule`, with optional `utils`
and `constraints`; `within`, required for violation queries; and `match` and
`no_match` snippet lists. Give the requirement a longer outer fence:

````text
::::{req} errors
A validation function that collects validation errors MUST hold them in a
parameter or local variable named `errors`, and a function with such a variable
is a site. After one of its checks fails, a site MUST still run every later check
that does not depend on the failed check's result.

:::{sites} error_accumulators
```yaml
language: rust
paths: [src]
rule:
  kind: function_item
  has:
    stopBy: { kind: function_item }
    any:
      - kind: parameter
        has: { field: pattern, regex: "^errors$" }
      - kind: let_declaration
        has: { field: pattern, regex: "^errors$" }
match:
  - "fn check(value: &str, errors: &mut Vec<String>) {}"
  - |
    fn run() {
        let mut errors = Vec::new();
        check("", &mut errors);
    }
no_match:
  - "fn check(value: &str, problems: &mut Vec<String>) {}"
  - |
    fn run() {
        let mut problems = Vec::new();
        check("", &mut problems);
    }
```
:::

:::{violations} early_exits
```yaml
language: rust
paths: [src]
within: error_accumulators
rule:
  any:
    - kind: try_expression
    - kind: return_expression
match:
  - |
    fn check(v: &Value, errors: &mut Vec<String>) -> Option<u8> {
        let n = v.as_u64()?;
        if v.is_null() { errors.push("null".into()); }
        Some(n as u8)
    }
  - |
    fn check(v: &Value, errors: &mut Vec<String>) {
        if !v.is_object() { errors.push("object".into()); return; }
        if v.is_null() { errors.push("null".into()); }
    }
no_match:
  - |
    fn parse(v: &Value) -> Option<u8> {
        let n = v.as_u64()?;
        Some(n as u8)
    }
  - |
    fn check(v: &Value, errors: &mut Vec<String>) {
        if v.is_null() { errors.push("null".into()); }
    }
```
:::
::::
````

Each site-query match makes a site: the innermost outline node containing the
match, such as the enclosing function. A violation match is reported to the site
that contains it.

## Do

- Make a requirement distributed only when its obligation applies on its own at
  each site; when particular code carries out the requirement as a whole, use
  `{impl}` there.
- Describe sites exactly as the site queries select them. When a query relies on
  a convention, such as how a variable is named, state the convention as an
  obligation, so code that breaks it is a real gap you fix in the code.
- Separate obligations each site upholds from obligations particular code
  carries out, and mark that code `{impl}`.
- State the requirement's scope in its text, and use `paths` and `exclude` for
  scope a rule cannot see, such as authoring code outside validation.
- Give every query a `match` snippet for each alternative way it selects, and a
  `no_match` near miss for each selection condition: a construct that differs
  from a match only in failing that condition, such as the right type with the
  wrong name. Each `has`, `inside`, and `not` clause, each regex, and a violation query's `within` is a selection condition needing a near miss; each `any` alternative needs its own `match` snippet instead. This applies to violation queries too.
- Write a violation query, with `within` naming the site query, for every kind
  of construct that could break the obligation, such as each early exit in a
  function that must keep going.
- Before review, check that the site queries select every place the obligation
  applies and no others: think about places they could miss, such as a list
  held in a local variable or under another name, and places they select that
  the obligation does not cover. Widen or narrow the query, or state the
  convention as an obligation so a miss becomes a code gap. The requirement's
  review searches for both.
- Present the queries and snippets with the requirement text when reviewing it
  with the user.
- Check with `habeas info <full-name>` that the sites are the ones you expect.

## Don't

- Don't describe sites more broadly than the queries select, as in "however it is
  named".
- Don't state a per-site obligation that a reviewer can check only by reading
  other sites, such as "not reported by another check"; narrow it to the site,
  such as "does not report one failure twice".
- Don't reference the distributed requirement from a site just because it is a
  site; sites need none. Being a site also does not satisfy Rust item coverage,
  so each site still needs its own references for what it implements, and a
  site that carries out one of the requirement's other obligations, such as the
  function that reports the collected errors, gets `{impl}` of it like any
  other implementing code.

## Gotchas

- Patterns parse as standalone code, so a type or parameter fragment needs
  ast-grep's `context` and `selector` form, such as
  `{ context: "fn f(x: &mut Vec<String>) {}", selector: reference_type }`. A plain
  pattern that parses differently silently matches nothing; the snippets catch
  this.
- Each snippet parses as a whole file, so a Rust snippet must be complete items,
  not a bare expression.
- `cargo test --workspace` runs every snippet and reports a wrong result, and a
  site query that matches nothing in the project.

## Review

`habeas author --all` authors a site review for each site, judging the per-site
obligations and each violation match in it. The requirement's own review checks
the Do list above. To fix a false site review, see "Site reviews" in the
habeas-evidence skill.

# Citation and site reviews

## Citation reviews

Every source item outside the book with a `{cite}` other than `habeas:util`
gets a citation review. For project requirements and examples it checks that
the citation should not be `{impl}` or `{test}`. For built-ins it checks their
permitted forms and stated criteria. A `habeas:coverage` citation needs a
substantive justification for incomplete execution coverage, in fenced
commentary or a Rust doc comment; acknowledging a gap is insufficient.
Fix a false review by changing the references, justification, or item.

A closed `:::{cite} full.name` fence outside a book requirement or example also
gets a review when it contains a resolved citation other than `habeas:util`.
This `book_commentary` review checks that the passage's claims agree with and
are supported by the cited declarations. The outermost eligible fence groups
nested commentary citations into one review; nested requirements and examples
keep their own assessment. Ordinary inline book citations do not opt in.
These passages use the same citation evidence, freshness checks, and
`habeas author --all` workflow as source-item reviews.

## Site reviews

A distributed requirement (the habeas skill's
`references/distributed-requirements.md`) gets a site review for each site,
authored by `habeas author --all` or, for one requirement,
`habeas author sites:<full-name>`. A site the requirement does not apply to is
judged true, so a false review means the reviewer found the requirement applies
and is broken:

1. If it does apply, fix the code at the site.
2. If the site should not be covered, narrow the site query or tighten the text
   describing sites; if the obligation's text admits a reading under which the
   site fails, tighten that text. Both edit the requirement, so take them
   through habeas-requirements steps 3 to 5 (subagent review, the user's agreement, commit).

If `habeas info <full-name>` reports many sites the requirement does not apply
to, the site query selects too much.

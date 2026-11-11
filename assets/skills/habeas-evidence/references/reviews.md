# Citation and site reviews

## Citation reviews

Every source item outside the book with a `{cite}` other than `habeas:util`
gets a citation review, which checks that each citation is sensible and should
not be `{impl}` or `{test}`. Fix a false review by changing the references or
the item.

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

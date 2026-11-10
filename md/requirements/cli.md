# Command-line interface

:::{req} clap
The `habeas` CLI MUST use `clap` to define and parse its commands and options.
:::

:::{req} mdbook
The `mdbook` subcommand MUST act as the mdBook preprocessor for HTML books, rendering requirement definitions and linked reference badges. This project's `book.toml` MUST configure `cargo habeas mdbook` as the `habeas` preprocessor, using a repository-local Cargo alias for `cargo run --release -p habeas --`. Consumer books MUST continue to invoke the installed `habeas mdbook` CLI directly.
:::

:::{req} setup
Running `habeas setup` in an existing Cargo and mdBook project MUST add Habeas as a dev dependency, configure `habeas mdbook` as its preprocessor, add the badge stylesheet, enable sidebar folding with `output.html.fold.enable` unless the book already sets it, and create `tests/habeas.rs` that calls `habeas::validate()`. Repeating setup MUST avoid duplicate configuration and MUST NOT overwrite a differing test or stylesheet. Setup MUST NOT install or modify agent skills.
:::

:::{req} status
`habeas status [<selection>] [--all] [--verbose]` MUST consider requirements and named examples matched by the {term}`requirements.references.definition_selection`; without a selection it MUST consider every definition. By default, it MUST list matching definitions in full-name order when their evidence is missing, has a false determination, or fails the same evidence and reference checks as `habeas::validate()`. It MUST summarize FIXME judgments under {cite}`requirements.cli.fixme_summary`, with accepted FIXME subjects listed individually only when `--verbose` or `--all` is selected. `--all` MUST list every matching definition with its kind and evidence state. Every form MUST show validation findings, including repository-wide Rust function coverage findings, work from within a Cargo and mdBook project, and report invalid arguments or a supplied selection matching no definition clearly. When files that reference discovery does not consider because they are untracked in a Git worktree contain references, `habeas status` MUST list them with a reminder to stage them with `git add`. Every form MUST also list, in a "Disagreements" section, each selected definition, citation review, and site review whose recorded rationale states that its assessments disagreed under {cite}`requirements.authoring.confirmation`, so those findings are visible although validation accepts them.
:::

:::{req} fixme_summary
`habeas status` MUST group the selected requirement and example judgments marked
FIXME by issue reference and report how many distinct judgments each issue
affects. A recorded FIXME string of the form `owner/repo#N` under
{cite}`requirements.ci.outcomes` MUST identify that issue, compared exactly; any
other string is a gap description without an issue.

A judgment MUST count at most once for each issue, even when that issue occurs
repeatedly or in several FIXME strings. A judgment naming several issues MUST
count in each corresponding group. Judgments with no issue string MUST be counted once in a separate "FIXME without an issue" group.
Issue groups MUST be ordered by decreasing affected-judgment count, then by
reference; the group without an issue MUST follow them. A supplied
selection MUST restrict these counts to the selected subjects.

`--verbose` MUST list the full name and kind of every judgment in each issue
group and in the group without an issue, along with its original FIXME strings.
The summary MUST include valid FIXME determinations even when other evidence
checks fail; grouping MUST NOT hide their validation findings. Malformed
determinations MUST remain validation errors under {cite}`requirements.ci.outcomes`.
:::

:::{req} fixme_issues
`habeas status` MUST look up, under {cite}`requirements.authoring.github_reads`,
every issue that a `:github:` option line of a named or unnamed `{fixme}` under
{cite}`requirements.references.roles` names, and MUST report each `{fixme}`
naming an issue that is closed or does not exist as an error, with its file and
line and the issue. It MUST also report the item that contains that `{fixme}` as
failing: the outline node containing it, or the requirement or example
declaration whose body contains it, in the same way it reports an uncovered
Rust item. When GitHub cannot be reached, the issue MUST be assumed open.
Repository validation MUST NOT contact GitHub.
:::

:::{req} status_table_alignment
When `habeas status` or `habeas status --all` displays a Markdown table, it MUST left-align and space-pad the Definition and Evidence columns so every row, including the header and separator row, has its `|` column delimiters in the same positions. Column widths MUST be determined by the widest displayed cell in each column.
:::

:::{req} shared_status_rendering
`habeas status` and the mdBook requirements index MUST generate their Definition/Evidence Markdown tables through the same renderer. That renderer MUST calculate column widths, align the columns, and display the same evidence-state labels in both outputs. Callers MAY select different definitions and supply different kind-labeled name cells so mdBook can link to detail pages.
:::

:::{req} info
:impl: assets/skills/habeas-requirements/SKILL.md
:impl: assets/skills/habeas-evidence/SKILL.md
`habeas info <selection>` MUST show the requirements and named examples matched by the {term}`requirements.references.definition_selection`. It MUST print matches in full-name order as Markdown with the same kind-labeled definition, Validation findings, Rationale, and Inputs structure and evidence states as mdBook detail pages, including readable output for missing or invalid evidence. It MUST work without a requirements index directive and report an invalid selection or no matches clearly. Habeas's bundled agent guidance MUST tell agents they can use this command to inspect a requirement or named example.
:::

:::{req} bootstrap_skill
:impl: assets/skills/habeas-bootstrap-requirements/SKILL.md
Habeas MUST bundle a skill for deriving initial requirements from an existing codebase. The skill MUST direct agents to read the code and tests, map every first-party test to at least one requirement or example, identify and map every source unit implementing end-user-visible functionality or interactions to a requirement, and report gaps rather than claim complete coverage without an independent inventory review.
:::

:::{req} requirements_review_skill
:impl: assets/skills/habeas-requirements/SKILL.md
:impl: assets/skills/habeas-bootstrap-requirements/SKILL.md
:impl: assets/skills/habeas-evidence/SKILL.md
The bundled requirements skill MUST direct agents adding or changing user-visible behavior in a Habeas project to draft stable requirement IDs and exact text, obtain the user's agreement, and install the agreed requirements in the mdBook before writing implementation or test code. It MUST suggest reaching that agreement by reviewing each new or changed requirement with the user in turn: presenting its exact text, identifying ambiguity, obligations that are hard to test or that nothing would enforce, and conflicts with other requirements, and proposing specific improvements for the user to accept, revise, or reject, then applying the user's decisions before moving on. Drafts MAY be uncommitted edits to the book, but MUST NOT be committed or implemented before agreement. It MUST direct agents to review substantive changes to agreed obligations with the user. The bootstrap skill MUST require user review of requirements inferred from existing code before installing them, and MUST suggest the same review in turn. The evidence skill MUST accept missing evidence or truthful false judgments from the harness while implementation is pending; it MUST NOT direct the supervising agent to write placeholder evidence itself.
:::

:::{req} behavior_review_guidance
:impl: assets/skills/habeas-requirements/SKILL.md
:impl: assets/skills/habeas-bootstrap-requirements/SKILL.md
:impl: assets/skills/habeas-evidence/SKILL.md
Habeas's bundled agent skills MUST direct agents reviewing code against requirements to inspect every end-user-observable result and branch, including errors and input handling, and verify both that the code fulfills each applicable requirement and that it introduces no additional end-user-observable behavior without an agreed requirement. They MUST explain the requirement and example review criteria in {cite}`requirements.ci.review_criteria`, including test adequacy and justified absence of tests for requirements, and the three outcomes and documented-gap convention in {cite}`requirements.ci.outcomes`. Agents MUST mark the roots of a requirement's implementation with `{impl}` and the tests that verify it with `{test}`, leaving code reached from those items unmarked, and use `{cite}` only for related code that does neither, under {cite}`requirements.references.procedure`. When validation reports an item uncovered with a coverage-loss review under {cite}`requirements.authoring.coverage_loss`, they MUST add `{impl}` where the review finds a root, and where it finds the item not needed, determine whether the item is dead code or the earlier read was mistaken before changing anything. Where requirements interact, agents MUST mark every requirement implemented at the relevant point and explain how their texts jointly account for the behavior. A citation alone MUST NOT count as that explanation.
:::

:::{req} internal_detail_guidance
:impl: assets/skills/habeas-requirements/SKILL.md
:impl: assets/skills/habeas-bootstrap-requirements/SKILL.md
:impl: assets/skills/habeas-evidence/SKILL.md
Habeas's bundled agent skills MUST direct agents to choose references by the
procedure in {cite}`requirements.references.procedure`. For internal code, they
MUST direct agents to state any invariant worth checking as an ordinary
requirement and implement it, and otherwise to cite the built-in `habeas:none` in the fenced form, with a
body justifying why the item exists, under {cite}`requirements.references.no_requirement`. They MUST
NOT suggest grouping requirements, such as `internal.config_parsing`, that state
no checkable obligation.
:::

:::{req} flip_guidance
:impl: assets/skills/habeas-evidence/SKILL.md
:impl: assets/skills/habeas-requirements/SKILL.md
:impl: assets/skills/habeas-bootstrap-requirements/SKILL.md
Habeas's bundled agent skills MUST state the goal that independent assessments
of the same subject reach the same determination, because the requirement, code,
and tests leave no gap a careful reviewer could find and no reading a careful
reviewer could take differently. They MUST tell agents that assessments of the
same subject that disagree are findings against that goal. They MUST direct agents to read both
rationales and decide whether one assessment found a real gap or the requirement
text admits more than one reading, then fix the source or tests, or tighten the
requirement text, rather than re-running an assessment to obtain a different
determination. Every change to agreed requirement text, including a rewording for precision,
MUST be reviewed with the user under
{cite}`requirements.cli.requirements_review_skill` before it is made, and the
skills MUST then direct agents to record the old text, new text, reason, and the
rationale that prompted the change in the project's audit log, such as
`audits/requirement-rewordings/`.
:::
:::{req} distributed_guidance
:impl: assets/skills/habeas-requirements/SKILL.md
:impl: assets/skills/habeas-requirements/references/distributed-requirements.md
:impl: assets/skills/habeas-evidence/SKILL.md
:impl: assets/skills/habeas-bootstrap-requirements/SKILL.md
Habeas's bundled agent skills MUST explain distributed requirements under
{cite}`requirements.references.distributed`. They MUST direct agents to make a
requirement distributed only when its obligation applies on its own at each of
many sites, such as a convention every validation function follows, and to use
`{impl}` instead when particular code carries out the requirement as a whole, and
to mark with `{impl}` the code that carries out any obligation of a distributed
requirement that its sites do not uphold. They
MUST direct agents to write site queries that select every place the obligation
applies and no others, violation queries for every kind of construct that could
break it, and
`match` and `no_match` snippets for every query that adequately test its conditions. They
MUST direct agents to describe sites exactly as the site queries select them,
stating as an obligation any convention the queries rely on, such as how a list
is named, and to state only per-site obligations that a reviewer can check at the
site and the helpers it calls. They MUST direct agents to
present the queries with the requirement text when reviewing the requirement with
the user. They MUST tell agents that sites need no reference to the requirement,
that `habeas author --all` authors site reviews under
{cite}`requirements.references.site_review`, and that a false site review is
addressed by fixing the site, or by tightening the queries or the requirement
text when the site should not be covered or the text admits more than one
reading, under {cite}`requirements.cli.flip_guidance`, not by re-running the
review. The requirements skill MUST give this guidance as a list of things to do and
things not to do. The bootstrap skill MUST direct agents to propose a distributed
requirement for a cross-cutting convention instead of marking each of its sites
with `{impl}`.
:::

:::{req} skill_structure
:impl: assets/skills/habeas-requirements/SKILL.md
:impl: assets/skills/habeas-requirements/references/distributed-requirements.md
:impl: assets/skills/habeas-evidence/SKILL.md
:impl: assets/skills/habeas-evidence/references/authoring-failures.md
:impl: assets/skills/habeas-requirements/references/syntax-details.md
:impl: assets/skills/habeas-bootstrap-requirements/SKILL.md
Habeas's bundled agent skills MUST jointly give the direction that the
requirements on them specify. Each topic MUST be stated in full in one skill,
its home, and another skill that needs the topic MUST refer to that skill rather
than restate it. The `habeas-requirements` skill MUST be the home of drafting and
agreeing requirements, directive syntax, the reference procedure, the review
criteria, and distributed requirements; the `habeas-evidence` skill MUST be the
home of the authoring workflow, outcomes and FIXME judgments, citation and site
reviews, and disagreeing assessments; and the `habeas-bootstrap-requirements`
skill MUST be the home of deriving an initial requirement set from existing code.
Each skill's description MUST state when to use it in terms of the user's task
and name the other skills for neighboring tasks, and each skill MUST open with
its workflow as ordered steps. Material needed only in specialized situations
MAY live in files the skill references, each with a statement of when to read it.
:::

:::{req} invocation
`habeas mdbook supports html` MUST succeed, and a support query for another renderer MUST fail. An unknown command or invalid setup option combination MUST fail with a usage message. In particular, `habeas agents` and `habeas setup --install-skills` MUST fail with a usage message.
:::

:::{req} setup_project
`habeas setup` MUST find an ancestor containing both `Cargo.toml` and `book.toml`, require a nonempty relative mdBook source path without parent components and a `SUMMARY.md` at that path, and report an error when these prerequisites are absent or invalid.
:::

:::{req} setup_css
`habeas setup` MUST preserve existing entries in `output.html.additional-css` while adding the Habeas stylesheet at most once. It MUST report an error if `additional-css` is not an array.
:::

:::{req} setup_local_path
When given `--path <crate-dir>` and no existing Habeas dev dependency, `habeas setup` MUST add that local crate as the dev dependency. Repeating setup MUST NOT add a duplicate Habeas dependency.
:::

:::{example} definition_glob_search
Given a requirement `foo.ready` and a named example `foo.case`, `habeas info 'foo.*'` displays both, and `habeas status 'foo.*' --all` lists both with their kinds and evidence states. {cite}`requirements.cli.info` {cite}`requirements.cli.status`
:::

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
`habeas status [<selection>] [--all] [--verbose]` MUST consider requirements and named examples matched by the {term}`requirements.references.definition_selection`; without a selection it MUST consider every definition. By default, it MUST list matching definitions in full-name order when their evidence is missing, has a false determination, or fails the same evidence and reference checks as `habeas::validate()`. It MUST summarize FIXME judgments under {cite}`requirements.cli.fixme_summary`, with accepted FIXME subjects listed individually only when `--verbose` or `--all` is selected. `--all` MUST list every matching definition with its kind and evidence state. Every form MUST show validation findings, the coverage report under {cite}`requirements.references.coverage.report`, including its warnings about items without recent coverage, work from within a Cargo and mdBook project, and report invalid arguments or a supplied selection matching no definition clearly. When files that reference discovery does not consider because they are untracked in a Git worktree contain references, `habeas status` MUST list them with a reminder to stage them with `git add`. Every form MUST also list, in a "Disagreements" section, each selected definition, citation review, and site review whose recorded rationale states that its assessments disagreed under {cite}`requirements.authoring.confirmation`, so those findings are visible although validation accepts them.
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
:impl: assets/skills/habeas/SKILL.md
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
The `habeas-requirements` skill MUST direct agents adding, changing, or removing end-user-observable behavior that no agreed requirement already specifies in a Habeas project to draft requirements and commit them, once agreed under {cite}`requirements.cli.requirements_review_skill.agreement` and {cite}`requirements.cli.requirements_review_skill.commit`, before writing implementation or test code.
:::

:::{req} requirements_review_skill.agreed_text
:impl: assets/skills/habeas-requirements/SKILL.md
The `habeas-requirements` skill MUST direct agents to take every change to agreed requirement text, including a rewording for precision and the removal of a requirement, through the same review and agreement before committing it.
:::

:::{req} bootstrap_skill.review
:impl: assets/skills/habeas-bootstrap-requirements/SKILL.md
The `habeas-bootstrap-requirements` skill MUST direct agents to take the requirements they infer from existing code, and the changes they propose to existing requirement text, through the review and agreement of {cite}`requirements.cli.requirements_review_skill` before committing them or tagging code against them. It MUST state that a general request to bootstrap is not agreement to specific text, and MUST direct agents to keep requirements the user rejects or defers on the gap list of {cite}`requirements.cli.bootstrap_skill`. Each area's agreed requirements MUST be committed together before that area's code is tagged.
:::

:::{req} pending_evidence_guidance
:impl: assets/skills/habeas-evidence/SKILL.md
The `habeas-evidence` skill MUST tell agents that missing evidence, or a false determination whose rationale names only implementation that is still pending, is expected for a requirement or example whose implementation is pending, and MUST direct them to report it to the user rather than keep trying to fix it.
:::

:::{req} pending_evidence_guidance.no_placeholder
:impl: assets/skills/habeas-evidence/SKILL.md
The `habeas-evidence` skill MUST tell agents not to write placeholder evidence for a requirement or example whose implementation is pending.
:::

:::{req} overview_skill
:impl: assets/skills/habeas/SKILL.md
Habeas MUST bundle a `habeas` skill whose description tells agents to use it at the start of any work in a Habeas project, naming as signs of one a `habeas` dev-dependency, a `habeas.toml`, `{req}` directives in the book, and `{impl}`, `{test}`, or `{cite}` references in code.
:::

:::{req} overview_skill.routing
:impl: assets/skills/habeas/SKILL.md
The `habeas` skill MUST route each kind of task to the bundled skill that covers it: design sessions and changes to behavior, code, tests, or requirement text, including renaming a code item in place, to `habeas-requirements`; moving code, tests, or requirements to another file, chapter, or outline node, and renaming a requirement, to `habeas-refactor`; creating or refreshing evidence, and fixing the findings that validation or `habeas status` reports, to `habeas-evidence`; and adopting Habeas in an existing codebase to `habeas-bootstrap-requirements`.
:::

:::{req} overview_skill.move_then_edit
:impl: assets/skills/habeas/SKILL.md
The `habeas` skill MUST tell agents that a change which both moves and edits code, tests, or requirements is done as the move first, under `habeas-refactor`, and then the edit, under the skill the edit routes to.
:::

:::{req} overview_skill.setup
:impl: assets/skills/habeas/SKILL.md
The `habeas` skill MUST tell agents that a project without the validation test that {cite}`requirements.cli.setup` creates is not configured for Habeas, to ask the user before running `habeas setup`, and, if setup is not completed, to do no Habeas requirements work in that project.
:::

:::{req} overview_skill.new_project
:impl: assets/skills/habeas/SKILL.md
The `habeas` skill MUST tell agents that a new project with little code starts with `habeas-requirements`, not `habeas-bootstrap-requirements`, even though most of its code has no references.
:::

:::{req} requirements_skill
:impl: assets/skills/habeas-requirements/SKILL.md
Habeas MUST bundle a `habeas-requirements` skill whose description tells agents to use it during design sessions and before changing code, tests, or requirement text in a Habeas project, including when the user asks for a feature, bug fix, or other change without mentioning requirements.
:::

:::{req} requirements_review_skill.no_change
:impl: assets/skills/habeas-requirements/SKILL.md
The `habeas-requirements` skill MUST direct agents, when a change adds, alters, or removes no end-user-observable behavior, to tell the user that no requirement is needed, and when an existing requirement already specifies the behavior the change produces, to tell the user which requirement covers it; in either case it MUST direct them to proceed to implementation without drafting.
:::

:::{req} requirements_review_skill.drafts
:impl: assets/skills/habeas-requirements/SKILL.md
The `habeas-requirements` skill MUST direct agents to write drafts as uncommitted, unstaged edits to the book with no draft marker, adding a new chapter file to `SUMMARY.md` and recording it with `git add -N`.
:::

:::{req} requirements_review_skill.drafts.rough
:impl: assets/skills/habeas-requirements/SKILL.md
The `habeas-requirements` skill MUST tell agents that drafts may be high-level, compound, or incomplete, and that they need not answer every question before review.
:::

:::{req} requirements_review_skill.drafts.foreign_edits
:impl: assets/skills/habeas-requirements/SKILL.md
Before drafting, the `habeas-requirements` skill MUST direct agents to ask the user whether uncommitted edits to the book that the agent did not make in the current task belong to the task.
:::

:::{req} requirements_review_skill.review
:impl: assets/skills/habeas-requirements/SKILL.md
The `habeas-requirements` skill MUST direct agents to put each new or changed requirement into a group, where a group holds requirements that cite each other or describe one feature. It MUST direct them to review every group with its own read-only subagent, with all the reviews running in parallel, before presenting any group to the user.
:::

:::{req} requirements_review_skill.review.inputs
:impl: assets/skills/habeas-requirements/SKILL.md
The `habeas-requirements` skill MUST direct agents to tell each review subagent where to find the review prompt of {cite}`requirements.cli.requirements_review_skill.review_prompt` and the `habeas` skill. It MUST direct them to give each subagent the group's full names and a brief that states the user's request, the decisions already made, and the open choices already noted.
:::

:::{req} requirements_review_skill.review.fallback
:impl: assets/skills/habeas-requirements/SKILL.md
Where subagents are unavailable, the `habeas-requirements` skill MUST direct agents to review each group themselves against the review prompt of {cite}`requirements.cli.requirements_review_skill.review_prompt`, one group at a time, before presenting any group to the user.
:::

:::{req} requirements_review_skill.present
:impl: assets/skills/habeas-requirements/SKILL.md
The `habeas-requirements` skill MUST direct agents to present each group to the user with its exact text, including any `{sites}` and `{violations}` queries, and with each design question the review returned, along with its options and the presenting agent's recommendation.
:::

:::{req} requirements_review_skill.present.apply
:impl: assets/skills/habeas-requirements/SKILL.md
Before presenting, the `habeas-requirements` skill MUST direct agents to apply the revised text the reviews return. Where reviews conflict, for example two reviews that split the same requirement differently, it MUST direct them to reconcile the conflict, and to present as a design question any conflict whose resolution changes what is promised.
:::

:::{req} requirements_review_skill.present.mechanical
:impl: assets/skills/habeas-requirements/SKILL.md
The `habeas-requirements` skill MUST direct agents not to raise mechanical changes to new text as questions. For text that was already agreed or that the user supplied, it MUST direct agents to show the change as a diff against that text, followed by a list of the mechanical changes.
:::

:::{req} requirements_review_skill.present.outside
:impl: assets/skills/habeas-requirements/SKILL.md
The `habeas-requirements` skill MUST direct agents to show the edits a review made to requirements outside its group in the same way as {cite}`requirements.cli.requirements_review_skill.present.mechanical` requires. It MUST also direct them to list every `{example}` a review added and to say that each one needs a test.
:::

:::{req} requirements_review_skill.present.rereview
:impl: assets/skills/habeas-requirements/SKILL.md
The `habeas-requirements` skill MUST direct agents to apply the user's decisions on each group. If a decision adds an obligation that no reviewer saw, the skill MUST direct them to review that group again before asking for agreement.
:::

:::{req} requirements_review_skill.agreement
:impl: assets/skills/habeas-requirements/SKILL.md
The `habeas-requirements` skill MUST direct agents not to commit requirements or write implementation or test code until the user has agreed to, rejected, or deferred every group.
:::

:::{req} requirements_review_skill.agreement.what_counts
:impl: assets/skills/habeas-requirements/SKILL.md
The `habeas-requirements` skill MUST state that exact text the user supplied, or explicit agreement earlier in the conversation, counts as agreement, and that a general request to make the change does not.
:::

:::{req} requirements_review_skill.agreement.no_user
:impl: assets/skills/habeas-requirements/SKILL.md
The `habeas-requirements` skill MUST direct an agent that cannot ask the user in the session to stop, leaving the drafts uncommitted, and report the revised drafts and the design questions.
:::

:::{req} requirements_review_skill.commit
:impl: assets/skills/habeas-requirements/SKILL.md
The `habeas-requirements` skill MUST direct agents, once every group is decided, to commit the agreed requirements of all groups in one commit before any implementation, leaving out uncommitted edits the user said do not belong to the task.
:::

:::{req} requirements_review_skill.commit.revert
:impl: assets/skills/habeas-requirements/SKILL.md
Before that commit, the `habeas-requirements` skill MUST direct agents to revert the drafts the user rejected or deferred and to list them in their report.
:::

:::{req} requirements_review_skill.commit.check
:impl: assets/skills/habeas-requirements/SKILL.md
Before that commit, the `habeas-requirements` skill MUST direct agents to check that the book holds exactly the agreed text.
:::

:::{req} requirements_review_skill.commit.validation
:impl: assets/skills/habeas-requirements/SKILL.md
The `habeas-requirements` skill MUST tell agents that the requirements commit may fail validation until evidence is authored, and that only the pull request as a whole must pass.
:::

:::{req} requirements_review_skill.commit.design_session
:impl: assets/skills/habeas-requirements/SKILL.md
In a design session with no code to write, the `habeas-requirements` skill MUST direct agents to stop after the requirements commit.
:::

:::{req} requirements_review_skill.text_change
:impl: assets/skills/habeas-requirements/SKILL.md
When implementation shows that agreed requirement text must change, the `habeas-requirements` skill MUST direct agents to stop work that depends on the change and take the changed requirements through review, agreement, and commit before continuing.
:::

:::{req} requirements_review_skill.text_change.uncovered
:impl: assets/skills/habeas-requirements/SKILL.md
When implementation shows end-user-observable behavior no requirement covers, the `habeas-requirements` skill MUST direct agents to draft a requirement for it and take it through review, agreement, and commit before implementing that behavior.
:::

:::{req} requirements_review_skill.text_change.rename
:impl: assets/skills/habeas-requirements/SKILL.md
The `habeas-requirements` skill MUST direct agents to rename or move a committed requirement only with the user's agreement, using `habeas-refactor`, and to commit the rename or move before changing the requirement's text.
:::

:::{req} requirements_review_skill.code_rename
:impl: assets/skills/habeas-requirements/SKILL.md
The `habeas-requirements` skill MUST tell agents that renaming or restructuring code in place, without changing behavior, needs no requirement review, but that the item's references move with it and the evidence that read it is reassessed.
:::

:::{req} requirements_review_skill.review_prompt
:impl: assets/skills/habeas-requirements/references/review-prompt.md
The `habeas-requirements` skill MUST supply the review prompt that each group's reviewer follows. The prompt MUST direct the reviewer not to edit files. It MUST direct the reviewer to make mechanical changes itself in the revised text it returns, and to return design questions for the user.
:::

:::{req} requirements_review_skill.review_prompt.context
:impl: assets/skills/habeas-requirements/references/review-prompt.md
The review prompt MUST direct the reviewer to read the `habeas` skill for directive syntax. It MUST also direct the reviewer to read the drafts as uncommitted edits to the book, the chapter around each requirement in the group, and every requirement that the group cites or overlaps.
:::

:::{req} requirements_review_skill.review_prompt.brief
:impl: assets/skills/habeas-requirements/references/review-prompt.md
The review prompt MUST direct the reviewer not to re-raise the decisions and open choices that the brief lists.
:::

:::{req} requirements_review_skill.review_prompt.mechanical
:impl: assets/skills/habeas-requirements/references/review-prompt.md
The review prompt MUST count the following as mechanical changes, which the reviewer makes in the revised text and never phrases as questions:

- splitting a compound requirement;
- choosing between MUST, SHOULD, and MAY when the draft's intent is clear, defaulting to MUST and keeping a SHOULD or MAY the draft already chose;
- rewording vague or inconsistent terms, or an ambiguity, where only one resolution is sensible;
- adding an `{example}` that pins an obligation down;
- updating other requirements that a change makes stale.
:::

:::{req} requirements_review_skill.review_prompt.names
:impl: assets/skills/habeas-requirements/references/review-prompt.md
The review prompt MUST direct the reviewer never to rename or move a committed requirement, and to raise a wrong name as a design question instead. When the reviewer splits a committed requirement, the prompt MUST direct it to keep the requirement's full name on the core obligation, to name the parts `<name>.<part>`, and to list every new name.
:::

:::{req} requirements_review_skill.review_prompt.design
:impl: assets/skills/habeas-requirements/references/review-prompt.md
The review prompt MUST direct the reviewer to return the following as design questions, each with why it matters, the options, and a recommendation:

- gaps where more than one answer is plausible;
- conflicts with other requirements, citing them by full name;
- MUST versus SHOULD where the choice depends on the user's intent;
- obligations nothing could check, where any fix changes what is promised.
:::

:::{req} requirements_review_skill.review_prompt.compound
:impl: assets/skills/habeas-requirements/references/review-prompt.md
The review prompt's criteria MUST count as compound a requirement that lists several independent forms, checks, or outputs. They MUST count a sentence beginning "It MUST also" as a sign of a compound requirement. They MUST direct the reviewer to split a compound requirement into one requirement for each obligation that could be implemented, could change, or could fail on its own, and to move shared detail, such as a template or format, into its own requirement that the others cite.
:::

:::{req} requirements_review_skill.review_prompt.criteria
:impl: assets/skills/habeas-requirements/references/review-prompt.md
The review prompt's criteria MUST direct the reviewer to find each of the following:

- text that a careful reviewer could read two ways;
- obligations that a reviewer could not judge from the requirement text, the code, and the tests, or that nothing would fail for if they were broken;
- text that prescribes an implementation instead of an outcome.
:::

:::{req} requirements_review_skill.review_prompt.distributed
:impl: assets/skills/habeas-requirements/references/review-prompt.md
For a requirement with `{sites}` or `{violations}` queries, the review prompt MUST direct the reviewer to apply the guidance of {cite}`requirements.cli.distributed_guidance`.
:::

:::{req} requirements_review_skill.review_prompt.gaps
:impl: assets/skills/habeas-requirements/references/review-prompt.md
The review prompt MUST direct the reviewer to find gaps. For each requirement, the reviewer lists the inputs and states the requirement applies to, its boundaries, its failure cases (including empty, missing, duplicate, and invalid input), and its observable outputs. The reviewer then lists the observable behavior that the brief implies but that no requirement in the group or the book states. The prompt MUST direct the reviewer to treat each case the text does not decide as a gap: it resolves the gap mechanically when only one answer is sensible, and otherwise returns it as a design question.
:::

:::{req} requirements_review_skill.review_prompt.examples
:impl: assets/skills/habeas-requirements/references/review-prompt.md
The review prompt MUST include at least one worked example of a mechanical change and at least one worked example of a design question. The mechanical example MUST show a flawed draft together with the change the reviewer makes to it.
:::

:::{req} requirements_review_skill.review_prompt.report
:impl: assets/skills/habeas-requirements/references/review-prompt.md
The review prompt MUST give a report template with these sections:

- the revised text of every requirement in the group, including new split requirements, as complete directives ready to paste;
- the revised text of requirements outside the group that the changes affect;
- the mechanical changes, including new full names and added examples;
- the design questions.
:::

:::{req} requirements_review_skill.review_prompt.no_questions
:impl: assets/skills/habeas-requirements/references/review-prompt.md
The review prompt MUST tell the reviewer that a report with only wording changes means it has not looked hard enough for gaps and conflicts. If the reviewer looks again and still finds no design questions, the prompt MUST direct it to say so explicitly.
:::

:::{req} behavior_review_guidance
:impl: assets/skills/habeas-requirements/SKILL.md
:impl: assets/skills/habeas-bootstrap-requirements/SKILL.md
:impl: assets/skills/habeas-evidence/SKILL.md
Habeas's bundled agent skills MUST direct agents reviewing code against requirements to inspect every end-user-observable result and branch, including errors and input handling, and verify both that the code fulfills each applicable requirement and that it introduces no additional end-user-observable behavior without an agreed requirement. They MUST explain the requirement and example review criteria in {cite}`requirements.ci.review_criteria`, including test adequacy and justified absence of tests for requirements, and the three outcomes and documented-gap convention in {cite}`requirements.ci.outcomes`. Agents MUST mark the roots of a requirement's implementation with `{impl}` and the tests that verify it with `{test}`, leaving code reached from those items unmarked, and use `{cite}` only for related code that does neither, under {cite}`requirements.references.procedure`. When Habeas warns that an item has lost its coverage, with a coverage-loss review under {cite}`requirements.authoring.coverage_loss`, they MUST add `{impl}` where the review finds a root, and where it finds the item not needed, determine whether the item is dead code or the earlier read was mistaken before changing anything. Where requirements interact, agents MUST mark every requirement implemented at the relevant point and explain how their texts jointly account for the behavior. A citation alone MUST NOT count as that explanation.
:::

:::{req} internal_detail_guidance
:impl: assets/skills/habeas-requirements/SKILL.md
:impl: assets/skills/habeas-bootstrap-requirements/SKILL.md
:impl: assets/skills/habeas-evidence/SKILL.md
Habeas's bundled agent skills MUST direct agents to choose references by the
procedure in {cite}`requirements.references.procedure`. For internal code, they
MUST direct agents to state any invariant worth checking as an ordinary
requirement and implement it, to cite the built-in `habeas:util` under
{cite}`requirements.references.util` on an item that only serves other code, and
to report to the user an item that does neither. They MUST
NOT suggest grouping requirements, such as `internal.config_parsing`, that state
no checkable obligation.
:::

:::{req} flip_guidance
:impl: assets/skills/habeas/SKILL.md
:impl: assets/skills/habeas-refactor/SKILL.md
:impl: assets/skills/habeas-evidence/SKILL.md
:impl: assets/skills/habeas-requirements/SKILL.md
:impl: assets/skills/habeas-bootstrap-requirements/SKILL.md
The `habeas-evidence` skill MUST state the goal that independent assessments
of the same subject reach the same determination, because the requirement, code,
and tests leave no gap a careful reviewer could find and no reading a careful
reviewer could take differently, and the other bundled skills MUST refer to it
rather than restate it. The skills MUST tell agents that assessments of the
same subject that disagree are findings against that goal. They MUST direct agents to read both
rationales and decide whether one assessment found a real gap or the requirement
text admits more than one reading, then fix the source or tests, or tighten the
requirement text, rather than re-running an assessment to obtain a different
determination. Every change to agreed requirement text, including a rewording for precision,
MUST be reviewed with the user under
{cite}`requirements.cli.requirements_review_skill.agreed_text` before it is committed, and the
skills MUST then direct agents to give the reason, and the rationale that
prompted the change, in the message of the commit that makes it. That message
MUST describe the change relative to the prior commit, not the intermediate
drafts that led to it.
:::
:::{req} distributed_guidance
:impl: assets/skills/habeas/SKILL.md
:impl: assets/skills/habeas/references/distributed-requirements.md
Habeas's bundled agent skills MUST explain distributed requirements under {cite}`requirements.references.distributed`.
:::

:::{req} distributed_guidance.when
:impl: assets/skills/habeas/references/distributed-requirements.md
The bundled skills MUST direct agents to make a requirement distributed only when its obligation applies on its own at each of many sites, such as a convention every validation function follows; to use `{impl}` instead when particular code carries out the requirement as a whole; and to mark with `{impl}` the code that carries out any obligation of a distributed requirement that its sites do not uphold.
:::

:::{req} distributed_guidance.queries
:impl: assets/skills/habeas/references/distributed-requirements.md
The bundled skills MUST direct agents to write site queries that select every place the obligation applies and no others, violation queries for every kind of construct that could break it, and, for every query, `match` and `no_match` snippets that test its conditions.
:::

:::{req} distributed_guidance.site_text
:impl: assets/skills/habeas/references/distributed-requirements.md
The bundled skills MUST direct agents to describe sites exactly as the site queries select them, to state as an obligation any convention the queries rely on, such as how a list is named, and to state only per-site obligations that a reviewer can check from the site and the helpers it calls.
:::

:::{req} distributed_guidance.reviews
:impl: assets/skills/habeas/references/distributed-requirements.md
:impl: assets/skills/habeas-evidence/references/reviews.md
The bundled skills MUST tell agents that sites need no reference to the requirement, that `habeas author --all` authors site reviews under {cite}`requirements.references.site_review`, and that a false site review is addressed by fixing the site, or by tightening the queries or the requirement text when the site should not be covered or the text admits more than one reading, under {cite}`requirements.cli.flip_guidance`, not by re-running the review.
:::

:::{req} distributed_guidance.lists
:impl: assets/skills/habeas/references/distributed-requirements.md
The `habeas` skill's reference on distributed requirements MUST give the guidance of {cite}`requirements.cli.distributed_guidance.when`, {cite}`requirements.cli.distributed_guidance.queries`, {cite}`requirements.cli.distributed_guidance.site_text`, and {cite}`requirements.cli.distributed_guidance.reviews` as a list of things to do and a list of things not to do.
:::

:::{req} distributed_guidance.bootstrap
:impl: assets/skills/habeas-bootstrap-requirements/SKILL.md
The `habeas-bootstrap-requirements` skill MUST direct agents to propose a distributed requirement for a cross-cutting convention instead of marking each of its sites with `{impl}`.
:::

:::{req} refactor_skill
:impl: assets/skills/habeas-refactor/SKILL.md
Habeas MUST bundle a `habeas-refactor` skill whose description tells agents to
use it when a change moves code, tests, or requirements to another file,
chapter, or outline node, including splitting or merging modules or chapters,
or renames a requirement, so that evidence whose content is unchanged is not
invalidated.
:::

:::{req} refactor_skill.workflow
:impl: assets/skills/habeas-refactor/SKILL.md
The `habeas-refactor` skill MUST direct agents to make the move with their own
edits, keeping it separate from any change to the moved code or text; to run
`habeas refactor --dry-run` under {cite}`requirements.refactor.dry_run` with a
mapping for every moved file, node, and definition name under
{cite}`requirements.refactor.mappings`; to review its report, including
records it would leave unchanged and the rationale spans it lists under
{cite}`requirements.refactor.rationale.untouched`; to run it without
`--dry-run`; to confirm with `habeas status` that only the reported records
are stale; and to commit the move and evidence update before changing
behavior, then reassess stale records with `habeas author` under the
habeas-evidence skill.
:::

:::{req} refactor_skill.limits
:impl: assets/skills/habeas-refactor/SKILL.md
The `habeas-refactor` skill MUST tell agents not to edit evidence files by hand
to follow a move, and that renaming a function or editing moved text is not a
move and needs reassessment.
:::

:::{req} skill_structure
:impl: assets/skills/habeas/SKILL.md
:impl: assets/skills/habeas-requirements/SKILL.md
:impl: assets/skills/habeas-evidence/SKILL.md
:impl: assets/skills/habeas-bootstrap-requirements/SKILL.md
:impl: assets/skills/habeas-refactor/SKILL.md
Habeas's bundled agent skills MUST jointly give the direction that the requirements on them specify. A skill consists of its `SKILL.md` and the files under its `references/` directory. Each topic MUST be stated in full in exactly one skill, its home.
:::

:::{req} skill_structure.refer
:impl: assets/skills/habeas/SKILL.md
:impl: assets/skills/habeas-requirements/SKILL.md
:impl: assets/skills/habeas-evidence/SKILL.md
:impl: assets/skills/habeas-bootstrap-requirements/SKILL.md
:impl: assets/skills/habeas-refactor/SKILL.md
A skill that needs a topic whose home under {cite}`requirements.cli.skill_structure` is another skill MUST refer to that skill instead of restating the topic. It MAY summarize the topic in a few lines.
:::

:::{req} skill_structure.habeas
:impl: assets/skills/habeas/SKILL.md
:impl: assets/skills/habeas/references/syntax-details.md
:impl: assets/skills/habeas/references/distributed-requirements.md
Under {cite}`requirements.cli.skill_structure`, the `habeas` skill MUST be the home of routing tasks to skills, `habeas status` and `habeas info`, directive and reference syntax, distributed requirements, and gotchas common to every task.
:::

:::{req} skill_structure.requirements
:impl: assets/skills/habeas-requirements/SKILL.md
:impl: assets/skills/habeas-requirements/references/review-prompt.md
Under {cite}`requirements.cli.skill_structure`, the `habeas-requirements` skill MUST be the home of drafting, reviewing, and agreeing requirements, the requirement review criteria, the procedure for choosing references, and the criteria the assessment applies to code and tests.
:::

:::{req} skill_structure.evidence
:impl: assets/skills/habeas-evidence/SKILL.md
:impl: assets/skills/habeas-evidence/references/authoring-failures.md
:impl: assets/skills/habeas-evidence/references/fixme.md
:impl: assets/skills/habeas-evidence/references/reviews.md
Under {cite}`requirements.cli.skill_structure`, the `habeas-evidence` skill MUST be the home of the authoring workflow, fixing validation findings, including uncovered Rust items, outcomes and FIXME judgments, citation and site reviews, disagreeing assessments, and the commit message for a change to agreed requirement text.
:::

:::{req} skill_structure.bootstrap
:impl: assets/skills/habeas-bootstrap-requirements/SKILL.md
Under {cite}`requirements.cli.skill_structure`, the `habeas-bootstrap-requirements` skill MUST be the home of deriving an initial requirement set from existing code.
:::

:::{req} skill_structure.refactor
:impl: assets/skills/habeas-refactor/SKILL.md
Under {cite}`requirements.cli.skill_structure`, the `habeas-refactor` skill MUST be the home of moving code, tests, and requirements to another file, chapter, or outline node, and of renaming requirements.
:::

::::{req} skill_conventions
Each bundled skill's description MUST state when to use it in terms of the user's task. It MAY name another bundled skill to use instead where an agent could confuse the two. Each bundled skill, recognized by its front matter, is a site:

:::{sites} skills
```yaml
language: markdown
paths: [assets/skills]
rule:
  kind: minus_metadata
match:
  - "---\nname: example\n---\n\n# Example\n"
no_match:
  - "# Example\n"
```
:::
::::

::::{req} skill_conventions.workflow
Each bundled skill MUST open, before any other section, with its workflow as ordered steps. Each bundled skill, recognized by its front matter, is a site:

:::{sites} skills
```yaml
language: markdown
paths: [assets/skills]
rule:
  kind: minus_metadata
match:
  - "---\nname: example\n---\n\n# Example\n"
no_match:
  - "# Example\n"
```
:::
::::

::::{req} skill_conventions.next
Each bundled skill's workflow MUST end by naming the bundled skill for the next task, or, when different outcomes lead to different tasks, the skill for each such outcome. An outcome that leads to no further task needs no skill named. Each bundled skill, recognized by its front matter, is a site:

:::{sites} skills
```yaml
language: markdown
paths: [assets/skills]
rule:
  kind: minus_metadata
match:
  - "---\nname: example\n---\n\n# Example\n"
no_match:
  - "# Example\n"
```
:::
::::

::::{req} skill_conventions.references
A bundled skill MAY put material needed only in specialized situations in files it links to. Each such link MUST say when to read the file. Each bundled skill, recognized by its front matter, is a site:

:::{sites} skills
```yaml
language: markdown
paths: [assets/skills]
rule:
  kind: minus_metadata
match:
  - "---\nname: example\n---\n\n# Example\n"
no_match:
  - "# Example\n"
```
:::
::::

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

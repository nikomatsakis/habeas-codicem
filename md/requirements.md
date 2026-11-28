# The `habeas` system

Habeas connects a project's prose to its implementation and tests. This reference
explains how to write requirements and examples, author evidence, check it, and
browse those connections in the book.

Start with [How it works](reference.md) for the workflow. The chapters below
explain each part of the system and specify its behavior. The
[SDK reference](requirements/sdk.md) explains the reusable building blocks for
implementing tools like Habeas.

- [CI and evidence validation](requirements/ci.md)
- [Configuration](requirements/configuration.md)
- [Authoring evidence](requirements/authoring.md)
- [Command-line interface](requirements/cli.md)
- [References and Rust items](requirements/references.md)
- [Named examples](requirements/examples.md)
- [Named definitions](requirements/definitions.md)
- [Book rendering](requirements/index/)
- [Installation](requirements/install.md)
- [Project integration](requirements/integration.md)
- [Project layout](requirements/layout.md)
- [Refactoring](requirements/refactor.md)
- [Test execution coverage](requirements/test-coverage.md)

Capitalized requirement terms in these chapters use the meanings defined by BCP
14 ([RFC 2119](https://www.rfc-editor.org/rfc/rfc2119) and
[RFC 8174](https://www.rfc-editor.org/rfc/rfc8174)).

## How the requirements fit together

The References chapter defines what source annotations mean and which items
need them. Other chapters cite those rules where a tool or a person uses them.
For example, the test annotation rule says that each test source item needs a `{test}` annotation. {cite}`requirements.references.coverage.tests`

Project integration specifies how `habeas::validate()` enforces that rule in a
Cargo test. The CLI chapter specifies what `habeas status` displays when the
same rule is violated. Authoring describes how reviewers use the annotations
to assess evidence. The skills guide authors in placing structural `{impl}`
markers, including ones proposed during review.
{cite}`requirements.integration.validate.test_annotations`
{cite}`requirements.cli.status.test_annotations`
{cite}`requirements.authoring.behavioral_review`
{cite}`requirements.references.structural_markers`

## What needs human review

The maintainer’s review goal is “I should not need to review any
changes that do not require a change in requirements.” A committed public API
is part of the requirements. Changing a public field type or a method signature
should put that decision in the requirements diff. An implementation change
that preserves the contract can instead be assessed by refreshing its evidence.

## Requirements as authoring inputs

:::{req} authoring_input
When authoring evidence for a requirement, its fenced declaration, including its complete body but excluding anchor option lines, MUST
be included in the mandatory input set defined by
{cite}`requirements.authoring.mandatory_inputs`.
:::

# Requirements

Capitalized requirement terms in this chapter use the meanings defined by BCP 14 ([RFC 2119](https://www.rfc-editor.org/rfc/rfc2119) and [RFC 8174](https://www.rfc-editor.org/rfc/rfc8174)).

The requirements are organized by the behavior they describe:

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

## Requirements as authoring inputs

:::{req} authoring_input
When authoring evidence for a requirement, its complete fenced declaration MUST
be included in the mandatory input set defined by
{cite}`requirements.authoring.mandatory_inputs`.
:::

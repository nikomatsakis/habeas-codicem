# Installation

:::{req} symposium
:impl: README.md
The getting-started documentation MUST tell users with a Cargo and mdBook project to install and initialize Symposium, run `cargo add --dev habeas`, and run `cargo agents sync`, reviewing the prompt to enable Habeas's plugin. It MUST explain that the session-start hook advises the agent to run `habeas setup` when configuration is incomplete, how to refresh skills after updating Habeas, and how to preserve local edits when migrating from skills copied by older Habeas versions.
:::

:::{req} symposium.skill_hook
:impl: README.md
The getting-started documentation MUST explain that the session-start hook tells the agent to load the `habeas` skill.
:::

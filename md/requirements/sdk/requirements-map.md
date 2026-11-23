# Allocating requirements to TBV and Habeas Codicem

This page maps existing HC contracts to their proposed SDK homes and records
the implementation work and API choices needed to align them. SDK contracts
live under `requirements.sdk.graph`, `requirements.sdk.sources`, and
`requirements.sdk.outlines`.

## Core SDK contracts

| Existing contract | Shared SDK responsibility | HC specialization |
| --- | --- | --- |
| `references.outlines`, `references.outline_selectors` | Outline module, default language selection, and stable selectors | Repository discovery and source-item-to-node mapping |
| `references.structural_hash` | Structural fingerprints and identity | Which fingerprint each evidence input uses |
| `references.outline_comments` | Comment classification and attachment hooks | Reference discovery and ownership |
| `authoring.delivered_inputs` | Record observations actually supplied to a procedure | Evidence input fields and source coverage |
| `authoring.text_search`, `authoring.structural_search` | Fixed source tools, typed inspection operations, result completeness, and recording policy | Project directory, node identities, and reference relationships |
| `ci.reference_set` | Outgoing-edge selection changes invalidate saved judgments | Which reference roles form a requirement's root set |
| `ci.outline_inputs` | Re-read observations and compare fingerprints | JSON schema, diagnostics, and evidence lookup |
| `authoring.deterministic_validation` | Checking saved results without an agent capability | CI acceptance policy and reported errors |

The identifiers in this table are abbreviated: each begins with `requirements.`.
Split mixed requirements into SDK and HC obligations. The SDK search tools use
a fixed discovery policy for search result sets and record selected source nodes
as separate inputs.

## Individual outline definitions

Move Rust-specific selector examples and contracts from
`requirements.references.rust_item_selectors` to the Rust outline page. Separate
the Rust documentation-comment rules from the shared structural-hash contract.
Place Markdown comment classification with Markdown, and give each other
provider's grammar mapping, naming, and comment rules its own page.

Use the [language chapters](outlines.md#language-specific-rules) for
each language's contracts and examples.

## Habeas Codicem application contracts

Keep declaration syntax, reference roles, review prompts, coverage, evidence JSON,
verdict meanings, and CI acceptance policy in the HC chapters. Tooling pages
should explain how commands assemble SDK procedures and inspection capabilities.
HC decides which `true`, `false`, or `FIXME` determinations satisfy its command
acceptance policy.

## Migration sequence

First review each shared contract and its language or application-specific
remainder. Then move declarations and update references and evidence mappings
together.

## Contracts to align before implementation

The following existing interfaces and contracts need changes to implement the
design described by these SDK chapters.

| Current interface or contract | Required change |
| --- | --- |
| `Agent<D>`, `CompletionProvider`, `Conversation`, and `ToolAgent::with_tools` as application extension points | Configure SDK-supplied agents through `AgentConfig`; keep conversation and tool machinery inside the SDK. |
| HC's backend construction and backend configuration types | Supply serializable backend configuration from the SDK while retaining HC's configuration loading, paths, and overrides. |
| Application-specific access policy in the SDK | Keep Habeas configuration and tracked-file rules in `habeas`; TBV inspection uses its project directory and `.gitignore`. |
| `SourceTools` observation callbacks that may return no dependency | Map source items into application nodes and require tracked source reads to record supplied node snapshots. |
| Static language registrations and manual registry assembly | Add filename-based selection and explicit cheaply cloneable `OutlineLang` values in `trust_but_verify::outline`. |
| Outline selection followed by manual content and fingerprint assembly | Return owned `OutlineContent` values with retained text, structural hash, and scheme; supply their SDK `NodeContents` implementation. |
| `requirements.authoring.github_reads` | Use HC issue nodes and criteria for GitHub reads and issue checks. |
| `requirements.authoring.initial_prompt` and the authoring tool-specific contracts | Use the SDK's fixed inspection and submission tools. Preserve application review instructions, reference relationships, redaction, and complete input accounting. |
| `requirements.authoring.text_search`, `requirements.authoring.structural_search`, and `requirements.authoring.finish_search` | Preserve complete selected-excerpt accounting when adopting SDK-owned search and input tracking. |
| `requirements.authoring.deterministic_validation`, `requirements.cli.fixme_issues`, the `requirements.ci.fixme_closed_issue` example, and the offline exception in `requirements.authoring.github_reads` | Decide how offline validation reports freshness of external issue inputs; preserve the distinction between an unavailable observation and a current one. |

The external-node design needs an offline access policy: using
available cached snapshots and reporting unavailable freshness, or allowing
validation to refresh issues over the network. HC chooses that policy; the SDK
compares whatever snapshots the domain can actually supply.

## API choices for review

The ownership boundaries are established. The following details still need
decisions before implementation:

- **Source capabilities.** Keep domains without source access usable with the SDK's graph
  tools, and add its built-in source operations when the domain supplies source
  access and source-node mapping. This is the recommended capability rule.
  The exact hook representation and agent-specific label-schema bounds remain
  to be specified for agent tools.
- **Agent initialization.** The displayed `with_agent(config) -> Self` suggests
  deferred backend initialization. Recommend reporting configuration or
  credential setup failures as `ValidationError::Agent` when an ask is awaited,
  preserving deterministic execution without model setup. A fallible setup
  method is the alternative.
- **Structural-search capability.** The outline-facing `OutlineLang` methods are
  sufficient for outlining, but ast-grep search also needs syntax support.
  Specify an optional syntax capability and an explicit
  unsupported-language error for outline-only implementations.
- **External access failures.** Decide how offline validation, missing issues,
  and failed refreshes affect an HC command. Recommend domain read errors for
  unavailable contents and an explicit HC policy for unavailable external
  freshness.
- **Existing FIXME reviews.** Recommend requiring relevance evidence through
  a new criterion identity or changed HC review revision. Evidence-format
  conversion preserves old judgments; the new criterion requires a new
  assessment.

`OutlineId` stores a serializable file path and selector. `OutlineContent`
retains the loaded source snapshot and the selected `OutlineLang`
fingerprinting rules.

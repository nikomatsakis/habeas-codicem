# Allocating requirements to Habeas SDK and Habeas Codicem

This page maps existing HC contracts to their proposed SDK homes and records
the implementation work and API choices needed to align them. SDK contracts
live under `requirements.sdk.graph`, `requirements.sdk.sources`, and
`requirements.sdk.outlines`.

## Core SDK contracts

| Existing contract | Shared SDK responsibility | HC specialization |
| --- | --- | --- |
| `references.outlines`, `references.outline_selectors` | Independent outline crate, default language selection, and stable selectors | Repository discovery and source-item-to-node mapping |
| `references.structural_hash` | Structural fingerprints and identity | Which fingerprint each evidence input uses |
| `references.outline_comments` | Comment classification and attachment hooks | Reference discovery and ownership |
| `authoring.delivered_inputs` | Record observations actually supplied to a procedure | Evidence input fields and source coverage |
| `authoring.text_search`, `authoring.structural_search` | Fixed source tools, typed inspection operations, result completeness, and recording policy | Resource scope, node identities, and reference relationships |
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

Use the [language chapters](sources.md#each-language-defines-its-outline) for
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
| `Graph::Error` | Define the common domain error as `Types::Error`, with `Graph: Types`. |
| Separate `Graph` and `Contents` traits | Put `NodeContent` and `node_content` on `Graph`; returned values implement `NodeContents::fingerprint` and `NodeContents::format`. |
| `Graph`, `Storage`, and `Domain` in `habeas_sdk::graph` | Define the domain traits, including `Types`, in `habeas_sdk::domain`. |
| `Agent<D>`, `CompletionProvider`, `Conversation`, and `ToolAgent::with_tools` as application extension points | Configure SDK-supplied agents through `AgentConfig`; keep conversation and tool machinery inside the SDK. |
| HC's backend construction and backend configuration types | Supply serializable backend configuration from the SDK while retaining HC's configuration loading, paths, and overrides. |
| `SourceTools` observation callbacks that may return no dependency | Map source items into application nodes and require tracked source reads to record supplied node snapshots. |
| `habeas_sdk::outline`, static language registrations, and manual registry assembly | Provide `habeas-sdk-outline`, default filename-based selection, and optional additional `Language` implementations. |
| Outline selection followed by manual content and fingerprint assembly | Return owned `SelectedSource` values with retained text, structural hash, and scheme; supply their SDK `NodeContents` implementation. |
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

- **Source capabilities.** Keep graph-only domains usable with the SDK's graph
  tools, and add its built-in source operations when the domain supplies source
  access and source-node mapping. This is the recommended capability rule.
  The exact hook representation and agent-specific label-schema bounds remain
  to be specified for agent tools.
- **Agent initialization.** The displayed `with_agent(config) -> Self` suggests
  deferred backend initialization. Recommend reporting configuration or
  credential setup failures as `ValidationError::Agent` when an ask is awaited,
  preserving deterministic execution without model setup. A fallible setup
  method is the alternative.
- **Additional languages.** Define what happens when a registration duplicates
  a language name or claims an already-supported extension. Recommend explicit
  replacement or a reported conflict. This
  decision determines the final registration signature.
- **Structural-search capability.** The outline-facing `Language` methods are
  sufficient for outlining, but ast-grep search also needs syntax support.
  Recommend an optional grammar or equivalent syntax capability and an explicit
  unsupported-language error for outline-only implementations.
- **External access failures.** Decide how offline validation, missing issues,
  and failed refreshes affect an HC command. Recommend domain read errors for
  unavailable contents and an explicit HC policy for unavailable external
  freshness.
- **Existing FIXME reviews.** Recommend requiring relevance evidence through
  a new criterion identity or changed HC review revision. Evidence-format
  conversion preserves old judgments; the new criterion requires a new
  assessment.

The outline address representation is also proposed here for review:
`OutlineId` retains serializable resource and selector data, while the resolved
document holds `Arc<dyn Language>`. This keeps persisted node identity separate
from the runtime language handle.

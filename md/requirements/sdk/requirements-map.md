# Allocating requirements to TBV and Habeas Codicem

This page maps HC contracts to the shared SDK capabilities they use and
identifies the application policy that stays in Habeas. SDK contracts
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
The SDK search tools use a fixed discovery policy for search result sets and
record selected source nodes as separate inputs.

## Individual outline definitions

The [language chapters](outlines.md#language-specific-rules) define each
language's selectable items, selectors, comment rules, and hashing algorithms.
The shared outline interface introduces `OutlineId`, `OutlineContent`, and
`OutlineLang` without exposing parser grammars.

## Habeas Codicem application contracts

Declaration syntax, reference roles, review prompts, coverage, evidence JSON,
verdict meanings, and CI acceptance policy belong to HC. Its tooling pages
explain how commands assemble SDK criteria and inspection capabilities.
HC decides which `true`, `false`, or `FIXME` determinations satisfy its command
acceptance policy.

## SDK ownership and interfaces

The SDK provides the following interfaces and keeps their underlying
conversation, provider, filesystem, and search machinery private.

| Capability | SDK interface | HC responsibility |
| --- | --- | --- |
| Agent configuration | Serializable `AgentConfig` and `Validator::with_agent`; backend initialization happens when an ask is awaited. | Configuration loading, paths, environment overrides, and review instructions. |
| File inspection | `Sources::new` and `Validator::with_sources`, using the project directory and `.gitignore`. | HC's configuration and tracked-file rules. |
| Source-node mapping | `SourceNodes::source_node` and tracked reads through `Domain`; every delivered source read records its node snapshot. | Source addresses, node identities, and reference relationships. |
| Source outlines | Filename-based language selection, cheaply cloneable `OutlineLang`, and owned `OutlineContent` implementing `NodeContents`. | Choosing the source nodes needed for a review. |
| Structural search | Optional `OutlineLang::search_structure`; outline-only languages return an unsupported-language error. | Choosing search rules for the question under review. |
| Redaction | Fixed credential recognition in model-visible text and bundled structural hashes. | Explicit credential fields in Habeas configuration. |

Domains without source-node mapping can still use graph inspection and
deterministic criteria. Configured agent tools accept serializable edge labels
with a JSON schema and report domain errors through `Display`; the exact bounds
appear with `with_agent` and `with_sources`.

## HC integration work

HC retains an internal assessment adapter for its review protocol and evidence
accounting. Further integration should preserve
`requirements.authoring.initial_prompt`, the authoring tool contracts, and
complete selected-excerpt accounting under `requirements.authoring.text_search`,
`requirements.authoring.structural_search`, and
`requirements.authoring.finish_search`.

External issue nodes need HC policy for unavailable observations and offline
freshness. That policy must reconcile `requirements.authoring.github_reads`,
`requirements.authoring.deterministic_validation`,
`requirements.cli.fixme_issues`, and the
`requirements.ci.fixme_closed_issue` example. The SDK compares the snapshots
that the domain can supply; HC decides when network access is permitted and how
an unavailable observation affects a command.

New FIXME relevance checks need a new criterion identity or review revision.
Evidence-format conversion preserves an existing judgment and does not perform
a new assessment.

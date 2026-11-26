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
The shared search policy requires the agent to close every delivered search
page and select the excerpts it relied on under
{cite}`requirements.sdk.graph.search.finish_search.behavior`. HC already enforces
this policy in its assessment adapter. The SDK tool implementation still needs
to adopt it.

## Individual outline definitions

The [language chapters](outlines.md#language-specific-rules) define each
language's selectable items, selectors, comment rules, and hashing algorithms.
The shared outline interface introduces `OutlineData`, `OutlineContent`, and
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
| Agent configuration | Serializable `AgentConfig` and `ValidationCache::with_agent`; backend initialization happens when an ask is awaited. | Configuration loading, paths, environment overrides, and review instructions. |
| File inspection | `Sources::new` selects the project directory for inspection and respects `.gitignore`. | HC's configuration and tracked-file rules. |
| Source-node mapping | Interning the data supplied by `Domain::source_node` and tracking reads through `Domain`; every delivered source read records its node snapshot. | Source addresses, node identities, and reference relationships. |
| Source outlines | Filename-based language selection, cheaply cloneable `OutlineLang`, and owned `OutlineContent` implementing `NodeContents`. | Choosing the source nodes needed for a review. |
| Structural search | Optional `OutlineLang::search_structure`; outline-only languages return an unsupported-language error. | Choosing search rules for the question under review. |
| Redaction | Fixed credential recognition in model-visible text and bundled structural hashes. | Reporting explicit credentials in tracked Habeas configuration files. |

Domains without source-node mapping can still use graph inspection and
deterministic criteria. Awaiting an agent request requires edge labels that implement
`DeserializeOwned`, `JsonSchema`, and `PartialEq`, and domain errors that
implement `Display`. See [agent calls](graph/agent-calls.md).

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

New FIXME relevance checks need a new criterion identity or review version.
Evidence-format conversion preserves an existing judgment and does not perform
a new assessment.

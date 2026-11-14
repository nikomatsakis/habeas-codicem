# Allocating requirements to the SDK and Habeas

The SDK graph and sources chapters define the core validation model. Habeas chapters should
define how requirement reviews, citation reviews, evidence files, and CLI commands
use that model. This page proposes the allocation; the earlier Habeas requirement IDs and their evidence have not yet moved.
The uncommitted SDK contracts now live under `requirements.sdk.graph` and
`requirements.sdk.sources`.

## Core SDK contracts

| Existing contract | Shared SDK responsibility | Habeas specialization |
| --- | --- | --- |
| `references.outlines`, `references.outline_selectors` | Outline construction and stable selectors | Default provider selection and repository discovery |
| `references.structural_hash` | Structural fingerprints and identity | Which fingerprint each evidence input uses |
| `references.outline_comments` | Comment classification and attachment hooks | Reference discovery and ownership |
| `authoring.delivered_inputs` | Record observations actually supplied to a procedure | Evidence input fields and source coverage |
| `authoring.text_search`, `authoring.structural_search` | Typed inspection operations and query observations, including empty results | Model tool schemas, reference filters, and discovery-only policy |
| `ci.reference_set` | Query-result changes invalidate saved judgments | Which reference roles form a requirement's root set |
| `ci.outline_inputs` | Re-read observations and compare fingerprints | JSON schema, diagnostics, and evidence lookup |
| `authoring.deterministic_validation` | Checking saved results without an agent capability | CI acceptance policy and reported errors |

The identifiers in this table are abbreviated: each begins with `requirements.`.
A mixed requirement needs splitting, rather than moving its entire current body
into the SDK. In particular, recording a search result as a dependency is distinct
from claiming that all matching source nodes were examined.

## Individual outline definitions

Move Rust-specific selector examples and contracts from
`requirements.references.rust_item_selectors` to the Rust outline page. Separate
the Rust documentation-comment rules from the shared structural-hash contract.
Place Markdown comment classification with Markdown, and give each other
provider's grammar mapping, naming, and comment rules its own page.

The [outline pages](sources.md) currently describe the implementation and provide
examples. They are the proposed homes for those language contracts, not duplicate
normative declarations.

## Habeas application contracts

Keep declaration syntax, reference roles, review prompts, coverage, evidence JSON,
verdict meanings, and CI acceptance policy in the Habeas chapters. Tooling pages
should explain how commands assemble SDK procedures and inspection capabilities.
An SDK dependency record does not itself decide whether `true`, `false`, or
`FIXME` is an acceptable Habeas determination.

## Migration sequence

First review each shared contract and its language or application-specific
remainder. Then move declarations and update references and evidence mappings
together. The Rust interface definitions already in the SDK chapter remain the
place to specify the public API; behavioral requirements should follow the
examples that introduce each interface.

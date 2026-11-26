# The trust-but-verify SDK

:::{req} public_api
Each public SDK item has one defining module path. The SDK exposes no public
re-exports. The chapter that introduces each supported type or free function owns its declaration.
The [public API inventory](sdk/public-api.md) links each item to that requirement,
which describes its purpose
and declares its public Rust interface, including fields, enum variants, trait
members, and inherent methods. A change to that interface requires a change to
the owning requirement. Every supported struct with public fields is
`#[non_exhaustive]`. Its constructor accepts the values needed to create it.
Additional fields have consuming builder methods that accept a concrete value.
Optional fields start as `None`. Backend inference options start empty and can
be added one key at a time.

The SDK’s public modules follow the interfaces introduced in these chapters.

- `domain`: application data and the `Domain` and `NodeContents` traits.
- `graph`: criterion definitions, graph observations, evidence, and functions for running criteria.
- `request`: `ValidationContext`, typed `Ask` builders, context values, and request phases.
- `agent`: configuration for the supplied agents.
- `sources`: project inspection, search queries, and their results.
- `outline`: selectable source contents and language rules; `outline::languages`
  supplies bundled languages, and `outline::selector` defines selector syntax.
- `storage`: filesystem evidence storage.

Source access, search engines, redaction, provider transports, agent-result paging,
and conversation machinery are implementation details of those interfaces.
:::

The trust-but-verify SDK (TBV) supplies tracked criterion execution and
configured agents for an application-defined graph. Habeas Codicem (HC), the
mdBook and requirements tooling, is one application of it.
A node identifies a thing: a requirement, a function, a test, or a GitHub issue.
The application can ask several questions about the same node. Each
{term}`requirements.sdk.graph.criteria.criterion`
value identifies a test to perform and supplies its execution procedure. The SDK
tracks each criterion's answer independently and supplies operations for loading,
checking, executing, and saving evidence. The application decides when to reuse
an answer and whether that answer satisfies its validation policy.

## Define application data and policy

The application interprets its node identities, obtains their contents, and
chooses which criteria to run. TBV runs agent conversations, supplies
inspection tools, and records dependencies.

| Application supplies | SDK supplies |
| --- | --- |
| Node identities, paths, and labeled relationships | Domain and tracked inspection interfaces |
| Node contents, formatting, and fingerprint rules | Recording and comparing supplied observations |
| Criteria, `Context` values, and the meaning of their answers | Criterion execution and typed ask builders |
| Serializable backend configuration and optional domain tools | Agent harness, built-in tools, and backend integration |
| Project directory and source-item-to-node mapping | Filesystem inspection respecting `.gitignore` and the outline module |
| Evidence location and command acceptance policy | Evidence comparison and storage helpers |

Source inspection operates on the selected project directory and respects
`.gitignore`. The application maps a selected file and outline item into its
graph, then supplies its contents through `Domain`.

## Start with one requirement and its inputs

Suppose the application's specification contains this requirement in
`md/requirements/auth.md`:

```markdown
:::{req} expiration
The application rejects all expired tokens. A token is expired when the
current time is greater than or equal to its expiration time.
:::
```

With `md/` as the book's source directory, this declaration is named
`requirements.auth.expiration`. Here is a small implementation and test,
reduced to the expiration check:

```rust
// {impl}`requirements.auth.expiration`
fn check_token(expires_at: u64, now: u64) -> bool {
    now < expires_at
}

// {test}`requirements.auth.expiration`
#[test]
fn expired_token_is_rejected() {
    assert!(check_token(100, 99));
    assert!(!check_token(100, 100));
    assert!(!check_token(100, 101));
}
```

The `{impl}` comment identifies `check_token` as an implementation reference. The
`{test}` comment selects `expired_token_is_rejected` as a test input.

To assess this requirement, HC asks the SDK agent with the requirement text
and the test function. The agent judges whether the tests adequately check the
requirement, and can request implementation source to answer a specific
question.

## SDK Perspective: obligations as a graph

The SDK represents things as nodes in a graph. A node has identity and contents;
the application chooses which criteria, if any, to verify for it. Each criterion
has its own inputs and saved evidence. The application defines how contents are
fetched and how each criterion is executed.

In our example, the requirement `requirements.auth.expiration` has a node, as does the Rust function `check_token` and the Rust test `expired_token_is_rejected`. The source for the requirement is the requirement text itself (*"The application rejects..."*). The source for the Rust nodes are their source content. These 3 nodes are connected by labeled edges, like so:

```mermaid
flowchart TD
    R["requirements.auth.expiration"]
    R -->|implemented by| I["check_token"]
    R -->|tested by| T["expired_token_is_rejected"]
```

Edges make related nodes available to the validation process. The callback
receives a context identifying `requirements.auth.expiration`; it explicitly
chooses which contents to read or insert into each agent call. HC can insert
the requirement and `Tests` successors while leaving implementation source to
later requests. The SDK records those reads and the outgoing selections the
procedure observes. If a relevant part of a recorded input changes, the saved
validation becomes stale. Fingerprints determine which changes count as
relevant.

## Defining the application domain

::::{definition} application_domain
:phrase: application domain

:::{cite} requirements.sdk.domain
An *application domain* is the application's graph, node contents, and evidence
storage, made available together for criterion execution. In the SDK, it
implements [`Domain`](../rustdocs/trust_but_verify/domain/trait.Domain.html) to supply
its shared types, node and edge operations, and evidence persistence.
:::
::::

:::{req} domain
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Every {term}`requirements.sdk.application_domain` implements
`trust_but_verify::domain::Domain`. A domain is cloneable and safe to share between
executions. The following chapters introduce its members one capability at a
time.

```rust,ignore
pub trait Domain: Clone + Send + Sync {
    type Error: Send;
    type NodeData: Clone + Eq + Hash + Send + Sync;
    type NodeContent: NodeContents;
    type EdgeLabel: Clone + Send + Sync;

    fn cache(&self) -> &ValidationCache<Self>;
    fn node_path(&self, node: &NodeId<Self>) -> NodePath;
    fn node_content(
        &self,
        node: &NodeId<Self>,
    ) -> Result<Self::NodeContent, Self::Error>;
    fn outgoing_edges(
        &self,
        node: &NodeId<Self>,
        label: Self::EdgeLabel,
    ) -> Result<Vec<NodeId<Self>>, Self::Error>;
    fn load_evidence(
        &self,
        node: &NodeId<Self>,
        criterion: &CriterionPath,
    ) -> impl Future<Output = Result<Option<Record<Self::NodeData, Self::EdgeLabel>>, Self::Error>> + Send;
    fn store_evidence(
        &self,
        node: &NodeId<Self>,
        criterion: &CriterionPath,
        record: &Record<Self::NodeData, Self::EdgeLabel>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn project_sources(&self) -> Option<SourceProject<Self>> { ... }
    fn source_node(
        &self,
        item: &OutlineData,
    ) -> Result<Self::NodeData, SourceError> { ... }
    fn source_address(&self, _data: &Self::NodeData) -> Option<OutlineData> { ... }
}
```
:::

:::{req} errors
{cite}`requirements.sdk.domain`

The {term}`requirements.sdk.application_domain` defines its central `Error`
associated type on `Domain`. Node reads, edge queries, and evidence persistence
use it to report internal errors:

```rust,ignore
// Defined in trust_but_verify::domain.
pub trait Domain: Clone + Send + Sync {
    // ...

    /// Application-defined errors from domain operations.
    type Error: Send;

    // ...
}
```
:::

Domains with mutable state commonly store it behind `Arc`, so cloned domains
access the same graph and evidence storage. Each domain also supplies a
[validation cache](sdk/validation-cache.md) through `Domain::cache`.

## SDK chapters

The chapters introduce each interface and the tools used with it:

| Chapter | Purpose | Related traits and types |
| --- | --- | --- |
| [Validation cache](sdk/validation-cache.md) | Intern node data, share source outlines, and configure the agent. | `ValidationCache`, `Domain::cache` |
| [Graph nodes](sdk/graph.md) | Node identities, stable paths, contents, and fingerprints. | [`Domain`](../rustdocs/trust_but_verify/domain/trait.Domain.html), [`NodeContents`](../rustdocs/trust_but_verify/domain/trait.NodeContents.html) |
| [Graph edges](sdk/graph-edges.md) | Relationship labels and successor lookup. | [`Domain`](../rustdocs/trust_but_verify/domain/trait.Domain.html) |
| [Criteria](sdk/graph/criteria.md) | Criterion types, completed answers, identity, review rules, and execution. | [`Criterion`](../rustdocs/trust_but_verify/graph/trait.Criterion.html) |
| [Running criteria](sdk/graph/running-criteria.md) | Execute criteria and check or reuse saved evidence. | `Validator`, `UsageStats` |
| [Validation context](sdk/graph/validation-context.md) | Tools for evaluating a criterion and recording its inputs. | [`ValidationContext`](../rustdocs/trust_but_verify/request/struct.ValidationContext.html) |
| [Agent calls](sdk/graph/agent-calls.md) | Prompt construction and typed answers. | [`Context`](../rustdocs/trust_but_verify/request/trait.Context.html), [`Ask`](../rustdocs/trust_but_verify/request/struct.Ask.html) |
| [Agent harness](sdk/graph/agent-harness.md) | Agent configuration, fixed inspection tools, and structured submissions. | [`AgentConfig`](../rustdocs/trust_but_verify/agent/enum.AgentConfig.html) |
| [Storage](sdk/graph/evidence.md) | Where and how evidence is saved. | [`Domain`](../rustdocs/trust_but_verify/domain/trait.Domain.html) |
| [Filesystem evidence storage](sdk/filesystem-storage.md) | Filesystem persistence for SDK evidence records. | [`Domain`](../rustdocs/trust_but_verify/domain/trait.Domain.html), [`FileEvidenceStorage`](../rustdocs/trust_but_verify/storage/struct.FileEvidenceStorage.html) |
| [Sources and inspection](sdk/sources.md) | Project files, discovery, and source-item-to-node mapping. | [`Sources`](../rustdocs/trust_but_verify/sources/struct.Sources.html), `Domain::source_node` |
| [Source outlines](sdk/outlines.md) | Default language selection and ready-to-use source contents. | `OutlineData`, [`OutlineLang`](../rustdocs/trust_but_verify/outline/trait.OutlineLang.html), [`OutlineContent`](../rustdocs/trust_but_verify/outline/struct.OutlineContent.html) |

## Putting it all together

The [Habeas Codicem integration](sdk/habeas.md) defines the `habeas` CLI workflow in
terms of the SDK, providing an example of how to use the domain interface and building
blocks together.

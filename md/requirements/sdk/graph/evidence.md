# Evidence storage

Implement `EvidenceStorage` to load and save SDK records using node identities
as keys. The SDK constructs those records from the callback's judgment and the
observations recorded during validation.

:::{req} evidence_storage
`habeas_sdk::graph` MUST expose the following storage interface:

```rust,ignore
use std::future::Future;

pub trait EvidenceStorage: Graph {
    fn load_evidence<'a>(
        &'a self,
        node: &'a Self::NodeId,
    ) -> impl Future<Output = Result<
        Option<Record<Self::NodeId, Self::JudgmentMetadata>>,
        Self::Error,
    >> + Send + 'a;

    fn store_evidence<'a>(
        &'a self,
        node: &'a Self::NodeId,
        record: &'a Record<Self::NodeId, Self::JudgmentMetadata>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a;
}
```

The node runner MUST use the requested `NodeId` directly for evidence lookup
and persistence. It MUST NOT require a separate evidence-key type or a
procedure name as part of the storage key.
`load_evidence` MUST distinguish an absent record (`Ok(None)`) from a storage
error (`Err`). Storage MUST preserve the record's procedure version,
dependencies, and judgment. The application MUST be able to choose the
persistence mechanism without filesystem paths or a global evidence listing
being required by this interface.
:::

## Load and store evidence through the graph

After reviewing the expiration requirement, Habeas can store its evidence in a
file. Another application might store the same kind of record in a database.
The SDK calls `graph.load_evidence(...)` and `graph.store_evidence(...)` in either
case. The graph implementation chooses where the data lives; the runner does
not need to enumerate every stored record.

The node identity is the evidence key. The runner calls
`graph.load_evidence(&expiration)` to find the saved validation for that node,
and uses the same identity when storing its replacement. There is no separate
procedure name in the lookup key.

The record retains the version of the validation logic in its `procedure`
field. This lets a change to that logic invalidate saved evidence even when
node contents are unchanged; it does not select a second record for the node.
The stored value `*` represents a legacy record that did not capture its
procedure version. It matches any requested version while retaining the
record's dependency checks.
The application chooses how to encode a node identity as a filename or database
key. Storage uses the SDK's `Record` directly so the runner can construct and
inspect it without an application-specific conversion.

## Construct the evidence record

When a procedure finishes, its context can produce a `Record`. This contains the
output and the dependencies supporting it. It also contains a procedure
identity: unchanged source inputs should not keep an old review current after
we change the question the procedure asks.

:::{req} record
`habeas_sdk::graph` MUST define the completed record as follows:

```rust,ignore
pub struct Record<N, M> {
    pub procedure: String,
    pub dependencies: Vec<Dependency<N>>,
    pub output: Judgment<M>,
}
```

The consumer chooses the procedure identity and judgment metadata type, and
selects serialization and storage through the graph's evidence methods. The
SDK MUST preserve the judgment's boolean result, rationale, and metadata.
The SDK MUST construct the dependency list from its captured observations; the
validation callback MUST NOT be required to return a dependency list.
:::

For example, the application can identify its recipe as `requirement-review-v1`:

```rust,ignore
let verdict = graph.validate(&context).await?;
let record = context.finish("requirement-review-v1".into(), verdict);
```

## Remember what a procedure actually reads

Calling `Graph::node_content` directly only resolves a node. A review normally reads
through an `InspectionContext`, which also remembers the dependency:

```rust,ignore
let ctx = validation_context.inspection();
let requirement = ctx.read(&requirement_id)?;
let implementation = ctx.read(&implementation_id)?;
```

The procedure receives the content, while the context remembers the node
identity and its fingerprint, computed through `Fingerprinting::fingerprint`.
The context records an outgoing-edge snapshot when the callback or an awaited
agent builder explicitly requests that selection. Merely creating a context
does not read the subject or all its successors.
A failed read returns an error without inventing a successful observation.
Repeating an identical observation adds no information. Reading the same identity with two different
fingerprints preserves both observations: CI must not silently forget one.

:::{req} dependencies
`habeas_sdk::graph` MUST define captured dependencies as follows:

```rust,ignore
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dependency<N> {
    Node(N, Fingerprint),
    OutgoingEdges(N, Fingerprint),
}
```
:::

:::{req} node_inputs
A record produced by finishing an SDK assessment context MUST include the
identity and fingerprint of every node successfully read through its assessment
context, whether requested by
ordinary code or a model tool. The recorded fingerprint MUST describe the
content supplied by that read.
:::

There is one important boundary in Habeas's model tools: preparing a page of
source text does not mean the model received it. Its delivery adapter records
only inputs on pages actually delivered. The SDK's `observe_read`
method lets such an adapter import the fingerprint of the
snapshot it supplied, without fetching that resource again. The adapter is
responsible for reporting that snapshot accurately. The SDK cannot intercept
arbitrary filesystem or network I/O performed by consumer Rust code.

## Compare the saved observations

CI loads a [saved evidence record](#construct-the-evidence-record)
and asks the SDK to check its freshness:

```rust,ignore
let current = record_is_current(&current_graph, "requirement-review-v1", &record)?;
```

The SDK owns this comparison; there is no application freshness callback.
The application supplies contents, edges, fingerprints, and stored records.
The SDK resolves each saved node or outgoing-edge
dependency and compares its fingerprint with the saved one. A different
concrete procedure identity also makes the record stale; the stored identity
`*` matches any requested version. An unresolvable dependency produces an error;
it cannot establish that the record is current. The current graph must observe
the state that CI intends to check; reusing yesterday's cached source snapshots
would not do that.

:::{req} checking_records
The SDK MUST implement the freshness comparison using graph operations and
fingerprints, without requiring an application freshness callback.
Checking a stored assessment record’s freshness MUST NOT execute its assessment
procedure or invoke a model. The SDK MUST compare the requested procedure
identity and the record’s captured dependencies with their current state. A
procedure-identity mismatch under
{cite}`requirements.sdk.graph.evidence.checking_records.procedure_match` or a
fingerprint mismatch MUST make the record not current; an error resolving a
dependency MUST be returned as an error.
:::

:::{req} checking_records.procedure_match
The SDK MUST consider the procedure identity to match when the stored
`Record::procedure` is `*` or equals the requested identity exactly. Only the
complete stored value `*` is a wildcard; other values MUST be compared
literally, without glob matching. A stored wildcard MUST NOT suppress recorded
dependency comparison or change the saved judgment.
:::

A current record is not necessarily a favorable one. The model may have found
that an implementation fails to meet its requirement. If its dependencies still
match, that negative finding is still current. Habeas decides which judgments
make CI pass; the SDK only answers the freshness question.

[Validation](validation.md) explains when the SDK reuses a stored judgment,
returns a missing or stale state, or invokes the callback again.

## Reuse the filesystem storage helper

Habeas can implement `EvidenceStorage` on its application object and delegate
the methods to a filesystem helper. An application using a database implements
the same trait and delegates to its database client. Neither the key nor the
record has to implement storage methods.

See [Filesystem evidence storage](../filesystem-storage.md) for the helper's
API, key mapping, serialization, and load/store behavior.

:::{req} evidence_storage.filesystem
The SDK MUST supply a filesystem-backed evidence-storage helper to which an
`EvidenceStorage` implementation can delegate its `load_evidence` and
`store_evidence` methods under {cite}`requirements.sdk.graph.evidence.evidence_storage`.
:::

# Evidence storage

Implement `EvidenceStorage` to load and save SDK records using node identities
as keys. The SDK constructs those records from the callback's judgment and the
observations recorded during validation.

:::{req} evidence_storage
The application implements this trait from `habeas_sdk::graph`:

```rust,ignore
use std::future::Future;

pub trait EvidenceStorage: Graph {
    /// Look up a node's record. Ok(None) means absent; Err means storage failed.
    /// The node identity is the key, with no separate procedure-name key.
    fn load_evidence<'a>(
        &'a self,
        node: &'a Self::NodeId,
    ) -> impl Future<Output = Result<
        Option<Record<Self::NodeId, Self::JudgmentMetadata, Self::EdgeLabel>>,
        Self::Error,
    >> + Send + 'a;

    /// Save the complete record, preserving its procedure version,
    /// dependencies, and judgment.
    fn store_evidence<'a>(
        &'a self,
        node: &'a Self::NodeId,
        record: &'a Record<Self::NodeId, Self::JudgmentMetadata, Self::EdgeLabel>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a;
}
```

The runner passes the requested node identity directly to these methods.
The application chooses the persistence mechanism; the interface requires
neither filesystem paths nor a global listing of evidence.
:::

## Load and store evidence through the graph

After reviewing the expiration requirement, Habeas can store its evidence in a
file. Another application might store the same kind of record in a database.
The SDK calls `graph.load_evidence(...)` and `graph.store_evidence(...)` in either
case. The graph implementation chooses where the data lives; the runner does
not need to enumerate every stored record.

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
```rust,ignore
/// The completed validation of one node, defined in habeas_sdk::graph.
pub struct Record<N, M, L = ()> {
    /// Application-chosen version of the validation rules.
    pub procedure: String,
    /// Observations captured by the SDK; the callback need not assemble these.
    pub dependencies: Vec<Dependency<N, L>>,
    /// Preserve the result, rationale, and application metadata as returned.
    pub output: Judgment<M>,
}
```

The application chooses the metadata type, serialization, and storage through
the graph's evidence methods. The SDK constructs `dependencies` from its trace.
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
The context records the source, label, and target-set fingerprint when the
callback or an awaited agent builder explicitly requests that selection. Merely creating a context
does not read the subject or all its successors.
A failed read returns an error without inventing a successful observation.
Repeating an identical observation adds no information. Reading the same identity with two different
fingerprints preserves both observations: CI must not silently forget one.

:::{req} dependencies
```rust,ignore
/// Observations supporting a judgment, defined in habeas_sdk::graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dependency<N, L = ()> {
    Node(N, Fingerprint),
    OutgoingEdges(N, L, Fingerprint),
}
```
:::

:::{req} node_inputs
Finishing an SDK assessment context includes every successful node read in the
record, whether ordinary code or a model tool requested it. Each dependency
contains the node identity and the fingerprint of the contents that read supplied.
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
The SDK checks freshness itself, using graph operations and fingerprints. It
requires no application freshness callback and runs neither the assessment
procedure nor a model. It compares the requested procedure identity and the
record's dependencies with their current state. A
procedure-identity mismatch under
{cite}`requirements.sdk.graph.evidence.checking_records.procedure_match` or a
fingerprint mismatch makes the record stale. A dependency-resolution error is
returned as an error.
:::

:::{req} checking_records.procedure_match
The procedure identity matches when the stored `Record::procedure` is `*` or
equals the requested identity exactly. Only the complete stored value `*` is a
wildcard; other values compare literally, without glob matching. A wildcard
leaves dependency comparison and the saved judgment unchanged.
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
The SDK supplies a filesystem-backed helper for the `load_evidence` and
`store_evidence` methods under
{cite}`requirements.sdk.graph.evidence.evidence_storage`. An application can
delegate to that helper when implementing `EvidenceStorage`.
:::

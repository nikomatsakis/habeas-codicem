# Evidence storage

The domain provides storage for each verification's evidence. A record belongs
to a source node and a `CriterionPath`, so several criteria can assess the same
contents without replacing one another's answers.

The SDK constructs the record from the completed result and the observations
captured during execution. Storage is a required domain capability; the
application chooses its backend.

## Look up a source and criterion

:::{req} evidence_storage
```rust,ignore
use std::future::Future;

// Defined in habeas_sdk::graph.
pub trait EvidenceStorage: Graph {
    /// Ok(None) means absent; Err means loading failed.
    fn load_evidence(
        &self,
        source: &Self::NodeId,
        criterion: &CriterionPath,
    ) -> impl Future<Output = Result<
        Option<Record<Self::NodeId, Self::EdgeLabel>>,
        Self::Error,
    >> + Send;

    /// Atomically replace only this source/criterion pair's complete record.
    fn store_evidence(
        &self,
        source: &Self::NodeId,
        criterion: &CriterionPath,
        record: &Record<Self::NodeId, Self::EdgeLabel>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}
```

The runner passes the source identity and criterion path to both methods. The
key preserves both parts: source paths `["a"]` and `["a", "b"]` with criteria
`["b", "c"]` and `["c"]` identify different records, even though flattening each
pair would produce the same segments.

Different criteria of a source have independent records. A revision change
invalidates the same criterion's record rather than creating another key.
The interface requires neither filesystem paths nor global evidence listing.

Successful loads observe either an old or new complete record, or absence
before the first save. Successful competing saves leave one complete record;
its revision, dependencies, and output all belong to that save. No ordering
between competing saves is guaranteed.
:::

An application can use files, a database, or an in-memory store. A memory backend
still implements the required storage interface. [Filesystem evidence
storage](../filesystem-storage.md) supplies a JSON-backed helper.

## Retain the completed result and its observations

The validation's `Ok` type is serializable and deserializable. The stored record
keeps its output as a JSON value so freshness can be checked before decoding it
as the current validation's result type.

:::{req} record
```rust,ignore
// Defined in habeas_sdk::graph.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Record<N, L = ()> {
    /// Application-chosen revision of this criterion's review rules.
    pub revision: String,
    /// Observations captured by the SDK.
    pub dependencies: Vec<Dependency<N, L>>,
    /// Serialized Validation::Ok; interpretation belongs to the application.
    pub output: serde_json::Value,
}
```

The SDK constructs the dependencies from execution's trace and encodes the
completed result using its Serde JSON representation; there is no required
boolean, rationale, or metadata shape. `Record`, `Dependency`, and
`Fingerprint` support Serde when their type parameters do. The graph's node
identities and edge labels need not implement Serde unless its storage backend
requires those bounds.
:::

## Decode only a current answer

:::{req} record.output
`verify` serializes a completed `Validation::Ok` before saving its record.
`check` and `verify` first compare a loaded record's revision and dependencies.
Only a current record's output is decoded into the requested `Validation::Ok`.
A changed revision can therefore make an old result stale even if that old
result would not decode as the new result type.

Malformed stored records are storage errors. An output that cannot decode for a
current revision is an output error, not missing evidence or a reason to execute
validation again. A different output encoding requires a different revision
under {cite}`requirements.sdk.graph.validation.procedure_identity`.
:::

The SDK does not infer validity from the returned Rust value. A completed
negative assessment is saved and reused in the same way as a positive one.
Applications sharing a source/criterion key and revision agree on its result
encoding and meaning.

## Remember what a procedure actually reads

Calling `Graph::node_content` directly resolves a node. Reading through the
context's inspection interface also records the observation:

```rust,ignore
let inspection = cx.inspection();
let requirement = inspection.read(&requirement_id)?;
let helper = inspection.read(&helper_id)?;
```

The caller receives contents while the context retains the source identity and
its fingerprint. A source node's identity alone does not make its contents an
input. An outgoing selection becomes an observation when the procedure or an
awaited builder explicitly asks for that label.

:::{req} dependencies
```rust,ignore
// Defined in habeas_sdk::graph.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Dependency<N, L = ()> {
    Node(N, Fingerprint),
    OutgoingEdges(N, L, Fingerprint),
}
```
:::

:::{req} node_inputs
Finishing a validation includes every successful tracked node read in its
record, whether ordinary code or a model tool requested it. Each observation
contains the node identity and the fingerprint of the contents actually supplied.

A failed read returns an error without inventing a successful observation.
Repeating an identical observation adds no information. Reading the same
identity with two different fingerprints preserves both observations.
:::

Prepared source pages are not observations until delivered. The SDK's
`observe_read` method lets a delivery adapter report the snapshot it actually
supplied without fetching it again. The adapter is responsible for reporting
that snapshot accurately; arbitrary filesystem or network reads by consumer
code are not automatically tracked.

An empty observed successor set has a fingerprint too. Adding the first test
can invalidate a criterion that previously established the absence of behavioral
obligations; adding the first implementation root can invalidate the analogous
structural assessment. Changing an input observed by one criterion does not
invalidate another criterion that did not observe it.

## Compare the saved observations

The SDK owns freshness comparison. It reads current contents and edge selections
through the domain, computes their fingerprints, and compares them with the
saved observations. Storage does not supply a freshness callback.

:::{req} checking_records
The SDK checks the loaded record's revision first. A mismatch establishes
staleness without resolving saved dependencies or decoding its output. Otherwise
it resolves the saved dependencies through graph operations and fingerprints,
comparing each with its saved observation. A fingerprint mismatch establishes
staleness and permits an early return; it need not resolve later dependencies.
An encountered dependency-resolution failure returns an error.

Checking executes no validation or model and writes no evidence. It uses the
current state that the caller intends to inspect, rather than reusing source
snapshots from an earlier invocation without establishing that they remain
current.
:::

:::{req} checking_records.procedure_match
The revision matches when the stored `Record::revision` is `*` or equals the
requested revision exactly. Only the complete stored value `*` is a wildcard;
other values compare literally. A wildcard leaves dependency comparison and
result decoding unchanged. It matches revisions for the selected source and
criterion only, and does not supply evidence for other criteria.
:::

The wildcard represents a supported legacy record that did not capture its
review-rule version. It does not certify a criterion that was never assessed.
Habeas chooses how to map its legacy assessments to the corresponding criteria.

A current record need not contain a favorable assessment. The SDK answers whether
its answer can be reused; Habeas owns its acceptance policy.

## Reuse the filesystem storage helper

:::{req} evidence_storage.filesystem
The SDK supplies a filesystem-backed helper for the methods under
{cite}`requirements.sdk.graph.evidence.evidence_storage`. The domain can delegate
to that helper. Its mapping distinguishes the source path from the criterion
path and preserves segment boundaries under
{cite}`requirements.sdk.filesystem-storage.keys`.
:::

[Validation](validation.md) defines `verify` and `check`. [Filesystem evidence
storage](../filesystem-storage.md) defines the helper and its encoding.

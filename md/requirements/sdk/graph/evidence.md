# Storage

The domain provides storage for each verification's
{term}`requirements.sdk.graph.evidence.verification_evidence`. A record belongs
to a source node and a `CriterionPath`, so several criteria can assess the same
contents without replacing one another's answers.

:::{definition} verification_evidence
:phrase: evidence

A {term}`requirements.sdk.graph.validation.criterion`'s completed answer,
its review-rule revision, and the recorded inputs used to produce it.
It belongs to the criterion's source node and criterion path. `Fingerprinted`
carries the typed answer and these observations; `Record` holds the encoded
answer, revision, and dependencies for storage under that source/criterion key.
:::

`Validator::verify` returns a `Fingerprinted` containing the completed result and the
observations captured during execution. The application calls `store_evidence`
to save it, and `load_evidence` to retrieve a typed answer later. Storage is a
required domain capability; the application chooses its backend.

## Refresh evidence

`Validator::refresh_evidence` implements the usual reuse workflow. It loads a
previous answer, checks its review-rule revision and recorded inputs, and
returns it when both still match. Otherwise it evaluates the criterion and
saves the new answer. Its definition uses the granular methods described below:

:::{req} refresh_evidence
```rust,ignore
use std::future::Future;

// Method on Validator<D>.
/// Reuse compatible, current evidence, or evaluate and save a new answer.
/// Errors from loading, decoding, checking, execution, or saving propagate.
pub fn refresh_evidence<V: Criterion<D>>(
    &self,
    criterion: V,
) -> impl Future<Output = Result<Fingerprinted<D, V::Ok>, ValidationError<D::Error>>> + Send {
    async move {
        if let Some(saved) = self.load_evidence(&criterion).await? {
            if procedure_matches(saved.revision(), &criterion.revision())
                && self.check_inputs(&saved).map_err(ValidationError::Graph)?
            {
                return Ok(saved);
            }
        }

        let inputs = self.verify(criterion).await?;
        self.store_evidence(&inputs).await?;
        Ok(inputs)
    }
}
```

Loading and decoding follow {cite}`requirements.sdk.graph.evidence.check_node`
and {cite}`requirements.sdk.graph.evidence.record.output`. Revision and input
comparison follow
{cite}`requirements.sdk.graph.evidence.checking_records.procedure_match` and
{cite}`requirements.sdk.graph.evidence.checking_records`. Evaluation follows
{cite}`requirements.sdk.graph.validators.validate_node`; saving follows
{cite}`requirements.sdk.graph.evidence.store_evidence`.
:::

A current negative assessment is reused just like a positive one. A missing
record, an incompatible revision, or changed inputs leads to a new evaluation.
A load or decoding failure is reported even when the stored evidence is stale,
as required by the loading contract.

The helper packages this workflow; the individual methods remain available for
applications that need to inspect an old answer, force a new evaluation with
`verify`, or decide when to save a result. Concurrent calls follow the validator's
independent-execution policy; [storage](#look-up-a-source-and-criterion) provides
atomic record replacement.

## Keep the answer with its inputs

`Fingerprinted<D, O>` keeps a criterion's answer together with the observations used to
produce it. It also retains the source identity, criterion path, and revision
needed to save the evidence. `D` supplies the node and edge-label types; `O` is
the criterion's answer type.

:::{req} inputs
```rust,ignore
// Defined in habeas_sdk::graph.
/// A completed answer and the inputs used to produce it.
pub struct Fingerprinted<D: Domain, O> { /* private fields */ }

impl<D: Domain, O> Fingerprinted<D, O> {
    /// The source node this answer concerns.
    pub fn source_node_id(&self) -> &D::NodeId;

    /// The question's path relative to that source.
    pub fn criterion_path(&self) -> &CriterionPath;

    /// The review rules used to produce this answer.
    pub fn revision(&self) -> &str;

    /// Recorded node-content and successor-set observations.
    pub fn dependencies(&self) -> &[Dependency<D::NodeId, D::EdgeLabel>];

    /// Borrow the completed answer.
    pub fn output(&self) -> &O;

    /// Take ownership of the completed answer.
    pub fn into_output(self) -> O;
}
```

`verify` constructs this value from the criterion's completed answer and
execution trace. `load_evidence` reconstructs it from the selected saved record.
Its node identities and edge labels keep their domain-defined types.
:::

## Look up a source and criterion

:::{req} evidence_storage
```rust,ignore
use std::future::Future;

// Defined in habeas_sdk::graph.
pub trait Storage: Graph {
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

The validator passes the source identity and criterion path to both methods. The
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

The criterion's `Ok` type is serializable and deserializable. The storage
interface uses `Record` to hold its encoded answer and input trace.

:::{req} record
```rust,ignore
// Defined in habeas_sdk::graph.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Record<N, L = ()> {
    /// Application-chosen revision of this criterion's review rules.
    pub revision: String,
    /// Observations captured by the SDK.
    pub dependencies: Vec<Dependency<N, L>>,
    /// Serialized Criterion::Ok; interpretation belongs to the application.
    pub output: serde_json::Value,
}
```

Saving encodes the completed result using its Serde JSON representation and
preserves the dependencies captured in `Fingerprinted`; there is no required boolean,
rationale, or metadata shape. `Record`, `Dependency`, and `Fingerprint` support
Serde when their type parameters do. The graph's node
identities and edge labels need not implement Serde unless its storage backend
requires those bounds.
:::

## Encode and decode the answer

:::{req} record.output
`Validator::store_evidence` serializes the answer in `Fingerprinted` before passing
the complete record to storage. A serialization failure returns
`ValidationError::Output` and leaves the existing record unchanged. A storage
failure returns `ValidationError::Graph`.

`Validator::load_evidence` decodes a loaded record's output into the requested
`Criterion::Ok`. An output that cannot decode returns `ValidationError::Output`,
including when the saved revision or inputs are stale. Missing evidence returns
`Ok(None)`; malformed stored records and storage failures return
`ValidationError::Graph`. Revision and input comparison belong to the caller's
reuse workflow under {cite}`requirements.sdk.graph.evidence.procedure_identity`.
:::

The SDK does not infer validity from the returned Rust value. A completed
negative assessment is saved and reused in the same way as a positive one.
Applications sharing a source/criterion key and revision agree on its result
encoding and meaning.

## Remember what a procedure actually reads

:::{definition} dependency
:phrase: dependency

A recorded observation of a node's contents or of the target set of a node's
outgoing edges with a given label. Its `Dependency` value stores the node
identity, the edge label when applicable, and the
{term}`requirements.sdk.graph.fingerprinting.observation_fingerprint` for
later comparison.
:::

:::{definition} dependency_trace
:phrase: dependency trace

The collection of input observations recorded during one criterion execution.
Each is represented by a {term}`requirements.sdk.graph.evidence.dependency`.
Tracked node reads and outgoing-edge selections, node contents supplied in
prompts, and additional contents delivered by tools contribute to this trace.
Discovering a node alone does not record its contents as an input.
:::

Calling `Contents::node_content` directly resolves a node. Reading through the
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
Finishing a verification includes every successful tracked node read in its
`Fingerprinted`, whether ordinary code or a model tool requested it. Each observation
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

## Identify the saved assessment

:::{req} procedure_identity
`load_evidence` selects a saved record using the source node and criterion path
from the supplied `Criterion`. `verify` captures those values and the criterion's
revision in its returned `Fingerprinted`. The pair of source and criterion selects a
record; its saved revision identifies the review rules used to produce it.

Changing review instructions, deterministic rules, input-selection policy, or
result encoding in a way that invalidates an old answer requires a changed
revision. This is independent of model selection and the agent's context-prefix
cache fingerprint. A stored legacy revision `*` retains the matching behavior of
{cite}`requirements.sdk.graph.evidence.checking_records.procedure_match`.
:::

## Load and store evidence

Loading returns the saved answer and its input trace, if a record exists.
The caller can then compare the revision and inputs or inspect the answer.

:::{req} check_node
```rust,ignore
// Method on Validator<D>.
/// Load this criterion's evidence and decode its answer as V::Ok.
/// Ok(None) means no saved record exists.
pub fn load_evidence<V: Criterion<D>>(
    &self,
    criterion: &V,
) -> impl Future<Output = Result<Option<Fingerprinted<D, V::Ok>>, ValidationError<D::Error>>> + Send;
```

`load_evidence` captures the source identity and criterion path while constructing
its future. It retains no reference to the criterion value while storage is
pending, so the criterion needs no `Sync` bound. It loads the complete record and
decodes its output under {cite}`requirements.sdk.graph.evidence.record.output`.
Revision and input comparison are separate steps.
:::

:::{req} store_evidence
```rust,ignore
// Method on Validator<D>.
/// Encode the answer and atomically replace this source/criterion's record.
pub fn store_evidence<O: serde::Serialize>(
    &self,
    inputs: &Fingerprinted<D, O>,
) -> impl Future<Output = Result<(), ValidationError<D::Error>>> + Send;
```

The method saves under the source identity and criterion path captured in
`Fingerprinted`, preserving its revision, input trace, and completed answer. Encoding follows
{cite}`requirements.sdk.graph.evidence.record.output`; atomic replacement follows
{cite}`requirements.sdk.graph.evidence.evidence_storage`. The result type needs
no `Sync` bound: the method encodes the answer and captures the record and
storage key before constructing the storage future.
:::

## Compare the saved observations

:::{definition} input_freshness
:phrase: input freshness

Whether observing each recorded {term}`requirements.sdk.graph.evidence.dependency`
in the state being checked produces a fingerprint matching its saved
fingerprint. Fresh inputs do not imply a favorable answer. Reusing evidence
also depends on compatibility with the criterion's current review-rule
revision, which is checked separately.
:::

`Validator::check_inputs` checks
{term}`requirements.sdk.graph.evidence.input_freshness`. It reads current
contents and edge selections through the
domain, computes their fingerprints, and compares them with the observations in
`Fingerprinted`. Revision comparison is a separate operation: the caller has the
current criterion and decides which review rules remain compatible.

:::{req} checking_records
```rust,ignore
// Method on Validator<D>.
/// Compare each recorded input with the current domain state.
/// Return true when every fingerprint still matches, including an empty trace.
pub fn check_inputs<O>(
    &self,
    inputs: &Fingerprinted<D, O>,
) -> Result<bool, D::Error>;
```

The method resolves the recorded dependencies through graph operations and
fingerprinting, comparing each with its saved observation. A fingerprint
mismatch returns `Ok(false)` and permits an early return; it need not resolve
later dependencies. An encountered dependency-resolution failure returns an
error.

It uses the current state that the caller intends to inspect, rather than
reusing source snapshots from an earlier invocation without establishing that
they remain current. These reads are separate from a criterion execution's
dependency trace.

Freshness checking has a separate observation scope from execution. Reads used
only to check old evidence do not become dependencies of the new result. A
checked snapshot can be reused if it still represents the current state; when
execution explicitly reads or delivers it, its fingerprint enters the new trace.
:::

:::{req} checking_records.procedure_match
```rust,ignore
// Defined in habeas_sdk::graph.
/// Compare the saved review-rule revision with the currently requested revision.
pub fn procedure_matches(stored: &str, requested: &str) -> bool;
```

The revision matches when the stored `Record::revision` is `*` or equals the
requested revision exactly. Only the complete stored value `*` is a wildcard;
other values compare literally. A wildcard leaves dependency comparison and
result decoding unchanged. It matches revisions for the selected source and
criterion only, and does not supply evidence for other criteria.
:::

The wildcard represents a supported legacy record that did not capture its
review-rule version. It does not certify a criterion that was never assessed.
Habeas chooses how to map its legacy assessments to the corresponding criteria.

A current record need not contain a favorable assessment. The SDK supplies the
comparisons; Habeas owns its reuse and acceptance policies.

## Reuse the filesystem storage helper

:::{req} evidence_storage.filesystem
The SDK supplies a filesystem-backed helper for the methods under
{cite}`requirements.sdk.graph.evidence.evidence_storage`. The domain can delegate
to that helper. Its mapping distinguishes the source path from the criterion
path and preserves segment boundaries under
{cite}`requirements.sdk.filesystem-storage.keys`.
:::

[Criteria definition](validation.md) defines the criterion, and
[Validators](validators.md) explains how to run it.
The methods above compose verification with this
chapter's evidence workflow.
[Filesystem evidence
storage](../filesystem-storage.md) defines the helper and its encoding.

# Storage

The domain provides storage for each verification's
{term}`requirements.sdk.graph.evidence.verification_evidence`. A record belongs
to a source node and a `CriterionPath`, so several criteria can assess the same
contents without replacing one another's answers.

:::{definition} verification_evidence
:phrase: evidence

*Evidence* is a {term}`requirements.sdk.graph.criteria.criterion`'s completed
answer, its criterion version, and the recorded inputs used to produce it.
It belongs to the criterion's source node and criterion path.
[`Fingerprinted`](../../../rustdocs/trust_but_verify/graph/struct.Fingerprinted.html)
carries the typed answer and these observations;
[`Record`](../../../rustdocs/trust_but_verify/graph/struct.Record.html) holds the
encoded answer, version, and dependencies for storage under that
source/criterion key.
:::

`verify` returns a `Fingerprinted` containing the completed result and the
observations captured during execution. The application calls `store_evidence`
to save it, and `load_evidence` to retrieve a typed answer later. Storage is a
required domain capability; the application chooses its backend.

## Refresh evidence

`refresh_evidence` implements the usual reuse workflow. It loads a
previous answer, checks its criterion version and recorded inputs, and
returns it when both still match. Otherwise it evaluates the criterion and
saves the new answer. The validator method is declared under {cite}`requirements.sdk.graph.running-criteria.refresh_evidence`.

:::{req} refresh_evidence
The validator MUST load the saved answer, check version compatibility and input freshness, and return a current answer immediately. If the record is missing, incompatible, or stale, it MUST verify the criterion and store the completed result before returning it. Errors from any step propagate to the caller.

Loading and decoding follow {cite}`requirements.sdk.graph.evidence.check_node`
and {cite}`requirements.sdk.graph.evidence.record.output`. Revision and input
comparison follow
{cite}`requirements.sdk.graph.evidence.checking_records.version_match` and
{cite}`requirements.sdk.graph.evidence.checking_records`. Evaluation follows
{cite}`requirements.sdk.graph.running-criteria.validate_node`; saving follows
{cite}`requirements.sdk.graph.evidence.store_evidence`.
:::

A current negative assessment is reused just like a positive one. A missing
record, an incompatible version, or changed inputs leads to a new evaluation.
A load or decoding failure is reported even when the stored evidence is stale,
as required by the loading contract.

The individual validator methods also support
applications that need to inspect an old answer, force a new evaluation with
`verify`, or decide when to save a result. Concurrent validators run independently. [Storage](#look-up-a-source-and-criterion)
provides atomic record replacement.

## Keep the answer with its inputs

`Fingerprinted<D, O>` keeps a criterion's answer together with the observations used to
produce it. It also retains the source identity, criterion path, and version
needed to save the evidence. `D` supplies the node and edge-label types; `O` is
the criterion's answer type.

:::{req} recorded_criterion_version
`verify` always captures `Some(criterion.version())`. When `load_evidence` loads
an older answer with no known version, `Fingerprinted::version()` returns `None`.
`Record::version` holds the same optional value. In the serialized `revision`
field, a concrete string represents `Some(version)` and `"*"` represents `None`.
:::

:::{req} inputs
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Keeps a typed output with its source, criterion, version, and observed dependencies. `output` borrows the answer and `into_output` consumes the wrapper.

`verify` constructs this value from the criterion's completed answer and
execution trace. `load_evidence` reconstructs it from the selected saved record.
Its dependencies use `NodeId<D>` handles from the current cache and domain-defined edge labels.

```rust,ignore
pub struct Fingerprinted<D: Domain, O> { /* private fields */ }

impl<D: Domain, O> Fingerprinted<D, O> {
    pub fn source_node_id(&self) -> &NodeId<D>;
    pub fn criterion_path(&self) -> &CriterionPath;
    pub fn version(&self) -> Option<&CriterionVersion>;
    pub fn dependencies(&self) -> &[Dependency<NodeId<D>, D::EdgeLabel>];
    pub fn output(&self) -> &O;
    pub fn into_output(self) -> O;
}
```
:::

## Look up a source and criterion

:::{req} evidence_storage
{cite}`requirements.sdk.domain`

`trust_but_verify::domain::Domain` provides evidence persistence for the
{term}`requirements.sdk.application_domain`:

```rust,ignore
use std::future::Future;

// Defined in trust_but_verify::domain.
pub trait Domain: Clone + Send + Sync {
    // ...

    /// Ok(None) means absent; Err means loading failed.
    fn load_evidence(
        &self,
        source: &NodeId<Self>,
        criterion: &CriterionPath,
    ) -> impl Future<Output = Result<
        Option<Record<Self::NodeData, Self::EdgeLabel>>,
        Self::Error,
    >> + Send;

    /// Atomically replace only this source/criterion pair's complete record.
    fn store_evidence(
        &self,
        source: &NodeId<Self>,
        criterion: &CriterionPath,
        record: &Record<Self::NodeData, Self::EdgeLabel>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    // ...
}
```

The SDK passes the source identity and criterion path to both methods. The
key preserves both parts: source paths `["a"]` and `["a", "b"]` with criteria
`["b", "c"]` and `["c"]` identify different records, even though flattening each
pair would produce the same segments.

Different criteria of a source have independent records. A version change
invalidates the same criterion's record rather than creating another key.

Successful loads observe either an old or new complete record, or absence
before the first save. Successful competing saves leave one complete record;
its version, dependencies, and output all belong to that save. No ordering
between competing saves is guaranteed.
:::

An application can use files, a database, or an in-memory store. [Filesystem evidence
storage](../filesystem-storage.md) supplies a JSON-backed helper.

## Retain the completed result and its observations

The criterion's `Ok` type is serializable and deserializable. The storage
interface uses `Record` to hold its encoded answer and input trace.

:::{req} persisted_dependencies
Before saving a record, the SDK replaces each dependency's `NodeId<D>` with the corresponding `D::NodeData`. When loading a record into an execution or checking it against a domain, the SDK interns each dependency's data in that domain's cache. Both conversions preserve the dependency kind, label, and observed fingerprint.
:::

:::{example} reassigned-handles
{cite}`requirements.sdk.graph.evidence.persisted_dependencies`
{cite}`requirements.sdk.graph.validation-context.snapshot`

A first run assigns handle `7` to requirement `R` and records a query for its implementation edges. The next run assigns `7` to requirement `S` and `12` to `R`. Loading the record resolves its dependency to `12`. If `R` has the same implementation targets, their edge fingerprint still matches, even if their handles also changed. Removing an implementation target changes the fingerprint and makes the saved observation stale.
:::

:::{req} record
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Saving encodes the completed result using its Serde JSON representation and
preserves the dependencies captured in `Fingerprinted`.
`Record`, `Dependency`, and `Fingerprint` support
Serde when their type parameters do. The graph's node
identities and edge labels need not implement Serde unless its storage backend
requires those bounds.

```rust,ignore
#[non_exhaustive]
pub struct Record<N, L = ()> {
    pub version: Option<CriterionVersion>,
    pub dependencies: Vec<Dependency<N, L>>,
    pub output: Value,
}

impl<N, L> Record<N, L> {
    pub fn new(
        version: Option<CriterionVersion>,
        dependencies: Vec<Dependency<N, L>>,
        output: Value,
    ) -> Self;
}
```

Trait implementations

```rust,ignore
impl<N: Clone, L: Clone> Clone for Record<N, L>;
impl<N: Debug, L: Debug> Debug for Record<N, L>;
impl<'de, N, L> Deserialize<'de> for Record<N, L> where N: Deserialize<'de>, L: Deserialize<'de>;
impl<N, L> Serialize for Record<N, L> where N: Serialize, L: Serialize;
```
:::

## Encode and decode the answer

:::{req} record.output
`store_evidence` serializes the answer in `Fingerprinted` before passing
the complete record to storage. A serialization failure returns
`ValidationError::Output` and leaves the existing record unchanged. A storage
failure returns `ValidationError::Graph`.

`load_evidence` decodes a loaded record's output into the requested
`Criterion::Ok`. An output that cannot decode returns `ValidationError::Output`,
including when the saved version or inputs are stale. Missing evidence returns
`Ok(None)`; malformed stored records and storage failures return
`ValidationError::Graph`. Version and input comparison belong to the caller's
reuse workflow under {cite}`requirements.sdk.graph.evidence.procedure_identity`.
:::

A completed negative assessment is saved and reused in the same way as a positive one.
Applications sharing a source/criterion key and version agree on its result
encoding and meaning.

## Remember what a procedure actually reads

:::{definition} dependency
:phrase: dependency

A *dependency* is a recorded observation of a node's contents or of the target
set of a node's outgoing edges with a given label. Its
[`Dependency`](../../../rustdocs/trust_but_verify/graph/enum.Dependency.html) value
stores the node identity, the edge label when applicable, and the
{term}`requirements.sdk.graph.observation_fingerprint` for
later comparison.
:::

:::{definition} dependency_trace
:phrase: dependency trace

A *dependency trace* is the collection of input observations recorded during
one criterion execution.
Each is represented by a {term}`requirements.sdk.graph.evidence.dependency`.
Tracked node reads and outgoing-edge selections, node contents supplied in
prompts, and additional contents delivered by tools contribute to this trace.
Discovering a node alone does not record its contents as an input.
:::

Calling `Domain::node_content` directly resolves a node. Reading through the
context's inspection interface also records the observation:

```rust,ignore
let requirement = cx.read(&requirement_id)?;
let helper = cx.read(&helper_id)?;
```

The caller receives contents while the context retains the source identity and
its fingerprint. An outgoing selection becomes an observation when the procedure
or an awaited builder explicitly asks for that label.

:::{req} dependencies
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

A node dependency records a node and its content fingerprint. An outgoing-edge dependency records the source node, label, and target-set fingerprint.

```rust,ignore
pub enum Dependency<N, L = ()> {
    Node(N, Fingerprint),
    OutgoingEdges(N, L, Fingerprint),
}
```

Trait implementations

```rust,ignore
impl<N: Clone, L: Clone> Clone for Dependency<N, L>;
impl<N: Debug, L: Debug> Debug for Dependency<N, L>;
impl<'de, N, L> Deserialize<'de> for Dependency<N, L> where N: Deserialize<'de>, L: Deserialize<'de>;
impl<N: PartialEq, L: PartialEq> PartialEq for Dependency<N, L>;
impl<N, L> Serialize for Dependency<N, L> where N: Serialize, L: Serialize;
impl<N: Eq, L: Eq> Eq for Dependency<N, L>;
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

The SDK records a prepared source page when its contents are delivered, using
the fingerprint of that retained snapshot.

An empty observed successor set has a fingerprint too. Adding the first edge
for that label changes its fingerprint. Each criterion's input freshness is
checked against its own recorded dependencies.

## Identify the saved assessment

:::{req} procedure_identity
`load_evidence` selects a saved record using the source node and criterion path
from the supplied `Criterion`. `verify` captures those values and the criterion's
version in its returned `Fingerprinted`. The pair of source and criterion selects a
record; its saved version identifies the review rules used to produce it.

Changing review instructions, deterministic rules, input-selection policy, or
result encoding in a way that invalidates an old answer requires a changed
version. This is independent of model selection.
A stored legacy version `*` retains the matching behavior of
{cite}`requirements.sdk.graph.evidence.checking_records.version_match`.
:::

## Load and store evidence

Loading returns the saved answer and its input trace, if a record exists.
The caller can then compare the version and inputs or inspect the answer.

:::{req} check_node
The method is declared under {cite}`requirements.sdk.graph.running-criteria.load_evidence`.

`load_evidence` captures the source identity and criterion path while constructing
its future. It retains no reference to the criterion value while storage is
pending, so the criterion needs no `Sync` bound. It loads the complete record and
decodes its output under {cite}`requirements.sdk.graph.evidence.record.output`.
Version and input comparison are separate steps.
:::

:::{req} store_evidence
The method is declared under {cite}`requirements.sdk.graph.running-criteria.store_evidence`.

The method saves under the source identity and criterion path captured in
`Fingerprinted`, preserving its version, input trace, and completed answer. Encoding follows
{cite}`requirements.sdk.graph.evidence.record.output`; atomic replacement follows
{cite}`requirements.sdk.graph.evidence.evidence_storage`. The result type needs
no `Sync` bound: the method encodes the answer and captures the record and
storage key before constructing the storage future.
:::

## Compare the saved observations

:::{definition} input_freshness
:phrase: input freshness

*Input freshness* describes whether observing each recorded
{term}`requirements.sdk.graph.evidence.dependency` in the state being checked
produces a fingerprint matching its saved fingerprint. Fresh inputs do not
imply a favorable answer. Reusing evidence
also depends on compatibility with the criterion's current review-rule
version, which is checked separately.
:::

`check_inputs` checks
{term}`requirements.sdk.graph.evidence.input_freshness`. It reads current
contents and edge selections through the
domain, computes their fingerprints, and compares them with the observations in
`Fingerprinted`. Revision comparison is a separate operation: the caller has the
current criterion and decides which review rules remain compatible.

:::{req} checking_records
The method is declared under {cite}`requirements.sdk.graph.running-criteria.check_inputs`.

The method resolves the recorded dependencies through graph operations and
fingerprinting, comparing each with its saved observation. A fingerprint
mismatch returns `Ok(false)` and permits an early return; it need not resolve
later dependencies. An encountered dependency-resolution failure returns an
error.

The method observes the current state that the caller intends to inspect.
Freshness checking has a separate observation scope from criterion execution.
Checked observations enter the new trace when execution explicitly reads
or delivers them.
A checked snapshot can be reused if it still represents the current state.
:::

:::{req} record.is_current
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

`record.is_current(graph, version)` returns `Ok(false)` immediately if the criterion version is incompatible. For a compatible version, it checks each saved dependency against the graph, stopping at the first changed fingerprint or domain error. An empty dependency list passes.

```rust,ignore
impl<N, L> Record<N, L> {
    pub fn is_current<D>(
        &self,
        graph: &D,
        version: &CriterionVersion,
    ) -> Result<bool, D::Error>
    where
        D: Domain<NodeData = N, EdgeLabel = L>;
}
```
:::

:::{req} checking_records.version_match
Compare the saved version with the current criterion using
`criterion.version().is_compatible_with(saved.version())` on a `Fingerprinted`,
or `criterion.version().is_compatible_with(record.version.as_ref())` on a `Record`.
A known saved version must match exactly. `None` passes the version check.
For example, a saved `v*` matches only a requested `v*`.
The wildcard applies only to the version of the selected source and criterion.
Reusing the answer still requires decoding its output and checking its dependencies.
:::

The wildcard represents a supported legacy record that did not capture its
criterion version.

## Reuse the filesystem storage helper

:::{req} evidence_storage.filesystem
The SDK supplies a filesystem-backed helper for the methods under
{cite}`requirements.sdk.graph.evidence.evidence_storage`. The domain can delegate
to that helper. Its mapping distinguishes the source path from the criterion
path and preserves segment boundaries under
{cite}`requirements.sdk.filesystem-storage.keys`.
:::

[Filesystem evidence storage](../filesystem-storage.md) defines the helper
and its encoding.

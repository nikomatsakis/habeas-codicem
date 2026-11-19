# Validation

A node supplies contents. A validation asks a particular question about those
contents and selects the inputs needed to answer it. Several validations can
share a source node while keeping their results and dependencies separate.

For example, Habeas can assess a requirement's text, its behavioral tests, and
its structural obligations independently. The SDK knows how to execute and
reuse these assessments; Habeas defines the questions and interprets the answers.

## Give each criterion a path

The graph identifies the source with a `NodeId` and its stable `NodePath`.
A validation identifies its question with a path relative to that source.

:::{req} criterion_path
```rust,ignore
// Defined in habeas_sdk::graph.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct CriterionPath {
    pub segments: Vec<String>,
}
```

A criterion path preserves segment order and boundaries. An empty path is valid,
as are empty segments. Its identity is stable across runs and changes to input
contents. Different questions that need independent records have different
paths; nested paths can distinguish several questions within one area.
:::

For example, a requirement might have criteria `text`, `behavior`, and
`structure`. A source item's citation checks might use
`["citations", "requirements.auth.expiration"]`.

The complete identity is the pair of source node and criterion path. Appending
criterion segments to a source `NodePath` gives a useful display path; storage
preserves the boundary between the two paths so different pairs cannot collide.
The [evidence chapter](evidence.md) defines that lookup.

## Define the question as a value

A validation value can carry the source identity, review instructions, and other
application state. It implements the execution procedure separately from the
domain, which always provides graph access, fingerprinting, and evidence storage.

:::{req} check_procedure
```rust,ignore
use serde::{Serialize, de::DeserializeOwned};
use std::future::Future;

// Defined in habeas_sdk::graph.
pub enum ValidationError<E> {
    Graph(E),
    NoAgentConfigured,
    Agent(String),
    Output(serde_json::Error),
}

pub trait Validation<D: Domain>: Send + Sized {
    /// The completed answer, including any negative assessment.
    /// The SDK serializes it for storage and reconstructs it when reused.
    /// Its Serde JSON representation must reconstruct the completed answer.
    type Ok: Serialize + DeserializeOwned + Send;

    fn source_node_id(&self) -> D::NodeId;
    fn criterion_path(&self) -> CriterionPath;

    /// Version of these review rules and the result's encoding.
    /// Change it when an old answer should no longer be reused.
    fn revision(&self) -> String;

    /// Execute this criterion with a private dependency trace.
    /// Contents are read only when explicitly selected through cx.
    fn execute(
        self,
        cx: &mut ValidationContext<'_, D>,
    ) -> impl Future<Output = Result<Self::Ok, ValidationError<D::Error>>> + Send;
}
```

Obtaining the source identity, criterion path, and revision does not execute the
criterion, invoke an agent, or record source-content dependencies. These values
identify the same question for checking and execution. Parameters that change
which question is being asked belong in its identity or revision.
:::

The application decides which validations to submit. There is no requirement
that every graph node has a criterion or that the SDK enumerates them.

## Share a validator, keep observations separate

:::{req} validator
```rust,ignore
// Defined in habeas_sdk::graph.
/// Shared domain access, caches, and optional agent configuration.
/// Evidence storage is required through Domain.
pub struct Validator<'d, D: Domain> { /* private fields */ }

impl<'d, D: Domain> Validator<'d, D> {
    pub fn new(domain: &'d D) -> Self;

    /// Configure the contexts created for execution.
    pub fn with_agent(
        self,
        agent: &'d dyn Agent<D>,
        renderer: &'d dyn AgentInputRenderer<D>,
    ) -> Self;
}
```

`Agent` and `AgentInputRenderer` are supplied by `habeas_sdk::procedure`.
Independent verifications share a validator through `&self`. Cached observations
can be reused when they still represent the state being checked. Each execution
has its own context and trace; dependencies and agent conversation state never
leak between different executions. Operations within one execution can share
its synchronized trace through context inspection and agent calls.

Concurrent verifications of the same source and criterion run independently,
each with its own context and trace. They can both execute when evidence is
missing or stale; the validator does not combine their executions.
:::

Here are illustrative application-defined validation values for one requirement:

```rust,ignore
let validator = Validator::new(&domain);

let text = validator.verify(TextualQuality {
    requirement: expiration.clone(),
}).await?;
let behavior = validator.verify(BehavioralAdequacy {
    requirement: expiration.clone(),
}).await?;
let structure = validator.verify(StructuralConformance {
    requirement: expiration,
}).await?;
```

Each value returns its own criterion path and result type. Running one need not
replace or re-execute the others.

## Select inputs through the context

The context identifies the validation's source node. A criterion path identifies
the record, not another source whose contents should be inserted into a prompt.

:::{req} root_inputs
```rust,ignore
// Methods on ValidationContext<'d, D>.

/// The source node. Obtaining its identity does not read its contents.
pub fn node_id(&self) -> &D::NodeId;

/// Select outgoing edges by label, preserving their order and duplicates.
/// Record this label's target-set fingerprint even when nothing matches.
/// Selecting edges does not read or validate their targets.
pub fn successors(
    &self,
    label: &D::EdgeLabel,
) -> Result<Vec<D::NodeId>, D::Error>
where
    D::EdgeLabel: PartialEq;
```

Labels match by equality. A change to the recorded target set makes prior
evidence stale, including adding an edge to an empty selection. Target contents
become dependencies only through explicit tracked reads or agent insertions.
An edge neither supplies nor requires a positive validation judgment for its target.
:::

For example, a behavioral criterion can select the requirement and its tests:

```rust,ignore
let answer: ExpirationAssessment = cx.ask()
    .context((&review_policy, &project_conventions))
    .context(&question)
    .current_node_contents()
    .successors_content(HabeasEdgeLabel::Tests)
    .await?;
```

A structural criterion can select implementations instead. A textual criterion
can read the declaration and its cited definitions. Each records only the inputs
it actually observes. If deciding which task to perform depends on an empty
selection, inspect that selection through the context as well.

[Agent calls](agent-calls.md) defines prompt construction and typed submissions.
[Context tools](context-tools.md) defines tracked discovery and content reads.
Independent filesystem or network reads are not automatically dependencies;
use tracked operations for information the answer relies on.

:::{req} deterministic_procedures
`Validation::execute` can compute an answer without a configured model provider.
A validation that never invokes the agent interface does not fail because no
agent is configured. Evidence storage is still required.
:::

## Return an answer, including a negative one

The result type belongs to the validation. The SDK preserves it without deciding
whether it is favorable. For applications wanting a boolean and explanation,
it provides this convenient result type:

:::{req} judgment
```rust,ignore
// Defined in habeas_sdk::graph.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Judgment<M> {
    pub result: bool,
    pub rationale: String,
    pub metadata: M,
}
```

A validation can use `Judgment<M>` as its `Ok` type when the metadata satisfies
its bounds, or use another serializable result. `Graph` has no judgment-metadata
associated type. The SDK does not interpret these fields or implement Habeas's
FIXME acknowledgment policy.
:::

A completed assessment finding a gap is an `Ok` value and can be saved and reused.
An `Err` means execution failed and produces no completed record.

## Check or reuse saved evidence

:::{req} procedure_identity
Checking and verification obtain the source node, criterion path, and revision
from the same `Validation` value. There is no node-wide revision resolver.
The pair of source and criterion selects a record; the revision determines
whether that record still answers the current question.

Changing review instructions, deterministic rules, input-selection policy, or
result encoding in a way that invalidates an old answer requires a changed
revision. This is independent of model selection and the agent's context-prefix
cache fingerprint. A stored legacy revision `*` retains the matching behavior of
{cite}`requirements.sdk.graph.evidence.checking_records.procedure_match`.
:::

:::{req} validate_node
```rust,ignore
// Method on Validator<'d, D>.
/// Reuse the current answer, including a negative assessment.
/// Otherwise execute and save the completed record before returning.
pub fn verify<V: Validation<D>>(
    &self,
    validation: V,
) -> impl Future<Output = Result<V::Ok, ValidationError<D::Error>>> + Send;
```

The SDK loads the record for this source and criterion and compares its revision
and dependencies under {cite}`requirements.sdk.graph.evidence.checking_records`.
Current evidence is decoded into `V::Ok` and returned without calling `execute`.
Missing or stale evidence starts a fresh context identifying the source node,
executes the supplied validation, and saves its result and captured dependencies.
Other criteria's records are unaffected.

Freshness checking has a separate observation scope from execution. Reads used
only to check old evidence do not become dependencies of the new result. A
checked snapshot can be reused if it still represents the current state; when
execution explicitly reads or delivers it, its fingerprint enters the new trace.
:::

:::{req} validate_node.errors
Storage, dependency-resolution, output-encoding, and current-output decoding
failures return errors rather than missing or stale states. An execution error
is never stored as a completed result. A serialization failure does not replace
an existing record. A save failure is returned rather than reporting successful
verification.
:::

## Inspect evidence without executing the criterion

CI needs to distinguish missing, stale, and current evidence without running an
agent or replacing records. It supplies the same validation value to `check`:

```rust,ignore
let review = BehavioralAdequacy { requirement: expiration };
let state = validator.check(&review).await?;
```

:::{req} check_node
```rust,ignore
// Defined in habeas_sdk::graph.
pub enum EvidenceState<O> {
    Current(O),
    Missing,
    Stale,
}

// Method on Validator<'d, D>.
pub fn check<V: Validation<D>>(
    &self,
    validation: &V,
) -> impl Future<Output = Result<EvidenceState<V::Ok>, ValidationError<D::Error>>> + Send;
```

`check` uses the same identity, revision, and freshness comparison as `verify`.
It reads the descriptor before awaiting; it does not retain a reference to the
validation value across asynchronous storage or dependency operations. Checking
invokes neither `execute` nor a model and writes no evidence. It needs no
configured agent. Storage, dependency-resolution, and current-output decoding
failures are errors, not missing or stale evidence.
:::

A current negative assessment is still current. Habeas decides which results
make CI pass and how to report the results of its separate criteria.

# Criteria

A criterion asks a question about a node and defines how to answer it. Its value
carries the source identity and any application state needed for the question.
Several criteria can use the same source node and select different inputs.

For example, a Habeas requirement can have questions about its prose, its tests,
and its implementation. The application defines those questions; the SDK
executes each criterion and saves its answer with the inputs it observed.

## Define the question as a value

Start with an application-defined struct. Here is an illustrative question about
whether tests adequately cover a requirement:

```rust,ignore
/// Determine whether the requirement's tests cover its observable behavior
/// and edge cases.
struct BehavioralAdequacy {
    requirement: HabeasNodeId,
    review_policy: String,
}
```

All criterion structs implement `Criterion<D>`, a trait that defines what a
successful result is and how it is evaluated.

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

pub trait Criterion<D: Domain>: Send + Sized {
    /// The completed answer, including any negative assessment.
    /// The SDK serializes it for storage and reconstructs it when reused.
    /// Its Serde JSON representation must reconstruct the completed answer.
    type Ok: Serialize + DeserializeOwned + Send;

    /// The node this question is about.
    fn source_node_id(&self) -> D::NodeId;

    /// Identify this question within the source node's saved evidence.
    fn criterion_path(&self) -> CriterionPath;

    /// Version of these review rules and the result's encoding.
    /// Change it when an old answer should no longer be reused.
    fn revision(&self) -> String;

    /// Select inputs through cx and compute the completed answer.
    fn execute(
        self,
        cx: &mut ValidationContext<'_, D>,
    ) -> impl Future<Output = Result<Self::Ok, ValidationError<D::Error>>> + Send;
}
```

The source identity, criterion path, and revision identify the same question for
checking and execution. Parameters that change which question is being asked
belong in its identity or revision.
:::

## Execute the criterion with a context

`execute` receives a `ValidationContext` for its source node. Its `ask` method
starts a typed agent request. Add instructions and node contents to the builder,
then await the answer. The context records the inputs the agent reads so the SDK
can tell when a saved answer needs to be recomputed.

```rust,ignore
use habeas_sdk::procedure::{Ask, ContextPhase};

// Defined in habeas_sdk::graph.
/// Tracked observations and agent calls for one criterion execution.
pub struct ValidationContext<'cx, D: Domain> { /* private fields */ }

impl<'cx, D: Domain> ValidationContext<'cx, D> {
    /// Start a request whose submitted answer is decoded into R.
    pub fn ask<R>(&self) -> Ask<'_, 'cx, D, R, ContextPhase>
    where
        R: serde::de::DeserializeOwned + schemars::JsonSchema + Send;

    // ... other tools ...
}
```

For Habeas, a behavioral review selects the requirement's text and its test
inputs. Here is an illustrative `execute` method from
`impl Criterion<HabeasDomain> for BehavioralAdequacy`:

```rust,ignore
async fn execute(
    self,
    cx: &mut ValidationContext<'_, HabeasDomain>,
) -> Result<ExpirationAssessment, ValidationError<std::io::Error>> {
    cx.ask()
        .context(&self.review_policy)
        .context("Check the expiration boundary and earlier/later times.")
        .current_node_contents()
        .successors_content(HabeasEdgeLabel::Tests)
        .await
}
```

`ExpirationAssessment` is an application-defined result implementing Serde and
`schemars::JsonSchema`. The policy and question form a reusable prompt prefix;
the requirement and tests supply the changing inputs.

A structural review can use `successors_content(HabeasEdgeLabel::Implements)`
to examine implementation items. A criterion checking the prose can read the
requirement and its cited definitions. Each criterion records the observations
made during its own execution. Inspecting an empty edge selection records it
too, so newly added inputs can invalidate the answer.

Habeas currently runs one combined `Review` criterion at `["review"]`; its
execution selects the requirement and mandatory inputs and applies Habeas's
review policy. The separate criteria above illustrate how that harness can be
reused for more specific questions. [Putting it together in Habeas](../habeas.md)
follows the current review.

[Agent calls](agent-calls.md) explains the `ask()` builder, prompt construction,
and typed submissions. [Context tools](context-tools.md) explains tracked
source discovery and content reads. Use tracked operations for information the
answer relies on, including filesystem or network inputs.

:::{req} deterministic_procedures
`Criterion::execute` can compute an answer without a configured model provider.
A criterion can compute its answer directly using tracked inputs; deterministic
execution needs no configured agent. Evidence storage is still required.
:::

## Give the question a stable path

A `NodeId` and its stable `NodePath` identify the source. `CriterionPath`
identifies a question relative to that source. For example, a requirement might
use `["text"]`, `["behavior"]`, and `["structure"]`; a source item's citation
checks might use `["citations", "requirements.auth.expiration"]`.

:::{req} criterion_path
```rust,ignore
// Defined in habeas_sdk::graph.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct CriterionPath {
    segments: Vec<String>,
}

impl CriterionPath {
    /// Construct a question's path, preserving segment order and boundaries.
    /// Empty paths and empty segments are valid.
    pub fn new(segments: Vec<String>) -> Self;

    /// Read the ordered path segments.
    pub fn segments(&self) -> &[String];
}
```

Its identity is stable across runs and changes to input contents. Different
questions that need independent records have different paths; nested paths can
distinguish several questions within one area.
:::

The complete identity is the pair of source node and criterion path. Appending
criterion segments to a source `NodePath` gives a useful display path; storage
preserves the boundary between the two paths so different pairs cannot collide.
The [evidence chapter](evidence.md) defines that lookup. The criterion path names
an evidence record; the context's source remains the graph node.

## Return a completed answer

The result type belongs to the criterion. The SDK preserves it; the application
interprets the answer. For applications wanting a boolean and explanation, the
SDK provides this convenient result type:

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

A criterion can use `Judgment<M>` as its `Ok` type when the metadata satisfies
its bounds, or use another serializable result. `Graph` has no judgment-metadata
associated type. The SDK does not interpret these fields or implement Habeas's
FIXME acknowledgment policy.
:::

A completed assessment finding a gap is an `Ok` value and can be saved and reused.
An `Err` means execution failed and produces no completed record.

## Run criteria with a validator

A `Validator<D>` owns a cloneable domain and agent configuration. Submit a
criterion to `verify` to reuse its saved answer or run its `execute` method.

:::{req} validator
```rust,ignore
use std::sync::Arc;

// Defined in habeas_sdk::graph.
/// Domain access and configuration shared by criterion executions.
/// Evidence storage is required through Domain.
pub struct Validator<D: Domain> { /* private fields */ }

impl<D: Domain> Validator<D> {
    /// Own the domain used to check and execute criteria.
    pub fn new(domain: D) -> Self;

    /// Share this agent and input renderer with execution contexts.
    pub fn with_agent(
        self,
        agent: Arc<dyn Agent<D>>,
        renderer: Arc<dyn AgentInputRenderer<D>>,
    ) -> Self;
}
```

`Agent` and `AgentInputRenderer` are supplied by `habeas_sdk::procedure`.
Independent verifications share a validator through `&self`. Cached observations
can be reused when they still represent the state being checked. Each execution
has its own context, dependency trace, and agent conversation state. Operations
within one execution share its synchronized trace through context inspection
and agent calls.

Concurrent verifications of the same source and criterion run independently,
each with its own context and trace. They can both execute when evidence is
missing or stale.
:::

Here are illustrative application-defined criteria for one requirement. `agent`
and `renderer` are configured `Arc` handles:

```rust,ignore
let validator = Validator::new(domain.clone())
    .with_agent(agent.clone(), renderer.clone());

let text = validator.verify(TextualQuality {
    requirement: expiration.clone(),
}).await?;
let behavior = validator.verify(BehavioralAdequacy {
    requirement: expiration.clone(),
    review_policy: test_review_policy,
}).await?;
let structure = validator.verify(StructuralConformance {
    requirement: expiration,
}).await?;
```

Each value supplies its own criterion path and result type. Their answers are
saved independently.

## Check or reuse saved evidence

:::{req} procedure_identity
Checking and verification obtain the source node, criterion path, and revision
from the same `Criterion` value. There is no node-wide revision resolver.
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
// Method on Validator<D>.
/// Reuse the current answer, including a negative assessment.
/// Otherwise execute and save the completed record before returning.
pub fn verify<V: Criterion<D>>(
    &self,
    criterion: V,
) -> impl Future<Output = Result<V::Ok, ValidationError<D::Error>>> + Send;
```

The SDK loads the record for this source and criterion and compares its revision
and dependencies under {cite}`requirements.sdk.graph.evidence.checking_records`.
Current evidence is decoded into `V::Ok` and returned without calling `execute`.
Missing or stale evidence starts a fresh context identifying the source node,
executes the supplied criterion, and saves its result and captured dependencies.
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

## Inspect saved evidence

CI needs to distinguish missing, stale, and current evidence. It supplies the
same criterion value to `check`:

```rust,ignore
let review = BehavioralAdequacy {
    requirement: expiration,
    review_policy: test_review_policy,
};
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

// Method on Validator<D>.
pub fn check<V: Criterion<D>>(
    &self,
    criterion: &V,
) -> impl Future<Output = Result<EvidenceState<V::Ok>, ValidationError<D::Error>>> + Send;
```

`check` uses the same identity, revision, and freshness comparison as `verify`.
It reads the descriptor before awaiting; it does not retain a reference to the
criterion value across asynchronous storage or dependency operations. Checking
invokes neither `execute` nor a model and writes no evidence. It needs no
configured agent. Storage, dependency-resolution, and current-output decoding
failures are errors, not missing or stale evidence.
:::

A current negative assessment is still current. Habeas decides which results
make CI pass and how to report the results of its separate criteria.

## Use tracked inputs directly

A criterion can also read inputs and compute part or all of its answer directly.
The context exposes the source identity and tracked graph operations:

:::{req} root_inputs
```rust,ignore
// Defined in habeas_sdk::graph.
/// Input selection and tracked observations for one criterion execution.
pub struct ValidationContext<'cx, D: Domain> { /* private fields */ }

impl<'cx, D: Domain> ValidationContext<'cx, D> {
    /// The source node this criterion examines.
    pub fn node_id(&self) -> &D::NodeId;

    /// Tracked graph reads and edge selections for this execution.
    pub fn inspection(&self) -> &InspectionContext<'cx, D>;

    /// Select outgoing edges by label, preserving their order and duplicates.
    /// Record this label's target-set fingerprint even when nothing matches.
    /// Target contents enter the trace when explicitly read or inserted.
    pub fn successors(
        &self,
        label: &D::EdgeLabel,
    ) -> Result<Vec<D::NodeId>, D::Error>
    where
        D::EdgeLabel: PartialEq;
}
```

Labels match by equality. A change to the recorded target set makes prior
evidence stale, including adding an edge to an empty selection. Target contents
become dependencies through tracked reads or agent insertions. An edge selects a
related source; assessments of that target remain separate.
:::

[Context tools](context-tools.md) covers content reads, filesystem exploration,
and source search.

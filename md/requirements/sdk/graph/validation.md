# Criteria and validators

In the Habeas SDK, a criterion is a question about a node that can be evaluated.
Define it with your own struct implementing `Criterion<D>`, then evaluate it
with a `Validator<D>`.

The following application example asks whether a requirement's tests cover its
behavior. This chapter defines the `BehavioralAdequacy` criterion and shows how
to evaluate it and manage its evidence. [Evaluating criteria](execution.md)
fills in its execution method.

:::{example} criterion_definition
{cite}`requirements.sdk.graph.validation.check_procedure`
{cite}`requirements.sdk.graph.validation.criterion_path`

```rust,ignore
/// Determine whether the requirement's tests cover its observable behavior
/// and edge cases.
struct BehavioralAdequacy {
    requirement: HabeasNodeId,
    review_policy: String,
}

/// The criterion's answer, submitted by the agent.
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
struct TestReview {
    adequate: bool,
    rationale: String,
}

impl Criterion<HabeasDomain> for BehavioralAdequacy {
    type Ok = TestReview;

    fn source_node_id(&self) -> HabeasNodeId {
        self.requirement.clone()
    }

    async fn execute(
        self,
        cx: &mut ValidationContext<'_, HabeasDomain>,
    ) -> Result<Self::Ok, ValidationError<std::io::Error>> {
        // Evaluation code goes here; the next chapter fills it in.
        todo!()
    }

    // ... criterion_path and revision are described below ...
}
```
:::

## Run criteria with a validator

A `Validator<D>` owns a cloneable domain and agent configuration. It supplies
four operations: load evidence, check its inputs, execute a criterion, and
store the resulting evidence. Applications compose these into their workflows.

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

    /// Share this agent with execution contexts.
    pub fn with_agent(
        self,
        agent: Arc<dyn Agent<D>>,
    ) -> Self;
}
```

`Agent` is supplied by `habeas_sdk::procedure`; the domain formats node contents.
[Agent harness](agent-harness.md) shows how to configure a provider and tools.
Independent verifications share a validator through `&self`. Cached observations
can be reused when they still represent the state being checked. Each execution
has its own context, dependency trace, and agent conversation state. Operations
within one execution share its synchronized trace through context inspection
and agent calls.

Concurrent verifications of the same source and criterion run independently,
each with its own context and trace. The storage backend handles atomic
replacement when their results are saved.
:::

The validator can now evaluate an instance of `BehavioralAdequacy`.

:::{example} evaluate_criterion
{cite}`requirements.sdk.graph.validation.validator`
{cite}`requirements.sdk.graph.validation.validate_node`
{cite}`requirements.sdk.graph.validation.store_evidence`
{cite}`requirements.sdk.graph.evidence.inputs`

Here `agent` is a configured `Arc` handle, and `domain` is Habeas's implementation
of the SDK's `Graph`, `Contents`, and `Storage` traits:

```rust,ignore
let validator = Validator::new(domain.clone())
    .with_agent(agent.clone());

let inputs = validator.verify(BehavioralAdequacy {
    requirement: requirement.clone(),
    review_policy: test_review_policy.clone(),
}).await?;

validator.store_evidence(&inputs).await?;
let assessment: &TestReview = inputs.output();
println!("{}: {}", assessment.adequate, assessment.rationale);
```

`verify` evaluates the question and returns `Fingerprinted<D, TestReview>`, containing
the answer and its recorded inputs. `store_evidence` saves that value. Loading
evidence and checking its inputs are separate operations, so the application
decides whether to reuse an answer or evaluate the criterion again.
:::

## Define the criterion's interface

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

The criterion defines its answer through `Ok`. Here, an `Ok(TestReview {
adequate: false, ... })` is a completed assessment finding a gap. The validator
can save and reuse that answer; an `Err` means execution failed and produces no
completed record.

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
The [Storage chapter](evidence.md) defines that lookup. The criterion path names
an evidence record; the context's source remains the graph node.

## Verify a criterion

:::{req} procedure_identity
`load_evidence` selects a saved record using the source node and criterion path
from the supplied `Criterion`. `verify` captures those values and the criterion's
revision in its returned `Fingerprinted`. The pair of source and criterion selects a
record; its saved revision identifies the review rules used to produce it.

Changing review instructions, deterministic rules, input-selection policy, or
result encoding in a way that invalidates an old answer requires a changed
revision. This is independent of model selection and the agent's context-prefix
cache fingerprint. A stored legacy revision `*` retains the matching behavior of
{cite}`requirements.sdk.graph.validation.checking_records.procedure_match`.
:::

:::{req} validate_node
```rust,ignore
// Method on Validator<D>.
/// Execute the criterion and return its completed answer and observed inputs.
/// Use store_evidence to save the returned value.
pub fn verify<V: Criterion<D>>(
    &self,
    criterion: V,
) -> impl Future<Output = Result<Fingerprinted<D, V::Ok>, ValidationError<D::Error>>> + Send;
```

Every call starts a fresh context identifying the source node and calls
`Criterion::execute`. Its completed answer, including a negative assessment,
and captured dependencies form the returned `Fingerprinted` under
{cite}`requirements.sdk.graph.evidence.inputs`. Loading and saving records are
separate operations.

Freshness checking has a separate observation scope from execution. Reads used
only to check old evidence do not become dependencies of the new result. A
checked snapshot can be reused if it still represents the current state; when
execution explicitly reads or delivers it, its fingerprint enters the new trace.
:::

:::{req} validate_node.errors
An execution error returns `Err` rather than a `Fingerprinted` containing a completed
answer. The caller can store only a completed verification.
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

`Validator::check_inputs` reads current contents and edge selections through the
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

## Compose a reuse workflow

The application decides when to reuse a saved answer. For example, it can
compare the saved revision and recorded inputs before choosing whether to
execute and store a new review:

```rust,ignore
let review = BehavioralAdequacy {
    requirement,
    review_policy: test_review_policy,
};

let inputs = match validator.load_evidence(&review).await? {
    Some(saved)
        if procedure_matches(saved.revision(), &review.revision())
            && validator.check_inputs(&saved)? => saved,
    _ => {
        let inputs = validator.verify(review).await?;
        validator.store_evidence(&inputs).await?;
        inputs
    }
};

let assessment = inputs.output();
println!("{}: {}", assessment.adequate, assessment.rationale);
```

Of the [comparisons above](#compare-the-saved-observations), `procedure_matches`
checks whether the saved answer was produced using compatible review rules;
`check_inputs` compares only its recorded observations. A current negative
assessment is still current. Habeas decides which answers make CI pass.

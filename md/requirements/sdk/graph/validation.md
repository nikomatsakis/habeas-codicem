# Criteria and validators

In the Habeas SDK, define a
{term}`requirements.sdk.graph.validation.criterion` with your own type
implementing `Criterion<D>`, then evaluate it with a `Validator<D>`.

## Define a criterion

:::{definition} criterion
:phrase: criterion

A test performed on a {term}`requirements.sdk.graph.node`. In the SDK, it is
represented by a value whose type implements `Criterion<D>` for the
{term}`requirements.sdk.application_domain`. Its `Criterion::Ok` type describes
the completed answer, which can include a negative assessment. An execution
error means no completed answer was produced.
:::

All criterion types implement `Criterion<D>`, a trait that defines what a
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
    /// Its Serde JSON representation must reconstruct the completed answer.
    type Ok: Serialize + DeserializeOwned + Send;

    /// The node this question is about.
    fn source_node_id(&self) -> D::NodeId;

    /// Identify this question relative to its source node.
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

:::{example} criterion_definition
{cite}`requirements.sdk.graph.validation.check_procedure`
{cite}`requirements.sdk.graph.validation.criterion_path`

As an example, consider how Habeas reviews requirements for behavioral adequacy:
whether their tests cover the required observable behavior and edge cases.
The following `BehavioralAdequacy` criterion models that question. Its fields
identify the requirement and review policy; `TestReview` describes the answer:

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

The criterion defines its answer through `Ok`. Here, an `Ok(TestReview {
adequate: false, ... })` is a completed assessment finding a gap. An `Err` means
the evaluation failed to produce a completed answer.

## Run criteria with a validator

A `Validator<D>` owns a cloneable domain. Construct it with `new`, then submit a
criterion to `verify` to evaluate it and return its answer with the inputs used
to produce it.

:::{req} validator
```rust,ignore
// Defined in habeas_sdk::graph.
/// Domain access and configuration shared by criterion executions.
pub struct Validator<D: Domain> { /* private fields */ }

impl<D: Domain> Validator<D> {
    /// Own the domain used to check and execute criteria.
    pub fn new(domain: D) -> Self;
}
```

Independent verifications share a validator through `&self`. Cached observations
can be reused when they still represent the state being checked. Each execution
has its own context, dependency trace, and agent conversation state. Operations
within one execution share its synchronized trace through context inspection
and agent calls.

Concurrent verifications of the same source and criterion run independently,
each with its own context and trace. [Storage](evidence.md#look-up-a-source-and-criterion)
describes atomic replacement when results are saved.
:::

:::{req} validate_node
```rust,ignore
// Method on Validator<D>.
/// Execute the criterion and return its completed answer and observed inputs.
pub fn verify<V: Criterion<D>>(
    &self,
    criterion: V,
) -> impl Future<Output = Result<Fingerprinted<D, V::Ok>, ValidationError<D::Error>>> + Send;
```

Every call constructs a fresh `ValidationContext` identifying the source node
and calls `Criterion::execute` with that context. Its completed answer, including
a negative assessment, and captured dependencies form the returned `Fingerprinted`
under {cite}`requirements.sdk.graph.evidence.inputs`.
:::

:::{req} validate_node.errors
An execution error returns `Err` rather than a `Fingerprinted` containing a
completed answer.
:::

:::{example} evaluate_criterion
{cite}`requirements.sdk.graph.validation.validator`
{cite}`requirements.sdk.graph.validation.validate_node`
{cite}`requirements.sdk.graph.agent-harness.with_agent`
{cite}`requirements.sdk.graph.evidence.refresh_evidence`
{cite}`requirements.sdk.graph.evidence.inputs`

We can now instantiate `BehavioralAdequacy` and evaluate it with a `Validator`.
Here `domain` supplies Habeas's graph, contents, and storage, and `agent`
implements the [agent interface](agent-harness.md#the-agent-interface).
[`with_agent`](agent-harness.md#configure-the-validators-agent) configures that
agent for the criterion's requests.
The `refresh_evidence` helper reuses the previous answer when the review rules
and inputs are current, or evaluates the criterion again:

```rust,ignore
let validator = Validator::new(domain.clone())
    .with_agent(agent.clone());

let inputs = validator.refresh_evidence(BehavioralAdequacy {
    requirement: requirement.clone(),
    review_policy: test_review_policy.clone(),
}).await?;

let assessment: &TestReview = inputs.output();
println!("{}: {}", assessment.adequate, assessment.rationale);
```
:::

This chapter focuses on defining the criterion and evaluating it with a
validator. [Storage](evidence.md#refresh-evidence) defines `refresh_evidence`
and explains the load, check, verify, and store steps it combines.
[Evaluating criteria](execution.md) fills in the criterion's execution method.

## Give the criterion a stable path

:::{definition} source_node
:phrase: source node

The {term}`requirements.sdk.graph.node` on which a
{term}`requirements.sdk.graph.validation.criterion` is evaluated, identified by
`Criterion::source_node_id`. Tracked reads of its own contents or those of other
nodes can supply inputs without changing the criterion's source node.
:::

A `NodeId` and its stable `NodePath` identify the
{term}`requirements.sdk.graph.validation.source_node`. `CriterionPath`
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
questions that need independent assessments have different paths; nested paths
can distinguish several questions within one area.
:::

The complete identity is the pair of source node and criterion path. Appending
criterion segments to a source `NodePath` gives a useful display path.
[Storage](evidence.md#identify-the-saved-assessment) explains how this pair
identifies a saved answer.

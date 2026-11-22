# Criteria and validators

In the Habeas SDK, a criterion is a question about a node that can be evaluated.
Define it with your own struct implementing `Criterion<D>`, then evaluate it
with a `Validator<D>`.

A requirement can be reviewed with `BehavioralAdequacy`, an application-defined
criterion. The validator can reuse its previous answer when the review rules
and inputs are current, or evaluate it again:

:::{example} evaluate_criterion
{cite}`requirements.sdk.graph.validation.validator`
{cite}`requirements.sdk.graph.evidence.refresh_evidence`
{cite}`requirements.sdk.graph.evidence.inputs`

Here `domain` implements Habeas's graph, contents, and storage, and `agent` is
an implementation of the [agent interface](agent-harness.md#the-agent-interface):

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

## Define a criterion

`BehavioralAdequacy` asks whether a requirement's tests cover its behavior. Its
fields identify the requirement and the review policy; its `Ok` type describes
the answer:

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

A `Validator<D>` owns a cloneable domain and agent configuration. Its `verify`
method evaluates a criterion and returns its answer with the inputs used to
produce it.

:::{req} validator
```rust,ignore
use habeas_sdk::procedure::Agent;
use std::sync::Arc;

// Defined in habeas_sdk::graph.
/// Domain access and configuration shared by criterion executions.
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

[`Agent<D>`](agent-harness.md#the-agent-interface) is the interface an `ask`
request calls to run a model conversation. It is defined in
`habeas_sdk::procedure`; the supplied `ToolAgent` implements it. The
[Agent harness chapter](agent-harness.md) shows how to configure a provider
and tools.
Independent verifications share a validator through `&self`. Cached observations
can be reused when they still represent the state being checked. Each execution
has its own context, dependency trace, and agent conversation state. Operations
within one execution share its synchronized trace through context inspection
and agent calls.

Concurrent verifications of the same source and criterion run independently,
each with its own context and trace. [Storage](evidence.md#look-up-a-source-and-criterion)
describes atomic replacement when results are saved.
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

The criterion defines its answer through `Ok`. Here, an `Ok(TestReview {
adequate: false, ... })` is a completed assessment finding a gap. An `Err` means
the evaluation failed to produce a completed answer.

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
questions that need independent assessments have different paths; nested paths
can distinguish several questions within one area.
:::

The complete identity is the pair of source node and criterion path. Appending
criterion segments to a source `NodePath` gives a useful display path.
[Storage](evidence.md#identify-the-saved-assessment) explains how this pair
identifies a saved answer.

## Verify a criterion

:::{req} validate_node
```rust,ignore
// Method on Validator<D>.
/// Execute the criterion and return its completed answer and observed inputs.
pub fn verify<V: Criterion<D>>(
    &self,
    criterion: V,
) -> impl Future<Output = Result<Fingerprinted<D, V::Ok>, ValidationError<D::Error>>> + Send;
```

Every call starts a fresh context identifying the source node and calls
`Criterion::execute`. Its completed answer, including a negative assessment,
and captured dependencies form the returned `Fingerprinted` under
{cite}`requirements.sdk.graph.evidence.inputs`.
:::

:::{req} validate_node.errors
An execution error returns `Err` rather than a `Fingerprinted` containing a
completed answer.
:::

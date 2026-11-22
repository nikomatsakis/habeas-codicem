# Criteria definition

An application defines a
{term}`requirements.sdk.graph.validation.criterion` to ask a question about a
graph node. This chapter describes the criterion's type, answer, and identity.
[Validators](validators.md) explains how to run criterion values, and
[Criteria evaluation](execution.md) explains how to write their execution methods.

## Define a criterion

:::{definition} criterion
:phrase: criterion

A value describing a test to perform on a {term}`requirements.sdk.graph.node`.
In the SDK, its type implements `Criterion<D>` for the
{term}`requirements.sdk.application_domain`. Its `Criterion::Ok` type describes
the completed answer, which can include a negative assessment. An execution
error means no completed answer was produced.
:::

:::{req} criterion_trait
The SDK provides the `Criterion<D>` trait below. Every criterion type implements
this trait, which defines its completed answer and how it is evaluated.

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
{cite}`requirements.sdk.graph.validation.criterion_trait`
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
        // Evaluation code goes here; the Criteria evaluation chapter fills it in.
        todo!()
    }

    // ... criterion_path and revision are described below ...
}
```
:::

The criterion defines its answer through `Ok`. Here, an `Ok(TestReview {
adequate: false, ... })` is a completed assessment finding a gap. An `Err` means
the evaluation failed to produce a completed answer.

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

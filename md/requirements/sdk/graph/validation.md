# Criteria definition

An application defines a
{term}`requirements.sdk.graph.validation.criterion` to ask a question about a
graph node. This chapter describes the criterion's type, answer, and identity.
[Validators](validators.md) explains how to run criterion values, and
[Criteria evaluation](execution.md) explains how to write their execution methods.

## Define a criterion

:::{definition} criterion
:phrase: criterion

A *criterion* is a value describing a test to perform on a
{term}`requirements.sdk.graph.node`.
In the SDK, its type implements
[`Criterion<D>`](../../../rustdocs/habeas_sdk/graph/trait.Criterion.html) for the
{term}`requirements.sdk.application_domain`. Its
members describe the question's answer, identity, review rules, and execution.
:::

:::{req} criterion_trait
Every {term}`requirements.sdk.graph.validation.criterion` type implements
`habeas_sdk::graph::Criterion<D>` for its
{term}`requirements.sdk.application_domain`:

```rust,ignore
// Defined in habeas_sdk::graph.
pub trait Criterion<D: Domain>: Send + Sized {
    // ... members introduced in the following sections and chapters ...
}
```
:::

## The application interprets criterion results

:::{req} criterion_trait.answer
{cite}`requirements.sdk.graph.validation.criterion_trait`

[`Criterion::Ok`](../../../rustdocs/habeas_sdk/graph/trait.Criterion.html#associatedtype.Ok)
defines the criterion's completed answer:

```rust,ignore
use serde::{Serialize, de::DeserializeOwned};

pub trait Criterion<D: Domain>: Send + Sized {
    /// The completed answer, including any negative assessment.
    /// Its Serde JSON representation must reconstruct the completed answer.
    type Ok: Serialize + DeserializeOwned + Send;

    // ... additional members introduced below ...
}
```
:::

:::{req} output_policy
Habeas SDK treats `Criterion::Ok` as an application-defined completed answer.
When returning or saving that answer, it does not interpret whether the answer
is favorable or satisfies an application requirement. A negative assessment
can be returned as `Ok(value)`; `Err` represents an execution error instead.
:::

:::{example} criterion_definition
{cite}`requirements.sdk.graph.validation.criterion_trait`
{cite}`requirements.sdk.graph.validation.criterion_trait.answer`

As an example, consider how Habeas Codicem reviews requirements for behavioral adequacy:
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

    // ... remaining members introduced in the following sections and chapters ...
}
```
:::

:::{example} result_interpretation
{cite}`requirements.sdk.graph.validation.output_policy`
{cite}`requirements.sdk.habeas.assessment_policy`

Suppose the `BehavioralAdequacy` criterion above returns this completed answer:

```rust,ignore
Ok(TestReview {
    adequate: false,
    rationale: "No test covers expired tokens.".into(),
})
```

In this example, HC's validation policy requires `adequate` to be `true`.
HC reports the inadequate tests and `habeas status` returns an error, even when
the saved answer is current. The SDK returns the completed answer;
HC applies the judgment that makes this a validation failure.
:::

## Criterion source node

:::{definition} source_node
:phrase: source node

A *source node* is the {term}`requirements.sdk.graph.node` on which a
{term}`requirements.sdk.graph.validation.criterion` is evaluated, identified by
[`Criterion::source_node_id`](../../../rustdocs/habeas_sdk/graph/trait.Criterion.html#tymethod.source_node_id).
Tracked reads of its own contents or those of other nodes can supply inputs
without changing the criterion's source node.
:::

:::{req} criterion_trait.source_node_id
{cite}`requirements.sdk.graph.validation.criterion_trait`

`Criterion::source_node_id` identifies the criterion's
{term}`requirements.sdk.graph.validation.source_node`:

```rust,ignore
pub trait Criterion<D: Domain>: Send + Sized {
    /// The node this question is about.
    fn source_node_id(&self) -> D::NodeId;

    // ... additional members ...
}
```
:::

## Give the criterion a stable path

:::{definition} question_path
:phrase: criterion path

A *criterion path* is an ordered sequence of string segments identifying a
{term}`requirements.sdk.graph.validation.criterion`'s question relative to its
{term}`requirements.sdk.graph.validation.source_node`.
[`CriterionPath`](../../../rustdocs/habeas_sdk/graph/struct.CriterionPath.html)
represents this path, while
[`NodeId`](../../../rustdocs/habeas_sdk/domain/trait.Domain.html#associatedtype.NodeId)
and its stable
{term}`requirements.sdk.graph.node_path` identify the source node.
For example, a requirement might use `["text"]`, `["behavior"]`, and
`["structure"]`; a source item's citation checks might use
`["citations", "requirements.auth.expiration"]`.
:::

:::{req} criterion_path
{cite}`requirements.sdk.graph.validation.criterion_trait`

`habeas_sdk::graph::CriterionPath` represents a
{term}`requirements.sdk.graph.validation.question_path`:

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

pub trait Criterion<D: Domain>: Send + Sized {
    /// Identify this question relative to its source node.
    fn criterion_path(&self) -> CriterionPath;

    // ... additional members ...
}
```

Its identity is stable across runs and changes to input contents. Different
questions that need independent assessments have different paths; nested paths
can distinguish several questions within one area.

The complete identity is the pair of source node and criterion path.
:::

## Version the criterion's rules

:::{req} criterion_trait.revision
{cite}`requirements.sdk.graph.validation.criterion_trait`

`Criterion::revision` identifies the version of the review rules and the
completed answer's encoding:

```rust,ignore
pub trait Criterion<D: Domain>: Send + Sized {
    /// Version of these review rules and the result's encoding.
    /// Change it when an old answer should no longer be reused.
    fn revision(&self) -> String;

    // ... execute is introduced in Criteria evaluation ...
}
```

The {term}`requirements.sdk.graph.validation.source_node`,
{term}`requirements.sdk.graph.validation.question_path`, and revision identify
the same question for checking and execution. Parameters that change which
question is being asked belong in its identity or revision.

[Storage](evidence.md#identify-the-saved-assessment) explains how the source node
and criterion path identify a saved answer and how its revision is recorded.
:::

## Evaluate the criterion

[Criteria evaluation](execution.md#execute-the-criterion-with-a-context)
introduces `Criterion::execute` and completes the `BehavioralAdequacy` example.

# Criteria

An application defines a
{term}`requirements.sdk.graph.criteria.criterion` to ask a question about a
graph node. This chapter introduces the criterion's answer, identity, review
rules, and execution method. [Running criteria](running-criteria.md) explains how to run
criterion values. [Agent calls](agent-calls.md) and
[Validation context](validation-context.md) describe the context available during execution.

## Define a criterion

:::{definition} criterion
:phrase: criterion

A *criterion* is a value describing a test to perform on a
{term}`requirements.sdk.graph.node`.
In the SDK, its type implements
[`Criterion<D>`](../../../rustdocs/trust_but_verify/graph/trait.Criterion.html) for the
{term}`requirements.sdk.application_domain`. Its
members describe the question's answer, identity, review rules, and execution.
:::

:::{req} criterion_trait
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Every {term}`requirements.sdk.graph.criteria.criterion` type implements
`trust_but_verify::graph::Criterion<D>` for its
{term}`requirements.sdk.application_domain`.

```rust,ignore
pub trait Criterion<D: Domain>: Send + Sized {
    type Ok: Serialize + DeserializeOwned + Send;

    fn source_node_id(&self) -> D::NodeId;
    fn criterion_path(&self) -> CriterionPath;
    fn version(&self) -> CriterionVersion;
    fn execute(
        self,
        ctx: &mut ValidationContext<'_, D>,
    ) -> impl Future<Output = Result<Self::Ok, ValidationError<D::Error>>> + Send;
}
```
:::

## Define the completed answer

:::{req} criterion_trait.answer
{cite}`requirements.sdk.graph.criteria.criterion_trait`

[`Criterion::Ok`](../../../rustdocs/trust_but_verify/graph/trait.Criterion.html#associatedtype.Ok)
defines the criterion's completed answer:

```rust,ignore
use serde::{Serialize, de::DeserializeOwned};

pub trait Criterion<D: Domain>: Send + Sized {
    // ...

    /// The completed answer, including any negative assessment.
    /// Its Serde JSON representation must reconstruct the completed answer.
    type Ok: Serialize + DeserializeOwned + Send;

    // ...
}
```
:::

:::{example} criterion_definition
{cite}`requirements.sdk.graph.criteria.criterion_trait`
{cite}`requirements.sdk.graph.criteria.criterion_trait.answer`

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
    // ...

    type Ok = TestReview;

    // ...
}
```
:::

## Criterion source node

:::{definition} source_node
:phrase: source node

A *source node* is the {term}`requirements.sdk.graph.node` on which a
{term}`requirements.sdk.graph.criteria.criterion` is evaluated, identified by
[`Criterion::source_node_id`](../../../rustdocs/trust_but_verify/graph/trait.Criterion.html#tymethod.source_node_id).
Tracked reads of its own contents or those of other nodes can supply inputs
without changing the criterion's source node.
:::

:::{req} criterion_trait.source_node_id
{cite}`requirements.sdk.graph.criteria.criterion_trait`

`Criterion::source_node_id` identifies the criterion's
{term}`requirements.sdk.graph.criteria.source_node`:

```rust,ignore
pub trait Criterion<D: Domain>: Send + Sized {
    // ...

    /// The node this question is about.
    fn source_node_id(&self) -> D::NodeId;

    // ...
}
```
:::

## Give the criterion a stable path

:::{definition} question_path
:phrase: criterion path

A *criterion path* is an ordered sequence of string segments identifying a
{term}`requirements.sdk.graph.criteria.criterion`'s question relative to its
{term}`requirements.sdk.graph.criteria.source_node`.
[`CriterionPath`](../../../rustdocs/trust_but_verify/graph/struct.CriterionPath.html)
represents this path, while
[`NodeId`](../../../rustdocs/trust_but_verify/domain/trait.Domain.html#associatedtype.NodeId)
and its stable
{term}`requirements.sdk.graph.node_path` identify the source node.
For example, a requirement might use `["text"]`, `["behavior"]`, and
`["structure"]`; a source item's citation checks might use
`["citations", "requirements.auth.expiration"]`.
:::

:::{req} criterion_path
{cite}`requirements.sdk.graph.criteria.criterion_trait`

`Criterion::criterion_path` identifies the question relative to its source. The
path preserves segment order and boundaries. Empty paths and empty segments are valid.

```rust,ignore
pub trait Criterion<D: Domain>: Send + Sized {
    // ...

    /// Identify this question relative to its source node.
    fn criterion_path(&self) -> CriterionPath;

    // ...
}
```

Its identity is stable across runs and changes to input contents. Different
questions that need independent assessments have different paths; nested paths
can distinguish several questions within one area.

The complete identity is the pair of source node and criterion path.
:::

:::{req} CriterionPath
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

An ordered sequence of strings identifying a criterion. `new` preserves segment boundaries and `segments` borrows the sequence.

```rust,ignore
pub struct CriterionPath { /* private fields */ }

impl CriterionPath {
    pub fn new(segments: Vec<String>) -> Self;
    pub fn segments(&self) -> &[String];
}
```

Trait implementations

```rust,ignore
impl Clone for CriterionPath;
impl Debug for CriterionPath;
impl<'de> Deserialize<'de> for CriterionPath;
impl Hash for CriterionPath;
impl PartialEq for CriterionPath;
impl Serialize for CriterionPath;
impl Eq for CriterionPath;
```
:::

## Version the criterion's rules

:::{definition} check_version
:phrase: criterion version

A *criterion version* is an application-chosen identifier for the rules used to
check a {term}`requirements.sdk.graph.criteria.criterion` and encode its answer.
Change the version when an answer produced under the old rules can no longer be reused.
:::

:::{req} criterion_version
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

`CriterionVersion::new` constructs a {term}`requirements.sdk.graph.criteria.check_version`
from the supplied string. It panics on `"*"`, which is reserved for unversioned
legacy evidence. When reading a saved version, use `from_recorded` to interpret
`"*"` as `None` and any other string as `Some(version)`.
`as_str()` borrows the original string. Compare this version with the optional
version from a saved record using
{cite}`requirements.sdk.graph.evidence.checking_records.version_match`.

```rust,ignore
pub struct CriterionVersion(/* private fields */);

impl CriterionVersion {
    pub fn new(value: impl Into<String>) -> Self;
    pub fn from_recorded(value: impl Into<String>) -> Option<Self>;
    pub fn as_str(&self) -> &str;
    pub fn is_compatible_with(&self, saved: Option<&Self>) -> bool;
}
```

Trait implementations

```rust,ignore
impl Clone for CriterionVersion;
impl Debug for CriterionVersion;
impl<'de> Deserialize<'de> for CriterionVersion;
impl Serialize for CriterionVersion;
impl PartialEq for CriterionVersion;
impl Eq for CriterionVersion;
```
:::

:::{req} criterion_trait.version
{cite}`requirements.sdk.graph.criteria.criterion_trait`

`Criterion::version` identifies the version of the review rules and the
completed answer's encoding:

```rust,ignore
pub trait Criterion<D: Domain>: Send + Sized {
    // ...

    /// Version of these review rules and the result's encoding.
    /// Change it when an old answer should no longer be reused.
    fn version(&self) -> CriterionVersion;

    // ...
}
```

The {term}`requirements.sdk.graph.criteria.source_node`,
{term}`requirements.sdk.graph.criteria.question_path`, and version identify
the same question for checking and execution. Parameters that change which
question is being asked belong in its identity or version.

[Storage](evidence.md#identify-the-saved-assessment) explains how the source node
and criterion path identify a saved answer and how its version is recorded.
:::

## Execute the criterion with a context

:::{req} execute
{cite}`requirements.sdk.graph.criteria.criterion_trait`

`Criterion::execute` receives a context for its
{term}`requirements.sdk.graph.criteria.source_node` and computes its
completed answer:

```rust,ignore
use std::future::Future;

pub trait Criterion<D: Domain>: Send + Sized {
    // ...

    /// Select inputs through cx and compute the completed answer.
    fn execute(
        self,
        cx: &mut ValidationContext<'_, D>,
    ) -> impl Future<Output = Result<Self::Ok, ValidationError<D::Error>>> + Send;

    // ...
}
```

An `Ok` value is the answer defined by
{cite}`requirements.sdk.graph.criteria.criterion_trait.answer`.
An `Err` value reports a [validation error](running-criteria.md#habeas-req-requirements.sdk.graph.running-criteria.ValidationError).
:::

`ValidationContext` supplies the tools `execute` can use to decide whether the
criterion is satisfied. See [Validation context](validation-context.md) for tracked
reads, source discovery, and agent requests.

The `BehavioralAdequacy` criterion asks the agent to examine
the requirement and its tests and submit a `TestReview`. Its `execute` method is
an application example of the SDK's request builder:

:::{example} behavioral_adequacy
{cite}`requirements.sdk.graph.agent-calls.judge`
{cite}`requirements.sdk.graph.agent-calls.judge.node_contents`
{cite}`requirements.sdk.graph.evidence.inputs`

Here is the `execute` method from
`impl Criterion<HabeasDomain> for BehavioralAdequacy`:

```rust,ignore
async fn execute(
    self,
    cx: &mut ValidationContext<'_, HabeasDomain>,
) -> Result<Self::Ok, ValidationError<std::io::Error>> {
    cx.ask()
        .context(&self.review_policy)
        .context("Check whether these tests cover the requirement's behavior and edge cases.")
        .contents_of_current_node()
        .contents_of_successors(HabeasEdgeLabel::Tests)
        .await
}
```

The [`ask` builder](agent-calls.md#build-a-request) determines what is given to
the [configured agent](agent-harness.md#choose-an-agent-backend):

- [`context()`](agent-calls.md#compose-reusable-context) provides common context,
  including the question.
- [`contents_of_current_node()`](agent-calls.md#deliver-and-record-node-inputs)
  provides the [contents](../graph.md#read-node-contents) of the criterion's source node:
  here, the requirement.
- [`contents_of_successors(label)`](agent-calls.md#deliver-and-record-node-inputs)
  provides the [contents](../graph.md#read-node-contents) of its successors with that label:
  here, the requirement's tests.

For any additional information the answer relies on, use
[tracked context tools](validation-context.md), including for filesystem or network
inputs.

:::

:::{cite} requirements.sdk.graph.agent-calls.author_procedure
The context records the contents supplied in the prompt. The agent also has
tools for requesting additional contents, which are recorded when delivered.
Awaiting the ask builder returns the submitted `TestReview`; `verify` returns
a `Fingerprinted` containing the criterion's completed answer and the identity and
fingerprint of each input used during execution. Those fingerprints let the
application check when the answer may be stale.

{cite}`requirements.sdk.graph.agent-calls.judge.submission`
{cite}`requirements.sdk.graph.running-criteria.validate_node`
{cite}`requirements.sdk.graph.evidence.inputs`
:::

:::{req} deterministic_procedures
`Criterion::execute` can compute an answer without a configured model provider.
A criterion can compute its answer directly using tracked inputs; deterministic
execution needs no configured agent.
:::

:::{cite} requirements.sdk.graph.criteria.deterministic_procedures
A criterion can also compute its answer with deterministic Rust code.
Its `ValidationContext` exposes the [inspection operations](validation-context.md)
used by the agent's graph tools.
[Searches and outlines](validation-context.md#find-a-node-then-read-its-contents)
help locate code to inspect; tracked reads and edge selections record input
identities and fingerprints, so changes to those inputs can invalidate the
resulting evidence.

{cite}`requirements.sdk.graph.validation-context.shared_context`
{cite}`requirements.sdk.graph.search.search_text.behavior`
{cite}`requirements.sdk.graph.validation-context.outline_source.behavior`
{cite}`requirements.sdk.graph.evidence.inputs`
{cite}`requirements.sdk.graph.evidence.checking_records`
:::

## The application interprets criterion results

:::{req} output_policy
TBV treats `Criterion::Ok` as an application-defined completed answer.
When returning or saving that answer, it does not interpret whether the answer
is favorable or satisfies an application requirement. A negative assessment
can be returned as `Ok(value)`; `Err` represents an execution error instead.
:::

:::{example} result_interpretation
{cite}`requirements.sdk.graph.criteria.output_policy`
{cite}`requirements.sdk.habeas.assessment_policy`

Suppose the `BehavioralAdequacy` criterion above returns this completed answer:

```rust,ignore
Ok(TestReview {
    adequate: false,
    rationale: "No test covers expired tokens.".into(),
})
```

In this example, HC's validation policy requires `adequate` to be `true`.
HC reports the inadequate tests and `habeas status` shows a failed determination,
even when the saved answer is current. Repository validation rejects the negative
assessment. The SDK returns the completed answer; HC applies the judgment that
makes this a validation failure.
:::

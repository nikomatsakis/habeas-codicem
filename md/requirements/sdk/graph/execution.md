# Criteria evaluation

A {term}`requirements.sdk.graph.validation.criterion`'s `execute` method defines
how its question is answered.
`Validator::verify` calls it with a `ValidationContext` that records the inputs
used during execution. This chapter completes the `BehavioralAdequacy` example
from [Criteria definition](validation.md).
[Validators](validators.md) explains how to start that execution.

## Execute the criterion with a context

`execute` receives a `ValidationContext` for its
{term}`requirements.sdk.graph.validation.source_node`. Its `ask` method
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

The builder methods determine what is given to the agent:

- `context` provides common context, including the question. These calls come
  first so the provider can cache the shared prompt prefix.
- `contents_of_current_node()` provides the [contents](fingerprinting.md) of the
  criterion's source node: here, the requirement.
- `contents_of_successors(label)` provides the [contents](fingerprinting.md) of
  its successors with that label: here, the requirement's tests.

:::

:::{cite} requirements.sdk.graph.agent-calls.author_procedure
The context records the contents supplied in the prompt. The agent also has
tools for requesting additional contents, which are recorded when delivered.
Awaiting the ask builder returns the submitted `TestReview`; `verify` returns
a `Fingerprinted` containing the criterion's completed answer and the identity and
fingerprint of each input used during execution. Those fingerprints let the
application check when the answer may be stale.

{cite}`requirements.sdk.graph.agent-calls.judge.submission`
{cite}`requirements.sdk.graph.validators.validate_node`
{cite}`requirements.sdk.graph.evidence.inputs`
:::

Habeas currently runs one combined `Review` criterion at `["review"]`; its
execution selects the requirement and mandatory inputs and applies Habeas's
review policy. This `BehavioralAdequacy` example illustrates how that harness can
be reused for a more specific question.
[Putting it together in Habeas](../habeas.md) follows the current review.

[Agent calls](agent-calls.md) explains the `ask()` builder, prompt construction,
and typed answers. [Agent harness](agent-harness.md) explains how providers and
tools are configured. [Context tools](context-tools.md) explains tracked
source discovery and content reads. Use tracked operations for information the
answer relies on, including filesystem or network inputs.

:::{req} deterministic_procedures
`Criterion::execute` can compute an answer without a configured model provider.
A criterion can compute its answer directly using tracked inputs; deterministic
execution needs no configured agent. Evidence storage is still required.
:::

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

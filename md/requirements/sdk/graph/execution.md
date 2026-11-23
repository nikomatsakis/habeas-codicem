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

The [`ask` builder](agent-calls.md#build-a-request) determines what is given to
the [configured agent](agent-harness.md#configure-the-supplied-harness):

- [`context()`](agent-calls.md#compose-reusable-context) provides common context,
  including the question. These calls come first so the provider can cache the
  shared prompt prefix.
- [`contents_of_current_node()`](agent-calls.md#deliver-and-record-node-inputs)
  provides the [contents](fingerprinting.md) of the criterion's source node:
  here, the requirement.
- [`contents_of_successors(label)`](agent-calls.md#deliver-and-record-node-inputs)
  provides the [contents](fingerprinting.md) of its successors with that label:
  here, the requirement's tests.

For any additional information the answer relies on, use
[tracked context tools](context-tools.md), including for filesystem or network
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
{cite}`requirements.sdk.graph.validators.validate_node`
{cite}`requirements.sdk.graph.evidence.inputs`
:::

:::{req} deterministic_procedures
`Criterion::execute` can compute an answer without a configured model provider.
A criterion can compute its answer directly using tracked inputs; deterministic
execution needs no configured agent. Evidence storage is still required.
:::

:::{cite} requirements.sdk.graph.execution.deterministic_procedures
A criterion can also compute its answer with deterministic Rust code.
Its `ValidationContext` exposes the [inspection operations](context-tools.md)
used by the agent's graph tools.
[Searches and outlines](context-tools.md#find-a-node-then-read-its-contents)
help locate code to inspect; tracked reads and edge selections record input
identities and fingerprints, so changes to those inputs can invalidate the
resulting evidence.

{cite}`requirements.sdk.graph.context-tools.shared_context`
{cite}`requirements.sdk.graph.context-tools.search_text.behavior`
{cite}`requirements.sdk.graph.context-tools.outline_source.behavior`
{cite}`requirements.sdk.graph.evidence.inputs`
{cite}`requirements.sdk.graph.evidence.checking_records`
:::

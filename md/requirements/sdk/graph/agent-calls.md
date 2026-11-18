# Agent calls

An agent call has two parts: reusable instructions and selected node contents.
The question is one of the instructions. Putting it before the node contents
gives the provider a prefix it can cache across reviews of different nodes.

The validation context creates a builder for each call:

```rust,ignore
let answer: ExpirationAssessment = ctx.ask()
    .context((&review_policy, &project_conventions))
    .context(&question)
    .current_node_contents()
    .successors_content(HabeasEdgeLabel::Tests)
    .await?;
```

Here Habeas supplies its review instructions, followed by the requirement and
its tests. Awaiting executes the request; each delivered node becomes an input
to the enclosing validation. A later call can choose different inputs without
erasing the earlier call's observations.

## Compose reusable context

A context value supplies text, borrowing a stored `String` or producing an
owned one:

:::{req} agent_context
```rust,ignore
use std::borrow::Cow;

/// Reusable agent instructions, defined in habeas_sdk::procedure.
pub trait Context {
    fn contents(&self) -> Cow<'_, String>;
}
```

The SDK implements this trait for the following combinations:

| Value | Rendering |
| --- | --- |
| `&C`, where `C: Context + ?Sized` | Forward to the referenced value. |
| `[C]` and `Vec<C>`, where `C: Context` | Render elements in order, separated by two newlines. An empty collection renders an empty string. |
| Heterogeneous tuples of 2 through 12 `Context` values | Render elements in order, separated by two newlines. |

Combinations can nest and preserve repeated elements. Each `.context(value)`
call obtains the text once and retains it for both the prefix and its hash,
without retaining a borrow of `value`. The value needs no `Hash`, `Serialize`,
`Send`, or `Sync` implementation. Repeated calls use the same two-newline
separator as combinations. No context calls give the empty prefix and the
same fingerprint as an explicitly empty rendered prefix.

The cache fingerprint hashes the exact UTF-8 prefix delivered to the agent,
using a versioned scheme. It preserves order and repetition and uses neither
object addresses nor type identities. It identifies this prefix, including the
question; node inputs, the result schema, and the rest of the request are outside
that fingerprint.
:::

For example, the application can define an instruction value:

```rust,ignore
struct Instructions {
    text: String,
}

impl Context for Instructions {
    fn contents(&self) -> std::borrow::Cow<'_, String> {
        std::borrow::Cow::Borrowed(&self.text)
    }
}
```

The result type describes the answer we want:

```rust,ignore
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ExpirationAssessment {
    satisfied: bool,
    rationale: String,
}
```

In the opening example, the tuple combines the first two instruction values.
The same prefix can be built with `.context(&review_policy)` followed by
`.context(&project_conventions)`. A slice or vector handles a variable number
of contexts of one type; tuples combine different types. Context values can
include their own headings when those help the agent. The question is another
context value, independent of the particular nodes being reviewed. The prefix
precedes node contents, allowing the provider adapter to use supported prompt
caching. There is no separate prompt argument or send method. The application
chooses the result type independently for each builder; it need not be
`Judgment`, and can be inferred from the receiving variable or supplied as
`ctx.ask::<ExpirationAssessment>()`.

## Build the request, then await it

The builder's types express the ordering rule: reusable context comes first.
After the first node insertion, only further node insertions and awaiting are
available. Either phase can be awaited, including an empty request.

:::{req} judge
`habeas_sdk::procedure` exposes `Ask` and its public `ContextPhase` and
`NodePhase` state types. The validation context starts a builder:

```rust,ignore
/// Create an independent request borrowing this context through a shared reference.
pub fn ask<R>(&self) -> Ask<'_, 'd, D, R, ContextPhase>
where
    R: serde::de::DeserializeOwned + schemars::JsonSchema + Send;
```

Its operations are:

```rust,ignore
impl<'a, 'd, D: Validation, R> Ask<'a, 'd, D, R, ContextPhase> {
    /// Append reusable text to the prefix.
    pub fn context(self, value: impl Context) -> Self;
}

impl<'a, 'd, D: Validation, R, P> Ask<'a, 'd, D, R, P> {
    /// Queue the current node's contents and enter the node phase.
    pub fn current_node_contents(self) -> Ask<'a, 'd, D, R, NodePhase>;

    /// Queue contents of successors with this label and enter the node phase.
    pub fn successors_content(
        self,
        label: D::EdgeLabel,
    ) -> Ask<'a, 'd, D, R, NodePhase>
    where
        D::EdgeLabel: PartialEq;
}
```

Both states implement `std::future::IntoFuture` with
`Output = Result<R, ValidationError<D::Error>>` and a `Send` future for
`R: serde::de::DeserializeOwned + schemars::JsonSchema + Send`.
Awaiting executes the assembled request, even with no contexts or node
insertions. Building it does not invoke an agent.
:::

:::{req} judge.phases
An ask builder begins in `ContextPhase`. Only that state exposes `context`,
composing the reusable prefix under
{cite}`requirements.sdk.graph.agent-calls.agent_context`. The first
`current_node_contents` or `successors_content` call enters `NodePhase`, even
with an empty successor selection. This state allows further node insertions
and awaiting, with no `context` method or transition back. The builder's types
enforce the restriction.
:::

:::{req} judge.node_contents
Node-insertion methods queue selections. They resolve no contents or edges,
render no inputs, and record no observations while building.

Awaiting with a configured agent resolves these selections in call order:

| Insertion | Contents delivered |
| --- | --- |
| `current_node_contents()` | The node identified by `ValidationContext::node_id`. |
| `successors_content(label)` | Targets of edges whose labels equal `label`, in returned edge order, including duplicate targets. |

Each successful successor selection records the complete outgoing-edge
snapshot, even when nothing matches. Only matching targets have their contents
resolved. Each delivered node is recorded with its supplied snapshot's
fingerprint under {cite}`requirements.sdk.graph.agent-calls.author_procedure`.
Input-resolution failures return `ValidationError::Graph`.
:::

## Render application contents for the agent

`NodeContent` can be parsed source or another application-defined structure.
Habeas renders requirements as text and parsed functions as source code. The
renderer gets a borrowed view of the already-read identity and contents; the SDK
retains those for fingerprinting and tracking.

:::{req} judge.rendering
```rust,ignore
// Defined in habeas_sdk::procedure.
pub struct AgentInput<'i, D: Graph> {
    pub node: &'i D::NodeId,
    pub contents: &'i D::NodeContent,
}

pub trait AgentInputRenderer<D: Graph>: Send + Sync {
    /// Present these supplied contents without fetching replacement contents.
    fn render(&self, input: &AgentInput<'_, D>) -> Result<String, D::Error>;
}
```

The application supplies the renderer through agent context configuration.
Rendering errors return `ValidationError::Graph`. Neither `Graph::NodeContent`
nor `Graph::NodeId` needs serialization or display bounds.
:::

:::{req} judge.caching
The provider adapter makes the rendered prefix available to its supported
prompt-caching mechanism. The prefix includes all context, including the
question, and precedes node contents. A cache hit is not guaranteed.
The prefix fingerprint follows
{cite}`requirements.sdk.graph.agent-calls.agent_context`.
:::

## Return a typed submission

The agent receives tracked inspection tools and a submission tool. The latter
turns the answer into the application's result type.

:::{req} judge.submission
The SDK derives the submission tool's JSON schema from `R` using `schemars`
and instructs the agent to finish through that tool. Its default name is
`submit_result`; harness configuration can preserve an application's existing
names and acceptance rules.

A successful call returns exactly one accepted submitted value, deserialized
into `R`. Ordinary assistant text does not count. Ending without a valid
accepted submission returns an error. Provider failures and invalid terminal
submissions return `ValidationError::Agent`, rather than a node judgment.
:::

The SDK generalizes Habeas's existing conversation harness. Its source tools,
paging, retries, and search-accounting gates can be reused with different typed
results. Habeas's assessment and verification calls have different result
schemas and currently use `submit_judgment` and `submit_verification`,
respectively. Both use the same harness; submission naming is configuration,
not a second SDK validation interface.

Rendered context values are explicit application-supplied text; the SDK does not
infer node identities from arbitrary strings. Node data whose freshness matters
belongs in explicit node insertions or tracked reads. Changes to validation
instructions belong in the procedure version used for evidence freshness.

## Keep a shared trace across calls

The callback can combine ordinary computation, tracked reads, and several agent
calls. Each call has its own conversation state; all contribute observations to
the same validation.

:::{req} author_procedure
`habeas_sdk::procedure` provides a `Send + Sync` per-validation
`ValidationContext<'d, D>` for `D: Validation`, with tracked `inspection(&self)`
and the `ask(&self)` builder under
{cite}`requirements.sdk.graph.agent-calls.judge`.

Inspection works through shared references. Concurrent calls keep independent
builder, conversation, and submission state, with a shared synchronized trace.
The trace belongs to the enclosing validation.
They require no exclusive context borrow across an agent call and do not
serialize complete interactions behind a context-wide lock.

The trace records the identities and supplied-content fingerprints of inputs
actually delivered, including tool reads. Awaiting with fewer or no node
insertions preserves earlier dependencies; an unawaited builder adds no
agent-delivery dependencies. Callbacks receive only the context under
{cite}`requirements.sdk.graph.validation.check_procedure`, without requiring
evidence storage. This is the same callback interface with or without agent calls.
:::

:::{req} procedures
An application-supplied assessment procedure can perform ordinary computation
and request zero or more model judgments. It can use intermediate judgments in
later computation and requests, then return a boolean judgment with
graph-defined metadata.
:::

An unconfigured agent is an error-returning implementation of the agent
interface. A validation can still perform all of its ordinary computation and
tracked reads. If it calls the agent, it receives a clear error. If the callback
propagates that error, the runner records no completed judgment. A callback can
also handle the error and reach a judgment by other means.

:::{req} model_capability
Without a configured agent, awaiting returns
`ValidationError::NoAgentConfigured`. It invokes no provider, resolves and
renders no queued node insertions, and fabricates no response. If the callback
propagates the error, the runner reports it without storing a completed
judgment. Deterministic validations need no separate callback trait.
:::

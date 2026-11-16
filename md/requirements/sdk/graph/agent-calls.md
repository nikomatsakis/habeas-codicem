# Agent calls


A validation asks an agent through its context. It first supplies reusable
context, including the question, then explicitly inserts the node contents
needed for this call. Awaiting the builder executes the request. Each reusable
value implements `Context`, which provides its text. A value can borrow a stored
`String` or produce an owned one:

:::{req} agent_context
`habeas_sdk::procedure` MUST expose the following trait:

```rust,ignore
use std::borrow::Cow;

pub trait Context {
    fn contents(&self) -> Cow<'_, String>;
}
```

The SDK MUST implement `Context` for `&C` where `C: Context + ?Sized`, and for
`[C]` and `Vec<C>` where `C: Context`. It MUST also implement `Context` for each
heterogeneous tuple arity from 2 through 12, with every element implementing
`Context`. A reference MUST forward to the referenced value. Slices, vectors,
and tuples MUST render each element in order and join the resulting contents
with two newline characters. Empty slices and vectors MUST render an empty
string. Composite contexts MUST preserve repeated elements and support nesting.

Each builder `context(value)` call MUST obtain the value's contents once and
retain that text for both prefix construction and hashing. It MUST NOT require
the value to implement `Hash`, `Serialize`, `Send`, or `Sync`, or retain a
borrow of the value after the call returns. Repeated builder calls MUST combine
those texts using the same two-newline separator as composite contexts. A
builder with no context calls MUST produce the empty prefix and the same
fingerprint as an explicitly empty rendered prefix.

The SDK MUST compute the context-prefix cache fingerprint from the exact UTF-8
text of that rendered prefix using a versioned hashing scheme. It MUST NOT
hash Rust object addresses or type identities. The fingerprint MUST describe
the same prefix delivered to the agent. Ordering and repetitions MUST be
preserved; contexts MUST NOT be treated as an unordered or deduplicated set.
This fingerprint identifies the prefix, including the question, not node inputs,
result schema, or the complete agent request.
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

With `review_policy`, `project_conventions`, and `question` holding `Instructions`
values, a tuple combines the first two in one call:

```rust,ignore
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ExpirationAssessment {
    satisfied: bool,
    rationale: String,
}

let answer: ExpirationAssessment = ctx.ask()
    .context((&review_policy, &project_conventions))
    .context(&question)
    .current_node_contents()
    .successors_content(HabeasEdgeLabel::Tests)
    .await?;
```

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

`AgentInput` is a borrowed view of a node identity and its already-read contents,
supplied by the SDK to the application's renderer. The callback chooses inputs
through the builder rather than assembling a slice of these views.

`NodeContent` is application-defined and may be a parsed structure rather than
text. The renderer answers one narrow question: how should that particular
value be presented to the agent? For example, Habeas can render a requirement
as its text and a parsed function as its source code. The SDK retains the node
identity and fingerprints the supplied contents for tracking; the renderer only
provides the agent-readable presentation. A validation chooses the current node
and labeled successors for each call, or awaits without inserting any node
contents. The agent also receives the SDK's tracked inspection tools. Making a
different selection for a later call does not remove observations already
recorded for the enclosing validation.

:::{req} judge
`habeas_sdk::procedure` MUST expose an `Ask` builder with two public state types,
`ContextPhase` and `NodePhase`. `ValidationContext<'d, D>` MUST expose:

```rust,ignore
pub fn ask<R>(&self) -> Ask<'_, 'd, D, R, ContextPhase>
where
    R: serde::de::DeserializeOwned + schemars::JsonSchema + Send;
```

For `D: Validation`, the builder MUST expose the following operations:

```rust,ignore
impl<'a, 'd, D: Validation, R> Ask<'a, 'd, D, R, ContextPhase> {
    pub fn context(self, value: impl Context) -> Self;
}

impl<'a, 'd, D: Validation, R, P> Ask<'a, 'd, D, R, P> {
    pub fn current_node_contents(self) -> Ask<'a, 'd, D, R, NodePhase>;

    pub fn successors_content(
        self,
        label: D::EdgeLabel,
    ) -> Ask<'a, 'd, D, R, NodePhase>
    where
        D::EdgeLabel: PartialEq;
}
```

Each `ask()` MUST create an independent builder borrowing the validation context
through a shared reference. Both supported states MUST implement
`std::future::IntoFuture` with `Output = Result<R, ValidationError<D::Error>>`
and a `Send` future, for
`R: serde::de::DeserializeOwned + schemars::JsonSchema + Send`. Awaiting either
state MUST execute the assembled request, including a request with no contexts
or node insertions. Building the request MUST NOT invoke an agent.
:::

:::{req} judge.phases
An ask builder MUST begin in `ContextPhase`. Only that state MUST expose
`context`, composing the reusable prefix under
{cite}`requirements.sdk.graph.agent-calls.agent_context`. The first
`current_node_contents` or `successors_content` call MUST transition to
`NodePhase`, even when the successor selection is empty. That state MUST permit
further node insertions and awaiting, but MUST NOT expose `context` or a
transition back to `ContextPhase`. The SDK MUST enforce this restriction through
the builder's types.
:::

:::{req} judge.node_contents
Builder node-insertion methods MUST queue their selections without resolving
node contents or outgoing edges, rendering node inputs, or recording
observations. When an ask builder with a configured agent is awaited,
`current_node_contents` MUST insert the contents
of the node identified by `ValidationContext::node_id`. `successors_content(label)`
MUST inspect that node's outgoing edges and insert the target contents of every
edge whose label compares equal to the supplied label. Insertions MUST follow
builder-call order; successors within an insertion MUST retain the returned
edge order and one entry per matching edge, including duplicate targets.

Each successful successor selection MUST record the complete outgoing-edge
snapshot, including when no edge matches. Target contents MUST be resolved only
for matching edges. Each node actually delivered MUST be recorded with the
fingerprint of the supplied snapshot under
{cite}`requirements.sdk.graph.agent-calls.author_procedure`. Input resolution
failures MUST be reported as `ValidationError::Graph`.
:::

:::{req} judge.rendering
`habeas_sdk::procedure` MUST expose:

```rust,ignore
pub struct AgentInput<'i, D: Graph> {
    pub node: &'i D::NodeId,
    pub contents: &'i D::NodeContent,
}

pub trait AgentInputRenderer<D: Graph>: Send + Sync {
    fn render(&self, input: &AgentInput<'_, D>) -> Result<String, D::Error>;
}
```

The application MUST supply an `AgentInputRenderer<D>` through agent context
configuration. Rendering MUST use the supplied contents without fetching
replacement contents. The SDK MUST report rendering errors as
`ValidationError::Graph` and MUST NOT impose serialization or display bounds on
`Graph::NodeContent` or `Graph::NodeId`.
:::

:::{req} judge.caching
The provider adapter MUST make the rendered context prefix available to its
supported prompt-caching mechanism. The prefix MUST include all supplied context,
including the question, and precede inserted node contents. A cache hit is not
guaranteed. The context-prefix fingerprint MUST follow
{cite}`requirements.sdk.graph.agent-calls.agent_context`.
:::

:::{req} judge.submission
The SDK MUST provide a structured submission tool with a JSON schema
derived from `R` using `schemars`, and instruct the agent to finish by submitting
its structured result through that tool. The default tool name MUST be
`submit_result`; the configured harness MAY retain an application's existing
submission names and acceptance rules. A successful call MUST return exactly
one accepted submitted value deserialized into `R`. Ordinary assistant text
MUST NOT count as a submitted result. An invocation ending without a valid
accepted submission MUST return an error. Provider failures and invalid terminal
submissions MUST be reported as `ValidationError::Agent`, not as a node judgment.
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

:::{req} author_procedure
`habeas_sdk::procedure` MUST provide a `Send + Sync` per-validation
`ValidationContext<'d, D>` for `D: Validation`. The context MUST expose tracked
inspection through `inspection(&self)` and the `ask(&self)` builder specified by
{cite}`requirements.sdk.graph.agent-calls.judge`. Inspection operations MUST be callable
through shared references. Concurrent calls MUST retain independent builder,
conversation, and submission state while recording their observations in the
same synchronized dependency trace for the enclosing validation. They MUST NOT
require an exclusive context borrow across an agent call or serialize complete
agent interactions behind a context-wide lock.

The SDK MUST record identified inputs actually delivered to an agent using the
fingerprints of their supplied contents, together with delivered tool reads.
Awaiting a builder with fewer or no node insertions MUST NOT discard dependencies
already captured for the validation. Preparing an unawaited builder MUST NOT
create agent-delivery dependencies. The callback MUST receive only the context
specified by
{cite}`requirements.sdk.graph.validation.check_procedure` and MUST NOT require
evidence-storage access. The SDK MUST use this single validation interface for
callbacks with and without agent calls.
:::

:::{req} procedures
The SDK MUST support consumer-supplied assessment procedures that perform
ordinary computation and request zero or more model judgments. A procedure
MUST be able to use intermediate judgment results in subsequent computation and
judgment requests, and return a boolean judgment with graph-defined metadata.
:::

An unconfigured agent is an error-returning implementation of the agent
interface. A validation can still perform all of its ordinary computation and
tracked reads. If it calls the agent, it receives a clear error. If the callback
propagates that error, the runner records no completed judgment. A callback can
also handle the error and reach a judgment by other means.

:::{req} model_capability
When no agent is configured, awaiting an ask builder
MUST return `ValidationError::NoAgentConfigured`. It MUST NOT invoke a provider
or resolve or render queued node insertions or fabricate an agent response.
If the validation callback propagates that
error, the runner MUST report the error and MUST NOT store a completed judgment.
This runtime behavior MUST NOT require a separate callback trait for
validations that do not call an agent.
:::

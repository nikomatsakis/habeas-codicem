# Agent calls


A validation asks an agent through its context. It builds a stable prefix from
application-defined context values, then supplies the question and selected
inputs when sending the prompt. Each value implements `Context`, which provides
its text. A value can borrow a stored `String` or produce an owned one:

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
This fingerprint identifies the prefix, not the varying prompt, node inputs,
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

With `review_policy` and `project_conventions` holding `Instructions` values,
a tuple combines both in one call:

```rust,ignore
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ExpirationAssessment {
    satisfied: bool,
    rationale: String,
}

let answer: ExpirationAssessment = ctx.ask()
    .context((&review_policy, &project_conventions))
    .prompt("Does this implementation reject expired tokens?", &agent_inputs)
    .await?;
```

The same prefix can be built with `.context(&review_policy)` followed by
`.context(&project_conventions)`. A slice or vector handles a variable number
of contexts of one type; tuples combine different types. Context values can
include their own headings when those help the agent. The prefix precedes the varying
prompt and inputs, allowing the provider adapter to use supported prompt
caching. The final `prompt` call starts the agent interaction. The application
chooses the result type independently for each call; it need not be `Judgment`.

`AgentInput` is a borrowed view of a node identity and its already-read contents,
selected for one agent call. Unlike `RootInput`, it carries no edge: an agent
can be shown a node that was discovered later or the source node itself.

`NodeContent` is application-defined and may be a parsed structure rather than
text. The renderer answers one narrow question: how should that particular
value be presented to the agent? For example, Habeas can render a requirement
as its text and a parsed function as its source code. The SDK retains the node
identity and fingerprints the supplied contents for tracking; the renderer only
provides the agent-readable presentation. A validation
normally selects them from its subject and root inputs, but can provide another
selection or an empty slice. The agent also receives the SDK's tracked inspection
tools. Selecting fewer inputs for a particular call does not remove observations
already recorded for the enclosing validation.

:::{req} judge
`habeas_sdk::procedure` MUST expose the following agent input type:

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
configuration to render the identified node contents for the agent. Rendering
MUST use the supplied contents without fetching replacement contents. The SDK
MUST report rendering errors as `ValidationError::Graph` and MUST NOT impose
serialization or display bounds on `Graph::NodeContent` or `Graph::NodeId`.

`ValidationContext<'d, D>` MUST provide `ask` with this signature:

```rust,ignore
pub fn ask(&self) -> Ask<'_, 'd, D>;
```

The returned builder `Ask` MUST provide the following methods:

```rust,ignore
pub fn context(self, value: impl Context) -> Self;

pub fn prompt<'a, R>(
    self,
    prompt: &'a str,
    inputs: &'a [AgentInput<'a, D>],
) -> impl Future<Output = Result<R, ValidationError<D::Error>>> + Send + 'a
where
    Self: 'a,
    R: serde::de::DeserializeOwned + schemars::JsonSchema + Send + 'a;
```

Each `ask()` MUST create an independent builder borrowing the context through
a shared reference. Repeated `context` calls MUST compose values according to
{cite}`requirements.sdk.graph.agent-calls.agent_context` and place the resulting prefix
before the per-call prompt and inputs.
Building the request MUST NOT invoke the agent. `prompt` MUST execute the
request with the supplied selection, including an empty slice. The provider
adapter MUST make this stable prefix available to its supported prompt-caching
mechanism; a cache hit is not guaranteed.

The SDK MUST automatically provide a `submit_result` tool with a JSON schema
derived from `R` using `schemars`, and instruct the agent to finish by submitting
its structured result through that tool. A successful call MUST return the
submitted value deserialized into `R`. Ordinary assistant text MUST NOT count
as a submitted result. An invocation ending without a valid submission MUST
return an error. Provider failures and invalid terminal submissions MUST be
reported as `ValidationError::Agent`, not as a node judgment.
:::

The prefix can hold instructions shared by many nodes, while the prompt varies
per node. Rendered context values and prompt text are explicit application-supplied
text; the SDK does not infer node identities from arbitrary strings. Node data
whose freshness matters should be supplied as identified inputs or read through
the tracked tools. Changes to validation instructions belong in the procedure
version used for evidence freshness.

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
Choosing a subset or empty slice for an agent call MUST NOT discard dependencies
already captured for the validation, including its initial roots.
The callback MUST receive the subject and prepared inputs specified by
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
When no agent is configured, executing an ask builder's `prompt`
MUST return `ValidationError::NoAgentConfigured`. It MUST NOT invoke a provider
or fabricate an agent response. If the validation callback propagates that
error, the runner MUST report the error and MUST NOT store a completed judgment.
This runtime behavior MUST NOT require a separate callback trait for
validations that do not call an agent.
:::

# Validators

A validator runs the values introduced in [Criteria](criteria.md).
It supplies the application domain and an execution context to the criterion,
then returns its completed answer with a trace of the inputs it used.

## Run criteria with a validator

:::{req} validator
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

A `Validator<D>` owns a cloneable domain. Construct it with `new`, then submit a
criterion to `verify` to evaluate it and return its answer with the inputs used
to produce it.

Independent verifications share a validator through `&self`. Cached observations
can be reused when they still represent the state being checked. Each execution
has its own context, dependency trace, and agent conversation state. Operations
within one execution share its synchronized trace through context inspection
and agent calls.

Concurrent verifications of the same source and criterion run independently,
each with its own context and trace. [Storage](evidence.md#look-up-a-source-and-criterion)
describes atomic replacement when results are saved.

```rust,ignore
pub struct Validator<D: Domain> { /* private fields */ }
```
:::

### Construct a validator

:::{req} validator.new
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Give `new` the application domain. It owns the domain and starts with no agent configured. You can run criteria that read graph data directly.

```rust,ignore
impl<D: Domain> Validator<D> {
    pub fn new(domain: D) -> Self;
}
```
:::

### Configure agent requests

:::{req} validator.with_agent
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Supply an `AgentConfig` when a criterion calls `ask`. Backend setup happens when the request is awaited. See {cite}`requirements.sdk.graph.agent-harness.with_agent`.

```rust,ignore
impl<D: Domain> Validator<D> {
    pub fn with_agent(self, config: AgentConfig) -> Self
    where
        D: 'static,
        D::EdgeLabel: DeserializeOwned + JsonSchema + PartialEq,
        D::Error: Display;
}
```
:::

### Attach project sources

:::{req} validator.with_sources
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Supply the files and languages the agent can inspect during a check. Sources and agent configuration can be attached in either order under {cite}`requirements.sdk.sources.provider.attachment`.

```rust,ignore
impl<D: SourceNodes + 'static> Validator<D>
where
    D::EdgeLabel: DeserializeOwned + JsonSchema + PartialEq,
    D::Error: Display
{
    pub fn with_sources(self, sources: Sources) -> Self;
}
```
:::

### Execute a criterion

:::{req} validator.verify
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Run the criterion with a fresh `ValidationContext`. `execute` borrows it for this call. The returned `Fingerprinted` owns the answer and the dependencies collected during execution. Call `store_evidence` to persist it. See {cite}`requirements.sdk.graph.validators.validate_node`.

```rust,ignore
impl<D: Domain> Validator<D> {
    pub fn verify<V: Criterion<D>>(
        &self,
        criterion: V,
    ) -> impl Future<Output = Result<Fingerprinted<D, V::Ok>, ValidationError<D::Error>>> + Send;
}
```
:::

### Save a completed answer

:::{req} validator.store_evidence
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Encode the answer and replace the record for its source node and criterion path, preserving its version and dependencies under {cite}`requirements.sdk.graph.evidence.store_evidence`.

```rust,ignore
impl<D: Domain> Validator<D> {
    pub fn store_evidence<O: Serialize>(
        &self,
        inputs: &Fingerprinted<D, O>,
    ) -> impl Future<Output = Result<(), ValidationError<D::Error>>> + Send;
}
```
:::

### Load an earlier answer

:::{req} validator.load_evidence
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Use the criterion to locate and decode a saved answer. `Ok(None)` means there is no record. A loaded answer can be stale. See {cite}`requirements.sdk.graph.evidence.check_node`.

```rust,ignore
impl<D: Domain> Validator<D> {
    pub fn load_evidence<V: Criterion<D>>(
        &self,
        criterion: &V,
    ) -> impl Future<Output = Result<Option<Fingerprinted<D, V::Ok>>, ValidationError<D::Error>>> + Send;
}
```
:::

### Check the recorded inputs

:::{req} validator.check_inputs
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Compare each saved observation with the current domain. A changed fingerprint returns `Ok(false)`. The caller checks criterion-version compatibility separately under {cite}`requirements.sdk.graph.evidence.checking_records.version_match`.

```rust,ignore
impl<D: Domain> Validator<D> {
    pub fn check_inputs<O>(
        &self,
        inputs: &Fingerprinted<D, O>,
    ) -> Result<bool, D::Error>;
}
```
:::

### Reuse an answer or run the criterion

:::{req} validator.refresh_evidence
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Return the saved answer when its version is compatible and its inputs are current. Otherwise execute the criterion and store the new result under {cite}`requirements.sdk.graph.evidence.refresh_evidence`.

```rust,ignore
impl<D: Domain> Validator<D> {
    pub fn refresh_evidence<V: Criterion<D>>(
        &self,
        criterion: V,
    ) -> impl Future<Output = Result<Fingerprinted<D, V::Ok>, ValidationError<D::Error>>> + Send;
}
```
:::

:::{req} validate_node
Every call constructs a fresh `ValidationContext` identifying the source node
and calls `Criterion::execute` with that context. Its completed answer, including
a negative assessment, and captured dependencies form the returned `Fingerprinted`
under {cite}`requirements.sdk.graph.evidence.inputs`.
:::

:::{req} validate_node.errors
An execution error returns `Err` rather than a `Fingerprinted` containing a
completed answer.
:::

:::{req} ValidationError
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Distinguishes a domain operation failure, a missing agent, an agent failure, and an error encoding or decoding the output.

```rust,ignore
pub enum ValidationError<E> {
    Graph(E),
    NoAgentConfigured,
    Agent(String),
    Output(serde_json::Error),
}
```

Trait implementations

```rust,ignore
impl<E: Debug> Debug for ValidationError<E>;
impl<E: Display> Display for ValidationError<E>;
impl<E: Error + 'static> Error for ValidationError<E>;
```
:::

:::{example} evaluate_criterion
{cite}`requirements.sdk.graph.validators.validator`
{cite}`requirements.sdk.graph.validators.validate_node`
{cite}`requirements.sdk.graph.agent-harness.with_agent`
{cite}`requirements.sdk.graph.evidence.refresh_evidence`
{cite}`requirements.sdk.graph.evidence.inputs`

We can now instantiate `BehavioralAdequacy` and evaluate it with a `Validator`.
Here `domain` supplies Habeas Codicem's graph, contents, and storage, and
`agent_config` selects one of the
[SDK-supplied agents](agent-harness.md#configure-the-validators-agent).
[`with_agent`](agent-harness.md#configure-the-validators-agent) configures that
agent for the criterion's requests.
The `refresh_evidence` helper reuses the previous answer when the review rules
and inputs are current, or evaluates the criterion again:

```rust,ignore
let validator = Validator::new(domain.clone())
    .with_agent(agent_config.clone());

let inputs = validator.refresh_evidence(BehavioralAdequacy {
    requirement: requirement.clone(),
    review_policy: test_review_policy.clone(),
}).await?;

let assessment: &TestReview = inputs.output();
println!("{}: {}", assessment.adequate, assessment.rationale);
```
:::

[Storage](evidence.md#refresh-evidence) defines `refresh_evidence` and explains
the load, check, verify, and store steps it combines.
[Criteria](criteria.md#execute-the-criterion-with-a-context) fills in the
criterion's execution method.


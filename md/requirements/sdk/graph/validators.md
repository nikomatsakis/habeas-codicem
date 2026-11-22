# Validators

## Run criteria with a validator

A `Validator<D>` owns a cloneable domain. Construct it with `new`, then submit a
criterion to `verify` to evaluate it and return its answer with the inputs used
to produce it.

:::{req} validator
```rust,ignore
// Defined in habeas_sdk::graph.
/// Domain access and configuration shared by criterion executions.
pub struct Validator<D: Domain> { /* private fields */ }

impl<D: Domain> Validator<D> {
    /// Own the domain used to check and execute criteria.
    pub fn new(domain: D) -> Self;
}
```

Independent verifications share a validator through `&self`. Cached observations
can be reused when they still represent the state being checked. Each execution
has its own context, dependency trace, and agent conversation state. Operations
within one execution share its synchronized trace through context inspection
and agent calls.

Concurrent verifications of the same source and criterion run independently,
each with its own context and trace. [Storage](evidence.md#look-up-a-source-and-criterion)
describes atomic replacement when results are saved.
:::

:::{req} validate_node
```rust,ignore
// Method on Validator<D>.
/// Execute the criterion and return its completed answer and observed inputs.
pub fn verify<V: Criterion<D>>(
    &self,
    criterion: V,
) -> impl Future<Output = Result<Fingerprinted<D, V::Ok>, ValidationError<D::Error>>> + Send;
```

Every call constructs a fresh `ValidationContext` identifying the source node
and calls `Criterion::execute` with that context. Its completed answer, including
a negative assessment, and captured dependencies form the returned `Fingerprinted`
under {cite}`requirements.sdk.graph.evidence.inputs`.
:::

:::{req} validate_node.errors
An execution error returns `Err` rather than a `Fingerprinted` containing a
completed answer.
:::

:::{example} evaluate_criterion
{cite}`requirements.sdk.graph.validators.validator`
{cite}`requirements.sdk.graph.validators.validate_node`
{cite}`requirements.sdk.graph.agent-harness.with_agent`
{cite}`requirements.sdk.graph.evidence.refresh_evidence`
{cite}`requirements.sdk.graph.evidence.inputs`

We can now instantiate `BehavioralAdequacy` and evaluate it with a `Validator`.
Here `domain` supplies Habeas's graph, contents, and storage, and `agent`
implements the [agent interface](agent-harness.md#the-agent-interface).
[`with_agent`](agent-harness.md#configure-the-validators-agent) configures that
agent for the criterion's requests.
The `refresh_evidence` helper reuses the previous answer when the review rules
and inputs are current, or evaluates the criterion again:

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


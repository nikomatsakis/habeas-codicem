# Running criteria

The functions in `trust_but_verify::graph` take a reference to the application
domain. Use `verify` to run a criterion or `refresh_evidence` to reuse a saved
answer when its inputs and version are current.

## Reuse an answer or run the criterion

:::{req} refresh_evidence
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Return the saved answer when its version is compatible and its inputs are current. Otherwise execute the criterion and store the new result under {cite}`requirements.sdk.graph.evidence.refresh_evidence`.

```rust,ignore
pub fn refresh_evidence<D: Domain, V: Criterion<D>>(
    domain: &D,
    criterion: V,
) -> impl Future<Output = Result<Fingerprinted<D, V::Ok>, ValidationError<D::Error>>> + Send;
```
:::

:::{example} evaluate_criterion
{cite}`requirements.sdk.graph.running-criteria.validate_node`
{cite}`requirements.sdk.graph.agent-harness.with_agent`
{cite}`requirements.sdk.graph.evidence.refresh_evidence`
{cite}`requirements.sdk.graph.evidence.inputs`

The domain supplies Habeas Codicem's graph, contents, and storage. Its
[validation cache](../validation-cache.md#configure-the-agent) selects the
agent that answers the criterion's requests.
The `refresh_evidence` helper reuses the previous answer when the review rules
and inputs are current, or evaluates the criterion again:

```rust,ignore
use trust_but_verify::graph::refresh_evidence;

let inputs = refresh_evidence(&domain, BehavioralAdequacy {
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

## Execute a criterion

:::{req} verify
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Run the criterion with a fresh `ValidationContext`. `execute` borrows it for this call. The returned `Fingerprinted` owns the answer and the dependencies collected during execution. Call `store_evidence` to persist it. See {cite}`requirements.sdk.graph.running-criteria.validate_node`.

```rust,ignore
pub fn verify<D: Domain, V: Criterion<D>>(
    domain: &D,
    criterion: V,
) -> impl Future<Output = Result<Fingerprinted<D, V::Ok>, ValidationError<D::Error>>> + Send;
```
:::

:::{req} verify.concurrent

Concurrent `verify` calls on the same domain keep separate dependency
records and agent conversations, even for the same source node and criterion.
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

## Save a completed answer

:::{req} store_evidence
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Encode the answer and replace the record for its source node and criterion path, preserving its version and dependencies under {cite}`requirements.sdk.graph.evidence.store_evidence`.

```rust,ignore
pub fn store_evidence<D: Domain, O: Serialize>(
    domain: &D,
    inputs: &Fingerprinted<D, O>,
) -> impl Future<Output = Result<(), ValidationError<D::Error>>> + Send;
```
:::

## Load an earlier answer

:::{req} load_evidence
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Use the criterion to locate and decode a saved answer. `Ok(None)` means there is no record. A loaded answer can be stale. See {cite}`requirements.sdk.graph.evidence.check_node`.

```rust,ignore
pub fn load_evidence<D: Domain, V: Criterion<D>>(
    domain: &D,
    criterion: &V,
) -> impl Future<Output = Result<Option<Fingerprinted<D, V::Ok>>, ValidationError<D::Error>>> + Send;
```
:::

## Check the recorded inputs

:::{req} check_inputs
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Compare each saved observation with the current domain. A changed fingerprint returns `Ok(false)`. The caller checks criterion-version compatibility separately under {cite}`requirements.sdk.graph.evidence.checking_records.version_match`.

```rust,ignore
pub fn check_inputs<D: Domain, O>(
    domain: &D,
    inputs: &Fingerprinted<D, O>,
) -> Result<bool, D::Error>;
```
:::

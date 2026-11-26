# Running criteria

:::{definition} validator
:phrase: validator

A *validator* runs criteria against one borrowed domain and accumulates agent usage for the caller's operation. The caller chooses the operation's scope by creating a validator and using it for one or more criteria.
:::

## Create a validator

:::{req} Validator
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Each validator owns its usage counters and updates them through an exclusive borrow. The borrowed domain supplies the shared node identities, outline snapshots, and agent configuration under {cite}`requirements.sdk.validation-cache.cache`.

```rust,ignore
pub struct Validator<'d, D: Domain> { /* private fields */ }
```
:::

:::{req} Validator.new
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

`Validator::new` borrows the domain for the validator's lifetime. All counters start at zero, including when another validator has already run criteria on the same domain.

```rust,ignore
impl<'d, D: Domain> Validator<'d, D> {
    pub fn new(domain: &'d D) -> Self;
}
```
:::

## Reuse an answer or run the criterion

:::{req} refresh_evidence
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Return the saved answer when its version is compatible and its inputs are current. Otherwise execute the criterion and store the new result under {cite}`requirements.sdk.graph.evidence.refresh_evidence`.

```rust,ignore
impl<'d, D: Domain> Validator<'d, D> {
    pub fn refresh_evidence<V: Criterion<D>>(
        &mut self,
        criterion: V,
    ) -> impl Future<Output = Result<Fingerprinted<D, V::Ok>, ValidationError<D::Error>>> + Send;
}
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
The validator’s `refresh_evidence` method reuses the previous answer when the review rules
and inputs are current, or evaluates the criterion again:

```rust,ignore
use trust_but_verify::graph::Validator;

let mut validator = Validator::new(&domain);
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

## Execute a criterion

:::{req} verify
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Run the criterion with a fresh `ValidationContext`. `execute` borrows it for this call. The returned `Fingerprinted` owns the answer and the dependencies collected during execution. Call `store_evidence` to persist it. See {cite}`requirements.sdk.graph.running-criteria.validate_node`.

```rust,ignore
impl<'d, D: Domain> Validator<'d, D> {
    pub fn verify<V: Criterion<D>>(
        &mut self,
        criterion: V,
    ) -> impl Future<Output = Result<Fingerprinted<D, V::Ok>, ValidationError<D::Error>>> + Send;
}
```
:::

:::{req} verify.concurrent

Separate validators can verify the same source node and criterion concurrently on the same domain. Each execution has its own dependency trace and agent conversation. Each validator counts only its own executions.
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

## Inspect usage

:::{req} Validator.usage
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

`usage()` returns the validator’s cumulative usage. Every agent call contributes its usage to the execution that invoked it, including calls that return an error. The validator adds that usage before `verify` or `refresh_evidence` returns, including on `Err`. Reusing current evidence adds no usage. Loading, checking, and storing evidence also leave the counters unchanged.

For totals across validators that share a domain cache, see {cite}`requirements.sdk.validation-cache.usage`.

```rust,ignore
impl<'d, D: Domain> Validator<'d, D> {
    pub fn usage(&self) -> &UsageStats;
}
```
:::

:::{req} UsageStats
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

`UsageStats` exposes the following totals. `attempts` counts each request sent to the provider once. `retries` counts attempts that repeat a failed request. `failed_attempts` counts attempts that returned a provider error, including those followed by a successful retry.

```rust,ignore
#[non_exhaustive]
pub struct UsageStats {
    pub attempts: u64,
    pub retries: u64,
    pub failed_attempts: u64,
    pub input_tokens: TokenCount,
    pub output_tokens: TokenCount,
    pub cached_input_tokens: TokenCount,
    pub cache_creation_input_tokens: TokenCount,
    pub reasoning_tokens: TokenCount,
}
```

Trait implementations

```rust,ignore
impl Clone for UsageStats;
impl Copy for UsageStats;
impl Debug for UsageStats;
impl Default for UsageStats;
impl Serialize for UsageStats;
```
:::

:::{req} TokenCount
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

For each token category, `reported` sums the provider’s measurements and `missing_attempts` counts attempts that supplied no measurement for that category. A reported zero is a measurement. Input and output totals count each token once, even when the provider also reports it in a cache or reasoning breakdown.

```rust,ignore
#[non_exhaustive]
pub struct TokenCount {
    pub reported: u64,
    pub missing_attempts: u64,
}
```

Trait implementations

```rust,ignore
impl Clone for TokenCount;
impl Copy for TokenCount;
impl Debug for TokenCount;
impl Default for TokenCount;
impl Serialize for TokenCount;
```
:::

:::{example} usage_after_failure
{cite}`requirements.sdk.graph.running-criteria.Validator.usage`
{cite}`requirements.sdk.graph.running-criteria.UsageStats`
{cite}`requirements.sdk.graph.running-criteria.TokenCount`

One provider request fails with throttling and supplies no token counts. Its retry succeeds and reports 120 input tokens. A second `verify` on the same validator fails after one provider attempt, also without token counts. After the second `verify` returns `Err`, `validator.usage()` shows three attempts, one retry, and two failed attempts. `input_tokens.reported` is 120 and `input_tokens.missing_attempts` is two. A fresh validator borrowing the same domain starts with zero in all five of those counters.
:::

## Save a completed answer

:::{req} store_evidence
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Encode the answer and replace the record for its source node and criterion path, preserving its version and dependencies under {cite}`requirements.sdk.graph.evidence.store_evidence`.

```rust,ignore
impl<'d, D: Domain> Validator<'d, D> {
    pub fn store_evidence<O: Serialize>(
        &self,
        inputs: &Fingerprinted<D, O>,
    ) -> impl Future<Output = Result<(), ValidationError<D::Error>>> + Send;
}
```
:::

## Load an earlier answer

:::{req} load_evidence
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Use the criterion to locate and decode a saved answer. `Ok(None)` means there is no record. A loaded answer can be stale. See {cite}`requirements.sdk.graph.evidence.check_node`.

```rust,ignore
impl<'d, D: Domain> Validator<'d, D> {
    pub fn load_evidence<V: Criterion<D>>(
        &self,
        criterion: &V,
    ) -> impl Future<Output = Result<Option<Fingerprinted<D, V::Ok>>, ValidationError<D::Error>>> + Send;
}
```
:::

## Check the recorded inputs

:::{req} check_inputs
:test: crates/trust-but-verify/tests/public_api.rs#fn[public_exports_match_the_appendix]

Compare each saved observation with the current domain. A changed fingerprint returns `Ok(false)`. The caller checks criterion-version compatibility separately under {cite}`requirements.sdk.graph.evidence.checking_records.version_match`.

```rust,ignore
impl<'d, D: Domain> Validator<'d, D> {
    pub fn check_inputs<O>(
        &self,
        inputs: &Fingerprinted<D, O>,
    ) -> Result<bool, D::Error>;
}
```
:::

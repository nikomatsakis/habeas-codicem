# Assessment calibration suite (v1)

Eight cases, arranged as four gap/fix pairs, extracted from this repository's
history. This is a small development set for tuning model profiles and future
tiered assessment (issue #36), not a model ranking or a held-out evaluation.

| Pair | Gap to recognize | Source |
| --- | --- | --- |
| pagination-boundary | An unredacted result just below 32 KiB is unnecessarily paged: a real runtime bug, plus a missing boundary test. | Actual snapshots before fix `3c6fd79` and after recorded reassessment `c46ba0e`. |
| live-test-root | A working integration test lacks an implementation reference to its own behavioral requirement. | Sol finding retained in `32562d1`; reconstruct by removing the repaired marker. |
| openai-concurrency | Runtime default/override logic exists, but the tests exercise only Bedrock/Ollama. | Sol finding retained in `32562d1`; reconstruct by omitting OpenAI from both test loops. |
| deleted-requirement | The example claims automatic orphan-evidence deletion; the test actually deletes the evidence itself. | Sol finding retained in `32562d1`; restore the old prose and test setup. |

The paired fixed cases are equally important: a model should stop reporting a
gap after it is repaired. Labels were reviewed against source and fixes, not
assigned by model vote. Only the pagination pair has historical false/true
assessments of both snapshots. The other fixed labels remain calibration
expectations: the latest Sol refresh ran out of credits before independently
confirming the repairs. A credible new finding should trigger label review,
not automatic dismissal as a false alarm.

The initial Luna/Sol comparison in `audits/openai-comparison-2026-11-15.json`
provides useful motivation, including Luna's incorrect claim that case-sensitive
configuration selection was untested. Its original working tree was not
committed (only a diff hash was retained), so this suite does not claim to replay
those exact inputs. Add that false-alarm example once a reviewed reconstruction
is available. Also add FIXME outcomes, tool-recovery cases, and held-out subjects
before using the suite to choose an automatic acceptance policy.

## Run without calling a model

From the repository root:

```sh
python3 benchmarks/assessment/run.py check
python3 -m unittest discover -s benchmarks/assessment -p 'test_*.py'
```

`check` materializes every snapshot and checks all mutation anchors. It prints
source-tree hashes. It does not compile historical code or call a provider.
History must contain the commits pinned in `cases.json` (a shallow clone may
need to fetch them).

## Evaluate an existing profile

Build the harness you want to evaluate, then choose a named profile from your
normal user configuration. No benchmark config contains credentials.

```sh
cargo build --locked
python3 benchmarks/assessment/run.py run \
  --binary "$PWD/target/debug/habeas" \
  --config luna \
  --case openai-concurrency-gap \
  --case openai-concurrency-fixed \
  --repetitions 3 \
  --output /private/tmp/habeas-benchmark-luna-01
```

`run` sends real model requests and may incur charges. Use a new output directory
for each run. Omit `--case` to run all eight cases. Runs are serial, each assessment
uses a fresh conversation, and policy is explicitly `once` to measure a single
assessor rather than confound the result with confirmation. There is no automatic
whole-assessment retry; the harness still applies its normal request retries.
A recorded billing/quota failure stops the remaining suite. A per-case timeout
(default 600 seconds) is an execution failure, not a model judgment.

For a fair comparison, hold reasoning effort and output limits fixed where
supported, and record any differences. The runner records binary/manifest/tree
hashes, profile name, resolved model/backend in usage, elapsed time, requests,
tool failures, and token counts. It does not export full profile configuration
because profiles can contain credentials; keep a sanitized record of effective
options alongside your results. Pricing and daily free allowances change, so
cost should be calculated from usage with a dated pricing schedule, not a fixed
price embedded in this suite.

## Isolation and scoring

Each case is a temporary Git tree made from tracked files at its pinned commit,
with any explicit reconstruction applied. Prior evidence, audits, this benchmark,
and all commit history are omitted. Expected labels and review notes stay outside
the tree the model can read. Snapshots are never built or executed: the selected
current harness assesses their source. Production evidence and user configuration
are not modified. Usage records are isolated and copied to the result directory.
The user's normal profile lookup and credentials remain in effect.

`results.json` reports label matches, false accepts, false alarms, unexpected
judgments (including FIXME), and execution failures separately. A boolean match
is deliberately named `label_match_pending_rationale_review`: returning false
for an invented reason is not success. Review each rationale against the case's
`expected_finding` and `review_checks`, recording whether it found the intended
gap, correctly classified it, and avoided unsupported additional claims.

Report completion rate over all attempts, false-accept rate over completed gap
cases, and false-alarm rate over completed fixed cases, with denominators. Also
report rationale-reviewed matches, tool failures, elapsed time, and total tokens
including unsuccessful attempts. Do not combine execution failures with wrong
judgments or omit them from cost accounting. Repeat across profiles before
inferring reliability; eight related cases are too few for a general quality claim.

To refine the suite, add pinned cases with source provenance and a concrete
review rubric, retain old versions, and separate new held-out cases from examples
used to tune prompts. Preserve unexpected genuine findings for label review.
Repository traffic sharing was approved for this repository; that is not a
blanket authorization for benchmarks drawn from other repositories.

## First local pilot

`results/local-pilot-2026-11-15.json` records a serial, one-attempt comparison on
`openai-concurrency-gap` and `openai-concurrency-fixed`. Both profiles used 64K
context, a 4K output cap, and a 180-second per-case timeout; Qwen thinking was off.

| Installed model | Gap case | Fixed case |
| --- | --- | --- |
| Ministral 3B | Timed out at 180 seconds after source tool calls | Submitted unsupported FIXME after 119 seconds, alleging default/enforcement logic was absent |
| Qwen3.5 4B | Invalid submission fields after 43 seconds | Invalid determination shape after 35 seconds |

None produced a correct, usable assessment. This does not establish that the
models are unsuitable for narrow repair proposals: the pilot exercises the full
source-exploring assessment protocol. Before expanding the suite, investigate
submission reliability with focused inputs and simpler schemas. The malformed
Qwen payloads were not retained by the harness, so their exact argument errors
cannot be diagnosed beyond its validation messages. Preserve this baseline when
trying prompt/protocol changes. The report includes partial usage, runtime and
cache caveats, and source-reviewed reasons for rejecting Ministral's finding.

## First adaptive pilot

`results/adaptive-pilot-2026-11-15.json` records one serial run of all eight
cases with the harness's `adaptive` model, `once`, low reasoning, and a 16,384
output-token limit. All eight completed without provider failures in 356 seconds.

| Pair | Gap case | Fixed case |
| --- | --- | --- |
| Pagination boundary | Mini incorrectly accepted the runtime bug | Mini correctly accepted the fix |
| Live test implementation root | Mini missed the missing implementation reference | Mini accepted the repaired case |
| OpenAI concurrency | Mini returned false; fresh Sol correctly identified missing OpenAI tests | Mini accepted the repaired coverage |
| Deleted requirement | Mini missed the prose/test mismatch | Mini accepted the repaired example |

Rationale review confirms one intended gap detected out of four, three false
accepts, no false alarms across four fixed cases, and one escalation out of eight.
The pagination miss is especially clear: mini quoted the faulty size condition
but declared it correct. A negative-result escalation rule cannot recover gaps
that mini incorrectly accepts. These results do not establish quality parity with
Sol, nor justify relying on cheap positive judgments without further evaluation.
Sampled audits remain deferred, per the chosen scope.

Usage across all attempts was 920,065 input tokens (721,872 cached) and 14,774
output tokens, including reasoning. The report separates usage by actual model;
it does not infer billing or free-usage allowance. Only one gap needed Sol, so
this is not a general estimate of escalation cost. Live known-FIXME cases and
repeat runs remain to be added; mock tests establish routing behavior, not model
judgment quality.

## Repeating mini's positive judgments

`results/mini-repeat-2026-11-15.json` records two fresh mini assessments of each
of the three missed gaps and their paired fixed snapshots, with unchanged
prompt and options. All twelve completed in 420 seconds, and every judgment
matched the original run: all six gap assessments returned true, as did all
six fixed assessments. Each missed gap therefore received three incorrect
positive judgments across the original run and these repeats.

Requiring two positive mini assessments would still accept all three gaps in
this experiment. Fresh conversations do not eliminate shared blind spots from
the same model and prompt. No extra mini-confirmation stage was added to the
adaptive implementation based on these results. Usage and reviewed rationales
are retained in the report; this selected six-case sample is not a general
estimate of repeatability.

## Gap-seeking wording experiment

`results/gap-seeking-pilot-2026-11-15.json` repeats those six cases twice with
only the initial review wording changed. The exact old/new text and base commit
are retained in `prompt_change`; replace that text in the named file and rebuild
to reproduce the variant. The experiment used a separate executable, and the
production template was restored. No decision-label or routing change was made.

The new opening asks “What gaps can you find, if any” and instructs the model
to seek concrete counterexamples, inspect actual assertions and example setup,
and check reference markers separately. All twelve assessments completed in
433 seconds with the same mini settings.

- Both pagination-gap runs identify the real reserve-band bug. One returns
  `false`; the other incorrectly uses `FIXME` without any acknowledgment.
- Both runs still miss each of the other two gaps.
- Both repaired-pagination runs incorrectly claim note/final-token assertions
  are absent. The shared `all_pages` helper contains those assertions and is
  called by the cited tests. Both also misuse `FIXME`.
- The repaired live-test case is accepted once and challenged once for missing
  a separate post-credential authoring-failure test. The latter interpretation
  needs adjudication; the existing provider-failure test exercises the same
  authoring failure/status path. The raw label mismatch is retained separately
  from rationale review.
- Both repaired deletion-example runs pass.

Thus the wording found one gap category consistently but introduced false
findings; it did not establish a reliable acceptance policy. Simulated two-mini
acceptance still misses two of three gaps and escalates two of three fixed
pairs. No Sol follow-up was run for this variant. These are development cases
used to inform the prompt, not held-out validation.

A next experiment should separate gap detection from acknowledgment using
explicit model-facing outcomes such as `satisfied`, `unacknowledged_gap`, and
`acknowledged_gap`, with validated acknowledgment references. The conventional
meaning of FIXME (“fix this”) may conflict with its current domain meaning
(“an existing acknowledgment covers this gap”); that is a hypothesis, not a
proven cause. The harness could translate explicit outcomes to the existing
stored evidence format. Evaluate that change separately from prompt wording.

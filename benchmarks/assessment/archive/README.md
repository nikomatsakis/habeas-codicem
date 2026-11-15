# Archived assessment history

`2026-11-15-ci.jsonl` contains 199 saved Sol assessments of 103 distinct subjects:
112 true and 87 false. All 87 false results have rationales recovered from verbose
CLI logs. The passing results have usage metadata but no retained rationale here.
These are historical model judgments, not counts of confirmed bugs or remaining
work. Repeated subjects are separate assessment events, not separate unique bugs.

Sources were the four local CI runs:

| Log in `/private/tmp` | Usage state directory in `/private/tmp` | Assessments |
|---|---|---:|
| `ci-sol-refresh.log` | `habeas-ci-refresh-state` | 66 |
| `ci-sol-final.log` | `habeas-ci-refresh-final-state` | 76 |
| `ci-sol-third.log` | `habeas-ci-refresh-third-state` | 56 |
| `ci-query-rule.log` | `habeas-ci-query-rule-state` | 1 |

Usage records were read from `habeas/authoring/usage` beneath each state directory.
Each archived record retains source hashes and paths, and rationale line ranges.
The extracted data remains usable without those temporary source files.

This is a partial archive of the work: earlier experiments, chat-only findings,
and Git evidence history have not been imported. No source revision is inferred
from the current checkout. No suggestion classification has been assigned yet.

See `tools/assessment_archive/README.md` at the repository root for the extractor,
record format, import behavior, and limitations.

## Automation review, 2026-11-15

`2026-11-15-ci.automation-review.json` records a classification of every archived
false response, linked by run ID and rationale hash. Three subagents divided the
87 responses; each response received one independent review, not three-reviewer
consensus. No edits were applied or new authoring assessments run.

Strict automation candidates are additions of named missing references to existing
items, subject to verifying the target, unchanged source hash, and claimed coverage.
New tests, behavior changes, specification decisions, and disputed interpretations
are excluded. These figures measure candidate eligibility, not demonstrated safety.

| Response classification | Count | Share of 87 failures |
|---|---:|---:|
| All findings are automation candidates | 25 | 28.7% |
| Some but not all findings are candidates | 19 | 21.8% |
| No findings are candidates | 43 | 49.4% |

The reviewers identified 148 findings: 58 reference-only candidates, 73 test
changes, 8 behavior changes, 6 specification/prompt changes, and 3 ambiguous
findings. Finding granularity involves judgment; response-level percentages are
the primary result. Repeated assessments count separately, so these figures do
not estimate the number of unique bugs. The 112 passing responses were excluded
because their rationales were not retained.

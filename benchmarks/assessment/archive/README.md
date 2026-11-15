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

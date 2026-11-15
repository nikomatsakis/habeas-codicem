# Assessment history extractor

`extract.py` archives existing Habeas usage JSON and verbose authoring logs into
JSONL, without calling a model or changing evidence. It uses only Python's standard
library. Run it from the repository root:

```sh
python3 tools/assessment_archive/extract.py \
  --usage-dir /path/to/state/habeas/authoring/usage \
  --log /path/to/verbose-authoring.log \
  --output benchmarks/assessment/archive/history.jsonl
```

Repeat `--usage-dir` and `--log` for multiple runs. Directories must contain usage
records, not general configuration files. Both batch and single-subject verbose
CLI formats are supported. Record IDs come from usage records, never from subject
names or guessed timestamps. A mismatch or conflicting record fails extraction.

Each line contains:

- The original `usage` metadata, including model, subject, outcome, times, tool
  counts, tokens, and input size metadata when present.
- A recovered `rationale`, or `null` with an explicit `rationale_status`.
- Source paths and SHA-256 hashes; rationale sources also include 1-based inclusive
  line ranges in the source log. These cover the result header and rationale.
- A rationale hash for identifying identical text across distinct assessments.

Re-importing merges by run ID, preserves previous rationales, and retains distinct
assessments of the same subject. Exact duplicate text is not treated as a duplicate
assessment. The archive is written atomically after all supplied inputs parse.

The normalized records are self-contained: temporary logs can disappear without
losing the extracted text. Source paths are provenance, not runtime dependencies.
The archive retains local paths and model-written prose. It does not read API-key
configuration, fetch conversations, or copy repository source files. It does not
redact the original logs; choose appropriate inputs before sharing an archive.

## Limits

CLI logs normally print failure rationales only. Missing passing rationales are
not filled from current evidence: it may belong to a later assessment. This is
not a full conversation transcript, nor a snapshot of the assessed code. Usage
metadata does not establish the Git revision when files were edited during runs.
A truncated log may contain only a partial rationale; extraction preserves the
available text and makes no completeness claim. Only supply logs from the
expected Habeas CLI format; log parsing is not a general transcript parser.

No inferred suggestion categories, acceptance decisions, or bug counts are stored.
These can be curated separately, keyed by run ID (and by individual finding if a
rationale contains several). Git evidence history and conversation-only findings
are not imported by this first version.

## Tests

```sh
python3 -m unittest discover -s tools/assessment_archive -v
```

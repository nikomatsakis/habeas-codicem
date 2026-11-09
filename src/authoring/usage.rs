use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rig_core::completion::Usage;
use serde::Serialize;
use serde_json::Value;

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

// {impl}`requirements.authoring.usage_storage`
// {impl}`requirements.authoring.usage_requests`
#[derive(Serialize)]
pub(super) struct UsageRecord {
    version: u32,
    run_id: String,
    project_root: PathBuf,
    subject: String,
    pub subject_kind: Option<String>,
    pub backend: Option<String>,
    pub model: Option<String>,
    /// The assessment policy, once a configuration is selected.
    // {impl}`requirements.authoring.policy`
    pub policy: Option<&'static str>,
    start_time_ms: u128,
    end_time_ms: Option<u128>,
    elapsed_ms: u128,
    outcome: String,
    pub determination: Option<String>,
    pub evidence_output: &'static str,
    pub evidence_written: bool,
    inputs: Vec<InputUsage>,
    requests: Vec<RequestUsage>,
    tool_calls: BTreeMap<String, usize>,
    tool_failures: BTreeMap<String, usize>,
    tool_skipped: BTreeMap<String, usize>,
    request_count: usize,
    tool_call_count: usize,
    tokens: TokenCounts,
}

// {impl}`requirements.authoring.usage_inputs`
#[derive(Serialize)]
struct InputUsage {
    origin: String,
    kind: &'static str,
    bytes: usize,
    lines: usize,
    components: usize,
    unique: bool,
    submitted_in_request: Option<usize>,
    sent_in_request: Option<usize>,
}

// {impl}`requirements.authoring.usage_requests`
#[derive(Serialize)]
struct RequestUsage {
    outcome: String,
    elapsed_ms: u128,
    tokens: TokenCounts,
}

// {impl}`requirements.authoring.usage_requests`
#[derive(Default, Serialize)]
struct TokenCounts {
    input: Option<u64>,
    output: Option<u64>,
    cached_input: Option<u64>,
    cache_creation_input: Option<u64>,
    reasoning: Option<u64>,
    complete: bool,
}

// {cite}`requirements.authoring.usage_storage`
pub(super) struct UsageLog {
    pub record: UsageRecord,
    pub path: PathBuf,
    started: Instant,
    seen: BTreeSet<String>,
    active_request: Option<(usize, Instant)>,
}

impl UsageLog {
    // {impl}`requirements.authoring.usage_storage`
    // {impl}`requirements.authoring.usage_requests`
    // {impl}`requirements.configuration.state_location`
    pub(super) fn start(state: &Path, root: &Path, subject: &str) -> io::Result<Self> {
        let start_time_ms = timestamp();
        let run_id = format!(
            "{}-{}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let directory = state.join("authoring/usage");
        let path = directory.join(format!("{run_id}.json"));
        fs::create_dir_all(&directory)
            .and_then(|()| OpenOptions::new().write(true).create_new(true).open(&path))
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("cannot record usage at {}: {error}", path.display()),
                )
            })?;
        let log = Self {
            record: UsageRecord {
                version: 1,
                run_id,
                project_root: root.to_owned(),
                subject: subject.to_owned(),
                subject_kind: None,
                backend: None,
                model: None,
                policy: None,
                start_time_ms,
                end_time_ms: None,
                elapsed_ms: 0,
                outcome: "unfinished".into(),
                determination: None,
                evidence_output: "file",
                evidence_written: false,
                inputs: Vec::new(),
                requests: Vec::new(),
                tool_calls: BTreeMap::new(),
                tool_failures: BTreeMap::new(),
                tool_skipped: BTreeMap::new(),
                request_count: 0,
                tool_call_count: 0,
                tokens: TokenCounts::default(),
            },
            path,
            started: Instant::now(),
            seen: BTreeSet::new(),
            active_request: None,
        };
        log.save()?;
        Ok(log)
    }

    // {impl}`requirements.authoring.usage_inputs`
    pub(super) fn prepared(&mut self, origin: &str, inputs: &[Value]) {
        for input in inputs {
            let kind = if input.get("item").is_some() {
                "outline"
            } else if input.get("requirements").is_some() {
                "requirements"
            } else if input.get("builtin").is_some() {
                // A built-in's fixed text, supplied but not recorded as evidence.
                "builtin"
            } else {
                "definition"
            };
            let text = input["text"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default();
            let bytes = text
                .iter()
                .filter_map(Value::as_str)
                .map(str::len)
                .sum::<usize>()
                + text.len().saturating_sub(1);
            self.record.inputs.push(InputUsage {
                origin: origin.to_owned(),
                kind,
                bytes,
                lines: text.len(),
                components: input
                    .get("matches")
                    .and_then(Value::as_array)
                    .map_or(1, Vec::len),
                unique: self.seen.insert(input.to_string()),
                submitted_in_request: None,
                sent_in_request: None,
            });
        }
    }

    // {impl}`requirements.authoring.usage_inputs`
    // {impl}`requirements.authoring.usage_requests`
    pub(super) fn begin_request(&mut self) -> io::Result<usize> {
        let index = self.record.requests.len();
        for input in &mut self.record.inputs {
            if input.submitted_in_request.is_none() {
                input.submitted_in_request = Some(index);
            }
        }
        self.record.requests.push(RequestUsage {
            outcome: "unfinished".into(),
            elapsed_ms: 0,
            tokens: TokenCounts::default(),
        });
        self.record.request_count = self.record.requests.len();
        self.record.tokens = TokenCounts::sum(&self.record.requests);
        self.record.elapsed_ms = self.started.elapsed().as_millis();
        self.active_request = Some((index, Instant::now()));
        self.save()?;
        Ok(index)
    }

    // {impl}`requirements.authoring.usage_requests`
    // {impl}`requirements.authoring.usage_inputs`
    pub(super) fn end_request(
        &mut self,
        index: usize,
        elapsed: u128,
        usage: Option<&Usage>,
    ) -> io::Result<()> {
        let tokens = usage.map(TokenCounts::from_usage).unwrap_or_default();
        if usage.is_some() {
            for input in &mut self.record.inputs {
                if input.sent_in_request.is_none() && input.submitted_in_request.is_some() {
                    input.sent_in_request = Some(index);
                }
            }
        }
        self.record.requests[index] = RequestUsage {
            outcome: if usage.is_some() {
                "completed"
            } else {
                "failed"
            }
            .into(),
            elapsed_ms: elapsed,
            tokens,
        };
        self.record.tokens = TokenCounts::sum(&self.record.requests);
        self.active_request = None;
        self.record.elapsed_ms = self.started.elapsed().as_millis();
        self.save()
    }

    // {impl}`requirements.authoring.usage_requests`
    // {impl}`requirements.authoring.usage_inputs`
    pub(super) fn tool(&mut self, name: &str, failed: bool) {
        let name = tool_name(name);
        *self.record.tool_calls.entry(name.to_owned()).or_default() += 1;
        self.record.tool_call_count += 1;
        if failed {
            *self
                .record
                .tool_failures
                .entry(name.to_owned())
                .or_default() += 1;
        }
    }

    // {impl}`requirements.authoring.usage_requests`
    // {impl}`requirements.authoring.usage_inputs`
    pub(super) fn skipped_tool(&mut self, name: &str) {
        let name = tool_name(name);
        *self.record.tool_calls.entry(name.to_owned()).or_default() += 1;
        *self.record.tool_skipped.entry(name.to_owned()).or_default() += 1;
        self.record.tool_call_count += 1;
    }

    // {impl}`requirements.authoring.usage_storage`
    // {impl}`requirements.configuration.state_location`
    // {impl}`requirements.authoring.usage_requests`
    pub(super) fn finish(&mut self, outcome: &str) -> io::Result<()> {
        if let Some((index, started)) = self.active_request.take() {
            let request = &mut self.record.requests[index];
            request.elapsed_ms = started.elapsed().as_millis();
            if outcome == "cancelled" {
                request.outcome = "cancelled".into();
            }
        }
        self.record.tokens = TokenCounts::sum(&self.record.requests);
        self.record.outcome = outcome.to_owned();
        self.record.end_time_ms = Some(timestamp());
        self.record.elapsed_ms = self.started.elapsed().as_millis();
        self.save()
    }

    // {impl}`requirements.authoring.usage_storage`
    // {impl}`requirements.configuration.state_location`
    pub(super) fn save(&self) -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&self.path)?;
        serde_json::to_writer_pretty(&mut file, &self.record)?;
        file.write_all(b"\n")
    }
}

impl TokenCounts {
    // {impl}`requirements.authoring.usage_requests`
    fn from_usage(usage: &Usage) -> Self {
        Self {
            input: usage.input_tokens,
            output: usage.output_tokens,
            cached_input: usage.cached_input_tokens,
            cache_creation_input: usage.cache_creation_input_tokens,
            reasoning: usage.reasoning_tokens,
            complete: usage.input_tokens.is_some() && usage.output_tokens.is_some(),
        }
    }

    // {impl}`requirements.authoring.usage_requests`
    fn sum(requests: &[RequestUsage]) -> Self {
        if requests.is_empty() {
            return Self::default();
        }
        // Cache and reasoning counters are subsets, never added to token totals.
        // Keep measured subtotals when another request has no measurement.
        let (input, _) = Self::sum_counter(requests, |tokens| tokens.input);
        let (output, _) = Self::sum_counter(requests, |tokens| tokens.output);
        let (cached_input, cached_complete) =
            Self::sum_counter(requests, |tokens| tokens.cached_input);
        let (cache_creation_input, creation_complete) =
            Self::sum_counter(requests, |tokens| tokens.cache_creation_input);
        let (reasoning, reasoning_complete) =
            Self::sum_counter(requests, |tokens| tokens.reasoning);
        Self {
            input,
            output,
            cached_input,
            cache_creation_input,
            reasoning,
            complete: requests.iter().all(|r| r.tokens.complete)
                && cached_complete
                && creation_complete
                && reasoning_complete,
        }
    }

    // {impl}`requirements.authoring.usage_requests`
    fn sum_counter(
        requests: &[RequestUsage],
        counter: fn(&Self) -> Option<u64>,
    ) -> (Option<u64>, bool) {
        let mut measured = 0;
        let mut total = 0;
        for request in requests {
            if let Some(value) = counter(&request.tokens) {
                measured += 1;
                total += value;
            }
        }
        (
            (measured > 0).then_some(total),
            measured == 0 || measured == requests.len(),
        )
    }
}

// {impl}`requirements.authoring.usage_requests`
// {impl}`requirements.authoring.usage_storage`
// {impl}`requirements.authoring.usage_inputs`
fn tool_name(name: &str) -> &str {
    match name {
        "habeas" | "ripgrep" | "ast_grep" | "github" | "list_files" | "outline" | "references"
        | "submit_judgment" => name,
        _ => "unknown",
    }
}

// {impl}`requirements.authoring.usage_storage`
fn timestamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Duration;

    // {test}`requirements.authoring.usage_storage`
    fn persisted(log: &UsageLog) -> Value {
        serde_json::from_str(&fs::read_to_string(&log.path).unwrap()).unwrap()
    }

    // {test}`requirements.authoring.usage_inputs`
    // {test}`requirements.authoring.usage_storage`
    #[test]
    fn input_metadata_counts_components_and_reads_without_persisting_source() {
        let state = tempfile::tempdir().unwrap();
        let mut log =
            UsageLog::start(state.path(), Path::new("/project"), "requirements.case").unwrap();
        let file = json!({
            "file": "private-source-file.rs", "item": "fn[private_item]", "hash": "0".repeat(64),
            "kind": "fn", "line_start": 1, "line_end": 2,
            "text": ["λ", "DO_NOT_LOG_SOURCE"]
        });
        let requirements = json!({
            "requirements": "private.match.*",
            "matches": ["private.match.a", "private.match.b"],
            "text": ["PRIVATE_REQUIREMENT_TEXT"]
        });
        log.prepared("mandatory", &[file.clone(), requirements]);
        log.prepared("habeas", &[file]);
        log.save().unwrap();
        let prepared = persisted(&log);
        assert_eq!(prepared["inputs"][0]["kind"], "outline");
        assert_eq!(prepared["inputs"][0]["bytes"], "λ\nDO_NOT_LOG_SOURCE".len());
        assert_eq!(prepared["inputs"][0]["lines"], 2);
        assert_eq!(prepared["inputs"][1]["components"], 2);
        assert_eq!(prepared["inputs"][0]["unique"], true);
        assert_eq!(prepared["inputs"][2]["unique"], false);
        assert_eq!(prepared["inputs"][2]["origin"], "habeas");
        assert!(prepared["inputs"][0]["submitted_in_request"].is_null());
        assert!(prepared["inputs"][0]["sent_in_request"].is_null());
        let index = log.begin_request().unwrap();
        let submitted = persisted(&log);
        assert_eq!(submitted["inputs"][0]["submitted_in_request"], index);
        assert!(submitted["inputs"][0]["sent_in_request"].is_null());
        log.end_request(index, 7, Some(&Usage::new())).unwrap();
        log.record.determination = Some("FIXME".into());
        log.record.evidence_written = true;
        log.finish("completed").unwrap();
        let sent = persisted(&log);
        assert_eq!(sent["inputs"][0]["sent_in_request"], index);
        assert_eq!(sent["requests"][0]["outcome"], "completed");
        assert_eq!(sent["outcome"], "completed");
        assert_eq!(sent["determination"], "FIXME");
        assert_eq!(sent["evidence_written"], true);
        assert!(sent["tokens"]["input"].is_null());
        assert!(sent["tokens"]["output"].is_null());
        assert_eq!(sent["tokens"]["complete"], false);
        let serialized = fs::read_to_string(&log.path).unwrap();
        for private in [
            "DO_NOT_LOG_SOURCE",
            "PRIVATE_REQUIREMENT_TEXT",
            "private-source-file.rs",
            "private_item",
            "private.match",
        ] {
            assert!(
                !serialized.contains(private),
                "stored source content: {private}"
            );
        }
    }

    // {test}`requirements.authoring.usage_requests`
    #[test]
    fn token_totals_keep_subsets_separate_and_missing_counters_unknown() {
        let state = tempfile::tempdir().unwrap();
        let mut log =
            UsageLog::start(state.path(), Path::new("/project"), "requirements.case").unwrap();
        let measured = Usage::new()
            .input_tokens(100)
            .output_tokens(20)
            .cached_input_tokens(30)
            .cache_creation_input_tokens(10)
            .reasoning_tokens(5);
        let first = log.begin_request().unwrap();
        log.end_request(first, 3, Some(&measured)).unwrap();
        let second = log.begin_request().unwrap();
        let in_flight = persisted(&log);
        assert_eq!(in_flight["tokens"]["complete"], false);
        assert_eq!(in_flight["tokens"]["input"], 100);
        log.end_request(second, 4, Some(&measured)).unwrap();
        let complete = persisted(&log);
        assert_eq!(complete["tokens"]["input"], 200);
        assert_eq!(complete["tokens"]["output"], 40);
        assert_eq!(complete["tokens"]["cached_input"], 60);
        assert_eq!(complete["tokens"]["cache_creation_input"], 20);
        assert_eq!(complete["tokens"]["reasoning"], 10);
        assert_eq!(complete["tokens"]["complete"], true);
        let third = log.begin_request().unwrap();
        log.end_request(third, 2, None).unwrap();
        log.finish("failed").unwrap();
        let failed = persisted(&log);
        assert_eq!(failed["request_count"], 3);
        assert_eq!(failed["requests"][2]["outcome"], "failed");
        assert_eq!(failed["outcome"], "failed");
        assert_eq!(failed["requests"][0]["tokens"]["input"], 100);
        assert!(failed["requests"][2]["tokens"]["input"].is_null());
        assert_eq!(failed["tokens"]["input"], 200);
        assert_eq!(failed["tokens"]["output"], 40);
        assert_eq!(failed["tokens"]["complete"], false);
    }

    // {test}`requirements.authoring.usage_requests`
    #[test]
    fn partial_failure_retains_available_totals_and_optional_measurements() {
        let state = tempfile::tempdir().unwrap();
        let mut log =
            UsageLog::start(state.path(), Path::new("/project"), "requirements.case").unwrap();
        let first = log.begin_request().unwrap();
        log.end_request(
            first,
            2,
            Some(&Usage::new().input_tokens(10).output_tokens(3)),
        )
        .unwrap();
        let known = persisted(&log);
        assert_eq!(known["tokens"]["complete"], true);
        assert!(known["tokens"]["cached_input"].is_null());
        let failed = log.begin_request().unwrap();
        log.end_request(failed, 1, None).unwrap();
        let partial = persisted(&log);
        assert_eq!(partial["tokens"]["input"], 10);
        assert_eq!(partial["tokens"]["output"], 3);
        assert_eq!(partial["tokens"]["complete"], false);
        assert!(partial["requests"][failed]["tokens"]["input"].is_null());
        assert!(partial["tokens"]["cached_input"].is_null());

        let mut optional =
            UsageLog::start(state.path(), Path::new("/project"), "requirements.case").unwrap();
        let first = optional.begin_request().unwrap();
        optional
            .end_request(
                first,
                1,
                Some(&Usage::new().input_tokens(10).output_tokens(3)),
            )
            .unwrap();
        let second = optional.begin_request().unwrap();
        optional
            .end_request(
                second,
                1,
                Some(
                    &Usage::new()
                        .input_tokens(5)
                        .output_tokens(2)
                        .cached_input_tokens(0),
                ),
            )
            .unwrap();
        let mixed = persisted(&optional);
        assert_eq!(mixed["tokens"]["input"], 15);
        assert_eq!(mixed["tokens"]["output"], 5);
        assert_eq!(mixed["tokens"]["cached_input"], 0);
        assert!(mixed["tokens"]["reasoning"].is_null());
        assert_eq!(mixed["tokens"]["complete"], false);
    }

    // {test}`requirements.authoring.usage_storage`
    // {test}`requirements.authoring.usage_requests`
    #[test]
    fn cancelled_request_persists_elapsed_time_and_incomplete_totals() {
        let state = tempfile::tempdir().unwrap();
        let mut log =
            UsageLog::start(state.path(), Path::new("/project"), "requirements.case").unwrap();
        let initial = persisted(&log);
        assert_eq!(initial["outcome"], "unfinished");
        assert!(initial["end_time_ms"].is_null());
        let first = log.begin_request().unwrap();
        log.end_request(
            first,
            2,
            Some(&Usage::new().input_tokens(2).output_tokens(3)),
        )
        .unwrap();
        let second = log.begin_request().unwrap();
        log.active_request = Some((second, Instant::now() - Duration::from_millis(12)));
        log.finish("cancelled").unwrap();
        let cancelled = persisted(&log);
        assert_eq!(cancelled["outcome"], "cancelled");
        assert!(!cancelled["end_time_ms"].is_null());
        assert_eq!(cancelled["requests"][second]["outcome"], "cancelled");
        assert!(
            cancelled["requests"][second]["elapsed_ms"]
                .as_u64()
                .unwrap()
                >= 12
        );
        assert_eq!(cancelled["tokens"]["complete"], false);
        assert_eq!(cancelled["tokens"]["input"], 2);
        assert_eq!(cancelled["tokens"]["output"], 3);
        assert_eq!(cancelled["requests"][first]["tokens"]["input"], 2);
    }

    // {test}`requirements.authoring.usage_requests`
    #[test]
    fn completed_requests_and_the_run_record_elapsed_time() {
        let state = tempfile::tempdir().unwrap();
        let mut log =
            UsageLog::start(state.path(), Path::new("/project"), "requirements.case").unwrap();
        assert_eq!(persisted(&log)["elapsed_ms"], 0);
        log.started = Instant::now() - Duration::from_millis(50);
        let request = log.begin_request().unwrap();
        // The caller measures each attempt, including its abandoned waits.
        log.end_request(
            request,
            20,
            Some(&Usage::new().input_tokens(1).output_tokens(1)),
        )
        .unwrap();
        let during = persisted(&log);
        assert_eq!(during["requests"][request]["outcome"], "completed");
        let request_elapsed = during["requests"][request]["elapsed_ms"].as_u64().unwrap();
        assert_eq!(request_elapsed, 20, "{during}");
        assert!(during["elapsed_ms"].as_u64().unwrap() >= 50, "{during}");
        log.finish("completed").unwrap();
        let finished = persisted(&log);
        let run_elapsed = finished["elapsed_ms"].as_u64().unwrap();
        assert!(run_elapsed >= 50, "{finished}");
        assert!(run_elapsed >= request_elapsed, "{finished}");
        assert_eq!(
            finished["requests"][request]["elapsed_ms"]
                .as_u64()
                .unwrap(),
            request_elapsed
        );
    }

    // {test}`requirements.authoring.usage_requests`
    // {test}`requirements.authoring.usage_inputs`
    // {test}`requirements.authoring.usage_storage`
    #[test]
    fn tool_outcomes_are_distinct_and_unknown_names_do_not_leak_model_text() {
        let state = tempfile::tempdir().unwrap();
        let mut log =
            UsageLog::start(state.path(), Path::new("/project"), "requirements.case").unwrap();
        log.tool("list_files", false);
        log.tool("outline", false);
        log.tool("habeas", true);
        log.skipped_tool("ripgrep");
        log.tool("ast_grep", false);
        log.tool("github", false);
        log.tool("MODEL_TEXT_SHOULD_NOT_APPEAR", true);
        log.skipped_tool("OTHER_MODEL_TEXT");
        log.finish("completed").unwrap();
        let value = persisted(&log);
        assert_eq!(value["tool_call_count"], 8);
        assert_eq!(value["tool_calls"]["list_files"], 1);
        assert_eq!(value["tool_calls"]["ast_grep"], 1);
        assert_eq!(value["tool_calls"]["github"], 1);
        assert_eq!(value["tool_calls"]["outline"], 1);
        assert_eq!(value["tool_failures"]["habeas"], 1);
        assert_eq!(value["tool_skipped"]["ripgrep"], 1);
        assert_eq!(value["tool_calls"]["unknown"], 2);
        assert_eq!(value["tool_failures"]["unknown"], 1);
        assert_eq!(value["tool_skipped"]["unknown"], 1);
        assert!(value["inputs"].as_array().unwrap().is_empty());
        assert!(value["tokens"]["input"].is_null());
        assert_eq!(value["tokens"]["complete"], false);
        let serialized = fs::read_to_string(&log.path).unwrap();
        assert!(!serialized.contains("MODEL_TEXT"));
    }

    // {test}`requirements.authoring.usage_inputs`
    // {test}`requirements.authoring.usage_requests`
    #[test]
    fn failed_submission_can_be_confirmed_on_a_later_attempt() {
        let state = tempfile::tempdir().unwrap();
        let mut log =
            UsageLog::start(state.path(), Path::new("/project"), "requirements.case").unwrap();
        log.prepared(
            "mandatory",
            &[json!({"file": "source", "text": ["source content"]})],
        );
        let first = log.begin_request().unwrap();
        log.end_request(first, 2, None).unwrap();
        assert!(persisted(&log)["inputs"][0]["sent_in_request"].is_null());
        let retry = log.begin_request().unwrap();
        log.end_request(retry, 3, Some(&Usage::new())).unwrap();
        let value = persisted(&log);
        assert_eq!(value["inputs"][0]["submitted_in_request"], first);
        assert_eq!(value["inputs"][0]["sent_in_request"], retry);
        assert_eq!(value["request_count"], 2);
    }
}

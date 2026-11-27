use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rig_core::completion::Usage;
use serde::Serialize;

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize)]
// {impl}`requirements.authoring.credentials`
pub(super) struct UsageRecord {
    version: u32,
    run_id: String,
    project_root: PathBuf,
    subject: String,
    pub subject_kind: Option<String>,
    pub backend: Option<String>,
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routing_reason: Option<&'static str>,
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
    requests: Vec<RequestUsage>,
    tool_failures: BTreeMap<String, usize>,
    tool_skipped: BTreeMap<String, usize>,
    pub(super) provider_usage: trust_but_verify::graph::UsageStats,
    request_count: usize,
    tokens: TokenCounts,
}

#[derive(Serialize)]
struct RequestUsage {
    outcome: String,
    elapsed_ms: u128,
    tokens: TokenCounts,
    /// A failed request's HTTP status, provider error code, and whether it
    /// was retried; never its message.
    #[serde(skip_serializing_if = "Option::is_none")]
    failure: Option<RequestFailure>,
}

/// What usage records of a failed request.
// {impl}`requirements.authoring.usage_requests.failures`
#[derive(Serialize)]
struct RequestFailure {
    http_status: Option<u16>,
    error_code: Option<String>,
    retried: bool,
}

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
    active_request: Option<(usize, Instant)>,
    /// Called once, when the conversation's first provider request
    /// finishes, whether or not it succeeded.
    first_request_finished: Option<Box<dyn FnOnce() + Send>>,
}

impl UsageLog {
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
                version: 2,
                run_id,
                project_root: root.to_owned(),
                subject: subject.to_owned(),
                subject_kind: None,
                backend: None,
                model: None,
                attempts: Vec::new(),
                routing_reason: None,
                policy: None,
                start_time_ms,
                end_time_ms: None,
                elapsed_ms: 0,
                outcome: "unfinished".into(),
                determination: None,
                evidence_output: "file",
                evidence_written: false,
                requests: Vec::new(),
                tool_failures: BTreeMap::new(),
                tool_skipped: BTreeMap::new(),
                provider_usage: Default::default(),
                request_count: 0,
                tokens: TokenCounts::default(),
            },
            path,
            started: Instant::now(),
            active_request: None,
            first_request_finished: None,
        };
        log.save()?;
        Ok(log)
    }

    // {impl}`requirements.authoring.adaptive`
    pub(super) fn handoff_first_request(&mut self, next: &mut Self) {
        next.first_request_finished = self.first_request_finished.take();
    }

    pub(super) fn begin_request(&mut self) -> io::Result<usize> {
        let index = self.record.requests.len();
        self.record.requests.push(RequestUsage {
            outcome: "unfinished".into(),
            elapsed_ms: 0,
            tokens: TokenCounts::default(),
            failure: None,
        });
        self.record.request_count = self.record.requests.len();
        self.record.tokens = TokenCounts::sum(&self.record.requests);
        self.record.elapsed_ms = self.started.elapsed().as_millis();
        self.active_request = Some((index, Instant::now()));
        self.save()?;
        Ok(index)
    }

    /// Calls `finished` once this conversation's first provider request
    /// has finished, whether or not it succeeded.
    pub(super) fn on_first_request(&mut self, finished: impl FnOnce() + Send + 'static) {
        self.first_request_finished = Some(Box::new(finished));
    }

    /// Records that a failed request is being retried.
    // {impl}`requirements.authoring.usage_requests.failures`
    pub(super) fn retried(&mut self, index: usize) -> io::Result<()> {
        if let Some(failure) = &mut self.record.requests[index].failure {
            failure.retried = true;
        }
        self.save()
    }

    pub(super) fn end_request(
        &mut self,
        index: usize,
        elapsed: u128,
        usage: Option<&Usage>,
        failure: Option<(Option<u16>, Option<String>)>,
    ) -> io::Result<()> {
        let tokens = usage.map(TokenCounts::from_usage).unwrap_or_default();
        self.record.requests[index] = RequestUsage {
            outcome: if usage.is_some() {
                "completed"
            } else {
                "failed"
            }
            .into(),
            elapsed_ms: elapsed,
            tokens,
            failure: failure.map(|(http_status, error_code)| RequestFailure {
                http_status,
                error_code,
                retried: false,
            }),
        };
        self.record.tokens = TokenCounts::sum(&self.record.requests);
        self.active_request = None;
        self.record.elapsed_ms = self.started.elapsed().as_millis();
        if let Some(finished) = self.first_request_finished.take() {
            finished();
        }
        self.save()
    }

    pub(super) fn tool(&mut self, name: &str, failed: bool) {
        let name = tool_name(name);
        if failed {
            *self
                .record
                .tool_failures
                .entry(name.to_owned())
                .or_default() += 1;
        }
    }

    pub(super) fn skipped_tool(&mut self, name: &str) {
        let name = tool_name(name);
        *self.record.tool_skipped.entry(name.to_owned()).or_default() += 1;
    }

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

fn tool_name(name: &str) -> &str {
    match name {
        "habeas" | "ripgrep" | "ast_grep" | "github" | "list_files" | "outline" | "references"
        | "next_page" | "finish_search" | "submit_judgment" | "submit_result" => name,
        _ => "unknown",
    }
}

// {cite}`habeas:util`
fn timestamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::time::Duration;

    // {cite}`habeas:util`
    fn persisted(log: &UsageLog) -> Value {
        serde_json::from_str(&fs::read_to_string(&log.path).unwrap()).unwrap()
    }

    // {test}`requirements.authoring.cache_warmup`
    #[test]
    fn the_first_finished_request_is_reported_once_whatever_its_outcome() {
        let state = tempfile::tempdir().unwrap();
        let mut log =
            UsageLog::start(state.path(), Path::new("/project"), "requirements.case").unwrap();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = calls.clone();
        log.on_first_request(move || {
            counted.fetch_add(1, Ordering::Relaxed);
        });
        let first = log.begin_request().unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        log.end_request(first, 1, None, Some((None, None))).unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        let second = log.begin_request().unwrap();
        log.end_request(second, 1, Some(&Usage::new()), None)
            .unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 1);
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
        log.end_request(first, 3, Some(&measured), None).unwrap();
        let second = log.begin_request().unwrap();
        let in_flight = persisted(&log);
        assert_eq!(in_flight["tokens"]["complete"], false);
        assert_eq!(in_flight["tokens"]["input"], 100);
        log.end_request(second, 4, Some(&measured), None).unwrap();
        let complete = persisted(&log);
        assert_eq!(complete["tokens"]["input"], 200);
        assert_eq!(complete["tokens"]["output"], 40);
        assert_eq!(complete["tokens"]["cached_input"], 60);
        assert_eq!(complete["tokens"]["cache_creation_input"], 20);
        assert_eq!(complete["tokens"]["reasoning"], 10);
        assert_eq!(complete["tokens"]["complete"], true);
        let third = log.begin_request().unwrap();
        log.end_request(third, 2, None, Some((None, None))).unwrap();
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
            None,
        )
        .unwrap();
        let known = persisted(&log);
        assert_eq!(known["tokens"]["complete"], true);
        assert!(known["tokens"]["cached_input"].is_null());
        let failed = log.begin_request().unwrap();
        log.end_request(failed, 1, None, Some((None, None)))
            .unwrap();
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
                None,
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
                None,
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
            None,
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
            None,
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
        assert_eq!(value["tool_failures"]["habeas"], 1);
        assert_eq!(value["tool_skipped"]["ripgrep"], 1);
        assert_eq!(value["tool_failures"]["unknown"], 1);
        assert_eq!(value["tool_skipped"]["unknown"], 1);
        assert!(
            value["provider_usage"]["tools"]
                .as_object()
                .unwrap()
                .is_empty()
        );
        assert!(value["tokens"]["input"].is_null());
        assert_eq!(value["tokens"]["complete"], false);
        let serialized = fs::read_to_string(&log.path).unwrap();
        assert!(!serialized.contains("MODEL_TEXT"));
    }

    // {test}`requirements.authoring.usage_requests`
    // {test}`requirements.authoring.usage_requests.failures`
    #[test]
    fn failed_submission_can_be_confirmed_on_a_later_attempt() {
        let state = tempfile::tempdir().unwrap();
        let mut log =
            UsageLog::start(state.path(), Path::new("/project"), "requirements.case").unwrap();
        let first = log.begin_request().unwrap();
        log.end_request(
            first,
            2,
            None,
            Some((Some(429), Some("ThrottlingException".to_owned()))),
        )
        .unwrap();
        // A failed request records its status, code, and retry, but no
        // message text.
        assert_eq!(
            persisted(&log)["requests"][first]["failure"],
            json!({"http_status": 429, "error_code": "ThrottlingException", "retried": false})
        );
        log.retried(first).unwrap();
        assert_eq!(
            persisted(&log)["requests"][first]["failure"]["retried"],
            true
        );
        let retry = log.begin_request().unwrap();
        log.end_request(retry, 3, Some(&Usage::new()), None)
            .unwrap();
        let value = persisted(&log);
        assert_eq!(value["request_count"], 2);
    }
}

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

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
    pub(super) provider_usage: trust_but_verify::graph::UsageStats,
}

pub(super) struct UsageLog {
    pub record: UsageRecord,
    pub path: PathBuf,
    started: Instant,
    /// Called when the first conversation that contacted a provider finishes.
    first_conversation_finished: Option<Box<dyn FnOnce() + Send>>,
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
                version: 3,
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
                provider_usage: Default::default(),
            },
            path,
            started: Instant::now(),
            first_conversation_finished: None,
        };
        log.save()?;
        Ok(log)
    }

    // {impl}`requirements.authoring.adaptive`
    pub(super) fn handoff_first_conversation(&mut self, next: &mut Self) {
        next.first_conversation_finished = self.first_conversation_finished.take();
    }

    pub(super) fn on_first_conversation(&mut self, finished: impl FnOnce() + Send + 'static) {
        self.first_conversation_finished = Some(Box::new(finished));
    }

    pub(super) fn observe_usage(&mut self, usage: &trust_but_verify::graph::UsageStats) {
        self.record.provider_usage = usage.clone();
        if usage.attempts > 0
            && let Some(finished) = self.first_conversation_finished.take()
        {
            finished();
        }
    }

    // {impl}`requirements.configuration.state_location`
    // {impl}`requirements.authoring.usage_requests`
    pub(super) fn finish(&mut self, outcome: &str) -> io::Result<()> {
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
    // {test}`requirements.authoring.usage_storage`
    #[test]
    fn unfinished_runs_are_visible_before_the_backend_is_selected() {
        let state = tempfile::tempdir().unwrap();
        let log = UsageLog::start(state.path(), state.path(), "requirements.example").unwrap();
        let record: serde_json::Value =
            serde_json::from_slice(&fs::read(log.path).unwrap()).unwrap();
        assert_eq!(record["outcome"], "unfinished");
        assert!(
            record["end_time_ms"].is_null()
                && record["backend"].is_null()
                && record["model"].is_null()
        );
        assert_eq!(record["evidence_written"], false);
    }
}

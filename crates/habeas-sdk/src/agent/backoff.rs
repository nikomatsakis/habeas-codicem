use rig_core::error::ProviderError;
use std::{
    io,
    time::{Duration, Instant, SystemTime},
};

// {cite}`habeas:util`
pub struct ProviderBackoff {
    window: Duration,
    failing: Option<(Instant, u32)>,
}
impl Default for ProviderBackoff {
    // {cite}`habeas:util`
    fn default() -> Self {
        Self {
            window: Duration::from_secs(1),
            failing: None,
        }
    }
}

// {impl}`requirements.authoring.provider_backoff.limit`
pub const RETRY_LIMIT: Duration = Duration::from_secs(60);
// {cite}`habeas:util`
pub enum Retry {
    Wait(Duration),
    Stop(io::Error),
}

// {impl}`requirements.authoring.usage_requests.failures`
pub fn failure_codes(error: &io::Error) -> (Option<u16>, Option<String>) {
    error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<ProviderError>())
        .map_or((None, None), |provider| {
            let report = provider.report();
            (report.http_status, report.code)
        })
}

impl ProviderBackoff {
    // {impl}`requirements.authoring.provider_backoff.limit`
    pub fn retry(&mut self, error: io::Error, now: Instant) -> Retry {
        let Some(delay) = self.next_delay(&error) else {
            return Retry::Stop(error);
        };
        let (first, attempts) = self.failing.get_or_insert((now, 0));
        *attempts += 1;
        if now + delay > *first + RETRY_LIMIT {
            let (status, code) = failure_codes(&error);
            let last = match (status, code) {
                (Some(status), Some(code)) => format!("HTTP {status} {code}"),
                (Some(status), None) => format!("HTTP {status}"),
                (None, Some(code)) => code,
                (None, None) => error.to_string(),
            };
            return Retry::Stop(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "the provider kept failing for a minute after {attempts} attempts; last failure: {last}"
                ),
            ));
        }
        Retry::Wait(delay)
    }

    // {cite}`habeas:util`
    pub fn next_delay(&mut self, error: &io::Error) -> Option<Duration> {
        let provider = error
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<ProviderError>());
        let transient = if error.kind() == io::ErrorKind::TimedOut {
            true
        } else {
            let report = provider?.report();
            matches!(report.http_status, Some(429 | 500 | 502 | 503 | 504))
                || matches!(
                    report.code.as_deref(),
                    Some(
                        "ThrottlingException"
                            | "ServiceUnavailableException"
                            | "InternalServerException"
                            | "ModelNotReadyException"
                    )
                )
        };
        if !transient {
            return None;
        }
        let millis = self.window.as_millis() as u64;
        self.window = (self.window * 2).min(Duration::from_secs(60));
        let jittered = Duration::from_millis(fastrand::u64(millis / 2..=millis));
        let minimum = provider
            .and_then(|provider| retry_after(provider, SystemTime::now()))
            .unwrap_or_default();
        Some(jittered.max(minimum))
    }
}

// {cite}`habeas:util`
pub fn retry_after(error: &ProviderError, now: SystemTime) -> Option<Duration> {
    let header = error
        .provider_response_headers()?
        .get("retry-after")?
        .to_str()
        .ok()?
        .trim();
    if let Ok(seconds) = header.parse() {
        return Some(Duration::from_secs(seconds));
    }
    httpdate::parse_http_date(header)
        .ok()
        .map(|deadline| deadline.duration_since(now).unwrap_or_default())
}

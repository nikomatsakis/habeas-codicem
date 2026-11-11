use std::io;
use std::time::{Duration, Instant, SystemTime};

use rig_core::completion::{CacheRetention, CompletionRequest, CompletionResponse};
use rig_core::error::ProviderError;
use rig_core::message::{AssistantContent, ToolCall, ToolFunction, ToolName};
use rig_core::{DynModel, operation::Completion};
use serde_json::{Map, Value};

use crate::config::{BackendConfig, BedrockConfig};

pub(super) trait CompletionProvider {
    /// Sends one request and returns the model's response.
    // {cite}`habeas:util`
    async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse>;

    /// The output-token limit each request asks for, when one is known.
    // {cite}`habeas:util`
    fn output_limit(&self) -> Option<u64> {
        None
    }
}

pub(super) struct ProviderBackoff {
    window: Duration,
    /// When the first of the current consecutive transient failures came,
    /// and how many attempts have failed since.
    failing: Option<(Instant, u32)>,
}

impl Default for ProviderBackoff {
    fn default() -> Self {
        Self {
            window: Duration::from_secs(1),
            failing: None,
        }
    }
}

/// How long a request may keep failing transiently before retrying stops.
// {impl}`requirements.authoring.provider_backoff.limit`
pub(super) const RETRY_LIMIT: Duration = Duration::from_secs(60);

// {cite}`habeas:util`
/// What to do after a failed request.
pub(super) enum Retry {
    /// Wait this long, then retry.
    Wait(Duration),
    /// Stop: the failure is permanent, or retrying would pass the limit.
    Stop(io::Error),
}

/// A failed request's HTTP status and provider error code, when reported.
// {impl}`requirements.authoring.usage_requests.failures`
pub(super) fn failure_codes(error: &io::Error) -> (Option<u16>, Option<String>) {
    error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<ProviderError>())
        .map_or((None, None), |provider| {
            let report = provider.report();
            (report.http_status, report.code)
        })
}

impl ProviderBackoff {
    /// Decides whether to retry a failed request at `now`: after a
    /// transient failure while the next attempt still starts within
    /// [`RETRY_LIMIT`] of the first consecutive failure.
    // {impl}`requirements.authoring.provider_backoff.limit`
    pub(super) fn retry(&mut self, error: io::Error, now: Instant) -> Retry {
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
            return Retry::Stop(io::Error::other(format!(
                "the provider kept failing for a minute after {attempts} attempts; last failure: {last}"
            )));
        }
        Retry::Wait(delay)
    }

    pub(super) fn next_delay(&mut self, error: &io::Error) -> Option<Duration> {
        // Rig preserves HTTP status and Bedrock's exception code. Do not infer
        // throttling from "tokens" in a message: a too-large input cannot be
        // fixed by resending the same request.
        let provider = error
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<ProviderError>());
        let transient = if error.kind() == io::ErrorKind::TimedOut {
            true
        } else {
            // Either signal is enough: AWS JSON services may report a
            // throttling or unavailability code with another status, such as
            // `ThrottlingException` with HTTP 400.
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
        // Equal jitter keeps parallel assessments from retrying in lockstep,
        // while ensuring each retry waits at least half the current window.
        let jittered = Duration::from_millis(fastrand::u64(millis / 2..=millis));
        let minimum = provider
            .and_then(|provider| retry_after(provider, SystemTime::now()))
            .unwrap_or_default();
        Some(jittered.max(minimum))
    }
}

// {cite}`habeas:util`
/// Wraps Rig's error unchanged, so `ProviderBackoff::next_delay` can read its
/// HTTP status, provider code, and `Retry-After` header.
fn provider_failure(error: ProviderError) -> io::Error {
    io::Error::other(error)
}

fn retry_after(error: &ProviderError, now: SystemTime) -> Option<Duration> {
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

// :::{impl} requirements.authoring.provider
// Holds one backend-erased Rig model, so Bedrock and Ollama share this provider
// type; `bedrock` and `options` are the only backend-specific state.
// :::
pub(super) struct RigProvider {
    model: DynModel<Completion>,
    options: serde_json::Value,
    bedrock: bool,
    /// Whether the model takes prompt-cache checkpoints.
    caches: bool,
}

impl RigProvider {
    // {impl}`requirements.authoring.usage_requests`
    // {impl}`requirements.authoring.credentials`
    // {impl}`requirements.authoring.cache_checkpoints`
    pub(super) async fn connect(config: &BackendConfig) -> io::Result<Self> {
        let model = match config {
            BackendConfig::Bedrock(config) => {
                let sdk = aws_sdk_bedrockruntime::Client::new(&bedrock_sdk_config(config).await);
                rig_bedrock::client::BedrockRuntime::from(sdk)
                    .completion(&config.model)
                    .erase()
            }
            BackendConfig::Ollama(config) => {
                // Rig's reqwest transport sends each request once; it has no
                // hidden retry layer to disable.
                let mut provider = rig_core::providers::ollama::OllamaConfig::new()
                    .with_base_url(config.endpoint());
                if let Some(key) = config.api_key()? {
                    provider = provider.with_api_key(key);
                }
                provider.client().native_completion(&config.model).erase()
            }
        };
        Ok(Self {
            model,
            options: config.options_json()?,
            bedrock: matches!(config, BackendConfig::Bedrock(_)),
            caches: match config {
                BackendConfig::Bedrock(config) => bedrock_caches(&config.model),
                // Ollama has no prompt-cache checkpoints.
                BackendConfig::Ollama(_) => false,
            },
        })
    }
}

// {impl}`requirements.authoring.provider_backoff`
async fn bedrock_sdk_config(config: &BedrockConfig) -> aws_config::SdkConfig {
    bedrock_sdk_loader(
        config,
        aws_config::defaults(aws_config::BehaviorVersion::latest()),
    )
    .load()
    .await
}

/// Applies a Bedrock record to an SDK loader, leaving every omitted value to
/// the loader's standard resolution.
fn bedrock_sdk_loader(
    config: &BedrockConfig,
    loader: aws_config::ConfigLoader,
) -> aws_config::ConfigLoader {
    // Keep each provider attempt visible to the usage ledger. An SDK retry
    // would otherwise be hidden inside one recorded request.
    let mut loader = loader.retry_config(aws_config::retry::RetryConfig::disabled());
    if let Some(profile) = &config.aws_profile {
        loader = loader.profile_name(profile);
    }
    if let Some(region) = &config.region {
        loader = loader.region(aws_config::Region::new(region.clone()));
    }
    if let Some(credentials) = &config.credentials {
        loader = loader.credentials_provider(aws_sdk_bedrockruntime::config::Credentials::new(
            credentials.access_key_id.expose_secret(),
            credentials.secret_access_key.expose_secret(),
            credentials
                .session_token
                .as_ref()
                .map(|value| value.expose_secret().to_owned()),
            None,
            "Habeas configuration",
        ));
    }
    loader
}

impl CompletionProvider for RigProvider {
    // {impl}`requirements.authoring.configuration_storage`
    // {impl}`requirements.authoring.cache_checkpoints`
    async fn complete(&self, mut request: CompletionRequest) -> io::Result<CompletionResponse> {
        let mut options = self
            .options
            .as_object()
            .expect("configuration options are a map")
            .clone();
        if self.bedrock {
            apply_bedrock_options(&mut request, &mut options)?;
        }
        if self.caches {
            place_cache_checkpoints(&mut request);
        }
        request.additional_params = (!options.is_empty()).then_some(Value::Object(options));
        let mut response = self.model.call(request).await.map_err(provider_failure)?;
        if self.bedrock {
            normalize_bedrock_client_calls(&mut response)?;
        }
        Ok(response)
    }

    /// Bedrock's configured `maxTokens` or the default; Ollama's
    /// `num_predict` when it sets a positive limit.
    // {cite}`habeas:util`
    fn output_limit(&self) -> Option<u64> {
        if self.bedrock {
            Some(
                self.options
                    .get("maxTokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(DEFAULT_BEDROCK_MAX_TOKENS),
            )
        } else {
            self.options
                .get("num_predict")
                .and_then(Value::as_u64)
                .filter(|limit| *limit > 0)
        }
    }
}

// {cite}`habeas:util`
fn normalize_bedrock_client_calls(response: &mut CompletionResponse) -> io::Result<()> {
    // Rig 0.44 treats every typed Converse toolUse block as a hosted tool.
    // Newer Claude responses explicitly identify client calls as "tool_use".
    for block in &mut response.choice {
        let AssistantContent::Opaque(opaque) = block else {
            continue;
        };
        let Some(call) = opaque.item.get("toolUse") else {
            continue;
        };
        if call.get("type").and_then(Value::as_str) != Some("tool_use") {
            continue;
        }
        let id = call
            .get("toolUseId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| io::Error::other("Bedrock client tool call has no toolUseId"))?;
        let name = call
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| io::Error::other("Bedrock client tool call has no name"))?;
        let name = ToolName::new(name).map_err(io::Error::other)?;
        let input = call
            .get("input")
            .filter(|input| input.is_object())
            .ok_or_else(|| io::Error::other("Bedrock client tool call input must be an object"))?;
        let recovered = ToolCall::from_wire(id, ToolFunction::new(name, input.clone()));
        // Keeping the native item preserves the original block for replay,
        // including its type, without disturbing signed reasoning or origin.
        *block = AssistantContent::ToolCall(recovered).with_native(opaque.item.clone());
    }
    Ok(())
}

/// The output-token limit a Bedrock request asks for when its
/// configuration sets no `maxTokens`. Habeas never sets a typed thinking
/// budget, so Rig's budget-below-`max_tokens` check is not reached.
const DEFAULT_BEDROCK_MAX_TOKENS: u64 = 16_384;

// {cite}`habeas:util`
fn apply_bedrock_options(
    request: &mut CompletionRequest,
    options: &mut Map<String, Value>,
) -> io::Result<()> {
    if options.contains_key("inferenceConfig") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Bedrock options.inferenceConfig is not a nested wrapper; put temperature, maxTokens, topP, and stopSequences directly in the options table",
        ));
    }
    if let Some(value) = options.remove("temperature") {
        request.temperature = Some(
            value
                .as_f64()
                .ok_or_else(|| option_type("temperature", "a number"))?,
        );
    }
    if let Some(value) = options.remove("maxTokens") {
        let tokens = value
            .as_u64()
            .filter(|value| i32::try_from(*value).is_ok())
            .ok_or_else(|| {
                option_type(
                    "maxTokens",
                    "a nonnegative integer representable by Converse's signed 32-bit field",
                )
            })?;
        request.max_tokens = Some(tokens);
    } else {
        request.max_tokens = Some(DEFAULT_BEDROCK_MAX_TOKENS);
    }
    if let Some(value) = options.remove("topP") {
        request.options.top_p = Some(
            value
                .as_f64()
                .ok_or_else(|| option_type("topP", "a number"))?,
        );
    }
    if let Some(value) = options.remove("stopSequences") {
        request.options.stop = serde_json::from_value(value)
            .map_err(|_| option_type("stopSequences", "an array of strings"))?;
    }
    Ok(())
}

/// Whether a Bedrock model takes Converse cache checkpoints: Rig places
/// them for the Claude and Nova families and refuses them for any other.
fn bedrock_caches(model: &str) -> bool {
    matches!(
        rig_bedrock::completion::Family::of(model),
        rig_bedrock::completion::Family::Claude | rig_bedrock::completion::Family::Nova
    )
}

/// Asks Rig for its cache checkpoints, which Bedrock places after the
/// system prompt and at the end of the last message as separate
/// `cachePoint` blocks, leaving the request's content unchanged. They are
/// placed after reasoning turns too, which Bedrock accepts; the vendored
/// `rig-bedrock` is patched to allow that.
fn place_cache_checkpoints(request: &mut CompletionRequest) {
    request.options.cache = Some(CacheRetention::Short);
}

fn option_type(name: &str, expected: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("Bedrock options.{name} must be {expected}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_bedrockruntime::config::ProvideCredentials;
    use rig_core::completion::{FinishReason, Message, Usage};
    use rig_core::message::{Opaque, Origin, Reasoning, Text};
    use serde_json::json;

    // {cite}`habeas:util`
    fn provider_error(status: Option<u16>, code: Option<&str>) -> ProviderError {
        ProviderError::from_provider_body("Too many tokens")
            .with_provider_status(status.map(|status| status.try_into().unwrap()))
            .with_provider_code(code.map(str::to_owned))
    }

    // {cite}`habeas:util`
    fn with_retry_after(error: ProviderError, value: &str) -> ProviderError {
        let mut headers = error
            .provider_response_headers()
            .cloned()
            .unwrap_or_default();
        headers.insert("retry-after", value.parse().unwrap());
        error.with_response_headers(Some(headers))
    }

    // {test}`requirements.authoring.provider_backoff`
    #[test]
    fn retries_throttling_unavailability_and_timeouts_but_not_permanent_errors() {
        for (status, code) in [
            (Some(429), None),
            (Some(429), Some("ThrottlingException")),
            (None, Some("ThrottlingException")),
            (Some(500), None),
            (Some(502), None),
            (Some(503), Some("ServiceUnavailableException")),
            (Some(504), None),
            (None, Some("ServiceUnavailableException")),
            (None, Some("InternalServerException")),
            (None, Some("ModelNotReadyException")),
        ] {
            assert!(
                ProviderBackoff::default()
                    .next_delay(&io::Error::other(provider_error(status, code)))
                    .is_some()
            );
        }
        for (status, code) in [
            (Some(400), Some("ValidationException")),
            (Some(403), Some("AccessDeniedException")),
            (Some(501), None),
            (None, None),
        ] {
            assert!(
                ProviderBackoff::default()
                    .next_delay(&io::Error::other(provider_error(status, code)))
                    .is_none()
            );
        }
        assert!(
            ProviderBackoff::default()
                .next_delay(&io::Error::other("Too many tokens; HTTP 429"))
                .is_none()
        );
        assert!(
            ProviderBackoff::default()
                .next_delay(&io::Error::new(io::ErrorKind::TimedOut, "no response"))
                .is_some()
        );
    }

    // {test}`requirements.authoring.provider_backoff`
    #[test]
    fn transient_bedrock_codes_retry_whatever_their_http_status() {
        for (status, code) in [
            (Some(400), "ThrottlingException"),
            (Some(400), "ServiceUnavailableException"),
            (Some(501), "InternalServerException"),
            (Some(424), "ModelNotReadyException"),
        ] {
            assert!(
                ProviderBackoff::default()
                    .next_delay(&io::Error::other(provider_error(status, Some(code))))
                    .is_some(),
                "{status:?} {code}"
            );
        }
        // A permanent code does not override a transient status.
        assert!(
            ProviderBackoff::default()
                .next_delay(&io::Error::other(provider_error(
                    Some(503),
                    Some("ValidationException")
                )))
                .is_some()
        );
    }

    // {test}`requirements.authoring.provider_backoff.limit`
    #[test]
    fn retrying_stops_once_a_request_has_failed_for_a_minute() {
        let throttled = || io::Error::other(provider_error(Some(429), Some("ThrottlingException")));
        let start = Instant::now();
        let mut backoff = ProviderBackoff::default();
        let mut now = start;
        let mut attempts = 0;
        let message = loop {
            match backoff.retry(throttled(), now) {
                Retry::Wait(delay) => {
                    attempts += 1;
                    now += delay;
                    // No attempt starts after the minute.
                    assert!(now <= start + RETRY_LIMIT);
                }
                Retry::Stop(error) => break error.to_string(),
            }
        };
        assert!(attempts > 3, "{attempts}");
        assert!(message.contains("kept failing for a minute"), "{message}");
        assert!(
            message.contains(&format!("after {} attempts", attempts + 1)),
            "{message}"
        );
        assert!(
            message.contains("HTTP 429 ThrottlingException"),
            "{message}"
        );
        // A Retry-After past the minute stops at once.
        let late = io::Error::other(with_retry_after(
            provider_error(Some(429), Some("ThrottlingException")),
            "90",
        ));
        assert!(matches!(
            ProviderBackoff::default().retry(late, Instant::now()),
            Retry::Stop(_)
        ));
        // The last failure is named by whatever it reports.
        for (error, last) in [
            (
                io::Error::other(provider_error(Some(503), None)),
                "last failure: HTTP 503",
            ),
            (
                io::Error::other(provider_error(None, Some("ThrottlingException"))),
                "last failure: ThrottlingException",
            ),
            (
                io::Error::new(io::ErrorKind::TimedOut, "no response within ten minutes"),
                "last failure: no response within ten minutes",
            ),
        ] {
            let mut backoff = ProviderBackoff::default();
            let message = match backoff.retry(error, start + RETRY_LIMIT) {
                Retry::Stop(error) => panic!("first failure stops: {error}"),
                Retry::Wait(_) => {
                    let again = match last {
                        _ if last.contains("503") => {
                            io::Error::other(provider_error(Some(503), None))
                        }
                        _ if last.contains("Throttling") => {
                            io::Error::other(provider_error(None, Some("ThrottlingException")))
                        }
                        _ => io::Error::new(
                            io::ErrorKind::TimedOut,
                            "no response within ten minutes",
                        ),
                    };
                    match backoff.retry(again, start + RETRY_LIMIT * 3) {
                        Retry::Stop(error) => error.to_string(),
                        Retry::Wait(_) => panic!("retrying passed the limit"),
                    }
                }
            };
            assert!(message.contains(last), "{message}");
        }
        // A permanent failure stops with its own error.
        let permanent = io::Error::other(provider_error(Some(400), Some("ValidationException")));
        assert!(matches!(
            ProviderBackoff::default().retry(permanent, Instant::now()),
            Retry::Stop(error) if !error.to_string().contains("kept failing")
        ));
    }

    // {test}`requirements.authoring.usage_requests.failures`
    #[test]
    fn failure_codes_are_the_status_and_code_a_provider_reports() {
        assert_eq!(
            failure_codes(&io::Error::other(provider_error(
                Some(429),
                Some("ThrottlingException")
            ))),
            (Some(429), Some("ThrottlingException".to_owned()))
        );
        assert_eq!(failure_codes(&io::Error::other("plain")), (None, None));
        assert_eq!(
            failure_codes(&io::Error::new(io::ErrorKind::TimedOut, "no response")),
            (None, None)
        );
    }

    // {test}`requirements.authoring.provider_backoff`
    #[test]
    fn jittered_backoff_grows_to_a_ceiling() {
        let error = io::Error::other(provider_error(Some(429), Some("ThrottlingException")));
        let mut backoff = ProviderBackoff::default();
        for attempt in 0..100 {
            let window = Duration::from_secs(1_u64 << attempt.min(6)).min(Duration::from_secs(60));
            let delay = backoff.next_delay(&error).unwrap();
            assert!((window / 2..=window).contains(&delay));
        }
    }

    // {test}`requirements.authoring.provider_backoff`
    #[test]
    fn retry_after_accepts_seconds_and_dates_without_shortening_provider_delay() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let future = httpdate::fmt_http_date(now + Duration::from_secs(90));
        let past = httpdate::fmt_http_date(now - Duration::from_secs(90));
        for (header, expected) in [
            ("90", Some(Duration::from_secs(90))),
            (future.as_str(), Some(Duration::from_secs(90))),
            (past.as_str(), Some(Duration::ZERO)),
            ("invalid", None),
        ] {
            let error = with_retry_after(provider_error(Some(429), None), header);
            assert_eq!(retry_after(&error, now), expected);
        }
        let error = io::Error::other(with_retry_after(provider_error(Some(429), None), "90"));
        assert_eq!(
            ProviderBackoff::default().next_delay(&error),
            Some(Duration::from_secs(90))
        );
    }

    // {test}`requirements.authoring.provider_backoff`
    // {test}`requirements.authoring.usage_requests`
    #[tokio::test]
    async fn bedrock_sdk_retries_are_disabled() {
        let config: BedrockConfig =
            toml::from_str("model = 'test'\nregion = 'us-west-2'\n").unwrap();
        let sdk = bedrock_sdk_config(&config).await;
        let retry = sdk.retry_config().expect("retry configuration is set");
        assert_eq!(retry.max_attempts(), 1, "{retry:?}");
    }

    // {test}`requirements.authoring.provider_backoff`
    #[test]
    fn rig_failures_keep_their_provider_report_for_backoff() {
        let mut backoff = ProviderBackoff::default();
        let error = provider_failure(provider_error(Some(429), None));
        assert!(backoff.next_delay(&error).is_some());
        let error = provider_failure(provider_error(None, Some("ServiceUnavailableException")));
        assert!(backoff.next_delay(&error).is_some());
        let error = provider_failure(provider_error(Some(400), Some("ValidationException")));
        assert_eq!(backoff.next_delay(&error), None);
    }

    // {test}`requirements.authoring.credentials`
    // {test}`requirements.authoring.rig_configuration_mapping`
    // {test}`requirements.authoring.configuration_precedence`
    #[tokio::test]
    async fn explicit_bedrock_credentials_reach_the_sdk_without_the_default_chain() {
        let config: BedrockConfig = toml::from_str("model = 'test'\nregion = 'us-west-2'\n[credentials]\naccess_key_id = 'TEST_ACCESS'\nsecret_access_key = 'test-secret'\nsession_token = 'test-session'\n").unwrap();
        let sdk = bedrock_sdk_config(&config).await;
        let credentials = sdk
            .credentials_provider()
            .unwrap()
            .provide_credentials()
            .await
            .unwrap();
        assert_eq!(credentials.access_key_id(), "TEST_ACCESS");
        assert_eq!(credentials.secret_access_key(), "test-secret");
        assert_eq!(credentials.session_token(), Some("test-session"));
        assert_eq!(sdk.region().unwrap().as_ref(), "us-west-2");
    }

    // {test}`requirements.authoring.configuration_precedence`
    #[tokio::test]
    #[allow(deprecated)] // The non-deprecated profile-file names live in `aws-runtime`.
    async fn omitted_bedrock_profile_and_region_use_standard_sdk_resolution() {
        use aws_config::profile::profile_file::{ProfileFileKind, ProfileFiles};
        let dir = tempfile::tempdir().unwrap();
        let config_file = dir.path().join("config");
        let credentials_file = dir.path().join("credentials");
        std::fs::write(
            &config_file,
            "[default]\nregion = eu-west-1\n[profile other]\nregion = ap-south-1\n",
        )
        .unwrap();
        std::fs::write(
            &credentials_file,
            "[default]\naws_access_key_id = DEFAULT_KEY\naws_secret_access_key = default\n\
             [other]\naws_access_key_id = OTHER_KEY\naws_secret_access_key = other\n",
        )
        .unwrap();
        let files = ProfileFiles::builder()
            .with_file(ProfileFileKind::Config, &config_file)
            .with_file(ProfileFileKind::Credentials, &credentials_file)
            .build();
        let standard = || {
            aws_config::defaults(aws_config::BehaviorVersion::latest()).profile_files(files.clone())
        };
        let habeas = |text: &str| {
            let config: BedrockConfig = toml::from_str(text).unwrap();
            bedrock_sdk_loader(&config, standard()).load()
        };
        async fn resolved(sdk: aws_config::SdkConfig) -> (Option<String>, String) {
            let key = sdk
                .credentials_provider()
                .unwrap()
                .provide_credentials()
                .await
                .unwrap()
                .access_key_id()
                .to_owned();
            (sdk.region().map(|region| region.to_string()), key)
        }

        // Omitted values resolve exactly as the SDK would with no override.
        let omitted = resolved(habeas("model = 'test'").await).await;
        assert_eq!(omitted, resolved(standard().load().await).await);
        // An explicit profile is the one the SDK resolves from.
        let profile = resolved(habeas("model = 'test'\naws_profile = 'other'").await).await;
        assert_eq!(
            profile,
            resolved(standard().profile_name("other").load().await).await
        );
        // An explicit region overrides whatever the profile or environment says.
        let region =
            resolved(habeas("model = 'test'\naws_profile = 'other'\nregion = 'us-east-2'").await)
                .await;
        assert_eq!(region.0.as_deref(), Some("us-east-2"));
        assert_eq!(region.1, profile.1);
    }

    // {test}`requirements.authoring.configuration_storage`
    // {test}`requirements.authoring.rig_configuration_mapping`
    // {test}`requirements.authoring.output_limit`
    #[test]
    fn bedrock_native_option_types_are_checked_and_only_max_tokens_is_defaulted() {
        let mut request = CompletionRequest::new("prompt");
        let mut options = Map::from_iter([("top_k".into(), json!(12))]);
        apply_bedrock_options(&mut request, &mut options).unwrap();
        assert!(request.temperature.is_none());
        assert_eq!(request.max_tokens, Some(16_384));
        assert!(request.options.is_default());
        assert_eq!(options["top_k"], 12);
        for value in [
            json!({"temperature":"hot"}),
            json!({"maxTokens":-1}),
            json!({"maxTokens":2147483648_u64}),
            json!({"topP":true}),
            json!({"stopSequences":"stop"}),
            json!({"inferenceConfig":{}}),
        ] {
            let mut options = value.as_object().unwrap().clone();
            assert!(
                apply_bedrock_options(&mut CompletionRequest::new("prompt"), &mut options).is_err()
            );
        }
    }

    // {test}`requirements.authoring.cache_checkpoints`
    #[test]
    fn cache_checkpoints_are_requested_for_caching_models() {
        assert!(bedrock_caches(
            "us.anthropic.claude-sonnet-4-5-20250929-v1:0"
        ));
        assert!(bedrock_caches(
            "arn:aws:bedrock:us-east-1:123:inference-profile/us.anthropic.claude-opus-4-1-20250805-v1:0"
        ));
        assert!(bedrock_caches("us.amazon.nova-pro-v1:0"));
        assert!(!bedrock_caches("meta.llama3-70b-instruct-v1:0"));
        assert!(!bedrock_caches("fixture-bedrock-model"));

        let mut request = CompletionRequest::new("subject").preamble("system");
        place_cache_checkpoints(&mut request);
        assert_eq!(request.options.cache, Some(CacheRetention::Short));
        // Only the cache option changes.
        let mut expected = CompletionRequest::new("subject").preamble("system");
        expected.options.cache = Some(CacheRetention::Short);
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );

        // Checkpoints are requested after a reasoning turn too.
        let mut request = CompletionRequest::new("subject").preamble("system");
        request.chat_history.push(Message::Assistant(
            rig_core::message::AssistantMessage::new(vec![AssistantContent::Reasoning(
                Reasoning::new("Think."),
            )]),
        ));
        place_cache_checkpoints(&mut request);
        assert_eq!(request.options.cache, Some(CacheRetention::Short));
    }

    // {test}`requirements.authoring.provider`
    // {test}`requirements.authoring.rig_configuration_mapping`
    #[test]
    fn typed_client_calls_preserve_order_and_native_history() {
        let typed = json!({"toolUse":{
            "type":"tool_use", "toolUseId":"client-1", "name":"habeas",
            "input":{"file":"src/lib.rs","line_start":1}
        }});
        let reasoning = AssistantContent::Reasoning(Reasoning::new("Inspect the source."))
            .with_native(json!({"reasoningContent":{"reasoningText":{
                "text":"Inspect the source.","signature":"original-signature"
            }}}));
        let content = vec![
            reasoning,
            AssistantContent::Opaque(Opaque {
                item: typed.clone(),
                replay: true,
            }),
            AssistantContent::Text(Text::new("Then submit the judgment.")),
            AssistantContent::Opaque(Opaque {
                item: json!({"toolUse":{"type":"server_tool_use","toolUseId":"server-1","name":"server","input":{}}}),
                replay: true,
            }),
            AssistantContent::Opaque(Opaque {
                item: json!({"toolUse":{"type":"future_tool_use","toolUseId":"future-1","name":"future","input":{}}}),
                replay: true,
            }),
            AssistantContent::ToolCall(ToolCall::from_wire(
                "client-2",
                ToolFunction::new(ToolName::new("list_files").unwrap(), json!({})),
            )),
        ];
        let origin = Origin::new(
            "bedrock.converse",
            "aws_bedrock",
            "us.anthropic.claude-sonnet-5-5",
        );
        let raw = json!({"original":"provider response"});
        let mut response = CompletionResponse::new(
            content.clone(),
            Usage::default(),
            origin.clone(),
            raw.clone(),
        )
        .with_finish_reason(FinishReason::ToolCalls);
        response.provider_request_id = Some("request-1".into());

        normalize_bedrock_client_calls(&mut response).unwrap();
        assert_eq!(response.choice.len(), content.len());
        for index in [0, 2, 3, 4, 5] {
            assert_eq!(response.choice[index], content[index]);
        }
        let calls = response.tool_calls().collect::<Vec<_>>();
        assert_eq!(calls.len(), 2);
        assert_eq!(
            calls[0].id,
            rig_core::message::CallId::from_wire("client-1")
        );
        assert_eq!(calls[0].function.name.as_str(), "habeas");
        assert_eq!(
            calls[0].function.arguments,
            typed["toolUse"]["input"].as_object().unwrap().clone()
        );
        assert_eq!(response.choice[1].native_item(), Some(&typed));
        assert_eq!(
            calls[1].id,
            rig_core::message::CallId::from_wire("client-2")
        );
        assert_eq!(response.raw, raw);
        assert_eq!(response.origin, origin);
        assert_eq!(response.provider_request_id.as_deref(), Some("request-1"));
        assert_eq!(response.finish_reason(), Some(FinishReason::ToolCalls));
        let normalized = response.clone();
        normalize_bedrock_client_calls(&mut response).unwrap();
        assert_eq!(response, normalized);
    }

    // {test}`requirements.authoring.provider`
    // {test}`requirements.authoring.rig_configuration_mapping`
    #[test]
    fn malformed_typed_client_calls_are_reported() {
        for call in [
            json!({"type":"tool_use","name":"habeas","input":{}}),
            json!({"type":"tool_use","toolUseId":"id","input":{}}),
            json!({"type":"tool_use","toolUseId":"id","name":"habeas","input":[]}),
        ] {
            let mut response = CompletionResponse::new(
                vec![AssistantContent::Opaque(Opaque {
                    item: json!({"toolUse":call}),
                    replay: true,
                })],
                Usage::default(),
                Origin::new("bedrock.converse", "aws_bedrock", "model"),
                Value::Null,
            );
            assert!(normalize_bedrock_client_calls(&mut response).is_err());
        }
    }
}

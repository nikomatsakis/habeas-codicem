//! Model-assisted evidence authoring, invoked explicitly and never by validation.

pub(crate) mod access;
mod batch;
mod inputs;
mod progress;
mod providers;
mod tools;
mod usage;

pub use batch::{BatchOutcome, author_all, author_all_with_options};

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use rig_core::completion::{CompletionRequest, ToolDefinition};
use rig_core::message::{Message, ToolResultContent, turn_failure};
use serde_json::{Value, json};
use tokio::sync::watch;

use crate::config::{self, BackendKind, ResolvedAuthoringConfig};
use crate::judgment::Judgment;
use inputs::{AssessmentKind, AssessmentSources};
use providers::{CompletionProvider, RigProvider};
use usage::UsageLog;

/// How long a completion request may go without a response.
// {impl}`requirements.authoring.provider_backoff`
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

// {cite}`requirements.authoring.invocation`
pub struct AuthoringOutcome {
    pub subject: String,
    pub backend: String,
    pub model: String,
    pub evidence_path: Option<PathBuf>,
    pub usage_path: PathBuf,
    pub determination: Judgment,
    pub rationale: String,
}

// {cite}`requirements.authoring.reporting`
#[derive(Clone, Copy, Debug, Default)]
pub struct AuthoringOptions {
    pub verbose: bool,
}

// {cite}`requirements.authoring.invocation`
#[derive(Clone, Debug)]
pub(crate) enum AuthoringSubject {
    Definition(String),
}

impl AuthoringSubject {
    // {impl}`requirements.authoring.batch`
    // {impl}`requirements.authoring.usage_storage`
    pub(crate) fn label(&self) -> String {
        let Self::Definition(name) = self;
        name.clone()
    }
}

// {cite}`requirements.authoring.persistence`
pub(crate) struct PendingAssessment {
    pub(crate) subject: AuthoringSubject,
    pub(crate) record: Value,
    outcome: AuthoringOutcome,
    usage: UsageLog,
}

impl PendingAssessment {
    // {impl}`requirements.authoring.batch`
    // {impl}`requirements.authoring.persistence`
    // {impl}`requirements.authoring.usage_storage`
    // {impl}`requirements.configuration.state_location`
    pub(crate) fn persisted(mut self, path: PathBuf) -> io::Result<AuthoringOutcome> {
        self.usage.record.evidence_written = true;
        self.outcome.evidence_path = Some(path);
        self.usage.finish("completed").map_err(|error| {
            io::Error::other(format!(
                "cannot record usage at {} (evidence written: true): {error}",
                self.usage.path.display()
            ))
        })?;
        Ok(self.outcome)
    }

    // {impl}`requirements.authoring.batch`
    // {impl}`requirements.authoring.usage_storage`
    // {impl}`requirements.configuration.state_location`
    // {impl}`requirements.authoring.read_denial`
    pub(crate) fn failed(mut self, error: io::Error) -> io::Error {
        let outcome = if error.kind() == io::ErrorKind::Interrupted {
            "cancelled"
        } else {
            "failed"
        };
        match self.usage.finish(outcome) {
            Ok(()) => io::Error::new(
                error.kind(),
                format!("{error}; usage: {}", self.usage.path.display()),
            ),
            Err(usage_error) => io::Error::new(
                error.kind(),
                format!(
                    "{error}; cannot record failed usage at {}: {usage_error}",
                    self.usage.path.display()
                ),
            ),
        }
    }
}

// {cite}`requirements.authoring.invocation`
#[derive(Clone, Copy)]
pub enum EvidenceOutput {
    File,
    Stdout,
}

// {impl}`requirements.authoring.invocation`
pub async fn author_evidence(
    root: &Path,
    subject: &str,
    selection: Option<&str>,
) -> io::Result<AuthoringOutcome> {
    author_evidence_with_output(root, subject, selection, EvidenceOutput::File).await
}

// {impl}`requirements.authoring.invocation`
// {impl}`requirements.authoring.usage_storage`
// {impl}`requirements.configuration.state_location`
// {impl}`requirements.authoring.read_denial`
pub async fn author_evidence_with_output(
    root: &Path,
    subject: &str,
    selection: Option<&str>,
    output: EvidenceOutput,
) -> io::Result<AuthoringOutcome> {
    let root = fs::canonicalize(root)?;
    let state = usage_state_root(&root)?;
    let mut usage = UsageLog::start(&state, &root, subject)?;
    usage.record.evidence_output = match output {
        EvidenceOutput::File => "file",
        EvidenceOutput::Stdout => "stdout",
    };
    usage.save()?;
    let result = tokio::select! {
        result = configured_assessment(&root, subject, selection, output, &mut usage) => result,
        signal = tokio::signal::ctrl_c() => {
            match signal {
                Ok(()) => Err(io::Error::new(io::ErrorKind::Interrupted, "evidence authoring cancelled")),
                Err(error) => Err(error),
            }
        }
    };
    let outcome = match &result {
        Ok(_) => "completed",
        Err(error) if error.kind() == io::ErrorKind::Interrupted => "cancelled",
        Err(_) => "failed",
    };
    usage.finish(outcome).map_err(|error| {
        let cause = result
            .as_ref()
            .err()
            .map_or(String::new(), |cause| format!("{cause}; "));
        io::Error::new(
            result.as_ref().err().map_or(error.kind(), io::Error::kind),
            format!(
                "{cause}cannot record usage at {} (evidence written: {}): {error}",
                usage.path.display(),
                usage.record.evidence_written
            ),
        )
    })?;
    result.map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("{error}; usage: {}", usage.path.display()),
        )
    })
}

/// Resolves the usage state root, refusing a location inside the project
/// before any state directory is created.
// {impl}`requirements.authoring.usage_storage`
// {impl}`requirements.configuration.state_location`
fn usage_state_root(root: &Path) -> io::Result<PathBuf> {
    let state = config::state_root()?;
    if resolved_location(&state)?.starts_with(root) {
        return Err(io::Error::other(format!(
            "authoring usage state must be outside the project repository: {}",
            state.display()
        )));
    }
    fs::create_dir_all(&state).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "cannot create authoring state directory {}: {error}",
                state.display()
            ),
        )
    })?;
    Ok(state)
}

/// Resolves symlinks in the nearest existing ancestor of `path`, keeping the
/// components that do not exist yet.
// {impl}`requirements.authoring.usage_storage`
// {impl}`requirements.configuration.state_location`
fn resolved_location(path: &Path) -> io::Result<PathBuf> {
    let path = std::path::absolute(path)?;
    let existing = path
        .ancestors()
        .find(|ancestor| ancestor.exists())
        .expect("an absolute path has an existing root");
    let missing = path
        .strip_prefix(existing)
        .expect("ancestor is a prefix of its path");
    Ok(fs::canonicalize(existing)?.join(missing))
}

// {impl}`requirements.authoring.batch`
// {impl}`requirements.authoring.usage_storage`
// {impl}`requirements.configuration.state_location`
// {impl}`requirements.authoring.read_denial`
// {impl}`requirements.authoring.concurrency`
pub(crate) async fn prepare_assessment(
    root: &Path,
    subject: &AuthoringSubject,
    config: &ResolvedAuthoringConfig,
    mut cancellation: watch::Receiver<Option<&'static str>>,
) -> io::Result<PendingAssessment> {
    let root = fs::canonicalize(root)?;
    let state = usage_state_root(&root)?;
    let mut usage = UsageLog::start(&state, &root, &subject.label())?;
    let result = tokio::select! {
        biased;
        _ = cancellation.changed() => {
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                cancellation.borrow().unwrap_or("evidence authoring cancelled"),
            ))
        }
        result = configured_record(&root, subject, config, &mut usage) => result,
        signal = tokio::signal::ctrl_c() => {
            match signal {
                Ok(()) => Err(io::Error::new(io::ErrorKind::Interrupted, "evidence authoring cancelled")),
                Err(error) => Err(error),
            }
        }
    };
    match result {
        Ok((determination, record)) => {
            let outcome = AuthoringOutcome {
                subject: subject.label(),
                backend: usage.record.backend.clone().expect("configured backend"),
                model: usage.record.model.clone().expect("configured model"),
                evidence_path: None,
                usage_path: usage.path.clone(),
                determination,
                rationale: record["rationale"]
                    .as_str()
                    .expect("validated judgment rationale")
                    .to_owned(),
            };
            Ok(PendingAssessment {
                subject: subject.clone(),
                record,
                outcome,
                usage,
            })
        }
        Err(error) => {
            let outcome = if error.kind() == io::ErrorKind::Interrupted {
                "cancelled"
            } else {
                "failed"
            };
            usage.finish(outcome).map_err(|usage_error| {
                io::Error::new(
                    error.kind(),
                    format!(
                        "{error}; cannot record usage at {}: {usage_error}",
                        usage.path.display()
                    ),
                )
            })?;
            Err(io::Error::new(
                error.kind(),
                format!("{error}; usage: {}", usage.path.display()),
            ))
        }
    }
}

// {impl}`requirements.authoring.invocation`
// {impl}`requirements.authoring.configuration_precedence`
// {impl}`requirements.authoring.rig_configuration_mapping`
// {impl}`requirements.authoring.usage_storage`
// {impl}`requirements.authoring.persistence`
// {impl}`requirements.configuration.loading`
async fn configured_assessment(
    root: &Path,
    subject: &str,
    selection: Option<&str>,
    output: EvidenceOutput,
    usage: &mut UsageLog,
) -> io::Result<AuthoringOutcome> {
    let config = config::resolve_authoring(root, selection)?;
    let (determination, evidence) = configured_record(
        root,
        &AuthoringSubject::Definition(subject.to_owned()),
        &config,
        usage,
    )
    .await?;
    let json = format!("{}\n", serde_json::to_string_pretty(&evidence)?);
    let evidence_path = match output {
        EvidenceOutput::File => {
            let path = root.join("evidence").join(format!("{subject}.json"));
            fs::create_dir_all(path.parent().expect("evidence parent"))?;
            fs::write(&path, json)?;
            Some(path)
        }
        EvidenceOutput::Stdout => {
            let mut stdout = io::stdout().lock();
            stdout.write_all(json.as_bytes())?;
            stdout.flush()?;
            None
        }
    };
    usage.record.evidence_written = true;
    usage.save()?;
    Ok(AuthoringOutcome {
        subject: subject.into(),
        backend: usage.record.backend.clone().expect("configured backend"),
        model: config.model_ref().model,
        evidence_path,
        usage_path: usage.path.clone(),
        determination,
        rationale: evidence["rationale"]
            .as_str()
            .expect("validated judgment rationale")
            .to_owned(),
    })
}

// {impl}`requirements.authoring.invocation`
// {impl}`requirements.authoring.configuration_precedence`
// {impl}`requirements.authoring.rig_configuration_mapping`
// {impl}`requirements.authoring.usage_storage`
// {impl}`requirements.authoring.mandatory_inputs`
// {impl}`requirements.authoring.usage_inputs`
// {impl}`requirements.authoring.provider`
// {impl}`requirements.authoring.read_denial`
async fn configured_record(
    root: &Path,
    subject: &AuthoringSubject,
    config: &ResolvedAuthoringConfig,
    usage: &mut UsageLog,
) -> io::Result<(Judgment, Value)> {
    let model = config.model_ref();
    let backend = match model.backend {
        BackendKind::Bedrock => "bedrock",
        BackendKind::Ollama => "ollama",
    };
    usage.record.backend = Some(backend.into());
    usage.record.model = Some(model.model.clone());
    let AuthoringSubject::Definition(name) = subject;
    let mut sources = AssessmentSources::prepare(root, name)?;
    usage.record.subject_kind = Some(sources.subject_kind.noun().into());
    usage.prepared("mandatory", sources.mandatory_inputs());
    usage.save()?;
    let provider = RigProvider::connect(&config.backend)
        .await
        .map_err(|error| {
            io::Error::other(format!(
                "{}: authoring.configs.{}: {error}",
                config.config_path.display(),
                config.name
            ))
        })?;
    assess(&provider, &mut sources, usage).await
}

// {impl}`requirements.authoring.initial_prompt`
// {impl}`requirements.ci.review_criteria`
// {impl}`requirements.ci.outcomes`
// {impl}`requirements.examples.evidence`
// {impl}`requirements.authoring.mandatory_inputs`
fn initial_prompt(sources: &AssessmentSources) -> io::Result<String> {
    // Keep the approved template as the sole source of these instructions.
    let chapter = include_str!("../../md/requirements/authoring.md");
    let opening = "Bring your expert judgment to the task of assessing whether a project's sources satisfy a requirement or example.";
    let start = chapter.find(opening).expect("approved prompt template");
    let template = chapter[start..]
        .split("\n```")
        .next()
        .expect("prompt closing fence");
    Ok(template
        .replace("{{subject_kind}}", sources.subject_kind.noun())
        .replace("{{subject_name}}", &sources.subject_name)
        .replace(
            "{{mandatory_inputs}}",
            &serde_json::to_string_pretty(sources.mandatory_inputs())?,
        ))
}

// {impl}`requirements.authoring.submission`
fn submission(arguments: &Value) -> io::Result<(Judgment, String)> {
    let object = arguments
        .as_object()
        .ok_or_else(|| io::Error::other("submit_judgment expects an object"))?;
    if object.len() != 2
        || !object.contains_key("determination")
        || !object.contains_key("rationale")
    {
        return Err(io::Error::other(
            "submit_judgment requires exactly determination and rationale",
        ));
    }
    let judgment = Judgment::from_value(&object["determination"]).map_err(io::Error::other)?;
    let rationale = object["rationale"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| io::Error::other("submit_judgment rationale must be nonempty Markdown"))?;
    Ok((judgment, rationale.to_owned()))
}

// {impl}`requirements.authoring.submission`
// {impl}`requirements.authoring.initial_prompt`
// {impl}`requirements.ci.outcomes`
fn tool_definitions() -> io::Result<Vec<ToolDefinition>> {
    let mut definitions = tools::definitions();
    let determination = json!({"oneOf":[{"type":"boolean"},{"type":"object","additionalProperties":false,"required":["FIXME"],"properties":{"FIXME":{"type":"array","minItems":1,"items":{"type":"string","minLength":1}}}}]});
    definitions.push(json!({"name": "submit_judgment", "description": "Submit your final judgment exactly once. No other message supplies the result.",
        "parameters": {"type":"object", "additionalProperties": false, "required":["determination","rationale"], "properties": {
            "determination": determination,
            "rationale":{"type":"string","minLength":1}}}}));
    definitions
        .into_iter()
        .map(|value| serde_json::from_value(value).map_err(io::Error::other))
        .collect()
}

// {impl}`requirements.authoring.submission`
// {impl}`requirements.authoring.initial_prompt`
// {impl}`requirements.authoring.delivered_inputs`
// {impl}`requirements.authoring.persistence`
// {impl}`requirements.authoring.usage_inputs`
// {impl}`requirements.authoring.usage_requests`
// {impl}`requirements.authoring.usage_storage`
// {impl}`requirements.authoring.mandatory_inputs`
// {impl}`requirements.authoring.read_denial`
// {impl}`requirements.authoring.provider_backoff`
// {impl}`requirements.authoring.submission_count`
// {impl}`requirements.authoring.source_tools`
// {impl}`requirements.authoring.file_discovery`
// {impl}`requirements.authoring.text_search`
// {impl}`requirements.ci.authoring_inputs`
// {impl}`requirements.authoring.rig_configuration_mapping`
// {impl}`requirements.definitions.authoring_inputs`
// {impl}`requirements.ci.reference_set`
async fn assess(
    provider: &impl CompletionProvider,
    sources: &mut AssessmentSources,
    usage: &mut UsageLog,
) -> io::Result<(Judgment, Value)> {
    let mut request = CompletionRequest::new(initial_prompt(sources)?);
    request.tools = tool_definitions()?;
    let mut pending = sources.mandatory_inputs().to_vec();
    let mut backoff = providers::ProviderBackoff::default();
    loop {
        let index = usage.begin_request()?;
        let started = Instant::now();
        // An abandoned request is retried like any other transient failure.
        let response =
            match tokio::time::timeout(REQUEST_TIMEOUT, provider.complete(request.clone())).await {
                Ok(response) => response,
                Err(_) => Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "provider request had no response within ten minutes",
                )),
            };
        usage.end_request(
            index,
            started.elapsed().as_millis(),
            response.as_ref().ok().map(|r| &r.usage),
        )?;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                if let Some(delay) = backoff.next_delay(&error) {
                    tokio::time::sleep(delay).await;
                    continue;
                }
                return Err(error);
            }
        };
        backoff = providers::ProviderBackoff::default();
        sources.record_inputs(&pending);
        pending.clear();
        if let Some(error) = turn_failure(
            &response.choice,
            Some(&response.stop()),
            response.finish_reason().as_ref(),
        ) {
            return Err(io::Error::other(error));
        }
        let calls: Vec<_> = response.tool_calls().cloned().collect();
        let submissions: Vec<_> = calls
            .iter()
            .filter(|call| call.function.name.as_str() == "submit_judgment")
            .collect();
        if !submissions.is_empty() {
            for call in &calls {
                if call.function.name.as_str() != "submit_judgment" {
                    usage.skipped_tool(call.function.name.as_str());
                }
            }
            if submissions.len() != 1 {
                for _ in &submissions {
                    usage.tool("submit_judgment", true);
                }
                return Err(io::Error::other(
                    "submit_judgment must be invoked exactly once; multiple submissions received",
                ));
            }
            let result = submission(&Value::Object(submissions[0].function.arguments.clone()));
            usage.tool("submit_judgment", result.is_err());
            let (judgment, rationale) = result?;
            usage.record.determination = Some(judgment.kind().into());
            usage.save()?;
            let AssessmentKind::Definition(kind) = &sources.subject_kind;
            let evidence = json!({kind.noun(): sources.subject_text,
                "determination": judgment.to_value(), "rationale": rationale,
                "references": sources.reference_set, "inputs": sources.recorded_inputs()});
            return Ok((judgment, evidence));
        }
        if calls.is_empty() {
            return Err(io::Error::other(
                "model finished without invoking submit_judgment",
            ));
        }
        request.chat_history.push(
            response
                .message()
                .ok_or_else(|| io::Error::other("model returned an empty response"))?,
        );
        let mut results = Vec::new();
        for call in calls {
            match tools::dispatch(
                sources,
                call.function.name.as_str(),
                &Value::Object(call.function.arguments.clone()),
            ) {
                Ok(delivery) => {
                    usage.tool(call.function.name.as_str(), false);
                    usage.prepared(call.function.name.as_str(), &delivery.inputs);
                    pending.extend(delivery.inputs);
                    results.push(
                        call.result(vec![ToolResultContent::text(delivery.result.to_string())]),
                    );
                }
                Err(error) => {
                    usage.tool(call.function.name.as_str(), true);
                    if error.kind() == io::ErrorKind::PermissionDenied {
                        return Err(error);
                    }
                    results
                        .push(call.error_result(vec![ToolResultContent::text(error.to_string())]));
                }
            }
        }
        usage.save()?;
        request.chat_history.push(Message::tool_results(results));
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::time::Duration;

    use rig_core::completion::{CompletionResponse, FinishReason, Usage};
    use rig_core::message::{AssistantContent, Origin, ToolCall, ToolFunction, ToolName};

    use super::*;

    // {test}`requirements.authoring.submission`
    struct JudgmentProvider(Value);

    impl CompletionProvider for JudgmentProvider {
        // {test}`requirements.authoring.submission`
        async fn complete(&self, _request: CompletionRequest) -> io::Result<CompletionResponse> {
            Ok(CompletionResponse::new(
                vec![AssistantContent::ToolCall(ToolCall::from_wire(
                    "judgment",
                    ToolFunction::new(
                        ToolName::new("submit_judgment").unwrap(),
                        json!({"determination": self.0, "rationale": "The supplied source implements the requirement."}),
                    ),
                ))],
                Usage::default(),
                Origin::new("fixture", "fixture", "fixture"),
                Value::Null,
            ).with_finish_reason(FinishReason::ToolCalls))
        }
    }

    // {test}`requirements.authoring.provider_backoff`
    struct ThrottledProvider {
        failures: usize,
        status: u16,
        code: &'static str,
        requests: RefCell<Vec<Value>>,
    }

    impl CompletionProvider for ThrottledProvider {
        // {test}`requirements.authoring.provider_backoff`
        // {test}`requirements.authoring.usage_requests`
        async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
            self.requests
                .borrow_mut()
                .push(serde_json::to_value(&request).unwrap());
            if self.requests.borrow().len() <= self.failures {
                return Err(io::Error::other(
                    rig_core::error::ProviderError::from_provider_body("Too many tokens")
                        .with_provider_status(Some(self.status.try_into().unwrap()))
                        .with_provider_code(Some(self.code.into())),
                ));
            }
            JudgmentProvider(json!(true)).complete(request).await
        }
    }

    /// Reads one source on the first turn, then submits.
    // {test}`requirements.definitions.authoring_inputs`
    // {test}`requirements.authoring.read_denial`
    struct ReadingProvider {
        read: Value,
        requests: RefCell<Vec<Value>>,
    }

    impl CompletionProvider for ReadingProvider {
        // {test}`requirements.definitions.authoring_inputs`
        // {test}`requirements.authoring.read_denial`
        async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
            self.requests
                .borrow_mut()
                .push(serde_json::to_value(&request).unwrap());
            if self.requests.borrow().len() > 1 {
                return JudgmentProvider(json!(true)).complete(request).await;
            }
            Ok(CompletionResponse::new(
                vec![AssistantContent::ToolCall(ToolCall::from_wire(
                    "read",
                    ToolFunction::new(ToolName::new("habeas").unwrap(), self.read.clone()),
                ))],
                Usage::default(),
                Origin::new("fixture", "fixture", "fixture"),
                Value::Null,
            )
            .with_finish_reason(FinishReason::ToolCalls))
        }
    }

    // {test}`requirements.definitions.authoring_inputs`
    #[tokio::test]
    async fn definitions_introduced_by_a_read_are_supplied_and_recorded_before_submission() {
        let project = inputs::tests::project();
        fs::write(
            project.path().join("md/terms.md"),
            format!(
                ":::{{definition}} value\n:phrase: value\nA {}.\n:::\n\n:::{{definition}} unit\n:phrase: unit\nA unit of {}.\n:::\n",
                inputs::tests::role("term", "terms.unit"),
                inputs::tests::role("term", "terms.value"),
            ),
        )
        .unwrap();
        fs::write(
            project.path().join("src/lib.rs"),
            format!(
                "// {}\nfn work() {{}}\n",
                inputs::tests::role("term", "terms.value")
            ),
        )
        .unwrap();
        let mut sources = AssessmentSources::prepare(project.path(), "requirements.ready").unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut usage =
            UsageLog::start(state.path(), project.path(), &sources.subject_name).unwrap();
        let provider = ReadingProvider {
            read: json!({"file": "src/lib.rs", "item": "fn[work]"}),
            requests: RefCell::default(),
        };
        let (_, evidence) = assess(&provider, &mut sources, &mut usage).await.unwrap();

        let definitions = evidence["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|input| input.get("definition").is_some())
            .collect::<Vec<_>>();
        assert_eq!(
            definitions
                .iter()
                .map(|input| input["definition"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["terms.unit", "terms.value"],
            "the transitive set, once each: {evidence}"
        );
        for input in &definitions {
            let text = input["text"].as_array().unwrap();
            assert!(text[0].as_str().unwrap().starts_with(":::{definition}"));
            assert!(text[1].as_str().unwrap().starts_with(":phrase: "));
        }
        // The submission came after the definitions were delivered.
        let requests = provider.requests.borrow();
        assert_eq!(requests.len(), 2);
        let delivered = requests[1].to_string();
        assert!(delivered.contains("A unit of"), "{delivered}");
        assert!(!requests[0].to_string().contains("A unit of"));
    }

    // {test}`requirements.authoring.read_denial`
    #[tokio::test]
    async fn malformed_and_failed_reads_return_to_the_model_and_the_assessment_continues() {
        for read in [
            json!({"file": "src/lib.rs", "line_start": "one"}),
            json!({"file": "src/lib.rs"}),
            json!({"file": "src/lib.rs", "item": "fn[work]", "requirements": "*"}),
            json!({"file": "missing.rs", "line_start": 1}),
            json!({"file": "src/lib.rs", "item": "fn[absent]"}),
            json!({"file": "src/lib.rs", "line_start": 99}),
        ] {
            let project = inputs::tests::project();
            fs::write(project.path().join("src/lib.rs"), "fn work() {}\n").unwrap();
            let mut sources =
                AssessmentSources::prepare(project.path(), "requirements.ready").unwrap();
            let state = tempfile::tempdir().unwrap();
            let mut usage =
                UsageLog::start(state.path(), project.path(), &sources.subject_name).unwrap();
            let provider = ReadingProvider {
                read: read.clone(),
                requests: RefCell::default(),
            };
            let failure = tools::dispatch(&sources, "habeas", &read)
                .err()
                .unwrap_or_else(|| panic!("{read} fails"));
            assert_ne!(failure.kind(), io::ErrorKind::PermissionDenied, "{read}");
            let (_, evidence) = assess(&provider, &mut sources, &mut usage)
                .await
                .unwrap_or_else(|error| panic!("{read}: {error}"));
            // The failure went back to the model as a tool error, and the model
            // then submitted in a second request.
            let requests = provider.requests.borrow();
            assert_eq!(requests.len(), 2, "{read}");
            let message = serde_json::to_string(&failure.to_string()).unwrap();
            let message = message.trim_matches('"');
            assert!(!requests[0].to_string().contains(message), "{read}");
            assert!(requests[1].to_string().contains(message), "{read}");
            assert!(
                evidence["inputs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|input| input
                        .get("file")
                        .is_none_or(|file| file != "src/lib.rs" && file != "missing.rs")),
                "{read}: {evidence}"
            );
        }
    }

    // {test}`requirements.authoring.read_denial`
    #[cfg(unix)]
    #[tokio::test]
    async fn a_read_through_a_symlink_leaving_the_project_aborts_the_assessment() {
        let project = inputs::tests::project();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.rs"), "fn secret() {}\n").unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("secret.rs"),
            project.path().join("src/escape.rs"),
        )
        .unwrap();
        let mut sources = AssessmentSources::prepare(project.path(), "requirements.ready").unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut usage =
            UsageLog::start(state.path(), project.path(), &sources.subject_name).unwrap();
        let provider = ReadingProvider {
            read: json!({"file": "src/escape.rs", "line_start": 1}),
            requests: RefCell::default(),
        };
        let error = assess(&provider, &mut sources, &mut usage)
            .await
            .expect_err("the escaping read aborts");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
        assert!(error.to_string().contains("src/escape.rs"), "{error}");
        assert!(error.to_string().contains("outside the project"), "{error}");
        // No result went back to the model and no further request was made.
        assert_eq!(provider.requests.borrow().len(), 1);
    }

    // {test}`requirements.ci.review_criteria`
    #[test]
    fn initial_prompt_carries_the_review_criteria() {
        let project = inputs::tests::project();
        let sources = AssessmentSources::prepare(project.path(), "requirements.ready").unwrap();
        let prompt = initial_prompt(&sources).unwrap();
        for criterion in [
            "Subject: requirement requirements.ready",
            "Assess only the named subject, using cited requirements and definitions as context.",
            "implements every obligation\ncompletely and exactly as stated",
            "tests cover the interesting points in\nthe design space, including relevant boundaries and failure cases",
            "When another requirement implements part of the subject, assess\nwhether its text carries out that part; do not reassess its implementation,\ntests, or evidence.",
            "without an {impl} reference to it, including code that refers to it\nonly with {cite}, is a gap",
            "behavior in the {impl} code that this\nrequirement does not specify, unless another definition that code implements\nspecifies it",
            "If there are no tests,\nassess whether simplicity or impracticality justifies their absence, and explain.",
            "For an example, assess whether test setup and assertions cover its scenario and\nexpected outcomes.",
            "with source locations",
            "for\na requirement, explain the implementation and testing judgments.",
            "Explain the coverage.",
        ] {
            assert!(
                prompt.contains(criterion),
                "missing {criterion:?} in:\n{prompt}"
            );
        }
    }

    // {test}`requirements.authoring.provider_backoff`
    fn retry_fixture() -> (
        tempfile::TempDir,
        tempfile::TempDir,
        AssessmentSources,
        UsageLog,
    ) {
        let project = inputs::tests::project();
        let sources = AssessmentSources::prepare(project.path(), "requirements.ready").unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut usage =
            UsageLog::start(state.path(), project.path(), &sources.subject_name).unwrap();
        usage.prepared("mandatory", sources.mandatory_inputs());
        (project, state, sources, usage)
    }

    // {test}`requirements.authoring.provider_backoff`
    // {test}`requirements.authoring.usage_requests`
    // {test}`requirements.authoring.usage_inputs`
    // {test}`requirements.ci.reference_set`
    #[tokio::test(start_paused = true)]
    async fn throttled_requests_retry_unchanged_and_record_every_attempt() {
        let (_project, _state, mut sources, mut usage) = retry_fixture();
        let provider = ThrottledProvider {
            failures: 8,
            status: 429,
            code: "ThrottlingException",
            requests: RefCell::default(),
        };
        let started = tokio::time::Instant::now();
        let (_, record) = assess(&provider, &mut sources, &mut usage).await.unwrap();
        assert!(started.elapsed() >= Duration::from_millis(500));
        let requests = provider.requests.borrow();
        assert_eq!(requests.len(), 9);
        assert!(requests.iter().all(|request| request == &requests[0]));
        assert_eq!(record["inputs"], json!(sources.mandatory_inputs()));
        assert_eq!(record["references"], json!(sources.reference_set));

        let log: Value = serde_json::from_slice(&fs::read(&usage.path).unwrap()).unwrap();
        assert_eq!(log["request_count"], 9);
        for attempt in 0..8 {
            assert_eq!(log["requests"][attempt]["outcome"], "failed");
            assert_eq!(log["requests"][attempt]["tokens"]["complete"], false);
        }
        assert_eq!(log["requests"][8]["outcome"], "completed");
        assert_eq!(log["tool_calls"]["submit_judgment"], 1);
        for input in log["inputs"].as_array().unwrap() {
            assert_eq!(input["submitted_in_request"], 0);
            assert_eq!(input["sent_in_request"], 8);
        }
    }

    // {test}`requirements.authoring.provider_backoff`
    #[tokio::test(start_paused = true)]
    async fn permanent_provider_failure_returns_without_waiting_or_retrying() {
        let (_project, _state, mut sources, mut usage) = retry_fixture();
        let provider = ThrottledProvider {
            failures: 8,
            status: 400,
            code: "ValidationException",
            requests: RefCell::default(),
        };
        let started = tokio::time::Instant::now();
        let error = assess(&provider, &mut sources, &mut usage)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("Too many tokens"));
        assert_eq!(started.elapsed(), Duration::ZERO);
        assert_eq!(provider.requests.borrow().len(), 1);
    }

    // {test}`requirements.authoring.provider_backoff`
    #[tokio::test(start_paused = true)]
    async fn cancelling_during_backoff_prevents_another_request() {
        let (_project, _state, mut sources, mut usage) = retry_fixture();
        let provider = ThrottledProvider {
            failures: 8,
            status: 429,
            code: "ThrottlingException",
            requests: RefCell::default(),
        };
        assert!(
            tokio::time::timeout(
                Duration::from_millis(100),
                assess(&provider, &mut sources, &mut usage),
            )
            .await
            .is_err()
        );
        tokio::time::advance(Duration::from_secs(120)).await;
        assert_eq!(provider.requests.borrow().len(), 1);
        assert!(sources.recorded_inputs().is_empty());
        let log: Value = serde_json::from_slice(&fs::read(&usage.path).unwrap()).unwrap();
        assert_eq!(log["request_count"], 1);
        assert_eq!(log["requests"][0]["outcome"], "failed");
    }

    // {test}`requirements.authoring.provider_backoff`
    struct HangingProvider {
        requests: RefCell<usize>,
    }

    impl CompletionProvider for HangingProvider {
        // {test}`requirements.authoring.provider_backoff`
        async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
            *self.requests.borrow_mut() += 1;
            if *self.requests.borrow() == 1 {
                std::future::pending::<()>().await;
            }
            JudgmentProvider(json!(true)).complete(request).await
        }
    }

    // {test}`requirements.authoring.provider_backoff`
    // {test}`requirements.authoring.usage_requests`
    #[tokio::test(start_paused = true)]
    async fn unanswered_requests_are_abandoned_and_retried() {
        let (_project, _state, mut sources, mut usage) = retry_fixture();
        let provider = HangingProvider {
            requests: RefCell::default(),
        };
        let started = tokio::time::Instant::now();
        let (judgment, _) = assess(&provider, &mut sources, &mut usage).await.unwrap();
        assert_eq!(judgment.to_value(), json!(true));
        assert!(started.elapsed() >= REQUEST_TIMEOUT);
        assert_eq!(*provider.requests.borrow(), 2);
        let log: Value = serde_json::from_slice(&fs::read(&usage.path).unwrap()).unwrap();
        assert_eq!(log["request_count"], 2);
        assert_eq!(log["requests"][0]["outcome"], "failed");
        assert_eq!(log["requests"][1]["outcome"], "completed");
    }
    /// Fails with throttling before both requests, answering the first
    /// recovered request with a source read and the second with a judgment.
    // {test}`requirements.authoring.provider_backoff`
    struct ThrottledToolProvider {
        requests: RefCell<Vec<(tokio::time::Instant, Value)>>,
    }

    impl CompletionProvider for ThrottledToolProvider {
        // {test}`requirements.authoring.provider_backoff`
        async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
            let attempt = self.requests.borrow().len();
            self.requests.borrow_mut().push((
                tokio::time::Instant::now(),
                serde_json::to_value(&request).unwrap(),
            ));
            match attempt {
                0 | 2 => Err(io::Error::other(
                    rig_core::error::ProviderError::from_provider_body("slow down")
                        .with_provider_status(Some(429.try_into().unwrap())),
                )),
                1 => Ok(CompletionResponse::new(
                    vec![AssistantContent::ToolCall(ToolCall::from_wire(
                        "read",
                        ToolFunction::new(
                            ToolName::new("habeas").unwrap(),
                            json!({"file": "src/lib.rs", "item": "fn[run]"}),
                        ),
                    ))],
                    Usage::default(),
                    Origin::new("fixture", "fixture", "fixture"),
                    Value::Null,
                )
                .with_finish_reason(FinishReason::ToolCalls)),
                _ => JudgmentProvider(json!(true)).complete(request).await,
            }
        }
    }

    // {test}`requirements.authoring.provider_backoff`
    // {test}`requirements.authoring.usage_requests`
    #[tokio::test(start_paused = true)]
    async fn success_resets_backoff_and_retries_keep_history_without_repeating_tools() {
        let (project, _state, mut sources, mut usage) = retry_fixture();
        fs::write(project.path().join("src/lib.rs"), "fn run() {}\n").unwrap();
        let provider = ThrottledToolProvider {
            requests: RefCell::default(),
        };
        let (_, record) = assess(&provider, &mut sources, &mut usage).await.unwrap();
        let requests = provider.requests.borrow();
        assert_eq!(requests.len(), 4);
        // Each throttled request waits within the initial one-second window:
        // the successful tool turn reset the window doubled by the first failure.
        for (failed, retried) in [(0, 1), (2, 3)] {
            let delay = requests[retried].0 - requests[failed].0;
            assert!(
                delay >= Duration::from_millis(500) && delay <= Duration::from_secs(1),
                "{delay:?}"
            );
        }
        assert_eq!(requests[0].1, requests[1].1);
        // The retry after the tool turn resends the same history, tool
        // results, and tools.
        assert_eq!(requests[2].1, requests[3].1);
        assert_ne!(requests[1].1, requests[2].1);
        assert!(requests[3].1.to_string().contains("fn run()"));

        // The read ran once, and its input was recorded after the retried
        // request succeeded.
        let log: Value = serde_json::from_slice(&fs::read(&usage.path).unwrap()).unwrap();
        assert_eq!(log["tool_calls"]["habeas"], 1, "{log:#}");
        assert_eq!(log["request_count"], 4);
        let inputs = record["inputs"].as_array().unwrap();
        assert!(
            inputs
                .iter()
                .any(|input| input["file"] == "src/lib.rs" && input["item"] == "fn[run]"),
            "{inputs:#?}"
        );
    }

    // {test}`requirements.authoring.submission`
    #[test]
    fn submissions_accept_exactly_a_judgment_and_a_nonempty_rationale() {
        for (determination, expected) in [
            (json!(true), Judgment::True),
            (json!(false), Judgment::False),
            (
                json!({"FIXME": ["#125", "gap"]}),
                Judgment::Fixme(vec!["#125".into(), "gap".into()]),
            ),
        ] {
            let (judgment, rationale) =
                submission(&json!({"determination": determination, "rationale": "**Done.**"}))
                    .unwrap();
            assert_eq!(judgment, expected);
            assert_eq!(rationale, "**Done.**");
        }
        for arguments in [
            json!("true"),
            json!([true, "rationale"]),
            json!({}),
            json!({"rationale": "Missing determination."}),
            json!({"determination": true}),
            json!({"determination": true, "rationale": "Extra.", "confidence": 1}),
            json!({"determination": "true", "rationale": "String judgment."}),
            json!({"determination": 1, "rationale": "Numeric judgment."}),
            json!({"determination": null, "rationale": "Null judgment."}),
            json!({"determination": {"FIXME": []}, "rationale": "Empty FIXME."}),
            json!({"determination": {"FIXME": "#1"}, "rationale": "Not an array."}),
            json!({"determination": {"FIXME": [""]}, "rationale": "Empty entry."}),
            json!({"determination": {"FIXME": ["  "]}, "rationale": "Blank entry."}),
            json!({"determination": {"FIXME": [1]}, "rationale": "Numeric entry."}),
            json!({"determination": {"FIXME": ["#1"], "note": "x"}, "rationale": "Extra key."}),
            json!({"determination": {}, "rationale": "Empty object."}),
            json!({"determination": true, "rationale": ""}),
            json!({"determination": true, "rationale": " \n\t"}),
            json!({"determination": true, "rationale": 7}),
        ] {
            assert!(submission(&arguments).is_err(), "accepted {arguments}");
        }
    }

    // {test}`requirements.authoring.submission`
    #[test]
    fn submit_judgment_is_advertised_with_exactly_its_two_fields() {
        let definitions = serde_json::to_value(tool_definitions().unwrap()).unwrap();
        let submit = definitions
            .as_array()
            .unwrap()
            .iter()
            .find(|definition| definition["name"] == "submit_judgment")
            .expect("submit_judgment is advertised");
        let parameters = &submit["parameters"];
        assert_eq!(parameters["type"], "object");
        assert_eq!(parameters["additionalProperties"], false);
        assert_eq!(
            parameters["required"],
            json!(["determination", "rationale"])
        );
        assert_eq!(
            parameters["properties"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            ["determination", "rationale"]
        );
        assert_eq!(
            parameters["properties"]["rationale"],
            json!({"type": "string", "minLength": 1})
        );
        assert_eq!(
            parameters["properties"]["determination"]["oneOf"][0],
            json!({"type": "boolean"})
        );
        assert!(
            submit["description"]
                .as_str()
                .unwrap()
                .contains("exactly once")
        );
    }

    /// Returns one scripted response per request; each response is a list of
    /// tool calls.
    // {test}`requirements.authoring.submission`
    struct ScriptedProvider {
        turns: Vec<Vec<(&'static str, Value)>>,
        requests: RefCell<usize>,
    }

    impl CompletionProvider for ScriptedProvider {
        // {test}`requirements.authoring.submission`
        async fn complete(&self, _request: CompletionRequest) -> io::Result<CompletionResponse> {
            let turn = *self.requests.borrow();
            *self.requests.borrow_mut() += 1;
            let calls = self.turns[turn]
                .iter()
                .enumerate()
                .map(|(index, (name, arguments))| {
                    AssistantContent::ToolCall(ToolCall::from_wire(
                        format!("call-{turn}-{index}"),
                        ToolFunction::new(ToolName::new(*name).unwrap(), arguments.clone()),
                    ))
                })
                .collect();
            Ok(CompletionResponse::new(
                calls,
                Usage::default(),
                Origin::new("fixture", "fixture", "fixture"),
                Value::Null,
            )
            .with_finish_reason(FinishReason::ToolCalls))
        }
    }

    // {test}`requirements.authoring.submission`
    #[tokio::test]
    async fn a_submission_finishes_the_assessment_wherever_it_arrives() {
        let read = json!({"file": "src/lib.rs", "item": "fn[work]"});
        let submit = json!({"determination": false, "rationale": "Gaps remain."});
        let invalid = json!({"determination": "false", "rationale": "Gaps remain."});
        for (turns, accepted, requests) in [
            // After earlier tool turns.
            (
                vec![
                    vec![("habeas", read.clone())],
                    vec![("habeas", read.clone())],
                    vec![("submit_judgment", submit.clone())],
                ],
                true,
                3,
            ),
            // Alongside other tool calls in the same response, before or after
            // them: the submission finishes and the other calls are skipped.
            (
                vec![vec![
                    ("habeas", read.clone()),
                    ("submit_judgment", submit.clone()),
                ]],
                true,
                1,
            ),
            (
                vec![vec![
                    ("submit_judgment", submit.clone()),
                    ("ripgrep", json!({"pattern": "work"})),
                ]],
                true,
                1,
            ),
            // Multiple submissions anywhere in one response fail even when the
            // first is valid, and invalid arguments fail without a retry.
            (
                vec![vec![
                    ("submit_judgment", submit.clone()),
                    ("habeas", read.clone()),
                    ("submit_judgment", submit.clone()),
                ]],
                false,
                1,
            ),
            (
                vec![
                    vec![("habeas", read.clone())],
                    vec![("submit_judgment", invalid.clone())],
                    vec![("submit_judgment", submit.clone())],
                ],
                false,
                2,
            ),
        ] {
            let project = inputs::tests::project();
            fs::write(project.path().join("src/lib.rs"), "fn work() {}\n").unwrap();
            let mut sources =
                AssessmentSources::prepare(project.path(), "requirements.ready").unwrap();
            let state = tempfile::tempdir().unwrap();
            let mut usage =
                UsageLog::start(state.path(), project.path(), &sources.subject_name).unwrap();
            let provider = ScriptedProvider {
                turns: turns.clone(),
                requests: RefCell::default(),
            };
            let result = assess(&provider, &mut sources, &mut usage).await;
            assert_eq!(*provider.requests.borrow(), requests, "{turns:?}");
            let record = serde_json::to_value(&usage.record).unwrap();
            if accepted {
                let (judgment, evidence) = result.unwrap();
                assert_eq!(judgment, Judgment::False);
                assert_eq!(evidence["rationale"], "Gaps remain.");
                let last = turns.last().unwrap();
                if last.len() > 1 {
                    // The other calls in the submitting response were skipped,
                    // not run, so they delivered no inputs.
                    let skipped = record["tool_skipped"].as_object().unwrap();
                    assert_eq!(
                        skipped.values().filter_map(Value::as_u64).sum::<u64>(),
                        1,
                        "{record:#}"
                    );
                    assert!(record["tool_failures"].as_object().unwrap().is_empty());
                    assert!(
                        evidence["inputs"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .all(|input| input["file"] != "src/lib.rs"),
                        "{evidence}"
                    );
                }
            } else {
                assert!(result.is_err(), "{turns:?}");
            }
        }
    }
}

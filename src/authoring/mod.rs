//! Model-assisted evidence authoring, invoked explicitly and never by validation.

mod batch;
mod inputs;
mod providers;
mod tools;
mod usage;

pub use batch::{BatchOutcome, author_all};

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use rig_core::completion::{CompletionRequest, ToolDefinition};
use rig_core::message::{Message, ToolResultContent, turn_failure};
use serde_json::{Value, json};

use crate::config::{self, BackendKind, ResolvedAuthoringConfig};
use crate::judgment::Judgment;
use inputs::{AssessmentKind, AssessmentSources};
use providers::{CompletionProvider, RigProvider};
use usage::UsageLog;

pub struct AuthoringOutcome {
    pub subject: String,
    pub backend: String,
    pub model: String,
    pub evidence_path: Option<PathBuf>,
    pub usage_path: PathBuf,
    pub determination: Judgment,
}

#[derive(Clone, Debug)]
pub(crate) enum AuthoringSubject {
    Definition(String),
    RustFunction {
        rust_file: String,
        rust_item: String,
    },
}

impl AuthoringSubject {
    // {impl}`requirements.authoring.batch`
    // {impl}`requirements.authoring.function_attestations`
    // {impl}`requirements.authoring.usage_storage`
    pub(crate) fn label(&self) -> String {
        match self {
            Self::Definition(name) => name.clone(),
            Self::RustFunction {
                rust_file,
                rust_item,
            } => format!("{rust_file}:{rust_item}"),
        }
    }
}

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
    pub(crate) fn failed(mut self, error: io::Error) -> io::Error {
        match self.usage.finish("failed") {
            Ok(()) => io::Error::new(
                error.kind(),
                format!("{error}; usage: {}", self.usage.path.display()),
            ),
            Err(usage_error) => io::Error::other(format!(
                "{error}; cannot record failed usage at {}: {usage_error}",
                self.usage.path.display()
            )),
        }
    }
}

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
pub async fn author_evidence_with_output(
    root: &Path,
    subject: &str,
    selection: Option<&str>,
    output: EvidenceOutput,
) -> io::Result<AuthoringOutcome> {
    let root = fs::canonicalize(root)?;
    let state = config::state_root()?;
    fs::create_dir_all(&state)?;
    if fs::canonicalize(&state)?.starts_with(&root) {
        return Err(io::Error::other(
            "authoring usage state must be outside the project repository",
        ));
    }
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
        io::Error::other(format!(
            "cannot record usage at {} (evidence written: {}): {error}",
            usage.path.display(),
            usage.record.evidence_written
        ))
    })?;
    result.map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("{error}; usage: {}", usage.path.display()),
        )
    })
}

// {impl}`requirements.authoring.batch`
// {impl}`requirements.authoring.function_attestations`
// {impl}`requirements.authoring.usage_storage`
// {impl}`requirements.configuration.state_location`
pub(crate) async fn prepare_assessment(
    root: &Path,
    subject: &AuthoringSubject,
    config: &ResolvedAuthoringConfig,
) -> io::Result<PendingAssessment> {
    let root = fs::canonicalize(root)?;
    let state = config::state_root()?;
    fs::create_dir_all(&state)?;
    if fs::canonicalize(&state)?.starts_with(&root) {
        return Err(io::Error::other(
            "authoring usage state must be outside the project repository",
        ));
    }
    let mut usage = UsageLog::start(&state, &root, &subject.label())?;
    let result = tokio::select! {
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
                io::Error::other(format!(
                    "{error}; cannot record usage at {}: {usage_error}",
                    usage.path.display()
                ))
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
    })
}

// {impl}`requirements.authoring.invocation`
// {impl}`requirements.authoring.function_attestations`
// {impl}`requirements.authoring.configuration_precedence`
// {impl}`requirements.authoring.rig_configuration_mapping`
// {impl}`requirements.authoring.usage_storage`
// {impl}`requirements.authoring.mandatory_inputs`
// {impl}`requirements.authoring.usage_inputs`
// {impl}`requirements.authoring.provider`
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
    let mut sources = match subject {
        AuthoringSubject::Definition(name) => AssessmentSources::prepare(root, name),
        AuthoringSubject::RustFunction {
            rust_file,
            rust_item,
        } => AssessmentSources::prepare_function(root, rust_file, rust_item),
    }
    .map_err(io::Error::other)?;
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
// {impl}`requirements.authoring.function_attestations`
// {impl}`requirements.ci.review_criteria`
// {impl}`requirements.ci.outcomes`
// {impl}`requirements.examples.evidence`
fn initial_prompt(sources: &AssessmentSources) -> io::Result<String> {
    // Keep the approved template as the sole source of these instructions.
    let chapter = include_str!("../../md/requirements/authoring.md");
    let opening = match &sources.subject_kind {
        AssessmentKind::Definition(_) => {
            "Bring your expert judgment to the task of assessing whether a project's sources satisfy a requirement or example."
        }
        AssessmentKind::RustFunction { .. } => {
            "Bring your expert judgment to the task of assessing a Rust function's citation completeness."
        }
    };
    let start = chapter.find(opening).expect("approved prompt template");
    let template = chapter[start..]
        .split("\n```")
        .next()
        .expect("prompt closing fence");
    let citations = match &sources.subject_kind {
        AssessmentKind::Definition(_) => String::new(),
        AssessmentKind::RustFunction { citations, .. } => serde_json::to_string_pretty(citations)?,
    };
    Ok(template
        .replace("{{subject_kind}}", sources.subject_kind.noun())
        .replace("{{subject_name}}", &sources.subject_name)
        .replace("{{citations}}", &citations)
        .replace(
            "{{mandatory_inputs}}",
            &serde_json::to_string_pretty(sources.mandatory_inputs())?,
        ))
}

// {impl}`requirements.authoring.submission`
// {impl}`requirements.authoring.function_attestations`
fn submission(arguments: &Value, function: bool) -> io::Result<(Judgment, String)> {
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
    if function && !object["determination"].is_boolean() {
        return Err(io::Error::other(
            "Rust function attestation determination must be boolean",
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
// {impl}`requirements.authoring.function_attestations`
fn tool_definitions(function: bool) -> io::Result<Vec<ToolDefinition>> {
    let mut definitions = tools::definitions();
    let determination = if function {
        json!({"type": "boolean"})
    } else {
        json!({"oneOf":[{"type":"boolean"},{"type":"object","additionalProperties":false,"required":["FIXME"],"properties":{"FIXME":{"type":"array","minItems":1,"items":{"type":"string","minLength":1}}}}]})
    };
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
// {impl}`requirements.authoring.function_attestations`
// {impl}`requirements.authoring.usage_storage`
// {impl}`requirements.authoring.mandatory_inputs`
async fn assess(
    provider: &impl CompletionProvider,
    sources: &mut AssessmentSources,
    usage: &mut UsageLog,
) -> io::Result<(Judgment, Value)> {
    let mut request = CompletionRequest::new(initial_prompt(sources)?);
    let function = matches!(sources.subject_kind, AssessmentKind::RustFunction { .. });
    request.tools = tool_definitions(function)?;
    let mut pending = sources.mandatory_inputs().to_vec();
    loop {
        let index = usage.begin_request()?;
        let started = Instant::now();
        let response = provider.complete(request.clone()).await;
        usage.end_request(
            index,
            started.elapsed().as_millis(),
            response.as_ref().ok().map(|r| &r.usage),
        )?;
        let response = response?;
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
            let result = submission(
                &Value::Object(submissions[0].function.arguments.clone()),
                function,
            );
            usage.tool("submit_judgment", result.is_err());
            let (judgment, rationale) = result?;
            usage.record.determination = Some(judgment.kind().into());
            usage.save()?;
            let evidence = match &sources.subject_kind {
                AssessmentKind::Definition(kind) => json!({kind.noun(): sources.subject_text,
                    "determination": judgment.to_value(), "rationale": rationale, "inputs": sources.recorded_inputs()}),
                AssessmentKind::RustFunction {
                    rust_item,
                    citations,
                } => json!({
                    "rust_item": rust_item, "citations": citations,
                    "determination": judgment.to_value(), "rationale": rationale, "inputs": sources.recorded_inputs()
                }),
            };
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
                    results.push(call.error_result(vec![ToolResultContent::text(error)]));
                }
            }
        }
        usage.save()?;
        request.chat_history.push(Message::tool_results(results));
    }
}

#[cfg(test)]
mod tests {
    use rig_core::completion::{CompletionResponse, FinishReason, Usage};
    use rig_core::message::{AssistantContent, Origin, ToolCall, ToolFunction, ToolName};

    use super::*;

    struct JudgmentProvider(Value);

    impl CompletionProvider for JudgmentProvider {
        // {test}`requirements.authoring.function_attestations`
        // {test}`requirements.authoring.submission`
        async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
            let submit = request
                .tools
                .iter()
                .find(|tool| tool.name == "submit_judgment")
                .unwrap();
            assert_eq!(
                submit.parameters["properties"]["determination"],
                json!({"type": "boolean"})
            );
            Ok(CompletionResponse::new(
                vec![AssistantContent::ToolCall(ToolCall::from_wire(
                    "judgment",
                    ToolFunction::new(
                        ToolName::new("submit_judgment").unwrap(),
                        json!({"determination": self.0, "rationale": "The supplied source accounts for this citation set."}),
                    ),
                ))],
                Usage::default(),
                Origin::new("fixture", "fixture", "fixture"),
                Value::Null,
            ).with_finish_reason(FinishReason::ToolCalls))
        }
    }

    // {test}`requirements.authoring.function_attestations`
    // {test}`requirements.authoring.submission`
    // {test}`requirements.authoring.delivered_inputs`
    // {test}`requirements.authoring.usage_storage`
    // {test}`requirements.authoring.mandatory_inputs`
    #[tokio::test]
    async fn function_judgments_use_boolean_schema_and_harness_owned_inputs() {
        let project = inputs::tests::project();
        fs::write(
            project.path().join("src/lib.rs"),
            format!(
                "// {}\npub fn ready() {{}}\n",
                inputs::tests::role("impl", "requirements.ready")
            ),
        )
        .unwrap();
        let state = tempfile::tempdir().unwrap();
        for value in [json!(true), json!(false), json!({"FIXME": ["#125"]})] {
            let mut sources =
                AssessmentSources::prepare_function(project.path(), "src/lib.rs", "fn[ready]")
                    .unwrap();
            let prompt = initial_prompt(&sources).unwrap();
            assert!(prompt.starts_with("Bring your expert judgment to the task of assessing a Rust function's citation completeness."));
            assert!(prompt.contains("src/lib.rs:fn[ready]"));
            assert!(prompt.contains("requirements.ready"));
            assert!(!prompt.contains("{{"));
            let mut usage =
                UsageLog::start(state.path(), project.path(), &sources.subject_name).unwrap();
            usage.prepared("mandatory", sources.mandatory_inputs());
            let result = assess(&JudgmentProvider(value.clone()), &mut sources, &mut usage).await;
            assert!(!project.path().join("evidence").exists());
            assert!(!usage.record.evidence_written);
            if value.is_boolean() {
                let (judgment, record) = result.unwrap();
                assert_eq!(judgment.to_value(), value);
                assert_eq!(record["determination"], value);
                assert_eq!(record["rust_item"], "fn[ready]");
                assert_eq!(
                    record["citations"],
                    json!([{"role": "impl", "name": "requirements.ready"}])
                );
                assert_eq!(record["inputs"], json!(sources.mandatory_inputs()));
                assert!(record.get("requirement").is_none());
                assert_eq!(usage.record.determination.as_deref(), Some(judgment.kind()));
            } else {
                assert!(result.unwrap_err().to_string().contains("must be boolean"));
                assert!(usage.record.determination.is_none());
            }
        }
    }
}

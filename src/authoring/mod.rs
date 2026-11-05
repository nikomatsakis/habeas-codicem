//! Model-assisted evidence authoring, invoked explicitly and never by validation.

mod inputs;
mod providers;
mod tools;
mod usage;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

use rig_core::completion::{CompletionRequest, ToolDefinition};
use rig_core::message::{Message, ToolResultContent, turn_failure};
use serde_json::{Value, json};

use crate::config::{self, BackendKind};
use crate::judgment::Judgment;
use inputs::AssessmentSources;
use providers::{CompletionProvider, RigProvider};
use usage::UsageLog;

pub struct AuthoringOutcome {
    pub subject: String,
    pub backend: String,
    pub model: String,
    pub evidence_path: PathBuf,
    pub usage_path: PathBuf,
    pub determination: Judgment,
}

// {impl}`requirements.authoring.invocation`
// {impl}`requirements.authoring.usage_storage`
// {impl}`requirements.configuration.state_location`
pub async fn author_evidence(
    root: &Path,
    subject: &str,
    selection: Option<&str>,
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
    let result = tokio::select! {
        result = configured_assessment(&root, subject, selection, &mut usage) => result,
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

// {impl}`requirements.authoring.invocation`
// {impl}`requirements.authoring.configuration_precedence`
// {impl}`requirements.authoring.rig_configuration_mapping`
// {impl}`requirements.authoring.usage_storage`
async fn configured_assessment(
    root: &Path,
    subject: &str,
    selection: Option<&str>,
    usage: &mut UsageLog,
) -> io::Result<AuthoringOutcome> {
    let config = config::resolve_authoring(root, selection)?;
    let model = config.model_ref();
    let backend = match model.backend {
        BackendKind::Bedrock => "bedrock",
        BackendKind::Ollama => "ollama",
    };
    usage.record.backend = Some(backend.into());
    usage.record.model = Some(model.model.clone());
    let mut sources = AssessmentSources::prepare(root, subject).map_err(io::Error::other)?;
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
    let (determination, evidence_path) = assess(&provider, root, &mut sources, usage).await?;
    Ok(AuthoringOutcome {
        subject: subject.into(),
        backend: backend.into(),
        model: model.model,
        evidence_path,
        usage_path: usage.path.clone(),
        determination,
    })
}

// {impl}`requirements.authoring.initial_prompt`
// {impl}`requirements.ci.review_criteria`
// {impl}`requirements.ci.outcomes`
fn initial_prompt(sources: &AssessmentSources) -> io::Result<String> {
    // Keep the approved template as the sole source of these instructions.
    let chapter = include_str!("../../md/requirements/authoring.md");
    let start = chapter
        .find("Bring your expert judgment")
        .expect("approved prompt template");
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
fn tool_definitions() -> io::Result<Vec<ToolDefinition>> {
    let mut definitions = tools::definitions();
    definitions.push(json!({"name": "submit_judgment", "description": "Submit your final judgment exactly once. No other message supplies the result.",
        "parameters": {"type":"object", "additionalProperties": false, "required":["determination","rationale"], "properties": {
            "determination": {"oneOf":[{"type":"boolean"},{"type":"object","additionalProperties":false,"required":["FIXME"],"properties":{"FIXME":{"type":"array","minItems":1,"items":{"type":"string","minLength":1}}}}]},
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
async fn assess(
    provider: &impl CompletionProvider,
    root: &Path,
    sources: &mut AssessmentSources,
    usage: &mut UsageLog,
) -> io::Result<(Judgment, PathBuf)> {
    let mut request = CompletionRequest::new(initial_prompt(sources)?);
    request.tools = tool_definitions()?;
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
            let result = submission(&Value::Object(submissions[0].function.arguments.clone()));
            usage.tool("submit_judgment", result.is_err());
            let (judgment, rationale) = result?;
            usage.record.determination = Some(judgment.kind().into());
            usage.save()?;
            let evidence_path = root
                .join("evidence")
                .join(format!("{}.json", sources.subject_name));
            let evidence = json!({sources.subject_kind.noun(): sources.subject_text,
                "determination": judgment.to_value(), "rationale": rationale, "inputs": sources.recorded_inputs()});
            fs::create_dir_all(evidence_path.parent().expect("evidence parent"))?;
            fs::write(
                &evidence_path,
                format!("{}\n", serde_json::to_string_pretty(&evidence)?),
            )?;
            usage.record.evidence_written = true;
            usage.save()?;
            return Ok((judgment, evidence_path));
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

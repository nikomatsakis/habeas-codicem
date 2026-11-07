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

// {impl}`requirements.authoring.invocation`
pub struct AuthoringOutcome {
    pub subject: String,
    pub backend: String,
    pub model: String,
    pub evidence_path: Option<PathBuf>,
    pub usage_path: PathBuf,
    pub determination: Judgment,
    pub rationale: String,
}

// {impl}`requirements.authoring.reporting`
// {impl}`requirements.authoring.confirmation`
#[derive(Clone, Copy, Debug, Default)]
pub struct AuthoringOptions {
    pub verbose: bool,
    /// Confirm `true` and FIXME results with a second assessment.
    pub confirm: bool,
}

// :::{impl} requirements.authoring.invocation
// The `Definition` variant is the one requirement or named example given on the
// command line: the subject the command assesses and reports.
// :::
// {impl}`requirements.references.citation_review`
#[derive(Clone, Debug)]
pub(crate) enum AuthoringSubject {
    Definition(String),
    /// The citation review of one source item outside the book.
    Citation {
        file: String,
        item: String,
    },
}

impl AuthoringSubject {
    /// Parses a single-subject name: a full requirement or example name, or
    /// `citations:<file>#<selector>` for a citation review, where an omitted
    /// or empty selector names the file's root node.
    // {impl}`requirements.authoring.invocation`
    // {impl}`requirements.references.citation_review`
    pub(crate) fn parse(subject: &str) -> Self {
        match subject.strip_prefix("citations:") {
            Some(rest) => {
                let (file, item) = rest.split_once('#').unwrap_or((rest, ""));
                Self::Citation {
                    file: file.to_owned(),
                    item: item.to_owned(),
                }
            }
            None => Self::Definition(subject.to_owned()),
        }
    }

    // {impl}`requirements.authoring.batch`
    // {impl}`requirements.authoring.usage_storage`
    // {impl}`requirements.references.citation_review`
    pub(crate) fn label(&self) -> String {
        match self {
            Self::Definition(name) => name.clone(),
            Self::Citation { file, item } => crate::citations::subject_label(file, item),
        }
    }

    /// Where the subject's record is written.
    // {impl}`requirements.authoring.persistence`
    // {impl}`requirements.references.citation_review`
    pub(crate) fn evidence_path(&self, root: &Path) -> PathBuf {
        match self {
            Self::Definition(name) => root.join("evidence").join(format!("{name}.json")),
            Self::Citation { file, item } => root.join(crate::citations::review_path(file, item)),
        }
    }

    /// Prepares the subject's sources and mandatory inputs.
    // {impl}`requirements.authoring.mandatory_inputs`
    // {impl}`requirements.references.citation_review`
    fn sources(&self, root: &Path) -> io::Result<AssessmentSources> {
        match self {
            Self::Definition(name) => AssessmentSources::prepare(root, name),
            Self::Citation { file, item } => AssessmentSources::prepare_citation(root, file, item),
        }
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

// {impl}`requirements.authoring.invocation`
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
    author_evidence_with_options(
        root,
        subject,
        selection,
        output,
        AuthoringOptions::default(),
    )
    .await
}

/// Assesses one subject, optionally confirming its result with further
/// independent conversations.
// {impl}`requirements.authoring.invocation`
// {impl}`requirements.authoring.usage_storage`
// {impl}`requirements.configuration.state_location`
// {impl}`requirements.authoring.read_denial`
// {impl}`requirements.authoring.confirmation`
pub async fn author_evidence_with_options(
    root: &Path,
    subject: &str,
    selection: Option<&str>,
    output: EvidenceOutput,
    options: AuthoringOptions,
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
        result = configured_assessment(&root, subject, selection, output, options.confirm, &mut usage) => result,
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
    confirm: bool,
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
        result = configured_record(&root, subject, config, confirm, &mut usage) => result,
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
    confirm: bool,
    usage: &mut UsageLog,
) -> io::Result<AuthoringOutcome> {
    let config = config::resolve_authoring(root, selection)?;
    let parsed = AuthoringSubject::parse(subject);
    let (determination, evidence) =
        configured_record(root, &parsed, &config, confirm, usage).await?;
    let json = format!("{}\n", serde_json::to_string_pretty(&evidence)?);
    let evidence_path = match output {
        EvidenceOutput::File => {
            let path = parsed.evidence_path(root);
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
    confirm: bool,
    usage: &mut UsageLog,
) -> io::Result<(Judgment, Value)> {
    let model = config.model_ref();
    let backend = match model.backend {
        BackendKind::Bedrock => "bedrock",
        BackendKind::Ollama => "ollama",
    };
    usage.record.backend = Some(backend.into());
    usage.record.model = Some(model.model.clone());
    // Mandatory inputs are prepared, and an unreadable one reported, before
    // connecting to the provider.
    let sources = subject.sources(root)?;
    let provider = RigProvider::connect(&config.backend)
        .await
        .map_err(|error| {
            io::Error::other(format!(
                "{}: authoring.configs.{}: {error}",
                config.config_path.display(),
                config.name
            ))
        })?;
    let state = usage_state_root(root)?;
    assess_subject(&provider, root, &state, subject, sources, usage, confirm).await
}

/// Assesses a subject and, with `confirm`, confirms a `true` or FIXME result
/// in an independent second conversation, verifying the less favorable of
/// disagreeing assessments in a third. Each conversation has its own usage
/// record; `usage` is the first's.
// {impl}`requirements.authoring.confirmation`
// {impl}`requirements.authoring.usage_storage`
// {impl}`requirements.authoring.mandatory_inputs`
// {impl}`requirements.authoring.usage_inputs`
async fn assess_subject(
    provider: &impl CompletionProvider,
    root: &Path,
    state: &Path,
    subject: &AuthoringSubject,
    mut sources: AssessmentSources,
    usage: &mut UsageLog,
    confirm: bool,
) -> io::Result<(Judgment, Value)> {
    usage.record.subject_kind = Some(sources.subject_kind.noun().into());
    usage.prepared("mandatory", sources.mandatory_inputs());
    usage.save()?;
    let first = assess(provider, &mut sources, usage).await?;
    if !confirm || first.0 == Judgment::False {
        return Ok(first);
    }
    let mut log = followup_log(state, root, subject, usage, "second assessment")?;
    let result = match followup_sources(root, subject, &mut log) {
        Ok(mut sources) => assess(provider, &mut sources, &mut log).await,
        Err(error) => Err(error),
    };
    let second = finish_followup(log, "second assessment", result)?;
    if first.0.kind() == second.0.kind() {
        return Ok(first);
    }
    let first_kind = first.0.kind();
    let second_kind = second.0.kind();
    let (less, more) = if favorability(&second.0) < favorability(&first.0) {
        (second, first)
    } else {
        (first, second)
    };
    let less_judgment = less.0.clone();
    let less_kind = less.0.kind();
    let less_rationale = less.1["rationale"].as_str().unwrap_or_default().to_owned();
    let mut log = followup_log(state, root, subject, usage, "verification")?;
    let result = match followup_sources(root, subject, &mut log) {
        Ok(mut sources) => {
            verify(
                provider,
                &mut sources,
                &mut log,
                &less_judgment,
                &less_rationale,
            )
            .await
        }
        Err(error) => Err(error),
    };
    let (confirmed, verification) = finish_followup(log, "verification", result)?;
    let (mut chosen, outcome) = if confirmed {
        (less, "confirmed")
    } else {
        (more, "did not confirm")
    };
    let note = format!(
        "**Confirmation:** the assessments disagreed: the first determination was `{first_kind}` and an independent second assessment's was `{second_kind}`. A verifier checked the less favorable `{}` determination and {outcome} it, so the `{}` assessment is recorded.\n\nVerification:\n\n{}",
        less_kind,
        chosen.0.kind(),
        verification.trim()
    );
    let rationale = format!(
        "{}\n\n{note}",
        chosen.1["rationale"].as_str().unwrap_or_default().trim()
    );
    chosen.1["rationale"] = Value::String(rationale);
    usage.record.determination = Some(chosen.0.kind().into());
    usage.save()?;
    Ok(chosen)
}

/// Orders determinations: `false` is less favorable than FIXME, and FIXME
/// than `true`.
// {impl}`requirements.authoring.confirmation`
fn favorability(judgment: &Judgment) -> u8 {
    match judgment {
        Judgment::False => 0,
        Judgment::Fixme(_) => 1,
        Judgment::True => 2,
    }
}

/// Starts the usage record of a further conversation about a subject.
// {impl}`requirements.authoring.confirmation`
// {impl}`requirements.authoring.usage_storage`
fn followup_log(
    state: &Path,
    root: &Path,
    subject: &AuthoringSubject,
    first: &UsageLog,
    purpose: &str,
) -> io::Result<UsageLog> {
    let mut usage = UsageLog::start(state, root, &format!("{} ({purpose})", subject.label()))?;
    usage.record.backend = first.record.backend.clone();
    usage.record.model = first.record.model.clone();
    usage.record.evidence_output = first.record.evidence_output;
    Ok(usage)
}

/// Fresh sources for an independent conversation, recorded in its usage.
// {impl}`requirements.authoring.confirmation`
// {impl}`requirements.authoring.usage_inputs`
fn followup_sources(
    root: &Path,
    subject: &AuthoringSubject,
    usage: &mut UsageLog,
) -> io::Result<AssessmentSources> {
    let sources = subject.sources(root)?;
    usage.record.subject_kind = Some(sources.subject_kind.noun().into());
    usage.prepared("mandatory", sources.mandatory_inputs());
    usage.save()?;
    Ok(sources)
}

/// Finishes a further conversation's usage record with its outcome.
// {impl}`requirements.authoring.confirmation`
// {impl}`requirements.authoring.usage_storage`
fn finish_followup<T>(mut usage: UsageLog, purpose: &str, result: io::Result<T>) -> io::Result<T> {
    let outcome = match &result {
        Ok(_) => "completed",
        Err(error) if error.kind() == io::ErrorKind::Interrupted => "cancelled",
        Err(_) => "failed",
    };
    usage.finish(outcome)?;
    result.map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("{purpose}: {error}; usage: {}", usage.path.display()),
        )
    })
}

/// The opening line of each approved prompt template in the authoring chapter.
// {impl}`requirements.authoring.initial_prompt`
const DEFINITION_PROMPT: &str = "Bring your expert judgment to the task of assessing whether a project's sources satisfy a requirement or example.";
// {impl}`requirements.authoring.citation_prompt`
const CITATION_PROMPT: &str =
    "Bring your expert judgment to the task of reviewing a source item's citations.";
// {impl}`requirements.authoring.confirmation`
const VERIFICATION_PROMPT: &str = "Another reviewer judged {{subject_kind}} {{subject_name}} as {{determination}} for these reasons:";

/// An approved prompt template, from its opening line to its closing fence.
// {impl}`requirements.authoring.initial_prompt`
// {impl}`requirements.authoring.citation_prompt`
// {impl}`requirements.authoring.confirmation`
fn prompt_template(opening: &str) -> &'static str {
    // Keep the approved templates as the sole source of these instructions.
    let chapter = include_str!("../../md/requirements/authoring.md");
    let start = chapter.find(opening).expect("approved prompt template");
    chapter[start..]
        .split("\n```")
        .next()
        .expect("prompt closing fence")
}

/// Replaces `{{name}}` placeholders in one pass, so substituted text that
/// itself contains a placeholder, such as a requirement quoting a template,
/// is left as written.
// {impl}`requirements.authoring.initial_prompt`
// {impl}`requirements.authoring.citation_prompt`
// {impl}`requirements.authoring.confirmation`
fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut result = String::with_capacity(template.len());
    let mut rest = template;
    'scan: while let Some(position) = rest.find("{{") {
        result.push_str(&rest[..position]);
        rest = &rest[position..];
        for (name, value) in values {
            let placeholder = format!("{{{{{name}}}}}");
            if let Some(after) = rest.strip_prefix(placeholder.as_str()) {
                result.push_str(value);
                rest = after;
                continue 'scan;
            }
        }
        result.push_str("{{");
        rest = &rest[2..];
    }
    result.push_str(rest);
    result
}

// {impl}`requirements.authoring.initial_prompt`
// {impl}`requirements.ci.review_criteria`
// {impl}`requirements.ci.outcomes`
// {impl}`requirements.examples.evidence`
// {impl}`requirements.authoring.mandatory_inputs`
// {impl}`requirements.authoring.citation_prompt`
fn initial_prompt(sources: &AssessmentSources) -> io::Result<String> {
    let mandatory = serde_json::to_string_pretty(sources.mandatory_inputs())?;
    Ok(match &sources.subject_kind {
        AssessmentKind::Definition(_) => fill(
            prompt_template(DEFINITION_PROMPT),
            &[
                ("subject_kind", sources.subject_kind.noun()),
                ("subject_name", &sources.subject_name),
                ("mandatory_inputs", &mandatory),
            ],
        ),
        AssessmentKind::Citation(item) => fill(
            prompt_template(CITATION_PROMPT),
            &[
                ("item_file", &item.file),
                ("item_selector", item.selector_label()),
                (
                    "citations",
                    &serde_json::to_string(&item.citations_value())?,
                ),
                ("mandatory_inputs", &mandatory),
            ],
        ),
    })
}

/// The verifier's prompt for a determination and its rationale.
// {impl}`requirements.authoring.confirmation`
fn verification_prompt(
    sources: &AssessmentSources,
    determination: &Judgment,
    rationale: &str,
) -> io::Result<String> {
    Ok(fill(
        prompt_template(VERIFICATION_PROMPT),
        &[
            ("subject_kind", sources.subject_kind.noun()),
            ("subject_name", &sources.subject_name),
            ("determination", &determination.to_value().to_string()),
            ("rationale", rationale.trim()),
            (
                "mandatory_inputs",
                &serde_json::to_string_pretty(sources.mandatory_inputs())?,
            ),
        ],
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

/// Parses a `submit_verification` call: exactly a boolean `confirmed` and a
/// nonempty `rationale`.
// {impl}`requirements.authoring.confirmation`
fn verification(arguments: &Value) -> io::Result<(bool, String)> {
    let object = arguments
        .as_object()
        .ok_or_else(|| io::Error::other("submit_verification expects an object"))?;
    if object.len() != 2 || !object.contains_key("confirmed") || !object.contains_key("rationale") {
        return Err(io::Error::other(
            "submit_verification requires exactly confirmed and rationale",
        ));
    }
    let confirmed = object["confirmed"]
        .as_bool()
        .ok_or_else(|| io::Error::other("submit_verification confirmed must be a boolean"))?;
    let rationale = object["rationale"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            io::Error::other("submit_verification rationale must be nonempty Markdown")
        })?;
    Ok((confirmed, rationale.to_owned()))
}

/// The source tools and one submission tool.
// {impl}`requirements.authoring.submission`
// {impl}`requirements.authoring.initial_prompt`
// {impl}`requirements.ci.outcomes`
// {impl}`requirements.authoring.confirmation`
fn tool_definitions_with(submit: &str) -> io::Result<Vec<ToolDefinition>> {
    let mut definitions = tools::definitions();
    if submit == "submit_verification" {
        definitions.push(json!({"name": "submit_verification", "description": "Submit your verification exactly once. No other message supplies the result.",
            "parameters": {"type":"object", "additionalProperties": false, "required":["confirmed","rationale"], "properties": {
                "confirmed":{"type":"boolean"},
                "rationale":{"type":"string","minLength":1}}}}));
    } else {
        let determination = json!({"oneOf":[{"type":"boolean"},{"type":"object","additionalProperties":false,"required":["FIXME"],"properties":{"FIXME":{"type":"array","minItems":1,"items":{"type":"string","minLength":1}}}}]});
        definitions.push(json!({"name": "submit_judgment", "description": "Submit your final judgment exactly once. No other message supplies the result.",
            "parameters": {"type":"object", "additionalProperties": false, "required":["determination","rationale"], "properties": {
                "determination": determination,
                "rationale":{"type":"string","minLength":1}}}}));
    }
    definitions
        .into_iter()
        .map(|value| serde_json::from_value(value).map_err(io::Error::other))
        .collect()
}

// {impl}`requirements.authoring.submission`
// {impl}`requirements.authoring.initial_prompt`
// {impl}`requirements.ci.outcomes`
#[cfg(test)]
fn tool_definitions() -> io::Result<Vec<ToolDefinition>> {
    tool_definitions_with("submit_judgment")
}

/// The record a judgment produces: definition evidence, or a citation
/// review with the item's file, selector, and citations.
// {impl}`requirements.authoring.persistence`
// {impl}`requirements.ci.schema`
// {impl}`requirements.ci.reference_set`
// {impl}`requirements.references.citation_review`
fn record(sources: &AssessmentSources, judgment: &Judgment, rationale: &str) -> Value {
    match &sources.subject_kind {
        AssessmentKind::Definition(kind) => json!({kind.noun(): sources.subject_text,
            "determination": judgment.to_value(), "rationale": rationale,
            "references": sources.reference_set, "inputs": sources.recorded_inputs()}),
        AssessmentKind::Citation(item) => json!({"file": item.file, "item": item.item,
            "citations": item.citations_value(), "determination": judgment.to_value(),
            "rationale": rationale, "inputs": sources.recorded_inputs()}),
    }
}

// {impl}`requirements.authoring.submission`
// {impl}`requirements.authoring.initial_prompt`
// {impl}`requirements.authoring.persistence`
// {impl}`requirements.authoring.usage_storage`
// {impl}`requirements.authoring.mandatory_inputs`
// {impl}`requirements.ci.reference_set`
// {impl}`requirements.ci.schema`
// {impl}`requirements.references.citation_review`
async fn assess(
    provider: &impl CompletionProvider,
    sources: &mut AssessmentSources,
    usage: &mut UsageLog,
) -> io::Result<(Judgment, Value)> {
    let prompt = initial_prompt(sources)?;
    let (judgment, rationale) = converse(
        provider,
        sources,
        usage,
        prompt,
        "submit_judgment",
        submission,
    )
    .await?;
    usage.record.determination = Some(judgment.kind().into());
    usage.save()?;
    let evidence = record(sources, &judgment, &rationale);
    Ok((judgment, evidence))
}

/// Verifies a determination and its rationale in its own conversation,
/// returning whether the verifier confirmed it and the verifier's rationale.
// {impl}`requirements.authoring.confirmation`
async fn verify(
    provider: &impl CompletionProvider,
    sources: &mut AssessmentSources,
    usage: &mut UsageLog,
    determination: &Judgment,
    rationale: &str,
) -> io::Result<(bool, String)> {
    let prompt = verification_prompt(sources, determination, rationale)?;
    converse(
        provider,
        sources,
        usage,
        prompt,
        "submit_verification",
        verification,
    )
    .await
}

/// One conversation: sends the prompt with the source tools and the named
/// submission tool, delivers tool results until the model submits exactly
/// once, and returns the parsed submission.
// {impl}`requirements.authoring.submission`
// {impl}`requirements.authoring.delivered_inputs`
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
// {impl}`requirements.authoring.confirmation`
async fn converse<T>(
    provider: &impl CompletionProvider,
    sources: &mut AssessmentSources,
    usage: &mut UsageLog,
    prompt: String,
    submit: &str,
    parse: impl Fn(&Value) -> io::Result<T>,
) -> io::Result<T> {
    let mut request = CompletionRequest::new(prompt);
    request.tools = tool_definitions_with(submit)?;
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
            .filter(|call| call.function.name.as_str() == submit)
            .collect();
        if !submissions.is_empty() {
            for call in &calls {
                if call.function.name.as_str() != submit {
                    usage.skipped_tool(call.function.name.as_str());
                }
            }
            if submissions.len() != 1 {
                for _ in &submissions {
                    usage.tool(submit, true);
                }
                return Err(io::Error::other(format!(
                    "{submit} must be invoked exactly once; multiple submissions received"
                )));
            }
            let result = parse(&Value::Object(submissions[0].function.arguments.clone()));
            usage.tool(submit, result.is_err());
            return result;
        }
        if calls.is_empty() {
            return Err(io::Error::other(format!(
                "model finished without invoking {submit}"
            )));
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

    // {test}`requirements.ci.outcomes`
    #[test]
    fn submit_judgment_advertises_exactly_the_three_outcome_shapes() {
        let definitions = serde_json::to_value(tool_definitions().unwrap()).unwrap();
        let submit = definitions
            .as_array()
            .unwrap()
            .iter()
            .find(|definition| definition["name"] == "submit_judgment")
            .expect("submit_judgment is advertised");
        // `true` or `false`, or an object holding exactly `FIXME` with a
        // nonempty array of nonempty strings.
        assert_eq!(
            submit["parameters"]["properties"]["determination"],
            json!({"oneOf": [
                {"type": "boolean"},
                {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["FIXME"],
                    "properties": {"FIXME": {
                        "type": "array",
                        "minItems": 1,
                        "items": {"type": "string", "minLength": 1}
                    }}
                }
            ]})
        );
    }

    // {test}`requirements.ci.outcomes`
    #[test]
    fn initial_prompt_conveys_when_to_choose_each_outcome() {
        let project = inputs::tests::project();
        let sources = AssessmentSources::prepare(project.path(), "requirements.ready").unwrap();
        let prompt = initial_prompt(&sources).unwrap();
        for guidance in [
            "Choose true when the subject meets these criteria.",
            "Choose {\"FIXME\": [\"...\"]} when\nthe remaining gaps are explicitly acknowledged in source comments or tests;\nlist their issue references or other identifying content.",
            "The usual convention\nis FIXME(#125) or FIXME(rust-lang/rust#123), but references may occur elsewhere.",
            "Choose false for other gaps or insufficient evidence.",
            "including any acknowledged gaps",
        ] {
            assert!(
                prompt.contains(guidance),
                "missing {guidance:?} in:\n{prompt}"
            );
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

    /// Answers each request with the next scripted tool call, recording the
    /// requests it receives.
    // {test}`requirements.authoring.confirmation`
    // {test}`requirements.authoring.citation_prompt`
    struct RecordingProvider {
        turns: Vec<(&'static str, Value)>,
        requests: RefCell<Vec<Value>>,
    }

    impl RecordingProvider {
        // {test}`requirements.authoring.confirmation`
        fn new(turns: Vec<(&'static str, Value)>) -> Self {
            Self {
                turns,
                requests: RefCell::default(),
            }
        }

        /// The prompt and advertised tool names of each request.
        // {test}`requirements.authoring.confirmation`
        fn prompts(&self) -> Vec<(String, Vec<String>)> {
            self.requests
                .borrow()
                .iter()
                .map(|request| {
                    let text = request.to_string();
                    let tools = request["tools"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
                        .collect();
                    (text, tools)
                })
                .collect()
        }
    }

    impl CompletionProvider for RecordingProvider {
        // {test}`requirements.authoring.confirmation`
        async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
            let turn = self.requests.borrow().len();
            self.requests
                .borrow_mut()
                .push(serde_json::to_value(&request).unwrap());
            let (name, arguments) = self.turns[turn].clone();
            Ok(CompletionResponse::new(
                vec![AssistantContent::ToolCall(ToolCall::from_wire(
                    format!("call-{turn}"),
                    ToolFunction::new(ToolName::new(name).unwrap(), arguments),
                ))],
                Usage::default(),
                Origin::new("fixture", "fixture", "fixture"),
                Value::Null,
            )
            .with_finish_reason(FinishReason::ToolCalls))
        }
    }

    // {test}`requirements.authoring.confirmation`
    fn judgment(determination: Value, rationale: &str) -> (&'static str, Value) {
        (
            "submit_judgment",
            json!({"determination": determination, "rationale": rationale}),
        )
    }

    // {test}`requirements.authoring.confirmation`
    fn verdict(confirmed: bool, rationale: &str) -> (&'static str, Value) {
        (
            "submit_verification",
            json!({"confirmed": confirmed, "rationale": rationale}),
        )
    }

    /// Runs one subject's assessment with `turns`, returning the outcome, the
    /// provider, and the subjects of the usage records written.
    // {test}`requirements.authoring.confirmation`
    async fn confirmed(
        turns: Vec<(&'static str, Value)>,
        confirm: bool,
    ) -> (io::Result<(Judgment, Value)>, RecordingProvider, Vec<Value>) {
        let project = inputs::tests::project();
        let state = tempfile::tempdir().unwrap();
        let subject = AuthoringSubject::Definition("requirements.ready".into());
        let sources = subject.sources(project.path()).unwrap();
        let mut usage = UsageLog::start(state.path(), project.path(), &subject.label()).unwrap();
        let provider = RecordingProvider::new(turns);
        let result = assess_subject(
            &provider,
            project.path(),
            state.path(),
            &subject,
            sources,
            &mut usage,
            confirm,
        )
        .await;
        usage.finish("completed").unwrap();
        let mut logs = fs::read_dir(state.path().join("authoring/usage"))
            .unwrap()
            .map(|entry| {
                serde_json::from_slice::<Value>(&fs::read(entry.unwrap().path()).unwrap()).unwrap()
            })
            .collect::<Vec<_>>();
        logs.sort_by_key(|log| log["subject"].as_str().unwrap_or_default().to_owned());
        (result, provider, logs)
    }

    // {test}`requirements.authoring.confirmation`
    #[tokio::test]
    async fn without_confirm_or_after_a_false_judgment_one_assessment_is_recorded() {
        for (turns, confirm) in [
            (vec![judgment(json!(true), "Covered.")], false),
            (vec![judgment(json!(false), "A gap.")], true),
        ] {
            let (result, provider, logs) = confirmed(turns.clone(), confirm).await;
            let (_, record) = result.unwrap();
            assert_eq!(record["rationale"], turns[0].1["rationale"]);
            assert_eq!(provider.requests.borrow().len(), 1);
            assert_eq!(logs.len(), 1);
        }
    }

    // {test}`requirements.authoring.confirmation`
    #[tokio::test]
    async fn agreeing_assessments_record_the_first() {
        let (result, provider, logs) = confirmed(
            vec![
                judgment(json!({"FIXME": ["#1"]}), "First."),
                judgment(json!({"FIXME": ["#2"]}), "Second."),
            ],
            true,
        )
        .await;
        let (judgment, record) = result.unwrap();
        assert_eq!(judgment, Judgment::Fixme(vec!["#1".into()]));
        assert_eq!(record["rationale"], "First.");
        // Two independent conversations, each with the initial prompt and its
        // own usage record.
        let prompts = provider.prompts();
        assert_eq!(prompts.len(), 2);
        assert!(
            prompts
                .iter()
                .all(|(text, tools)| text.contains("Bring your expert judgment")
                    && tools.contains(&"submit_judgment".to_owned()))
        );
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0]["subject"], "requirements.ready");
        assert_eq!(logs[1]["subject"], "requirements.ready (second assessment)");
        assert_eq!(logs[1]["outcome"], "completed");
        assert_eq!(logs[1]["determination"], "FIXME");
    }

    // {test}`requirements.authoring.confirmation`
    #[tokio::test]
    async fn disagreeing_assessments_verify_the_less_favorable_one() {
        for (turns, recorded, outcome) in [
            // The second assessment is less favorable and is confirmed.
            (
                vec![
                    judgment(json!(true), "Looks complete."),
                    judgment(json!(false), "The empty case is unhandled."),
                    verdict(true, "The empty case panics."),
                ],
                Judgment::False,
                "confirmed",
            ),
            // The first is less favorable and is not confirmed.
            (
                vec![
                    judgment(json!({"FIXME": ["#7"]}), "Issue 7 is open."),
                    judgment(json!(true), "Complete."),
                    verdict(false, "Issue 7 does not affect this."),
                ],
                Judgment::True,
                "did not confirm",
            ),
        ] {
            let (result, provider, logs) = confirmed(turns.clone(), true).await;
            let (judgment, record) = result.unwrap();
            assert_eq!(judgment, recorded);
            assert_eq!(record["determination"], judgment.to_value());
            let rationale = record["rationale"].as_str().unwrap();
            assert!(
                rationale.contains("the assessments disagreed"),
                "{rationale}"
            );
            assert!(
                rationale.contains(&format!("and {outcome} it")),
                "{rationale}"
            );
            assert!(
                rationale.contains(turns[2].1["rationale"].as_str().unwrap()),
                "{rationale}"
            );

            // The verifier receives the less favorable determination and its
            // rationale, with the source tools and submit_verification only.
            let prompts = provider.prompts();
            assert_eq!(prompts.len(), 3);
            let (less, less_rationale) = if turns[1].1["determination"] == json!(false) {
                ("false", turns[1].1["rationale"].as_str().unwrap())
            } else {
                (
                    "{\\\"FIXME\\\":[\\\"#7\\\"]}",
                    turns[0].1["rationale"].as_str().unwrap(),
                )
            };
            let (verifier, tools) = &prompts[2];
            assert!(
                verifier.contains(&format!(
                    "Another reviewer judged requirement requirements.ready as {less} for these reasons"
                )),
                "{verifier}"
            );
            assert!(verifier.contains(less_rationale), "{verifier}");
            assert!(verifier.contains("Mandatory inputs:"), "{verifier}");
            assert!(tools.contains(&"submit_verification".to_owned()));
            assert!(tools.contains(&"habeas".to_owned()));
            assert!(!tools.contains(&"submit_judgment".to_owned()));

            assert_eq!(logs.len(), 3);
            assert_eq!(logs[2]["subject"], "requirements.ready (verification)");
            assert_eq!(logs[2]["outcome"], "completed");
        }
    }

    // {test}`requirements.authoring.confirmation`
    #[test]
    fn verifications_accept_exactly_a_boolean_and_a_nonempty_rationale() {
        assert_eq!(
            verification(&json!({"confirmed": true, "rationale": "Checked."})).unwrap(),
            (true, "Checked.".to_owned())
        );
        for arguments in [
            json!({"confirmed": "true", "rationale": "x"}),
            json!({"confirmed": true}),
            json!({"confirmed": true, "rationale": " "}),
            json!({"confirmed": true, "rationale": "x", "extra": 1}),
        ] {
            assert!(verification(&arguments).is_err(), "accepted {arguments}");
        }
        let definitions =
            serde_json::to_value(tool_definitions_with("submit_verification").unwrap()).unwrap();
        let submit = definitions
            .as_array()
            .unwrap()
            .iter()
            .find(|definition| definition["name"] == "submit_verification")
            .unwrap();
        assert_eq!(
            submit["parameters"]["properties"],
            json!({"confirmed": {"type": "boolean"}, "rationale": {"type": "string", "minLength": 1}})
        );
    }

    // {test}`requirements.authoring.initial_prompt`
    // {test}`requirements.authoring.confirmation`
    #[test]
    fn placeholders_are_filled_in_one_pass() {
        assert_eq!(
            fill("{{a}} {{b}} {{c}}", &[("a", "{{b}}"), ("b", "2")]),
            "{{b}} 2 {{c}}"
        );
    }

    // {test}`requirements.authoring.citation_prompt`
    // {test}`requirements.references.citation_review`
    // {test}`requirements.authoring.mandatory_inputs`
    #[tokio::test]
    async fn citation_reviews_use_their_prompt_and_record_the_item_and_citations() {
        let project = inputs::tests::project();
        let cite = concat!("{", "cite}`requirements.ready`");
        fs::write(
            project.path().join("src/lib.rs"),
            format!(
                "// {cite}\n// :::{{cite}} habeas:none\n// Formats output for the caller.\n// :::\nfn helper() {{}}\n"
            ),
        )
        .unwrap();
        let subject = AuthoringSubject::parse("citations:src/lib.rs#fn[helper]");
        assert_eq!(
            subject.evidence_path(project.path()),
            project
                .path()
                .join("evidence/citations/src/lib.rs/fn[helper].json")
        );
        let mut sources = subject.sources(project.path()).unwrap();
        let mandatory = sources.mandatory_inputs().to_vec();
        assert_eq!(mandatory[0]["file"], "src/lib.rs");
        assert_eq!(mandatory[0]["item"], "fn[helper]");
        assert_eq!(mandatory[1]["requirements"], "requirements.ready");
        assert_eq!(mandatory[2]["builtin"], "habeas:none");
        assert!(
            mandatory[2]["text"][0]
                .as_str()
                .unwrap()
                .starts_with("Cite this only when")
        );

        let state = tempfile::tempdir().unwrap();
        let mut usage = UsageLog::start(state.path(), project.path(), &subject.label()).unwrap();
        let provider = RecordingProvider::new(vec![judgment(json!(true), "Both are sensible.")]);
        let (_, record) = assess(&provider, &mut sources, &mut usage).await.unwrap();
        let prompts = provider.prompts();
        let prompt = &prompts[0].0;
        assert!(
            prompt.contains("reviewing a source item's citations"),
            "{prompt}"
        );
        assert!(
            prompt.contains("Subject: src/lib.rs fn[helper]"),
            "{prompt}"
        );
        assert!(
            prompt.contains("Formats output for the caller."),
            "{prompt}"
        );
        // The initial request carries the complete mandatory set.
        assert!(
            prompt.contains(mandatory[0]["hash"].as_str().unwrap()),
            "{prompt}"
        );
        assert!(prompt.contains("The project MUST be ready."), "{prompt}");
        assert!(prompt.contains("Cite this only when"), "{prompt}");
        assert!(!prompt.contains("{{"), "{prompt}");

        let citations = json!([
            {"role": "cite", "name": "requirements.ready", "commentary": ""},
            {"role": "cite", "name": "habeas:none", "commentary": "Formats output for the caller."}
        ]);
        assert_eq!(record["file"], "src/lib.rs");
        assert_eq!(record["item"], "fn[helper]");
        assert_eq!(record["citations"], citations);
        assert_eq!(record["determination"], true);
        let inputs = record["inputs"].as_array().unwrap();
        assert!(inputs.iter().all(|input| input.get("builtin").is_none()));
        assert_eq!(inputs.len(), 2, "{inputs:#?}");

        // The recorded review satisfies validation for its item.
        let path = subject.evidence_path(project.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, record.to_string()).unwrap();
        let errors = crate::validate_repository(project.path()).unwrap().errors;
        assert!(
            !errors
                .iter()
                .any(|error| error.starts_with("evidence/citations/")),
            "{errors:#?}"
        );

        // A name without a selector reviews the file's root node.
        assert!(matches!(
            AuthoringSubject::parse("citations:notes.txt"),
            AuthoringSubject::Citation { file, item } if file == "notes.txt" && item.is_empty()
        ));
    }
}

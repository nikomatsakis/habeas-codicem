//! Model-assisted evidence authoring, invoked explicitly and never by validation.

pub(crate) mod access;
mod adaptive;
mod batch;
pub(crate) mod corrections;
mod domain_tools;
pub(crate) mod inputs;
mod line_numbers;
mod progress;
mod sdk;
#[cfg(test)]
pub(crate) mod test_backend;
mod usage;

pub use batch::{BatchOutcome, author_all, author_all_with_options, author_sites_with_options};

use serde_json::{Value, json};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use tokio::sync::watch;

use crate::DefinitionKind;
use crate::config::{self, AssessmentPolicy, BackendKind, ResolvedAuthoringConfig};
use crate::judgment::Judgment;
use inputs::{AssessmentKind, AssessmentSources};

use usage::UsageLog;

pub struct AuthoringOutcome {
    pub subject: String,
    pub backend: String,
    pub model: String,
    pub evidence_path: Option<PathBuf>,
    pub usage_path: PathBuf,
    pub determination: Judgment,
    pub rationale: String,
    /// The assessment policy the subject was assessed under.
    pub policy: AssessmentPolicy,
}

/// The opening of the note a recorded rationale ends with when its
/// assessments disagreed under the `confirm` policy.
pub(crate) const DISAGREEMENT_NOTE: &str = "**Confirmation:** the assessments disagreed";

#[derive(Clone, Copy, Debug, Default)]
pub struct AuthoringOptions {
    pub verbose: bool,
    /// `--policy <name>`, overriding the configuration's `policy`.
    pub policy: Option<AssessmentPolicy>,
    /// `--force`: assess every selected subject, including current ones.
    /// Otherwise (`--missing`), only subjects whose evidence is missing,
    /// stale, or invalid are assessed.
    pub force: bool,
}

// The `Definition` variant is the one requirement or named example given on the
// command line: the subject the command assesses and reports.
#[derive(Clone, Debug)]
pub(crate) enum AuthoringSubject {
    Definition(String),
    /// The citation review of a source item or opted-in book passage.
    Citation {
        file: String,
        item: String,
    },
    /// The review of one site of a distributed requirement.
    Site {
        requirement: String,
        file: String,
        item: String,
    },
}

impl AuthoringSubject {
    /// Parses a single-subject name: a full requirement or example name, or
    /// `citations:<file>#<selector>` for a citation review, where an omitted
    /// or empty selector names the file's root node.
    /// `sites:<full-name>/<file>#<selector>` names a site review, where an
    /// omitted selector names the file's root node.
    pub(crate) fn parse(subject: &str) -> Self {
        if let Some(rest) = subject.strip_prefix("sites:") {
            let (requirement, path) = rest.split_once('/').unwrap_or((rest, ""));
            let (file, item) = path.split_once('#').unwrap_or((path, ""));
            return Self::Site {
                requirement: requirement.to_owned(),
                file: file.to_owned(),
                item: item.to_owned(),
            };
        }
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

    pub(crate) fn label(&self) -> String {
        match self {
            Self::Definition(name) => name.clone(),
            Self::Citation { file, item } => crate::citations::subject_label(file, item),
            Self::Site {
                requirement,
                file,
                item,
            } => crate::sites::subject_label(requirement, file, item),
        }
    }

    /// Where the subject's record is written.
    pub(crate) fn evidence_path(&self, root: &Path) -> PathBuf {
        match self {
            Self::Definition(name) => root.join("evidence").join(format!("{name}.json")),
            Self::Citation { file, item } => root.join(crate::citations::review_path(file, item)),
            Self::Site {
                requirement,
                file,
                item,
            } => root.join(crate::sites::review_path(requirement, file, item)),
        }
    }

    /// The subject kind recorded in usage, as `AssessmentKind::noun` names
    /// it, known before configuration and sources are prepared so that an
    /// early failure still records it. `None` when no requirement or example
    /// has the name, or the book cannot be read.
    fn kind_noun(&self, root: &Path) -> Option<&'static str> {
        match self {
            Self::Definition(name) => crate::evidence::load_definitions(root)
                .ok()?
                .get(name)
                .filter(|definition| definition.kind != DefinitionKind::Term)
                .map(|definition| definition.kind.noun()),
            Self::Citation { .. } => Some("citation review"),
            Self::Site { .. } => Some("site review"),
        }
    }

    /// Prepares the subject's sources and mandatory inputs.
    #[cfg(test)]
    fn sources(&self, root: &Path) -> io::Result<AssessmentSources> {
        self.domain_sources(crate::sdk::HabeasDomain::discover(root)?)
    }

    fn domain_sources(&self, domain: crate::sdk::HabeasDomain) -> io::Result<AssessmentSources> {
        match self {
            Self::Definition(name) => {
                let github = crate::github::GithubIssues::from_env(&domain.root);
                AssessmentSources::prepare_with_context(domain, name, github)
            }
            Self::Citation { file, item } => {
                AssessmentSources::prepare_citation(domain, file, item)
            }
            Self::Site {
                requirement,
                file,
                item,
            } => AssessmentSources::prepare_site(domain, requirement, file, item),
        }
    }
}

// {cite}`requirements.authoring.persistence`
pub(crate) struct PendingAssessment {
    pub(crate) subject: AuthoringSubject,
    pub(crate) record: Value,
    outcome: AuthoringOutcome,
    findings: Vec<corrections::Finding>,
    usage: UsageLog,
}

/// Keep the selected conversation's authorization to edit separate from the
/// record. Confirmation can merge the other assessor's evidence dependencies.
#[derive(Debug)]
struct Assessment {
    determination: Judgment,
    record: Value,
    findings: Vec<corrections::Finding>,
}

impl PendingAssessment {
    fn persisted(
        mut self,
        path: PathBuf,
    ) -> io::Result<(AuthoringOutcome, Vec<corrections::Finding>)> {
        self.usage.record.evidence_written = true;
        self.outcome.evidence_path = Some(path);
        self.usage.finish("completed").map_err(|error| {
            io::Error::other(format!(
                "cannot record usage at {} (evidence written: true): {error}",
                self.usage.path.display()
            ))
        })?;
        Ok((self.outcome, self.findings))
    }

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

#[derive(Clone, Copy)]
pub enum EvidenceOutput {
    File,
    Stdout,
}

/// Authors one subject's evidence file under `--missing`, returning `None`
/// when its evidence is current.
pub async fn author_evidence(
    root: &Path,
    subject: &str,
    selection: Option<&str>,
) -> io::Result<Option<AuthoringOutcome>> {
    author_evidence_with_output(root, subject, selection, EvidenceOutput::File).await
}

pub async fn author_evidence_with_output(
    root: &Path,
    subject: &str,
    selection: Option<&str>,
    output: EvidenceOutput,
) -> io::Result<Option<AuthoringOutcome>> {
    author_evidence_with_options(
        root,
        subject,
        selection,
        output,
        AuthoringOptions::default(),
    )
    .await
}

/// Assesses one subject under its assessment policy, returning `None`
/// without resolving configuration or invoking a model when, under
/// `--missing`, its evidence is current. `--stdout` implies `--force`.
pub async fn author_evidence_with_options(
    root: &Path,
    subject: &str,
    selection: Option<&str>,
    output: EvidenceOutput,
    options: AuthoringOptions,
) -> io::Result<Option<AuthoringOutcome>> {
    let root = fs::canonicalize(root)?;
    let force = options.force || matches!(output, EvidenceOutput::Stdout);
    if !force && batch::subject_is_current(&root, &AuthoringSubject::parse(subject))? {
        let subject = AuthoringSubject::parse(subject);
        crate::evidence_format::convert_file(
            &subject.evidence_path(&root),
            &crate::sdk::HabeasNode::review_subject(&subject),
        )?;
        return Ok(None);
    }
    let state = usage_state_root(&root)?;
    let mut usage = UsageLog::start(&state, &root, subject)?;
    usage.record.subject_kind = AuthoringSubject::parse(subject)
        .kind_noun(&root)
        .map(str::to_owned);
    usage.record.evidence_output = match output {
        EvidenceOutput::File => "file",
        EvidenceOutput::Stdout => "stdout",
    };
    usage.save()?;
    let result = tokio::select! {
        result = configured_assessment(&root, subject, selection, output, options.policy, &mut usage) => result,
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
    let (assessment, findings, config) = result.map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("{error}; usage: {}", usage.path.display()),
        )
    })?;
    if assessment.determination == Judgment::False {
        eprintln!("{subject}: false");
        if options.verbose {
            eprintln!("{}", assessment.rationale);
        }
    }
    if findings.is_empty() || matches!(output, EvidenceOutput::Stdout) {
        for finding in &findings {
            eprintln!("{subject}: proposed correction: {}", finding.description());
        }
        return Ok(Some(assessment));
    }
    let policy = assessment.policy;
    let mut batch = BatchOutcome {
        completed: vec![assessment],
        failures: Vec::new(),
        unchanged: 0,
        deleted: Vec::new(),
        policy: Some(policy),
        cancelled: false,
    };
    let round = [(subject.to_owned(), findings)].into();
    let mut progress = progress::BatchProgress::new(options.verbose, false);
    batch::finish_corrections(&root, &config, policy, &mut progress, &mut batch, round).await?;
    if batch.cancelled {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "evidence authoring cancelled",
        ));
    }
    if !batch.failures.is_empty() {
        return Err(io::Error::other(
            batch
                .failures
                .into_iter()
                .map(|(label, reason)| format!("{label}: {reason}"))
                .collect::<Vec<_>>()
                .join("\n"),
        ));
    }
    Ok(batch.completed.into_iter().find(|a| a.subject == subject))
}

/// Resolves the usage state root, refusing a location inside the project
/// before any state directory is created.
fn usage_state_root(root: &Path) -> io::Result<PathBuf> {
    let state = config::state_root()?;
    if config::state_in_project(&state, root)? {
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

pub(crate) async fn prepare_assessment(
    domain: crate::sdk::HabeasDomain,
    subject: &AuthoringSubject,
    config: &ResolvedAuthoringConfig,
    policy: AssessmentPolicy,
    mut cancellation: watch::Receiver<Option<&'static str>>,
    first_conversation_finished: Option<Box<dyn FnOnce() + Send>>,
) -> io::Result<PendingAssessment> {
    let root = domain.root.clone();
    let state = usage_state_root(&root)?;
    let mut usage = UsageLog::start(&state, &root, &subject.label())?;
    usage.record.subject_kind = match subject {
        AuthoringSubject::Definition(name) => domain.definitions.get(name).map(|d| d.kind.noun()),
        AuthoringSubject::Citation { .. } => Some("citation review"),
        AuthoringSubject::Site { .. } => Some("site review"),
    }
    .map(str::to_owned);
    usage.save()?;
    if let Some(finished) = first_conversation_finished {
        usage.on_first_conversation(finished);
    }
    let result = tokio::select! {
        biased;
        _ = cancellation.changed() => {
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                cancellation.borrow().unwrap_or("evidence authoring cancelled"),
            ))
        }
        result = configured_record(domain, subject, config, policy, &mut usage) => result,
        signal = tokio::signal::ctrl_c() => {
            match signal {
                Ok(()) => Err(io::Error::new(io::ErrorKind::Interrupted, "evidence authoring cancelled")),
                Err(error) => Err(error),
            }
        }
    };
    match result {
        Ok(Assessment {
            determination,
            record,
            findings,
        }) => {
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
                policy,
            };
            Ok(PendingAssessment {
                subject: subject.clone(),
                record,
                outcome,
                findings,
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

async fn configured_assessment(
    root: &Path,
    subject: &str,
    selection: Option<&str>,
    output: EvidenceOutput,
    policy: Option<AssessmentPolicy>,
    usage: &mut UsageLog,
) -> io::Result<(
    AuthoringOutcome,
    Vec<corrections::Finding>,
    ResolvedAuthoringConfig,
)> {
    let config = config::resolve_authoring(root, selection)?;
    let policy = config.effective_policy(policy);
    let parsed = AuthoringSubject::parse(subject);
    let Assessment {
        determination,
        record: evidence,
        findings,
    } = configured_record(
        crate::sdk::HabeasDomain::discover(root)?,
        &parsed,
        &config,
        policy,
        usage,
    )
    .await?;
    let json = format!("{}\n", serde_json::to_string_pretty(&evidence)?);
    let evidence_path = match output {
        EvidenceOutput::File => {
            let path = parsed.evidence_path(root);
            crate::evidence_format::write_atomic(&path, &evidence)?;
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
    Ok((
        AuthoringOutcome {
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
            policy,
        },
        findings,
        config,
    ))
}

async fn configured_record(
    domain: crate::sdk::HabeasDomain,
    subject: &AuthoringSubject,
    config: &ResolvedAuthoringConfig,
    policy: AssessmentPolicy,
    usage: &mut UsageLog,
) -> io::Result<Assessment> {
    let model = config.model_ref();
    let backend = backend_name(&model);
    usage.record.backend = Some(backend.into());
    usage.record.model = Some(model.model.clone());
    usage.record.policy = Some(policy.name());
    // Mandatory inputs are prepared, and an unreadable one reported, before
    // connecting to the provider.
    let project_root = domain.root.clone();
    let root = project_root.as_ref();
    let sources = subject.domain_sources(domain)?;
    if matches!(&config.backend, config::BackendConfig::Openai(openai) if openai.model() == "adaptive")
    {
        return adaptive::assess(root, subject, config, sources, usage, policy).await;
    }
    let state = usage_state_root(root)?;
    assess_subject(
        &config.backend,
        root,
        &state,
        subject,
        sources,
        usage,
        policy,
    )
    .await
}

/// The backend name usage records give a model's backend.
fn backend_name(model: &config::ModelRef) -> &'static str {
    match model.backend {
        BackendKind::Bedrock => "bedrock",
        BackendKind::Ollama => "ollama",
        BackendKind::Openai => "openai",
    }
}

/// Assesses a subject and, under the `confirm` policy, confirms a `true` or FIXME result
/// in an independent second conversation. Each conversation has its own usage
/// record; `usage` is the first's.
async fn assess_subject(
    provider: &config::BackendConfig,
    root: &Path,
    state: &Path,
    subject: &AuthoringSubject,
    mut sources: AssessmentSources,
    usage: &mut UsageLog,
    policy: AssessmentPolicy,
) -> io::Result<Assessment> {
    usage.record.subject_kind = Some(sources.subject_kind.noun().into());
    usage.save()?;
    let first = assess(provider, &mut sources, usage).await?;
    // Under `once`, the single assessment is recorded.
    if policy == AssessmentPolicy::Once || first.determination == Judgment::False {
        return Ok(first);
    }
    let mut log = followup_log(state, root, subject, usage, "second assessment")?;
    let result = match followup_sources(&sources.domain, subject, &mut log) {
        Ok(mut sources) => assess(provider, &mut sources, &mut log).await,
        Err(error) => Err(error),
    };
    let second = finish_followup(log, "second assessment", result)?;
    let chosen = select_assessment(first, second);
    usage.record.determination = Some(chosen.determination.kind().into());
    usage.save()?;
    Ok(chosen)
}

// {cite}`habeas:util`
fn select_assessment(first: Assessment, second: Assessment) -> Assessment {
    let description = |Assessment {
                           determination: judgment,
                           record,
                           ..
                       }: &Assessment| {
        let applies = record["applies"]
            .as_bool()
            .map(|a| format!(" (applies {a})"))
            .unwrap_or_default();
        format!(
            "`{}`{applies}: {}",
            judgment.kind(),
            record["rationale"].as_str().unwrap_or_default()
        )
    };
    let disagree = favorability(&first) != favorability(&second);
    let report = disagree.then(|| {
        format!(
            "{DISAGREEMENT_NOTE}\n\nFirst assessment {}\n\nSecond assessment {}",
            description(&first),
            description(&second)
        )
    });
    let (mut chosen, other, ordinal) = if favorability(&second) < favorability(&first) {
        (second, first, "second")
    } else {
        (first, second, "first")
    };
    if let Some(report) = report {
        chosen.record["rationale"] = json!(format!(
            "{report}\n\nRecorded the {ordinal} assessment (`{}`).",
            chosen.determination.kind()
        ));
    }
    for field in ["inputs", "dependencies"] {
        if let (Some(all), Some(extra)) = (
            chosen.record[field].as_array_mut(),
            other.record[field].as_array(),
        ) {
            for input in extra {
                if !all.contains(input) {
                    all.push(input.clone());
                }
            }
        }
    }
    chosen
}

/// Orders assessments: `false` is less favorable than FIXME, and FIXME
/// than `true`; between site reviews with the same determination, the one
/// with `applies` true is less favorable.
fn favorability(assessment: &Assessment) -> (u8, bool) {
    let kind = match assessment.determination {
        Judgment::False => 0,
        Judgment::Fixme(_) => 1,
        Judgment::True => 2,
    };
    (kind, assessment.record["applies"] == Value::Bool(false))
}

/// Starts the usage record of a further conversation about a subject.
fn followup_log(
    state: &Path,
    root: &Path,
    subject: &AuthoringSubject,
    first: &UsageLog,
    purpose: &str,
) -> io::Result<UsageLog> {
    let mut usage = UsageLog::start(state, root, &format!("{} ({purpose})", subject.label()))?;
    usage.record.subject_kind = first.record.subject_kind.clone();
    usage.record.backend = first.record.backend.clone();
    usage.record.model = first.record.model.clone();
    usage.record.policy = first.record.policy;
    usage.record.evidence_output = first.record.evidence_output;
    Ok(usage)
}

// {cite}`habeas:util`
/// Fresh sources for an independent conversation, recorded in its usage.
fn followup_sources(
    domain: &crate::sdk::HabeasDomain,
    subject: &AuthoringSubject,
    usage: &mut UsageLog,
) -> io::Result<AssessmentSources> {
    let sources = subject.domain_sources(domain.clone())?;
    usage.record.subject_kind = Some(sources.subject_kind.noun().into());
    usage.save()?;
    Ok(sources)
}

// {cite}`habeas:util`
/// Finishes a further conversation's usage record with its outcome.
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

/// The `index`th text block of approved requirement `name` in the authoring
/// chapter, which is the sole source of these instructions.
fn approved_text(name: &str, index: usize) -> &'static str {
    let chapter = include_str!("../../md/requirements/authoring.md");
    let opening = format!(":::{{req}} {name}\n");
    let start = chapter.find(&opening).expect("approved requirement") + opening.len();
    let body = &chapter[start..];
    let body = &body[..body.find("\n:::\n").expect("requirement closing fence")];
    body.split("```text\n")
        .nth(index + 1)
        .and_then(|block| block.split("\n```").next())
        .expect("approved text block")
}

/// The shared blocks every assessment prompt fills: Habeas's terms, the
/// reference roles, and the judgment rules.
fn shared_blocks() -> [(&'static str, &'static str); 3] {
    [
        ("habeas_background", approved_text("habeas_background", 0)),
        ("reference_roles", approved_text("habeas_background", 1)),
        ("judgment_rules", approved_text("judgment_rules", 0)),
    ]
}

/// The judgment rules' guidance on citing source locations, which a
/// rejection for citing line numbers repeats.
fn location_guidance() -> &'static str {
    let rules = approved_text("judgment_rules", 0);
    let start = rules
        .find("- Give a source location")
        .expect("location guidance");
    let rest = &rules[start..];
    rest.find("\n- ").map_or(rest, |end| &rest[..end])
}

/// The account of distributed requirements that the site prompt, the
/// distributed task receive.
fn distributed_background() -> &'static str {
    approved_text("habeas_background", 2)
}

/// Replaces `{{name}}` placeholders in one pass, so substituted text that
/// itself contains a placeholder, such as a requirement quoting a template,
/// is left as written.
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

/// The task text of a requirement or example review.
fn subject_task(kind: &DefinitionKind) -> String {
    match kind {
        DefinitionKind::Requirement => format!(
            "{}\n{}",
            approved_text("requirement_task", 0),
            approved_text("testing_bar", 0)
        ),
        // Terms are never assessment subjects.
        DefinitionKind::Example | DefinitionKind::Term => {
            approved_text("example_task", 0).to_owned()
        }
    }
}

/// The filled distributed task a distributed requirement's subject message
/// carries, or nothing for any other subject.
fn distributed_task(kind: &DefinitionKind, distributed: bool) -> String {
    if *kind == DefinitionKind::Requirement && distributed {
        fill(
            approved_text("distributed_task", 0),
            &[("distributed_background", distributed_background())],
        )
    } else {
        String::new()
    }
}

/// A conversation's opening: the system prompt, the same for every
/// assessment of its kind, and the subject message that follows it.
// {cite}`habeas:util`
#[derive(Clone, Debug, PartialEq, Eq)]
struct Prompt {
    system: String,
    subject: String,
}

impl Prompt {
    /// Fills the system and subject blocks of approved prompt `name`.
    fn filled(name: &str, values: &[(&str, &str)]) -> Self {
        Self {
            system: fill(approved_text(name, 0), values),
            subject: fill(approved_text(name, 1), values),
        }
    }

    /// Both parts, for tests that look for text wherever it is sent.
    // {test}`requirements.authoring.prompt_parts`
    #[cfg(test)]
    fn joined(&self) -> String {
        format!("{}\n\n{}", self.system, self.subject)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReviewKind {
    Standard,
    Structural,
    Behavioral,
}

impl ReviewKind {
    fn name(self) -> &'static str {
        match self {
            Self::Standard => "review",
            Self::Structural => "structural",
            Self::Behavioral => "behavioral",
        }
    }
}

#[cfg(test)]
fn initial_prompt(sources: &AssessmentSources) -> io::Result<Prompt> {
    let kind = if sources.review_roots.is_some() {
        ReviewKind::Structural
    } else {
        ReviewKind::Standard
    };
    criterion_prompt(sources, kind)
}

#[cfg(test)]
fn criterion_prompt(sources: &AssessmentSources, kind: ReviewKind) -> io::Result<Prompt> {
    initial_prompt_with_inputs(sources, kind, &sources.inputs_for(kind)?)
}

// {impl}`requirements.authoring.prompt_parts`
fn initial_prompt_with_inputs(
    sources: &AssessmentSources,
    review: ReviewKind,
    inputs: &[Value],
) -> io::Result<Prompt> {
    let mandatory = serde_json::to_string_pretty(inputs)?;
    let mut values = shared_blocks().to_vec();
    values.push(("mandatory_inputs", &mandatory));
    Ok(match &sources.subject_kind {
        AssessmentKind::Definition(kind, distributed) => {
            let acknowledgments = serde_json::to_string_pretty(&sources.model_acknowledgments())?;
            let mut task = subject_task(kind);
            if review == ReviewKind::Behavioral {
                task = format!("{}\n\n{task}", approved_text("behavioral_review", 0));
            }
            let distributed = distributed_task(kind, *distributed);
            values.extend([
                ("subject_kind", sources.subject_kind.noun()),
                ("subject_name", &sources.subject_name),
                ("subject_task", &task),
                ("distributed_task", &distributed),
                ("acknowledgments", &acknowledgments),
            ]);
            if review == ReviewKind::Structural {
                let roots = serde_json::to_string_pretty(
                    &sources
                        .review_roots
                        .as_ref()
                        .expect("requirement roots")
                        .implementation,
                )?;
                values.push(("review_roots", &roots));
                Prompt::filled("structural_prompt", &values)
            } else {
                Prompt::filled("initial_prompt", &values)
            }
        }
        AssessmentKind::Citation(item) => {
            let citations = serde_json::to_string(&item.model_citations_value())?;
            values.extend([
                ("review_kind", item.review_kind.label()),
                ("item_file", item.file.as_str()),
                ("item_selector", item.selector_label()),
                ("citations", &citations),
            ]);
            Prompt::filled("citation_prompt", &values)
        }
        AssessmentKind::Site(site, _) => {
            let matches = serde_json::to_string(&site.matches_value())?;
            values.extend([
                ("requirement_name", site.requirement.as_str()),
                ("site_file", site.file.as_str()),
                ("site_selector", site.selector_label()),
                ("matches", &matches),
                ("distributed_background", distributed_background()),
            ]);
            Prompt::filled("site_prompt", &values)
        }
    })
}

fn submission(arguments: &Value) -> io::Result<(Judgment, String)> {
    let object = arguments
        .as_object()
        .ok_or_else(|| io::Error::other("submit_result expects an object"))?;
    if object.len() != 2
        || !object.contains_key("determination")
        || !object.contains_key("rationale")
    {
        return Err(io::Error::other(
            "submit_result requires exactly determination and rationale",
        ));
    }
    let judgment = Judgment::from_value(&object["determination"]).map_err(io::Error::other)?;
    let rationale = object["rationale"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| io::Error::other("submit_result rationale must be nonempty Markdown"))?;
    Ok((judgment, rationale.to_owned()))
}

/// A submitted site review: its judgment, rationale, and `applies`.
/// `applies` false requires a `true` determination.
fn site_submission(arguments: &Value) -> io::Result<(Judgment, String, bool)> {
    let object = arguments
        .as_object()
        .ok_or_else(|| io::Error::other("submit_result expects an object"))?;
    if object.len() != 3 || !object.contains_key("applies") {
        return Err(io::Error::other(
            "submit_result requires exactly applies, determination, and rationale",
        ));
    }
    let applies = object["applies"]
        .as_bool()
        .ok_or_else(|| io::Error::other("submit_result applies must be a boolean"))?;
    let mut rest = object.clone();
    rest.remove("applies");
    let (judgment, rationale) = submission(&Value::Object(rest))?;
    if !applies && judgment != Judgment::True {
        return Err(io::Error::other(
            "submit_result determination must be true when applies is false",
        ));
    }
    Ok((judgment, rationale, applies))
}

fn record(
    sources: &AssessmentSources,
    judgment: &Judgment,
    rationale: &str,
    applies: Option<bool>,
) -> Value {
    match &sources.subject_kind {
        AssessmentKind::Site(site, fixmes) => json!({"requirement": site.requirement,
            "file": site.file, "item": site.item, "queries": site.queries(),
            "applies": applies.unwrap_or(true),
            "fixmes": fixmes.iter().map(crate::sites::SiteFixme::to_value).collect::<Vec<_>>(),
            "determination": judgment.to_value(), "rationale": rationale,
            "inputs": sources.recorded_inputs()}),
        AssessmentKind::Definition(kind, _) => json!({kind.noun(): sources.subject_text,
            "determination": judgment.to_value(), "rationale": rationale,
            "references": sources.reference_set, "inputs": sources.recorded_inputs()}),
        AssessmentKind::Citation(item) => json!({"file": item.file, "item": item.item,
            "citations": item.citations_value(), "determination": judgment.to_value(),
            "rationale": rationale, "inputs": sources.recorded_inputs()}),
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    // {test}`requirements.authoring.refresh`
    #[tokio::test]
    async fn default_authoring_apis_preserve_current_records_without_configuration() {
        let project = inputs::tests::project();
        let state = tempfile::tempdir().unwrap();
        let subject = "requirements.ready";
        let mut sources = AssessmentSources::prepare(project.path(), subject).unwrap();
        let mut usage = UsageLog::start(state.path(), project.path(), subject).unwrap();
        let Assessment { record, .. } = assess(
            &RecordingProvider::new(vec![judgment(json!(true), "Covered.")]),
            &mut sources,
            &mut usage,
        )
        .await
        .unwrap();
        let path = project.path().join("evidence/requirements.ready.json");
        let bytes = serde_json::to_vec_pretty(&record).unwrap();
        fs::create_dir(path.parent().unwrap()).unwrap();
        fs::write(&path, &bytes).unwrap();
        let before_usage = fs::read(&usage.path).unwrap();
        // An invalid selection would fail if any API resolved model configuration.
        let selection = Some("unconfigured-fixture-backend");
        let outcome = author_all(project.path(), selection).await.unwrap();
        assert_eq!(outcome.unchanged, 1);
        assert!(outcome.completed.is_empty());
        assert!(outcome.failures.is_empty());
        assert!(outcome.deleted.is_empty());
        assert!(outcome.policy.is_none());
        assert!(!outcome.cancelled);
        assert!(
            author_evidence(project.path(), subject, selection)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            author_evidence_with_output(project.path(), subject, selection, EvidenceOutput::File)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            fs::read(path).unwrap(),
            bytes,
            "current evidence is preserved byte for byte"
        );
        assert_eq!(fs::read(&usage.path).unwrap(), before_usage);
        assert_eq!(
            fs::read_dir(state.path().join("authoring/usage"))
                .unwrap()
                .count(),
            1,
            "skipped assessments start no usage records"
        );
    }

    // {test}`requirements.authoring.usage_storage`
    #[test]
    fn failed_persistence_records_failure_or_cancellation_and_retains_the_cause() {
        let project = inputs::tests::project();
        let state = tempfile::tempdir().unwrap();
        for (kind, expected, block_usage) in [
            (io::ErrorKind::PermissionDenied, "failed", false),
            (io::ErrorKind::Interrupted, "cancelled", false),
            (io::ErrorKind::PermissionDenied, "failed", true),
            (io::ErrorKind::Interrupted, "cancelled", true),
        ] {
            let subject = AuthoringSubject::Definition("requirements.ready".into());
            let usage = UsageLog::start(state.path(), project.path(), &subject.label()).unwrap();
            let path = usage.path.clone();
            let pending = PendingAssessment {
                subject,
                record: json!({}),
                findings: Vec::new(),
                outcome: AuthoringOutcome {
                    subject: "requirements.ready".into(),
                    backend: "fixture".into(),
                    model: "fixture".into(),
                    evidence_path: None,
                    usage_path: path.clone(),
                    determination: Judgment::True,
                    rationale: "Fixture assessment.".into(),
                    policy: AssessmentPolicy::Once,
                },
                usage,
            };
            if block_usage {
                fs::remove_file(&path).unwrap();
                fs::create_dir(&path).unwrap();
            }
            let error = pending.failed(io::Error::new(kind, "cannot persist fixture evidence"));
            assert_eq!(error.kind(), kind, "retain the original failure kind");
            let message = error.to_string();
            assert!(
                message.contains("cannot persist fixture evidence"),
                "{message}"
            );
            assert!(message.contains(path.to_str().unwrap()), "{message}");
            if block_usage {
                assert!(message.contains("cannot record failed usage"), "{message}");
                assert!(
                    path.is_dir(),
                    "failed usage writes do not replace the obstruction"
                );
            } else {
                let record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                assert_eq!(record["outcome"], expected);
                assert_eq!(record["evidence_written"], false);
                assert!(!record["end_time_ms"].is_null());
                assert!(record["determination"].is_null());
                assert!(!message.contains("cannot record failed usage"));
            }
            assert!(
                !project
                    .path()
                    .join("evidence/requirements.ready.json")
                    .exists()
            );
        }
    }

    // {test}`requirements.ci.review_criteria`
    // {test}`requirements.authoring.requirement_task`
    // {test}`requirements.authoring.testing_bar`
    // {test}`requirements.authoring.behavioral_review`
    // {test}`requirements.authoring.example_task`
    // {test}`requirements.authoring.judgment_rules`
    // {test}`requirements.authoring.habeas_background`
    // {test}`requirements.examples.evidence`
    // {test}`requirements.examples.definition`
    #[test]
    fn initial_prompt_carries_the_review_criteria() {
        let project = inputs::tests::project();
        let sources = AssessmentSources::prepare(project.path(), "requirements.ready").unwrap();
        let prompt = criterion_prompt(&sources, ReviewKind::Behavioral)
            .unwrap()
            .joined();
        for criterion in [
            "Subject: requirement requirements.ready\nJudge against: the subject's text.",
            approved_text("habeas_background", 0),
            approved_text("habeas_background", 1),
            approved_text("judgment_rules", 0),
            approved_text("requirement_task", 0),
            approved_text("testing_bar", 0),
            "Derive testable obligations and expected outcomes from each MUST, MUST NOT,",
            "Testing criteria:",
            "The project MUST be ready.",
        ] {
            assert!(
                prompt.contains(criterion),
                "missing {criterion:?} in:\n{prompt}"
            );
        }
        assert!(prompt.contains(&format!(
            "{}\n\n{}\n{}",
            approved_text("behavioral_review", 0),
            approved_text("requirement_task", 0),
            approved_text("testing_bar", 0)
        )));
        assert!(!prompt.contains(approved_text("example_task", 0)));
        assert!(!prompt.contains("This subject is a distributed requirement."));
        assert!(!prompt.contains(distributed_background()));
        assert!(!prompt.contains("{{"), "{prompt}");

        // An example's task replaces the requirement task.
        let requirements = project.path().join("md/requirements.md");
        let mut book = fs::read_to_string(&requirements).unwrap();
        let cite = "{cite}`requirements.ready`";
        book.push_str(&format!(
            "\n:::{{example}} case\nA ready case. {cite}\n:::\n"
        ));
        fs::write(&requirements, book).unwrap();
        let sources = AssessmentSources::prepare(project.path(), "requirements.case").unwrap();
        let prompt = initial_prompt(&sources).unwrap().joined();
        assert!(
            prompt.contains("Subject: example requirements.case"),
            "{prompt}"
        );
        assert!(
            prompt.contains(approved_text("example_task", 0)),
            "{prompt}"
        );
        assert!(
            prompt.contains("First check that it describes a concrete case\nwith expected behavior; a missing concrete case or expected behavior is a gap."),
            "{prompt}"
        );
        assert!(
            !prompt.contains(approved_text("requirement_task", 0)),
            "{prompt}"
        );
        assert!(
            !prompt.contains(approved_text("testing_bar", 0)),
            "{prompt}"
        );
        assert!(!prompt.contains("Testing criteria:"), "{prompt}");
        assert!(!prompt.contains("{{"), "{prompt}");
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
    fn initial_prompt_conveys_when_to_choose_each_outcome() {
        let project = inputs::tests::project();
        let sources = AssessmentSources::prepare(project.path(), "requirements.ready").unwrap();
        let prompt = initial_prompt(&sources).unwrap().joined();
        for guidance in [
            "Submit Success when you find no gap",
            "A gap is acknowledged only by a {fixme} reference",
            "a {fixme} naming a\n  closed issue no longer acknowledges its gap",
            "Include any acknowledged gaps",
            "A {fixme} describing a gap that your review establishes\n  no longer exists within its task's scope is itself a gap.",
            "A plain FIXME or TODO comment is\n  not an acknowledgment.",
            "An acknowledgment of an implementation bug alone does not establish a\n  test-adequacy gap or acknowledge a different missing-test or assertion gap.",
            "Do not inspect implementation correctness merely to validate an out-of-scope\n  acknowledgment.",
        ] {
            assert!(
                prompt.contains(guidance),
                "missing {guidance:?} in:\n{prompt}"
            );
        }
    }

    use super::test_backend::RecordingProvider;

    // {cite}`habeas:util`
    fn prompt_section<'a>(prompt: &'a str, heading: &str) -> &'a str {
        let section = prompt.split_once(&format!("# {heading}\n\n")).unwrap().1;
        section
            .split_once("\n\n# ")
            .map_or(section, |(body, _)| body)
    }

    fn judgment(determination: Value, rationale: &str) -> (&'static str, Value) {
        let result = match determination {
            Value::Bool(true) => json!({"status":"success", "rationale":rationale}),
            Value::Bool(false) => json!({"status":"failed", "corrections":[
                {"kind":"manual", "instruction":"Repair the gap.", "rationale":rationale}
            ]}),
            fixme => {
                json!({"status":"acknowledged_gap", "entries":fixme["FIXME"], "rationale":rationale})
            }
        };
        ("submit_result", json!({"result":result}))
    }

    // {test}`requirements.authoring.confirmation.selection`
    // {test}`requirements.authoring.confirmation.disagreement`
    #[test]
    fn site_confirmation_selects_the_applicable_judgment_in_either_order() {
        for first_applies in [true, false] {
            let assessment = |applies| Assessment {
                determination: Judgment::True,
                record: json!({"applies":applies,"rationale": if applies {"The site complies."} else {"Outside the requirement."}}),
                findings: vec![],
            };
            let result =
                select_assessment(assessment(first_applies), assessment(!first_applies)).record;
            assert_eq!(result["applies"], true);
            for explanation in [
                "The site complies.",
                "Outside the requirement.",
                if first_applies {
                    "Recorded the first"
                } else {
                    "Recorded the second"
                },
            ] {
                assert!(result["rationale"].as_str().unwrap().contains(explanation));
            }
        }
    }

    // {test}`requirements.authoring.confirmation.disagreement`
    // {test}`requirements.authoring.confirmation.selection`
    #[test]
    fn confirmation_orders_all_judgments_and_preserves_the_selected_payload() {
        fn assessment(rank: usize, author: &str, applies: Option<bool>) -> Assessment {
            let determination = match rank {
                0 => Judgment::False,
                1 => Judgment::Fixme(vec![format!("{author}'s acknowledged gap")]),
                2 => Judgment::True,
                _ => unreachable!(),
            };
            let mut record = json!({"determination": determination.to_value(), "rationale":author});
            if let Some(applies) = applies {
                record["applies"] = json!(applies);
            }
            let mut findings = vec![];
            if determination == Judgment::False {
                findings.push(corrections::Finding::capture(
                    corrections::Correction::Manual {
                        instruction: format!("{author}'s repair"),
                        rationale: "Missing assertion".into(),
                    },
                    &[],
                ));
            }
            Assessment {
                determination,
                record,
                findings,
            }
        }
        for (left, right) in [
            (None, None),
            (Some(false), Some(false)),
            (Some(true), Some(true)),
            (Some(false), Some(true)),
            (Some(true), Some(false)),
        ] {
            for first in 0..3 {
                for second in 0..3 {
                    let chosen = select_assessment(
                        assessment(first, "first", left),
                        assessment(second, "second", right),
                    );
                    let (author, applies) = if first < second || (first == second && left >= right)
                    {
                        ("first", left)
                    } else {
                        ("second", right)
                    };
                    let expected = assessment(first.min(second), author, applies);
                    assert_eq!(chosen.determination, expected.determination);
                    assert_eq!(
                        chosen.record["determination"],
                        expected.record["determination"]
                    );
                    let explanation = chosen.record["rationale"].as_str().unwrap();
                    if first == second && left == right {
                        assert_eq!(explanation, author);
                    } else {
                        for (label, rank, site, reason) in [
                            ("First", first, left, "first"),
                            ("Second", second, right, "second"),
                        ] {
                            let judgment = assessment(rank, reason, site).determination;
                            let applicability = site
                                .map(|applies| format!(" (applies {applies})"))
                                .unwrap_or_default();
                            assert!(
                                explanation.contains(&format!(
                                    "{label} assessment `{}`{applicability}: {reason}",
                                    judgment.kind()
                                )),
                                "{explanation}"
                            );
                        }
                        assert!(explanation.contains(&format!("Recorded the {author} assessment")));
                    }
                    if chosen.determination == Judgment::False {
                        assert_eq!(chosen.findings.len(), 1);
                        assert_eq!(
                            chosen.findings[0].description(),
                            format!("{author}'s repair")
                        );
                    }
                }
            }
        }
    }

    fn citation_judgment(determination: bool, rationale: &str) -> (&'static str, Value) {
        (
            "submit_result",
            json!({"result":{"determination":determination,"rationale":rationale}}),
        )
    }

    /// Runs one subject's assessment with `turns`, returning the outcome, the
    /// provider, and the subjects of the usage records written.
    // {cite}`habeas:util`
    async fn confirmed(
        turns: Vec<(&'static str, Value)>,
        confirm: bool,
    ) -> (io::Result<Assessment>, RecordingProvider, Vec<Value>) {
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
            if confirm {
                AssessmentPolicy::Confirm
            } else {
                AssessmentPolicy::Once
            },
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
            let Assessment { record, .. } = result.unwrap();
            assert!(record["rationale"].as_str().unwrap().contains(if confirm {
                "A gap."
            } else {
                "Covered."
            }));
            assert_eq!(provider.requests.lock().unwrap().len(), 1);
            assert_eq!(logs.len(), 1);
        }
    }

    // {test}`requirements.ci.authoring_inputs`
    #[tokio::test]
    async fn reassessment_records_only_freshly_delivered_inputs() {
        let project = inputs::tests::project();
        let subject = AuthoringSubject::Definition("requirements.ready".into());
        let path = subject.evidence_path(project.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, json!({"determination":false,"rationale":"OLD_RATIONALE","inputs":[{"file":"deleted.rs","item":"fn[old]","hash":"0".repeat(64)}]}).to_string()).unwrap();
        let mut sources = subject.sources(project.path()).unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut usage = UsageLog::start(state.path(), project.path(), &subject.label()).unwrap();
        let provider = RecordingProvider::new(vec![judgment(json!(true), "Fresh assessment.")]);
        let Assessment { record, .. } = assess(&provider, &mut sources, &mut usage).await.unwrap();
        assert!(
            record["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|input| input["requirements"] == "requirements.ready")
        );
        for stale in ["deleted.rs", "fn[old]", "OLD_RATIONALE"] {
            assert!(!record.to_string().contains(stale));
            assert!(!provider.prompts()[0].0.contains(stale));
        }
        assert_eq!(
            record["criteria"]["behavioral"]["output"]["rationale"],
            "Fresh assessment."
        );
    }

    // {test}`requirements.authoring.github_reads`
    #[tokio::test]
    async fn a_successful_github_tool_read_is_delivered_without_adding_an_evidence_input() {
        let root = inputs::tests::project();
        let state = tempfile::tempdir().unwrap();
        let github = crate::github::tests::FakeGithub::start(&[(
            "/repos/example/repo/issues/42",
            200,
            json!({"state":"closed","title":"Distinct issue title","body":"Distinct issue body"}),
        )]);
        let issues = crate::github::GithubIssues::with_api(
            github.endpoint.clone(),
            Some(state.path().join("cache")),
            None,
        );
        let mut sources =
            AssessmentSources::prepare_with_github(root.path(), "requirements.ready", issues)
                .unwrap();
        let provider = RecordingProvider::new(vec![
            ("github", json!({"issue":"example/repo#42"})),
            judgment(json!(true), "The requirement is covered."),
        ]);
        let mut log = UsageLog::start(state.path(), root.path(), "requirements.ready").unwrap();
        let Assessment { record, .. } = assess(&provider, &mut sources, &mut log).await.unwrap();
        let requests = provider.requests.lock().unwrap();
        let delivered: Value = serde_json::from_str(
            requests[1]["messages"].as_array().unwrap().last().unwrap()["content"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            delivered,
            json!({"reference":"example/repo#42","state":"closed","title":"Distinct issue title","body":"Distinct issue body"})
        );
        let baseline =
            RecordingProvider::new(vec![judgment(json!(true), "The requirement is covered.")]);
        let mut plain = AuthoringSubject::parse("requirements.ready")
            .sources(root.path())
            .unwrap();
        let Assessment {
            record: without_read,
            ..
        } = assess(&baseline, &mut plain, &mut log).await.unwrap();
        assert_eq!(record["inputs"], without_read["inputs"]);
        assert_eq!(record["dependencies"], without_read["dependencies"]);
    }

    // {test}`requirements.authoring.delivered_inputs`
    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.definitions.authoring_inputs`
    #[tokio::test]
    // {test}`requirements.authoring.line_numbers.rejection`
    async fn citation_and_example_assessments_record_selected_and_explicit_reads() {
        let project = inputs::tests::project();
        fs::write(project.path().join("md/requirements.md"), ":::{req} ready\nThe project MUST be ready.\n:::\n:::{example} sample\nA ready case. {cite}`requirements.ready`\n:::\n").unwrap();
        fs::write(
            project.path().join("md/terms.md"),
            ":::{definition} unused\n:phrase: sometimes\nOptional vocabulary.\n:::\n",
        )
        .unwrap();
        fs::write(project.path().join("src/lib.rs"), "// {cite}`requirements.ready`\nfn helper() {}\nfn first() { test_first(); }\nfn second() {}\nfn untouched() {}\n").unwrap();
        let state = tempfile::tempdir().unwrap();
        for (subject, example) in [
            ("citations:src/lib.rs#fn[helper]", false),
            ("requirements.sample", true),
        ] {
            let mut sources = AuthoringSubject::parse(subject)
                .sources(project.path())
                .unwrap();
            let mut log = UsageLog::start(state.path(), project.path(), subject).unwrap();
            let answer = if example {
                json!({"status":"success","rationale":"Checked both functions."})
            } else {
                json!({"determination":true,"rationale":"The citation is supported."})
            };
            let mut turns = vec![
                (
                    "search_text",
                    json!({"path":"src/lib.rs","pattern":"test_first","window_bytes":0}),
                ),
                ("finish_search", json!("first")),
                (
                    "read_node",
                    json!({"path":"src/lib.rs","location":"fn[second]"}),
                ),
                ("read_node", json!({"data":{"Definition":"terms.unused"}})),
            ];
            if example {
                let mut rejected = answer.clone();
                rejected["rationale"] = json!("See src/lib.rs:12 here.");
                turns.push(("submit_result", json!({"result":rejected})));
            }
            turns.push(("submit_result", json!({"result":answer})));
            let provider = RecordingProvider::new(turns);
            let inputs = assess(&provider, &mut sources, &mut log)
                .await
                .unwrap()
                .record["inputs"]
                .as_array()
                .unwrap()
                .clone();
            if example {
                let requests = provider.requests.lock().unwrap();
                assert_eq!(requests.len(), 6);
                let correction =
                    requests[5]["messages"].as_array().unwrap().last().unwrap()["content"]
                        .as_str()
                        .unwrap();
                assert!(
                    correction.contains("src/lib.rs:12")
                        && correction.contains("file and outline selector")
                        && correction
                            .split_whitespace()
                            .any(|word| uuid::Uuid::parse_str(word).is_ok()),
                    "{correction}"
                );
                drop(requests);
                provider.assert_tools(0);
            } else {
                provider.assert_tools(0);
            }
            for selector in ["fn[first]", "fn[second]"] {
                assert!(
                    inputs.contains(
                        &crate::outline_input(project.path(), "src/lib.rs", selector).unwrap()
                    ),
                    "{inputs:?}"
                );
            }
            assert!(
                inputs.iter().any(|i| i["definition"] == "terms.unused"
                    && i["text"]
                        == json!([
                            ":::{definition} unused",
                            ":phrase: sometimes",
                            "Optional vocabulary.",
                            ":::"
                        ])),
                "{inputs:?}"
            );
            assert!(!inputs.iter().any(|i| i["item"] == "fn[untouched]"));
            let requests = provider.requests.lock().unwrap();
            let text = requests[4]["messages"].as_array().unwrap().last().unwrap()["content"]
                .as_str()
                .unwrap();
            assert!(
                text.contains("Optional vocabulary.") && text.contains(":phrase: sometimes"),
                "{text}"
            );
        }
    }

    // {test}`requirements.authoring.confirmation`
    // {test}`requirements.authoring.usage_storage`
    // {test}`requirements.authoring.citation_prompt`
    // {test}`requirements.authoring.line_numbers.citation`
    #[tokio::test]
    async fn citation_reviews_are_confirmed_with_fresh_targeted_conversations() {
        let project = inputs::tests::project();
        fs::write(
            project.path().join("src/lib.rs"),
            "// {cite}`requirements.ready`\nfn helper() {}\n",
        )
        .unwrap();
        let subject = AuthoringSubject::parse("citations:src/lib.rs#fn[helper]");
        let sources = subject.sources(project.path()).unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut usage = UsageLog::start(state.path(), project.path(), &subject.label()).unwrap();
        let provider = RecordingProvider::new(vec![
            citation_judgment(true, "First citation review at src/lib.rs:1."),
            citation_judgment(true, "First citation review of `src/lib.rs#fn[helper]`."),
            citation_judgment(true, "Second citation review."),
        ]);
        let Assessment { record, .. } = assess_subject(
            &provider,
            project.path(),
            state.path(),
            &subject,
            sources,
            &mut usage,
            AssessmentPolicy::Confirm,
        )
        .await
        .unwrap();
        assert_eq!(record["item"], "fn[helper]");
        assert_eq!(
            record["rationale"],
            "First citation review of `src/lib.rs#fn[helper]`."
        );
        let prompts = provider.prompts();
        assert_eq!(prompts.len(), 3);
        let correction = provider.requests.lock().unwrap()[1]["messages"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .to_string();
        assert!(
            correction.contains(r#"The rationale cites a line number (\"src/lib.rs:1\")"#),
            "{correction}"
        );
        assert!(correction.contains("file and outline selector"));
        assert!(
            prompts
                .iter()
                .all(|(text, _)| text.contains("Item under review: src/lib.rs fn[helper]"))
        );
        assert!(
            !prompts[2]
                .0
                .contains("First citation review at src/lib.rs:1.")
        );
        let logs: Vec<Value> = fs::read_dir(state.path().join("authoring/usage"))
            .unwrap()
            .map(|p| serde_json::from_slice(&fs::read(p.unwrap().path()).unwrap()).unwrap())
            .collect();
        assert_eq!(logs.len(), 2);
        assert_ne!(logs[0]["run_id"], logs[1]["run_id"]);
    }

    // {test}`requirements.authoring.line_numbers.citation`
    #[tokio::test]
    async fn a_citation_review_fails_when_the_revised_rationale_still_uses_line_numbers() {
        let project = inputs::tests::project();
        fs::write(
            project.path().join("src/lib.rs"),
            "// {cite}`requirements.ready`\nfn helper() {}\n",
        )
        .unwrap();
        let subject = AuthoringSubject::parse("citations:src/lib.rs#fn[helper]");
        let state = tempfile::tempdir().unwrap();
        let mut usage = UsageLog::start(state.path(), project.path(), &subject.label()).unwrap();
        let provider = RecordingProvider::new(vec![
            citation_judgment(true, "See src/lib.rs:1."),
            citation_judgment(true, "See src/lib.rs:2."),
        ]);
        let result = assess_subject(
            &provider,
            project.path(),
            state.path(),
            &subject,
            subject.sources(project.path()).unwrap(),
            &mut usage,
            AssessmentPolicy::Confirm,
        )
        .await;
        assert!(result.is_err());
        assert_eq!(provider.prompts().len(), 2);
        assert!(!subject.evidence_path(project.path()).exists());
    }

    // {test}`requirements.authoring.confirmation.selection`
    // {test}`requirements.authoring.confirmation`
    // {test}`requirements.authoring.line_numbers.citation`
    #[tokio::test]
    async fn disagreeing_citation_assessments_record_both_explanations() {
        let project = inputs::tests::project();
        fs::write(
            project.path().join("src/lib.rs"),
            "// {cite}`requirements.ready`\nfn helper() {}\n",
        )
        .unwrap();
        let subject = AuthoringSubject::parse("citations:src/lib.rs#fn[helper]");
        let state = tempfile::tempdir().unwrap();
        let mut usage = UsageLog::start(state.path(), project.path(), &subject.label()).unwrap();
        let provider = RecordingProvider::new(vec![
            citation_judgment(true, "Fully covered."),
            citation_judgment(false, "The cited function omits a check."),
        ]);
        let Assessment { record, .. } = assess_subject(
            &provider,
            project.path(),
            state.path(),
            &subject,
            subject.sources(project.path()).unwrap(),
            &mut usage,
            AssessmentPolicy::Confirm,
        )
        .await
        .unwrap();
        assert_eq!(record["determination"], false);
        let rationale = record["rationale"].as_str().unwrap();
        assert!(rationale.contains("The cited function omits a check."));
        assert!(rationale.contains("Fully covered."));
        assert!(rationale.contains("Recorded the second"));
        assert_eq!(provider.prompts().len(), 2);
    }

    // {test}`requirements.authoring.citation_prompt`
    // {test}`requirements.authoring.submission`
    #[tokio::test]
    async fn citation_submission_requires_a_boolean_and_nonempty_rationale() {
        let project = inputs::tests::project();
        fs::write(
            project.path().join("src/lib.rs"),
            "// {cite}`requirements.ready`\nfn helper() {}\n",
        )
        .unwrap();
        let subject = AuthoringSubject::parse("citations:src/lib.rs#fn[helper]");
        let state = tempfile::tempdir().unwrap();
        for result in [
            json!({"determination":{"FIXME":["#1"]},"rationale":"A gap."}),
            json!({"determination":"true","rationale":"Covered."}),
            json!({"determination":true,"rationale":""}),
            json!({"determination":false,"rationale":"  "}),
        ] {
            let provider =
                RecordingProvider::new(vec![("submit_result", json!({"result": result}))]);
            let mut sources = subject.sources(project.path()).unwrap();
            let mut log = UsageLog::start(state.path(), project.path(), &subject.label()).unwrap();
            assert!(
                assess(&provider, &mut sources, &mut log).await.is_err(),
                "{result}"
            );
            let requests = provider.requests.lock().unwrap();
            let tool = requests[0]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["function"]["name"] == "submit_result")
                .unwrap();
            let fields = &tool["function"]["parameters"]["properties"]["result"]["properties"];
            assert_eq!(fields["determination"]["type"], "boolean");
            assert_eq!(fields["rationale"]["minLength"], 1);
            assert!(fields.get("applies").is_none());
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
        let Assessment {
            determination: judgment,
            record,
            ..
        } = result.unwrap();
        assert_eq!(judgment, Judgment::Fixme(vec!["#1".into()]));
        assert_eq!(
            record["criteria"]["behavioral"]["output"]["rationale"],
            "First."
        );
        // Two independent conversations, each with the initial prompt and its
        // own usage record.
        let prompts = provider.prompts();
        assert_eq!(prompts.len(), 2);
        assert_eq!(prompts[0], prompts[1]);
        assert!(prompts.iter().all(|(text, tools)| {
            text.contains("Assess only the requirement's observable behavioral obligations")
                && tools.contains(&"submit_result".to_owned())
        }));
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0]["subject"], "requirements.ready");
        assert_eq!(logs[1]["subject"], "requirements.ready (second assessment)");
        assert_eq!(logs[1]["outcome"], "completed");
        assert_eq!(logs[1]["determination"], "FIXME");
    }

    // {test}`requirements.authoring.initial_prompt`
    #[test]
    fn placeholders_are_filled_in_one_pass() {
        assert_eq!(
            fill("{{a}} {{b}} {{c}}", &[("a", "{{b}}"), ("b", "2")]),
            "{{b}} 2 {{c}}"
        );
    }

    // {test}`requirements.test-coverage.check.exceptions.author`
    // {test}`requirements.references.citation_review`
    // {test}`requirements.authoring.citation_prompt`
    #[tokio::test]
    // {test}`requirements.sdk.habeas.citation_criterion`
    async fn coverage_explanations_use_standard_citation_authoring_and_freshness() {
        let project = inputs::tests::project();
        let source = "// :::{cite} habeas:coverage\n// Defensive invariant abort, inaccessible through the API.\n// :::\nfn helper() {}\n";
        fs::write(project.path().join("src/lib.rs"), source).unwrap();
        let subject = AuthoringSubject::parse("citations:src/lib.rs#fn[helper]");
        let mut sources = subject.sources(project.path()).unwrap();
        let mandatory = sources.mandatory_inputs().to_vec();
        assert_eq!(mandatory[0]["item"], "fn[helper]");
        assert!(
            mandatory[0]["text"].to_string().contains("fn helper() {}"),
            "{mandatory:?}"
        );
        assert!(
            mandatory[0]["hash"]
                .as_str()
                .is_some_and(|hash| hash.len() == 64),
            "{mandatory:?}"
        );
        assert!(mandatory.iter().any(|input| {
            input["builtin"] == "habeas:coverage"
                && input["text"][0]
                    .as_str()
                    .unwrap()
                    .contains("difficulty alone")
        }));
        let prompt = initial_prompt(&sources).unwrap().joined();
        assert!(prompt.contains("Defensive invariant abort"));
        assert!(prompt.contains("For a built-in citation"));
        assert!(prompt.contains("MUST NOT submit a FIXME"));
        let state = tempfile::tempdir().unwrap();
        let mut usage = UsageLog::start(state.path(), project.path(), &subject.label()).unwrap();
        let provider = RecordingProvider::new(vec![citation_judgment(
            true,
            "The supplied source and explanation support the citation.",
        )]);
        let Assessment { record, .. } = assess(&provider, &mut sources, &mut usage).await.unwrap();
        assert_eq!(record["determination"], true);
        assert_eq!(record["citations"][0]["name"], "habeas:coverage");
        assert!(
            record["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|input| input["file"] == "src/lib.rs"
                    && input["item"] == "fn[helper]"
                    && input["hash"] == mandatory[0]["hash"]),
            "{record:?}"
        );
        assert!(
            record["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .all(|input| input.get("builtin").is_none())
        );
        use trust_but_verify::domain::{Domain, NodeContents};
        use trust_but_verify::graph::Dependency;
        let source_node = crate::sdk::HabeasNode::review_subject(&subject);
        assert!(matches!(source_node, crate::sdk::HabeasNode::Source(_)));
        assert_eq!(
            source_node.review_criterion().segments(),
            ["citation-review"]
        );
        let node = sources.domain.cache().intern_node(source_node.clone());
        let fingerprint = sources.domain.node_content(&node).unwrap().fingerprint();
        let expected = vec![Dependency::Node(source_node.clone(), fingerprint)];
        assert_eq!(
            crate::evidence_format::decode(&record, &source_node)
                .unwrap()
                .dependencies,
            expected
        );
        // Previously the same observation was also credited to a citation-specific node.
        let mut historical = record.clone();
        let identity = &mut historical["dependencies"][0]["Node"][0];
        *identity = json!({"CitationReview": identity["Source"].clone()});
        assert_eq!(
            crate::evidence_format::decode(&historical, &source_node)
                .unwrap()
                .dependencies,
            expected
        );
        let criterion = source_node.review_criterion();
        let decoded = crate::evidence_format::decode(&record, &source_node).unwrap();
        sources
            .domain
            .store_evidence(&node, &criterion, &decoded)
            .await
            .unwrap();
        let path = project
            .path()
            .join("evidence/citations/src/lib.rs/fn[helper].json");
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap(),
            record
        );
        let loaded = sources
            .domain
            .load_evidence(&node, &criterion)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.dependencies, expected);
        assert_eq!(loaded.output, decoded.output);
        fs::write(
            project.path().join("src/lib.rs"),
            source.replace("inaccessible", "reachable"),
        )
        .unwrap();
        let definitions = crate::evidence::load_definitions(project.path()).unwrap();
        let mut errors = Vec::new();
        let references = crate::evidence::collect_references(project.path(), &mut errors);
        let outlines = crate::inputs::OutlineCache::default();
        let item =
            crate::citations::citation_items(project.path(), &definitions, &references, &outlines)
                .remove(&("src/lib.rs".into(), "fn[helper]".into()))
                .unwrap();
        crate::citations::validate_review(
            &crate::sdk::HabeasDomain::new(project.path(), &definitions, &outlines),
            &item.review_path(),
            &item,
            &mut errors,
        );
        assert!(
            errors
                .iter()
                .any(|error| error.contains("citations changed")),
            "{errors:?}"
        );
    }

    // {test}`requirements.references.citation_review.book_commentary`
    // {test}`requirements.authoring.citation_prompt`
    // {test}`requirements.authoring.mandatory_inputs`
    #[tokio::test]
    async fn book_commentary_reviews_supply_the_whole_passage_and_check_concordance() {
        let project = inputs::tests::project();
        fs::write(
            project.path().join("md/terms.md"),
            ":::{definition} unit\n:phrase: unit\nA unit.\n:::\n",
        )
        .unwrap();
        fs::write(
            project.path().join("md/requirements.md"),
            ":::{req} ready\nReadiness is measured in {term}`terms.unit`.\n:::\n",
        )
        .unwrap();
        let body = "# Notes\n\nUnrelated chapter text.\n\n:::{cite} requirements.ready\nReadiness is measured in units. {cite}`habeas:coverage` explains the exemption.\n:::\n\nAdjacent prose.\n";
        fs::write(project.path().join("md/notes.md"), body).unwrap();
        let definitions = crate::evidence::load_definitions(project.path()).unwrap();
        let references = crate::evidence::collect_references(project.path(), &mut Vec::new());
        let items = crate::citations::citation_items(
            project.path(),
            &definitions,
            &references,
            &crate::inputs::OutlineCache::default(),
        );
        let item = items
            .values()
            .find(|item| item.file == "md/notes.md")
            .unwrap();
        let subject =
            AuthoringSubject::parse(&crate::citations::subject_label(&item.file, &item.item));
        let mut sources = subject.sources(project.path()).unwrap();
        let mandatory = sources.mandatory_inputs();
        assert!(mandatory.iter().any(|input| {
            input["builtin"] == "habeas:coverage"
                && input["text"][0]
                    .as_str()
                    .is_some_and(|s| s.contains("difficulty alone"))
        }));
        assert_eq!(mandatory[0]["file"], "md/notes.md");
        assert_eq!(mandatory[0]["item"], item.item);
        assert!(
            mandatory
                .iter()
                .any(|input| input["requirements"] == "requirements.ready")
        );
        assert!(
            mandatory
                .iter()
                .any(|input| input["definition"] == "terms.unit")
        );
        let state = tempfile::tempdir().unwrap();
        let mut usage = UsageLog::start(state.path(), project.path(), &subject.label()).unwrap();
        let provider = RecordingProvider::new(vec![citation_judgment(
            true,
            "The explanation is supported.",
        )]);
        let Assessment { record, .. } = assess(&provider, &mut sources, &mut usage).await.unwrap();
        let prompt = provider.prompts()[0].0.clone();
        for text in [
            "Review kind: book_commentary",
            "Readiness is measured in units.",
            "Readiness is measured in",
            "A unit.",
            "Contradicting or unsupported claims are gaps",
        ] {
            assert!(prompt.contains(text), "{text}: {prompt}");
        }
        assert!(!prompt.contains("Unrelated chapter text") && !prompt.contains("Adjacent prose"));
        assert!(!prompt.contains("{{review_kind}}"));
        assert_eq!(record["file"], "md/notes.md");
        assert_eq!(record["item"], item.item);
        assert_eq!(record["determination"], true);
        assert!(
            record["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|input| input["file"] == "md/notes.md" && input["item"] == item.item)
        );
        let path = subject.evidence_path(project.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
        let validate = |current: &crate::citations::CitationItem| {
            let mut errors = Vec::new();
            crate::citations::validate_review(
                &crate::sdk::HabeasDomain::new(
                    project.path(),
                    &definitions,
                    &crate::inputs::OutlineCache::default(),
                ),
                &current.review_path(),
                current,
                &mut errors,
            );
            errors
        };
        assert!(validate(item).is_empty());
        // A text change in the same passage invalidates its recorded read.
        fs::write(
            project.path().join("md/notes.md"),
            body.replace(
                "Readiness is measured in units.",
                "Readiness is measured in seconds.",
            ),
        )
        .unwrap();
        let stale = validate(item);
        assert!(
            stale.iter().any(|error| error.contains("hash")),
            "{stale:?}"
        );
        let mut changed = subject.sources(project.path()).unwrap();
        let provider = RecordingProvider::new(vec![citation_judgment(
            false,
            "Seconds contradicts the cited unit description.",
        )]);
        let Assessment {
            determination,
            record,
            ..
        } = assess(&provider, &mut changed, &mut usage).await.unwrap();
        assert_eq!(determination, Judgment::False);
        assert_eq!(record["determination"], false);
        assert!(
            provider.prompts()[0]
                .0
                .contains("Readiness is measured in seconds.")
        );
        fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
        assert!(
            validate(item)
                .iter()
                .any(|error| error.contains("determination is false"))
        );
        // Removing the opt-in passage orphans its existing review.
        fs::write(project.path().join("md/notes.md"), "Ordinary prose.\n").unwrap();
        let mut errors = Vec::new();
        let references = crate::evidence::collect_references(project.path(), &mut errors);
        crate::citations::validate(
            &crate::sdk::HabeasDomain::new(
                project.path(),
                &definitions,
                &crate::inputs::OutlineCache::default(),
            ),
            &references,
            &[path],
            &mut errors,
            &mut |_| {},
        );
        assert!(
            errors
                .iter()
                .any(|error| error.contains("orphaned citation review")),
            "{errors:?}"
        );
    }

    // {test}`requirements.authoring.citation_prompt`
    // {test}`requirements.references.citation_review`
    // {test}`requirements.authoring.mandatory_inputs`
    // {test}`requirements.authoring.habeas_background`
    // {test}`requirements.authoring.judgment_rules`
    // {test}`requirements.authoring.adaptive`
    #[tokio::test]
    // {test}`requirements.sdk.graph.agent-calls.subject_message`
    // {test}`requirements.sdk.graph.agent-calls.named_input`
    // {test}`requirements.authoring.no_line_numbers`
    async fn citation_reviews_use_their_prompt_and_record_the_item_and_citations() {
        let project = inputs::tests::project();
        let cite = "{cite}`requirements.ready`";
        fs::write(
            project.path().join("src/lib.rs"),
            format!(
                "// {cite}\n// :::{{cite}} habeas:util\n// Formats output for the caller.\n// :::\n// :::{{fixme}} requirements.ready\n// Remaining readiness gap.\n// :::\nfn helper() {{}}\n"
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
        assert!(!sources.has_applicable_fixme);
        assert!(sources.model_acknowledgments().is_empty());
        let mandatory = sources.mandatory_inputs().to_vec();
        assert_eq!(mandatory[0]["file"], "src/lib.rs");
        assert_eq!(mandatory[0]["item"], "fn[helper]");
        assert_eq!(mandatory[1]["requirements"], "requirements.ready");
        assert_eq!(mandatory[2]["builtin"], "habeas:util");
        assert!(
            mandatory[2]["text"][0]
                .as_str()
                .unwrap()
                .starts_with("Cite this on an item that serves")
        );

        let state = tempfile::tempdir().unwrap();
        let mut usage = UsageLog::start(state.path(), project.path(), &subject.label()).unwrap();
        let provider = RecordingProvider::new(vec![citation_judgment(true, "Both are sensible.")]);
        let Assessment { record, .. } = assess(&provider, &mut sources, &mut usage).await.unwrap();
        let prompts = provider.prompts();
        let mut tools = prompts[0].1.clone();
        tools.sort();
        assert_eq!(
            tools,
            [
                "finish_search",
                "get_next_page",
                "github",
                "list_sources",
                "lookup_requirements",
                "outgoing_edges",
                "outline_source",
                "read_node",
                "run_named_query",
                "search_structure",
                "search_text",
                "submit_result"
            ]
        );

        let prompt = &prompts[0].0;
        assert!(prompt.contains("Review kind: source_item"), "{prompt}");
        assert!(
            prompt.contains("Review one source item's citations."),
            "{prompt}"
        );
        assert!(
            prompt.contains("Item under review: src/lib.rs fn[helper]"),
            "{prompt}"
        );
        assert!(
            prompt.contains("Formats output for the caller."),
            "{prompt}"
        );
        // The initial request carries the complete mandatory set.
        assert!(prompt.contains("fn helper()"), "{prompt}");
        assert!(prompt.contains("The project MUST be ready."), "{prompt}");
        assert!(
            prompt.contains("Cite this on an item that serves"),
            "{prompt}"
        );
        // Prompts are recorded as serialized requests.
        for background in [
            approved_text("habeas_background", 0),
            approved_text("habeas_background", 1),
            approved_text("judgment_rules", 0),
        ] {
            let background = serde_json::to_string(background).unwrap();
            assert!(prompt.contains(background.trim_matches('"')), "{prompt}");
        }
        assert!(!prompt.contains("{{"), "{prompt}");

        let citations = json!([
            {"role": "cite", "name": "requirements.ready", "commentary": ""},
            {"role": "cite", "name": "habeas:util", "commentary": "Formats output for the caller."}
        ]);
        let request: Value = serde_json::from_str(prompt).unwrap();
        // Compare the actual placeholder payload, rather than text which
        // could also appear independently in the mandatory source inputs.
        let user = request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|message| message["role"] == "user")
            .unwrap();
        let subject_text = user["content"].as_str().unwrap();
        assert!(
            subject_text.starts_with("# Subject\n\nItem under review:"),
            "{subject_text}"
        );
        let task_end = subject_text.find("Citations to check: ").unwrap();
        assert!(subject_text.find("fn helper()").unwrap() > task_end);
        assert!(subject_text.find("The project MUST be ready.").unwrap() > task_end);
        let payload = subject_text
            .lines()
            .find_map(|line| line.strip_prefix("Citations to check: "))
            .unwrap();
        assert_eq!(serde_json::from_str::<Value>(payload).unwrap(), citations);
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

        // The root node's selector is shown as `<root>` in the prompt.
        fs::write(project.path().join("notes.txt"), format!("Notes. {cite}\n")).unwrap();
        let sources = AuthoringSubject::parse("citations:notes.txt")
            .sources(project.path())
            .unwrap();
        let prompt = initial_prompt(&sources).unwrap().joined();
        assert!(
            prompt.contains("Item under review: notes.txt <root>"),
            "{prompt}"
        );
        assert!(!prompt.contains("{{"), "{prompt}");
    }

    // {test}`requirements.authoring.testing_bar`
    // {test}`requirements.authoring.prompt_parts`
    // {test}`requirements.authoring.distributed_task`
    // {test}`requirements.authoring.habeas_background`
    // {test}`requirements.authoring.initial_prompt`
    // {test}`requirements.ci.review_criteria`
    #[test]
    fn a_distributed_requirement_task_continues_with_the_filled_distributed_task() {
        let project =
            crate::sites::tests::project(crate::sites::tests::RULE, crate::sites::tests::SOURCE);
        let sources = AssessmentSources::prepare(project.path(), "requirements.rule").unwrap();
        let prompt = initial_prompt(&sources).unwrap();
        let task = fill(
            approved_text("distributed_task", 0),
            &[("distributed_background", distributed_background())],
        );
        assert!(task.starts_with(distributed_background()), "{task}");
        // The requirement task stays in the system prompt; the distributed
        // task follows the subject line in the subject message.
        assert!(
            prompt.system.contains(&format!(
                "{}\n{}\n\n# How to judge",
                approved_text("requirement_task", 0),
                approved_text("testing_bar", 0)
            )),
            "{}",
            prompt.system
        );
        assert!(!prompt.system.contains(&task), "{}", prompt.system);
        assert!(
            prompt.subject.contains(&format!(
                "Judge against: the subject's text.\n\n{task}\n\n# Acknowledgments\n\n[]\n\n# Mandatory inputs"
            )),
            "{}",
            prompt.subject
        );
        assert!(
            prompt
                .system
                .contains(approved_text("habeas_background", 1))
        );
        assert!(!prompt.joined().contains("{{"), "{}", prompt.joined());
        let chapter = project.path().join("md/requirements.md");
        let mut text = fs::read_to_string(&chapter).unwrap();
        text.push_str(&crate::sites::tests::RULE.replace("{req} rule", "{req} other"));
        fs::write(chapter, text).unwrap();
        let other = AssessmentSources::prepare(project.path(), "requirements.other").unwrap();
        let other = initial_prompt(&other).unwrap();
        assert_eq!(prompt.system, other.system);
        assert!(
            other
                .subject
                .contains("Subject: requirement requirements.other")
        );
        assert!(other.subject.contains(&task));
        for subject in ["requirements.rule", "requirements.other"] {
            assert!(!other.system.contains(subject));
        }
    }

    /// Every system prompt is the same for two subjects of its kind, and
    /// every subject-dependent value is in the subject message.
    // {test}`requirements.authoring.prompt_parts`
    // {test}`requirements.authoring.initial_prompt`
    // {test}`requirements.authoring.structural_prompt`
    // {test}`requirements.authoring.structural_review`
    // {test}`requirements.authoring.habeas_background`
    // {test}`requirements.authoring.judgment_rules`
    // {test}`requirements.authoring.citation_prompt`
    // {test}`requirements.authoring.site_prompt`
    #[test]
    // {test}`requirements.authoring.no_line_numbers`
    fn subjects_of_one_kind_share_a_byte_identical_system_prompt() {
        // Check every literal segment of the selected approved blocks, allowing
        // only declared placeholders to expand, then check the opening message roles.
        fn assert_opening(prompt: &Prompt, template: &str) {
            let placeholders = regex::Regex::new(r"\{\{[^}]+\}\}").unwrap();
            for (index, actual) in [&prompt.system, &prompt.subject].into_iter().enumerate() {
                let expected = approved_text(template, index);
                let mut pattern = String::from(r"\A");
                let mut end = 0;
                for placeholder in placeholders.find_iter(expected) {
                    pattern.push_str(&regex::escape(&expected[end..placeholder.start()]));
                    pattern.push_str("(?s:.*?)");
                    end = placeholder.end();
                }
                pattern.push_str(&regex::escape(&expected[end..]));
                pattern.push_str(r"\z");
                assert!(
                    regex::Regex::new(&pattern).unwrap().is_match(actual),
                    "{template} block {index}: {actual}"
                );
                assert!(!actual.contains("{{"), "{actual}");
            }
        }
        let project =
            crate::sites::tests::project(crate::sites::tests::RULE, crate::sites::tests::SOURCE);
        let requirements = project.path().join("md/requirements.md");
        let mut book = fs::read_to_string(&requirements).unwrap();
        let cite = "{cite}`requirements.rule`";
        book.push_str(&format!(
            "\n:::{{req}} other\nThe project MUST also work.\n:::\n\n:::{{req}} second\nThe project MUST keep working.\n:::\n\n:::{{req}} structure_one\nThe API MUST delegate to a helper.\n:::\n\n:::{{req}} structure_two\nThe API MUST expose a function.\n:::\n\n:::{{example}} first_case\nA case. {cite}\n:::\n\n:::{{example}} second_case\nAnother case. {cite}\n:::\n"
        ));
        fs::write(
            project.path().join("md/terms.md"),
            ":::{definition} size\n:phrase: size\nThe size of a batch.\n:::\n",
        )
        .unwrap();
        fs::write(
            &requirements,
            book.replace(
                "The project MUST also work.",
                "The project MUST also work for each {term}`terms.size`.",
            ),
        )
        .unwrap();
        let source = project.path().join("src/lib.rs");
        let mut text = fs::read_to_string(&source).unwrap();
        text.push_str(&format!(
            "\n// {cite}\nfn cites_one() {{}}\n\n// {cite}\n// :::{{cite}} habeas:util\n// Formats output.\n// :::\nfn cites_two() {{}}\n\n// {{impl}}`requirements.structure_one`\nfn delegates() {{ helper(); }}\nfn helper() {{}}\n// {{impl}}`requirements.structure_two`\npub fn api() {{}}\n"
        ));
        text.push_str("\n// {test}`requirements.other`\nfn check_batch_size() {}\n// :::{fixme} requirements.other\n// Handle larger batches.\n// :::\nfn grow() {}\n");
        fs::write(&source, text).unwrap();
        let position = regex::Regex::new(r#"(?i)\.(?:rs|md|toml):\d+|\blines?\s+\d+|"(?:line|column|line_start|line_end|start_line|end_line)"\s*:"#).unwrap();
        for (first, second, template) in [
            (
                "requirements.other",
                "requirements.second",
                "structural_prompt",
            ),
            (
                "requirements.structure_one",
                "requirements.structure_two",
                "structural_prompt",
            ),
            (
                "requirements.first_case",
                "requirements.second_case",
                "initial_prompt",
            ),
            (
                "citations:src/lib.rs#fn[cites_one]",
                "citations:src/lib.rs#fn[cites_two]",
                "citation_prompt",
            ),
            (
                "sites:requirements.rule/src/lib.rs#fn[check_one]",
                "sites:requirements.rule/src/lib.rs#fn[check_two]",
                "site_prompt",
            ),
        ] {
            let sources = |subject: &str| {
                AuthoringSubject::parse(subject)
                    .sources(project.path())
                    .unwrap()
            };
            let (first, second) = (sources(first), sources(second));
            for sources in [&first, &second] {
                for input in sources
                    .mandatory_inputs()
                    .iter()
                    .chain(sources.model_acknowledgments().iter())
                {
                    if input.get("file").is_some() {
                        assert!(
                            input["file"].is_string() && input["item"].is_string(),
                            "{input}"
                        );
                    }
                    let mut metadata = input.clone();
                    let fields = metadata.as_object_mut().unwrap();
                    fields.remove("text");
                    fields.remove("body");
                    assert!(!position.is_match(&metadata.to_string()), "{metadata}");
                }
            }
            assert_eq!(
                first.review_roots.is_some(),
                template == "structural_prompt"
            );
            assert_eq!(
                second.review_roots.is_some(),
                template == "structural_prompt"
            );
            let (one, two) = (
                initial_prompt(&first).unwrap(),
                initial_prompt(&second).unwrap(),
            );
            assert_opening(&one, template);
            assert_opening(&two, template);
            for prompt in [&one, &two] {
                assert!(
                    !position.is_match(&prompt.subject),
                    "{template}: {}",
                    prompt.subject
                );
                assert!(
                    !position.is_match(&prompt.system),
                    "{template}: {}",
                    prompt.system
                );
                assert!(
                    prompt
                        .system
                        .contains(approved_text("habeas_background", 0))
                );
                assert!(prompt.system.contains(approved_text("judgment_rules", 0)));
                assert!(prompt.system.contains("Skip generated assessment records"));
                assert!(prompt.system.contains("out of `finish_search.used`"));
                assert_eq!(
                    prompt
                        .system
                        .contains(approved_text("habeas_background", 1)),
                    template != "site_prompt"
                );
            }
            assert_eq!(one.system, two.system, "{}", first.subject_name);
            assert_ne!(one.subject, two.subject);
            assert!(one.subject.starts_with("# Subject\n\n"), "{}", one.subject);
            assert!(!one.system.contains(&first.subject_name));
        }
    }

    // {test}`requirements.authoring.usage_storage`
    #[test]
    fn usage_subject_kind_is_known_before_sources_are_prepared() {
        let project =
            crate::sites::tests::project(crate::sites::tests::RULE, crate::sites::tests::SOURCE);
        let root = project.path();
        let requirements = root.join("md/requirements.md");
        let mut book = fs::read_to_string(&requirements).unwrap();
        let cite = "{cite}`requirements.rule`";
        book.push_str(&format!("\n:::{{example}} case\nA case. {cite}\n:::\n"));
        fs::write(&requirements, book).unwrap();
        let source = root.join("src/lib.rs");
        let mut text = fs::read_to_string(&source).unwrap();
        text.push_str(&format!("\n// {cite}\nfn cites() {{}}\n"));
        fs::write(&source, text).unwrap();
        for (subject, kind) in [
            ("requirements.rule", "requirement"),
            ("requirements.case", "example"),
            ("citations:src/lib.rs#fn[cites]", "citation review"),
            (
                "sites:requirements.rule/src/lib.rs#fn[check_one]",
                "site review",
            ),
        ] {
            let subject = AuthoringSubject::parse(subject);
            assert_eq!(subject.kind_noun(root), Some(kind), "{}", subject.label());
            // The same kind the prepared sources record.
            assert_eq!(subject.sources(root).unwrap().subject_kind.noun(), kind);
        }
        assert_eq!(
            AuthoringSubject::parse("requirements.missing").kind_noun(root),
            None
        );
    }

    /// A requirement whose only site query selects inner attributes, which
    /// belong to their file's root node.
    const ATTRIBUTES: &str = r##"::::{req} attributes
Crate attributes MUST be justified.

:::{sites} inner
```yaml
language: rust
paths: [src]
rule: { kind: inner_attribute_item }
match:
  - "#![allow(dead_code)]"
no_match:
  - "#[allow(dead_code)]\nfn a() {}"
```
:::
::::
"##;

    /// A project whose `requirements.rule` cites `requirements.other` and
    /// `habeas:util`, with a root-node site of `requirements.attributes`.
    // {cite}`habeas:util`
    pub(crate) fn citing_site_project() -> tempfile::TempDir {
        let cites = "{cite}`requirements.other` and {cite}`habeas:util`";
        let rule = crate::sites::tests::RULE.replace(
            "instead of panicking.",
            &format!("instead of panicking, as {cites} describe."),
        );
        crate::sites::tests::project(
            &format!("{rule}\n:::{{req}} other\nOther MUST hold.\n:::\n\n{ATTRIBUTES}"),
            &format!("#![allow(dead_code)]\n{}", crate::sites::tests::SOURCE),
        )
    }

    // {test}`requirements.references.site_review`
    // {test}`requirements.references.builtin_definitions`
    #[test]
    fn site_reviews_cite_declarations_go_stale_with_them_and_name_root_sites() {
        let project = citing_site_project();
        let root = project.path();
        // Records a current review of a site with its mandatory inputs.
        let write = |label: &str, queries: &[&str], drop: Option<&str>| {
            let subject = AuthoringSubject::parse(label);
            let AuthoringSubject::Site {
                requirement,
                file,
                item,
            } = &subject
            else {
                unreachable!()
            };
            let mut sources = subject.sources(root).unwrap();
            let mandatory = sources.mandatory_inputs().to_vec();
            sources.record_inputs(&mandatory);
            let inputs = sources
                .recorded_inputs()
                .iter()
                .filter(|input| drop.is_none_or(|name| input["requirements"] != name))
                .cloned()
                .collect::<Vec<_>>();
            let path = subject.evidence_path(root);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(
                &path,
                json!({"requirement": requirement, "file": file, "item": item,
                    "queries": queries, "applies": true, "fixmes": [],
                    "determination": true, "rationale": "Fine.", "inputs": inputs})
                .to_string(),
            )
            .unwrap();
            (sources, path)
        };
        let review_errors = |path: &Path| {
            let prefix = format!("{}:", crate::evidence::display_path(root, path));
            crate::sites::tests::inspect(root)
                .0
                .into_iter()
                .filter(|error| error.starts_with(&prefix))
                .collect::<Vec<_>>()
        };

        // A site review receives each declaration and built-in text its
        // requirement cites.
        let (sources, two) = write(
            "sites:requirements.rule/src/lib.rs#fn[check_two]",
            &["checkers"],
            None,
        );
        let mandatory = sources.mandatory_inputs();
        assert!(
            mandatory
                .iter()
                .any(|input| input["requirements"] == "requirements.other"
                    && input["text"][1] == "Other MUST hold."),
            "{mandatory:#?}"
        );
        assert_eq!(
            mandatory
                .iter()
                .filter(|input| input["builtin"] == "habeas:util")
                .count(),
            1,
            "{mandatory:#?}"
        );
        assert_eq!(review_errors(&two), Vec::<String>::new());

        // A review without a cited declaration is incomplete.
        let (_, one) = write(
            "sites:requirements.rule/src/lib.rs#fn[check_one]",
            &["checkers", "panics"],
            Some("requirements.other"),
        );
        assert!(
            review_errors(&one)
                .iter()
                .any(|error| error
                    .contains("inputs must include the declaration of requirements.other")),
            "{:#?}",
            review_errors(&one)
        );

        // Omitting the selector names the file's root node.
        let (_, attributes) = write("sites:requirements.attributes/src/lib.rs", &["inner"], None);
        assert!(
            attributes.ends_with("evidence/sites/requirements.attributes/src/lib.rs/_root.json"),
            "{}",
            attributes.display()
        );
        assert_eq!(review_errors(&attributes), Vec::<String>::new());

        // Changing the requirement's or a cited declaration makes the review
        // stale.
        let chapter = root.join("md/requirements.md");
        let original = fs::read_to_string(&chapter).unwrap();
        for (old, new) in [
            ("Other MUST hold.", "Other MUST still hold."),
            ("Each checker MUST", "Every checker MUST"),
        ] {
            fs::write(&chapter, original.replace(old, new)).unwrap();
            assert!(
                review_errors(&two)
                    .iter()
                    .any(|error| error.contains("input text changed")),
                "{new}: {:#?}",
                review_errors(&two)
            );
        }
    }

    // {test}`requirements.authoring.site_prompt`
    // {test}`requirements.authoring.habeas_background`
    // {test}`requirements.references.site_review`
    // {test}`requirements.authoring.mandatory_inputs`
    // {test}`requirements.authoring.judgment_rules`
    // {test}`requirements.authoring.no_line_numbers`
    #[tokio::test]
    async fn site_reviews_use_their_prompt_and_record_applies_and_the_site() {
        let project = crate::sites::tests::project(
            crate::sites::tests::RULE,
            &crate::sites::tests::SOURCE.replace(
                "fn check_two() {}",
                "fn check_two() {\n    // :::{fixme} requirements.rule\n    // Panics.\n    // :::\n}",
            ),
        );
        let root = project.path();
        let declaration = crate::sites::tests::RULE.replace("Each checker MUST report problems instead of panicking.", "Each checker MUST report problems instead of panicking. {cite}`requirements.other` {term}`terms.value`");
        fs::write(
            root.join("md/requirements.md"),
            format!("{declaration}\n:::{{req}} other\nOther MUST hold.\n:::\n"),
        )
        .unwrap();
        fs::write(
            root.join("md/terms.md"),
            ":::{definition} value\n:phrase: value\nA useful value.\n:::\n",
        )
        .unwrap();
        let subject = AuthoringSubject::parse("sites:requirements.rule/src/lib.rs#fn[check_two]");
        assert_eq!(
            subject.evidence_path(root),
            root.join("evidence/sites/requirements.rule/src/lib.rs/fn[check_two].json")
        );
        let mut sources = subject.sources(root).unwrap();
        let mandatory = sources.mandatory_inputs().to_vec();
        assert_eq!(mandatory[0]["file"], "src/lib.rs");
        assert_eq!(mandatory[0]["item"], "fn[check_two]");
        assert_eq!(mandatory[1]["requirements"], "requirements.rule");
        assert_eq!(mandatory.len(), 4, "{mandatory:#?}");
        assert!(
            mandatory
                .iter()
                .any(|input| input["requirements"] == "requirements.other")
        );
        assert!(
            mandatory
                .iter()
                .any(|input| input["definition"] == "terms.value")
        );

        // The subject message names the requirement, the site's file and
        // selector, and each query match with its kind, name and lines.
        let matches =
            json!([{"kind": "sites", "query": "checkers", "item": "fn[check_two]"}]).to_string();
        let mandatory_inputs =
            serde_json::to_string_pretty(&sources.model_mandatory_inputs()).unwrap();
        let expected_subject = fill(
            approved_text("site_prompt", 1),
            &[
                ("requirement_name", "requirements.rule"),
                ("site_file", "src/lib.rs"),
                ("site_selector", "fn[check_two]"),
                ("matches", &matches),
                ("mandatory_inputs", &mandatory_inputs),
            ],
        );
        let initial = initial_prompt(&sources).unwrap();
        assert_eq!(initial.subject, expected_subject);
        assert!(
            initial
                .subject
                .starts_with(&format!(
                    "# Subject\n\nSite under review: src/lib.rs fn[check_two]\nRequirement: requirements.rule\nMatches: {matches}\n"
                )),
            "{}",
            initial.subject
        );

        let state = tempfile::tempdir().unwrap();
        let mut usage = UsageLog::start(state.path(), root, &subject.label()).unwrap();
        let provider = RecordingProvider::new(vec![(
            "submit_result",
            json!({"result":{"applies": true, "determination": {"FIXME": ["Panics."]}, "rationale": "Acknowledged."}}),
        )]);
        let Assessment { record, .. } = assess(&provider, &mut sources, &mut usage).await.unwrap();
        let prompts = provider.prompts();
        let (prompt, tools) = &prompts[0];
        let prompt = serde_json::from_str::<Value>(prompt).unwrap();
        let text = prompt.to_string();
        assert!(
            text.contains(
                serde_json::to_string(&prompt_section(&initial.subject, "Subject").trim())
                    .unwrap()
                    .trim_matches('"')
            ),
            "the subject message reaches the model"
        );
        let heading = approved_text("site_prompt", 0)
            .split("\n\n# Background")
            .next()
            .unwrap();
        assert!(
            text.contains(serde_json::to_string(heading).unwrap().trim_matches('"')),
            "{heading}\n{text}"
        );
        for block in [
            approved_text("habeas_background", 0),
            distributed_background(),
            approved_text("judgment_rules", 0),
        ] {
            assert!(
                text.contains(serde_json::to_string(block).unwrap().trim_matches('"')),
                "{block}"
            );
        }
        assert!(
            !text.contains(
                serde_json::to_string(approved_text("habeas_background", 1))
                    .unwrap()
                    .trim_matches('"')
            ),
            "the site prompt has no reference roles"
        );
        assert!(!text.contains("{{"), "{text}");
        assert!(tools.contains(&"submit_result".to_owned()));
        provider.assert_tools(0);
        let schema = prompt["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["function"]["name"] == "submit_result")
            .unwrap();
        let result = &schema["function"]["parameters"]["properties"]["result"];
        assert_eq!(result["properties"]["applies"]["type"], "boolean");
        assert!(
            result["required"]
                .as_array()
                .unwrap()
                .contains(&json!("applies"))
        );
        assert_eq!(record["requirement"], "requirements.rule");
        assert_eq!(record["file"], "src/lib.rs");
        assert_eq!(record["item"], "fn[check_two]");
        assert_eq!(record["queries"], json!(["checkers"]));
        assert_eq!(record["applies"], true);
        let outside_scope = RecordingProvider::new(vec![
            (
                "submit_result",
                json!({"result":{"applies":false,"determination":false,"rationale":"Outside scope."}}),
            ),
            (
                "submit_result",
                json!({"result":{"applies":false,"determination":true,"rationale":"Outside scope."}}),
            ),
        ]);
        let mut sources = subject.sources(root).unwrap();
        let Assessment {
            record: outside_record,
            ..
        } = assess(&outside_scope, &mut sources, &mut usage)
            .await
            .unwrap();
        assert_eq!(outside_record["applies"], false);
        assert!(
            outside_scope.prompts()[1]
                .0
                .contains("determination must be true when applies is false")
        );
        assert_eq!(
            record["fixmes"],
            json!([{"item": "fn[check_two]", "commentary": "Panics."}])
        );
        // The recorded review satisfies validation for its site.
        let path = subject.evidence_path(root);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, record.to_string()).unwrap();
        let errors = crate::validate_repository(root).unwrap().errors;
        let prefix = crate::sites::review_path("requirements.rule", "src/lib.rs", "fn[check_two]");
        assert!(
            !errors.iter().any(|error| error.starts_with(&prefix)),
            "{errors:#?}"
        );
    }

    // {test}`requirements.authoring.credentials`
    // {test}`requirements.authoring.redaction`
    #[test]
    fn site_prompts_exclude_credentials_from_source_and_configuration() {
        let token = crate::redaction::tests::github_token();
        let source = crate::sites::tests::SOURCE.replace(
            "fn check_two() {}",
            &format!("fn check_two() {{ let credential = \"{token}\"; }}"),
        );
        let project = crate::sites::tests::project(crate::sites::tests::RULE, &source);
        fs::write(project.path().join("habeas.toml"), "version = 1\n[authoring.configs.local]\nbackend = 'openai'\napi_key = 'SITE_CONFIG_SECRET'\n").unwrap();
        let subject = AuthoringSubject::parse("sites:requirements.rule/src/lib.rs#fn[check_two]");
        let sources = subject.sources(project.path()).unwrap();
        let prompt = initial_prompt(&sources).unwrap().joined();
        assert!(prompt.contains("[REDACTED:GitHub Token]"));
        assert!(!prompt.contains(&token));
        assert!(!prompt.contains("SITE_CONFIG_SECRET"));
    }

    // {test}`requirements.authoring.site_prompt`
    // {test}`requirements.authoring.submission`
    #[test]
    fn site_submissions_require_applies_and_a_true_determination_when_it_is_false() {
        assert_eq!(
            site_submission(
                &json!({"applies": false, "determination": true, "rationale": "Not a checker."})
            )
            .unwrap(),
            (Judgment::True, "Not a checker.".to_owned(), false)
        );
        for arguments in [
            json!({"determination": true, "rationale": "x"}),
            json!({"applies": "no", "determination": true, "rationale": "x"}),
            json!({"applies": false, "determination": false, "rationale": "x"}),
            json!({"applies": false, "determination": {"FIXME": ["a"]}, "rationale": "x"}),
            json!({"applies": true, "determination": true, "rationale": "x", "extra": 1}),
        ] {
            assert!(site_submission(&arguments).is_err(), "accepted {arguments}");
        }
    }

    // {test}`requirements.authoring.structural_review`
    // {test}`requirements.authoring.behavioral_review`
    // {test}`requirements.authoring.structural_inputs`
    // {test}`requirements.references.authoring_inputs`
    // {test}`requirements.ci.reference_set`
    #[test]
    fn structural_roots_select_inputs_and_version_the_reference_digest() {
        let root = inputs::tests::project();
        let (path, configuration) = crate::execution_coverage::configure(root.path()).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, configuration).unwrap();
        let impl_role = inputs::tests::role("impl", "requirements.ready");
        let test_role = inputs::tests::role("test", "requirements.ready");
        let implementation =
            format!("// {impl_role}\npub fn api() {{ helper(); }}\nfn helper() {{}}\n");
        let declaration = ":::{req} ready\n:impl: src/lib.rs#fn[api]\nThe public api function delegates to helper.\n:::\n";
        fs::write(root.path().join("md/requirements.md"), declaration).unwrap();
        fs::write(root.path().join("src/lib.rs"), &implementation).unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        assert!(sources.review_roots.is_some());
        let inputs = sources.mandatory_inputs();
        let roots = inputs
            .iter()
            .filter(|input| input["file"] == "src/lib.rs")
            .collect::<Vec<_>>();
        assert_eq!(
            roots.len(),
            1,
            "anchor and reference must deduplicate: {inputs:?}"
        );
        assert_eq!(roots[0]["item"], "fn[api]");
        assert!(roots[0]["text"].to_string().contains("helper();"));
        assert!(!roots[0]["text"].to_string().contains("fn helper"));
        // In-source references select their own enclosing node without an anchor.
        fs::write(
            root.path().join("md/requirements.md"),
            ":::{req} ready\nThe public api function delegates to helper.\n:::\n",
        )
        .unwrap();
        let unanchored = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        assert!(unanchored.review_roots.is_some());
        assert_eq!(unanchored.mandatory_inputs(), inputs);
        // A structural assessment also includes plain-text implementation roots
        // and complete refining declarations, without recursively reading helpers.
        let plain = format!("Plain implementation\n{impl_role}\nWhole file content.\n");
        fs::write(root.path().join("src/details.txt"), &plain).unwrap();
        fs::write(root.path().join("md/requirements.md"), format!(":::{{req}} ready\nThe public api function delegates to helper.\n:::\n:::{{req}} refined\n{impl_role}\nComplete refinement.\nSecond paragraph.\n:::\n")).unwrap();
        let refined = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        assert!(refined.review_roots.is_some());
        let refined_inputs = refined.mandatory_inputs();
        assert!(
            refined_inputs
                .iter()
                .any(|input| input["file"] == "src/details.txt"
                    && input["item"] == ""
                    && input["text"]
                        == json!(["Plain implementation", &impl_role, "Whole file content."])),
            "{refined_inputs:?}"
        );
        assert!(
            refined_inputs
                .iter()
                .any(|input| input["requirements"] == "requirements.refined"
                    && input["text"].to_string().contains("Complete refinement.")
                    && input["text"].to_string().contains("Second paragraph.")),
            "{refined_inputs:?}"
        );
        fs::remove_file(root.path().join("src/details.txt")).unwrap();
        fs::write(root.path().join("md/requirements.md"), declaration).unwrap();
        let definitions = crate::evidence::load_definitions(root.path()).unwrap();
        let domain = crate::sdk::HabeasDomain::new(root.path(), &definitions, &sources.outlines);
        let entries = domain
            .reference_entries(&crate::sdk::RequirementId("requirements.ready".into()))
            .unwrap();
        let historical = crate::evidence::digest_entries(entries);
        assert_ne!(sources.reference_set, historical);
        use sha2::{Digest, Sha256};
        let structural_only =
            Sha256::digest(format!("habeas-structural-assessment-v1\0{historical}"));
        let structural_only: String = structural_only
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_ne!(sources.reference_set, structural_only);
        fs::create_dir_all(root.path().join("evidence")).unwrap();
        let record = json!({"requirement": sources.subject_text, "determination": true,
            "rationale":"A former test-adequacy review.", "references":historical,
            "inputs":inputs.iter().map(crate::inputs::recorded_input).collect::<Vec<_>>()});
        fs::write(
            root.path().join("evidence/requirements.ready.json"),
            record.to_string(),
        )
        .unwrap();
        let errors = crate::evidence::validate_repository(root.path())
            .unwrap()
            .errors
            .join("\n");
        assert!(errors.contains("reference set changed"), "{errors}");
        // Structural sources are dependencies, so implementation changes stale a current review.
        let mut current = record;
        current["references"] = json!(sources.reference_set);
        fs::write(
            root.path().join("evidence/requirements.ready.json"),
            current.to_string(),
        )
        .unwrap();
        let errors = crate::evidence::validate_repository(root.path())
            .unwrap()
            .errors;
        assert!(errors.is_empty(), "{errors:?}");
        fs::write(
            root.path().join("src/lib.rs"),
            implementation.replace("helper();", "other();"),
        )
        .unwrap();
        assert!(
            crate::evidence::validate_repository(root.path())
                .unwrap()
                .errors
                .iter()
                .any(|error| error.contains("input") && error.contains("changed"))
        );
        // Adding a test keeps the implementation root in the review.
        fs::write(
            root.path().join("src/lib.rs"),
            format!("{implementation}\n// {test_role}\n#[test]\nfn checks_api() {{ api(); }}\n"),
        )
        .unwrap();
        fs::write(
            root.path().join("md/requirements.md"),
            declaration.replace(":impl: src/lib.rs#fn[api]\n", ""),
        )
        .unwrap();
        let tested = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        assert!(tested.review_roots.is_some());
        assert!(
            tested
                .mandatory_inputs()
                .iter()
                .any(|input| input["item"] == "fn[api]")
        );
        let definitions = crate::evidence::load_definitions(root.path()).unwrap();
        let domain = crate::sdk::HabeasDomain::new(root.path(), &definitions, &tested.outlines);
        assert_ne!(
            tested.reference_set,
            crate::evidence::digest_entries(
                domain
                    .reference_entries(&crate::sdk::RequirementId("requirements.ready".into()))
                    .unwrap()
            )
        );
        // Test anchors also keep the implementation root.
        fs::write(root.path().join("src/lib.rs"), &implementation).unwrap();
        fs::write(
            root.path().join("md/requirements.md"),
            declaration.replace(":impl:", ":test:"),
        )
        .unwrap();
        assert!(
            AssessmentSources::prepare(root.path(), "requirements.ready")
                .unwrap()
                .review_roots
                .is_some()
        );
        // An implementation anchor alone is enough, without an in-source reference.
        fs::write(root.path().join("src/lib.rs"), "pub fn api() {}\n").unwrap();
        fs::write(root.path().join("md/requirements.md"), declaration).unwrap();
        assert!(
            AssessmentSources::prepare(root.path(), "requirements.ready")
                .unwrap()
                .review_roots
                .is_some()
        );
        let check_single_review = || {
            let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
            assert!(sources.review_roots.is_none());
            assert!(
                !sources
                    .mandatory_inputs()
                    .iter()
                    .any(|input| input["file"] == "src/lib.rs")
            );
            let definitions = crate::evidence::load_definitions(root.path()).unwrap();
            let domain =
                crate::sdk::HabeasDomain::new(root.path(), &definitions, &sources.outlines);
            let entries = domain
                .reference_entries(&crate::sdk::RequirementId("requirements.ready".into()))
                .unwrap();
            assert_eq!(
                sources.reference_set,
                crate::evidence::digest_entries(entries)
            );
        };
        fs::write(
            root.path().join("md/requirements.md"),
            ":::{req} base\nContext.\n:::\n:::{example} ready\nA case. {cite}`requirements.base`\n:::\n",
        )
        .unwrap();
        fs::write(root.path().join("src/lib.rs"), &implementation).unwrap();
        check_single_review();
        let distributed = format!("{declaration}\n").replace("The public api", "::::{sites} functions\n```yaml\nlanguage: rust\nrule: {kind: function_item}\nmatch: ['fn api() {}']\nno_match: ['struct Other;']\n```\n::::\nThe public api");
        fs::write(root.path().join("md/requirements.md"), distributed).unwrap();
        check_single_review();
        fs::write(root.path().join("md/requirements.md"), format!(":::{{req}} ready\nStructure.\n:::\n:::{{req}} refined\n{impl_role}\nRefinement.\n:::\n")).unwrap();
        fs::write(root.path().join("src/lib.rs"), "fn unreferenced() {}\n").unwrap();
        let refined = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        assert!(refined.review_roots.is_some());
        let inputs = refined.mandatory_inputs();
        assert!(
            inputs
                .iter()
                .any(|input| input["requirements"] == "requirements.refined"
                    && input["text"]
                        == json!([":::{req} refined", impl_role, "Refinement.", ":::"]))
        );
        assert!(inputs.iter().all(|input| input.get("file").is_none()));
    }

    // {test}`requirements.ci.review_criteria`
    // {test}`requirements.references.authoring_inputs`
    // {test}`requirements.authoring.structural_review`
    // {test}`requirements.authoring.behavioral_review`
    // {test}`requirements.authoring.structural_inputs`
    // {test}`requirements.ci.reference_set`
    // {test}`requirements.authoring.prompt_parts`
    #[test]
    fn removing_either_root_changes_the_evidence_but_keeps_both_checks() {
        let project = inputs::tests::project();
        let root = project.path();
        fs::write(
            root.join("api.md"),
            "# API\n\nThe public signature is `fn ready() -> bool`.\n",
        )
        .unwrap();
        let mut digests = std::collections::HashSet::new();
        let mut systems = std::collections::HashSet::new();
        for has_impl in [true, false] {
            for has_test in [true, false] {
                let anchor = if has_impl { ":impl: api.md\n" } else { "" };
                let marker = if has_test {
                    "// {test}`requirements.ready`\n"
                } else {
                    ""
                };
                fs::write(root.join("md/requirements.md"), format!(":::{{req}} ready\n{anchor}Document the public signature and return true when ready.\n:::\n")).unwrap();
                fs::write(
                    root.join("src/lib.rs"),
                    format!("{marker}fn checks_readiness() {{ assert!(ready()); }}\n"),
                )
                .unwrap();
                let sources = AssessmentSources::prepare(root, "requirements.ready").unwrap();
                for (kind, own, other, present, instruction) in [
                    (
                        ReviewKind::Structural,
                        "api.md",
                        "src/lib.rs",
                        has_impl,
                        "Without implementation edges",
                    ),
                    (
                        ReviewKind::Behavioral,
                        "src/lib.rs",
                        "api.md",
                        has_test,
                        "Without test edges",
                    ),
                ] {
                    let prompt = criterion_prompt(&sources, kind).unwrap();
                    assert_eq!(prompt.subject.contains(own), present);
                    assert!(!prompt.subject.contains(other));
                    assert!(prompt.system.contains(instruction));
                    systems.insert(prompt.system);
                }
                assert!(digests.insert(sources.reference_set));
            }
        }
        assert_eq!(systems.len(), 2);
    }

    // {test}`requirements.authoring.structural_inputs`
    // {test}`requirements.authoring.structural_review`
    // {test}`requirements.authoring.behavioral_review`
    // {test}`requirements.authoring.read_denial.mandatory`
    #[test]
    fn structural_roots_obey_mandatory_access_and_resolution_rules() {
        let root = inputs::tests::project();
        fs::write(
            root.path().join("md/requirements.md"),
            ":::{req} ready\n:impl: src/lib.rs#fn[api]\nStructure.\n:::\n",
        )
        .unwrap();
        let role = inputs::tests::role("impl", "requirements.ready");
        fs::write(
            root.path().join("src/lib.rs"),
            format!("// {role}\nfn api() {{}}\n"),
        )
        .unwrap();
        fs::write(root.path().join(".gitignore"), "src/lib.rs\n").unwrap();
        let error = AssessmentSources::prepare(root.path(), "requirements.ready")
            .err()
            .unwrap();
        assert!(error.to_string().contains("excluded"), "{error}");
        fs::remove_file(root.path().join(".gitignore")).unwrap();
        fs::write(
            root.path().join("md/requirements.md"),
            ":::{req} ready\n:impl: src/lib.rs#fn[missing]\nStructure.\n:::\n",
        )
        .unwrap();
        let error = AssessmentSources::prepare(root.path(), "requirements.ready")
            .err()
            .unwrap();
        assert!(error.to_string().contains("missing"), "{error}");
        fs::create_dir_all(root.path().join("evidence")).unwrap();
        fs::write(
            root.path().join("evidence/generated.rs"),
            "fn generated() {}\n",
        )
        .unwrap();
        fs::write(
            root.path().join("md/requirements.md"),
            ":::{req} ready\n:impl: evidence/generated.rs\nStructure.\n:::\n",
        )
        .unwrap();
        fs::write(root.path().join("src/lib.rs"), "fn unreferenced() {}\n").unwrap();
        let allowed = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        assert!(allowed.review_roots.is_some());
        assert!(
            allowed
                .mandatory_inputs()
                .iter()
                .any(|input| input["file"] == "evidence/generated.rs")
        );
    }

    // {test}`requirements.authoring.submission`
    // {test}`requirements.authoring.structural_review`
    // {test}`requirements.authoring.behavioral_review`
    #[tokio::test]
    async fn each_criterion_rejects_markers_belonging_to_its_peer() {
        let root = inputs::tests::project();
        let mut sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut usage = UsageLog::start(state.path(), root.path(), "requirements.ready").unwrap();
        let misplaced = |role| {
            (
                "submit_result",
                json!({"result":{"status":"failed","corrections":[{"kind":"add_reference","role":role,"file":"src/lib.rs","selector":"fn[ready]","rationale":"Missing marker for the other criterion."}]}}),
            )
        };
        let provider = RecordingProvider::script(
            vec![
                misplaced("test"),
                judgment(json!(true), "No structural obligations."),
                misplaced("impl"),
                judgment(json!(true), "The tests cover readiness."),
            ],
            true,
        );
        let assessment = assess(&provider, &mut sources, &mut usage).await.unwrap();
        assert_eq!(assessment.determination, Judgment::True);
        assert!(assessment.findings.is_empty());
        let prompts = provider.prompts();
        assert_eq!(prompts.len(), 4);
        assert!(
            prompts[1]
                .0
                .contains("structural criterion cannot propose a {test} marker")
        );
        assert!(!prompts[2].0.contains("No structural obligations."));
        assert!(
            prompts[3]
                .0
                .contains("behavioral criterion cannot propose a {impl} marker")
        );
        for (role, label) in [("test", "structural"), ("impl", "behavioral")] {
            let mut turns = Vec::new();
            if label == "behavioral" {
                turns.push(judgment(json!(true), "No structural obligations."));
            }
            turns.extend([misplaced(role), misplaced(role)]);
            let provider = RecordingProvider::script(turns, true);
            let error = assess(&provider, &mut sources, &mut usage)
                .await
                .unwrap_err();
            let expected = format!("{label} criterion cannot propose a {{{role}}} marker");
            assert!(error.to_string().contains(&expected), "{error}");
            assert!(provider.prompts().last().unwrap().0.contains(&expected));
        }
    }

    // {test}`requirements.ci.outcomes`
    // {test}`requirements.authoring.submission`
    #[tokio::test]
    async fn combined_judgment_retains_both_criteria_and_prioritizes_failures() {
        let root = inputs::tests::project();
        let state = tempfile::tempdir().unwrap();
        let structural_gap = json!({"FIXME":["Structural gap"]});
        let behavioral_gap = json!({"FIXME":["Behavioral gap"]});
        for (structural, behavioral, expected) in [
            (structural_gap.clone(), json!(false), json!(false)),
            (structural_gap.clone(), json!(true), structural_gap.clone()),
            (
                structural_gap,
                behavioral_gap,
                json!({"FIXME":["Structural gap", "Behavioral gap"]}),
            ),
            (json!(false), json!(true), json!(false)),
        ] {
            let first = judgment(structural, "Structural assessment.");
            let second = judgment(behavioral, "Behavioral assessment.");
            let provider = RecordingProvider::script(vec![first.clone(), second.clone()], true);
            let mut sources =
                AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
            let mut usage =
                UsageLog::start(state.path(), root.path(), "requirements.ready").unwrap();
            let assessment = assess(&provider, &mut sources, &mut usage).await.unwrap();
            assert_eq!(assessment.record["determination"], expected);
            assert_eq!(
                assessment.record["criteria"]["structural"]["output"],
                first.1["result"]
            );
            assert_eq!(
                assessment.record["criteria"]["behavioral"]["output"],
                second.1["result"]
            );
        }
    }

    // {test}`requirements.authoring.submission`
    // {test}`requirements.sdk.habeas.assessment_policy`
    // {test}`requirements.authoring.structural_prompt`
    // {test}`requirements.authoring.structural_review`
    // {test}`requirements.authoring.confirmation`
    // {test}`requirements.authoring.behavioral_review`
    // {test}`requirements.ci.outcomes`
    // {test}`requirements.ci.review_criteria`
    #[tokio::test]
    async fn structural_assessments_and_confirmation_use_the_same_structural_contract() {
        let root = inputs::tests::project();
        fs::write(root.path().join("md/requirements.md"), ":::{req} ready\nThe project MUST be ready. {cite}`requirements.context`\n:::\n:::{req} context\nReadiness is shared by both callers.\n:::\n").unwrap();
        let role = inputs::tests::role("impl", "requirements.ready");
        fs::write(
            root.path().join("src/lib.rs"),
            format!("// {role}\n// :::{{fixme}} requirements.ready\n// The remaining structural gap.\n// :::\npub fn api() {{}}\n// {{test}}`requirements.ready`\n#[test]\nfn checks_ready() {{}}\n"),
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let initial = initial_prompt(&sources).unwrap();
        for text in [
            "stated structural obligations",
            "Inspect the supplied implementation roots",
            "pub fn api()",
        ] {
            assert!(initial.joined().contains(text), "missing {text}");
        }
        assert!(!initial.system.contains("Testing criteria:"));
        assert!(!initial.joined().contains("{{"));
        assert!(
            initial
                .subject
                .starts_with("# Subject\n\nSubject: requirement requirements.ready\n")
        );
        let acknowledgment = sources.model_acknowledgments();
        assert_eq!(acknowledgment.len(), 1);
        assert!(
            acknowledgment[0]["body"]
                .as_str()
                .unwrap()
                .contains("The remaining structural gap.")
        );
        assert_eq!(
            serde_json::from_str::<Value>(
                prompt_section(&initial.subject, "Acknowledgments").trim()
            )
            .unwrap(),
            json!(acknowledgment)
        );
        let provider = RecordingProvider::script(
            vec![
                judgment(json!(true), "The API exposes the required signature."),
                judgment(json!(true), "No behavioral obligation."),
                judgment(json!(false), "The required signature is absent."),
                judgment(json!(true), "No behavioral obligation."),
            ],
            true,
        );
        let state = tempfile::tempdir().unwrap();
        let mut usage = UsageLog::start(state.path(), root.path(), "requirements.ready").unwrap();
        let subject = AuthoringSubject::Definition("requirements.ready".into());
        let Assessment {
            determination: answer,
            record,
            ..
        } = assess_subject(
            &provider,
            root.path(),
            state.path(),
            &subject,
            sources,
            &mut usage,
            AssessmentPolicy::Confirm,
        )
        .await
        .unwrap();
        assert_eq!(answer, Judgment::False);
        assert!(
            record["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|input| input["item"] == "fn[api]")
        );
        let prompts = provider.prompts();
        assert_eq!(prompts.len(), 4);
        assert_eq!(
            record["criteria"]["structural"]["output"]["status"],
            "failed"
        );
        assert_eq!(
            record["criteria"]["behavioral"]["output"]["status"],
            "success"
        );
        use trust_but_verify::domain::Domain;
        let graph = crate::sdk::HabeasDomain::discover(root.path()).unwrap();
        let node = graph
            .cache()
            .intern_node(crate::sdk::HabeasNode::review_subject(&subject));
        crate::evidence_format::write_atomic(&subject.evidence_path(root.path()), &record).unwrap();
        for name in ["structural", "behavioral"] {
            let path = trust_but_verify::graph::CriterionPath::new(vec![name.into()]);
            let loaded = graph.load_evidence(&node, &path).await.unwrap().unwrap();
            assert_eq!(loaded.output, record["criteria"][name]["output"]);
            assert_eq!(
                serde_json::to_string(&loaded.dependencies)
                    .unwrap()
                    .contains("fn[api]"),
                name == "structural"
            );
        }
        let mut incomplete = record.clone();
        incomplete.as_object_mut().unwrap().remove("criteria");
        assert!(crate::evidence_format::view(&incomplete).is_err());
        for (index, (prompt, _)) in prompts.iter().enumerate() {
            let structural = index % 2 == 0;
            provider.assert_tools(index);
            assert_eq!(
                prompt.contains("Inspect the supplied implementation roots"),
                structural
            );
            assert!(prompt.contains("The project MUST be ready."));
            assert!(prompt.contains("Readiness is shared by both callers."));
            assert_eq!(prompt.contains("fn checks_ready()"), !structural);
            assert_eq!(
                prompt.contains(approved_text("behavioral_review", 0)),
                !structural
            );
            assert_eq!(prompt.contains("pub fn api()"), structural);
            assert_eq!(prompt.contains("Testing criteria:"), !structural);
            for prior_verdict in [
                "The API exposes the required signature.",
                "No behavioral obligation.",
                "The required signature is absent.",
            ] {
                assert!(
                    !prompt.contains(prior_verdict),
                    "request {index} contains a prior verdict: {prior_verdict}"
                );
            }
            assert!(prompt.contains("The remaining structural gap."));
        }
    }
}

async fn assess(
    provider: &config::BackendConfig,
    sources: &mut AssessmentSources,
    usage: &mut UsageLog,
) -> io::Result<Assessment> {
    sdk::assess(provider, sources, usage).await
}

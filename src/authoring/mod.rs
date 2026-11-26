//! Model-assisted evidence authoring, invoked explicitly and never by validation.

pub(crate) mod access;
mod adaptive;
mod batch;
pub(crate) mod inputs;
mod line_numbers;
mod pages;
mod progress;
mod providers;
mod sdk;
mod searches;
mod tools;
mod usage;

pub use batch::{BatchOutcome, author_all, author_all_with_options, author_sites_with_options};

use rig_core::completion::{CompletionRequest, ToolDefinition};
use rig_core::message::{Message, ToolResultContent, UserContent};
use serde_json::{Value, json};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use tokio::sync::watch;

use crate::DefinitionKind;
use crate::config::{self, AssessmentPolicy, BackendKind, ResolvedAuthoringConfig};
use crate::judgment::Judgment;
use inputs::{AssessmentKind, AssessmentSources};
use providers::{CompletionProvider, RigProvider};
use usage::UsageLog;

// {impl}`requirements.authoring.invocation`
pub struct AuthoringOutcome {
    pub subject: String,
    pub backend: String,
    pub model: String,
    pub evidence_path: Option<PathBuf>,
    pub usage_path: PathBuf,
    pub determination: Judgment,
    pub rationale: String,
    /// The assessment policy the subject was assessed under.
    // {impl}`requirements.authoring.policy`
    pub policy: AssessmentPolicy,
}

/// The opening of the note a recorded rationale ends with when its
/// assessments disagreed under the `confirm` policy.
// {impl}`requirements.cli.status`
pub(crate) const DISAGREEMENT_NOTE: &str = "**Confirmation:** the assessments disagreed";

// {impl}`requirements.authoring.policy`
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

// :::{impl} requirements.authoring.invocation
// The `Definition` variant is the one requirement or named example given on the
// command line: the subject the command assesses and reports.
// :::
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
    // {impl}`requirements.authoring.invocation`
    // {impl}`requirements.references.citation_review`
    // {impl}`requirements.references.site_review`
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
    // {impl}`requirements.authoring.persistence`
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
    // {impl}`requirements.authoring.mandatory_inputs`
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
    usage: UsageLog,
}

impl PendingAssessment {
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

    // {impl}`requirements.authoring.usage_storage`
    // {impl}`requirements.configuration.state_location`
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

/// Authors one subject's evidence file under `--missing`, returning `None`
/// when its evidence is current.
// {impl}`requirements.authoring.invocation`
// {impl}`requirements.authoring.refresh`
pub async fn author_evidence(
    root: &Path,
    subject: &str,
    selection: Option<&str>,
) -> io::Result<Option<AuthoringOutcome>> {
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
// {impl}`requirements.authoring.invocation`
// {impl}`requirements.configuration.state_location`
// {impl}`requirements.authoring.policy`
// {impl}`requirements.authoring.read_denial.mandatory`
// {impl}`requirements.authoring.provider_backoff`
// {impl}`requirements.references.citation_review`
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
    result.map(Some).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("{error}; usage: {}", usage.path.display()),
        )
    })
}

/// Resolves the usage state root, refusing a location inside the project
/// before any state directory is created.
// {impl}`requirements.configuration.state_location`
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

// {impl}`requirements.authoring.usage_storage`
// {impl}`requirements.configuration.state_location`
// {impl}`requirements.authoring.policy`
// {impl}`requirements.authoring.provider_backoff`
pub(crate) async fn prepare_assessment(
    domain: crate::sdk::HabeasDomain,
    subject: &AuthoringSubject,
    config: &ResolvedAuthoringConfig,
    policy: AssessmentPolicy,
    mut cancellation: watch::Receiver<Option<&'static str>>,
    first_request_finished: Option<Box<dyn FnOnce() + Send>>,
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
    if let Some(finished) = first_request_finished {
        usage.on_first_request(finished);
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
                policy,
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
// {impl}`requirements.authoring.rig_configuration_mapping`
// {impl}`requirements.authoring.persistence`
// {impl}`requirements.configuration.loading`
// {impl}`requirements.authoring.policy`
// {impl}`requirements.authoring.configuration`
// {impl}`requirements.authoring.submission`
async fn configured_assessment(
    root: &Path,
    subject: &str,
    selection: Option<&str>,
    output: EvidenceOutput,
    policy: Option<AssessmentPolicy>,
    usage: &mut UsageLog,
) -> io::Result<AuthoringOutcome> {
    let config = config::resolve_authoring(root, selection)?;
    let policy = config.effective_policy(policy);
    let parsed = AuthoringSubject::parse(subject);
    let (determination, evidence) = configured_record(
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
        policy,
    })
}

// {impl}`requirements.authoring.invocation`
// {impl}`requirements.authoring.mandatory_inputs`
// {impl}`requirements.authoring.usage_inputs`
// {impl}`requirements.authoring.provider`
// {impl}`requirements.authoring.policy`
// {impl}`requirements.authoring.read_denial.mandatory`
// {impl}`requirements.authoring.adaptive`
async fn configured_record(
    domain: crate::sdk::HabeasDomain,
    subject: &AuthoringSubject,
    config: &ResolvedAuthoringConfig,
    policy: AssessmentPolicy,
    usage: &mut UsageLog,
) -> io::Result<(Judgment, Value)> {
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
    assess_subject(&provider, root, &state, subject, sources, usage, policy).await
}

// {impl}`requirements.authoring.usage_storage`
/// The backend name usage records give a model's backend.
fn backend_name(model: &config::ModelRef) -> &'static str {
    match model.backend {
        BackendKind::Bedrock => "bedrock",
        BackendKind::Ollama => "ollama",
        BackendKind::Openai => "openai",
    }
}

/// Assesses a subject and, under the `confirm` policy, confirms a `true` or FIXME result
/// in an independent second conversation, verifying the less favorable of
/// disagreeing assessments in a third. Each conversation has its own usage
/// record; `usage` is the first's.
// {impl}`requirements.authoring.verifier_outcome`
// {impl}`requirements.authoring.mandatory_inputs`
// {impl}`requirements.authoring.policy`
async fn assess_subject(
    provider: &impl CompletionProvider,
    root: &Path,
    state: &Path,
    subject: &AuthoringSubject,
    mut sources: AssessmentSources,
    usage: &mut UsageLog,
    policy: AssessmentPolicy,
) -> io::Result<(Judgment, Value)> {
    usage.record.subject_kind = Some(sources.subject_kind.noun().into());
    usage.prepared("mandatory", sources.mandatory_inputs());
    usage.save()?;
    let first = assess(provider, &mut sources, usage).await?;
    // Under `once`, the single assessment is recorded.
    if policy == AssessmentPolicy::Once || first.0 == Judgment::False {
        return Ok(first);
    }
    let mut log = followup_log(state, root, subject, usage, "second assessment")?;
    let result = match followup_sources(&sources.domain, subject, &mut log) {
        Ok(mut sources) => assess(provider, &mut sources, &mut log).await,
        Err(error) => Err(error),
    };
    let second = finish_followup(log, "second assessment", result)?;
    // A site review's assessments also disagree when only `applies` differs.
    let applies = |assessment: &(Judgment, Value)| assessment.1["applies"].as_bool();
    if first.0.kind() == second.0.kind() && applies(&first) == applies(&second) {
        let mut first = first;
        merge_review_inputs(&mut first.1, &[&second.1], &[], &[]);
        return Ok(first);
    }
    let describe = |assessment: &(Judgment, Value)| match applies(assessment) {
        Some(applies) => format!("{}` with applies `{applies}", assessment.0.kind()),
        None => assessment.0.kind().to_owned(),
    };
    let first_kind = describe(&first);
    let second_kind = describe(&second);
    let (less, more) = if favorability(&second) < favorability(&first) {
        (second, first)
    } else {
        (first, second)
    };
    let less_judgment = less.0.clone();
    let less_kind = describe(&less);
    let less_rationale = less.1["rationale"].as_str().unwrap_or_default().to_owned();
    let mut log = followup_log(state, root, subject, usage, "verification")?;
    let result = match followup_sources(&sources.domain, subject, &mut log) {
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
    let verification = finish_followup(log, "verification", result)?;
    let chosen = confirmed_record(
        less,
        more,
        &first_kind,
        &second_kind,
        &less_kind,
        verification,
    );
    usage.record.determination = Some(chosen.0.kind().into());
    usage.save()?;
    Ok(chosen)
}

// {cite}`habeas:util`
fn confirmed_record(
    less: (Judgment, Value),
    more: (Judgment, Value),
    first_kind: &str,
    second_kind: &str,
    less_kind: &str,
    verification: Verification,
) -> (Judgment, Value) {
    let dependent = [less.1.clone(), more.1.clone()];
    let applies = |assessment: &(Judgment, Value)| assessment.1["applies"].as_bool();
    let (mut chosen, conclusion) = if !verification.real {
        // A verifier deciding `applies` otherwise than the more favorable
        // assessment establishes `true` on its own rationale.
        match verification.applies {
            Some(verified) if applies(&more) != Some(verified) => {
                let mut record = more.1;
                record["determination"] = Judgment::True.to_value();
                record["rationale"] = Value::String(verification.rationale.clone());
                (
                    (Judgment::True, record),
                    "found none of its claimed gaps real and decided applies otherwise",
                )
            }
            _ => (more, "found none of its claimed gaps real"),
        }
    } else {
        let (judgment, conclusion) = if verification.acknowledged {
            (
                Judgment::Fixme(verification.acknowledgments.clone()),
                "found its real gaps all acknowledged",
            )
        } else {
            (Judgment::False, "found an unacknowledged real gap")
        };
        let mut record = less.1;
        record["determination"] = judgment.to_value();
        ((judgment, record), conclusion)
    };
    // A site review records the verifier's `applies`.
    if let Some(verified) = verification.applies {
        chosen.1["applies"] = Value::Bool(verified);
    }
    let note = format!(
        "{DISAGREEMENT_NOTE}: the first determination was `{first_kind}` and an independent second assessment's was `{second_kind}`. A verifier checked the less favorable `{less_kind}` determination and {conclusion}, so `{}` is recorded.\n\nVerification:\n\n{}",
        chosen.0.kind(),
        verification.rationale.trim()
    );
    let rationale = format!(
        "{}\n\n{note}",
        chosen.1["rationale"].as_str().unwrap_or_default().trim()
    );
    chosen.1["rationale"] = Value::String(rationale);
    merge_review_inputs(
        &mut chosen.1,
        &dependent.iter().collect::<Vec<_>>(),
        &verification.dependencies,
        &verification.inputs,
    );
    chosen
}

// {cite}`habeas:util`
fn merge_review_inputs(
    value: &mut Value,
    assessments: &[&Value],
    extra: &[trust_but_verify::graph::Dependency<
        crate::sdk::HabeasNode,
        crate::sdk::HabeasEdgeLabel,
    >],
    extra_inputs: &[Value],
) {
    for field in ["dependencies", "inputs"] {
        let mut all = value[field].as_array().cloned().unwrap_or_default();
        for source in assessments
            .iter()
            .flat_map(|record| record[field].as_array().into_iter().flatten())
        {
            if !all.contains(source) {
                all.push(source.clone());
            }
        }
        let added = if field == "dependencies" {
            serde_json::to_value(extra).expect("dependency JSON")
        } else {
            Value::Array(extra_inputs.to_vec())
        };
        for source in added.as_array().expect("array") {
            if !all.contains(source) {
                all.push(source.clone());
            }
        }
        if field == "inputs" || value.get("format_version").is_some() {
            value[field] = Value::Array(all);
        }
    }
}

/// Orders assessments: `false` is less favorable than FIXME, and FIXME
/// than `true`; between site reviews with the same determination, the one
/// with `applies` true is less favorable.
fn favorability(assessment: &(Judgment, Value)) -> (u8, bool) {
    let kind = match assessment.0 {
        Judgment::False => 0,
        Judgment::Fixme(_) => 1,
        Judgment::True => 2,
    };
    (kind, assessment.1["applies"] == Value::Bool(false))
}

/// Starts the usage record of a further conversation about a subject.
// {impl}`requirements.authoring.policy`
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
    usage.prepared("mandatory", sources.mandatory_inputs());
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
// {impl}`requirements.authoring.initial_prompt`
// {impl}`requirements.authoring.habeas_background`
// {impl}`requirements.authoring.judgment_rules`
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
// {impl}`requirements.authoring.habeas_background`
// {impl}`requirements.authoring.judgment_rules`
fn shared_blocks() -> [(&'static str, &'static str); 3] {
    [
        ("habeas_background", approved_text("habeas_background", 0)),
        ("reference_roles", approved_text("habeas_background", 1)),
        ("judgment_rules", approved_text("judgment_rules", 0)),
    ]
}

/// The judgment rules' guidance on citing source locations, which a
/// rejection for citing line numbers repeats.
// {impl}`requirements.authoring.line_numbers.rejection`
fn location_guidance() -> &'static str {
    let rules = approved_text("judgment_rules", 0);
    let start = rules
        .find("- Give a source location")
        .expect("location guidance");
    let rest = &rules[start..];
    rest.find("\n- ").map_or(rest, |end| &rest[..end])
}

/// The account of distributed requirements that the site prompt, the
/// distributed task, and a site review's verifier receive.
// {impl}`requirements.authoring.habeas_background`
fn distributed_background() -> &'static str {
    approved_text("habeas_background", 2)
}

/// Replaces `{{name}}` placeholders in one pass, so substituted text that
/// itself contains a placeholder, such as a requirement quoting a template,
/// is left as written.
// {impl}`requirements.authoring.initial_prompt`
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
// {impl}`requirements.authoring.initial_prompt`
// {impl}`requirements.authoring.requirement_task`
// {impl}`requirements.authoring.testing_bar`
// {impl}`requirements.authoring.example_task`
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
// {impl}`requirements.authoring.initial_prompt`
// {impl}`requirements.authoring.distributed_task`
// {impl}`requirements.authoring.habeas_background`
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
    // {impl}`requirements.authoring.initial_prompt`
    fn filled(name: &str, values: &[(&str, &str)]) -> Self {
        Self {
            system: fill(approved_text(name, 0), values),
            subject: fill(approved_text(name, 1), values),
        }
    }

    /// The opening request: the system prompt as a leading system message,
    /// then the subject message as the user's.
    fn request(self) -> CompletionRequest {
        CompletionRequest::new(Message::user(self.subject)).preamble(self.system)
    }

    /// Both parts, for tests that look for text wherever it is sent.
    // {test}`requirements.authoring.prompt_parts`
    #[cfg(test)]
    fn joined(&self) -> String {
        format!("{}\n\n{}", self.system, self.subject)
    }
}

// {impl}`requirements.authoring.initial_prompt`
// {impl}`requirements.ci.review_criteria`
// {impl}`requirements.ci.outcomes`
// {impl}`requirements.examples.evidence`
// {impl}`requirements.examples.definition`
// {impl}`requirements.authoring.mandatory_inputs`
// {impl}`requirements.authoring.citation_prompt`
// {impl}`requirements.authoring.redaction`
// {impl}`requirements.authoring.github_reads`
// {impl}`requirements.authoring.site_prompt`
// {impl}`requirements.authoring.habeas_background`
// {impl}`requirements.authoring.judgment_rules`
// {impl}`requirements.authoring.credentials`
// {impl}`requirements.authoring.structural_prompt`
fn initial_prompt(sources: &AssessmentSources) -> io::Result<Prompt> {
    initial_prompt_with_inputs(sources, &sources.model_mandatory_inputs())
}

// :::{fixme} requirements.ci.review_criteria
// :github: nikomatsakis/habeas-codicem#51
// One prompt is selected rather than ensuring independent structural and
// behavioral checks. Mixed requirements receive only the behavioral task; absent
// edge kinds do not each receive an explicit absence check. Tests exercise the
// exclusive prompt paths and confirmation, not the full independent-check matrix.
// :::
// :::{fixme} requirements.ci.outcomes
// :github: nikomatsakis/habeas-codicem#51
// The current single-task result does not ensure both structural and behavioral
// conclusions before accepting true. Regression tests cover existing outcomes,
// but do not yet reject an overall success when the omitted check would find a gap.
// :::
fn initial_prompt_with_inputs(sources: &AssessmentSources, inputs: &[Value]) -> io::Result<Prompt> {
    let mandatory = serde_json::to_string_pretty(inputs)?;
    let mut values = shared_blocks().to_vec();
    values.push(("mandatory_inputs", &mandatory));
    Ok(match &sources.subject_kind {
        AssessmentKind::Definition(kind, distributed) => {
            let acknowledgments = serde_json::to_string_pretty(&sources.model_acknowledgments())?;
            let task = subject_task(kind);
            let distributed = distributed_task(kind, *distributed);
            values.extend([
                ("subject_kind", sources.subject_kind.noun()),
                ("subject_name", &sources.subject_name),
                ("subject_task", &task),
                ("distributed_task", &distributed),
                ("acknowledgments", &acknowledgments),
            ]);
            Prompt::filled(
                if sources.structural {
                    "structural_prompt"
                } else {
                    "initial_prompt"
                },
                &values,
            )
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

// {impl}`requirements.authoring.verifier_prompt`
/// The body of a filled prompt's `# heading` section, up to the next heading.
fn prompt_section<'a>(prompt: &'a str, heading: &str) -> &'a str {
    let opening = format!("# {heading}\n\n");
    let Some(start) = prompt.find(&opening) else {
        return "";
    };
    let body = &prompt[start + opening.len()..];
    body.find("\n\n# ").map_or(body, |end| &body[..end])
}

/// The criteria an assessment applied, for its verifier: its subject message
/// after the "# Subject" heading and before "# Mandatory inputs", then, for a
/// site review, the account of distributed requirements, and then its "Your
/// task" section.
// {impl}`requirements.authoring.habeas_background`
fn review_instructions(prompt: &Prompt, site: bool) -> String {
    let named = prompt_section(&prompt.subject, "Subject").trim();
    let background = if site {
        format!("{}\n\n", distributed_background())
    } else {
        String::new()
    };
    format!(
        "{named}\n\n{background}{}",
        prompt_section(&prompt.system, "Your task")
    )
}

/// The verifier's added step for a site review.
const SITE_STEP: &str = "4. Also set applies to whether the requirement governs the site, as the reviewer's step 1 decides it; if it does not, set real to false.";

/// The verifier's prompt for a determination and its rationale.
// {impl}`requirements.authoring.judgment_rules`
// {impl}`requirements.authoring.github_reads`
// {impl}`requirements.authoring.habeas_background`
// {impl}`requirements.authoring.prompt_parts`
fn verification_prompt(
    sources: &AssessmentSources,
    determination: &Judgment,
    rationale: &str,
) -> io::Result<Prompt> {
    verification_prompt_with_inputs(
        sources,
        determination,
        rationale,
        &sources.model_mandatory_inputs(),
    )
}

fn verification_prompt_with_inputs(
    sources: &AssessmentSources,
    determination: &Judgment,
    rationale: &str,
    inputs: &[Value],
) -> io::Result<Prompt> {
    let site = matches!(sources.subject_kind, AssessmentKind::Site(..));
    let instructions = review_instructions(&initial_prompt_with_inputs(sources, inputs)?, site);
    let mandatory = serde_json::to_string_pretty(inputs)?;
    let determination = determination.to_value().to_string();
    let mut values = shared_blocks().to_vec();
    if site {
        // A site review's verifier gets no reference roles.
        values.retain(|(name, _)| *name != "reference_roles");
        values.push(("reference_roles", ""));
    }
    values.extend([
        ("subject_kind", sources.subject_kind.noun()),
        ("subject_name", sources.subject_name.as_str()),
        ("determination", &determination),
        ("rationale", rationale.trim()),
        ("review_instructions", &instructions),
        ("site_step", if site { SITE_STEP } else { "" }),
        ("mandatory_inputs", &mandatory),
    ]);
    Ok(Prompt::filled("verifier_prompt", &values))
}

// {cite}`habeas:util`
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

/// A submitted site review: its judgment, rationale, and `applies`.
/// `applies` false requires a `true` determination.
// {impl}`requirements.authoring.site_prompt`
fn site_submission(arguments: &Value) -> io::Result<(Judgment, String, bool)> {
    let object = arguments
        .as_object()
        .ok_or_else(|| io::Error::other("submit_judgment expects an object"))?;
    if object.len() != 3 || !object.contains_key("applies") {
        return Err(io::Error::other(
            "submit_judgment requires exactly applies, determination, and rationale",
        ));
    }
    let applies = object["applies"]
        .as_bool()
        .ok_or_else(|| io::Error::other("submit_judgment applies must be a boolean"))?;
    let mut rest = object.clone();
    rest.remove("applies");
    let (judgment, rationale) = submission(&Value::Object(rest))?;
    if !applies && judgment != Judgment::True {
        return Err(io::Error::other(
            "submit_judgment determination must be true when applies is false",
        ));
    }
    Ok((judgment, rationale, applies))
}

// {impl}`requirements.authoring.verifier_outcome`
// {impl}`requirements.authoring.verifier_prompt`
/// A verifier's findings about the claimed gaps of an assessment, and, for a
/// site review, whether the requirement applies to the site.
#[derive(Debug, PartialEq)]
struct Verification {
    real: bool,
    acknowledged: bool,
    acknowledgments: Vec<String>,
    rationale: String,
    applies: Option<bool>,
    dependencies: Vec<
        trust_but_verify::graph::Dependency<crate::sdk::HabeasNode, crate::sdk::HabeasEdgeLabel>,
    >,
    inputs: Vec<Value>,
}

/// Parses a `submit_verification` call: exactly booleans `real` and
/// `acknowledged`, an array of nonempty `acknowledgments`, which must be
/// nonempty when both are true, and a nonempty `rationale`.
fn verification(arguments: &Value) -> io::Result<Verification> {
    const FIELDS: [&str; 4] = ["real", "acknowledged", "acknowledgments", "rationale"];
    let invalid = |message: &str| io::Error::other(format!("submit_verification {message}"));
    let object = arguments
        .as_object()
        .ok_or_else(|| invalid("expects an object"))?;
    if object.len() != FIELDS.len() || !FIELDS.iter().all(|field| object.contains_key(*field)) {
        return Err(invalid(
            "requires exactly real, acknowledged, acknowledgments, and rationale",
        ));
    }
    let flag = |field: &str| {
        object[field]
            .as_bool()
            .ok_or_else(|| invalid(&format!("{field} must be a boolean")))
    };
    let (real, acknowledged) = (flag("real")?, flag("acknowledged")?);
    let acknowledgments = object["acknowledgments"]
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .map(|item| {
                    item.as_str()
                        .filter(|text| !text.trim().is_empty())
                        .map(str::to_owned)
                })
                .collect::<Option<Vec<_>>>()
        })
        .ok_or_else(|| invalid("acknowledgments must be an array of nonempty strings"))?;
    if real && acknowledged && acknowledgments.is_empty() {
        return Err(invalid(
            "acknowledgments must be nonempty when real and acknowledged are true",
        ));
    }
    let rationale = object["rationale"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| invalid("rationale must be nonempty Markdown"))?;
    Ok(Verification {
        real,
        acknowledged,
        acknowledgments,
        rationale: rationale.to_owned(),
        applies: None,
        dependencies: Vec::new(),
        inputs: Vec::new(),
    })
}

/// Parses a site review's `submit_verification` call: the four fields and a
/// boolean `applies`; `applies` false with `real` true is invalid.
fn site_verification(arguments: &Value) -> io::Result<Verification> {
    let invalid = |message: &str| io::Error::other(format!("submit_verification {message}"));
    let object = arguments
        .as_object()
        .ok_or_else(|| invalid("expects an object"))?;
    let applies = object
        .get("applies")
        .ok_or_else(|| {
            invalid("requires exactly real, acknowledged, acknowledgments, rationale, and applies")
        })?
        .as_bool()
        .ok_or_else(|| invalid("applies must be a boolean"))?;
    let mut rest = object.clone();
    rest.remove("applies");
    let mut verification = verification(&Value::Object(rest))?;
    if !applies && verification.real {
        return Err(invalid("real must be false when applies is false"));
    }
    verification.applies = Some(applies);
    Ok(verification)
}

/// The source tools and one submission tool.
// {impl}`requirements.authoring.initial_prompt`
// {impl}`requirements.ci.outcomes`
// {impl}`requirements.authoring.site_prompt`
// {impl}`requirements.authoring.tool_descriptions.common_case`
fn tool_definitions_with(submit: &str, site: bool) -> io::Result<Vec<ToolDefinition>> {
    let mut definitions = tools::definitions();
    let mut submission = if submit == "submit_verification" {
        json!({"name": "submit_verification", "description": "Finish a gap verification by submitting its result until one submission is accepted, rather than using a source tool or a text message. real says whether the alleged gap is real; acknowledged says whether applicable FIXME comments acknowledge every real gap. acknowledgments lists their issue references, e.g. [\"owner/repo#123\"], or [] when none apply. rationale is a nonempty explanation grounded in the reviewed sources. Cite files and outline selectors, never line numbers, including in illustrative examples. Summarize diagnostic formats with placeholders rather than quoting numeric source locations. Close every delivered search with finish_search before submitting. A rejected submission leaves this verification open. A successful submission ends this verification and records these answers.",
            "parameters": {"type":"object", "additionalProperties": false, "required":["real","acknowledged","acknowledgments","rationale"], "properties": {
                "real":{"type":"boolean"},
                "acknowledged":{"type":"boolean"},
                "acknowledgments":{"type":"array","items":{"type":"string","minLength":1}},
                "rationale":{"type":"string","minLength":1}}}})
    } else {
        let determination = json!({"oneOf":[{"type":"boolean"},{"type":"object","additionalProperties":false,"required":["FIXME"],"properties":{"FIXME":{"type":"array","minItems":1,"items":{"type":"string","minLength":1}}}}]});
        json!({"name": "submit_judgment", "description": "Finish the assessment by submitting your final judgment until one submission is accepted, rather than using a source tool or a text message. determination is true when the subject meets its review criteria, false when it has an unacknowledged gap, or {\"FIXME\":[\"owner/repo#123\"]} naming the issues in applicable FIXME comments that acknowledge every gap. rationale is a nonempty explanation grounded in the reviewed sources. Cite files and outline selectors, never line numbers, including in illustrative examples. Summarize diagnostic formats with placeholders rather than quoting numeric source locations. Close every delivered search with finish_search before submitting. A rejected submission leaves this assessment open. A successful submission ends this assessment and records the judgment.",
            "parameters": {"type":"object", "additionalProperties": false, "required":["determination","rationale"], "properties": {
                "determination": determination,
                "rationale":{"type":"string","minLength":1}}}})
    };
    // A site review also records whether the requirement applies.
    if site {
        let description = submission["description"].as_str().unwrap().to_owned();
        submission["description"] = json!(format!(
            "{description} applies says whether this requirement applies to the reviewed site; when false, report no real gap{}.",
            if submit == "submit_verification" {
                " (real false)"
            } else {
                " (determination true)"
            }
        ));
        let parameters = &mut submission["parameters"];
        parameters["properties"]["applies"] = json!({"type": "boolean"});
        parameters["required"]
            .as_array_mut()
            .expect("required fields")
            .push(json!("applies"));
    }
    definitions.push(submission);
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
    tool_definitions_with("submit_judgment", false)
}

#[cfg(test)]
mod submission_description_tests {
    // {test}`requirements.authoring.tool_descriptions.common_case`
    // {test}`requirements.authoring.no_line_numbers`
    #[test]
    fn submission_descriptions_explain_arguments_and_successful_completion() {
        for tool in ["submit_judgment", "submit_verification"] {
            for site in [false, true] {
                let definitions = super::tool_definitions_with(tool, site).unwrap();
                let value = serde_json::to_value(definitions.last().unwrap()).unwrap();
                let description = value["description"].as_str().unwrap();
                for argument in value["parameters"]["properties"]
                    .as_object()
                    .unwrap()
                    .keys()
                {
                    assert!(
                        description.contains(argument),
                        "{tool} omits {argument}: {description}"
                    );
                }
                for phrase in [
                    "until one submission is accepted",
                    "source tool",
                    "nonempty explanation",
                    "outline selectors",
                    "never line numbers",
                    "A successful submission ends",
                    "owner/repo#123",
                ] {
                    assert!(
                        description.contains(phrase),
                        "{tool} omits {phrase}: {description}"
                    );
                }
                if tool == "submit_judgment" {
                    assert!(
                        description.contains("true when the subject meets its review criteria")
                    );
                    assert!(description.contains("false when it has an unacknowledged gap"));
                    assert!(description.contains(r#"{"FIXME":["owner/repo#123"]}"#));
                } else {
                    assert!(description.contains("real says whether the alleged gap is real"));
                    assert!(description.contains("acknowledged says whether"));
                    assert!(description.contains("or [] when none apply"));
                }
                assert_eq!(description.contains("applies says whether"), site);
            }
        }
    }
}

/// The record a judgment produces: definition evidence, or a citation
/// review with the item's file, selector, and citations.
// {impl}`requirements.authoring.persistence`
// {impl}`requirements.references.site_review`
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

/// Verifies a determination and its rationale in its own conversation,
/// returning the verifier's findings.
// {impl}`requirements.authoring.verifier_prompt`
async fn verify(
    provider: &impl CompletionProvider,
    sources: &mut AssessmentSources,
    usage: &mut UsageLog,
    determination: &Judgment,
    rationale: &str,
) -> io::Result<Verification> {
    sdk::verify(provider, sources, usage, determination, rationale).await
}

type InputObserver<'a> = dyn Fn(&[Value]) + Send + Sync + 'a;

/// One conversation: sends the prompt with the source tools and the named
/// submission tool, delivers tool results until a submission is accepted,
/// and returns the parsed submission.
// {impl}`requirements.authoring.delivered_inputs`
// {impl}`requirements.authoring.usage_requests`
// {impl}`requirements.authoring.mandatory_inputs`
// {impl}`requirements.authoring.provider_backoff`
// {impl}`requirements.authoring.submission_count`
// {impl}`requirements.authoring.source_tools`
// {impl}`requirements.authoring.file_discovery`
// {impl}`requirements.authoring.text_search`
// {impl}`requirements.definitions.authoring_inputs`
// {impl}`requirements.authoring.prompt_parts`
// {impl}`requirements.authoring.result_pages`
// {impl}`requirements.authoring.output_limit`
// {impl}`requirements.authoring.line_numbers.rejection`
// {impl}`requirements.authoring.line_numbers.resubmission`
// {impl}`requirements.authoring.submission`
// {impl}`requirements.authoring.no_line_numbers`
// {impl}`requirements.authoring.usage_requests.failures`
// {impl}`requirements.authoring.provider_backoff.limit`
// {impl}`requirements.authoring.tool_descriptions.special_cases`
// {impl}`requirements.authoring.cache_checkpoints`
// {impl}`requirements.authoring.submission.searches`
// {impl}`requirements.authoring.submission.searches.recovery`
// {impl}`requirements.authoring.submission.searches.line_numbers`
// {impl}`requirements.authoring.submission.searches.accompanying_calls`
async fn converse_observed<T: Send>(
    provider: &impl CompletionProvider,
    sources: &mut AssessmentSources,
    usage: &mut UsageLog,
    prompt: Prompt,
    (submit, site): (&str, bool),
    parse: impl Fn(&Value) -> io::Result<T> + Send,
    observer: Option<&InputObserver<'_>>,
) -> io::Result<T> {
    let pending = sources.mandatory_inputs().to_vec();
    let mut adapter = HabeasConversation {
        sources,
        usage,
        site,
        submit,
        parse,
        pending,
        observer,
        pages: pages::ResultPages::default(),
        rejection: line_numbers::Rejection::default(),
    };
    trust_but_verify::agent_runtime::converse(provider, &mut adapter, prompt.request(), submit)
        .await
}

// {cite}`habeas:util`
struct HabeasConversation<'a, F> {
    sources: &'a mut AssessmentSources,
    usage: &'a mut UsageLog,
    site: bool,
    submit: &'a str,
    parse: F,
    pending: Vec<Value>,
    observer: Option<&'a InputObserver<'a>>,
    pages: pages::ResultPages,
    rejection: line_numbers::Rejection,
}

impl<T: Send, F: Fn(&Value) -> io::Result<T> + Send> trust_but_verify::agent_runtime::Conversation
    for HabeasConversation<'_, F>
{
    type Output = T;

    // {cite}`habeas:util`
    fn tools(&self) -> io::Result<Vec<ToolDefinition>> {
        tool_definitions_with(self.submit, self.site)
    }
    fn submit(
        &mut self,
        submit: &str,
        arguments: &Value,
    ) -> io::Result<trust_but_verify::agent_runtime::Submission<T>> {
        use trust_but_verify::agent_runtime::Submission;
        let resubmitted = self.rejection.resubmitted(arguments);
        let result = (self.parse)(resubmitted.as_ref().unwrap_or(arguments))?;
        let outstanding = self.pages.outstanding_searches();
        if !outstanding.is_empty() {
            return Ok(Submission::Retry(format!(
                "Submission not accepted: outstanding searches: {}. Call finish_search for every listed search_id, using used:[] when no snippets qualify, then resubmit.",
                outstanding.join(", ")
            )));
        }
        if resubmitted.is_some() {
            return Ok(Submission::Accepted(result));
        }
        match self
            .rejection
            .decide(submit, arguments, location_guidance())
        {
            line_numbers::Outcome::Accept => Ok(Submission::Accepted(result)),
            line_numbers::Outcome::Fail(error) => Err(io::Error::other(error)),
            line_numbers::Outcome::Reject(message) => Ok(Submission::Retry(message)),
        }
    }
    async fn dispatch(&mut self, name: &str, arguments: &Value) -> io::Result<String> {
        let delivery = tools::dispatch_paged(self.sources, &mut self.pages, name, arguments)
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    line_numbers::without_source_lines(&error.to_string()),
                )
            })?;
        if name == "finish_search" {
            self.sources.record_inputs(&delivery.inputs);
            if let Some(observer) = self.observer {
                observer(&delivery.inputs);
            }
        } else {
            self.usage.prepared(name, &delivery.inputs);
            self.pending.extend(delivery.inputs);
        }
        self.usage.search_result(&delivery.result);
        Ok(delivery.result.to_string())
    }
    // {cite}`habeas:util`
    fn delivered(&mut self) -> io::Result<()> {
        self.sources.record_inputs(&self.pending);
        if let Some(observer) = self.observer {
            observer(&self.pending);
        }
        self.pending.clear();
        Ok(())
    }
    // {cite}`habeas:util`
    fn begin_request(&mut self) -> io::Result<usize> {
        self.usage.begin_request()
    }
    fn end_request(
        &mut self,
        index: usize,
        elapsed: std::time::Duration,
        response: &io::Result<rig_core::completion::CompletionResponse>,
    ) -> io::Result<()> {
        self.usage.end_request(
            index,
            elapsed.as_millis(),
            response.as_ref().ok().map(|r| &r.usage),
            response.as_ref().err().map(providers::failure_codes),
        )
    }
    // {cite}`habeas:util`
    fn retried(&mut self, index: usize) -> io::Result<()> {
        self.usage.retried(index)
    }
    // {cite}`habeas:util`
    fn skipped_tool(&mut self, name: &str) {
        self.usage.skipped_tool(name);
    }
    // {cite}`habeas:util`
    fn tool(&mut self, name: &str, failed: bool) {
        self.usage.tool(name, failed);
    }
    // {cite}`habeas:util`
    fn save(&mut self) -> io::Result<()> {
        self.usage.save()
    }
    // {cite}`habeas:util`
    fn cut_off(
        &mut self,
        response: &rig_core::completion::CompletionResponse,
        limit: u64,
    ) -> Vec<Message> {
        cut_off_continuation(response, limit, self.usage)
    }
}

/// The messages that continue a conversation after `response` was cut off
/// at `limit` output tokens: the response as it stands, then one user
/// message answering each of its incomplete calls without running it and
/// asking for a shorter response.
// {cite}`habeas:util`
fn cut_off_continuation(
    response: &rig_core::completion::CompletionResponse,
    limit: u64,
    usage: &mut UsageLog,
) -> Vec<Message> {
    let mut content = Vec::new();
    for call in response.tool_calls() {
        usage.skipped_tool(call.function.name.as_str());
        // Converse requires every call in history to have a result.
        content.push(UserContent::ToolResult(call.error_result(vec![
            ToolResultContent::text("not run: the call was cut off at the output limit"),
        ])));
    }
    content.push(UserContent::text(
        approved_text("output_limit", 0).replace("<limit>", &limit.to_string()),
    ));
    response
        .message()
        .into_iter()
        .chain([Message::User { content }])
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use rig_core::completion::{CompletionResponse, FinishReason, Usage};
    use rig_core::message::{AssistantContent, Origin, ToolCall, ToolFunction, ToolName};

    use super::*;

    // {test}`requirements.authoring.refresh`
    #[tokio::test]
    async fn default_authoring_apis_preserve_current_records_without_configuration() {
        let project = inputs::tests::project();
        let state = tempfile::tempdir().unwrap();
        let subject = "requirements.ready";
        let mut sources = AssessmentSources::prepare(project.path(), subject).unwrap();
        let mut usage = UsageLog::start(state.path(), project.path(), subject).unwrap();
        let (_, record) = assess(&JudgmentProvider(json!(true)), &mut sources, &mut usage)
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

    // {test}`requirements.authoring.output_limit`
    #[tokio::test]
    async fn a_cut_off_response_without_a_known_output_limit_fails_without_continuing() {
        struct CutOffProvider(std::sync::atomic::AtomicUsize);
        impl CompletionProvider for CutOffProvider {
            async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
                self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(JudgmentProvider(json!(true))
                    .complete(request)
                    .await?
                    .with_finish_reason(FinishReason::Length))
            }
        }
        let project = inputs::tests::project();
        let state = tempfile::tempdir().unwrap();
        let mut sources = AssessmentSources::prepare(project.path(), "requirements.ready").unwrap();
        let mut usage =
            UsageLog::start(state.path(), project.path(), &sources.subject_name).unwrap();
        let provider = CutOffProvider(std::sync::atomic::AtomicUsize::new(0));
        let error = assess(&provider, &mut sources, &mut usage)
            .await
            .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("response was incomplete"), "{message}");
        assert!(
            message.contains("output limit the configuration does not set"),
            "{message}"
        );
        assert_eq!(
            provider.0.load(std::sync::atomic::Ordering::Relaxed),
            1,
            "an unknown limit must not start a continuation"
        );
        assert!(
            usage.record.determination.is_none(),
            "the cut-off judgment is not accepted"
        );
    }

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

    // {cite}`habeas:util`
    struct ThrottledProvider {
        failures: usize,
        status: u16,
        code: &'static str,
        requests: Mutex<Vec<Value>>,
    }

    impl CompletionProvider for ThrottledProvider {
        // {cite}`habeas:util`
        async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
            self.requests
                .lock()
                .unwrap()
                .push(serde_json::to_value(&request).unwrap());
            if self.requests.lock().unwrap().len() <= self.failures {
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
    // {cite}`habeas:util`
    struct ReadingProvider {
        read: Value,
        requests: Mutex<Vec<Value>>,
    }

    impl CompletionProvider for ReadingProvider {
        // {cite}`habeas:util`
        async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
            self.requests
                .lock()
                .unwrap()
                .push(serde_json::to_value(&request).unwrap());
            if self.requests.lock().unwrap().len() > 1 {
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
            requests: Mutex::default(),
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
        let requests = provider.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        let delivered = requests[1].to_string();
        assert!(delivered.contains("A unit of"), "{delivered}");
        assert!(!requests[0].to_string().contains("A unit of"));
    }

    /// Searches with a paged result, asks for its second page with the
    /// token the first carried, and then submits.
    // {cite}`habeas:util`
    struct PagingProvider {
        requests: Mutex<Vec<String>>,
    }

    impl CompletionProvider for PagingProvider {
        // {cite}`habeas:util`
        async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
            let last = serde_json::to_string(request.chat_history.last().unwrap()).unwrap();
            self.requests.lock().unwrap().push(last.clone());
            let turn = self.requests.lock().unwrap().len();
            let (name, arguments) = match turn {
                1 => ("habeas", json!({"file": "src/lib.rs", "line_start": 1})),
                2 => {
                    // The tool result is JSON text inside the serialized message.
                    let marker = r#"\"next\":\""#;
                    let start = last.find(marker).expect("a paged result") + marker.len();
                    ("next_page", json!({"token": &last[start..start + 16]}))
                }
                _ => return JudgmentProvider(json!(true)).complete(request).await,
            };
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

    // {test}`requirements.authoring.result_pages`
    // {test}`requirements.authoring.delivered_inputs`
    #[tokio::test]
    async fn only_inputs_on_delivered_pages_are_recorded() {
        let project = inputs::tests::project();
        let mut source = String::new();
        for index in 0..600 {
            source.push_str(&format!(
                "fn needle_{index}() {{\n    let text = \"padding padding padding\";\n}}\n\n"
            ));
        }
        fs::write(project.path().join("src/lib.rs"), source).unwrap();
        let mut sources = AssessmentSources::prepare(project.path(), "requirements.ready").unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut usage =
            UsageLog::start(state.path(), project.path(), &sources.subject_name).unwrap();
        let provider = PagingProvider {
            requests: Mutex::default(),
        };
        let (_, evidence) = assess(&provider, &mut sources, &mut usage).await.unwrap();
        let requests = provider.requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        let delivered = |request: &str| {
            (0..600)
                .filter(|index| request.contains(&format!("fn needle_{index}()")))
                .collect::<Vec<_>>()
        };
        let (first, second) = (delivered(&requests[1]), delivered(&requests[2]));
        assert!(!first.is_empty() && !second.is_empty());
        // The pages are consecutive and leave entries undelivered.
        assert_eq!(second[0], first.last().unwrap() + 1);
        let last = *second.last().unwrap();
        assert!(last < 599, "{last}");
        let recorded = evidence["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|input| input["item"].as_str())
            .filter_map(|item| item.strip_prefix("fn[needle_"))
            .map(|item| item.trim_end_matches(']').parse::<usize>().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(recorded, (0..=last).collect::<Vec<_>>());
        let log: Value = serde_json::from_str(&fs::read_to_string(&usage.path).unwrap()).unwrap();
        assert_eq!(log["tool_calls"]["next_page"], 1);
    }

    // {test}`requirements.authoring.no_line_numbers`
    #[tokio::test]
    async fn tool_errors_give_the_model_no_line_numbers_it_did_not_supply() {
        let project = inputs::tests::project();
        fs::write(project.path().join("src/lib.rs"), "fn work() {}\n").unwrap();
        fs::write(
            project.path().join("src/broken.rs"),
            "fn ok() {}\nfn broken( {\n",
        )
        .unwrap();
        let sources = AssessmentSources::prepare(project.path(), "requirements.ready").unwrap();
        // A range past the end repeats only the supplied start line.
        let past = tools::dispatch(
            &sources,
            "habeas",
            &json!({"file": "src/lib.rs", "line_start": 99}),
        )
        .err()
        .unwrap()
        .to_string();
        assert!(
            past.starts_with("src/lib.rs:99 is not an inclusive range"),
            "{past}"
        );
        // A parse error names its line, but the model receives it without.
        let read = json!({"file": "src/broken.rs", "item": ""});
        let parse = tools::dispatch(&sources, "habeas", &read)
            .err()
            .unwrap()
            .to_string();
        assert!(
            regex::Regex::new(r"line \d+: ").unwrap().is_match(&parse),
            "{parse}"
        );
        let mut sources = sources;
        let state = tempfile::tempdir().unwrap();
        let mut usage =
            UsageLog::start(state.path(), project.path(), &sources.subject_name).unwrap();
        let provider = ReadingProvider {
            read,
            requests: Mutex::default(),
        };
        assess(&provider, &mut sources, &mut usage).await.unwrap();
        let delivered = provider.requests.lock().unwrap()[1].to_string();
        assert!(delivered.contains("cannot parse"), "{delivered}");
        assert!(
            !regex::Regex::new(r"line \d+: ")
                .unwrap()
                .is_match(&delivered),
            "{delivered}"
        );
    }

    // {test}`requirements.authoring.read_denial`
    // {test}`requirements.authoring.tool_descriptions.special_cases`
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
                requests: Mutex::default(),
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
            let requests = provider.requests.lock().unwrap();
            assert_eq!(requests.len(), 2, "{read}");
            let message =
                serde_json::to_string(&line_numbers::without_source_lines(&failure.to_string()))
                    .unwrap();
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
    // {test}`requirements.authoring.tool_descriptions.special_cases`
    #[tokio::test]
    async fn a_read_through_a_symlink_leaving_the_project_returns_an_error_to_the_model() {
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
            requests: Mutex::default(),
        };
        let (_, evidence) = assess(&provider, &mut sources, &mut usage).await.unwrap();
        // The denial went back to the model, naming the path and the reason,
        // and the assessment continued without recording the file.
        let requests = provider.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        let delivered = requests[1].to_string();
        assert!(delivered.contains("src/escape.rs"), "{delivered}");
        assert!(delivered.contains("outside the project"), "{delivered}");
        assert!(!delivered.contains("fn secret"), "{delivered}");
        assert!(!evidence.to_string().contains("escape.rs"), "{evidence}");
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
        let prompt = initial_prompt(&sources).unwrap().joined();
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
        // The testing criteria follow the requirement task.
        assert!(prompt.contains(&format!(
            "{}\n{}",
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

    // {cite}`habeas:util`
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
    // {test}`requirements.authoring.usage_requests.failures`
    // {test}`requirements.authoring.usage_inputs`
    // {test}`requirements.ci.reference_set`
    #[tokio::test(start_paused = true)]
    async fn throttled_requests_retry_unchanged_and_record_every_attempt() {
        let (_project, _state, mut sources, mut usage) = retry_fixture();
        let provider = ThrottledProvider {
            failures: 5,
            status: 429,
            code: "ThrottlingException",
            requests: Mutex::default(),
        };
        let started = tokio::time::Instant::now();
        let (_, record) = assess(&provider, &mut sources, &mut usage).await.unwrap();
        assert!(started.elapsed() >= Duration::from_millis(500));
        let requests = provider.requests.lock().unwrap();
        assert_eq!(requests.len(), 6);
        assert!(requests.iter().all(|request| request == &requests[0]));
        assert_eq!(record["inputs"], json!(sources.mandatory_inputs()));
        assert_eq!(record["references"], json!(sources.reference_set));

        let log: Value = serde_json::from_slice(&fs::read(&usage.path).unwrap()).unwrap();
        assert_eq!(log["request_count"], 6);
        for attempt in 0..5 {
            assert_eq!(log["requests"][attempt]["outcome"], "failed");
            assert_eq!(log["requests"][attempt]["tokens"]["complete"], false);
            assert_eq!(
                log["requests"][attempt]["failure"],
                json!({"http_status": 429, "error_code": "ThrottlingException", "retried": true})
            );
        }
        assert!(log["requests"][5].get("failure").is_none());
        assert_eq!(log["requests"][5]["outcome"], "completed");
        assert_eq!(log["tool_calls"]["submit_judgment"], 1);
        for input in log["inputs"].as_array().unwrap() {
            assert_eq!(input["submitted_in_request"], 0);
            assert_eq!(input["sent_in_request"], 5);
        }
    }

    // {test}`requirements.authoring.provider_backoff`
    // {test}`requirements.authoring.usage_requests.failures`
    #[tokio::test(start_paused = true)]
    async fn permanent_provider_failure_returns_without_waiting_or_retrying() {
        let (_project, _state, mut sources, mut usage) = retry_fixture();
        let provider = ThrottledProvider {
            failures: 8,
            status: 400,
            code: "ValidationException",
            requests: Mutex::default(),
        };
        let started = tokio::time::Instant::now();
        let error = assess(&provider, &mut sources, &mut usage)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("Too many tokens"));
        assert_eq!(started.elapsed(), Duration::ZERO);
        assert_eq!(provider.requests.lock().unwrap().len(), 1);
        // The failure is recorded as not retried.
        let log: Value = serde_json::from_slice(&fs::read(&usage.path).unwrap()).unwrap();
        assert_eq!(
            log["requests"][0]["failure"],
            json!({"http_status": 400, "error_code": "ValidationException", "retried": false})
        );
    }

    // {test}`requirements.authoring.provider_backoff.limit`
    #[tokio::test(start_paused = true)]
    async fn a_request_failing_for_a_minute_fails_the_assessment() {
        let (_project, _state, mut sources, mut usage) = retry_fixture();
        let provider = ThrottledProvider {
            failures: usize::MAX,
            status: 503,
            code: "ServiceUnavailableException",
            requests: Mutex::default(),
        };
        let started = tokio::time::Instant::now();
        let error = assess(&provider, &mut sources, &mut usage)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            started.elapsed() <= providers::RETRY_LIMIT,
            "{:?}",
            started.elapsed()
        );
        let attempts = provider.requests.lock().unwrap().len();
        assert!(attempts > 3, "{attempts}");
        assert!(
            error.contains(&format!(
                "after {attempts} attempts; last failure: HTTP 503 ServiceUnavailableException"
            )),
            "{error}"
        );
        let log: Value = serde_json::from_slice(&fs::read(&usage.path).unwrap()).unwrap();
        assert_eq!(log["requests"][attempts - 1]["failure"]["retried"], false);
    }

    // {test}`requirements.authoring.provider_backoff`
    #[tokio::test(start_paused = true)]
    async fn cancelling_during_backoff_prevents_another_request() {
        let (_project, _state, mut sources, mut usage) = retry_fixture();
        let provider = ThrottledProvider {
            failures: 8,
            status: 429,
            code: "ThrottlingException",
            requests: Mutex::default(),
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
        assert_eq!(provider.requests.lock().unwrap().len(), 1);
        assert!(sources.recorded_inputs().is_empty());
        let log: Value = serde_json::from_slice(&fs::read(&usage.path).unwrap()).unwrap();
        assert_eq!(log["request_count"], 1);
        assert_eq!(log["requests"][0]["outcome"], "failed");
    }

    // {cite}`habeas:util`
    struct HangingProvider {
        requests: Mutex<usize>,
    }

    impl CompletionProvider for HangingProvider {
        // {cite}`habeas:util`
        async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
            *self.requests.lock().unwrap() += 1;
            if *self.requests.lock().unwrap() == 1 {
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
            requests: Mutex::default(),
        };
        let started = tokio::time::Instant::now();
        let (judgment, _) = assess(&provider, &mut sources, &mut usage).await.unwrap();
        assert_eq!(judgment.to_value(), json!(true));
        assert!(
            (Duration::from_secs(600)..=Duration::from_secs(601)).contains(&started.elapsed()),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(*provider.requests.lock().unwrap(), 2);
        let log: Value = serde_json::from_slice(&fs::read(&usage.path).unwrap()).unwrap();
        assert_eq!(log["request_count"], 2);
        assert_eq!(log["requests"][0]["outcome"], "failed");
        assert_eq!(log["requests"][1]["outcome"], "completed");
    }
    /// Fails with throttling before both requests, answering the first
    /// recovered request with a source read and the second with a judgment.
    // {cite}`habeas:util`
    struct ThrottledToolProvider {
        requests: Mutex<Vec<(tokio::time::Instant, Value)>>,
    }

    impl CompletionProvider for ThrottledToolProvider {
        // {cite}`habeas:util`
        async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
            let attempt = self.requests.lock().unwrap().len();
            self.requests.lock().unwrap().push((
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
            requests: Mutex::default(),
        };
        let (_, record) = assess(&provider, &mut sources, &mut usage).await.unwrap();
        let requests = provider.requests.lock().unwrap();
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
        let prompt = initial_prompt(&sources).unwrap().joined();
        for guidance in [
            "Choose true when your task finds no gap, {\"FIXME\": [\"...\"]} when every gap is\nacknowledged, and false otherwise.",
            "You MUST end by calling submit_judgment\nwith determination and rationale until one submission is accepted.\nA rejected submission does not complete the assessment. A judgment written in your\nreply text is not accepted.",
            "A gap is acknowledged only by a {fixme} reference",
            "a {fixme} naming a\n  closed issue no longer acknowledges its gap",
            "including\n  any acknowledged gaps",
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
                .contains("until one submission is accepted")
        );
    }

    /// Returns one scripted response per request; each response is a list of
    /// tool calls.
    // {cite}`habeas:util`
    struct ScriptedProvider {
        turns: Vec<Vec<(&'static str, Value)>>,
        requests: Mutex<usize>,
    }

    impl CompletionProvider for ScriptedProvider {
        // {cite}`habeas:util`
        async fn complete(&self, _request: CompletionRequest) -> io::Result<CompletionResponse> {
            let turn = *self.requests.lock().unwrap();
            *self.requests.lock().unwrap() += 1;
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

    enum SearchAction {
        Call(&'static str, Value),
        Finish(usize, Vec<&'static str>),
        RetainRejected,
    }

    struct SearchProvider {
        submit: &'static str,
        turns: Vec<Vec<SearchAction>>,
        requests: Mutex<Vec<Value>>,
    }

    fn delivered_searches(value: &Value, results: &mut Vec<Value>) {
        match value {
            Value::Object(object) => {
                if object.contains_key("search_id") && object.contains_key("matches") {
                    results.push(value.clone());
                } else {
                    for value in object.values() {
                        delivered_searches(value, results);
                    }
                }
            }
            Value::Array(array) => {
                for value in array {
                    delivered_searches(value, results);
                }
            }
            Value::String(text) => {
                if let Ok(value) = serde_json::from_str::<Value>(text) {
                    delivered_searches(&value, results);
                }
            }
            _ => {}
        }
    }

    impl CompletionProvider for SearchProvider {
        async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
            let history = serde_json::to_value(&request.chat_history).unwrap();
            let turn = self.requests.lock().unwrap().len();
            self.requests.lock().unwrap().push(history.clone());
            if self.turns[turn].is_empty() {
                return Ok(CompletionResponse::new(
                    vec![AssistantContent::text("The review is complete.")],
                    Usage::default(),
                    Origin::new("fixture", "fixture", "fixture"),
                    Value::Null,
                ));
            }
            let mut searches = Vec::new();
            delivered_searches(&history, &mut searches);
            let calls = self.turns[turn]
                .iter()
                .enumerate()
                .map(|(index, action)| {
                    let (name, arguments) = match action {
                        SearchAction::Call(name, arguments) => (*name, arguments.clone()),
                        SearchAction::Finish(index, used) => (
                            "finish_search",
                            json!({"search_id":searches[*index]["search_id"],"used":used}),
                        ),
                        SearchAction::RetainRejected => {
                            let text = history.to_string();
                            let captures =
                                regex::Regex::new(r"rationale set to exactly ([0-9a-f-]{36})")
                                    .unwrap()
                                    .captures_iter(&text)
                                    .last()
                                    .unwrap()
                                    .get(1)
                                    .unwrap();
                            (
                                self.submit,
                                json!({"determination":true,"rationale":captures.as_str()}),
                            )
                        }
                    };
                    AssistantContent::ToolCall(ToolCall::from_wire(
                        format!("search-{turn}-{index}"),
                        ToolFunction::new(ToolName::new(name).unwrap(), arguments),
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

    // {test}`requirements.authoring.submission.searches`
    // {test}`requirements.authoring.submission.searches.diagnostic`
    // {test}`requirements.authoring.submission.searches.recovery`
    // {test}`requirements.authoring.submission.searches.accompanying_calls`
    // {test}`requirements.authoring.finish_search.inputs`
    // {test}`requirements.authoring.search_accounting.recovery`
    #[tokio::test]
    async fn every_submission_kind_waits_for_all_searches_and_can_recover() {
        use SearchAction::{Call, Finish};
        for submit in ["submit_judgment", "submit_verification"] {
            let root = inputs::tests::project();
            fs::write(
                root.path().join("src/lib.rs"),
                "fn helper() { assert!(true); }\nfn untouched() {}\n",
            )
            .unwrap();
            let mut sources =
                AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
            let state = tempfile::tempdir().unwrap();
            let mut usage =
                UsageLog::start(state.path(), root.path(), &sources.subject_name).unwrap();
            let arguments = if submit == "submit_judgment" {
                json!({"determination":true,"rationale":"Helper checks its assertion."})
            } else {
                json!({"real":false,"acknowledged":false,"acknowledgments":[],
                    "rationale":"Helper checks its assertion."})
            };
            let provider = SearchProvider {
                submit,
                turns: vec![
                    vec![
                        Call("ripgrep", json!({"pattern":"assert","path":"src/lib.rs"})),
                        Call(
                            "ast_grep",
                            json!({"language":"rust","rule":{"rule":{"pattern":"absent()"}}}),
                        ),
                    ],
                    // Accompanying completion is skipped, so both IDs remain open.
                    vec![
                        Finish(0, vec!["1"]),
                        Call(
                            "habeas",
                            json!({"file":"src/lib.rs","item":"fn[untouched]"}),
                        ),
                        Call("ripgrep", json!({"pattern":"absent","path":"src/lib.rs"})),
                        Call(submit, arguments.clone()),
                    ],
                    vec![Finish(0, vec!["1"])],
                    vec![Call(submit, arguments.clone())],
                    vec![Finish(1, vec![])],
                    vec![Call(submit, arguments)],
                ],
                requests: Mutex::default(),
            };
            if submit == "submit_judgment" {
                let (_, evidence) = assess(&provider, &mut sources, &mut usage).await.unwrap();
                assert!(
                    evidence["inputs"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|input| input["item"] == "fn[helper]" && input.get("hash").is_some())
                );
            } else {
                let result = verify(
                    &provider,
                    &mut sources,
                    &mut usage,
                    &Judgment::False,
                    "Alleged gap",
                )
                .await
                .unwrap();
                assert!(!result.real);
            }
            let requests = provider.requests.lock().unwrap();
            assert_eq!(requests.len(), 6);
            let mut searches = Vec::new();
            delivered_searches(&requests[1], &mut searches);
            assert_eq!(searches.len(), 2);
            let blocked = requests[2].as_array().unwrap().last().unwrap().to_string();
            assert!(blocked.contains("not run: the response also made a submission"));
            assert!(blocked.contains(searches[0]["search_id"].as_str().unwrap()));
            assert!(blocked.contains(searches[1]["search_id"].as_str().unwrap()));
            assert!(blocked.contains("Call finish_search for every listed search_id"));
            assert!(blocked.contains("then resubmit"));
            let latest = requests[4].as_array().unwrap().last().unwrap().to_string();
            assert!(latest.contains("outstanding searches"));
            assert!(!latest.contains(searches[0]["search_id"].as_str().unwrap()));
            assert!(latest.contains(searches[1]["search_id"].as_str().unwrap()));
            assert_eq!(
                sources
                    .recorded_inputs()
                    .iter()
                    .filter(|input| input["item"] == "fn[helper]")
                    .count(),
                1
            );
            let log: Value =
                serde_json::from_str(&fs::read_to_string(&usage.path).unwrap()).unwrap();
            assert_eq!(log["tool_calls"]["finish_search"], 3);
            assert_eq!(log["tool_skipped"]["finish_search"], 1);
            assert_eq!(log["tool_skipped"]["habeas"], 1);
            assert_eq!(log["tool_skipped"]["ripgrep"], 1);
            assert!(
                !sources
                    .recorded_inputs()
                    .iter()
                    .any(|input| input["item"] == "fn[untouched]")
            );
            assert_eq!(
                log["tool_failures"]
                    .as_object()
                    .unwrap()
                    .values()
                    .filter_map(Value::as_u64)
                    .sum::<u64>(),
                2
            );
        }
    }

    // {test}`requirements.authoring.finish_search.inputs`
    // {test}`requirements.authoring.finish_search.unselected`
    // {test}`requirements.authoring.finish_search.deduplication`
    #[tokio::test]
    async fn multiple_selected_tags_are_recorded_before_acknowledgment_and_survive_a_failed_turn() {
        use SearchAction::{Call, Finish};
        struct Provider {
            scripted: SearchProvider,
            fail_after_finish: bool,
        }
        impl CompletionProvider for Provider {
            async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
                if self.fail_after_finish && self.scripted.requests.lock().unwrap().len() == 2 {
                    let history = serde_json::to_value(&request.chat_history).unwrap();
                    let acknowledgment = history.as_array().unwrap().last().unwrap().to_string();
                    assert!(acknowledgment.contains("finished"));
                    assert!(!acknowledgment.contains("fn first"));
                    self.scripted.requests.lock().unwrap().push(history);
                    return Err(io::Error::other("fixture stopped after acknowledgment"));
                }
                self.scripted.complete(request).await
            }
        }
        for fail_after_finish in [false, true] {
            let root = inputs::tests::project();
            fs::write(
                root.path().join("src/lib.rs"),
                "fn first() { assert!(true); }\nfn unused() {}\nfn third() { assert_eq!(1, 1); }\n",
            )
            .unwrap();
            let expected = ["fn[third]", "fn[first]"]
                .map(|item| crate::inputs::outline_input(root.path(), "src/lib.rs", item).unwrap());
            let mut sources =
                AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
            let state = tempfile::tempdir().unwrap();
            let mut usage =
                UsageLog::start(state.path(), root.path(), &sources.subject_name).unwrap();
            let provider = Provider {
                fail_after_finish,
                scripted: SearchProvider {
                    submit: "submit_judgment",
                    turns: vec![
                        vec![Call("ripgrep", json!({"pattern":"fn","path":"src/lib.rs"}))],
                        vec![Finish(0, vec!["3", "1", "3"])],
                        vec![Call(
                            "submit_judgment",
                            json!({"determination":true,
                            "rationale":"Both selected snippets contain assertions."}),
                        )],
                    ],
                    requests: Mutex::default(),
                },
            };
            let outcome = assess(&provider, &mut sources, &mut usage).await;
            if fail_after_finish {
                assert!(
                    outcome
                        .err()
                        .unwrap()
                        .to_string()
                        .contains("fixture stopped")
                );
            } else {
                let (_, evidence) = outcome.unwrap();
                for input in &expected {
                    assert!(evidence["inputs"].as_array().unwrap().contains(input));
                }
            }
            for input in &expected {
                assert_eq!(
                    sources
                        .recorded_inputs()
                        .iter()
                        .filter(|seen| *seen == input)
                        .count(),
                    1
                );
            }
            assert!(
                !sources
                    .recorded_inputs()
                    .iter()
                    .any(|input| input["item"] == "fn[unused]")
            );
            assert_eq!(provider.scripted.requests.lock().unwrap().len(), 3);
        }
    }

    // {test}`requirements.authoring.finish_search.invalid`
    // {test}`requirements.authoring.finish_search.atomicity`
    #[tokio::test]
    async fn a_bad_tag_returns_a_recoverable_error_without_closing_the_search() {
        use SearchAction::{Call, Finish};
        let root = inputs::tests::project();
        fs::write(root.path().join("src/lib.rs"), "fn helper() {}\n").unwrap();
        let mut sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut usage = UsageLog::start(state.path(), root.path(), &sources.subject_name).unwrap();
        let submit = json!({"determination":true,"rationale":"Complete."});
        let provider = SearchProvider {
            submit: "submit_judgment",
            turns: vec![
                vec![Call(
                    "ripgrep",
                    json!({"pattern":"helper","path":"src/lib.rs"}),
                )],
                vec![Finish(0, vec!["1", "missing"])],
                vec![Call("submit_judgment", submit.clone())],
                vec![Finish(0, vec![])],
                vec![Call("submit_judgment", submit)],
            ],
            requests: Mutex::default(),
        };
        let (_, evidence) = assess(&provider, &mut sources, &mut usage).await.unwrap();
        let requests = provider.requests.lock().unwrap();
        assert!(requests[2].to_string().contains("unknown tag"));
        assert!(requests[3].to_string().contains("outstanding searches"));
        assert_eq!(requests.len(), 5);
        assert!(
            !evidence["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|input| input["item"] == "fn[helper]")
        );
    }

    // {test}`requirements.authoring.submission.searches.line_numbers`
    // {test}`requirements.authoring.line_numbers.rejection`
    // {test}`requirements.authoring.line_numbers.resubmission`
    #[tokio::test]
    async fn search_gate_preserves_line_number_correction_and_uuid_resubmission() {
        use SearchAction::{Call, Finish, RetainRejected};
        let root = inputs::tests::project();
        fs::write(root.path().join("src/lib.rs"), "fn helper() {}\n").unwrap();
        let mut sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut usage = UsageLog::start(state.path(), root.path(), &sources.subject_name).unwrap();
        let cited = json!({"determination":false,"rationale":"src/lib.rs:1 needs work."});
        let provider = SearchProvider {
            submit: "submit_judgment",
            turns: vec![
                vec![Call("ripgrep", json!({"pattern":"absent"}))],
                vec![Call("submit_judgment", cited.clone())],
                vec![Finish(0, vec![])],
                vec![Call("submit_judgment", cited)],
                vec![Call(
                    "ripgrep",
                    json!({"pattern":"helper","path":"src/lib.rs"}),
                )],
                vec![RetainRejected],
                vec![Finish(1, vec![])],
                vec![RetainRejected],
            ],
            requests: Mutex::default(),
        };
        let (judgment, evidence) = assess(&provider, &mut sources, &mut usage).await.unwrap();
        assert_eq!(judgment, Judgment::False);
        assert_eq!(evidence["rationale"], "src/lib.rs:1 needs work.");
        let requests = provider.requests.lock().unwrap();
        assert!(requests[2].to_string().contains("outstanding searches"));
        assert!(!requests[2].to_string().contains("rationale set to exactly"));
        assert!(requests[4].to_string().contains("rationale set to exactly"));
        assert!(
            requests[6]
                .as_array()
                .unwrap()
                .last()
                .unwrap()
                .to_string()
                .contains("outstanding searches")
        );
    }

    // {test}`requirements.authoring.line_numbers.resubmission`
    // {test}`requirements.authoring.submission.searches.line_numbers`
    // {test}`requirements.authoring.submission.searches.accompanying_calls`
    #[tokio::test]
    async fn a_second_line_number_rationale_fails_only_after_searches_are_finished() {
        use SearchAction::{Call, Finish};
        let root = inputs::tests::project();
        fs::write(root.path().join("src/lib.rs"), "fn untouched() {}\n").unwrap();
        let mut sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut usage = UsageLog::start(state.path(), root.path(), &sources.subject_name).unwrap();
        let second = json!({"determination":true,"rationale":"src/lib.rs:2 is fine."});
        let provider = SearchProvider {
            submit: "submit_judgment",
            turns: vec![
                vec![
                    Call(
                        "habeas",
                        json!({"file":"src/lib.rs","item":"fn[untouched]"}),
                    ),
                    Call(
                        "submit_judgment",
                        json!({"determination":false,"rationale":"src/lib.rs:1 fails."}),
                    ),
                ],
                vec![Call("ripgrep", json!({"pattern":"absent"}))],
                vec![Call("submit_judgment", second.clone())],
                vec![Finish(0, vec![])],
                vec![Call("submit_judgment", second)],
            ],
            requests: Mutex::default(),
        };
        let error = assess(&provider, &mut sources, &mut usage)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("after a rejection"), "{error}");
        let requests = provider.requests.lock().unwrap();
        assert_eq!(requests.len(), 5);
        assert!(
            requests[1]
                .as_array()
                .unwrap()
                .last()
                .unwrap()
                .to_string()
                .contains("not run")
        );
        assert!(
            requests[3]
                .as_array()
                .unwrap()
                .last()
                .unwrap()
                .to_string()
                .contains("outstanding searches")
        );
        assert!(usage.record.determination.is_none());
        assert!(
            !sources
                .recorded_inputs()
                .iter()
                .any(|input| input["item"] == "fn[untouched]")
        );
    }

    // {test}`requirements.authoring.submission.searches.line_numbers`
    // {test}`requirements.authoring.submission`
    #[tokio::test]
    async fn the_search_gate_validates_arguments_first_and_does_not_consume_the_reminder() {
        use SearchAction::{Call, Finish};
        for malformed in [true, false] {
            let root = inputs::tests::project();
            fs::write(root.path().join("src/lib.rs"), "fn helper() {}\n").unwrap();
            let mut sources =
                AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
            let state = tempfile::tempdir().unwrap();
            let mut usage =
                UsageLog::start(state.path(), root.path(), &sources.subject_name).unwrap();
            let valid = json!({"determination":true,"rationale":"Complete."});
            let mut turns = vec![
                vec![Call("ripgrep", json!({"pattern":"absent"}))],
                vec![Call(
                    "submit_judgment",
                    if malformed {
                        json!({"determination":true,"rationale":""})
                    } else {
                        valid.clone()
                    },
                )],
            ];
            if !malformed {
                turns.extend([
                    vec![],
                    vec![Finish(0, vec![])],
                    vec![Call("submit_judgment", valid)],
                ]);
            }
            let provider = SearchProvider {
                submit: "submit_judgment",
                turns,
                requests: Mutex::default(),
            };
            let result = assess(&provider, &mut sources, &mut usage).await;
            if malformed {
                let error = result.unwrap_err().to_string();
                assert!(error.contains("rationale"), "{error}");
                assert!(!error.contains("outstanding searches"), "{error}");
                assert_eq!(provider.requests.lock().unwrap().len(), 2);
                assert!(usage.record.determination.is_none());
            } else {
                assert_eq!(result.unwrap().0, Judgment::True);
                assert_eq!(provider.requests.lock().unwrap().len(), 5);
                assert!(
                    provider.requests.lock().unwrap()[3]
                        .to_string()
                        .contains("Reminder:")
                );
            }
        }
    }

    // {test}`requirements.authoring.search_accounting.helper_freshness`
    // {test}`requirements.authoring.finish_search.snapshot`
    // {test}`requirements.authoring.finish_search.inputs`
    // {test}`requirements.authoring.delivered_inputs`
    #[tokio::test]
    async fn a_helper_used_only_through_a_search_snippet_invalidates_saved_evidence_when_weakened()
    {
        use SearchAction::{Call, Finish};
        let root = inputs::tests::project();
        let helper_source = "fn assert_refusal(bytes: &mut Vec<u8>) {\n    let before = bytes.clone();\n    assert!(!try_insert(bytes));\n    assert_eq!(*bytes, before);\n}";
        let source = format!(
            "{helper_source}\nfn try_insert(_bytes: &mut Vec<u8>) -> bool {{ false }}\n\
            // {{test}}`requirements.ready`\n#[test]\nfn refuses_without_change() {{\n\
                let mut bytes = vec![1, 2];\n    assert_refusal(&mut bytes);\n}}\nfn unrelated() {{}}\n"
        );
        fs::write(root.path().join("src/lib.rs"), &source).unwrap();
        let marked_test =
            crate::inputs::outline_input(root.path(), "src/lib.rs", "fn[refuses_without_change]")
                .unwrap();
        let mut sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut usage = UsageLog::start(state.path(), root.path(), &sources.subject_name).unwrap();
        let provider = SearchProvider {
            submit: "submit_judgment",
            turns: vec![
                vec![Call(
                    "ripgrep",
                    json!({"pattern":"assert_refusal","path":"src/lib.rs","context":5}),
                )],
                vec![Finish(0, vec!["1"])],
                vec![Call(
                    "submit_judgment",
                    json!({"determination":true,
                    "rationale":"The assert_refusal snippet asserts refusal and byte preservation."}),
                )],
            ],
            requests: Mutex::default(),
        };
        let (_, evidence) = assess(&provider, &mut sources, &mut usage).await.unwrap();
        let inputs = evidence["inputs"].as_array().unwrap();
        let helper = inputs
            .iter()
            .find(|input| input["item"] == "fn[assert_refusal]")
            .unwrap();
        assert!(inputs.contains(&marked_test));
        assert!(!inputs.iter().any(|input| input["item"] == "fn[unrelated]"));
        assert!(helper.get("text").is_none());
        let mut errors = Vec::new();
        crate::evidence::validate_input(
            root.path(),
            &std::collections::BTreeMap::new(),
            &crate::inputs::OutlineCache::default(),
            helper,
            "helper",
            &mut errors,
        );
        assert!(errors.is_empty(), "{errors:?}");
        fs::write(
            root.path().join("src/lib.rs"),
            source.replace(helper_source, "fn assert_refusal(_bytes: &mut Vec<u8>) {}"),
        )
        .unwrap();
        assert_eq!(
            crate::inputs::outline_input(root.path(), "src/lib.rs", "fn[refuses_without_change]")
                .unwrap(),
            marked_test
        );
        crate::evidence::validate_input(
            root.path(),
            &std::collections::BTreeMap::new(),
            &crate::inputs::OutlineCache::default(),
            helper,
            "helper",
            &mut errors,
        );
        assert!(
            errors.iter().any(|error| error.contains("hash")),
            "{errors:?}"
        );
        let log: Value = serde_json::from_str(&fs::read_to_string(&usage.path).unwrap()).unwrap();
        assert!(log["tool_calls"].get("habeas").is_none());
        assert_eq!(log["tool_calls"]["finish_search"], 1);
    }

    // {cite}`habeas:util`
    /// Submits `first`, then, on each later request, the submission
    /// `next` makes from the UUID in the conversation's last rejection.
    struct ResubmittingProvider {
        first: Value,
        next: fn(&str) -> Value,
        requests: Mutex<Vec<String>>,
    }

    impl CompletionProvider for ResubmittingProvider {
        // {cite}`habeas:util`
        async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
            let history = serde_json::to_string(&request.chat_history).unwrap();
            let turn = self.requests.lock().unwrap().len();
            self.requests.lock().unwrap().push(history.clone());
            let arguments = if turn == 0 {
                self.first.clone()
            } else {
                let uuid = regex::Regex::new(
                    r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}",
                )
                .unwrap()
                .find_iter(&history)
                .last()
                .map(|found| found.as_str().to_owned())
                .unwrap_or_default();
                (self.next)(&uuid)
            };
            let call = AssistantContent::ToolCall(ToolCall::from_wire(
                format!("call-{turn}"),
                ToolFunction::new(ToolName::new("submit_judgment").unwrap(), arguments),
            ));
            Ok(CompletionResponse::new(
                vec![call],
                Usage::default(),
                Origin::new("fixture", "fixture", "fixture"),
                Value::Null,
            )
            .with_finish_reason(FinishReason::ToolCalls))
        }
    }

    // {test}`requirements.authoring.line_numbers.rejection`
    // {test}`requirements.authoring.line_numbers.resubmission`
    // {test}`requirements.authoring.submission`
    #[tokio::test]
    async fn a_rationale_citing_line_numbers_is_rejected_once() {
        let cited = json!({"determination": false, "rationale": "`work` at src/lib.rs:1 fails."});
        type Next = fn(&str) -> Value;
        let uuid = regex::Regex::new(r"[0-9a-f]{8}-[0-9a-f]{4}-").unwrap();
        let cases: [(Next, Option<&str>); 3] = [
            // Exactly the UUID keeps the rejected submission.
            (
                |uuid| json!({"determination": true, "rationale": uuid}),
                Some("`work` at src/lib.rs:1 fails."),
            ),
            // A rationale without line numbers replaces it.
            (
                |_| json!({"determination": false, "rationale": "`src/lib.rs#fn[work]` fails."}),
                Some("`src/lib.rs#fn[work]` fails."),
            ),
            // Line numbers again fail the assessment.
            (
                |_| json!({"determination": false, "rationale": "Line 1 fails."}),
                None,
            ),
        ];
        for (next, accepted) in cases {
            let project = inputs::tests::project();
            fs::write(project.path().join("src/lib.rs"), "fn work() {}\n").unwrap();
            let mut sources =
                AssessmentSources::prepare(project.path(), "requirements.ready").unwrap();
            let state = tempfile::tempdir().unwrap();
            let mut usage =
                UsageLog::start(state.path(), project.path(), &sources.subject_name).unwrap();
            let provider = ResubmittingProvider {
                first: cited.clone(),
                next,
                requests: Mutex::default(),
            };
            let result = assess(&provider, &mut sources, &mut usage).await;
            let requests = provider.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            // The rejection quotes the citation and repeats the guidance.
            assert!(requests[1].contains("src/lib.rs:1"), "{}", requests[1]);
            assert!(
                requests[1].contains("Give a source location"),
                "{}",
                requests[1]
            );
            // Both turns are counted in usage.
            let record = serde_json::to_value(&usage.record).unwrap();
            assert_eq!(
                record["requests"].as_array().unwrap().len(),
                2,
                "{record:#}"
            );
            match accepted {
                Some(rationale) => {
                    let (_, evidence) = result.unwrap();
                    assert_eq!(evidence["rationale"], rationale);
                    // The UUID is not recorded.
                    assert!(!uuid.is_match(&evidence.to_string()), "{evidence}");
                }
                None => {
                    let error = result.unwrap_err().to_string();
                    assert!(error.contains("after a rejection"), "{error}");
                }
            }
        }
    }

    // {test}`requirements.authoring.line_numbers.rejection`
    #[tokio::test]
    async fn verification_line_citations_are_rejected_with_the_correct_retry_tool() {
        let project = inputs::tests::project();
        let mut sources = AssessmentSources::prepare(project.path(), "requirements.ready").unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut usage =
            UsageLog::start(state.path(), project.path(), &sources.subject_name).unwrap();
        let provider = RecordingProvider::new(vec![
            verdict(true, &[], "src/lib.rs:3 fails."),
            verdict(true, &[], "`src/lib.rs#fn[ready]` fails."),
        ]);
        let result = verify(
            &provider,
            &mut sources,
            &mut usage,
            &Judgment::False,
            "A gap.",
        )
        .await
        .unwrap();
        assert_eq!(result.rationale, "`src/lib.rs#fn[ready]` fails.");
        let prompts = provider.prompts();
        assert_eq!(prompts.len(), 2);
        assert!(prompts[1].0.contains("Call submit_verification once more"));
        assert!(prompts[1].0.contains("src/lib.rs:3"));
        assert_eq!(
            serde_json::to_value(&usage.record).unwrap()["requests"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    // A first submission giving no line numbers is accepted at once.
    // {test}`requirements.authoring.line_numbers.rejection`
    #[tokio::test]
    async fn a_rationale_without_line_numbers_is_accepted_at_once() {
        let project = inputs::tests::project();
        let mut sources = AssessmentSources::prepare(project.path(), "requirements.ready").unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut usage =
            UsageLog::start(state.path(), project.path(), &sources.subject_name).unwrap();
        let provider = ResubmittingProvider {
            first: json!({"determination": true, "rationale": "`src/lib.rs` is fine."}),
            next: |_| unreachable!(),
            requests: Mutex::default(),
        };
        assess(&provider, &mut sources, &mut usage).await.unwrap();
        assert_eq!(provider.requests.lock().unwrap().len(), 1);
    }

    // {cite}`habeas:util`
    /// Records each request's conversation and answers with the next
    /// scripted tool calls.
    struct RemindedProvider {
        turns: Vec<Vec<(&'static str, Value)>>,
        requests: Mutex<Vec<String>>,
    }

    impl CompletionProvider for RemindedProvider {
        // {cite}`habeas:util`
        async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
            let turn = self.requests.lock().unwrap().len();
            self.requests
                .lock()
                .unwrap()
                .push(serde_json::to_string(&request.chat_history).unwrap());
            let mut content: Vec<AssistantContent> = self.turns[turn]
                .iter()
                .enumerate()
                .map(|(index, (name, arguments))| {
                    AssistantContent::ToolCall(ToolCall::from_wire(
                        format!("call-{turn}-{index}"),
                        ToolFunction::new(ToolName::new(*name).unwrap(), arguments.clone()),
                    ))
                })
                .collect();
            if content.is_empty() {
                content.push(AssistantContent::text("The judgment is true."));
            }
            Ok(CompletionResponse::new(
                content,
                Usage::default(),
                Origin::new("fixture", "fixture", "fixture"),
                Value::Null,
            )
            .with_finish_reason(FinishReason::Stop))
        }
    }

    // {test}`requirements.authoring.submission`
    // {test}`requirements.authoring.submission_count`
    #[tokio::test]
    async fn a_response_without_a_submission_is_reminded_once() {
        let submit = json!({"determination": true, "rationale": "Fine."});
        for (turns, accepted, count) in [
            (
                vec![vec![], vec![("submit_judgment", submit.clone())]],
                true,
                2,
            ),
            (vec![vec![], vec![]], false, 2),
            (
                vec![
                    vec![],
                    vec![("habeas", json!({"file":"src/lib.rs","item":"fn[work]"}))],
                    vec![],
                ],
                false,
                3,
            ),
        ] {
            let project = inputs::tests::project();
            fs::write(project.path().join("src/lib.rs"), "fn work() {}\n").unwrap();
            let mut sources =
                AssessmentSources::prepare(project.path(), "requirements.ready").unwrap();
            let state = tempfile::tempdir().unwrap();
            let mut usage =
                UsageLog::start(state.path(), project.path(), &sources.subject_name).unwrap();
            let provider = RemindedProvider {
                turns,
                requests: Mutex::default(),
            };
            let result = assess(&provider, &mut sources, &mut usage).await;
            let requests = provider.requests.lock().unwrap();
            assert_eq!(requests.len(), count);
            assert!(!requests[0].contains("Reminder:"));
            assert!(
                requests[1].contains(
                    "Reminder: you MUST submit your result with the submit_judgment tool."
                ),
                "{}",
                requests[1]
            );
            // Both turns are counted in usage.
            let record = serde_json::to_value(&usage.record).unwrap();
            assert_eq!(record["requests"].as_array().unwrap().len(), count);
            assert_eq!(
                requests
                    .last()
                    .unwrap()
                    .matches("Reminder: you MUST")
                    .count(),
                1
            );
            if accepted {
                assert_eq!(result.unwrap().1["rationale"], "Fine.");
            } else {
                let error = result.unwrap_err().to_string();
                assert!(error.contains("even after a reminder"), "{error}");
            }
        }
    }

    // {test}`requirements.authoring.submission`
    // {test}`requirements.authoring.line_numbers.rejection`
    // {test}`requirements.authoring.submission.searches.accompanying_calls`
    #[tokio::test]
    async fn a_submission_finishes_the_assessment_wherever_it_arrives() {
        let read = json!({"file": "src/lib.rs", "item": "fn[work]"});
        let submit = json!({"determination": false, "rationale": "Gaps remain."});
        let invalid = json!({"determination": "false", "rationale": "Gaps remain."});
        let cited = json!({"determination": false, "rationale": "See line 10."});
        let invalid_cited = json!({"determination": "false", "rationale": "See line 10."});
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
            (
                vec![vec![
                    ("submit_judgment", cited.clone()),
                    ("submit_judgment", cited.clone()),
                ]],
                false,
                1,
            ),
            (vec![vec![("submit_judgment", invalid_cited)]], false, 1),
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
                requests: Mutex::default(),
            };
            let result = assess(&provider, &mut sources, &mut usage).await;
            assert_eq!(*provider.requests.lock().unwrap(), requests, "{turns:?}");
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
                let error = result.unwrap_err().to_string();
                assert!(!error.contains("line numbers"), "{turns:?}: {error}");
                if turns.last().unwrap().len() > 1 {
                    assert!(error.contains("exactly once"), "{error}");
                } else {
                    assert!(error.contains("expected true, false"), "{error}");
                }
            }
        }
    }

    /// Answers each request with the next scripted tool call, recording the
    /// requests it receives.
    // {cite}`habeas:util`
    struct RecordingProvider {
        turns: Vec<(&'static str, Value)>,
        requests: Mutex<Vec<Value>>,
    }

    impl RecordingProvider {
        // {cite}`habeas:util`
        fn new(turns: Vec<(&'static str, Value)>) -> Self {
            Self {
                turns,
                requests: Mutex::default(),
            }
        }

        /// The prompt and advertised tool names of each request.
        // {cite}`habeas:util`
        fn prompts(&self) -> Vec<(String, Vec<String>)> {
            self.requests
                .lock()
                .unwrap()
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
        // {cite}`habeas:util`
        async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
            let turn = self.requests.lock().unwrap().len();
            self.requests
                .lock()
                .unwrap()
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

    // {cite}`habeas:util`
    fn judgment(determination: Value, rationale: &str) -> (&'static str, Value) {
        (
            "submit_judgment",
            json!({"determination": determination, "rationale": rationale}),
        )
    }

    // {cite}`habeas:util`
    fn verdict(real: bool, acknowledgments: &[&str], rationale: &str) -> (&'static str, Value) {
        (
            "submit_verification",
            json!({"real": real, "acknowledged": !acknowledgments.is_empty(),
                "acknowledgments": acknowledgments, "rationale": rationale}),
        )
    }

    /// Runs one subject's assessment with `turns`, returning the outcome, the
    /// provider, and the subjects of the usage records written.
    // {cite}`habeas:util`
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
            let (_, record) = result.unwrap();
            assert_eq!(record["rationale"], turns[0].1["rationale"]);
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
        let (_, record) = assess(&provider, &mut sources, &mut usage).await.unwrap();
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
        assert_eq!(record["rationale"], "Fresh assessment.");
    }

    // {test}`requirements.authoring.confirmation`
    // {test}`requirements.authoring.usage_storage`
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
            judgment(json!(true), "First citation review."),
            judgment(json!(true), "Second citation review."),
        ]);
        let (_, record) = assess_subject(
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
        assert_eq!(record["rationale"], "First citation review.");
        let prompts = provider.prompts();
        assert_eq!(prompts.len(), 2);
        assert!(
            prompts
                .iter()
                .all(|(text, _)| text.contains("Item under review: src/lib.rs fn[helper]"))
        );
        assert!(!prompts[1].0.contains("First citation review."));
        let logs: Vec<Value> = fs::read_dir(state.path().join("authoring/usage"))
            .unwrap()
            .map(|p| serde_json::from_slice(&fs::read(p.unwrap().path()).unwrap()).unwrap())
            .collect();
        assert_eq!(logs.len(), 2);
        assert_ne!(logs[0]["run_id"], logs[1]["run_id"]);
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
        assert!(prompts.iter().all(|(text, tools)| {
            text.contains("Assess whether a project's tests adequately check")
                && tools.contains(&"submit_judgment".to_owned())
        }));
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0]["subject"], "requirements.ready");
        assert_eq!(logs[1]["subject"], "requirements.ready (second assessment)");
        assert_eq!(logs[1]["outcome"], "completed");
        assert_eq!(logs[1]["determination"], "FIXME");
    }

    // {test}`requirements.authoring.confirmation`
    // {test}`requirements.authoring.verifier_prompt`
    // {test}`requirements.authoring.verifier_outcome`
    // {test}`requirements.authoring.habeas_background`
    // {test}`requirements.authoring.judgment_rules`
    #[tokio::test]
    async fn disagreeing_assessments_verify_the_less_favorable_one() {
        for (turns, recorded, kept, conclusion, less) in [
            // A real, unacknowledged gap records false with the less favorable
            // (second) assessment's rationale.
            (
                vec![
                    judgment(json!(true), "Looks complete."),
                    judgment(json!(false), "The empty case is unhandled."),
                    verdict(true, &[], "The empty case panics."),
                ],
                Judgment::False,
                1,
                "found an unacknowledged real gap",
                1,
            ),
            // A false assessment ranks below an acknowledged gap.
            (
                vec![
                    judgment(json!({"FIXME": ["#7"]}), "Only the known gap."),
                    judgment(json!(false), "An additional unacknowledged gap."),
                    verdict(true, &[], "The additional gap is real."),
                ],
                Judgment::False,
                1,
                "found an unacknowledged real gap",
                1,
            ),
            // A FIXME whose gap is real but unacknowledged also becomes false.
            (
                vec![
                    judgment(json!({"FIXME": ["#7"]}), "Issue 7 is open."),
                    judgment(json!(true), "Complete."),
                    verdict(true, &[], "Issue 7 is closed."),
                ],
                Judgment::False,
                0,
                "found an unacknowledged real gap",
                0,
            ),
            // Real and acknowledged gaps record FIXME with the verifier's
            // acknowledgments.
            (
                vec![
                    judgment(json!(true), "Looks complete."),
                    judgment(json!(false), "The empty case is unhandled."),
                    verdict(true, &["example/queue#9"], "A fixme tracks it."),
                ],
                Judgment::Fixme(vec!["example/queue#9".into()]),
                1,
                "found its real gaps all acknowledged",
                1,
            ),
            // No real gap records the more favorable (second) assessment.
            (
                vec![
                    judgment(json!({"FIXME": ["#7"]}), "Issue 7 is open."),
                    judgment(json!(true), "Complete."),
                    verdict(false, &[], "Issue 7 does not affect this."),
                ],
                Judgment::True,
                1,
                "found none of its claimed gaps real",
                0,
            ),
            (
                vec![
                    judgment(json!({"FIXME": ["#7"]}), "Only the known gap."),
                    judgment(json!(false), "An additional unacknowledged gap."),
                    verdict(false, &[], "The additional gap is not real."),
                ],
                Judgment::Fixme(vec!["#7".into()]),
                0,
                "found none of its claimed gaps real",
                1,
            ),
            (
                vec![
                    judgment(json!(true), "Complete."),
                    judgment(json!({"FIXME":["#7"]}), "Issue 7 is open."),
                    verdict(true, &["#7"], "The known gap is real and acknowledged."),
                ],
                Judgment::Fixme(vec!["#7".into()]),
                1,
                "found its real gaps all acknowledged",
                1,
            ),
            (
                vec![
                    judgment(json!(true), "Complete."),
                    judgment(json!({"FIXME":["#7"]}), "Issue 7 is open."),
                    verdict(false, &[], "The known gap does not apply."),
                ],
                Judgment::True,
                0,
                "found none of its claimed gaps real",
                1,
            ),
        ] {
            let (result, provider, logs) = confirmed(turns.clone(), true).await;
            let (judgment, record) = result.unwrap();
            assert_eq!(judgment, recorded);
            assert_eq!(record["determination"], judgment.to_value());
            let rationale = record["rationale"].as_str().unwrap();
            assert!(
                rationale.starts_with(turns[kept].1["rationale"].as_str().unwrap()),
                "{rationale}"
            );
            assert!(
                rationale.contains("the assessments disagreed"),
                "{rationale}"
            );
            assert!(rationale.contains(conclusion), "{rationale}");
            assert!(
                rationale.contains(turns[2].1["rationale"].as_str().unwrap()),
                "{rationale}"
            );

            // Prompts are recorded as serialized requests, so expected text is
            // compared in its JSON-escaped form.
            // The verifier receives the less favorable determination, its
            // rationale, and the criteria it applied, with the source tools
            // and submit_verification only.
            let prompts = provider.prompts();
            assert_eq!(prompts.len(), 3);
            let determination = turns[less].1["determination"].to_string();
            let less_rationale = turns[less].1["rationale"].as_str().unwrap();
            let escaped = serde_json::to_string(&format!(
                "The reviewer judged requirement requirements.ready as {determination}."
            ))
            .unwrap();
            let (text, tools) = &prompts[2];
            assert!(text.contains(escaped.trim_matches('"')), "{text}");
            assert!(text.contains(less_rationale), "{text}");
            assert!(
                text.contains("Judge against: the subject's text."),
                "{text}"
            );
            assert!(text.contains("Derive testable obligations"), "{text}");
            assert!(text.contains("# Mandatory inputs"), "{text}");
            // The verifier of a requirement carries both background texts,
            // the judgment rules, and the subject's mandatory input.
            let text = serde_json::from_str::<Value>(text).unwrap().to_string();
            for background in [
                approved_text("habeas_background", 0),
                approved_text("habeas_background", 1),
                approved_text("judgment_rules", 0),
                "\"requirements\": \"requirements.ready\"",
                "The project MUST be ready.",
            ] {
                let background = serde_json::to_string(background).unwrap();
                assert!(text.contains(background.trim_matches('"')), "{text}");
            }
            assert!(!text.contains("{{"), "{text}");
            assert!(tools.contains(&"submit_verification".to_owned()));
            assert!(tools.contains(&"habeas".to_owned()));
            assert!(!tools.contains(&"submit_judgment".to_owned()));
            // The verifier has the assessments' source tools.
            let source_tools = |tools: &[String]| {
                tools
                    .iter()
                    .filter(|tool| !tool.starts_with("submit_"))
                    .cloned()
                    .collect::<Vec<_>>()
            };
            assert_eq!(source_tools(tools), source_tools(&prompts[0].1));

            assert_eq!(logs.len(), 3);
            assert_eq!(logs[2]["subject"], "requirements.ready (verification)");
            assert_eq!(logs[2]["outcome"], "completed");
        }
    }

    // {test}`requirements.authoring.verifier_prompt`
    // {test}`requirements.authoring.habeas_background`
    // {test}`requirements.authoring.judgment_rules`
    #[test]
    fn verifier_prompts_name_examples_and_citation_reviews() {
        let project = inputs::tests::project();
        let cite = "{cite}`requirements.ready`";
        let requirements = project.path().join("md/requirements.md");
        let mut book = fs::read_to_string(&requirements).unwrap();
        book.push_str(&format!(
            "\n:::{{example}} case\nA ready case. {cite}\n:::\n"
        ));
        fs::write(&requirements, book).unwrap();
        fs::write(
            project.path().join("src/lib.rs"),
            format!("// {cite}\n// :::{{cite}} habeas:util\n// Formats output.\n// :::\nfn helper() {{}}\n"),
        )
        .unwrap();
        for (subject, header, named) in [
            (
                "requirements.case",
                "The reviewer judged example requirements.case as false.",
                "Subject: example requirements.case",
            ),
            (
                "citations:src/lib.rs#fn[helper]",
                "The reviewer judged citation review citations:src/lib.rs#fn[helper] as false.",
                "Item under review: src/lib.rs fn[helper]\nReview kind: source_item\nCitations to check:",
            ),
        ] {
            let sources = AuthoringSubject::parse(subject)
                .sources(project.path())
                .unwrap();
            let parts = verification_prompt(&sources, &Judgment::False, "A claimed gap.").unwrap();
            let prompt = parts.joined();
            let initial = initial_prompt(&sources).unwrap();
            // The reviewer's criteria are its subject message between the
            // "# Subject" and "# Mandatory inputs" headings, then its task.
            let subject_section = prompt_section(&initial.subject, "Subject").trim();
            let first = prompt_section(&initial.system, "Task");
            assert!(
                parts.subject.contains(&format!(
                    "The criteria the reviewer applied:\n\n{subject_section}\n\n{}",
                    prompt_section(&initial.system, "Your task")
                )),
                "{prompt}"
            );
            for expected in [
                header,
                named,
                "A claimed gap.",
                approved_text("habeas_background", 0),
                approved_text("habeas_background", 1),
                approved_text("judgment_rules", 0),
            ] {
                assert!(
                    prompt.contains(expected),
                    "missing {expected:?} in:\n{prompt}"
                );
            }
            // The rationale and subject stay out of the system prompt.
            assert!(!parts.system.contains("A claimed gap."), "{prompt}");
            assert!(!parts.system.contains(&sources.subject_name), "{prompt}");
            assert!(!prompt.contains(first), "{prompt}");
            assert!(!prompt.contains(SITE_STEP), "{prompt}");
            // Every mandatory input is delivered.
            let mandatory =
                serde_json::to_string_pretty(&sources.model_mandatory_inputs()).unwrap();
            assert!(prompt.contains(&mandatory), "{prompt}");
            assert!(!prompt.contains("{{"), "{prompt}");
        }
    }

    // {test}`requirements.authoring.verifier_prompt`
    // {test}`requirements.authoring.verifier_outcome`
    #[test]
    fn verifications_accept_exactly_their_four_fields() {
        assert_eq!(
            verification(&json!({"real": true, "acknowledged": true,
                "acknowledgments": ["example/queue#1"], "rationale": "Checked."}))
            .unwrap(),
            Verification {
                real: true,
                acknowledged: true,
                acknowledgments: vec!["example/queue#1".into()],
                rationale: "Checked.".into(),
                applies: None,
                dependencies: Vec::new(),
                inputs: Vec::new(),
            }
        );
        assert!(
            verification(&json!({"real": false, "acknowledged": false,
                "acknowledgments": [], "rationale": "None hold."}))
            .is_ok()
        );
        for arguments in [
            json!({"real": "true", "acknowledged": false, "acknowledgments": [], "rationale": "x"}),
            json!({"real": true, "acknowledged": 1, "acknowledgments": [], "rationale": "x"}),
            json!({"real": true, "acknowledged": false, "acknowledgments": "", "rationale": "x"}),
            json!({"real": true, "acknowledged": false, "acknowledgments": [" "], "rationale": "x"}),
            json!({"real": true, "acknowledged": true, "acknowledgments": [], "rationale": "x"}),
            json!({"real": true, "acknowledged": false, "acknowledgments": []}),
            json!({"real": true, "acknowledged": false, "acknowledgments": [], "rationale": " "}),
            json!({"real": true, "acknowledged": false, "acknowledgments": [], "rationale": "x", "extra": 1}),
            json!({"confirmed": true, "rationale": "x"}),
        ] {
            assert!(verification(&arguments).is_err(), "accepted {arguments}");
        }
        let definitions =
            serde_json::to_value(tool_definitions_with("submit_verification", false).unwrap())
                .unwrap();
        let submit = definitions
            .as_array()
            .unwrap()
            .iter()
            .find(|definition| definition["name"] == "submit_verification")
            .unwrap();
        assert_eq!(
            submit["parameters"]["required"],
            json!(["real", "acknowledged", "acknowledgments", "rationale"])
        );
        assert_eq!(
            submit["parameters"]["properties"],
            json!({"real": {"type": "boolean"}, "acknowledged": {"type": "boolean"},
                "acknowledgments": {"type": "array", "items": {"type": "string", "minLength": 1}},
                "rationale": {"type": "string", "minLength": 1}})
        );
    }

    // {test}`requirements.authoring.initial_prompt`
    // {test}`requirements.authoring.verifier_prompt`
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
        let provider = RecordingProvider::new(vec![judgment(
            json!(true),
            "The supplied source and explanation support the citation.",
        )]);
        let (_, record) = assess(&provider, &mut sources, &mut usage).await.unwrap();
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
        let body = "# Notes\n\nUnrelated chapter text.\n\n:::{cite} requirements.ready\nReadiness is measured in units.\n:::\n\nAdjacent prose.\n";
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
        let provider =
            RecordingProvider::new(vec![judgment(json!(true), "The explanation is supported.")]);
        let (_, record) = assess(&provider, &mut sources, &mut usage).await.unwrap();
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
        let provider = RecordingProvider::new(vec![judgment(
            json!(false),
            "Seconds contradicts the cited unit description.",
        )]);
        let (determination, record) = assess(&provider, &mut changed, &mut usage).await.unwrap();
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
    async fn citation_reviews_use_their_prompt_and_record_the_item_and_citations() {
        let project = inputs::tests::project();
        let cite = "{cite}`requirements.ready`";
        fs::write(
            project.path().join("src/lib.rs"),
            format!(
                "// {cite}\n// :::{{cite}} habeas:util\n// Formats output for the caller.\n// :::\nfn helper() {{}}\n"
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
        let provider = RecordingProvider::new(vec![judgment(json!(true), "Both are sensible.")]);
        let (_, record) = assess(&provider, &mut sources, &mut usage).await.unwrap();
        let prompts = provider.prompts();
        let expected: Vec<_> = tool_definitions_with("submit_judgment", false)
            .unwrap()
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        assert_eq!(prompts[0].1, expected);
        let duplicate = json!({"determination":true,"rationale":"Both are sensible."});
        let duplicate_provider = ScriptedProvider {
            turns: vec![vec![
                ("submit_judgment", duplicate.clone()),
                ("submit_judgment", duplicate),
            ]],
            requests: Mutex::default(),
        };
        let mut duplicate_sources = subject.sources(project.path()).unwrap();
        assert!(
            assess(&duplicate_provider, &mut duplicate_sources, &mut usage)
                .await
                .unwrap_err()
                .to_string()
                .contains("exactly once")
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
        assert!(
            prompt.contains(mandatory[0]["hash"].as_str().unwrap()),
            "{prompt}"
        );
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
        let user = request["chat_history"]
            .as_array()
            .unwrap()
            .iter()
            .find(|message| message["role"] == "user")
            .unwrap();
        let subject_text = user["content"][0]["text"].as_str().unwrap();
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
        // The verifier's criteria carry the distributed task too.
        let verifier = verification_prompt(&sources, &Judgment::False, "A gap.").unwrap();
        assert!(verifier.subject.contains(&task), "{}", verifier.subject);
        assert!(!verifier.system.contains(&task), "{}", verifier.system);
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
    // {test}`requirements.authoring.verifier_prompt`
    #[test]
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
            let request = Prompt {
                system: prompt.system.clone(),
                subject: prompt.subject.clone(),
            }
            .request();
            assert_eq!(
                request.chat_history,
                vec![
                    Message::system(prompt.system.clone()),
                    Message::user(prompt.subject.clone())
                ]
            );
        }
        let project =
            crate::sites::tests::project(crate::sites::tests::RULE, crate::sites::tests::SOURCE);
        let requirements = project.path().join("md/requirements.md");
        let mut book = fs::read_to_string(&requirements).unwrap();
        let cite = "{cite}`requirements.rule`";
        book.push_str(&format!(
            "\n:::{{req}} other\nThe project MUST also work.\n:::\n\n:::{{req}} second\nThe project MUST keep working.\n:::\n\n:::{{req}} structure_one\nThe API MUST delegate to a helper.\n:::\n\n:::{{req}} structure_two\nThe API MUST expose a function.\n:::\n\n:::{{example}} first_case\nA case. {cite}\n:::\n\n:::{{example}} second_case\nAnother case. {cite}\n:::\n"
        ));
        fs::write(&requirements, book).unwrap();
        let source = project.path().join("src/lib.rs");
        let mut text = fs::read_to_string(&source).unwrap();
        text.push_str(&format!(
            "\n// {cite}\nfn cites_one() {{}}\n\n// {cite}\n// :::{{cite}} habeas:util\n// Formats output.\n// :::\nfn cites_two() {{}}\n\n// {{impl}}`requirements.structure_one`\nfn delegates() {{ helper(); }}\nfn helper() {{}}\n// {{impl}}`requirements.structure_two`\npub fn api() {{}}\n"
        ));
        fs::write(&source, text).unwrap();
        for (first, second, template) in [
            (
                "requirements.other",
                "requirements.second",
                "initial_prompt",
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
            assert_eq!(first.structural, template == "structural_prompt");
            assert_eq!(second.structural, template == "structural_prompt");
            let (one, two) = (
                initial_prompt(&first).unwrap(),
                initial_prompt(&second).unwrap(),
            );
            assert_opening(&one, template);
            assert_opening(&two, template);
            for prompt in [&one, &two] {
                assert!(
                    prompt
                        .system
                        .contains(approved_text("habeas_background", 0))
                );
                assert!(prompt.system.contains(approved_text("judgment_rules", 0)));
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
            let (one, two) = (
                verification_prompt(&first, &Judgment::False, "One gap.").unwrap(),
                verification_prompt(&second, &Judgment::Fixme(vec!["#2".into()]), "Two gaps.")
                    .unwrap(),
            );
            assert_opening(&one, "verifier_prompt");
            assert_opening(&two, "verifier_prompt");
            for prompt in [&one, &two] {
                assert!(
                    prompt
                        .system
                        .contains(approved_text("habeas_background", 0))
                );
                assert!(prompt.system.contains(approved_text("judgment_rules", 0)));
                assert_eq!(
                    prompt
                        .system
                        .contains(approved_text("habeas_background", 1)),
                    template != "site_prompt"
                );
            }
            assert_eq!(one.system, two.system, "verifier of {}", first.subject_name);
            assert!(one.subject.contains("One gap."));
            assert!(!one.system.contains("One gap."));
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

    /// Runs the review of site `fn[check_one]` of `requirements.rule` with
    /// `turns`, returning the outcome and the provider.
    // {test}`requirements.references.site_review`
    async fn site_confirmed(
        turns: Vec<(&'static str, Value)>,
    ) -> (io::Result<(Judgment, Value)>, RecordingProvider) {
        let project =
            crate::sites::tests::project(crate::sites::tests::RULE, crate::sites::tests::SOURCE);
        let state = tempfile::tempdir().unwrap();
        let subject = AuthoringSubject::parse("sites:requirements.rule/src/lib.rs#fn[check_one]");
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
            AssessmentPolicy::Confirm,
        )
        .await;
        (result, provider)
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
            "submit_judgment",
            json!({"applies": true, "determination": {"FIXME": ["Panics."]}, "rationale": "Acknowledged."}),
        )]);
        let (_, record) = assess(&provider, &mut sources, &mut usage).await.unwrap();
        let prompts = provider.prompts();
        let (prompt, tools) = &prompts[0];
        let prompt = serde_json::from_str::<Value>(prompt).unwrap();
        let text = prompt.to_string();
        assert!(
            text.contains(
                serde_json::to_string(&initial.subject)
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
        assert!(tools.contains(&"submit_judgment".to_owned()));
        let expected: Vec<_> = tool_definitions_with("submit_judgment", false)
            .unwrap()
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        assert_eq!(*tools, expected);
        let duplicate = json!({"applies":true,"determination":true,"rationale":"Covered."});
        let duplicate_provider = ScriptedProvider {
            turns: vec![vec![
                ("submit_judgment", duplicate.clone()),
                ("submit_judgment", duplicate),
            ]],
            requests: Mutex::default(),
        };
        let mut duplicate_sources = subject.sources(root).unwrap();
        assert!(
            assess(&duplicate_provider, &mut duplicate_sources, &mut usage)
                .await
                .unwrap_err()
                .to_string()
                .contains("exactly once")
        );
        let submit = provider.requests.lock().unwrap()[0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "submit_judgment")
            .unwrap()
            .clone();
        assert_eq!(
            submit["parameters"]["required"],
            json!(["determination", "rationale", "applies"])
        );

        assert_eq!(record["requirement"], "requirements.rule");
        assert_eq!(record["file"], "src/lib.rs");
        assert_eq!(record["item"], "fn[check_two]");
        assert_eq!(record["queries"], json!(["checkers"]));
        assert_eq!(record["applies"], true);
        let outside_scope = RecordingProvider::new(vec![(
            "submit_judgment",
            json!({"applies": false, "determination": true, "rationale": "Outside scope."}),
        )]);
        let mut sources = subject.sources(root).unwrap();
        let (_, outside_record) = assess(&outside_scope, &mut sources, &mut usage)
            .await
            .unwrap();
        assert_eq!(outside_record["applies"], false);
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
    // {test}`requirements.authoring.verifier_outcome`
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

    // {test}`requirements.authoring.verifier_prompt`
    // {test}`requirements.authoring.verifier_outcome`
    #[test]
    fn site_verifications_require_applies_and_no_real_gap_when_it_is_false() {
        let verified = site_verification(&json!({"real": false, "acknowledged": false,
            "acknowledgments": [], "rationale": "Not a site.", "applies": false}))
        .unwrap();
        assert_eq!(verified.applies, Some(false));
        for arguments in [
            json!({"real": false, "acknowledged": false, "acknowledgments": [], "rationale": "x"}),
            json!({"real": true, "acknowledged": false, "acknowledgments": [], "rationale": "x", "applies": false}),
            json!({"real": true, "acknowledged": false, "acknowledgments": [], "rationale": "x", "applies": 1}),
        ] {
            assert!(
                site_verification(&arguments).is_err(),
                "accepted {arguments}"
            );
        }
        let definitions =
            serde_json::to_value(tool_definitions_with("submit_verification", true).unwrap())
                .unwrap();
        let submit = definitions
            .as_array()
            .unwrap()
            .iter()
            .find(|definition| definition["name"] == "submit_verification")
            .unwrap();
        assert_eq!(
            submit["parameters"]["required"],
            json!([
                "real",
                "acknowledged",
                "acknowledgments",
                "rationale",
                "applies"
            ])
        );
        assert_eq!(submit["parameters"]["additionalProperties"], false);
        assert_eq!(
            submit["parameters"]["properties"],
            json!({
                "real":{"type":"boolean"},
                "acknowledged":{"type":"boolean"},
                "acknowledgments":{"type":"array","items":{"type":"string","minLength":1}},
                "rationale":{"type":"string","minLength":1},
                "applies":{"type":"boolean"}
            })
        );
    }

    // {test}`requirements.authoring.confirmation`
    // {test}`requirements.authoring.verifier_prompt`
    // {test}`requirements.authoring.verifier_outcome`
    // {test}`requirements.authoring.habeas_background`
    #[tokio::test]
    // {test}`requirements.authoring.judgment_rules`
    async fn site_assessments_differing_only_in_applies_are_verified() {
        let site = |applies: bool, determination: Value, rationale: &str| {
            (
                "submit_judgment",
                json!({"applies": applies, "determination": determination, "rationale": rationale}),
            )
        };
        let check = |applies: bool, real: bool, rationale: &str| {
            (
                "submit_verification",
                json!({"real": real, "acknowledged": false, "acknowledgments": [],
                    "rationale": rationale, "applies": applies}),
            )
        };
        // Agreement needs the same applies too.
        let (result, provider) = site_confirmed(vec![
            site(true, json!(true), "Reports problems."),
            site(true, json!(true), "Also fine."),
        ])
        .await;
        assert_eq!(result.unwrap().1["rationale"], "Reports problems.");
        assert_eq!(provider.requests.lock().unwrap().len(), 2);

        for (turns, recorded, applies, kept, reviewed) in [
            // The verifier finds the requirement does not apply, unlike the
            // more favorable assessment it keeps: true on its rationale.
            (
                vec![
                    site(false, json!(true), "Not a checker."),
                    site(true, json!(true), "Reports problems."),
                    check(true, false, "It is a checker after all."),
                ],
                Judgment::True,
                true,
                "It is a checker after all.",
                "Reports problems.",
            ),
            // The verifier agrees with the more favorable assessment.
            (
                vec![
                    site(true, json!(true), "Reports problems."),
                    site(false, json!(true), "Not a checker."),
                    check(false, false, "Only a helper."),
                ],
                Judgment::True,
                false,
                "Not a checker.",
                "Reports problems.",
            ),
            // A real gap in the applying assessment records false.
            (
                vec![
                    site(false, json!(true), "Not a checker."),
                    site(true, json!(false), "It panics."),
                    check(true, true, "It panics in `src/lib.rs#fn[check_one]`."),
                ],
                Judgment::False,
                true,
                "It panics.",
                "It panics.",
            ),
            (
                vec![
                    site(false, json!(true), "Not a checker."),
                    site(true, json!(false), "It panics."),
                    (
                        "submit_verification",
                        json!({
                            "applies":true, "real":true, "acknowledged":true,
                            "acknowledgments":["o/r#1"], "rationale":"The site acknowledges its panic."
                        }),
                    ),
                ],
                Judgment::Fixme(vec!["o/r#1".into()]),
                true,
                "It panics.",
                "It panics.",
            ),
            (
                vec![
                    site(true, json!({"FIXME":["o/r#1"]}), "The known gap."),
                    site(true, json!(false), "Another gap."),
                    check(false, false, "Only a helper."),
                ],
                Judgment::True,
                false,
                "Only a helper.",
                "Another gap.",
            ),
        ] {
            let (result, provider) = site_confirmed(turns.clone()).await;
            let (judgment, record) = result.unwrap();
            assert_eq!(judgment, recorded);
            assert_eq!(record["determination"], judgment.to_value());
            assert_eq!(record["applies"], applies, "{record}");
            let rationale = record["rationale"].as_str().unwrap();
            assert!(rationale.starts_with(kept), "{rationale}");
            assert!(
                rationale.contains("the assessments disagreed"),
                "{rationale}"
            );

            let prompts = provider.prompts();
            let (text, tools) = &prompts[2];
            let text = serde_json::from_str::<Value>(text).unwrap().to_string();
            let escaped = |value: &str| {
                serde_json::to_string(value)
                    .unwrap()
                    .trim_matches('"')
                    .to_owned()
            };
            assert!(
                text.contains(&escaped(
                    "The reviewer judged site review sites:requirements.rule/src/lib.rs#fn[check_one] as"
                )),
                "{text}"
            );
            assert!(
                text.contains(&escaped(approved_text("judgment_rules", 0))),
                "{text}"
            );
            assert!(text.contains(&escaped(SITE_STEP)), "{text}");
            assert!(
                text.contains(&escaped(approved_text("habeas_background", 0))),
                "{text}"
            );
            assert!(
                text.contains(&escaped(&format!(
                    "requirement states for each of its sites.\n\n{}\n\nThe mandatory inputs at the end contain the site",
                    distributed_background()
                ))),
                "{text}"
            );
            assert!(
                !text.contains(&escaped(approved_text("habeas_background", 1))),
                "{text}"
            );
            assert!(!text.contains("{{"), "{text}");
            assert!(text.contains(&escaped(reviewed)), "{text}");
            assert!(text.contains("# Mandatory inputs"), "{text}");
            assert!(tools.contains(&"submit_verification".to_owned()));
            assert!(!tools.contains(&"submit_judgment".to_owned()));
            let requests = provider.requests.lock().unwrap();
            let subject = requests[2]["chat_history"][1]["content"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|part| part["text"].as_str())
                .collect::<String>();
            let inputs: Value =
                serde_json::from_str(prompt_section(&subject, "Mandatory inputs").trim()).unwrap();
            assert_eq!(
                inputs[0]["text"],
                json!(["fn check_one() { panic!(\"x\"); }"])
            );
            assert_eq!(
                requests[2]["tools"],
                serde_json::to_value(tool_definitions_with("submit_verification", true).unwrap())
                    .unwrap()
            );
            let source_tools = |request: &Value| {
                request["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|tool| {
                        !matches!(
                            tool["name"].as_str(),
                            Some("submit_judgment" | "submit_verification")
                        )
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            };
            assert_eq!(source_tools(&requests[2]), source_tools(&requests[0]));
            assert_eq!(source_tools(&requests[2]), source_tools(&requests[1]));
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
        assert!(sources.structural);
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
        assert!(unanchored.structural);
        assert_eq!(unanchored.mandatory_inputs(), inputs);
        // A structural assessment also includes plain-text implementation roots
        // and complete refining declarations, without recursively reading helpers.
        let plain = format!("Plain implementation\n{impl_role}\nWhole file content.\n");
        fs::write(root.path().join("src/details.txt"), &plain).unwrap();
        fs::write(root.path().join("md/requirements.md"), format!(":::{{req}} ready\nThe public api function delegates to helper.\n:::\n:::{{req}} refined\n{impl_role}\nComplete refinement.\nSecond paragraph.\n:::\n")).unwrap();
        let refined = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        assert!(refined.structural);
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
        // Adding a test selects test adequacy, including with an implementation anchor.
        fs::write(
            root.path().join("src/lib.rs"),
            format!("{implementation}\n// {test_role}\n#[test]\nfn checks_api() {{ api(); }}\n"),
        )
        .unwrap();
        let tested = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        assert!(!tested.structural);
        assert!(
            !tested
                .mandatory_inputs()
                .iter()
                .any(|input| input["item"] == "fn[api]")
        );
        let definitions = crate::evidence::load_definitions(root.path()).unwrap();
        let domain = crate::sdk::HabeasDomain::new(root.path(), &definitions, &tested.outlines);
        assert_eq!(
            tested.reference_set,
            crate::evidence::digest_entries(
                domain
                    .reference_entries(&crate::sdk::RequirementId("requirements.ready".into()))
                    .unwrap()
            )
        );
        // Test anchors have the same precedence as in-source test references.
        fs::write(root.path().join("src/lib.rs"), &implementation).unwrap();
        fs::write(
            root.path().join("md/requirements.md"),
            declaration.replace(":impl:", ":test:"),
        )
        .unwrap();
        assert!(
            !AssessmentSources::prepare(root.path(), "requirements.ready")
                .unwrap()
                .structural
        );
        // An implementation anchor alone is enough, without an in-source reference.
        fs::write(root.path().join("src/lib.rs"), "pub fn api() {}\n").unwrap();
        fs::write(root.path().join("md/requirements.md"), declaration).unwrap();
        assert!(
            AssessmentSources::prepare(root.path(), "requirements.ready")
                .unwrap()
                .structural
        );
        fs::write(
            root.path().join("md/requirements.md"),
            ":::{req} base\nContext.\n:::\n:::{example} ready\nA case. {cite}`requirements.base`\n:::\n",
        )
        .unwrap();
        fs::write(root.path().join("src/lib.rs"), &implementation).unwrap();
        assert!(
            !AssessmentSources::prepare(root.path(), "requirements.ready")
                .unwrap()
                .structural
        );
        let distributed = format!("{declaration}\n").replace("The public api", "::::{sites} functions\n```yaml\nlanguage: rust\nrule: {kind: function_item}\nmatch: ['fn api() {}']\nno_match: ['struct Other;']\n```\n::::\nThe public api");
        fs::write(root.path().join("md/requirements.md"), distributed).unwrap();
        assert!(
            !AssessmentSources::prepare(root.path(), "requirements.ready")
                .unwrap()
                .structural
        );
        fs::write(root.path().join("md/requirements.md"), format!(":::{{req}} ready\nStructure.\n:::\n:::{{req}} refined\n{impl_role}\nRefinement.\n:::\n")).unwrap();
        fs::write(root.path().join("src/lib.rs"), "fn unreferenced() {}\n").unwrap();
        assert!(
            !AssessmentSources::prepare(root.path(), "requirements.ready")
                .unwrap()
                .structural
        );
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
        fs::write(
            root.path().join("habeas.toml"),
            "version=1\n[evidence]\ndisallow=['src/lib.rs']\n",
        )
        .unwrap();
        let error = AssessmentSources::prepare(root.path(), "requirements.ready")
            .err()
            .unwrap();
        assert!(error.to_string().contains("disallowed"), "{error}");
        fs::remove_file(root.path().join("habeas.toml")).unwrap();
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
        let omitted = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        assert!(!omitted.structural);
        fs::write(
            root.path().join("habeas.toml"),
            "version=1\n[evidence]\nallow=['evidence/generated.rs']\n",
        )
        .unwrap();
        let allowed = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        assert!(allowed.structural);
        assert!(
            allowed
                .mandatory_inputs()
                .iter()
                .any(|input| input["file"] == "evidence/generated.rs")
        );
        // An omitted generated test edge cannot override a real implementation.
        fs::remove_file(root.path().join("habeas.toml")).unwrap();
        fs::write(
            root.path().join("src/lib.rs"),
            format!("// {role}\nfn api() {{}}\n"),
        )
        .unwrap();
        fs::write(
            root.path().join("md/requirements.md"),
            ":::{req} ready\n:test: evidence/generated.rs\nStructure.\n:::\n",
        )
        .unwrap();
        assert!(
            AssessmentSources::prepare(root.path(), "requirements.ready")
                .unwrap()
                .structural
        );
        fs::write(
            root.path().join("habeas.toml"),
            "version=1\n[evidence]\nallow=['evidence/generated.rs']\n",
        )
        .unwrap();
        assert!(
            !AssessmentSources::prepare(root.path(), "requirements.ready")
                .unwrap()
                .structural
        );
        fs::remove_file(root.path().join("habeas.toml")).unwrap();
        // A plain-text implementation alone is not a supported code profile.
        fs::write(root.path().join("src/lib.rs"), "fn unreferenced() {}\n").unwrap();
        fs::write(
            root.path().join("src/details.txt"),
            format!("{role}\nPlain content.\n"),
        )
        .unwrap();
        fs::write(
            root.path().join("md/requirements.md"),
            ":::{req} ready\n:impl: src/details.txt\nStructure.\n:::\n",
        )
        .unwrap();
        assert!(
            !AssessmentSources::prepare(root.path(), "requirements.ready")
                .unwrap()
                .structural
        );
    }

    // {test}`requirements.authoring.structural_prompt`
    // {test}`requirements.authoring.structural_review`
    // {test}`requirements.authoring.confirmation`
    // {test}`requirements.authoring.verifier_prompt`
    // {test}`requirements.ci.review_criteria`
    #[tokio::test]
    async fn structural_assessments_and_confirmation_use_the_same_structural_contract() {
        let root = inputs::tests::project();
        let role = inputs::tests::role("impl", "requirements.ready");
        fs::write(
            root.path().join("src/lib.rs"),
            format!("// {role}\n// :::{{fixme}} requirements.ready\n// The remaining structural gap.\n// :::\npub fn api() {{}}\n"),
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let initial = initial_prompt(&sources).unwrap();
        for text in [
            "stated structural obligations",
            "Inspect the supplied implementation roots",
            "Observable runtime",
            "testing gap",
            "marking gap",
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
        let verification = verification_prompt(
            &sources,
            &Judgment::False,
            "The required signature is absent.",
        )
        .unwrap();
        assert!(verification.subject.contains("using structural assessment"));
        assert!(
            verification
                .subject
                .contains(prompt_section(&initial.system, "Your task"))
        );
        let provider = RecordingProvider::new(vec![
            judgment(json!(true), "The structure is present."),
            judgment(json!(false), "The required signature is absent."),
            (
                "submit_verification",
                json!({"real":false,"acknowledged":false,"acknowledgments":[],"rationale":"The signature is present."}),
            ),
        ]);
        let state = tempfile::tempdir().unwrap();
        let mut usage = UsageLog::start(state.path(), root.path(), "requirements.ready").unwrap();
        let subject = AuthoringSubject::Definition("requirements.ready".into());
        let (answer, record) = assess_subject(
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
        assert_eq!(answer, Judgment::True);
        assert!(
            record["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|input| input["item"] == "fn[api]")
        );
        let prompts = provider.prompts();
        assert_eq!(prompts.len(), 3);
        assert!(prompts.iter().all(
            |(prompt, _)| prompt.contains("stated structural obligations")
                || prompt.contains("Inspect the supplied implementation roots")
        ));
        let requests = provider.requests.lock().unwrap();
        for (index, request) in requests.iter().enumerate() {
            let submit_name = if index == 2 {
                "submit_verification"
            } else {
                "submit_judgment"
            };
            let expected =
                serde_json::to_value(tool_definitions_with(submit_name, false).unwrap()).unwrap();
            assert_eq!(
                request["tools"], expected,
                "structural conversation {index} must retain tools and submission schemas"
            );
            let prompt = &prompts[index].0;
            assert!(prompt.contains("requirements.ready"));
            assert!(prompt.contains("The remaining structural gap."));
        }
    }
}

async fn assess(
    provider: &impl CompletionProvider,
    sources: &mut AssessmentSources,
    usage: &mut UsageLog,
) -> io::Result<(Judgment, Value)> {
    sdk::assess(provider, sources, usage).await
}

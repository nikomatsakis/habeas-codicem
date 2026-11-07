use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use tokio::sync::watch;
use tokio::task::JoinSet;

use super::progress::BatchProgress;
use super::{
    AuthoringOptions, AuthoringOutcome, AuthoringSubject, PendingAssessment, prepare_assessment,
};
use crate::DefinitionKind;
use crate::config;
use crate::evidence::{InspectionProgress, inspect_repository_with_progress};
use crate::index::{EvidenceState, LoadedDetailEvidence};

// {cite}`requirements.authoring.batch`
pub struct BatchOutcome {
    pub completed: Vec<AuthoringOutcome>,
    pub failures: Vec<(String, String)>,
    pub unchanged: usize,
}

// {impl}`requirements.authoring.batch`
// {impl}`requirements.authoring.concurrency`
// {impl}`requirements.authoring.configuration_precedence`
// {impl}`requirements.authoring.read_denial`
// {impl}`requirements.authoring.progress`
pub async fn author_all(root: &Path, selection: Option<&str>) -> io::Result<BatchOutcome> {
    author_all_with_options(root, selection, AuthoringOptions::default()).await
}

// {impl}`requirements.authoring.batch`
// {impl}`requirements.authoring.concurrency`
// {impl}`requirements.authoring.configuration_precedence`
// {impl}`requirements.authoring.read_denial`
// {impl}`requirements.authoring.progress`
// {impl}`requirements.authoring.reporting`
// {impl}`requirements.ci.authoring_inputs`
// {impl}`requirements.configuration.loading`
// {impl}`requirements.authoring.confirmation`
// {impl}`requirements.references.citation_review`
pub async fn author_all_with_options(
    root: &Path,
    selection: Option<&str>,
    options: AuthoringOptions,
) -> io::Result<BatchOutcome> {
    let root = fs::canonicalize(root)?;
    let mut progress = BatchProgress::new(options.verbose);
    let inspection = inspect_repository_with_progress(&root, |update| progress.inspect(update))?;
    if !inspection.discovery_errors.is_empty() {
        return Err(io::Error::other(inspection.discovery_errors.join("\n")));
    }
    let mut subjects = Vec::new();
    let mut unchanged = 0;
    let total = inspection.definitions.len();
    progress.inspect(InspectionProgress {
        stage: "selecting assessments",
        completed: 0,
        total: Some(total),
    });
    for (checked, (name, definition)) in inspection.definitions.iter().enumerate() {
        if definition.kind != DefinitionKind::Term {
            let evidence = LoadedDetailEvidence::load(&root, name, &inspection.report);
            // A current false judgment stays until its subject or inputs change.
            if matches!(
                evidence.state(),
                EvidenceState::Valid | EvidenceState::Fixme | EvidenceState::DeterminationFalse
            ) {
                unchanged += 1;
            } else {
                subjects.push(AuthoringSubject::Definition(name.clone()));
            }
        }
        progress.inspect(InspectionProgress {
            stage: "selecting assessments",
            completed: checked + 1,
            total: Some(total),
        });
    }
    // Every source item outside the book with citations needs a review.
    // {impl}`requirements.references.citation_review`
    let outlines = crate::inputs::OutlineCache::default();
    for item in crate::citations::citation_items(
        &root,
        &inspection.definitions,
        &inspection.references,
        &outlines,
    )
    .into_values()
    {
        let evidence =
            LoadedDetailEvidence::load_path(&root, &item.review_path(), &inspection.report);
        if matches!(
            evidence.state(),
            EvidenceState::Valid | EvidenceState::Fixme | EvidenceState::DeterminationFalse
        ) {
            unchanged += 1;
        } else {
            subjects.push(AuthoringSubject::Citation {
                file: item.file,
                item: item.item,
            });
        }
    }
    let mut outcome = BatchOutcome {
        completed: Vec::new(),
        failures: Vec::new(),
        unchanged,
    };
    if subjects.is_empty() {
        return Ok(outcome);
    }
    let config = config::resolve_authoring(&root, selection)?;
    let concurrency = config.effective_concurrency();
    progress.println(&format!(
        "Authoring {} assessments with concurrency {concurrency}; {unchanged} current records unchanged",
        subjects.len()
    ));
    progress.start(subjects.len());
    let mut subjects = subjects.into_iter();
    let mut tasks = JoinSet::new();
    let (cancel, cancellation) = watch::channel(None::<&'static str>);
    // The batch listens for interruption itself, so a signal arriving while no
    // assessment is waiting (between tasks, or while a record is written)
    // still stops the batch. Poll once now to register the listener before
    // any assessment starts; registered signals are kept until polled again.
    let interrupt = tokio::signal::ctrl_c();
    tokio::pin!(interrupt);
    let mut listening = true;
    observe_interrupt(interrupt.as_mut(), &mut listening, &cancel).await;
    loop {
        // A signal received while the batch was busy stops further launches.
        observe_interrupt(interrupt.as_mut(), &mut listening, &cancel).await;
        while cancellation.borrow().is_none() && tasks.len() < concurrency {
            let Some(subject) = subjects.next() else {
                break;
            };
            let root = root.clone();
            let config = config.clone();
            let cancellation = cancellation.clone();
            let cancel = cancel.clone();
            tasks.spawn(async move {
                let result =
                    prepare_assessment(&root, &subject, &config, options.confirm, cancellation)
                        .await;
                if let Err(error) = &result {
                    cancel_on_error(&cancel, error);
                }
                (subject.label(), result)
            });
        }
        progress.refresh(tasks.len(), subjects.len());
        let joined = tokio::select! {
            biased;
            signal = &mut interrupt, if listening => {
                listening = false;
                if signal.is_ok() {
                    stop(&cancel);
                }
                continue;
            }
            joined = tasks.join_next() => joined,
        };
        let Some(result) = joined else {
            break;
        };
        let (label, result) = match result {
            Ok(result) => result,
            Err(error) => {
                progress.failed("authoring task", &error);
                outcome
                    .failures
                    .push(("authoring task".into(), error.to_string()));
                continue;
            }
        };
        // ...and discards a judgment not yet written when it arrived.
        observe_interrupt(interrupt.as_mut(), &mut listening, &cancel).await;
        let result = result.and_then(|pending| {
            // Serialize the stop signal with persistence: a write already in
            // progress finishes before the signal; later completions are discarded.
            let stop_reason = cancellation.borrow();
            if let Some(reason) = *stop_reason {
                return Err(pending.failed(io::Error::new(io::ErrorKind::Interrupted, reason)));
            }
            match write_record(&root, &pending) {
                Ok(path) => pending.persisted(path),
                Err(error) => Err(pending.failed(error)),
            }
        });
        match result {
            Ok(assessment) => {
                progress.completed(&assessment);
                outcome.completed.push(assessment);
            }
            Err(error) => {
                cancel_on_error(&cancel, &error);
                progress.failed(&label, &error);
                outcome.failures.push((label, error.to_string()));
            }
        }
    }
    if let Some(reason) = *cancellation.borrow() {
        let remaining = subjects.len();
        if remaining > 0 {
            let message = format!("{reason}; {remaining} assessments not started");
            progress.println(&format!("batch: {message}"));
            outcome.failures.push(("batch".into(), message));
        }
    }
    progress.finish(cancellation.borrow().is_some(), subjects.len());
    Ok(outcome)
}

// {impl}`requirements.authoring.batch`
// {impl}`requirements.authoring.read_denial`
fn cancel_on_error(cancel: &watch::Sender<Option<&'static str>>, error: &io::Error) {
    if error.kind() == io::ErrorKind::Interrupted {
        stop(cancel);
    }
}

/// Records an interruption that has already arrived, without waiting. The
/// first poll registers the listener.
// {impl}`requirements.authoring.batch`
async fn observe_interrupt(
    interrupt: std::pin::Pin<&mut impl std::future::Future<Output = io::Result<()>>>,
    listening: &mut bool,
    cancel: &watch::Sender<Option<&'static str>>,
) {
    if !*listening {
        return;
    }
    tokio::select! {
        biased;
        signal = interrupt => {
            *listening = false;
            if signal.is_ok() {
                stop(cancel);
            }
        }
        () = std::future::ready(()) => {}
    }
}

/// Signals every assessment and the launcher that the batch is stopping.
// {impl}`requirements.authoring.batch`
fn stop(cancel: &watch::Sender<Option<&'static str>>) {
    cancel.send_if_modified(|current| {
        if current.is_none() {
            *current = Some("evidence authoring cancelled");
            true
        } else {
            false
        }
    });
}

// {impl}`requirements.authoring.batch`
// {impl}`requirements.authoring.persistence`
fn write_record(root: &Path, pending: &PendingAssessment) -> io::Result<PathBuf> {
    let path = pending.subject.evidence_path(root);
    let record = pending.record.clone();
    let parent = path.parent().expect("evidence parent");
    fs::create_dir_all(parent)?;
    // Other assessments may read the existing selectors while this one finishes.
    // Replace the complete file so those reads see either complete version.
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    writeln!(file, "{}", serde_json::to_string_pretty(&record)?)?;
    file.persist(&path).map_err(|error| error.error)?;
    Ok(path)
}

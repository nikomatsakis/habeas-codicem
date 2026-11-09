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
use crate::config::{self, AssessmentPolicy};
use crate::evidence::{InspectionProgress, RepositoryInspection, inspect_repository_with_progress};
use crate::index::{EvidenceState, LoadedDetailEvidence};

// {impl}`requirements.authoring.batch`
// {impl}`requirements.authoring.refresh`
// {impl}`requirements.authoring.policy`
pub struct BatchOutcome {
    pub completed: Vec<AuthoringOutcome>,
    pub failures: Vec<(String, String)>,
    /// Selected subjects skipped because their evidence is current.
    pub unchanged: usize,
    /// Orphaned review files deleted, relative to the project.
    pub deleted: Vec<String>,
    /// The policy assessments ran under, when any ran.
    pub policy: Option<AssessmentPolicy>,
}

/// The inspection phase that selects definitions and citation reviews.
// {impl}`requirements.authoring.progress`
const SELECTION_STAGE: &str = "selecting assessments";

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
// {impl}`requirements.authoring.github_reads`
// {impl}`requirements.authoring.refresh`
// {impl}`requirements.authoring.policy`
pub async fn author_all_with_options(
    root: &Path,
    selection: Option<&str>,
    options: AuthoringOptions,
) -> io::Result<BatchOutcome> {
    run_batch(root, selection, options, None).await
}

/// Authors the site reviews of one distributed requirement, as
/// `habeas author sites:<full-name>`, under the batch's refresh rules.
// {impl}`requirements.references.site_review`
// {impl}`requirements.authoring.refresh`
pub async fn author_sites_with_options(
    root: &Path,
    requirement: &str,
    selection: Option<&str>,
    options: AuthoringOptions,
) -> io::Result<BatchOutcome> {
    run_batch(root, selection, options, Some(requirement)).await
}

/// Runs a batch over every subject, or, with `sites_of`, over the site
/// reviews of one distributed requirement.
// {impl}`requirements.authoring.batch`
// {impl}`requirements.authoring.concurrency`
// {impl}`requirements.authoring.read_denial`
// {impl}`requirements.authoring.progress`
// {impl}`requirements.authoring.reporting`
// {impl}`requirements.authoring.confirmation`
// {impl}`requirements.references.citation_review`
// {impl}`requirements.references.site_review`
// {impl}`requirements.authoring.github_reads`
// {impl}`requirements.authoring.refresh`
// {impl}`requirements.authoring.policy`
// {impl}`requirements.authoring.cache_warmup`
async fn run_batch(
    root: &Path,
    selection: Option<&str>,
    options: AuthoringOptions,
    sites_of: Option<&str>,
) -> io::Result<BatchOutcome> {
    let root = fs::canonicalize(root)?;
    let mut progress = BatchProgress::new(options.verbose);
    let mut inspection =
        inspect_repository_with_progress(&root, |update| progress.inspect(update))?;
    if !inspection.discovery_errors.is_empty() {
        return Err(io::Error::other(inspection.discovery_errors.join("\n")));
    }
    // A FIXME record whose acknowledging issue closed is stale.
    // {impl}`requirements.authoring.github_reads`
    crate::github::add_issue_findings(
        &mut inspection,
        &crate::github::GithubIssues::from_env(&root),
    );
    if let Some(requirement) = sites_of
        && !inspection.sites.contains_key(requirement)
    {
        return Err(io::Error::other(format!(
            "sites:{requirement}: no distributed requirement named {requirement}"
        )));
    }
    // Orphaned reviews are deleted before, and independently of, any model
    // configuration.
    let deleted = if sites_of.is_none() {
        delete_orphaned_reviews(&root, &inspection)?
    } else {
        Vec::new()
    };
    for path in &deleted {
        progress.println(&format!("Deleted orphaned review {path}"));
    }
    let (subjects, unchanged) =
        select_subjects(&root, &inspection, sites_of, options.force, &mut |update| {
            progress.inspect(update)
        });
    let mut outcome = BatchOutcome {
        completed: Vec::new(),
        failures: Vec::new(),
        unchanged,
        deleted,
        policy: None,
    };
    // With nothing to assess, no configuration or credentials are resolved.
    // {impl}`requirements.authoring.configuration`
    if subjects.is_empty() {
        return Ok(outcome);
    }
    let config = config::resolve_authoring(&root, selection)?;
    let concurrency = config.effective_concurrency();
    let policy = config.effective_policy(options.policy);
    outcome.policy = Some(policy);
    progress.println(&format!(
        "Authoring {} assessments under the {policy} policy with concurrency {concurrency}; {unchanged} current records skipped",
        subjects.len()
    ));
    progress.start(subjects.len());
    let mut subjects = subjects
        .into_iter()
        .map(|subject| (assessment_kind(&inspection, &subject), subject))
        .collect::<Vec<_>>();
    let mut warmup = CacheWarmup::default();
    let (warmed, mut warmed_kinds) = tokio::sync::mpsc::unbounded_channel();
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
            let Some((kind, subject, warms)) = warmup.next(&mut subjects) else {
                break;
            };
            let root = root.clone();
            let config = config.clone();
            let cancellation = cancellation.clone();
            let cancel = cancel.clone();
            let first_request_finished = warms.then(|| {
                let warmed = warmed.clone();
                Box::new(move || {
                    let _ = warmed.send(kind);
                }) as Box<dyn FnOnce() + Send>
            });
            tasks.spawn(async move {
                let result = prepare_assessment(
                    &root,
                    &subject,
                    &config,
                    policy,
                    cancellation,
                    first_request_finished,
                )
                .await;
                if let Err(error) = &result {
                    cancel_on_error(&cancel, error);
                }
                (kind, subject.label(), result)
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
            Some(kind) = warmed_kinds.recv() => {
                warmup.warmed(kind);
                continue;
            }
            joined = tasks.join_next() => joined,
        };
        let Some(result) = joined else {
            break;
        };
        // A warming assessment signals before it finishes.
        while let Ok(kind) = warmed_kinds.try_recv() {
            warmup.warmed(kind);
        }
        let (label, result) = match result {
            Ok((kind, label, result)) => {
                warmup.finished(kind);
                (label, result)
            }
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

/// The kind of a subject's assessment, whose system prompt every
/// assessment of that kind shares: requirement, example, citation review, or
/// site review.
// {impl}`requirements.authoring.cache_warmup`
// {impl}`requirements.authoring.prompt_parts`
fn assessment_kind(inspection: &RepositoryInspection, subject: &AuthoringSubject) -> &'static str {
    match subject {
        AuthoringSubject::Definition(name) => inspection
            .definitions
            .get(name)
            .map_or("requirement", |definition| definition.kind.noun()),
        AuthoringSubject::Citation { .. } => "citation review",
        AuthoringSubject::Site { .. } => "site review",
    }
}

/// Which assessment kinds have warmed their prompt cache: a kind's first
/// assessment starts alone, and the others of its kind wait until its first
/// provider request has finished, while other kinds proceed.
// {impl}`requirements.authoring.cache_warmup`
#[derive(Default)]
struct CacheWarmup {
    /// Kinds whose first assessment is running but has not yet finished its
    /// first provider request.
    warming: std::collections::BTreeSet<&'static str>,
    /// Kinds whose cache an earlier provider request has written.
    warm: std::collections::BTreeSet<&'static str>,
}

impl CacheWarmup {
    /// Takes the first queued subject that may start now, and whether it
    /// warms its kind: a subject of a warm kind, or the first of a kind
    /// with none warming.
    // {impl}`requirements.authoring.cache_warmup`
    fn next(
        &mut self,
        queue: &mut Vec<(&'static str, AuthoringSubject)>,
    ) -> Option<(&'static str, AuthoringSubject, bool)> {
        let index = queue
            .iter()
            .position(|(kind, _)| !self.warming.contains(kind))?;
        let (kind, subject) = queue.remove(index);
        let warms = !self.warm.contains(kind);
        if warms {
            self.warming.insert(kind);
        }
        Some((kind, subject, warms))
    }

    /// Records that a kind's first provider request has finished.
    // {impl}`requirements.authoring.cache_warmup`
    fn warmed(&mut self, kind: &'static str) {
        self.warming.remove(kind);
        self.warm.insert(kind);
    }

    /// Records that an assessment of a kind finished. One that finished
    /// without a provider request warmed nothing, so another of its kind
    /// takes its place.
    // {impl}`requirements.authoring.cache_warmup`
    fn finished(&mut self, kind: &'static str) {
        self.warming.remove(kind);
    }
}

/// Deletes each citation or site review whose item or site no longer exists,
/// returning their project-relative paths.
// {impl}`requirements.authoring.batch`
// {impl}`requirements.references.citation_review`
// {impl}`requirements.references.site_review`
fn delete_orphaned_reviews(
    root: &Path,
    inspection: &RepositoryInspection,
) -> io::Result<Vec<String>> {
    let outlines = crate::inputs::OutlineCache::default();
    let mut expected = crate::citations::citation_items(
        root,
        &inspection.definitions,
        &inspection.references,
        &outlines,
    )
    .values()
    .map(|item| item.review_path())
    .collect::<std::collections::BTreeSet<_>>();
    expected.extend(
        inspection
            .sites
            .values()
            .flatten()
            .map(crate::sites::SiteItem::review_path),
    );
    let mut deleted = Vec::new();
    for directory in [crate::citations::CITATIONS_DIR, crate::sites::SITES_DIR] {
        let directory = root.join("evidence").join(directory);
        if !directory.is_dir() {
            continue;
        }
        // A listing failure aborts, so this is not a validation site.
        let mut failures = Vec::new();
        let files = crate::evidence::files_with_extension(root, &directory, "json", &mut failures);
        if !failures.is_empty() {
            return Err(io::Error::other(failures.join("\n")));
        }
        for file in files {
            let relative = crate::evidence::display_path(root, &file);
            if !expected.contains(&relative) {
                fs::remove_file(&file)?;
                deleted.push(relative);
            }
        }
    }
    Ok(deleted)
}

/// Whether a record is current: valid, an accepted FIXME, or a false
/// judgment whose subject and inputs are unchanged.
// {impl}`requirements.authoring.refresh`
fn current(evidence: &LoadedDetailEvidence) -> bool {
    matches!(
        evidence.state(),
        EvidenceState::Valid | EvidenceState::Fixme | EvidenceState::DeterminationFalse
    )
}

/// Whether one named subject's evidence is current, so that `--missing`
/// keeps it without resolving configuration. A subject naming no current
/// definition, item, or site is not current; its assessment reports it.
// {impl}`requirements.authoring.refresh`
// {impl}`requirements.authoring.invocation`
pub(super) fn subject_is_current(root: &Path, subject: &AuthoringSubject) -> io::Result<bool> {
    let mut inspection = crate::evidence::inspect_repository(root)?;
    if !inspection.discovery_errors.is_empty() {
        return Err(io::Error::other(inspection.discovery_errors.join("\n")));
    }
    // A FIXME record whose acknowledging issue closed is stale.
    // {impl}`requirements.authoring.github_reads`
    crate::github::add_issue_findings(
        &mut inspection,
        &crate::github::GithubIssues::from_env(&root),
    );
    let path = match subject {
        AuthoringSubject::Definition(name) => {
            if inspection
                .definitions
                .get(name)
                .is_none_or(|definition| definition.kind == DefinitionKind::Term)
            {
                return Ok(false);
            }
            format!("evidence/{name}.json")
        }
        AuthoringSubject::Citation { file, item } => {
            let path = crate::citations::review_path(file, item);
            let outlines = crate::inputs::OutlineCache::default();
            let items = crate::citations::citation_items(
                root,
                &inspection.definitions,
                &inspection.references,
                &outlines,
            );
            if !items.values().any(|item| item.review_path() == path) {
                return Ok(false);
            }
            path
        }
        AuthoringSubject::Site {
            requirement,
            file,
            item,
        } => {
            let path = crate::sites::review_path(requirement, file, item);
            if !inspection
                .sites
                .values()
                .flatten()
                .any(|site| site.review_path() == path)
            {
                return Ok(false);
            }
            path
        }
    };
    Ok(current(&LoadedDetailEvidence::load_path(
        root,
        &path,
        &inspection.report,
    )))
}

/// Selects the definitions, citation reviews, and site reviews needing
/// assessment, or with `sites_of` only that requirement's site reviews,
/// counting current records as unchanged, and reports selection as one
/// counted phase. With `force`, every subject is selected.
// {impl}`requirements.authoring.batch`
// {impl}`requirements.authoring.progress`
// {impl}`requirements.references.citation_review`
// {impl}`requirements.references.site_review`
// {impl}`requirements.authoring.refresh`
fn select_subjects(
    root: &Path,
    inspection: &RepositoryInspection,
    sites_of: Option<&str>,
    force: bool,
    progress: &mut dyn FnMut(InspectionProgress),
) -> (Vec<AuthoringSubject>, usize) {
    let mut subjects = Vec::new();
    let mut unchanged = 0;
    // Selection checks every definition and every item needing a citation
    // review; finding those items comes first, with an activity display.
    progress(InspectionProgress {
        stage: SELECTION_STAGE,
        completed: 0,
        total: None,
    });
    // Every source item outside the book with citations needs a review.
    // {impl}`requirements.references.citation_review`
    let outlines = crate::inputs::OutlineCache::default();
    let (definitions, items) = if sites_of.is_some() {
        (Default::default(), Default::default())
    } else {
        (
            inspection.definitions.iter().collect::<Vec<_>>(),
            crate::citations::citation_items(
                root,
                &inspection.definitions,
                &inspection.references,
                &outlines,
            ),
        )
    };
    let sites = inspection
        .sites
        .iter()
        .filter(|(name, _)| sites_of.is_none_or(|selected| selected == *name))
        .flat_map(|(_, sites)| sites)
        .collect::<Vec<_>>();
    let total = definitions.len() + items.len() + sites.len();
    let mut checked = 0;
    let mut advance = |progress: &mut dyn FnMut(InspectionProgress)| {
        progress(InspectionProgress {
            stage: SELECTION_STAGE,
            completed: checked,
            total: Some(total),
        });
        checked += 1;
    };
    advance(progress);
    for (name, definition) in definitions {
        if definition.kind != DefinitionKind::Term {
            let evidence = LoadedDetailEvidence::load(root, name, &inspection.report);
            // A current false judgment stays until its subject or inputs change.
            if !force && current(&evidence) {
                unchanged += 1;
            } else {
                subjects.push(AuthoringSubject::Definition(name.clone()));
            }
        }
        advance(progress);
    }
    for item in items.into_values() {
        let evidence =
            LoadedDetailEvidence::load_path(root, &item.review_path(), &inspection.report);
        if !force && current(&evidence) {
            unchanged += 1;
        } else {
            subjects.push(AuthoringSubject::Citation {
                file: item.file,
                item: item.item,
            });
        }
        advance(progress);
    }
    // Every site of a distributed requirement needs a review.
    // {impl}`requirements.references.site_review`
    for site in sites {
        let evidence =
            LoadedDetailEvidence::load_path(root, &site.review_path(), &inspection.report);
        if !force && current(&evidence) {
            unchanged += 1;
        } else {
            subjects.push(AuthoringSubject::Site {
                requirement: site.requirement.clone(),
                file: site.file.clone(),
                item: site.item.clone(),
            });
        }
        advance(progress);
    }
    (subjects, unchanged)
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

#[cfg(test)]
mod tests {
    use super::*;

    // {test}`requirements.authoring.cache_warmup`
    #[test]
    fn each_kind_starts_one_assessment_until_its_first_request_finishes() {
        let subject = |name: &str| AuthoringSubject::Definition(name.into());
        let mut queue = vec![
            ("requirement", subject("a")),
            ("requirement", subject("b")),
            ("example", subject("c")),
            ("requirement", subject("d")),
            ("example", subject("e")),
        ];
        let mut warmup = CacheWarmup::default();
        let label = |next: Option<(&str, AuthoringSubject, bool)>| {
            next.map(|(kind, subject, warms)| (kind.to_owned(), subject.label(), warms))
        };
        let started =
            |kind: &str, name: &str, warms| Some((kind.to_owned(), name.to_owned(), warms));
        // One of each kind starts; the rest wait for their kind.
        assert_eq!(
            label(warmup.next(&mut queue)),
            started("requirement", "a", true)
        );
        assert_eq!(
            label(warmup.next(&mut queue)),
            started("example", "c", true)
        );
        assert_eq!(label(warmup.next(&mut queue)), None);
        // An assessment that finished without a request warmed nothing, so
        // the next of its kind warms it instead.
        warmup.finished("example");
        assert_eq!(
            label(warmup.next(&mut queue)),
            started("example", "e", true)
        );
        // Once a kind's first request finishes, the others of it start.
        warmup.warmed("requirement");
        assert_eq!(
            label(warmup.next(&mut queue)),
            started("requirement", "b", false)
        );
        assert_eq!(
            label(warmup.next(&mut queue)),
            started("requirement", "d", false)
        );
        assert!(queue.is_empty());
    }

    // {test}`requirements.authoring.progress`
    // {test}`requirements.authoring.batch`
    #[test]
    fn selection_counts_definitions_and_citation_reviews_in_one_phase() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("md")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("book.toml"), "[book]\nsrc = \"md\"\n").unwrap();
        fs::write(
            root.join("md/requirements.md"),
            ":::{req} ready\nReady.\n:::\n",
        )
        .unwrap();
        let cite = ["{", "cite}`requirements.ready`"].concat();
        fs::write(
            root.join("src/main.rs"),
            format!("// {cite}\nfn one() {{}}\n// {cite}\nfn two() {{}}\n"),
        )
        .unwrap();
        let inspection = crate::evidence::inspect_repository(root).unwrap();
        let mut updates = Vec::new();
        let (subjects, unchanged) =
            select_subjects(root, &inspection, None, false, &mut |update| {
                updates.push(update)
            });
        assert_eq!(unchanged, 0);
        assert_eq!(
            subjects
                .iter()
                .map(AuthoringSubject::label)
                .collect::<Vec<_>>(),
            [
                "requirements.ready",
                "citations:src/main.rs#fn[one]",
                "citations:src/main.rs#fn[two]"
            ]
        );
        assert!(
            updates
                .iter()
                .all(|update| update.stage == "selecting assessments")
        );
        assert_eq!(
            updates
                .iter()
                .map(|update| (update.completed, update.total))
                .collect::<Vec<_>>(),
            [
                (0, None),
                (0, Some(3)),
                (1, Some(3)),
                (2, Some(3)),
                (3, Some(3))
            ]
        );
    }

    // {test}`requirements.authoring.batch`
    // {test}`requirements.references.site_review`
    #[test]
    fn selection_includes_site_reviews_and_sites_of_selects_only_them() {
        let temp =
            crate::sites::tests::project(crate::sites::tests::RULE, crate::sites::tests::SOURCE);
        let root = temp.path();
        crate::sites::tests::write_review(root, "fn[check_two]", serde_json::json!({}));
        let inspection = crate::evidence::inspect_repository(root).unwrap();
        let labels = |subjects: Vec<AuthoringSubject>| {
            subjects
                .iter()
                .map(AuthoringSubject::label)
                .collect::<Vec<_>>()
        };
        let (subjects, unchanged) = select_subjects(root, &inspection, None, false, &mut |_| {});
        assert_eq!(
            labels(subjects),
            [
                "requirements.rule",
                "sites:requirements.rule/src/lib.rs#fn[check_one]",
                "sites:requirements.rule/src/lib.rs#mod[inner]/fn[check_three]",
            ]
        );
        assert_eq!(unchanged, 1);
        let (subjects, unchanged) = select_subjects(
            root,
            &inspection,
            Some("requirements.rule"),
            false,
            &mut |_| {},
        );
        assert_eq!(
            labels(subjects),
            [
                "sites:requirements.rule/src/lib.rs#fn[check_one]",
                "sites:requirements.rule/src/lib.rs#mod[inner]/fn[check_three]",
            ]
        );
        assert_eq!(unchanged, 1);
    }

    // {test}`requirements.authoring.batch`
    // {test}`requirements.references.site_review`
    // {test}`requirements.references.citation_review`
    #[test]
    fn orphaned_citation_and_site_reviews_are_deleted() {
        let temp =
            crate::sites::tests::project(crate::sites::tests::RULE, crate::sites::tests::SOURCE);
        let root = temp.path();
        crate::sites::tests::write_review(root, "fn[check_two]", serde_json::json!({}));
        crate::sites::tests::write_review(root, "fn[other]", serde_json::json!({}));
        fs::create_dir_all(root.join("evidence/citations/src/lib.rs")).unwrap();
        fs::write(
            root.join("evidence/citations/src/lib.rs/fn[gone].json"),
            "{}",
        )
        .unwrap();
        let inspection = crate::evidence::inspect_repository(root).unwrap();
        assert_eq!(
            delete_orphaned_reviews(root, &inspection).unwrap(),
            [
                "evidence/citations/src/lib.rs/fn[gone].json",
                "evidence/sites/requirements.rule/src/lib.rs/fn[other].json",
            ]
        );
        assert!(
            root.join("evidence/sites/requirements.rule/src/lib.rs/fn[check_two].json")
                .exists()
        );
        assert!(
            !root
                .join("evidence/sites/requirements.rule/src/lib.rs/fn[other].json")
                .exists()
        );
    }
}

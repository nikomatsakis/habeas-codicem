use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tokio::task::JoinSet;

use super::{AuthoringOutcome, AuthoringSubject, PendingAssessment, prepare_assessment};
use crate::DefinitionKind;
use crate::config;
use crate::evidence::inspect_repository;
use crate::index::{EvidenceState, LoadedDetailEvidence};

pub struct BatchOutcome {
    pub completed: Vec<AuthoringOutcome>,
    pub failures: Vec<(String, String)>,
    pub unchanged: usize,
}

// {impl}`requirements.authoring.batch`
// {impl}`requirements.authoring.concurrency`
// {impl}`requirements.authoring.configuration_precedence`
pub async fn author_all(root: &Path, selection: Option<&str>) -> io::Result<BatchOutcome> {
    let root = fs::canonicalize(root)?;
    let inspection = inspect_repository(&root)?;
    if !inspection.discovery_errors.is_empty() {
        return Err(io::Error::other(inspection.discovery_errors.join("\n")));
    }
    let mut subjects = Vec::new();
    let mut unchanged = 0;
    for (name, definition) in &inspection.definitions {
        if definition.kind == DefinitionKind::Term {
            continue;
        }
        let evidence = LoadedDetailEvidence::load(&root, name, &inspection.report);
        if matches!(
            evidence.state(),
            EvidenceState::Valid | EvidenceState::Fixme
        ) {
            unchanged += 1;
        } else {
            subjects.push(AuthoringSubject::Definition(name.clone()));
        }
    }
    for (rust_file, rust_item, valid) in inspection.rust_functions {
        if valid {
            unchanged += 1;
        } else {
            subjects.push(AuthoringSubject::RustFunction {
                rust_file,
                rust_item,
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
    eprintln!(
        "Authoring {} assessments with concurrency {concurrency}; {unchanged} current records unchanged",
        subjects.len()
    );
    let mut subjects = subjects.into_iter();
    let mut tasks = JoinSet::new();
    let mut cancelled = false;
    loop {
        while !cancelled && tasks.len() < concurrency {
            let Some(subject) = subjects.next() else {
                break;
            };
            let root = root.clone();
            let config = config.clone();
            tasks.spawn(async move {
                let result = prepare_assessment(&root, &subject, &config).await;
                (subject.label(), result)
            });
        }
        let Some(result) = tasks.join_next().await else {
            break;
        };
        let (label, result) = match result {
            Ok(result) => result,
            Err(error) => {
                eprintln!("authoring task: {error}");
                outcome
                    .failures
                    .push(("authoring task".into(), error.to_string()));
                continue;
            }
        };
        let result = result.and_then(|pending| {
            // This collector is the sole writer: functions in the same source
            // file can finish concurrently without replacing each other's entries.
            match write_record(&root, &pending) {
                Ok(path) => pending.persisted(path),
                Err(error) => Err(pending.failed(error)),
            }
        });
        match result {
            Ok(assessment) => {
                eprintln!(
                    "{}: {} (usage: {})",
                    assessment.subject,
                    assessment.determination.kind(),
                    assessment.usage_path.display()
                );
                outcome.completed.push(assessment);
            }
            Err(error) => {
                cancelled |= error.kind() == io::ErrorKind::Interrupted;
                eprintln!("{label}: {error}");
                outcome.failures.push((label, error.to_string()));
            }
        }
    }
    if cancelled {
        let remaining = subjects.count();
        if remaining > 0 {
            outcome.failures.push((
                "batch".into(),
                format!("cancelled; {remaining} assessments not started"),
            ));
        }
    }
    Ok(outcome)
}

// {impl}`requirements.authoring.batch`
// {impl}`requirements.authoring.function_attestations`
// {impl}`requirements.authoring.persistence`
fn write_record(root: &Path, pending: &PendingAssessment) -> io::Result<PathBuf> {
    let (path, record) = match &pending.subject {
        AuthoringSubject::Definition(name) => (
            root.join("evidence").join(format!("{name}.json")),
            pending.record.clone(),
        ),
        AuthoringSubject::RustFunction {
            rust_file,
            rust_item,
        } => {
            let path = root.join("evidence/rust").join(format!("{rust_file}.json"));
            let mut entries = match fs::read_to_string(&path) {
                Ok(text) => {
                    let mut record: Value = serde_json::from_str(&text)?;
                    record
                        .get_mut("functions")
                        .and_then(Value::as_array_mut)
                        .ok_or_else(|| {
                            io::Error::other(format!(
                                "{}: functions must be an array",
                                path.display()
                            ))
                        })?
                        .clone()
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
                Err(error) => return Err(error),
            };
            entries
                .retain(|entry| entry.get("rust_item").and_then(Value::as_str) != Some(rust_item));
            entries.push(pending.record.clone());
            entries.sort_by(|a, b| a["rust_item"].as_str().cmp(&b["rust_item"].as_str()));
            (path, json!({"rust_file":rust_file,"functions":entries}))
        }
    };
    let parent = path.parent().expect("evidence parent");
    fs::create_dir_all(parent)?;
    // Other assessments may read the existing selectors while this one finishes.
    // Replace the complete file so those reads see either complete version.
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    writeln!(file, "{}", serde_json::to_string_pretty(&record)?)?;
    file.persist(&path).map_err(|error| error.error)?;
    Ok(path)
}

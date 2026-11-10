//! Coverage-loss reviews: when authoring replaces a record and an item its
//! previous inputs covered is no longer covered, an independent conversation
//! decides, for each definition whose previous record read the item, whether
//! the item is a root of its implementation or is not needed for it. The
//! result is not evidence; it is reported and kept under the state root,
//! keyed by item, for `habeas status`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use super::AuthoringSubject;
use super::inputs::AssessmentSources;
use super::providers::CompletionProvider;
use super::usage::UsageLog;
use crate::rust_coverage::EvidenceReads;

/// The coverage-loss review prompt: its system prompt and its subject
/// message, filled like the approved prompt templates.
// {impl}`requirements.authoring.coverage_loss`
// {impl}`requirements.authoring.prompt_parts`
pub(crate) const COVERAGE_LOSS_PROMPT: [&str; 2] = [
    r#"# Task

Review one source item that earlier assessments read but that nothing covers
any longer. Your finding for each definition should be the one any careful
reviewer would reach from the same text and sources.

# Background: Habeas terms

{{habeas_background}}

{{reference_roles}}

# Your task

The mandatory inputs at the end contain the item and the declarations of the
requirements and examples whose earlier assessments read it. Those assessments
have been replaced by ones that did not read the item, and it carries no
reference of its own, so validation reports it uncovered. For each listed
definition, decide whether the item is a root of the definition's
implementation or is not needed for it.

1. Read the earlier rationale given for the definition to see what the
   assessment relied on the item for.
2. Find the items marked {impl} for the definition with the references tool,
   and follow them: what they call, the types they use, and the implementations
   of trait methods they call, and what those reach in turn.
3. The item is a root if it carries out part of the definition, as the
   reference roles describe, and no item marked {impl} for the definition
   reaches it. Otherwise it is not needed for the definition: a marked item
   reaches it, or it carries out no part of the definition.
4. In each rationale, name the marked items you followed and say how the item
   is or is not reached; for an item that is not needed, say which of the two
   reasons applies.

# How to judge

{{judgment_rules}}

# Submitting

Call submit_coverage_review exactly once with findings: one entry for each
listed definition, with definition (its full name), finding ("root" or
"not needed"), and a nonempty rationale. That tool call is your final answer."#,
    r#"# Subject

Item under review: {{item_file}} {{item_selector}}
Definitions to decide: {{definitions}}

# Earlier rationales

{{rationales}}

# Mandatory inputs

{{mandatory_inputs}}"#,
];

/// A definition whose replaced record read a lost item, and that record's
/// rationale.
// {impl}`requirements.authoring.coverage_loss`
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Reader {
    pub(crate) definition: String,
    pub(crate) rationale: String,
}

/// An item that a replaced record's inputs covered and that is no longer
/// covered, with the definitions whose replaced records read it.
// {impl}`requirements.authoring.coverage_loss`
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LostItem {
    pub(crate) file: String,
    pub(crate) item: String,
    pub(crate) readers: Vec<Reader>,
}

impl LostItem {
    /// The review's subject label, `coverage-loss:<file>#<selector>`.
    // {impl}`requirements.authoring.coverage_loss`
    // {impl}`requirements.authoring.usage_storage`
    pub(crate) fn label(&self) -> String {
        format!("coverage-loss:{}#{}", self.file, self.item)
    }

    /// The selector as displayed, `<root>` for the root node.
    // {impl}`requirements.authoring.coverage_loss`
    fn selector_label(&self) -> &str {
        if self.item.is_empty() {
            "<root>"
        } else {
            &self.item
        }
    }
}

/// A record about to be replaced: the definition it assessed (a site
/// review's requirement; none for a citation review), its rationale, and the
/// outline inputs it recorded.
// {impl}`requirements.authoring.coverage_loss`
pub(crate) struct ReplacedRecord {
    definition: Option<String>,
    rationale: String,
    reads: EvidenceReads,
}

impl ReplacedRecord {
    /// Reads the record a subject's authoring is about to replace, if one
    /// exists and parses.
    // {impl}`requirements.authoring.coverage_loss`
    pub(crate) fn read(root: &Path, subject: &AuthoringSubject) -> Option<Self> {
        let text = fs::read_to_string(subject.evidence_path(root)).ok()?;
        let record = serde_json::from_str::<Value>(&text).ok()?;
        let definition = match subject {
            AuthoringSubject::Definition(name) => Some(name.clone()),
            AuthoringSubject::Site { requirement, .. } => Some(requirement.clone()),
            AuthoringSubject::Citation { .. } => None,
        };
        let mut reads = EvidenceReads::default();
        reads.add_record(&record);
        Some(Self {
            definition,
            rationale: record["rationale"].as_str().unwrap_or_default().to_owned(),
            reads,
        })
    }
}

/// The items the replaced records covered that are no longer covered, now
/// that the records replacing them are written, in path and source order.
/// An item only a citation review read names no definition to decide, and is
/// left to validation.
// {impl}`requirements.authoring.coverage_loss`
// {impl}`requirements.references.rust_item_coverage`
pub(crate) fn lost_items(root: &Path, replaced: &[ReplacedRecord]) -> io::Result<Vec<LostItem>> {
    if replaced.is_empty() {
        return Ok(Vec::new());
    }
    let root = fs::canonicalize(root)?;
    let definitions = crate::evidence::load_definitions(&root)?;
    let mut errors = Vec::new();
    let evidence = root.join("evidence");
    let evidence_files = if evidence.is_dir() {
        crate::evidence::files_with_extension(&root, &evidence, "json", &mut errors)
    } else {
        Vec::new()
    };
    let current = EvidenceReads::load(&evidence_files);
    let files = crate::evidence::discovered_files(&root, &mut errors);
    if !errors.is_empty() {
        return Err(io::Error::other(errors.join("\n")));
    }
    let mut lost = Vec::new();
    for path in crate::rust_coverage::rust_files(&root, &files) {
        let file = crate::evidence::display_path(&root, &path);
        if !replaced.iter().any(|record| record.reads.has_file(&file)) {
            continue;
        }
        // Validation reports unreadable or unparsable sources.
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok((outline, missing)) =
            crate::rust_coverage::unreferenced_items(&source, &definitions)
        else {
            continue;
        };
        for (index, ..) in missing {
            if current.covers(&file, &outline, index) {
                continue;
            }
            let mut readers: Vec<Reader> = Vec::new();
            for record in replaced {
                if let Some(definition) = &record.definition
                    && record.reads.covers(&file, &outline, index)
                    && !readers
                        .iter()
                        .any(|reader| &reader.definition == definition)
                {
                    readers.push(Reader {
                        definition: definition.clone(),
                        rationale: record.rationale.clone(),
                    });
                }
            }
            if !readers.is_empty() {
                lost.push(LostItem {
                    file: file.clone(),
                    item: outline.nodes[index].selector.clone(),
                    readers,
                });
            }
        }
    }
    Ok(lost)
}

/// What a coverage-loss review found an item to be for one definition.
// {impl}`requirements.authoring.coverage_loss`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoverageFinding {
    /// A root of the definition's implementation, which needs an `{impl}`.
    Root,
    /// Not needed for the definition.
    NotNeeded,
}

impl CoverageFinding {
    /// The finding as submitted and reported: `root` or `not needed`.
    // {impl}`requirements.authoring.coverage_loss`
    pub fn label(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::NotNeeded => "not needed",
        }
    }
}

/// One definition's finding and its rationale.
// {impl}`requirements.authoring.coverage_loss`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefinitionFinding {
    pub definition: String,
    pub finding: CoverageFinding,
    pub rationale: String,
}

/// A completed coverage-loss review of one item.
// {impl}`requirements.authoring.coverage_loss`
#[derive(Clone, Debug)]
pub struct CoverageLossReview {
    pub file: String,
    pub item: String,
    pub findings: Vec<DefinitionFinding>,
    /// Where the result is kept for `habeas status`.
    pub result_path: PathBuf,
    pub usage_path: PathBuf,
}

impl CoverageLossReview {
    /// The report of the review: a line naming the item, then each
    /// definition's finding and rationale.
    // {impl}`requirements.authoring.coverage_loss`
    pub fn report(&self) -> String {
        let mut report = format!(
            "Coverage-loss review of {} {}: no longer covered",
            self.file, self.item
        );
        for finding in &self.findings {
            report.push_str(&format!(
                "\n  {}: {}: {}",
                finding.definition,
                finding.finding.label(),
                one_line(&finding.rationale)
            ));
        }
        report
    }
}

/// Text on one line, with its whitespace collapsed.
// {impl}`requirements.authoring.coverage_loss`
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The prompt of a coverage-loss review.
// {impl}`requirements.authoring.coverage_loss`
// {impl}`requirements.authoring.prompt_parts`
// {impl}`requirements.authoring.habeas_background`
// {impl}`requirements.authoring.judgment_rules`
pub(super) fn prompt(lost: &LostItem, mandatory: &str) -> io::Result<super::Prompt> {
    let names = lost
        .readers
        .iter()
        .map(|reader| reader.definition.as_str())
        .collect::<Vec<_>>();
    let definitions = serde_json::to_string(&names)?;
    let rationales = lost
        .readers
        .iter()
        .map(|reader| {
            format!(
                "## {}\n\n{}",
                reader.definition,
                crate::redaction::redact_text(reader.rationale.trim())
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut values = super::shared_blocks().to_vec();
    values.extend([
        ("item_file", lost.file.as_str()),
        ("item_selector", lost.selector_label()),
        ("definitions", definitions.as_str()),
        ("rationales", rationales.as_str()),
        ("mandatory_inputs", mandatory),
    ]);
    Ok(super::Prompt {
        system: super::fill(COVERAGE_LOSS_PROMPT[0], &values),
        subject: super::fill(COVERAGE_LOSS_PROMPT[1], &values),
    })
}

/// The `submit_coverage_review` tool.
// {impl}`requirements.authoring.coverage_loss`
// {impl}`requirements.authoring.submission`
pub(super) fn tool_definition() -> Value {
    json!({"name": "submit_coverage_review", "description": "Submit your coverage-loss review exactly once: one finding for each listed definition. No other message supplies the result.",
        "parameters": {"type":"object", "additionalProperties": false, "required":["findings"], "properties": {
            "findings": {"type":"array", "minItems": 1, "items": {"type":"object", "additionalProperties": false,
                "required":["definition","finding","rationale"], "properties": {
                    "definition":{"type":"string","minLength":1},
                    "finding":{"type":"string","enum":["root","not needed"]},
                    "rationale":{"type":"string","minLength":1}}}}}}})
}

/// Parses a `submit_coverage_review` call: exactly one finding, `root` or
/// `not needed`, with a nonempty rationale for each of `definitions`.
// {impl}`requirements.authoring.coverage_loss`
// {impl}`requirements.authoring.submission`
pub(super) fn submission(
    arguments: &Value,
    definitions: &[String],
) -> io::Result<Vec<DefinitionFinding>> {
    let invalid = |message: &str| io::Error::other(format!("submit_coverage_review {message}"));
    let object = arguments
        .as_object()
        .ok_or_else(|| invalid("expects an object"))?;
    if object.len() != 1 {
        return Err(invalid("requires exactly findings"));
    }
    let entries = object
        .get("findings")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("findings must be an array"))?;
    let mut findings = Vec::new();
    for entry in entries {
        let entry = entry
            .as_object()
            .filter(|entry| entry.len() == 3)
            .ok_or_else(|| {
                invalid("each finding requires exactly definition, finding, and rationale")
            })?;
        let text = |field: &str| {
            entry
                .get(field)
                .and_then(Value::as_str)
                .filter(|text| !text.trim().is_empty())
                .ok_or_else(|| invalid(&format!("{field} must be a nonempty string")))
        };
        let definition = text("definition")?;
        if !definitions.iter().any(|name| name == definition) {
            return Err(invalid(&format!(
                "names {definition}, which is not a listed definition"
            )));
        }
        if findings
            .iter()
            .any(|found: &DefinitionFinding| found.definition == definition)
        {
            return Err(invalid(&format!(
                "has more than one finding for {definition}"
            )));
        }
        let finding = match text("finding")? {
            "root" => CoverageFinding::Root,
            "not needed" => CoverageFinding::NotNeeded,
            other => {
                return Err(invalid(&format!(
                    "finding {other:?} must be \"root\" or \"not needed\""
                )));
            }
        };
        findings.push(DefinitionFinding {
            definition: definition.to_owned(),
            finding,
            rationale: text("rationale")?.to_owned(),
        });
    }
    if let Some(missing) = definitions
        .iter()
        .find(|name| !findings.iter().any(|found| &found.definition == *name))
    {
        return Err(invalid(&format!("has no finding for {missing}")));
    }
    // Report findings in the order the definitions were listed.
    findings.sort_by_key(|found| {
        definitions
            .iter()
            .position(|name| *name == found.definition)
    });
    Ok(findings)
}

/// Runs one item's review in its own conversation with its own usage record,
/// and keeps its result under the state root.
// {impl}`requirements.authoring.coverage_loss`
// {impl}`requirements.authoring.usage_storage`
pub(super) async fn review(
    provider: &impl CompletionProvider,
    root: &Path,
    state: &Path,
    lost: LostItem,
    (backend, model): (&str, &str),
) -> io::Result<CoverageLossReview> {
    let mut usage = UsageLog::start(state, root, &lost.label())?;
    usage.record.subject_kind = Some("coverage-loss review".into());
    usage.record.backend = Some(backend.into());
    usage.record.model = Some(model.into());
    usage.save()?;
    let (file, item) = (lost.file.clone(), lost.item.clone());
    let definitions = lost
        .readers
        .iter()
        .map(|reader| reader.definition.clone())
        .collect::<Vec<_>>();
    let result = async {
        let mut sources = AssessmentSources::prepare_coverage_loss(root, lost)?;
        usage.prepared("mandatory", sources.mandatory_inputs());
        usage.save()?;
        let prompt = super::initial_prompt(&sources)?;
        super::converse(
            provider,
            &mut sources,
            &mut usage,
            prompt,
            ("submit_coverage_review", false),
            |arguments| submission(arguments, &definitions),
        )
        .await
    }
    .await;
    let outcome = match &result {
        Ok(_) => "completed",
        Err(error) if error.kind() == io::ErrorKind::Interrupted => "cancelled",
        Err(_) => "failed",
    };
    usage.finish(outcome)?;
    let findings = result.map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("{error}; usage: {}", usage.path.display()),
        )
    })?;
    let hash = crate::inputs::outline_input(root, &file, &item)
        .ok()
        .and_then(|input| input["hash"].as_str().map(str::to_owned));
    let result_path = store(state, root, &file, &item, hash.as_deref(), &findings)?;
    Ok(CoverageLossReview {
        file,
        item,
        findings,
        result_path,
        usage_path: usage.path.clone(),
    })
}

/// Where an item's latest coverage-loss review result is kept: under the
/// state root, by project and then by the item's file and selector.
// {impl}`requirements.authoring.coverage_loss`
// {impl}`requirements.configuration.state_location`
pub(crate) fn result_path(state: &Path, root: &Path, file: &str, item: &str) -> PathBuf {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(root.to_string_lossy().as_bytes());
    let project = digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    state
        .join("authoring/coverage-loss")
        .join(project)
        .join(file)
        .join(format!("{}.json", crate::citations::encoded_selector(item)))
}

/// Writes an item's latest review result, replacing an earlier one.
// {impl}`requirements.authoring.coverage_loss`
// {impl}`requirements.configuration.state_location`
fn store(
    state: &Path,
    root: &Path,
    file: &str,
    item: &str,
    hash: Option<&str>,
    findings: &[DefinitionFinding],
) -> io::Result<PathBuf> {
    let path = result_path(state, root, file, item);
    let record = json!({
        "version": 1,
        "project_root": root,
        "file": file,
        "item": item,
        "hash": hash,
        "reviewed_ms": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
        "findings": findings.iter().map(|found| json!({
            "definition": found.definition,
            "finding": found.finding.label(),
            "rationale": found.rationale,
        })).collect::<Vec<_>>(),
    });
    fs::create_dir_all(path.parent().expect("result parent"))
        .and_then(|()| {
            fs::write(
                &path,
                format!("{}\n", serde_json::to_string_pretty(&record)?),
            )
        })
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "cannot keep the coverage-loss review at {}: {error}",
                    path.display()
                ),
            )
        })?;
    Ok(path)
}

/// The latest kept review of an item as `habeas status` shows it beside the
/// uncovered item, or `None` when there is none or the state root cannot be
/// determined.
// {impl}`requirements.authoring.coverage_loss`
// {impl}`requirements.cli.status`
pub(crate) fn status_lines(root: &Path, file: &str, item: &str) -> Option<String> {
    let state = crate::config::state_root().ok()?;
    let text = fs::read_to_string(result_path(&state, root, file, item)).ok()?;
    let record = serde_json::from_str::<Value>(&text).ok()?;
    let mut lines = String::from("  latest coverage-loss review:");
    for finding in record["findings"].as_array()? {
        lines.push_str(&format!(
            "\n    {}: {}: {}",
            finding["definition"].as_str()?,
            finding["finding"].as_str()?,
            one_line(finding["rationale"].as_str()?)
        ));
    }
    Some(lines)
}

/// Reviews each item the replaced records no longer cover, connecting to
/// the configured provider only when there is one. Each review's failure is
/// returned with its label rather than stopping the others.
// {impl}`requirements.authoring.coverage_loss`
// {impl}`requirements.authoring.provider`
pub(super) async fn review_losses(
    root: &Path,
    config: &crate::config::ResolvedAuthoringConfig,
    replaced: &[ReplacedRecord],
    mut reported: impl FnMut(&Result<CoverageLossReview, (String, String)>),
) -> io::Result<CoverageLossOutcome> {
    let mut outcome = CoverageLossOutcome::default();
    let lost = lost_items(root, replaced)?;
    if lost.is_empty() {
        return Ok(outcome);
    }
    let provider = super::providers::RigProvider::connect(&config.backend)
        .await
        .map_err(|error| {
            io::Error::other(format!(
                "{}: authoring.configs.{}: {error}",
                config.config_path.display(),
                config.name
            ))
        })?;
    let state = super::usage_state_root(root)?;
    let model = config.model_ref();
    let backend = super::backend_name(&model);
    for item in lost {
        let label = item.label();
        let result = review(&provider, root, &state, item, (backend, &model.model))
            .await
            .map_err(|error| (label, error.to_string()));
        reported(&result);
        match result {
            Ok(review) => outcome.reviews.push(review),
            Err(failure) => outcome.failures.push(failure),
        }
    }
    Ok(outcome)
}

/// The coverage-loss reviews an authoring run completed and those that
/// failed, with their labels and errors.
// {impl}`requirements.authoring.coverage_loss`
#[derive(Clone, Debug, Default)]
pub struct CoverageLossOutcome {
    pub reviews: Vec<CoverageLossReview>,
    pub failures: Vec<(String, String)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoring::inputs::tests::{project, role};

    // {test}`requirements.authoring.coverage_loss`
    #[test]
    fn submissions_need_one_root_or_not_needed_finding_per_definition() {
        let listed = ["requirements.a".to_owned(), "requirements.b".to_owned()];
        let finding = |definition: &str, finding: &str| json!({"definition": definition, "finding": finding, "rationale": "Because."});
        let found = submission(
            &json!({"findings": [finding("requirements.b", "not needed"), finding("requirements.a", "root")]}),
            &listed,
        )
        .unwrap();
        assert_eq!(
            found
                .iter()
                .map(|found| (found.definition.as_str(), found.finding))
                .collect::<Vec<_>>(),
            [
                ("requirements.a", CoverageFinding::Root),
                ("requirements.b", CoverageFinding::NotNeeded)
            ]
        );
        for (arguments, message) in [
            (json!([]), "expects an object"),
            (
                json!({"findings": [], "extra": 1}),
                "requires exactly findings",
            ),
            (
                json!({"findings": [finding("requirements.a", "root")]}),
                "has no finding for requirements.b",
            ),
            (
                json!({"findings": [finding("requirements.a", "root"), finding("requirements.a", "root"), finding("requirements.b", "root")]}),
                "more than one finding for requirements.a",
            ),
            (
                json!({"findings": [finding("requirements.c", "root")]}),
                "not a listed definition",
            ),
            (
                json!({"findings": [finding("requirements.a", "maybe")]}),
                "must be \"root\" or \"not needed\"",
            ),
            (
                json!({"findings": [{"definition": "requirements.a", "finding": "root", "rationale": " "}]}),
                "rationale must be a nonempty string",
            ),
        ] {
            let error = submission(&arguments, &listed).err().unwrap();
            assert!(error.to_string().contains(message), "{arguments}: {error}");
        }
    }

    // {test}`requirements.authoring.coverage_loss`
    #[test]
    fn lost_items_are_those_replaced_records_covered_that_nothing_covers_now() {
        let root = project();
        fs::write(
            root.path().join("md/requirements.md"),
            ":::{req} ready\nReady.\n:::\n\n:::{req} other\nOther.\n:::\n",
        )
        .unwrap();
        fs::write(
            root.path().join("src/lib.rs"),
            format!(
                "fn lost() {{}}\n\nfn kept() {{}}\n\n// {}\nfn referenced() {{}}\n\nfn cited_only() {{}}\n",
                role("impl", "requirements.ready")
            ),
        )
        .unwrap();
        fs::create_dir_all(root.path().join("evidence")).unwrap();
        let input =
            |item: &str| crate::inputs::outline_input(root.path(), "src/lib.rs", item).unwrap();
        // A current record still reads `kept`.
        fs::write(
            root.path().join("evidence/requirements.other.json"),
            json!({"inputs": [input("fn[kept]")]}).to_string(),
        )
        .unwrap();
        let replaced = |definition: Option<&str>, items: &[&str]| {
            let mut reads = EvidenceReads::default();
            reads.add_record(
                &json!({"inputs": items.iter().map(|item| input(item)).collect::<Vec<_>>()}),
            );
            ReplacedRecord {
                definition: definition.map(str::to_owned),
                rationale: format!("{definition:?} read {items:?}"),
                reads,
            }
        };
        let lost = lost_items(
            root.path(),
            &[
                replaced(
                    Some("requirements.ready"),
                    &["fn[lost]", "fn[kept]", "fn[referenced]"],
                ),
                replaced(Some("requirements.ready"), &["fn[lost]"]),
                replaced(Some("requirements.other"), &["fn[lost]"]),
                replaced(None, &["fn[cited_only]"]),
            ],
        )
        .unwrap();
        // Each definition is listed once, with its first record's rationale;
        // an item only a citation review read is left to validation.
        assert_eq!(
            lost,
            [LostItem {
                file: "src/lib.rs".into(),
                item: "fn[lost]".into(),
                readers: vec![
                    Reader {
                        definition: "requirements.ready".into(),
                        rationale: "Some(\"requirements.ready\") read [\"fn[lost]\", \"fn[kept]\", \"fn[referenced]\"]".into(),
                    },
                    Reader {
                        definition: "requirements.other".into(),
                        rationale: "Some(\"requirements.other\") read [\"fn[lost]\"]".into(),
                    },
                ],
            }]
        );
        assert!(lost_items(root.path(), &[]).unwrap().is_empty());
    }

    // {test}`requirements.authoring.coverage_loss`
    #[test]
    fn results_are_kept_by_project_and_item_under_the_state_root() {
        let state = Path::new("/state");
        let path = result_path(
            state,
            Path::new("/work/project"),
            "src/lib.rs",
            "impl[A]/fn[b]",
        );
        assert!(
            path.starts_with("/state/authoring/coverage-loss"),
            "{path:?}"
        );
        assert!(
            path.ends_with("src/lib.rs/impl[A]%2Ffn[b].json"),
            "{path:?}"
        );
        assert_ne!(
            path,
            result_path(
                state,
                Path::new("/work/elsewhere"),
                "src/lib.rs",
                "impl[A]/fn[b]"
            )
        );
    }
}

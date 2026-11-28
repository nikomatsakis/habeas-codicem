use super::Judgment;
use crate::inputs::{OutlineCache, ParsedSource};
use crate::sdk::{HabeasEdgeLabel, HabeasNode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read, Write};
use std::{fmt, fs};
use trust_but_verify::domain::NodeContents;
use trust_but_verify::graph::{Dependency, Fingerprint};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Role {
    Test,
    Impl,
}
impl Role {
    fn marker(self) -> &'static str {
        match self {
            Self::Test => "test",
            Self::Impl => "impl",
        }
    }
}

// {impl}`requirements.authoring.corrections.rationale`
// {impl}`requirements.authoring.corrections`
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Correction {
    AddReference {
        role: Role,
        file: String,
        selector: String,
        rationale: String,
    },
    Manual {
        instruction: String,
        rationale: String,
    },
}

impl Correction {
    fn action(&self) -> String {
        match self {
            Self::Manual { instruction, .. } => instruction.clone(),
            Self::AddReference {
                role,
                file,
                selector,
                ..
            } => format!("Add `{{{}}}` to `{file}#{selector}`", role.marker()),
        }
    }
    fn rationale(&self) -> &str {
        match self {
            Self::Manual { rationale, .. } | Self::AddReference { rationale, .. } => rationale,
        }
    }
}
impl fmt::Display for Correction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} — {}", self.action(), self.rationale())
    }
}

// {impl}`requirements.authoring.submission`
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum AssessmentResult {
    Success {
        rationale: String,
    },
    Failed {
        #[schemars(length(min = 1))]
        corrections: Vec<Correction>,
    },
    AcknowledgedGap {
        #[schemars(length(min = 1))]
        entries: Vec<String>,
        rationale: String,
    },
}
impl AssessmentResult {
    // {impl}`requirements.authoring.corrections`
    // {impl}`requirements.authoring.corrections.rationale`
    pub(crate) fn evidence(&self) -> io::Result<(Judgment, String)> {
        let valid = |s: &str| !s.trim().is_empty();
        match self {
            Self::Success { rationale } if valid(rationale) => {
                Ok((Judgment::True, rationale.clone()))
            }
            Self::AcknowledgedGap { entries, rationale }
                if valid(rationale) && !entries.is_empty() && entries.iter().all(|s| valid(s)) =>
            {
                Ok((Judgment::Fixme(entries.clone()), rationale.clone()))
            }
            Self::Failed { corrections }
                if !corrections.is_empty()
                    && corrections.iter().all(|c| match c {
                        Correction::Manual {
                            instruction,
                            rationale,
                        } => valid(instruction) && valid(rationale),
                        Correction::AddReference {
                            file, rationale, ..
                        } => valid(file) && valid(rationale),
                    }) =>
            {
                Ok((
                    Judgment::False,
                    corrections
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("\n\n"),
                ))
            }
            _ => Err(io::Error::other(
                "assessment requires nonempty rationales, corrections, instructions or acknowledgment entries for the selected status",
            )),
        }
    }
}

/// Compute the requirement judgment from the two independently submitted results.
pub(crate) fn combine_criteria(criteria: &serde_json::Value) -> io::Result<AssessmentResult> {
    let mut explanations = Vec::new();
    let mut corrections = Vec::new();
    let mut entries = Vec::new();
    if criteria.as_object().map(|o| o.len()) != Some(2) {
        return Err(io::Error::other(
            "expected structural and behavioral criteria",
        ));
    }
    for name in ["structural", "behavioral"] {
        let record: trust_but_verify::graph::Record<HabeasNode, HabeasEdgeLabel> =
            serde_json::from_value(criteria[name].clone())?;
        let result: AssessmentResult = serde_json::from_value(record.output)?;
        let (_, rationale) = result.evidence()?;
        explanations.push(format!("{name}: {rationale}"));
        match result {
            AssessmentResult::Success { .. } => (),
            AssessmentResult::Failed { corrections: found } => corrections.extend(found),
            AssessmentResult::AcknowledgedGap { entries: found, .. } => {
                for entry in found {
                    if !entries.contains(&entry) {
                        entries.push(entry);
                    }
                }
            }
        }
    }
    let rationale = explanations.join("\n\n");
    let result = if !corrections.is_empty() {
        AssessmentResult::Failed { corrections }
    } else if !entries.is_empty() {
        AssessmentResult::AcknowledgedGap { entries, rationale }
    } else {
        AssessmentResult::Success { rationale }
    };
    Ok(result)
}

#[derive(Debug)]
pub(super) struct Finding {
    correction: Correction,
    fingerprint: Option<Fingerprint>,
}
impl Finding {
    pub(super) fn capture(
        correction: Correction,
        dependencies: &[Dependency<HabeasNode, HabeasEdgeLabel>],
    ) -> Self {
        let fingerprint = if let Correction::AddReference { file, selector, .. } = &correction {
            dependencies.iter().find_map(|d| match d {
                Dependency::Node(HabeasNode::Source(id), fp)
                    if id.resource.scheme == "repo"
                        && id.resource.location == *file
                        && id.selector == *selector =>
                {
                    Some(fp.clone())
                }
                _ => None,
            })
        } else {
            None
        };
        Self {
            correction,
            fingerprint,
        }
    }
    pub(super) fn description(&self) -> String {
        self.correction.action()
    }
    pub(super) fn into_edit(self, subject: &str) -> Result<ReferenceEdit, String> {
        let description = self.description();
        match self.correction {
            Correction::Manual { .. } => Err(description),
            Correction::AddReference {
                file,
                selector,
                role,
                ..
            } => Ok(ReferenceEdit {
                subject: subject.to_owned(),
                file,
                selector,
                role,
                description,
                fingerprint: self.fingerprint,
            }),
        }
    }
}
pub(super) struct ReferenceEdit {
    subject: String,
    file: String,
    selector: String,
    role: Role,
    description: String,
    fingerprint: Option<Fingerprint>,
}
impl ReferenceEdit {
    fn failed(&self, reason: impl fmt::Display) -> (String, String) {
        (
            self.subject.clone(),
            format!("{}: {reason}", self.description),
        )
    }
}
pub(super) struct Repairs {
    pub changed: BTreeSet<String>,
    pub outstanding: Vec<(String, String)>,
}

pub(super) async fn apply(
    inspection: &crate::evidence::RepositoryInspection,
    edits: &[ReferenceEdit],
) -> Repairs {
    let root = &inspection.root;
    let mut repairs = Repairs {
        changed: BTreeSet::new(),
        outstanding: Vec::new(),
    };
    let mut files = BTreeMap::<_, BTreeMap<_, _>>::new();
    for edit in edits {
        let key = (&edit.subject, edit.role.marker(), &edit.selector);
        files
            .entry(&edit.file)
            .or_default()
            .entry(key)
            .or_insert(edit);
    }
    let cache = OutlineCache::default();
    for (file, actions) in files {
        if !std::path::Path::new(file)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
        {
            repairs
                .outstanding
                .extend(actions.into_values().map(|action| {
                    action
                        .failed("expected a project-relative source path without parent components")
                }));
            continue;
        }
        let parsed = cache.parse(root, file);
        let mut insertions = BTreeMap::<usize, BTreeSet<String>>::new();
        let mut planned = Vec::new();
        for action in actions.into_values() {
            let result = parsed
                .as_ref()
                .as_ref()
                .map_err(Clone::clone)
                .and_then(|p| plan(inspection, &cache, p, action));
            match result {
                Ok(Some((offset, line))) => {
                    insertions.entry(offset).or_default().insert(line);
                    planned.push(action);
                }
                Ok(None) => eprintln!(
                    "{}: {} (already present)",
                    action.subject, action.description
                ),
                Err(reason) => repairs.outstanding.push(action.failed(reason)),
            }
        }
        if insertions.is_empty() {
            continue;
        }
        let p = parsed.as_ref().as_ref().unwrap();
        let mut source = p.source.clone();
        for (offset, lines) in insertions.into_iter().rev() {
            source.insert_str(offset, &lines.into_iter().collect::<String>());
        }
        let candidate = OutlineCache::with_overrides([(file.clone(), source.clone())].into());
        let checked = candidate.parse(root, file);
        let valid = checked.as_ref().as_ref().is_ok_and(|new| {
            planned.iter().all(|edit| {
                p.resolve(file, &edit.selector)
                    .is_ok_and(|index| attached(&candidate, new, edit, index))
            })
        });
        if !valid {
            repairs.outstanding.extend(
                planned
                    .into_iter()
                    .map(|edit| edit.failed("combined markers changed attachment")),
            );
            continue;
        }
        tokio::task::yield_now().await;
        let result = replace_source(p, &source).await;
        match result {
            Ok(()) => {
                for edit in planned {
                    eprintln!("{}: {} (applied)", edit.subject, edit.description);
                    repairs.changed.insert(edit.subject.clone());
                }
            }
            Err(e) => repairs
                .outstanding
                .extend(planned.into_iter().map(|edit| edit.failed(&e))),
        }
    }
    repairs
}

/// Stage the replacement before locking. Recheck the path and bytes on the
/// locked original immediately before publishing the staged file.
async fn replace_source(p: &ParsedSource, source: &str) -> io::Result<()> {
    let mut tmp = tempfile::NamedTempFile::new_in(p.path.parent().unwrap())?;
    tmp.write_all(source.as_bytes())?;
    tmp.as_file()
        .set_permissions(fs::metadata(&p.path)?.permissions())?;
    tokio::task::yield_now().await;
    let original = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&p.path)?;
    let mut locked = same_file::Handle::from_file(original)?;
    locked.as_file().try_lock().map_err(io::Error::other)?;
    let mut current = Vec::new();
    locked.as_file_mut().read_to_end(&mut current)?;
    if p.path.canonicalize()? != p.path
        || locked != same_file::Handle::from_path(&p.path)?
        || current != p.source.as_bytes()
    {
        return Err(io::Error::other("file changed while preparing corrections"));
    }
    tmp.persist(&p.path)?;
    Ok(())
}

fn attached(
    cache: &OutlineCache,
    source: &ParsedSource,
    edit: &ReferenceEdit,
    owner: usize,
) -> bool {
    crate::evidence::file_references(&cache.validation, &source.path, &source.source, false)
        .iter()
        .any(|r| {
            r.name == edit.subject
                && r.kind.label() == edit.role.marker()
                && (if source.outline.language == "plain-text" {
                    0
                } else {
                    source.reference_owner(r.line, r.column)
                }) == owner
        })
}
fn plan(
    inspection: &crate::evidence::RepositoryInspection,
    cache: &OutlineCache,
    p: &ParsedSource,
    edit: &ReferenceEdit,
) -> Result<Option<(usize, String)>, String> {
    let root = &inspection.root;
    if root.join(&edit.file) != p.path {
        return Err("expected a project-relative source path".into());
    }
    let index = p.resolve(&edit.file, &edit.selector)?;
    let definition = inspection
        .definitions
        .get(&edit.subject)
        .ok_or("definition no longer exists")?;
    if definition.anchors.iter().any(|a| {
        a.kind.label() == edit.role.marker()
            && a.file == edit.file
            && p.resolve(&edit.file, &a.item) == Ok(index)
    }) {
        return Ok(None);
    }
    if !inspection.source_files.contains(&p.path) {
        return Err("file is outside reference discovery".into());
    }
    if attached(cache, p, edit, index) {
        return Ok(None);
    }
    let expected = edit
        .fingerprint
        .as_ref()
        .ok_or("the assessor must inspect this exact item")?;
    let current = cache
        .source_node(root, &edit.file, &edit.selector)
        .map_err(|e| e.to_string())?;
    if current.fingerprint() != *expected {
        return Err("item changed since assessment".into());
    }
    if fs::metadata(&p.path)
        .map_err(|e| e.to_string())?
        .permissions()
        .readonly()
    {
        return Err("source file is read-only".into());
    }
    let (offset, line) = marker_insertion(p, index, edit)?;
    let mut candidate = p.source.clone();
    candidate.insert_str(offset, &line);
    let updated = OutlineCache::with_overrides([(edit.file.clone(), candidate)].into());
    let parsed = updated.parse(root, &edit.file);
    let parsed = parsed.as_ref().as_ref().map_err(Clone::clone)?;
    if (index != 0 && parsed.outline.nodes.len() != p.outline.nodes.len())
        || !attached(&updated, parsed, edit, index)
    {
        return Err("the inserted marker would not reference the selected item".into());
    }
    Ok(Some((offset, line)))
}

fn marker_insertion(
    p: &ParsedSource,
    index: usize,
    edit: &ReferenceEdit,
) -> Result<(usize, String), String> {
    let node = &p.outline.nodes[index];
    let marker = format!("{{{}}}`{}`", edit.role.marker(), edit.subject);
    if p.outline.language == "markdown"
        || (p.outline.language == "plain-text"
            && p.path.extension().is_some_and(|ext| ext == "txt"))
    {
        // Keep the role inside the selected prose item. Discovery below checks
        // the result, including Markdown's literal and directive contexts.
        let text = &p.source[node.byte_start..node.byte_end];
        let end = text.trim_end();
        let offset = if end.starts_with(":::")
            && end
                .lines()
                .last()
                .is_some_and(|line| line.trim().chars().all(|c| c == ':'))
        {
            end.rfind('\n')
                .map(|i| text[..i].trim_end().len())
                .unwrap_or(end.len())
        } else {
            end.len()
        };
        return Ok((node.byte_start + offset, format!(" {marker}")));
    }
    let (open, close) = match p.outline.language.as_str() {
        "rust" | "typescript" | "tsx" => ("//", ""),
        "python" | "toml" | "yaml" => ("#", ""),
        "css" => ("/*", " */"),
        _ => return Err("no supported reference comment syntax for this file".into()),
    };
    let newline = if p
        .source
        .split_once('\n')
        .is_some_and(|(s, _)| s.ends_with('\r'))
    {
        "\r\n"
    } else {
        "\n"
    };
    let (offset, prefix) = if index == 0 {
        (
            p.source.len(),
            if p.source.is_empty() || p.source.ends_with('\n') {
                ""
            } else {
                newline
            },
        )
    } else {
        let start = node.byte_start;
        let offset = p.source[..start].rfind('\n').map_or(0, |i| i + 1);
        let prefix = &p.source[offset..start];
        if !prefix.bytes().all(|b| matches!(b, b' ' | b'\t')) {
            return Err("no standalone item declaration for a reference comment".into());
        }
        (offset, prefix)
    };
    Ok((offset, format!("{prefix}{open} {marker}{close}{newline}")))
}

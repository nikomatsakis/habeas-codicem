use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::{Value, json};

use crate::definition_glob;
use crate::evidence::Declaration;
use crate::rust_items::{item_lines, parse_items};

// {impl}`requirements.ci.schema`
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum InputSelector {
    FileRange {
        file: String,
        line_start: usize,
        line_end: usize,
    },
    RustItem {
        rust_file: String,
        rust_item: String,
    },
    Requirements {
        requirements: String,
    },
}

// {impl}`requirements.references.evidence_coverage`
// {impl}`requirements.index.detail_format`
#[derive(Clone, Debug)]
pub(crate) struct SourceRange {
    pub(crate) file: String,
    pub(crate) line_start: usize,
    pub(crate) line_end: usize,
    pub(crate) name: Option<String>,
}

// {impl}`requirements.ci.inputs`
// {impl}`requirements.ci.rust_item_inputs`
// {impl}`requirements.ci.requirements_inputs`
pub(crate) struct ResolvedInput {
    pub(crate) ranges: Vec<SourceRange>,
    pub(crate) text: Vec<String>,
    pub(crate) matches: Vec<String>,
}

impl InputSelector {
    // {impl}`requirements.ci.schema`
    pub(crate) fn from_input(input: &Value) -> Option<Self> {
        let object = input.as_object()?;
        if let Some(file) = object.get("file").and_then(Value::as_str) {
            Some(Self::FileRange {
                file: file.to_owned(),
                line_start: usize::try_from(object.get("line_start")?.as_u64()?).ok()?,
                line_end: usize::try_from(object.get("line_end")?.as_u64()?).ok()?,
            })
        } else if let Some(rust_file) = object.get("rust_file").and_then(Value::as_str) {
            Some(Self::RustItem {
                rust_file: rust_file.to_owned(),
                rust_item: object.get("rust_item")?.as_str()?.to_owned(),
            })
        } else {
            Some(Self::Requirements {
                requirements: object.get("requirements")?.as_str()?.to_owned(),
            })
        }
    }

    // {impl}`requirements.cli.add_input`
    pub(crate) fn to_input(&self, resolved: &ResolvedInput) -> Value {
        match self {
            Self::FileRange {
                file,
                line_start,
                line_end,
            } => json!({
                "file": file,
                "line_start": line_start,
                "line_end": line_end,
                "text": resolved.text,
            }),
            Self::RustItem {
                rust_file,
                rust_item,
            } => json!({
                "rust_file": rust_file,
                "rust_item": rust_item,
                "text": resolved.text,
            }),
            Self::Requirements { requirements } => json!({
                "requirements": requirements,
                "matches": resolved.matches,
                "text": resolved.text,
            }),
        }
    }

    // {impl}`requirements.ci.inputs`
    // {impl}`requirements.ci.rust_item_inputs`
    // {impl}`requirements.ci.requirements_inputs`
    pub(crate) fn resolve(
        &self,
        root: &Path,
        definitions: &BTreeMap<String, Declaration>,
    ) -> Result<ResolvedInput, String> {
        match self {
            Self::FileRange {
                file,
                line_start,
                line_end,
            } => {
                let (_, content) = read_project_file(root, file)?;
                let lines = raw_lines(&content);
                if *line_start == 0 || *line_end < *line_start || *line_end > lines.len() {
                    return Err(format!(
                        "{file}:{line_start}-{line_end} is not an inclusive range within the file's {} lines",
                        lines.len()
                    ));
                }
                Ok(ResolvedInput {
                    ranges: vec![SourceRange {
                        file: file.clone(),
                        line_start: *line_start,
                        line_end: *line_end,
                        name: None,
                    }],
                    text: lines[*line_start - 1..*line_end].to_vec(),
                    matches: Vec::new(),
                })
            }
            Self::RustItem {
                rust_file,
                rust_item,
            } => {
                if Path::new(rust_file)
                    .extension()
                    .is_none_or(|extension| extension != "rs")
                {
                    return Err(format!("{rust_file}: Rust-item input requires a .rs file"));
                }
                let (_, content) = read_project_file(root, rust_file)?;
                let items = parse_items(&content)
                    .map_err(|error| format!("{rust_file}: cannot parse Rust source: {error}"))?;
                let mut matching = items.iter().filter(|item| item.selector == *rust_item);
                let Some(item) = matching.next() else {
                    return Err(format!("{rust_file}: no Rust item matches {rust_item:?}"));
                };
                if matching.next().is_some() {
                    return Err(format!("{rust_file}: Rust item {rust_item:?} is ambiguous"));
                }
                Ok(ResolvedInput {
                    ranges: vec![SourceRange {
                        file: rust_file.clone(),
                        line_start: item.line_start,
                        line_end: item.line_end,
                        name: None,
                    }],
                    text: item_lines(&content, item),
                    matches: Vec::new(),
                })
            }
            Self::Requirements { requirements } => {
                definition_glob::validate(requirements)?;
                let mut ranges = Vec::new();
                let mut text = Vec::new();
                let mut matches = Vec::new();
                for (name, definition) in definitions {
                    if !definition_glob::matches(requirements, name) {
                        continue;
                    }
                    check_project_file(root, &definition.file)?;
                    if !text.is_empty() {
                        text.push(String::new());
                    }
                    text.extend(definition.raw_lines.iter().cloned());
                    ranges.push(SourceRange {
                        file: definition.file.clone(),
                        line_start: definition.line_start,
                        line_end: definition.line_end,
                        name: Some(name.clone()),
                    });
                    matches.push(name.clone());
                }
                if matches.is_empty() {
                    return Err(format!(
                        "requirements glob {requirements:?} matches no requirement or example"
                    ));
                }
                Ok(ResolvedInput {
                    ranges,
                    text,
                    matches,
                })
            }
        }
    }
}

// {impl}`requirements.ci.input_containment`
fn check_project_file(root: &Path, file: &str) -> Result<PathBuf, String> {
    let relative = Path::new(file);
    if file.is_empty()
        || !relative
            .components()
            .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
    {
        return Err(format!(
            "{file:?}: file must be a path relative to the repository"
        ));
    }
    let root =
        fs::canonicalize(root).map_err(|error| format!("cannot open repository root: {error}"))?;
    let path = fs::canonicalize(root.join(relative))
        .map_err(|error| format!("cannot open {file}: {error}"))?;
    if !path.starts_with(root) {
        return Err(format!("{file} resolves outside the repository"));
    }
    if !path.is_file() {
        return Err(format!("{file} is not a project file"));
    }
    Ok(path)
}

// {impl}`requirements.ci.input_containment`
pub(crate) fn read_project_file(root: &Path, file: &str) -> Result<(PathBuf, String), String> {
    let path = check_project_file(root, file)?;
    let content =
        fs::read_to_string(&path).map_err(|error| format!("cannot read {file}: {error}"))?;
    Ok((path, content))
}

// {impl}`requirements.ci.inputs`
fn raw_lines(content: &str) -> Vec<String> {
    content
        .lines()
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
        .collect()
}

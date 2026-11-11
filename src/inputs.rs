use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::definition_glob;
use crate::evidence::Declaration;
use crate::outline::{self, Outline, ResolveError};
use crate::redaction::Redaction;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum InputSelector {
    Outline { file: String, item: String },
    Definition { definition: String },
    Requirements { requirements: String },
}

// {impl}`requirements.index.detail_format`
#[derive(Clone, Debug)]
pub(crate) struct SourceRange {
    pub(crate) file: String,
    pub(crate) line_start: usize,
    pub(crate) line_end: usize,
    pub(crate) name: Option<String>,
}

/// The outline node an outline input currently selects.
// {impl}`requirements.index.detail_format`
#[derive(Clone, Debug)]
pub(crate) struct ResolvedNode {
    /// Canonical path of the parsed file.
    pub(crate) path: PathBuf,
    pub(crate) index: usize,
    pub(crate) kind: &'static str,
    pub(crate) hash: String,
}

pub(crate) struct ResolvedInput {
    pub(crate) ranges: Vec<SourceRange>,
    /// Current source text; recorded only for requirements and definition inputs.
    pub(crate) text: Vec<String>,
    pub(crate) matches: Vec<String>,
    pub(crate) node: Option<ResolvedNode>,
}

/// A parsed source file.
pub(crate) struct ParsedSource {
    /// Canonical path.
    pub(crate) path: PathBuf,
    pub(crate) source: String,
    /// The source as models receive it and Habeas displays it, with the same
    /// lines as `source`.
    pub(crate) redacted: String,
    pub(crate) outline: Outline,
}

/// Parsed outlines, cached per file for one validation or authoring run.
#[derive(Default)]
pub(crate) struct OutlineCache {
    files: Mutex<HashMap<PathBuf, Arc<Result<ParsedSource, String>>>>,
    /// Content to parse in place of a file's, by project-relative path.
    overrides: HashMap<String, String>,
}

impl OutlineCache {
    /// A cache that parses `overrides` in place of those files' contents.
    pub(crate) fn with_overrides(overrides: HashMap<String, String>) -> Self {
        Self {
            files: Mutex::default(),
            overrides,
        }
    }

    /// Reads and parses a permitted file, or returns the cached result.
    pub(crate) fn parse(&self, root: &Path, file: &str) -> Arc<Result<ParsedSource, String>> {
        let key = root.join(file);
        if let Some(parsed) = self.files.lock().expect("outline cache lock").get(&key) {
            return parsed.clone();
        }
        let read = match self.overrides.get(file) {
            Some(content) => check_project_file(root, file).map(|path| (path, content.clone())),
            None => read_project_file(root, file),
        };
        let parsed = Arc::new(read.and_then(|(path, source)| {
            let redaction = Redaction::of_file(&path, &source);
            let profile = outline::profile_for_path(&path);
            let outline =
                outline::parse_redacted(profile, &source, &redaction).map_err(|error| {
                    format!("{file}: cannot parse {} source: {error}", profile.name)
                })?;
            Ok(ParsedSource {
                redacted: redaction.apply(&source).into_owned(),
                path: fs::canonicalize(&path).unwrap_or(path),
                source,
                outline,
            })
        }));
        self.files
            .lock()
            .expect("outline cache lock")
            .insert(key, parsed.clone());
        parsed
    }
}

impl ParsedSource {
    /// Resolves a selector to exactly one node.
    // {impl}`requirements.references.outline_selectors`
    pub(crate) fn resolve(&self, file: &str, item: &str) -> Result<usize, String> {
        match self.outline.resolve(item) {
            Ok(index) => Ok(index),
            Err(ResolveError::Invalid(error)) => Err(format!(
                "{file}: invalid outline selector {item:?}: {error}"
            )),
            Err(ResolveError::NotFound(_))
                if self.outline.resolve(&format!("{item}#1")).is_ok() =>
            {
                Err(format!(
                    "{file}: outline selector {item:?} is ambiguous; add a #N disambiguator"
                ))
            }
            Err(ResolveError::NotFound(_)) => {
                Err(format!("{file}: no outline node matches {item:?}"))
            }
        }
    }

    /// The innermost nodes covering the requested lines of an inclusive
    /// range. Blank lines between nodes would otherwise select their common
    /// ancestor, so they are skipped unless every requested line is blank.
    /// With `comments`, comment lines select the comments' owners.
    // {impl}`requirements.references.outline_comments`
    pub(crate) fn nodes_for_range(
        &self,
        line_start: usize,
        line_end: usize,
        comments: bool,
    ) -> Vec<usize> {
        let lines = self
            .source
            .lines()
            .enumerate()
            .map(|(index, line)| (index + 1, line))
            .skip(line_start.saturating_sub(1))
            .take((line_end + 1).saturating_sub(line_start))
            .collect::<Vec<_>>();
        let nonblank = lines
            .iter()
            .filter(|(_, line)| !line.trim().is_empty())
            .map(|(number, _)| *number)
            .collect::<Vec<_>>();
        if nonblank.is_empty() {
            self.outline
                .nodes_for_lines(lines.iter().map(|(number, _)| *number), comments)
        } else {
            self.outline.nodes_for_lines(nonblank, comments)
        }
    }

    /// A node's current inclusive line range and redacted source text.
    pub(crate) fn node_range(&self, file: &str, index: usize) -> (SourceRange, Vec<String>) {
        let node = &self.outline.nodes[index];
        (
            SourceRange {
                file: file.to_owned(),
                line_start: node.line_start,
                line_end: node.line_end.max(node.line_start),
                name: None,
            },
            self.outline.node_lines(&self.redacted, index),
        )
    }
}

impl InputSelector {
    pub(crate) fn from_input(input: &Value) -> Option<Self> {
        let object = input.as_object()?;
        if let Some(item) = object.get("item").and_then(Value::as_str) {
            Some(Self::Outline {
                file: object.get("file")?.as_str()?.to_owned(),
                item: item.to_owned(),
            })
        } else if let Some(definition) = object.get("definition").and_then(Value::as_str) {
            Some(Self::Definition {
                definition: definition.to_owned(),
            })
        } else {
            Some(Self::Requirements {
                requirements: object.get("requirements")?.as_str()?.to_owned(),
            })
        }
    }

    /// The recorded evidence input: selectors and hashes, never outline
    /// source text or line numbers.
    // {impl}`requirements.definitions.inputs`
    pub(crate) fn to_input(&self, resolved: &ResolvedInput) -> Value {
        match self {
            Self::Outline { file, item } => json!({
                "file": file,
                "item": item,
                "hash": resolved.node.as_ref().map(|node| node.hash.as_str()),
            }),
            Self::Definition { definition } => {
                json!({ "definition": definition, "text": resolved.text })
            }
            Self::Requirements { requirements } => json!({
                "requirements": requirements,
                "matches": resolved.matches,
                "text": resolved.text,
            }),
        }
    }

    /// The input as shown to the model: the recorded input plus, for an
    /// outline input, the node's kind and source text, without line numbers.
    // {impl}`requirements.authoring.mandatory_inputs`
    // {impl}`requirements.authoring.no_line_numbers`
    pub(crate) fn to_model_input(&self, resolved: &ResolvedInput) -> Value {
        let mut input = self.to_input(resolved);
        if let (Self::Outline { .. }, Some(node), Some(object)) =
            (self, &resolved.node, input.as_object_mut())
        {
            object.insert("kind".to_owned(), json!(node.kind));
            object.insert("text".to_owned(), json!(resolved.text));
        }
        input
    }

    pub(crate) fn resolve(
        &self,
        root: &Path,
        definitions: &BTreeMap<String, Declaration>,
        outlines: &OutlineCache,
    ) -> Result<ResolvedInput, String> {
        match self {
            Self::Outline { file, item } => {
                let parsed = outlines.parse(root, file);
                let parsed = parsed.as_ref().as_ref().map_err(Clone::clone)?;
                let index = parsed.resolve(file, item)?;
                let (range, text) = parsed.node_range(file, index);
                let node = &parsed.outline.nodes[index];
                Ok(ResolvedInput {
                    ranges: vec![range],
                    text,
                    matches: Vec::new(),
                    node: Some(ResolvedNode {
                        path: parsed.path.clone(),
                        index,
                        kind: if index == 0 { "file" } else { node.kind },
                        hash: node.structural_hash.clone(),
                    }),
                })
            }
            Self::Definition { definition } => {
                let target = definitions
                    .get(definition)
                    .filter(|d| d.kind == crate::DefinitionKind::Term)
                    .ok_or_else(|| format!("{definition}: no matching named definition"))?;
                check_project_file(root, &target.file)?;
                Ok(ResolvedInput {
                    ranges: vec![SourceRange {
                        file: target.file.clone(),
                        line_start: target.line_start,
                        line_end: target.line_end,
                        name: Some(definition.clone()),
                    }],
                    text: crate::redaction::redact_lines(&target.raw_lines),
                    matches: Vec::new(),
                    node: None,
                })
            }
            Self::Requirements { requirements } => {
                definition_glob::validate(requirements)?;
                let mut ranges = Vec::new();
                let mut text = Vec::new();
                let mut matches = Vec::new();
                for (name, definition) in definitions {
                    if definition.kind == crate::DefinitionKind::Term
                        || !definition_glob::matches(requirements, name)
                    {
                        continue;
                    }
                    check_project_file(root, &definition.file)?;
                    if !text.is_empty() {
                        text.push(String::new());
                    }
                    text.extend(crate::redaction::redact_lines(&definition.raw_lines));
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
                    node: None,
                })
            }
        }
    }
}

/// The recorded form of a model-facing input: outline inputs keep only
/// `file`, `item`, and `hash`.
// {impl}`requirements.authoring.delivered_inputs`
pub(crate) fn recorded_input(input: &Value) -> Value {
    match input.as_object() {
        Some(object) if object.contains_key("item") => json!({
            "file": object.get("file"),
            "item": object.get("item"),
            "hash": object.get("hash"),
        }),
        _ => input.clone(),
    }
}

/// The recorded form of a file path: an absolute path inside the project at
/// the canonical `root` becomes relative to the project for portability. Relative paths, and
/// paths that leave the project, are kept as given.
pub(crate) fn project_relative(root: &Path, file: &str) -> String {
    let path = Path::new(file);
    if !path.is_absolute() {
        return file.to_owned();
    }
    let Some(base) = path
        .ancestors()
        .find(|ancestor| fs::canonicalize(ancestor).is_ok_and(|ancestor| ancestor == root))
    else {
        return file.to_owned();
    };
    let parts = path
        .strip_prefix(base)
        .expect("ancestor is a prefix of its path")
        .components()
        .map(|part| match part {
            std::path::Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>();
    match parts {
        Some(parts) if !parts.is_empty() => parts.join("/"),
        _ => file.to_owned(),
    }
}

fn check_project_file(root: &Path, file: &str) -> Result<PathBuf, String> {
    crate::authoring::access::resolve_evidence_file(root, file).map_err(|error| error.to_string())
}

pub(crate) fn read_project_file(root: &Path, file: &str) -> Result<(PathBuf, String), String> {
    let path = check_project_file(root, file)?;
    let content =
        fs::read_to_string(&path).map_err(|error| format!("cannot read {file}: {error}"))?;
    Ok((path, content))
}

/// Resolves the outline node `item` in `file`, relative to the project
/// `root`, and returns the evidence input recording it: `{file, item, hash}`.
/// The empty selector selects the whole file. Intended for tools and tests
/// that construct evidence records.
// {impl}`requirements.ci.schema`
pub fn outline_input(root: &Path, file: &str, item: &str) -> std::io::Result<Value> {
    let root = fs::canonicalize(root)?;
    let selector = InputSelector::Outline {
        file: file.to_owned(),
        item: item.to_owned(),
    };
    let resolved = selector
        .resolve(&root, &BTreeMap::new(), &OutlineCache::default())
        .map_err(std::io::Error::other)?;
    Ok(selector.to_input(&resolved))
}

/// Returns the evidence inputs for the innermost outline nodes covering an
/// inclusive, one-based line range of `file`, as the `habeas` authoring tool
/// records them.
// {impl}`requirements.ci.outline_inputs`
// {impl}`requirements.authoring.source_tools`
pub fn outline_inputs_for_lines(
    root: &Path,
    file: &str,
    line_start: usize,
    line_end: usize,
) -> std::io::Result<Vec<Value>> {
    let root = fs::canonicalize(root)?;
    let outlines = OutlineCache::default();
    let parsed = outlines.parse(&root, file);
    let parsed = parsed
        .as_ref()
        .as_ref()
        .map_err(|error| std::io::Error::other(error.clone()))?;
    parsed
        .nodes_for_range(line_start, line_end, false)
        .into_iter()
        .map(|index| outline_input(&root, file, &parsed.outline.nodes[index].selector))
        .collect()
}

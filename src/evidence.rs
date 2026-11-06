use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde_json::{Map, Value};
use toml_edit::DocumentMut;

use crate::inputs::{InputSelector, OutlineCache, ResolvedInput, ResolvedNode};
use crate::judgment::Judgment;
use crate::{
    DefinitionKind, ReferenceKind, chapter_namespace, definition_blocks, full_requirement_name,
    reference_roles, valid_name,
};

// {impl}`requirements.integration.report_api`
#[derive(Debug)]
pub struct EvidenceReport {
    pub requirements: usize,
    pub examples: usize,
    pub evidence_files: usize,
    pub errors: Vec<String>,
}

// {impl}`requirements.references.anchors`
pub(crate) struct Declaration {
    pub(crate) kind: DefinitionKind,
    pub(crate) text: String,
    pub(crate) source: String,
    pub(crate) file: String,
    pub(crate) line_start: usize,
    pub(crate) line_end: usize,
    pub(crate) raw_lines: Vec<String>,
    pub(crate) anchors: Vec<Anchor>,
}

/// An `:impl:` or `:test:` option naming a project file, or one outline node
/// in it as `path#selector`.
// {impl}`requirements.references.anchors`
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Anchor {
    pub(crate) kind: ReferenceKind,
    pub(crate) file: String,
    /// Outline selector; empty for the file's root node.
    pub(crate) item: String,
}

/// The node an anchor currently resolves to.
// {impl}`requirements.references.anchors`
pub(crate) struct AnchorTarget {
    /// Canonical path of the anchored file.
    pub(crate) path: PathBuf,
    /// The anchored node followed by its outline ancestors.
    pub(crate) nodes: Vec<usize>,
    pub(crate) line_start: usize,
    pub(crate) line_end: usize,
}

impl Anchor {
    /// Parses an option value: a path, optionally followed by `#selector`.
    // {impl}`requirements.references.anchors`
    pub(crate) fn parse(kind: ReferenceKind, value: &str) -> Self {
        let (file, item) = value.split_once('#').unwrap_or((value, ""));
        Self {
            kind,
            file: file.to_owned(),
            item: item.to_owned(),
        }
    }

    /// The option value as written.
    // {impl}`requirements.references.anchors`
    pub(crate) fn label(&self) -> String {
        if self.item.is_empty() {
            self.file.clone()
        } else {
            format!("{}#{}", self.file, self.item)
        }
    }

    /// Resolves the anchored file and node.
    // {impl}`requirements.references.anchors`
    pub(crate) fn resolve(
        &self,
        root: &Path,
        outlines: &OutlineCache,
    ) -> Result<AnchorTarget, String> {
        if self.file.is_empty() {
            return Err("anchor path must be nonempty".to_owned());
        }
        let parsed = outlines.parse(root, &self.file);
        let parsed = parsed.as_ref().as_ref().map_err(Clone::clone)?;
        let index = parsed.resolve(&self.file, &self.item)?;
        let node = &parsed.outline.nodes[index];
        let mut nodes = vec![index];
        let mut ancestor = node.parent;
        while let Some(parent) = ancestor {
            nodes.push(parent);
            ancestor = parsed.outline.nodes[parent].parent;
        }
        Ok(AnchorTarget {
            path: parsed.path.clone(),
            nodes,
            line_start: node.line_start,
            line_end: node.line_end,
        })
    }

    /// The outline input selecting the anchored node.
    // {impl}`requirements.references.anchors`
    pub(crate) fn selector(&self) -> InputSelector {
        InputSelector::Outline {
            file: self.file.clone(),
            item: self.item.clone(),
        }
    }
}

// {impl}`requirements.integration.report_api`
pub fn validate_repository(root: &Path) -> io::Result<EvidenceReport> {
    Ok(inspect_repository(root)?.report)
}

pub(crate) struct RepositoryInspection {
    pub(crate) definitions: BTreeMap<String, Declaration>,
    pub(crate) references: Vec<SourceReference>,
    pub(crate) report: EvidenceReport,
    pub(crate) discovery_errors: Vec<String>,
}

/// Progress within one inspection phase; discovery has no known total.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InspectionProgress {
    pub(crate) stage: &'static str,
    pub(crate) completed: usize,
    pub(crate) total: Option<usize>,
}

// {impl}`requirements.authoring.mandatory_inputs`
// {impl}`requirements.ci.book_source`
// {impl}`requirements.ci.definition_errors`
pub(crate) fn load_definitions(root: &Path) -> io::Result<BTreeMap<String, Declaration>> {
    let root = fs::canonicalize(root)?;
    let mut errors = Vec::new();
    let md_root = book_source_dir(&root, &mut errors);
    let definitions = collect_definitions(&root, &md_root, &mut errors);
    if errors.is_empty() {
        Ok(definitions)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            errors.join("\n"),
        ))
    }
}

// {impl}`requirements.integration.report_api`
// {impl}`requirements.definitions.declaration`
// {impl}`requirements.authoring.batch`
// {impl}`requirements.ci.errors`
// {impl}`requirements.ci.diagnostics`
// {impl}`requirements.examples.evidence`
// {impl}`requirements.configuration.workspace_location`
// {impl}`requirements.configuration.workspace_structure`
// {impl}`requirements.configuration.loading`
pub(crate) fn inspect_repository(root: &Path) -> io::Result<RepositoryInspection> {
    inspect_repository_with_progress(root, |_| {})
}

// {impl}`requirements.integration.report_api`
// {impl}`requirements.definitions.declaration`
// {impl}`requirements.authoring.batch`
// {impl}`requirements.authoring.progress`
// {impl}`requirements.ci.errors`
// {impl}`requirements.ci.diagnostics`
// {impl}`requirements.examples.evidence`
// {impl}`requirements.configuration.workspace_location`
// {impl}`requirements.configuration.workspace_structure`
// {impl}`requirements.configuration.loading`
pub(crate) fn inspect_repository_with_progress(
    root: &Path,
    mut progress: impl FnMut(InspectionProgress),
) -> io::Result<RepositoryInspection> {
    let root = fs::canonicalize(root)?;
    progress(InspectionProgress {
        stage: "discovering requirements",
        completed: 0,
        total: None,
    });
    let mut errors = Vec::new();
    if let Err(error) = crate::config::load_workspace_evidence(&root) {
        errors.push(error.to_string());
    }
    let md_root = book_source_dir(&root, &mut errors);
    let definitions = collect_definitions(&root, &md_root, &mut errors);
    let mut discovery_errors = errors.clone();
    let evidence_root = root.join("evidence");
    let evidence_files = if evidence_root.is_dir() {
        files_with_extension(&root, &evidence_root, "json", &mut errors)
    } else {
        Vec::new()
    };
    let mut present = BTreeSet::new();
    let outlines = OutlineCache::default();
    let ordinary_files: Vec<_> = evidence_files.iter().collect();

    // {impl}`requirements.ci.orphans`
    for (completed, file) in ordinary_files.iter().enumerate() {
        progress(InspectionProgress {
            stage: "evidence files",
            completed,
            total: Some(ordinary_files.len()),
        });
        let relative = file
            .strip_prefix(&evidence_root)
            .expect("evidence file is inside evidence directory");
        let label = display_path(&root, file);
        let name = if relative.components().count() == 1 {
            relative
                .file_name()
                .and_then(|part| part.to_str())
                .and_then(|part| part.strip_suffix(".json"))
        } else {
            None
        };
        let expected = name
            .and_then(|name| definitions.get(name))
            .filter(|d| d.kind != DefinitionKind::Term);

        if let Some(name) = name.filter(|_| expected.is_some()) {
            present.insert(name.to_owned());
        } else {
            errors.push(format!("{label}: no matching requirement or example"));
        }
        validate_evidence_file(&root, file, expected, &definitions, &outlines, &mut errors);
    }
    progress(InspectionProgress {
        stage: "evidence files",
        completed: ordinary_files.len(),
        total: Some(ordinary_files.len()),
    });

    // {impl}`requirements.ci.coverage`
    for (name, definition) in &definitions {
        if definition.kind != DefinitionKind::Term && !present.contains(name) {
            errors.push(format!(
                "evidence/{name}.json: missing for {} at {}",
                definition.kind.noun(),
                definition.source
            ));
        }
    }

    let references = collect_references_with_progress(&root, &mut errors, &mut progress);
    validate_references(
        &root,
        &definitions,
        &references,
        &outlines,
        &mut errors,
        &mut progress,
    );
    // {impl}`requirements.references.rust_item_coverage`
    progress(InspectionProgress {
        stage: "Rust citations",
        completed: 0,
        total: None,
    });
    crate::rust_coverage::validate_rust_functions(
        &root,
        &definitions,
        &mut errors,
        &mut discovery_errors,
    );

    let examples = definitions
        .values()
        .filter(|definition| definition.kind == DefinitionKind::Example)
        .count();
    let requirements = definitions
        .values()
        .filter(|definition| definition.kind == DefinitionKind::Requirement)
        .count();
    Ok(RepositoryInspection {
        report: EvidenceReport {
            requirements,
            examples,
            evidence_files: evidence_files.len(),
            errors,
        },
        definitions,
        references,
        discovery_errors,
    })
}

// {impl}`requirements.ci.book_source`
// {impl}`requirements.configuration.loading`
// {impl}`requirements.ci.diagnostics`
fn book_source_dir(root: &Path, errors: &mut Vec<String>) -> PathBuf {
    let path = root.join("book.toml");
    let source = match fs::read_to_string(&path) {
        Ok(content) => match content.parse::<DocumentMut>() {
            Ok(book) => {
                let table = match book.get("book") {
                    Some(item) => match item.as_table_like() {
                        Some(table) => Some(table),
                        None => {
                            errors.push("book.toml: book must be a table".to_owned());
                            return root.join("md");
                        }
                    },
                    None => None,
                };
                match table.and_then(|table| table.get("src")) {
                    Some(item) => match item.as_str() {
                        Some(source) => source.to_owned(),
                        None => {
                            errors.push("book.toml: book.src must be a string".to_owned());
                            return root.join("md");
                        }
                    },
                    None => "src".to_owned(),
                }
            }
            Err(error) => {
                errors.push(format!("book.toml: invalid TOML: {error}"));
                return root.join("md");
            }
        },
        Err(error) => {
            errors.push(format!(
                "book.toml: cannot read book configuration: {error}"
            ));
            return root.join("md");
        }
    };
    let relative = Path::new(&source);
    if source.is_empty()
        || !relative
            .components()
            .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
    {
        errors.push("book.toml: book.src must be a directory inside the repository".to_owned());
        return root.join("md");
    }
    root.join(relative)
}

// {impl}`requirements.ci.definition_errors`
// {impl}`requirements.ci.identifiers`
// {impl}`requirements.examples.definition`
// {impl}`requirements.definitions.declaration`
// {impl}`requirements.references.anchors`
fn collect_definitions(
    root: &Path,
    md_root: &Path,
    errors: &mut Vec<String>,
) -> BTreeMap<String, Declaration> {
    let mut definitions = BTreeMap::new();

    for file in files_with_extension(root, md_root, "md", errors) {
        let relative = file
            .strip_prefix(md_root)
            .expect("chapter file is inside md directory");
        if relative == Path::new("SUMMARY.md") {
            continue;
        }

        let label = display_path(root, &file);
        let content = match fs::read_to_string(&file) {
            Ok(content) => content,
            Err(error) => {
                errors.push(format!("{label}: cannot read chapter: {error}"));
                continue;
            }
        };
        let namespace = chapter_namespace(relative);
        let blocks = definition_blocks(&content);
        for block in &blocks {
            let line = content[..block.start]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1;
            let source = format!("{label}:{line}");
            let name = full_requirement_name(&namespace, &block.name);
            if !valid_name(&name) {
                errors.push(format!(
                    "{source}: invalid {} name {name:?}",
                    block.kind.noun()
                ));
                continue;
            }

            if !block.closed {
                errors.push(format!(
                    "{source}: {} {name} is missing a closing colon fence",
                    block.kind.noun()
                ));
            }
            // {impl}`requirements.ci.span`
            // {impl}`requirements.references.anchors`
            let text = crate::without_options(&content, block.body_start..block.body_end, &blocks)
                .trim()
                .to_owned();
            let mut anchors = Vec::new();
            for option in &block.options {
                let kind = match (block.kind, option.name.as_str()) {
                    (DefinitionKind::Requirement, "impl") => ReferenceKind::Impl,
                    (DefinitionKind::Requirement | DefinitionKind::Example, "test") => {
                        ReferenceKind::Test
                    }
                    _ => {
                        errors.push(format!(
                            "{label}:{}: {} {name} has unsupported option :{}:",
                            option.line,
                            block.kind.noun(),
                            option.name
                        ));
                        continue;
                    }
                };
                if option.value.is_empty() {
                    errors.push(format!(
                        "{label}:{}: {} {name} has an :{}: option without a file path",
                        option.line,
                        block.kind.noun(),
                        option.name
                    ));
                    continue;
                }
                anchors.push(Anchor::parse(kind, &option.value));
            }
            if text.is_empty() {
                errors.push(format!(
                    "{source}: {} {name} has no text",
                    block.kind.noun()
                ));
            }

            if block.kind == DefinitionKind::Term
                && let Err(error) = crate::terms::declaration(&text)
            {
                errors.push(format!("{source}: definition {name}: {error}"));
            }
            match definitions.entry(name.clone()) {
                Entry::Vacant(slot) => {
                    // {impl}`requirements.references.anchors`
                    let raw_lines =
                        crate::without_options(&content, block.start..block.end, &blocks)
                            .lines()
                            .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
                            .collect::<Vec<_>>();
                    let line_count = content[block.start..block.end].lines().count();
                    slot.insert(Declaration {
                        kind: block.kind,
                        text,
                        source,
                        file: label.clone(),
                        line_start: line,
                        line_end: line + line_count.saturating_sub(1),
                        raw_lines,
                        anchors,
                    });
                }
                Entry::Occupied(previous) => errors.push(format!(
                    "{source}: duplicate {} {name}; first declared at {}",
                    block.kind.noun(),
                    previous.get().source
                )),
            }
        }
    }

    for (name, definition) in &definitions {
        if definition.kind == DefinitionKind::Example
            && !reference_roles(&definition.text, true)
                .into_iter()
                .any(|reference| {
                    matches!(reference.kind, ReferenceKind::Cite | ReferenceKind::Test)
                        && definitions
                            .get(reference.name)
                            .is_some_and(|target| target.kind == DefinitionKind::Requirement)
                })
        {
            errors.push(format!(
                "{}: example {name} must reference at least one requirement with {{cite}} or {{test}}",
                definition.source
            ));
        }
    }

    definitions
}

// {impl}`requirements.ci.coverage`
fn files_with_extension(
    root: &Path,
    dir: &Path,
    extension: &str,
    errors: &mut Vec<String>,
) -> Vec<PathBuf> {
    let mut stack = vec![dir.to_path_buf()];
    let mut files = Vec::new();

    while let Some(directory) = stack.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                errors.push(format!(
                    "{}: cannot list directory: {error}",
                    display_path(root, &directory)
                ));
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    errors.push(format!(
                        "{}: cannot read directory entry: {error}",
                        display_path(root, &directory)
                    ));
                    continue;
                }
            };
            let path = entry.path();
            let kind = match entry.file_type() {
                Ok(kind) => kind,
                Err(error) => {
                    errors.push(format!(
                        "{}: cannot inspect file: {error}",
                        display_path(root, &path)
                    ));
                    continue;
                }
            };
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() && path.extension().is_some_and(|value| value == extension) {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

// {impl}`requirements.ci.schema`
// {impl}`requirements.examples.evidence`
// {impl}`requirements.definitions.dependencies`
// {impl}`requirements.ci.diagnostics`
fn validate_evidence_file(
    root: &Path,
    path: &Path,
    expected: Option<&Declaration>,
    definitions: &BTreeMap<String, Declaration>,
    outlines: &OutlineCache,
    errors: &mut Vec<String>,
) {
    let label = display_path(root, path);
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) => {
            errors.push(format!("{label}: cannot read evidence: {error}"));
            return;
        }
    };
    let value: Value = match serde_json::from_str(&content) {
        Ok(value) => value,
        Err(error) => {
            errors.push(format!("{label}: invalid JSON: {error}"));
            return;
        }
    };
    let Some(object) = value.as_object() else {
        errors.push(format!("{label}: expected a JSON object"));
        return;
    };
    let kind = expected
        .map(|definition| definition.kind)
        .unwrap_or_else(|| {
            if object.contains_key("example") {
                DefinitionKind::Example
            } else {
                DefinitionKind::Requirement
            }
        });
    let text_field = kind.noun();
    check_keys(
        object,
        &[text_field, "determination", "rationale", "inputs"],
        &label,
        errors,
    );

    // {impl}`requirements.ci.requirement_text`
    if let Some(text) = string_field(object, text_field, &label, errors) {
        if text.trim().is_empty() {
            errors.push(format!("{label}: {text_field} must not be empty"));
        }
        if let Some(expected) = expected
            && text != expected.text
        {
            errors.push(format!(
                "{label}: {text_field} text changed at {}{}",
                expected.source,
                compact_diff(
                    &text.split('\n').collect::<Vec<_>>(),
                    &expected.text.split('\n').collect::<Vec<_>>()
                )
            ));
        }
    }

    let determination = match object.get("determination") {
        Some(value) => match Judgment::from_value(value) {
            Ok(judgment) => Some(judgment),
            Err(error) => {
                errors.push(format!("{label}: determination: {error}"));
                None
            }
        },
        None => {
            errors.push(format!("{label}: missing field \"determination\""));
            None
        }
    };
    let rationale = string_field(object, "rationale", &label, errors);
    if let Some(rationale) = rationale
        && rationale.trim().is_empty()
    {
        errors.push(format!("{label}: rationale must not be empty"));
    }
    // {impl}`requirements.ci.determination`
    if determination == Some(Judgment::False) {
        errors.push(format!(
            "{label}: determination is false{}",
            rationale
                .filter(|text| !text.trim().is_empty())
                .map_or(String::new(), |text| format!(": {}", text.trim()))
        ));
    }

    crate::terms::validate_dependencies(
        root,
        definitions,
        outlines,
        object,
        expected.map(|d| d.text.as_str()),
        &label,
        errors,
    );
    if let Some(inputs) = array_field(object, "inputs", &label, errors) {
        for (index, input) in inputs.iter().enumerate() {
            validate_input(
                root,
                definitions,
                outlines,
                input,
                &format!("{label}: inputs[{index}]"),
                errors,
            );
        }
    }
}

// {impl}`requirements.ci.schema`
// {impl}`requirements.ci.input_containment`
// {impl}`requirements.ci.outline_inputs`
// {impl}`requirements.ci.requirements_inputs`
// {impl}`requirements.definitions.inputs`
fn validate_input(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    outlines: &OutlineCache,
    input: &Value,
    label: &str,
    errors: &mut Vec<String>,
) -> Option<ResolvedInput> {
    let Some(object) = input.as_object() else {
        errors.push(format!("{label}: expected a JSON object"));
        return None;
    };
    if object.contains_key("definition") {
        check_keys(object, &["definition", "text"], label, errors);
        let definition = string_field(object, "definition", label, errors);
        let text = text_field(object, label, errors);
        let (Some(definition), Some(text)) = (definition, text) else {
            return None;
        };
        return validate_resolved_input(
            root,
            definitions,
            outlines,
            &InputSelector::Definition {
                definition: definition.to_owned(),
            },
            &text,
            None,
            label,
            errors,
        );
    }
    if object.contains_key("requirements") || object.contains_key("matches") {
        check_keys(object, &["requirements", "matches", "text"], label, errors);
        let requirements = string_field(object, "requirements", label, errors);
        let matches = array_field(object, "matches", label, errors).and_then(|values| {
            let mut names = Vec::new();
            for (index, value) in values.iter().enumerate() {
                match value.as_str() {
                    Some(name) => names.push(name.to_owned()),
                    None => errors.push(format!("{label}: matches[{index}] must be a string")),
                }
            }
            if names.len() != values.len() {
                return None;
            }
            if names.iter().collect::<BTreeSet<_>>().len() != names.len() {
                errors.push(format!(
                    "{label}: matches must have distinct definition names"
                ));
            }
            Some(names)
        });
        let text = text_field(object, label, errors);
        let (Some(requirements), Some(matches), Some(text)) = (requirements, matches, text) else {
            return None;
        };
        let selector = InputSelector::Requirements {
            requirements: requirements.to_owned(),
        };
        return validate_resolved_input(
            root,
            definitions,
            outlines,
            &selector,
            &text,
            Some(&matches),
            label,
            errors,
        );
    }
    validate_outline_input(root, definitions, outlines, object, label, errors)
}

// {impl}`requirements.ci.schema`
// {impl}`requirements.ci.outline_inputs`
// {impl}`requirements.configuration.evidence_paths`
// {impl}`requirements.ci.diagnostics`
fn validate_outline_input(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    outlines: &OutlineCache,
    object: &Map<String, Value>,
    label: &str,
    errors: &mut Vec<String>,
) -> Option<ResolvedInput> {
    check_keys(object, &["file", "item", "hash"], label, errors);
    let file = string_field(object, "file", label, errors).and_then(|file| {
        if file.is_empty() {
            errors.push(format!("{label}: file must be a nonempty path"));
            None
        } else {
            Some(file)
        }
    });
    let item = string_field(object, "item", label, errors);
    let hash = string_field(object, "hash", label, errors).and_then(|hash| {
        if hash.len() == 64
            && hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            Some(hash)
        } else {
            errors.push(format!(
                "{label}: hash must be a lowercase hexadecimal SHA-256 digest"
            ));
            None
        }
    });
    let (Some(file), Some(item), Some(hash)) = (file, item, hash) else {
        return None;
    };
    let selector = InputSelector::Outline {
        file: file.to_owned(),
        item: item.to_owned(),
    };
    let resolved = match selector.resolve(root, definitions, outlines) {
        Ok(resolved) => resolved,
        Err(error) => {
            errors.push(format!("{label}: {error}"));
            return None;
        }
    };
    if let (Some(node), Some(range)) = (&resolved.node, resolved.ranges.first())
        && node.hash != hash
    {
        errors.push(format!(
            "{label}: outline input changed: {file} {} (now lines {}-{}) no longer has the recorded hash",
            if item.is_empty() { "<root>" } else { item },
            range.line_start,
            range.line_end
        ));
    }
    Some(resolved)
}

// {impl}`requirements.ci.schema`
fn text_field(
    object: &Map<String, Value>,
    label: &str,
    errors: &mut Vec<String>,
) -> Option<Vec<String>> {
    let values = array_field(object, "text", label, errors)?;
    if values.is_empty() {
        errors.push(format!("{label}: text must be a nonempty array"));
    }
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            value.as_str().map(str::to_owned).or_else(|| {
                errors.push(format!("{label}: text[{index}] must be a string"));
                None
            })
        })
        .collect()
}

// {impl}`requirements.ci.requirements_inputs`
// {impl}`requirements.definitions.inputs`
// {impl}`requirements.ci.diagnostics`
#[allow(clippy::too_many_arguments)]
fn validate_resolved_input(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    outlines: &OutlineCache,
    selector: &InputSelector,
    text: &[String],
    matches: Option<&[String]>,
    label: &str,
    errors: &mut Vec<String>,
) -> Option<ResolvedInput> {
    let resolved = match selector.resolve(root, definitions, outlines) {
        Ok(resolved) => resolved,
        Err(error) => {
            errors.push(format!("{label}: {error}"));
            return None;
        }
    };
    if let Some(matches) = matches
        && matches != resolved.matches
    {
        let missing = resolved
            .matches
            .iter()
            .filter(|name| !matches.contains(name))
            .cloned()
            .collect::<Vec<_>>();
        let removed = matches
            .iter()
            .filter(|name| !resolved.matches.contains(name))
            .cloned()
            .collect::<Vec<_>>();
        errors.push(format!(
            "{label}: requirements glob matches changed; added {missing:?}, removed {removed:?}"
        ));
    }
    if text != resolved.text {
        let old = text.iter().map(String::as_str).collect::<Vec<_>>();
        let new = resolved.text.iter().map(String::as_str).collect::<Vec<_>>();
        errors.push(format!(
            "{label}: input text changed for {selector:?}{}",
            compact_diff(&old, &new)
        ));
    }
    Some(resolved)
}

// {impl}`requirements.ci.diagnostics`
fn compact_diff(recorded: &[&str], current: &[&str]) -> String {
    let prefix = recorded
        .iter()
        .zip(current.iter())
        .take_while(|(old, new)| old == new)
        .count();
    let suffix = recorded[prefix..]
        .iter()
        .rev()
        .zip(current[prefix..].iter().rev())
        .take_while(|(old, new)| old == new)
        .count();
    let removed = &recorded[prefix..recorded.len() - suffix];
    let added = &current[prefix..current.len() - suffix];
    let mut diff = String::new();
    for (mark, lines) in [("-", removed), ("+", added)] {
        for line in lines.iter().take(8) {
            diff.push_str(&format!("\n{mark} {line}"));
        }
        if lines.len() > 8 {
            diff.push_str(&format!("\n{mark} ... {} more lines", lines.len() - 8));
        }
    }
    diff
}

// {impl}`requirements.ci.schema`
fn check_keys(
    object: &Map<String, Value>,
    expected: &[&str],
    label: &str,
    errors: &mut Vec<String>,
) {
    for key in object.keys() {
        if !expected.contains(&key.as_str()) {
            errors.push(format!("{label}: unexpected field {key:?}"));
        }
    }
}

// {impl}`requirements.ci.schema`
fn string_field<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    label: &str,
    errors: &mut Vec<String>,
) -> Option<&'a str> {
    match object.get(key) {
        Some(Value::String(value)) => Some(value),
        Some(_) => {
            errors.push(format!("{label}: {key} must be a string"));
            None
        }
        None => {
            errors.push(format!("{label}: missing field {key:?}"));
            None
        }
    }
}

// {impl}`requirements.ci.schema`
fn array_field<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    label: &str,
    errors: &mut Vec<String>,
) -> Option<&'a Vec<Value>> {
    match object.get(key) {
        Some(Value::Array(values)) => Some(values),
        Some(_) => {
            errors.push(format!("{label}: {key} must be an array"));
            None
        }
        None => {
            errors.push(format!("{label}: missing field {key:?}"));
            None
        }
    }
}

// {impl}`requirements.ci.diagnostics`
fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

pub(crate) struct SourceReference {
    pub(crate) name: String,
    pub(crate) kind: ReferenceKind,
    pub(crate) file: PathBuf,
    pub(crate) line: usize,
}

// {impl}`requirements.references.evidence_coverage`
// {impl}`requirements.references.anchors`
struct InputRange {
    file: PathBuf,
    line_start: usize,
    line_end: usize,
    node: Option<ResolvedNode>,
}

// {impl}`requirements.references.evidence_coverage`
// {impl}`requirements.references.targets`
// {impl}`requirements.examples.evidence`
// {impl}`requirements.definitions.references`
// {impl}`requirements.definitions.declaration`
// {impl}`requirements.authoring.progress`
// {impl}`requirements.references.anchors`
fn validate_references(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    references: &[SourceReference],
    outlines: &OutlineCache,
    errors: &mut Vec<String>,
    progress: &mut impl FnMut(InspectionProgress),
) {
    let mut by_definition: BTreeMap<String, Vec<&SourceReference>> = BTreeMap::new();
    for reference in references {
        let target = definitions.get(&reference.name);
        if reference.kind == ReferenceKind::Term {
            if !target.is_some_and(|d| d.kind == DefinitionKind::Term) {
                errors.push(format!(
                    "{}:{}: term {} has no matching named definition",
                    display_path(root, &reference.file),
                    reference.line,
                    reference.name
                ));
            }
            continue;
        }
        if target.is_some_and(|d| d.kind != DefinitionKind::Term) {
            by_definition
                .entry(reference.name.clone())
                .or_default()
                .push(reference);
        } else {
            errors.push(format!(
                "{}:{}: {{{}}}`{}` has no matching requirement or example",
                display_path(root, &reference.file),
                reference.line,
                reference.kind.label(),
                reference.name
            ));
        }
    }

    let subjects: Vec<_> = definitions
        .iter()
        .filter(|(_, definition)| definition.kind != DefinitionKind::Term)
        .collect();
    for (completed, (name, definition)) in subjects.iter().enumerate() {
        progress(InspectionProgress {
            stage: "reference coverage",
            completed,
            total: Some(subjects.len()),
        });
        let evidence_file = root.join("evidence").join(format!("{name}.json"));
        let references = by_definition.remove(*name).unwrap_or_default();
        if definition.kind == DefinitionKind::Example
            && !references
                .iter()
                .any(|reference| reference.kind == ReferenceKind::Test)
            && !definition
                .anchors
                .iter()
                .any(|anchor| anchor.kind == ReferenceKind::Test)
            && fs::read_to_string(&evidence_file)
                .ok()
                .and_then(|content| serde_json::from_str::<Value>(&content).ok())
                .and_then(|value| value.get("determination").and_then(Value::as_bool))
                == Some(true)
        {
            errors.push(format!(
                "{}: example {name} has a true determination but no {{test}} reference",
                display_path(root, &evidence_file)
            ));
        }
        let inputs = evidence_input_ranges(root, &evidence_file, definitions, outlines);
        // `{cite}` points from its source to the definition, so it is not part of
        // the definition's evidence.
        for reference in references
            .into_iter()
            .filter(|reference| reference.kind != ReferenceKind::Cite)
        {
            if !inputs.iter().any(|input| {
                input.file == reference.file
                    && input.line_start <= reference.line
                    && reference.line <= input.line_end
            }) {
                errors.push(format!(
                    "{}: no input covers {{{}}}`{}` at {}:{}",
                    display_path(root, &evidence_file),
                    reference.kind.label(),
                    name,
                    display_path(root, &reference.file),
                    reference.line
                ));
            }
        }
        // An anchor is an edge to a file's root node or another node, so an
        // outline input must select that node or one of its ancestors.
        for anchor in &definition.anchors {
            match anchor.resolve(root, outlines) {
                Ok(target) => {
                    if !inputs.iter().any(|input| {
                        input.node.as_ref().is_some_and(|node| {
                            node.path == target.path && target.nodes.contains(&node.index)
                        })
                    }) {
                        errors.push(format!(
                            "{}: no input selects {} (lines {}-{}) anchored by :{}: on {} {name}",
                            display_path(root, &evidence_file),
                            anchor.label(),
                            target.line_start,
                            target.line_end,
                            anchor.kind.label(),
                            definition.kind.noun()
                        ));
                    }
                }
                Err(error) => errors.push(format!(
                    "{}: :{}: anchor {} on {} {name} at {} does not resolve to a readable, permitted file or node: {error}",
                    display_path(root, &evidence_file),
                    anchor.kind.label(),
                    anchor.label(),
                    definition.kind.noun(),
                    definition.source
                )),
            }
        }
    }
    progress(InspectionProgress {
        stage: "reference coverage",
        completed: subjects.len(),
        total: Some(subjects.len()),
    });
}

// {impl}`requirements.references.targets`
pub(crate) fn collect_references(root: &Path, errors: &mut Vec<String>) -> Vec<SourceReference> {
    collect_references_with_progress(root, errors, &mut |_| {})
}

// {impl}`requirements.references.targets`
// {impl}`requirements.authoring.progress`
fn collect_references_with_progress(
    root: &Path,
    errors: &mut Vec<String>,
    progress: &mut impl FnMut(InspectionProgress),
) -> Vec<SourceReference> {
    progress(InspectionProgress {
        stage: "discovering source references",
        completed: 0,
        total: None,
    });
    let mut stack = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = stack.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                errors.push(format!(
                    "{}: cannot scan references: {error}",
                    display_path(root, &directory)
                ));
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    errors.push(format!(
                        "{}: cannot scan directory entry: {error}",
                        display_path(root, &directory)
                    ));
                    continue;
                }
            };
            let path = entry.path();
            let kind = match entry.file_type() {
                Ok(kind) => kind,
                Err(error) => {
                    errors.push(format!(
                        "{}: cannot inspect reference source: {error}",
                        display_path(root, &path)
                    ));
                    continue;
                }
            };
            if kind.is_dir() {
                // Repository metadata and Cargo build output are skipped at
                // any depth; the book build and evidence snapshots at the
                // project root.
                let name = entry.file_name();
                let excluded = matches!(name.to_str(), Some(".git" | "target"))
                    || (directory == root && matches!(name.to_str(), Some("book" | "evidence")));
                if !excluded {
                    stack.push(path);
                }
            } else if kind.is_file() {
                files.push(path);
            }
        }
    }
    files.sort();

    let mut references = Vec::new();
    for (completed, file) in files.iter().enumerate() {
        progress(InspectionProgress {
            stage: "reference source files",
            completed,
            total: Some(files.len()),
        });
        let content = match fs::read_to_string(file) {
            Ok(content) => content,
            Err(error) if error.kind() == io::ErrorKind::InvalidData => continue,
            Err(error) => {
                errors.push(format!(
                    "{}: cannot read reference source: {error}",
                    display_path(root, file)
                ));
                continue;
            }
        };
        if content.contains('\0') {
            continue;
        }
        let markdown = file.extension().is_some_and(|extension| extension == "md");
        for role in reference_roles(&content, markdown) {
            let line = content[..role.start]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1;
            references.push(SourceReference {
                name: role.name.to_owned(),
                kind: role.kind,
                file: file.clone(),
                line,
            });
        }
    }
    progress(InspectionProgress {
        stage: "reference source files",
        completed: files.len(),
        total: Some(files.len()),
    });
    references
}

// {impl}`requirements.references.evidence_coverage`
fn evidence_input_ranges(
    root: &Path,
    path: &Path,
    definitions: &BTreeMap<String, Declaration>,
    outlines: &OutlineCache,
) -> Vec<InputRange> {
    let Some(inputs) = fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str::<Value>(&content).ok())
        .and_then(|value| value.get("inputs").and_then(Value::as_array).cloned())
    else {
        return Vec::new();
    };
    inputs
        .iter()
        .filter_map(InputSelector::from_input)
        .filter_map(|selector| selector.resolve(root, definitions, outlines).ok())
        .flat_map(|resolved| {
            let node = resolved.node;
            resolved
                .ranges
                .into_iter()
                .filter_map(|range| {
                    Some(InputRange {
                        file: fs::canonicalize(root.join(range.file)).ok()?,
                        line_start: range.line_start,
                        line_end: range.line_end,
                        node: node.clone(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::{Value, json};
    use tempfile::TempDir;

    use super::validate_repository;

    /// The recorded outline input for `item` in `file`.
    // {test}`requirements.ci.outline_inputs`
    fn outline(root: &std::path::Path, file: &str, item: &str) -> Value {
        crate::inputs::outline_input(root, file, item).unwrap()
    }

    // {test}`requirements.integration.report_api`
    fn fixture() -> TempDir {
        let temp = TempDir::new().unwrap();
        fs::create_dir_all(temp.path().join("md/reqs/foo")).unwrap();
        fs::create_dir_all(temp.path().join("evidence")).unwrap();
        fs::create_dir_all(temp.path().join("src")).unwrap();
        fs::write(temp.path().join("book.toml"), "[book]\nsrc = \"md\"\n").unwrap();
        fs::write(
            temp.path().join("src/main.rs"),
            "// first line\n// second line\n",
        )
        .unwrap();
        temp
    }

    // {test}`requirements.authoring.progress`
    // {test}`requirements.integration.report_api`
    // {test}`requirements.ci.errors`
    #[test]
    fn inspection_progress_preserves_validation_and_finishes_each_phase() {
        let temp = fixture();
        let root = temp.path();
        fs::write(
            root.join("md/requirements.md"),
            ":::{req} ready\nThe check MUST pass.\n:::\n",
        )
        .unwrap();
        fs::write(
            root.join("evidence/requirements.ready.json"),
            json!({
                "requirement": "The check MUST pass.",
                "determination": true,
                "rationale": "The inspected file has the expected lines.",
                "inputs": [outline(root, "src/main.rs", "")]
            })
            .to_string(),
        )
        .unwrap();

        for invalid in [false, true] {
            if invalid {
                fs::write(root.join("evidence/broken.json"), "{").unwrap();
                fs::write(root.join("src/broken.rs"), "fn {").unwrap();
                fs::write(root.join("src/lib.rs"), "fn retained() {}\n").unwrap();
                fs::write(root.join("src/binary.dat"), [0xff]).unwrap();
            }
            let expected = super::inspect_repository(root).unwrap();
            let mut updates = Vec::new();
            let inspected =
                super::inspect_repository_with_progress(root, |update| updates.push(update))
                    .unwrap();
            assert_eq!(inspected.report.requirements, expected.report.requirements);
            assert_eq!(inspected.report.examples, expected.report.examples);
            assert_eq!(
                inspected.report.evidence_files,
                expected.report.evidence_files
            );
            assert_eq!(inspected.report.errors, expected.report.errors);
            assert_eq!(inspected.discovery_errors, expected.discovery_errors);
            assert_eq!(
                inspected.definitions.keys().collect::<Vec<_>>(),
                expected.definitions.keys().collect::<Vec<_>>()
            );
            assert_eq!(inspected.report.requirements, 1);
            assert_eq!(inspected.report.examples, 0);
            if invalid {
                let errors = inspected.report.errors.join("\n");
                for error in [
                    "invalid JSON",
                    "cannot parse Rust source",
                    "src/lib.rs:1: function retained has no requirement or example citation",
                ] {
                    assert!(errors.contains(error), "missing {error:?}: {errors}");
                }
                let discovery = inspected.discovery_errors.join("\n");
                assert!(
                    discovery.contains("src/broken.rs")
                        && discovery.contains("cannot parse Rust source"),
                    "{discovery}"
                );
                assert!(!discovery.contains("retained"), "{discovery}");
            } else {
                assert!(inspected.report.errors.is_empty());
            }
            assert!(updates.iter().any(|update| update.total.is_none()));
            assert!(updates.iter().all(|update| {
                update
                    .total
                    .map_or(update.completed == 0, |total| update.completed <= total)
            }));
            let expected_counts = [
                ("evidence files", if invalid { 2 } else { 1 }),
                ("reference source files", if invalid { 6 } else { 3 }),
                ("reference coverage", 1),
            ];
            for (stage, total) in expected_counts {
                let completed: Vec<_> = updates
                    .iter()
                    .filter(|update| update.stage == stage && update.total.is_some())
                    .map(|update| {
                        assert_eq!(update.total, Some(total), "{stage}");
                        update.completed
                    })
                    .collect();
                assert_eq!(completed, (0..=total).collect::<Vec<_>>(), "{stage}");
            }
        }
    }

    // {test}`requirements.authoring.progress`
    #[test]
    fn inspection_progress_reports_empty_phases_as_complete() {
        let temp = TempDir::new().unwrap();
        fs::create_dir(temp.path().join("md")).unwrap();
        fs::write(temp.path().join("book.toml"), "[book]\nsrc = 'md'\n").unwrap();
        let mut updates = Vec::new();
        let inspected =
            super::inspect_repository_with_progress(temp.path(), |update| updates.push(update))
                .unwrap();
        assert!(inspected.report.errors.is_empty());
        for stage in ["evidence files", "reference coverage"] {
            assert!(
                updates.contains(&super::InspectionProgress {
                    stage,
                    completed: 0,
                    total: Some(0),
                }),
                "{stage}: {updates:?}"
            );
        }
    }

    // {test}`requirements.ci.book_source`
    #[test]
    fn book_source_defaults_only_when_absent_and_reports_invalid_configuration() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let path = root.join("book.toml");
        for (configuration, expected) in [
            ("", "src"),
            ("[book]\ntitle = 'Example'\n", "src"),
            ("[book]\nsrc = 'docs'\n", "docs"),
            ("book = { src = 'docs' }\n", "docs"),
        ] {
            fs::write(&path, configuration).unwrap();
            let mut errors = Vec::new();
            assert_eq!(
                super::book_source_dir(root, &mut errors),
                root.join(expected)
            );
            assert!(errors.is_empty(), "{errors:?}");
        }
        fs::remove_file(&path).unwrap();
        let mut errors = Vec::new();
        super::book_source_dir(root, &mut errors);
        assert!(
            errors
                .iter()
                .any(|error| error.contains("book.toml: cannot read book configuration"))
        );
        for (configuration, expected) in [
            ("[book", "invalid TOML"),
            ("book = 'wrong'", "book must be a table"),
            ("[book]\nsrc = 7", "book.src must be a string"),
            ("[book]\nsrc = false", "book.src must be a string"),
            ("[book]\nsrc = []", "book.src must be a string"),
            ("[book.src]", "book.src must be a string"),
            ("[book]\nsrc = ''", "book.src must be a directory"),
            ("[book]\nsrc = '/outside'", "book.src must be a directory"),
            ("[book]\nsrc = '../outside'", "book.src must be a directory"),
            (
                "[book]\nsrc = 'md/../outside'",
                "book.src must be a directory",
            ),
        ] {
            fs::write(&path, configuration).unwrap();
            let mut errors = Vec::new();
            super::book_source_dir(root, &mut errors);
            assert!(
                errors.iter().any(|error| error.contains(expected)),
                "{configuration:?}: {errors:?}"
            );
        }
    }

    // {test}`requirements.integration.report_api`
    // {test}`requirements.ci.book_source`
    // {test}`requirements.ci.coverage`
    // {test}`requirements.ci.requirement_text`
    // {test}`requirements.ci.outline_inputs`
    // {test}`requirements.ci.identifiers`
    // {test}`requirements.ci.span`
    #[test]
    fn validates_full_names_requirement_text_and_input_lines() {
        let temp = fixture();
        fs::write(
            temp.path().join("md/reqs/foo/bar.md"),
            "# Requirements\n\n:::{req} ready\nThe check MUST pass.\n\n```text\n{req}`example`\n```\n:::\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("evidence/foo.bar.ready.json"),
            json!({
                "requirement": "The check MUST pass.\n\n```text\n{req}`example`\n```",
                "determination": true,
                "rationale": "The inspected file has the expected lines.",
                "inputs": [outline(temp.path(), "src/main.rs", "")]
            })
            .to_string(),
        )
        .unwrap();

        let report = validate_repository(temp.path()).unwrap();
        assert_eq!(report.requirements, 1);
        assert_eq!(report.evidence_files, 1);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
    }

    // {test}`requirements.integration.report_api`
    // {test}`requirements.ci.errors`
    // {test}`requirements.ci.diagnostics`
    // {test}`requirements.ci.input_containment`
    // {test}`requirements.ci.definition_errors`
    // {test}`requirements.ci.orphans`
    // {test}`requirements.ci.coverage`
    // {test}`requirements.ci.determination`
    // {test}`requirements.ci.outline_inputs`
    // {test}`requirements.ci.requirement_text`
    // {test}`requirements.ci.schema`
    #[test]
    fn collects_missing_orphan_format_false_and_changed_line_errors() {
        let temp = fixture();
        fs::write(
            temp.path().join("md/requirements.md"),
            "# Requirements\n\n:::{req} one\nFirst.\n:::\n\n:::{req} two\nSecond.\n:::\n\n:::{req} three\nThird.\n:::\n\n:::{req} one\nDuplicate.\n:::\n\n:::{req} empty\n  \n:::\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("evidence/requirements.one.json"),
            json!({
                "requirement": "Old text.",
                "determination": false,
                "rationale": "Work remains.",
                "inputs": [
                    {
                        "file": "src/main.rs",
                        "item": "",
                        "hash": "0".repeat(64)
                    },
                    {
                        "file": "",
                        "item": 0,
                        "hash": "ABC"
                    },
                    {
                        "file": "src/main.rs",
                        "item": "fn[missing]",
                        "hash": "0".repeat(64)
                    },
                    {
                        "file": "src/main.rs",
                        "line_start": 1,
                        "line_end": 1,
                        "text": ["// first line"]
                    },
                    {
                        "requirements": "requirements.two",
                        "matches": ["requirements.two"],
                        "text": [1]
                    }
                ]
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            temp.path().join("evidence/requirements.three.json"),
            json!({"requirement": 3, "inputs": {}}).to_string(),
        )
        .unwrap();
        fs::write(
            temp.path().join("evidence/ghost.json"),
            json!({
                "requirement": "Ghost.",
                "determination": true,
                "rationale": "A valid but orphaned file.",
                "inputs": []
            })
            .to_string(),
        )
        .unwrap();
        fs::write(temp.path().join("evidence/broken.json"), "{").unwrap();

        let report = validate_repository(temp.path()).unwrap();
        let errors = report.errors.join("\n");
        for detail in [
            "no matching requirement",
            "invalid JSON",
            "missing for requirement",
            "requirement text changed",
            "- Old text.",
            "+ First.",
            "determination is false",
            "inputs[0]: outline input changed: src/main.rs <root> (now lines 1-2) no longer has the recorded hash",
            "inputs[1]: file must be a nonempty path",
            "inputs[1]: item must be a string",
            "inputs[1]: hash must be a lowercase hexadecimal SHA-256 digest",
            "inputs[2]: src/main.rs: no outline node matches \"fn[missing]\"",
            "inputs[3]: unexpected field \"line_start\"",
            "inputs[3]: missing field \"item\"",
            "inputs[4]: text[0] must be a string",
            "requirement must be a string",
            "missing field \"determination\"",
            "inputs must be an array",
            "duplicate requirement requirements.one; first declared at md/requirements.md:3",
            "requirement requirements.empty has no text",
        ] {
            assert!(errors.contains(detail), "missing {detail:?} in:\n{errors}");
        }
    }

    // {test}`requirements.references.evidence_coverage`
    // {test}`requirements.references.markdown_literals`
    #[test]
    fn source_references_are_covered_by_evidence_inputs() {
        let temp = fixture();
        fs::write(
            temp.path().join("md/reqs/foo/bar.md"),
            "# Requirements\n\n:::{req} ready\nReady.\n:::\n",
        )
        .unwrap();
        let implementation = ["{", "impl}`foo.bar.ready`"].concat();
        let citation = ["{", "cite}`foo.bar.ready`"].concat();
        let test = ["{", "test}`foo.bar.ready`"].concat();
        let example = ["{", "cite}`missing.example`"].concat();
        let source = format!("// {implementation}\n// {citation}\n");
        let notes = format!("# Notes\n\n{test}\n\n`{example}`\n\n```text\n{example}\n```\n");
        fs::write(temp.path().join("src/main.rs"), &source).unwrap();
        fs::write(temp.path().join("md/notes.md"), &notes).unwrap();
        fs::write(
            temp.path().join("evidence/foo.bar.ready.json"),
            json!({
                "requirement": "Ready.",
                "determination": true,
                "rationale": "The source and notes were examined.",
                "inputs": [
                    outline(temp.path(), "src/main.rs", ""),
                    crate::inputs::outline_inputs_for_lines(temp.path(), "md/notes.md", 3, 3)
                        .unwrap()[0]
                ]
            })
            .to_string(),
        )
        .unwrap();

        let report = validate_repository(temp.path()).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
    }

    // {test}`requirements.references.targets`
    // {test}`requirements.references.evidence_coverage`
    #[test]
    fn reports_unknown_references_and_uncovered_lines() {
        let temp = fixture();
        fs::write(
            temp.path().join("md/reqs/foo/bar.md"),
            "# Requirements\n\n:::{req} ready\nReady.\n:::\n",
        )
        .unwrap();
        let known = ["{", "test}`foo.bar.ready`"].concat();
        let unknown = ["{", "impl}`missing.requirement`"].concat();
        let source = format!("// {known}\n// {known}\n// {unknown}\n");
        fs::write(temp.path().join("src/notes.txt"), &source).unwrap();
        // Identical plain-text lines are interchangeable, so this selects the
        // first line only.
        let first = crate::inputs::outline_inputs_for_lines(temp.path(), "src/notes.txt", 1, 1)
            .unwrap()
            .remove(0);
        fs::write(
            temp.path().join("evidence/foo.bar.ready.json"),
            json!({
                "requirement": "Ready.",
                "determination": true,
                "rationale": "Only the first line was examined.",
                "inputs": [first]
            })
            .to_string(),
        )
        .unwrap();

        let report = validate_repository(temp.path()).unwrap();
        let errors = report.errors.join("\n");
        assert_eq!(report.errors.len(), 2, "{errors}");
        assert!(
            errors.contains(&format!(
                "src/notes.txt:3: {unknown} has no matching requirement"
            )),
            "{errors}"
        );
        assert!(
            errors.contains(&format!("no input covers {known} at src/notes.txt:2")),
            "{errors}"
        );
    }

    // {test}`requirements.ci.identifiers`
    #[test]
    fn invalid_full_names_from_chapter_paths_are_reported_and_not_defined() {
        let temp = fixture();
        fs::create_dir_all(temp.path().join("md/bad dir")).unwrap();
        fs::write(
            temp.path().join("md/bad dir/rules.md"),
            "# Rules\n\n:::{req} good\nGood.\n:::\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("md/reqs/index.md"),
            "# Top\n\n:::{req} top\nTop.\n:::\n",
        )
        .unwrap();
        let report = validate_repository(temp.path()).unwrap();
        let errors = report.errors.join("\n");
        assert!(
            errors.contains("md/bad dir/rules.md:3: invalid requirement name \"bad dir.rules.good\""),
            "{errors}"
        );
        // The invalid name is not defined, so no evidence is expected for it,
        // while `reqs/index.md` contributes an empty namespace.
        assert!(!errors.contains("evidence/bad dir"), "{errors}");
        assert!(
            errors.contains("evidence/top.json: missing for requirement at md/reqs/index.md:3"),
            "{errors}"
        );
        assert_eq!(report.errors.len(), 2, "{errors}");
    }

    // {test}`requirements.references.targets`
    #[test]
    fn reference_discovery_skips_evidence_snapshots_and_build_output() {
        let temp = fixture();
        let reference = ["{", "impl}`foo.bar.ready`"].concat();
        let line = format!("// {reference}\n");
        for file in [
            "src/counted.rs",
            "nested/book/counted.txt",
            "nested/evidence/counted.txt",
            "evidence/copy.json",
            "book/chapter.html",
            "target/debug/out.rs",
            "crates/sub/target/out.rs",
            ".git/info/notes",
        ] {
            let path = temp.path().join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, &line).unwrap();
        }
        fs::write(temp.path().join("src/binary.dat"), b"\0\0{impl}").unwrap();
        let mut errors = Vec::new();
        let mut found = super::collect_references(temp.path(), &mut errors)
            .into_iter()
            .map(|reference| {
                let file = reference.file.strip_prefix(temp.path()).unwrap();
                format!("{}:{}", file.display(), reference.line)
            })
            .collect::<Vec<_>>();
        found.sort();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            found,
            [
                "nested/book/counted.txt:1",
                "nested/evidence/counted.txt:1",
                "src/counted.rs:1",
            ]
        );
    }

    // {test}`requirements.ci.span`
    // {test}`requirements.ci.fence_lengths`
    #[test]
    fn colon_requirement_span_includes_markdown_and_excludes_following_text() {
        let temp = fixture();
        let first = "First MUST work.\n\n```text\n# Example heading\n:::\n```\n\n## New section\n\nSetext section\n--------------";
        fs::write(
            temp.path().join("md/requirements.md"),
            format!(
                "# Requirements\n\n:::{{req}} first\n{first}\n:::\n\nOutside the requirement.\n\n:::{{req}} second\nSecond MUST work.\n:::\n"
            ),
        )
        .unwrap();
        for (name, requirement) in [("first", first), ("second", "Second MUST work.")] {
            fs::write(
                temp.path()
                    .join(format!("evidence/requirements.{name}.json")),
                json!({
                    "requirement": requirement,
                    "determination": true,
                    "rationale": "The fixture records the current requirement body.",
                    "inputs": []
                })
                .to_string(),
            )
            .unwrap();
        }

        let report = validate_repository(temp.path()).unwrap();
        assert_eq!(report.requirements, 2);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
    }

    // {test}`requirements.ci.span`
    // {test}`requirements.ci.definition_errors`
    #[test]
    fn reports_a_missing_closing_colon_fence() {
        let temp = fixture();
        fs::write(
            temp.path().join("md/requirements.md"),
            ":::{req} ready\nReady.\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("evidence/requirements.ready.json"),
            json!({
                "requirement": "Ready.",
                "determination": true,
                "rationale": "The current body was examined.",
                "inputs": []
            })
            .to_string(),
        )
        .unwrap();

        let report = validate_repository(temp.path()).unwrap();
        assert_eq!(report.requirements, 1);
        assert!(
            report.errors.join("\n").contains(
                "md/requirements.md:1: requirement requirements.ready is missing a closing colon fence"
            ),
            "{:?}",
            report.errors
        );
    }

    // {test}`requirements.ci.fence_lengths`
    #[test]
    fn validates_four_or_more_colon_fences_and_longer_closers() {
        let body = "Body MUST remain.\n\n:::{note}\nNested.\n:::\n\nStill in the requirement.";
        for (opening, closing) in [("::::", "::::"), ("::::", ":::::"), (":::::::", "::::::::")] {
            let temp = fixture();
            fs::write(
                temp.path().join("md/requirements.md"),
                format!(
                    "# Requirements\n\n{opening}{{req}} ready\n{body}\n{closing}\n\nOutside the requirement.\n"
                ),
            )
            .unwrap();
            fs::write(
                temp.path().join("evidence/requirements.ready.json"),
                json!({
                    "requirement": body,
                    "determination": true,
                    "rationale": "The entire fenced body was examined.",
                    "inputs": []
                })
                .to_string(),
            )
            .unwrap();

            let report = validate_repository(temp.path()).unwrap();
            assert_eq!(report.requirements, 1, "{opening} ... {closing}");
            assert!(
                report.errors.is_empty(),
                "{opening} ... {closing}: {:?}",
                report.errors
            );
        }
    }

    // {test}`requirements.examples.definition`
    // {test}`requirements.examples.evidence`
    // {test}`requirements.ci.schema`
    // {test}`requirements.references.evidence_coverage`
    // {test}`requirements.ci.requirements_inputs`
    #[test]
    fn cited_example_with_a_test_and_matching_evidence_validates() {
        let temp = fixture();
        let chapter = "# Rules\n\n:::{req} ready\nReady.\n:::\n\n:::{example} case\nGiven ready, it succeeds. CITE`rules.ready`\n:::\n"
            .replace("CITE", "{cite}");
        fs::write(temp.path().join("md/rules.md"), &chapter).unwrap();
        let test_source = "// TEST`rules.case`\n#[test]\nfn checks_case() { assert!(true); }\n"
            .replace("TEST", "{test}");
        fs::write(temp.path().join("src/main.rs"), &test_source).unwrap();
        fs::write(
            temp.path().join("evidence/rules.ready.json"),
            json!({
                "requirement": "Ready.",
                "determination": true,
                "rationale": "The example cites this rule.",
                "inputs": crate::inputs::outline_inputs_for_lines(temp.path(), "md/rules.md", 8, 8)
                    .unwrap()
            })
            .to_string(),
        )
        .unwrap();
        let example_path = temp.path().join("evidence/rules.case.json");
        fs::write(
            &example_path,
            json!({
                "example": "Given ready, it succeeds. CITE`rules.ready`".replace("CITE", "{cite}"),
                "determination": true,
                "rationale": "checks_case sets up ready and asserts the result.",
                "inputs": [
                    outline(temp.path(), "src/main.rs", "fn[checks_case]"),
                    {
                        "requirements": "rules.*",
                        "matches": ["rules.case", "rules.ready"],
                        "text": [
                            chapter.lines().nth(6).unwrap(),
                            chapter.lines().nth(7).unwrap(),
                            chapter.lines().nth(8).unwrap(),
                            "",
                            chapter.lines().nth(2).unwrap(),
                            chapter.lines().nth(3).unwrap(),
                            chapter.lines().nth(4).unwrap()
                        ]
                    }
                ]
            })
            .to_string(),
        )
        .unwrap();

        let report = validate_repository(temp.path()).unwrap();
        assert_eq!(report.requirements, 1);
        assert_eq!(report.examples, 1);
        assert_eq!(report.evidence_files, 2);
        assert!(report.errors.is_empty(), "{:?}", report.errors);

        let old = fs::read_to_string(&example_path).unwrap();
        let mut uncovered: serde_json::Value = serde_json::from_str(&old).unwrap();
        uncovered["inputs"] = json!([]);
        fs::write(&example_path, uncovered.to_string()).unwrap();
        let errors = validate_repository(temp.path()).unwrap().errors.join("\n");
        assert!(errors.contains("no input covers"), "{errors}");
        assert!(errors.contains("src/main.rs:1"), "{errors}");
        fs::write(&example_path, &old).unwrap();
        fs::write(
            temp.path().join("src/main.rs"),
            test_source.replace("assert!(true)", "assert!(false)"),
        )
        .unwrap();
        let errors = validate_repository(temp.path()).unwrap().errors.join("\n");
        assert!(
            errors.contains(
                "outline input changed: src/main.rs fn[checks_case] (now lines 1-3) no longer has the recorded hash"
            ),
            "{errors}"
        );
        fs::write(temp.path().join("src/main.rs"), &test_source).unwrap();
        fs::write(
            temp.path().join("md/rules.md"),
            format!("{chapter}\n:::{req} added\nAdded.\n:::\n", req = "{req}"),
        )
        .unwrap();
        let errors = validate_repository(temp.path()).unwrap().errors.join("\n");
        assert!(
            errors.contains("requirements glob matches changed"),
            "{errors}"
        );

        let mut evidence: serde_json::Value = serde_json::from_str(&old).unwrap();
        evidence["example"] = "Outdated.".into();
        fs::write(&example_path, evidence.to_string()).unwrap();
        let errors = validate_repository(temp.path()).unwrap().errors.join("\n");
        assert!(errors.contains("example text changed"), "{errors}");
        assert!(errors.contains("- Outdated."), "{errors}");

        let mut wrong_field: serde_json::Value = serde_json::from_str(&old).unwrap();
        let example_text = wrong_field
            .as_object_mut()
            .unwrap()
            .remove("example")
            .unwrap();
        wrong_field["requirement"] = example_text;
        fs::write(&example_path, wrong_field.to_string()).unwrap();
        let errors = validate_repository(temp.path()).unwrap().errors.join("\n");
        assert!(errors.contains("missing field \"example\""), "{errors}");
        assert!(
            errors.contains("unexpected field \"requirement\""),
            "{errors}"
        );

        fs::remove_file(&example_path).unwrap();
        let errors = validate_repository(temp.path()).unwrap().errors.join("\n");
        assert!(errors.contains("missing for example"), "{errors}");
    }

    // {test}`requirements.references.evidence_coverage`
    // {test}`requirements.references.targets`
    #[test]
    fn uncovered_cite_is_not_reported_while_uncovered_impl_and_test_are() {
        let temp = fixture();
        fs::write(
            temp.path().join("md/rules.md"),
            ":::{req} ready\nReady.\n:::\n",
        )
        .unwrap();
        let source = "// CITE`rules.ready`\nfn helper() {}\n// IMPL`rules.ready`\nfn run() {}\n// TEST`rules.ready`\nfn check() {}\n"
            .replace("CITE", "{cite}")
            .replace("IMPL", "{impl}")
            .replace("TEST", "{test}");
        fs::write(temp.path().join("src/main.rs"), &source).unwrap();
        let record = |inputs: serde_json::Value| {
            json!({
                "requirement": "Ready.",
                "determination": true,
                "rationale": "The implementation and test were examined.",
                "inputs": inputs
            })
            .to_string()
        };
        let path = temp.path().join("evidence/rules.ready.json");
        fs::write(&path, record(json!([]))).unwrap();
        let errors = validate_repository(temp.path()).unwrap().errors;
        let uncovered: Vec<_> = errors
            .iter()
            .filter(|error| error.contains("no input covers"))
            .collect();
        assert_eq!(uncovered.len(), 2, "{errors:#?}");
        assert!(
            uncovered
                .iter()
                .any(|error| error.contains("impl") && error.contains("src/main.rs:3")),
            "{errors:#?}"
        );
        assert!(
            uncovered
                .iter()
                .any(|error| error.contains("test") && error.contains("src/main.rs:5")),
            "{errors:#?}"
        );
        assert!(
            !errors.iter().any(|error| error.contains("src/main.rs:1")),
            "{errors:#?}"
        );

        fs::write(
            &path,
            record(json!([
                outline(temp.path(), "src/main.rs", "fn[run]"),
                outline(temp.path(), "src/main.rs", "fn[check]")
            ])),
        )
        .unwrap();
        let report = validate_repository(temp.path()).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
    }

    // {test}`requirements.ci.definition_errors`
    // {test}`requirements.examples.definition`
    // {test}`requirements.examples.evidence`
    #[test]
    fn example_errors_include_duplicates_invalid_bodies_and_missing_tests() {
        let temp = fixture();
        let chapter = ":::{req} repeated\nReady.\n:::\n\n:::{req} repeated\nAgain.\n:::\n\n:::{example} repeated\nGiven ready. CITE`rules.repeated`\n:::\n\n:::{example} case\nGiven ready. CITE`rules.repeated`\n:::\n\n:::{example} case\nAgain. CITE`rules.repeated`\n:::\n\n:::{example} empty\n \n:::\n\n:::{example} unclosed\nNo close."
            .replace("CITE", "{cite}");
        fs::write(temp.path().join("md/rules.md"), chapter).unwrap();
        fs::write(
            temp.path().join("evidence/rules.case.json"),
            json!({
                "example": "Given ready. CITE`rules.repeated`".replace("CITE", "{cite}"),
                "determination": true,
                "rationale": "No test has been cited yet.",
                "inputs": []
            })
            .to_string(),
        )
        .unwrap();
        let errors = validate_repository(temp.path()).unwrap().errors.join("\n");
        for detail in [
            "duplicate requirement rules.repeated; first declared at md/rules.md:1",
            "duplicate example rules.repeated; first declared at md/rules.md:1",
            "duplicate example rules.case; first declared at md/rules.md:13",
            "example rules.empty has no text",
            "example rules.empty must reference at least one requirement with {cite} or {test}",
            "example rules.unclosed is missing a closing colon fence",
            "example rules.case has a true determination but no {test} reference",
        ] {
            assert!(errors.contains(detail), "missing {detail:?} in:\n{errors}");
        }
    }

    // {test}`requirements.examples.definition`
    #[test]
    fn examples_may_reference_their_requirement_with_cite_or_test() {
        let temp = fixture();
        let chapter = ":::{req} ready\nReady.\n:::\n\n:::{example} tested\nGiven ready. TEST`rules.ready`\n:::\n\n:::{example} cited\nGiven ready. CITE`rules.ready`\n:::\n\n:::{example} nested\nLike the tested case. CITE`rules.tested`\n:::\n"
            .replace("CITE", "{cite}")
            .replace("TEST", "{test}");
        fs::write(temp.path().join("md/rules.md"), chapter).unwrap();
        let errors = validate_repository(temp.path()).unwrap().errors.join("\n");
        assert!(
            !errors.contains("example rules.tested must reference"),
            "{errors}"
        );
        assert!(
            !errors.contains("example rules.cited must reference"),
            "{errors}"
        );
        assert!(
            errors.contains(
                "example rules.nested must reference at least one requirement with {cite} or {test}"
            ),
            "{errors}"
        );
    }

    // {test}`requirements.references.anchors`
    // {test}`requirements.ci.requirement_text`
    #[test]
    fn anchored_files_must_be_completely_covered_and_resolvable() {
        let temp = fixture();
        let root = temp.path();
        fs::create_dir_all(root.join("skills")).unwrap();
        fs::write(root.join("skills/ready.md"), "# Skill\nUse ready.\n").unwrap();
        fs::write(
            root.join("md/requirements.md"),
            ":::{req} ready\n:impl: skills/ready.md\n:test: missing.sh\nReady MUST hold.\n:::\n\n:::{example} case\n:test: skills/ready.md\nA ready case. CITE`requirements.ready`\n:::\n".replace("CITE", "{cite}"),
        )
        .unwrap();
        // The root node is the whole file; its heading's section is not.
        let skill = |item: &str| outline(root, "skills/ready.md", item);
        let write = |name: &str, field: &str, text: &str, inputs: Value| {
            fs::write(
                root.join(format!("evidence/{name}.json")),
                json!({
                    field: text,
                    "determination": true,
                    "rationale": "Checked.",
                    "inputs": inputs,
                })
                .to_string(),
            )
            .unwrap();
        };
        // The option lines are not part of the definition text.
        write(
            "requirements.ready",
            "requirement",
            "Ready MUST hold.",
            json!([skill("section[Skill]")]),
        );
        write(
            "requirements.case",
            "example",
            &"A ready case. CITE`requirements.ready`".replace("CITE", "{cite}"),
            json!([skill("")]),
        );
        let errors = validate_repository(root).unwrap().errors.join("\n");
        assert!(!errors.contains("text changed"), "{errors}");
        assert!(
            errors.contains("evidence/requirements.ready.json: no input selects skills/ready.md (lines 1-2) anchored by :impl: on requirement requirements.ready"),
            "{errors}"
        );
        assert!(
            errors.contains("evidence/requirements.ready.json: :test: anchor missing.sh on requirement requirements.ready at md/requirements.md:1 does not resolve"),
            "{errors}"
        );
        assert!(!errors.contains("requirements.case.json"), "{errors}");

        fs::write(
            root.join("md/requirements.md"),
            ":::{req} ready\n:impl: skills/ready.md\nReady MUST hold.\n:::\n\n:::{example} case\n:test: skills/ready.md\nA ready case. CITE`requirements.ready`\n:::\n".replace("CITE", "{cite}"),
        )
        .unwrap();
        write(
            "requirements.ready",
            "requirement",
            "Ready MUST hold.",
            json!([skill("")]),
        );
        let report = validate_repository(root).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);

        // Changing the anchored file makes the evidence stale.
        fs::write(root.join("skills/ready.md"), "# Skill\nUse ready now.\n").unwrap();
        let errors = validate_repository(root).unwrap().errors.join("\n");
        assert!(
            errors.contains("evidence/requirements.ready.json: inputs[0]: outline input changed: skills/ready.md <root> (now lines 1-2) no longer has the recorded hash"),
            "{errors}"
        );
    }

    // {test}`requirements.references.anchors`
    // {test}`requirements.ci.outline_inputs`
    #[test]
    fn selector_anchors_are_covered_by_inputs_selecting_the_node_or_an_ancestor() {
        let temp = fixture();
        let root = temp.path();
        fs::write(
            root.join("src/lib.rs"),
            "mod inner {\n    fn run() {}\n    fn other() {}\n}\n",
        )
        .unwrap();
        fs::write(
            root.join("md/requirements.md"),
            ":::{req} ready\n:impl: src/lib.rs#mod[inner]/fn[run]\nReady MUST hold.\n:::\n",
        )
        .unwrap();
        let write = |inputs: Value| {
            fs::write(
                root.join("evidence/requirements.ready.json"),
                json!({
                    "requirement": "Ready MUST hold.",
                    "determination": true,
                    "rationale": "Checked.",
                    "inputs": inputs,
                })
                .to_string(),
            )
            .unwrap();
        };
        let anchor_errors = || {
            validate_repository(root)
                .unwrap()
                .errors
                .into_iter()
                .filter(|error| error.contains("anchor"))
                .collect::<Vec<_>>()
        };
        write(json!([outline(root, "src/lib.rs", "mod[inner]/fn[other]")]));
        let errors = anchor_errors();
        assert_eq!(
            errors,
            [
                "evidence/requirements.ready.json: no input selects src/lib.rs#mod[inner]/fn[run] (lines 2-2) anchored by :impl: on requirement requirements.ready"
            ],
        );
        for item in ["mod[inner]/fn[run]", "mod[inner]", ""] {
            write(json!([outline(root, "src/lib.rs", item)]));
            assert!(anchor_errors().is_empty(), "{item}: {:?}", anchor_errors());
        }
        fs::write(
            root.join("md/requirements.md"),
            ":::{req} ready\n:impl: src/lib.rs#fn[missing]\nReady MUST hold.\n:::\n",
        )
        .unwrap();
        let errors = anchor_errors().join("\n");
        assert!(
            errors.contains(":impl: anchor src/lib.rs#fn[missing] on requirement requirements.ready at md/requirements.md:1 does not resolve to a readable, permitted file or node: src/lib.rs: no outline node matches \"fn[missing]\""),
            "{errors}"
        );
    }

    // {test}`requirements.references.anchors`
    #[test]
    fn unsupported_and_empty_options_are_validation_errors() {
        let temp = fixture();
        fs::write(
            temp.path().join("md/requirements.md"),
            ":::{req} ready\n:phrase: ready\n:impl:\nReady MUST hold.\n:::\n\n:::{example} case\n:impl: src/main.rs\nA case. CITE`requirements.ready`\n:::\n".replace("CITE", "{cite}"),
        )
        .unwrap();
        let errors = validate_repository(temp.path()).unwrap().errors.join("\n");
        for expected in [
            "md/requirements.md:2: requirement requirements.ready has unsupported option :phrase:",
            "md/requirements.md:3: requirement requirements.ready has an :impl: option without a file path",
            "md/requirements.md:8: example requirements.case has unsupported option :impl:",
        ] {
            assert!(errors.contains(expected), "missing {expected:?}: {errors}");
        }
        assert!(super::load_definitions(temp.path()).is_err());
    }

    // {test}`requirements.references.anchors`
    // {test}`requirements.ci.requirements_inputs`
    #[test]
    fn requirements_inputs_omit_option_lines() {
        let temp = fixture();
        fs::write(
            temp.path().join("md/requirements.md"),
            "# Rules\n\n:::{req} ready\n:impl: src/main.rs\nReady MUST hold.\n:::\n",
        )
        .unwrap();
        let definitions = super::load_definitions(temp.path()).unwrap();
        let ready = &definitions["requirements.ready"];
        assert_eq!(ready.text, "Ready MUST hold.");
        assert_eq!((ready.line_start, ready.line_end), (3, 6));
        assert_eq!(
            ready.raw_lines,
            [":::{req} ready", "Ready MUST hold.", ":::"]
        );
        assert_eq!(
            ready.anchors,
            [super::Anchor {
                kind: crate::ReferenceKind::Impl,
                file: "src/main.rs".to_owned(),
                item: String::new(),
            }]
        );
        let resolved = crate::inputs::InputSelector::Requirements {
            requirements: "requirements.ready".to_owned(),
        }
        .resolve(
            &fs::canonicalize(temp.path()).unwrap(),
            &definitions,
            &crate::inputs::OutlineCache::default(),
        )
        .unwrap();
        assert_eq!(resolved.text, ready.raw_lines);
    }
}

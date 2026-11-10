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

// :::{impl} requirements.authoring.batch
// `discovery_errors` keeps discovery, read, scan, and parse failures apart from
// evidence validation errors, so a batch reports them instead of treating a
// partial scan as an empty batch.
// :::
pub(crate) struct RepositoryInspection {
    /// The canonical project root.
    pub(crate) root: PathBuf,
    pub(crate) definitions: BTreeMap<String, Declaration>,
    pub(crate) references: Vec<SourceReference>,
    /// The sites of each distributed requirement.
    pub(crate) sites: crate::sites::Sites,
    pub(crate) report: EvidenceReport,
    pub(crate) discovery_errors: Vec<String>,
}

/// Progress within one inspection phase; discovery has no known total.
// {impl}`requirements.authoring.progress`
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
    // Any failure aborts, so this is not a validation site.
    let mut failures = Vec::new();
    let md_root = book_source_dir(&root, &mut failures);
    let definitions = collect_definitions(&root, &md_root, &mut failures);
    if failures.is_empty() {
        Ok(definitions)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            failures.join("\n"),
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
    // Citation reviews are validated against their source items below.
    // {impl}`requirements.references.citation_review`
    let (review_files, ordinary_files): (Vec<_>, Vec<_>) = evidence_files
        .iter()
        .partition(|file| crate::citations::is_review_path(&evidence_root, file));
    let review_files: Vec<PathBuf> = review_files.into_iter().cloned().collect();
    // Site reviews are validated against the sites of their requirements.
    // {impl}`requirements.references.site_review`
    // {impl}`requirements.ci.orphans`
    let (site_files, ordinary_files): (Vec<_>, Vec<_>) = ordinary_files
        .into_iter()
        .partition(|file| crate::sites::is_review_path(&evidence_root, file));
    let site_files: Vec<PathBuf> = site_files.into_iter().cloned().collect();

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
    // Missing evidence is reported as a validation error; inspection never
    // starts authoring.
    // {impl}`requirements.authoring.deterministic_validation`
    for (name, definition) in &definitions {
        if definition.kind != DefinitionKind::Term && !present.contains(name) {
            errors.push(format!(
                "evidence/{name}.json: missing for {} at {}",
                definition.kind.noun(),
                definition.source
            ));
        }
    }

    // Unreadable reference sources are discovery failures: a batch must not
    // treat a partial scan as complete.
    // {impl}`requirements.authoring.batch`
    let scanned = errors.len();
    progress(InspectionProgress {
        stage: "discovering source references",
        completed: 0,
        total: None,
    });
    let files = discovered_files(&root, &mut errors);
    let references = collect_references_with_progress(&root, &files, &mut errors, &mut progress);
    discovery_errors.extend(errors[scanned..].iter().cloned());
    validate_committed_credentials(&root, &mut errors);
    // {impl}`requirements.references.distributed`
    let sites = crate::sites::discover(&root, &definitions, &files, &outlines, &mut errors);
    validate_references(
        &root,
        &definitions,
        &references,
        &sites,
        &outlines,
        &mut errors,
        &mut progress,
    );
    // {impl}`requirements.references.citation_review`
    crate::citations::validate(
        &root,
        &definitions,
        &references,
        &outlines,
        &review_files,
        &mut errors,
        &mut progress,
    );
    // {impl}`requirements.references.site_review`
    crate::sites::validate(
        &root,
        &definitions,
        &references,
        &sites,
        &outlines,
        &site_files,
        &mut errors,
        &mut progress,
    );
    // {impl}`requirements.references.rust_item_coverage`
    crate::rust_coverage::validate_rust_functions(
        &root,
        &files,
        &definitions,
        &mut errors,
        &mut discovery_errors,
        &mut progress,
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
        root: root.clone(),
        report: EvidenceReport {
            requirements,
            examples,
            evidence_files: evidence_files.len(),
            errors,
        },
        definitions,
        references,
        sites,
        discovery_errors,
    })
}

// {impl}`requirements.ci.book_source`
// {impl}`requirements.configuration.loading`
// {impl}`requirements.ci.diagnostics`
// A malformed `book.toml` is one failed check: fall back to `md` so
// definition collection and the checks after it still run.
// {impl}`requirements.ci.errors`
pub(crate) fn book_source_dir(root: &Path, errors: &mut Vec<String>) -> PathBuf {
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
// {impl}`requirements.references.builtin_definitions`
// {impl}`requirements.examples.definition`
// {impl}`requirements.definitions.declaration`
// {impl}`requirements.references.anchors`
// {impl}`requirements.ci.requirements_inputs`
// {impl}`requirements.ci.errors`
// {impl}`requirements.ci.coverage`
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
        crate::sites::validate_query_placement(&label, &content, &blocks, errors);
        for block in &blocks {
            let line = content[..block.start]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1;
            let source = format!("{label}:{line}");
            let name = full_requirement_name(&namespace, &block.name);
            // An invalid name declares nothing, but the checks of the
            // declaration's fence, options, and text still run.
            // {impl}`requirements.references.builtin_definitions`
            let named = if name.starts_with("habeas:") || block.name.starts_with("habeas:") {
                errors.push(format!(
                    "{source}: invalid {} name {name:?}: names beginning with habeas: are reserved for built-in definitions",
                    block.kind.noun()
                ));
                false
            } else if !valid_name(&name) {
                errors.push(format!(
                    "{source}: invalid {} name {name:?}",
                    block.kind.noun()
                ));
                false
            } else {
                true
            };

            if !block.closed {
                errors.push(format!(
                    "{source}: {} {name} is missing a closing colon fence",
                    block.kind.noun()
                ));
            }
            // {impl}`requirements.ci.span`
            // {impl}`requirements.references.anchors`
            // {impl}`requirements.ci.requirement_text`
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
            if !named {
                continue;
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
            && !example_references_requirement(definition, &definitions)
        {
            errors.push(format!(
                "{}: example {name} must reference at least one requirement with {{cite}} or {{test}}",
                definition.source
            ));
        }
    }

    definitions
}

/// Whether an example's text has a valid `{cite}` or `{test}` reference to a
/// requirement. Definition collection reports an example without one, and
/// Rust item coverage counts a reference to an example only when it has one.
// {impl}`requirements.examples.definition`
// {impl}`requirements.references.rust_item_coverage`
pub(crate) fn example_references_requirement(
    example: &Declaration,
    definitions: &BTreeMap<String, Declaration>,
) -> bool {
    crate::text_references(&example.text, true)
        .into_iter()
        .any(|(kind, name)| {
            matches!(kind, ReferenceKind::Cite | ReferenceKind::Test)
                && definitions
                    .get(&name)
                    .is_some_and(|target| target.kind == DefinitionKind::Requirement)
        })
}

/// Lists every file with `extension` under `dir` at any depth. A symlink to a
/// file counts as a file; symlinked directories are not followed, so a cycle
/// cannot repeat the walk.
// {impl}`requirements.ci.coverage`
// {impl}`requirements.ci.orphans`
// {impl}`requirements.ci.errors`
pub(crate) fn files_with_extension(
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
            let is_file = kind.is_file()
                || (kind.is_symlink() && fs::metadata(&path).is_ok_and(|target| target.is_file()));
            if kind.is_dir() {
                stack.push(path);
            } else if is_file && path.extension().is_some_and(|value| value == extension) {
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
// A malformed evidence file is one failed check reporting its first parse
// problem; every well-formed record reports each failed field check.
// {impl}`requirements.ci.errors`
// {impl}`requirements.ci.outcomes`
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
        &[
            text_field,
            "determination",
            "rationale",
            "references",
            "inputs",
        ],
        &label,
        errors,
    );
    // The digest is compared with the current reference set during reference
    // validation.
    // {impl}`requirements.ci.reference_set`
    if let Some(references) = string_field(object, "references", &label, errors)
        && !is_sha256_hex(references)
    {
        errors.push(format!(
            "{label}: references must be a lowercase hexadecimal SHA-256 digest"
        ));
    }

    // {impl}`requirements.ci.requirement_text`
    if let Some(text) = string_field(object, text_field, &label, errors) {
        if text.trim().is_empty() {
            errors.push(format!("{label}: {text_field} must not be empty"));
        }
        if let Some(expected) = expected
            && text != expected.text
        {
            errors.push(format!(
                "{label}: {text_field} text changed ({}){}",
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
// {impl}`requirements.ci.errors`
pub(crate) fn validate_input(
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
        // Resolving the definition does not depend on a well-formed `text`.
        let definition = definition?;
        return validate_resolved_input(
            root,
            definitions,
            outlines,
            &InputSelector::Definition {
                definition: definition.to_owned(),
            },
            text.as_deref(),
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
            // Duplicates among the string entries are a separate failure.
            if names.iter().collect::<BTreeSet<_>>().len() != names.len() {
                errors.push(format!(
                    "{label}: matches must have distinct definition names"
                ));
            }
            (names.len() == values.len()).then_some(names)
        });
        let text = text_field(object, label, errors);
        // Resolving the selection depends only on `requirements`; each
        // comparison runs when its recorded field is well formed.
        let requirements = requirements?;
        let selector = InputSelector::Requirements {
            requirements: requirements.to_owned(),
        };
        return validate_resolved_input(
            root,
            definitions,
            outlines,
            &selector,
            text.as_deref(),
            matches.as_deref(),
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
// {impl}`requirements.ci.input_containment`
// {impl}`requirements.ci.errors`
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
        if is_sha256_hex(hash) {
            Some(hash)
        } else {
            errors.push(format!(
                "{label}: hash must be a lowercase hexadecimal SHA-256 digest"
            ));
            None
        }
    });
    // Resolution needs the file and item but not the hash.
    let (Some(file), Some(item)) = (file, item) else {
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
    if let (Some(node), Some(range), Some(hash)) = (&resolved.node, resolved.ranges.first(), hash)
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
    // Every element is checked, not only those before the first failure.
    let mut text = Vec::new();
    for (index, value) in values.iter().enumerate() {
        match value.as_str() {
            Some(line) => text.push(line.to_owned()),
            None => errors.push(format!("{label}: text[{index}] must be a string")),
        }
    }
    (text.len() == values.len()).then_some(text)
}

// {impl}`requirements.ci.requirements_inputs`
// {impl}`requirements.definitions.inputs`
// {impl}`requirements.ci.diagnostics`
// {impl}`requirements.ci.input_containment`
// {impl}`requirements.authoring.redaction`
// {impl}`requirements.ci.errors`
#[allow(clippy::too_many_arguments)]
fn validate_resolved_input(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    outlines: &OutlineCache,
    selector: &InputSelector,
    text: Option<&[String]>,
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
        let glob = match selector {
            InputSelector::Requirements { requirements } => requirements.as_str(),
            _ => "",
        };
        errors.push(format!(
            "{label}: requirements glob {glob:?} matches changed; added {missing:?}, removed {removed:?}"
        ));
    }
    if let Some(text) = text
        && text != resolved.text
    {
        // For a requirements input, name the still-matched definitions whose
        // current declaration no longer appears in the recorded text.
        let changed = matches.map_or_else(Vec::new, |matches| {
            matches
                .iter()
                .filter(|name| {
                    definitions.get(*name).is_some_and(|definition| {
                        let lines = crate::redaction::redact_lines(&definition.raw_lines);
                        resolved.matches.contains(name)
                            && !text
                                .windows(lines.len().max(1))
                                .any(|window| window == lines.as_slice())
                    })
                })
                .cloned()
                .collect::<Vec<_>>()
        });
        let old = text.iter().map(String::as_str).collect::<Vec<_>>();
        let new = resolved.text.iter().map(String::as_str).collect::<Vec<_>>();
        errors.push(format!(
            "{label}: input text changed for {selector:?}{}{}",
            if changed.is_empty() {
                String::new()
            } else {
                format!("; changed definitions {changed:?}")
            },
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
pub(crate) fn check_keys(
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
pub(crate) fn string_field<'a>(
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
pub(crate) fn array_field<'a>(
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
pub(crate) fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

// {impl}`requirements.references.roles`
// {impl}`requirements.ci.reference_set`
#[derive(Clone, Debug)]
pub(crate) struct SourceReference {
    pub(crate) name: String,
    pub(crate) kind: ReferenceKind,
    pub(crate) file: PathBuf,
    /// Line of the reference, or of a fenced reference's opening fence.
    pub(crate) line: usize,
    /// A fenced reference's body without comment delimiters; `None` for an
    /// inline reference.
    pub(crate) commentary: Option<String>,
    /// Whether a fenced reference closes within its comment run or file.
    pub(crate) closed: bool,
    /// Whether the source lies in the book's source directory.
    pub(crate) in_book: bool,
    /// Whether a fenced reference lies in the body of a requirement or
    /// example declaration.
    pub(crate) in_declaration: bool,
}

impl SourceReference {
    /// Whether a `{fixme}` can be an edge: named, fenced, and outside the
    /// book. Other placements are validation errors, and an unnamed
    /// `{fixme}` is never an edge. Every other role can be.
    // {impl}`requirements.references.roles`
    pub(crate) fn is_fixme_edge(&self) -> bool {
        self.kind != ReferenceKind::Fixme
            || (!self.name.is_empty() && self.commentary.is_some() && !self.in_book)
    }
}

/// One edge of a definition's reference set: its origin (an in-source
/// reference or a declaration anchor), role, file, and outline selector.
// {impl}`requirements.ci.reference_set`
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ReferenceSetEntry {
    origin: &'static str,
    role: &'static str,
    file: String,
    item: String,
    /// The words of a fenced reference's body, for a source outside the book.
    words: Vec<String>,
}

/// The reference-set digest of a definition: SHA-256 over a length-prefixed
/// encoding of its sorted incoming `{impl}`, `{test}`, and `{fixme}` edges.
/// A reference contributes the selector of the outline node its comment or
/// text belongs to (the root node for plain-text files), so moving it within
/// that node leaves the digest unchanged; an anchor contributes its file and
/// selector as written. `{cite}` references do not contribute. `root` must be
/// canonical, as reference discovery records canonical paths.
// {impl}`requirements.ci.reference_set`
// {impl}`requirements.references.outline_comments`
// {impl}`requirements.ci.errors`
pub(crate) fn reference_set_digest<'a>(
    root: &Path,
    outlines: &OutlineCache,
    name: &str,
    definition: &Declaration,
    references: impl IntoIterator<Item = &'a SourceReference>,
    sites: &crate::sites::Sites,
) -> String {
    use sha2::{Digest, Sha256};

    let mut entries = Vec::new();
    for reference in references {
        // A `{fixme}` inside a site of a distributed definition belongs to
        // that site's review instead.
        // {impl}`requirements.references.site_review`
        if reference.name != name
            || crate::sites::in_own_site(root, sites, reference)
            || !reference.is_fixme_edge()
            || !matches!(
                reference.kind,
                ReferenceKind::Impl | ReferenceKind::Test | ReferenceKind::Fixme
            )
        {
            continue;
        }
        let file = display_path(root, &reference.file);
        let parsed = outlines.parse(root, &file);
        let item = match parsed.as_ref() {
            Ok(parsed) if parsed.outline.profile.language.is_some() => {
                let index = parsed.outline.owner_at_line(reference.line);
                parsed.outline.nodes[index].selector.clone()
            }
            // Plain-text files have only a root node; a source that cannot be
            // outlined is reported elsewhere and contributes its whole file.
            _ => String::new(),
        };
        // Commentary in the book is part of the declaration text instead.
        let commentary = reference
            .commentary
            .as_deref()
            .filter(|_| !reference.in_book);
        entries.push(ReferenceSetEntry {
            origin: if commentary.is_some() {
                "fenced-reference"
            } else {
                "reference"
            },
            role: reference.kind.label(),
            file,
            item,
            words: commentary
                .map(|body| body.split_whitespace().map(str::to_owned).collect())
                .unwrap_or_default(),
        });
    }
    for anchor in &definition.anchors {
        entries.push(ReferenceSetEntry {
            origin: "anchor",
            role: anchor.kind.label(),
            file: anchor.file.clone(),
            item: anchor.item.clone(),
            words: Vec::new(),
        });
    }
    entries.sort();

    let mut hasher = Sha256::new();
    let mut field = |bytes: &[u8]| {
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    };
    field(&(entries.len() as u64).to_le_bytes());
    for entry in &entries {
        field(entry.origin.as_bytes());
        field(entry.role.as_bytes());
        field(entry.file.as_bytes());
        field(entry.item.as_bytes());
        // Only fenced entries carry words, so other entries encode as before.
        if entry.origin == "fenced-reference" {
            field(&(entry.words.len() as u64).to_le_bytes());
            for word in &entry.words {
                field(word.as_bytes());
            }
        }
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The current reference-set digest of a requirement or named example, as
/// recorded in its evidence's `references` field.
// {impl}`requirements.ci.reference_set`
pub fn reference_set_digest_for(root: &Path, name: &str) -> io::Result<String> {
    let root = fs::canonicalize(root)?;
    // Errors in other declarations are validation errors; they do not change
    // this definition's reference set.
    let mut ignored = Vec::new();
    let md_root = book_source_dir(&root, &mut ignored);
    let definitions = collect_definitions(&root, &md_root, &mut ignored);
    let definition = definitions
        .get(name)
        .filter(|definition| definition.kind != DefinitionKind::Term)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("{name}: no matching requirement or named example"),
            )
        })?;
    // A discovery failure aborts, so this is not a validation site.
    let mut failures = Vec::new();
    let files = discovered_files(&root, &mut failures);
    let references = collect_references_with_progress(&root, &files, &mut failures, &mut |_| {});
    if !failures.is_empty() {
        return Err(io::Error::other(failures.join("\n")));
    }
    let outlines = OutlineCache::default();
    let sites = definition_sites(&root, name, definition, &files, &outlines)?;
    Ok(reference_set_digest(
        &root,
        &outlines,
        name,
        definition,
        &references,
        &sites,
    ))
}

/// The sites of one definition, keyed as in an inspection, so a `{fixme}`
/// inside one of them can be told apart; empty unless it is distributed.
// {impl}`requirements.references.site_review`
pub(crate) fn definition_sites(
    root: &Path,
    name: &str,
    definition: &Declaration,
    files: &[PathBuf],
    outlines: &OutlineCache,
) -> io::Result<crate::sites::Sites> {
    let mut sites = crate::sites::Sites::new();
    if definition.kind == DefinitionKind::Requirement
        && crate::structural::has_query_directive(definition)
    {
        sites.insert(
            name.to_owned(),
            crate::sites::requirement_sites(root, name, definition, files, outlines)
                .map_err(io::Error::other)?,
        );
    }
    Ok(sites)
}

/// Whether a string is a lowercase hexadecimal SHA-256 digest.
// {impl}`requirements.ci.schema`
fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
// {impl}`requirements.references.discovery`
// {impl}`requirements.examples.evidence`
// {impl}`requirements.definitions.references`
// {impl}`requirements.definitions.declaration`
// {impl}`requirements.authoring.progress`
// {impl}`requirements.references.anchors`
// {impl}`requirements.ci.reference_set`
// {impl}`requirements.ci.errors`
fn validate_references(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    references: &[SourceReference],
    sites: &crate::sites::Sites,
    outlines: &OutlineCache,
    errors: &mut Vec<String>,
    progress: &mut impl FnMut(InspectionProgress),
) {
    let mut by_definition: BTreeMap<String, Vec<&SourceReference>> = BTreeMap::new();
    for reference in references {
        let location = format!("{}:{}", display_path(root, &reference.file), reference.line);
        // An unclosed fence has no complete body, so only the checks of its
        // name, which comes from the opening line, still run.
        // {impl}`requirements.references.roles`
        let unclosed = reference.commentary.is_some() && !reference.closed;
        if unclosed {
            errors.push(format!(
                "{location}: fenced {{{}}} reference to {} has no closing fence with at least as many colons within its comment run or file",
                reference.kind.label(),
                reference.name
            ));
        }
        // A `{fixme}` whose form or placement failed is not an edge, but
        // whether its name resolves is still checked.
        let edge = reference.kind != ReferenceKind::Fixme
            || validate_fixme_form(reference, &location, unclosed, errors);
        if reference.name.is_empty() {
            continue;
        }
        // {impl}`requirements.references.builtin_definitions`
        // {impl}`requirements.references.no_requirement`
        if crate::builtins::is_builtin_name(&reference.name) {
            match crate::builtins::builtin(&reference.name) {
                // Its form problems are already reported.
                Some(_) if unclosed || !edge => {}
                Some(builtin) => {
                    if let Err(problem) =
                        builtin.check(reference.kind, reference.commentary.as_deref())
                    {
                        errors.push(format!("{location}: {problem}"));
                    }
                }
                None => errors.push(format!(
                    "{location}: {{{}}}`{}` has no matching built-in definition",
                    reference.kind.label(),
                    reference.name
                )),
            }
            continue;
        }
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
            // A `{fixme}` inside one of its definition's sites acknowledges a
            // gap at that site only, so it is not part of the definition's
            // coverage or reference set.
            // {impl}`requirements.references.site_review`
            // {impl}`requirements.references.evidence_coverage`
            if edge && !unclosed && !crate::sites::in_own_site(root, sites, reference) {
                by_definition
                    .entry(reference.name.clone())
                    .or_default()
                    .push(reference);
            }
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
        if let Some(entries) = fs::read_to_string(&evidence_file)
            .ok()
            .and_then(|content| serde_json::from_str::<Value>(&content).ok())
            .and_then(|value| Judgment::from_value(value.get("determination")?).ok())
            .and_then(|judgment| match judgment {
                Judgment::Fixme(entries) => Some(entries),
                _ => None,
            })
        {
            let fixmes = references
                .iter()
                .copied()
                .filter(|reference| reference.kind == ReferenceKind::Fixme)
                .collect::<Vec<_>>();
            validate_fixme_entries(
                root,
                &display_path(root, &evidence_file),
                name,
                &entries,
                &fixmes,
                errors,
            );
        }
        // {impl}`requirements.ci.reference_set`
        if let Some(recorded) = fs::read_to_string(&evidence_file)
            .ok()
            .and_then(|content| serde_json::from_str::<Value>(&content).ok())
            .and_then(|value| {
                value
                    .get("references")
                    .and_then(Value::as_str)
                    .filter(|digest| is_sha256_hex(digest))
                    .map(str::to_owned)
            })
            && recorded
                != reference_set_digest(
                    root,
                    outlines,
                    name,
                    definition,
                    references.iter().copied(),
                    sites,
                )
        {
            errors.push(format!(
                "{}: reference set changed: {{impl}}, {{test}}, or {{fixme}} references or anchors of {} {name} were added, removed, edited, or moved to another node",
                display_path(root, &evidence_file),
                definition.kind.noun()
            ));
        }
        let inputs = evidence_input_ranges(root, &evidence_file, definitions, outlines);
        // `{cite}` points from its source to the definition, so it is not part of
        // the definition's evidence. A `{fixme}` inside a site is covered by
        // the site's review instead and was left out above.
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

/// Reports the problems of a `{fixme}`'s form and placement, returning
/// whether it is a named edge whose name and coverage still need checking.
/// An unnamed `{fixme}` is not an edge; a misplaced named one is reported
/// once rather than also as uncovered.
// {impl}`requirements.references.roles`
fn validate_fixme_form(
    reference: &SourceReference,
    location: &str,
    unclosed: bool,
    errors: &mut Vec<String>,
) -> bool {
    let name = &reference.name;
    // The form and placement checks are independent: each is reported.
    let mut edge = true;
    if reference.commentary.is_none() {
        errors.push(format!(
            "{location}: inline {{fixme}} reference to {name} must use the fenced form, with a body describing the gap"
        ));
        edge = false;
    }
    if !name.is_empty() && reference.in_book {
        errors.push(if reference.in_declaration {
            format!(
                "{location}: a named {{fixme}} to {name} must not appear in a requirement or example declaration; an unnamed :::{{fixme}} with :github: lines may tie it to an issue"
            )
        } else {
            format!(
                "{location}: a named {{fixme}} to {name} must appear in a source outside the book"
            )
        });
        edge = false;
    }
    // The body checks need a complete body.
    let Some(body) = reference.commentary.as_deref() else {
        return false;
    };
    if unclosed {
        return edge && !name.is_empty();
    }
    let parsed = crate::fixme::FixmeBody::parse(body);
    for problem in &parsed.problems {
        errors.push(format!("{location}: {problem}"));
    }
    if name.is_empty() {
        if parsed.issues.is_empty() {
            errors.push(format!(
                "{location}: an unnamed {{fixme}} must name at least one issue with a :github: <owner>/<repo>#<number> line"
            ));
        }
        return false;
    }
    if parsed.description.is_empty() {
        errors.push(format!(
            "{location}: {{fixme}} to {name} has an empty body; describe the gap it acknowledges"
        ));
    }
    edge
}

/// Reports FIXME strings that no current `{fixme}` to the definition could
/// have produced: each string must be an issue a `{fixme}` names, as written
/// after `:github:`, or the description of one naming no issue, listed once;
/// a `{fixme}` evidently acknowledging a gap, through an issue only it
/// names, must have all of its issues listed. Whether each gap is real and
/// acknowledged is the reviewer's judgment and is not reassessed.
// {impl}`requirements.ci.outcomes`
// {impl}`requirements.references.site_review`
pub(crate) fn validate_fixme_entries(
    root: &Path,
    label: &str,
    name: &str,
    entries: &[String],
    fixmes: &[&SourceReference],
    errors: &mut Vec<String>,
) {
    let bodies = fixmes
        .iter()
        .map(|reference| {
            crate::fixme::FixmeBody::parse(reference.commentary.as_deref().unwrap_or(""))
        })
        .collect::<Vec<_>>();
    let known = bodies
        .iter()
        .flat_map(crate::fixme::FixmeBody::acknowledgments)
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    for entry in entries {
        if !seen.insert(entry) {
            errors.push(format!(
                "{label}: FIXME entry {entry:?} is listed more than once"
            ));
        } else if !known.contains(entry) {
            errors.push(format!(
                "{label}: FIXME entry {entry:?} is neither an issue named by a {{fixme}} to {name} nor the body of one naming no issue"
            ));
        }
    }
    for (index, (reference, body)) in fixmes.iter().zip(&bodies).enumerate() {
        let unique = |issue: &String| {
            bodies
                .iter()
                .enumerate()
                .all(|(other, body)| other == index || !body.issues.contains(issue))
        };
        let missing = body
            .issues
            .iter()
            .filter(|issue| !entries.contains(issue))
            .collect::<Vec<_>>();
        if !missing.is_empty()
            && body
                .issues
                .iter()
                .any(|issue| entries.contains(issue) && unique(issue))
        {
            errors.push(format!(
                "{label}: FIXME entries omit {} named by the acknowledging {{fixme}} to {name} at {}:{}",
                missing
                    .iter()
                    .map(|issue| format!("{issue:?}"))
                    .collect::<Vec<_>>()
                    .join(", "),
                display_path(root, &reference.file),
                reference.line
            ));
        }
    }
}

// {impl}`requirements.references.discovery`
pub(crate) fn collect_references(root: &Path, errors: &mut Vec<String>) -> Vec<SourceReference> {
    let files = discovered_files(root, errors);
    collect_references_with_progress(root, &files, errors, &mut |_| {})
}

/// The project files reference discovery considers under source-access
/// policy, reporting a failure to read the Git index as a discovery error.
// {impl}`requirements.references.discovery`
// {impl}`requirements.ci.errors`
pub(crate) fn discovered_files(root: &Path, errors: &mut Vec<String>) -> Vec<PathBuf> {
    match crate::authoring::access::DiscoverySources::load(root) {
        Ok(sources) => sources.files(errors),
        Err(error) => {
            errors.push(format!("cannot discover reference sources: {error}"));
            Vec::new()
        }
    }
}

/// Reports each workspace `habeas.toml` in the Git index that contains
/// explicit credentials, naming its credential settings but never their
/// values. Outside Git nothing is committed, so nothing is reported; a Git
/// index that cannot be read is reported by discovery.
// {impl}`requirements.authoring.credentials`
// {impl}`requirements.ci.errors`
fn validate_committed_credentials(root: &Path, errors: &mut Vec<String>) {
    let Ok(sources) = crate::authoring::access::DiscoverySources::load(root) else {
        return;
    };
    for file in sources
        .indexed()
        .into_iter()
        .flatten()
        .filter(|file| file.file_name().is_some_and(|name| name == "habeas.toml"))
    {
        let Some(fields) = fs::read_to_string(file)
            .ok()
            .and_then(|text| crate::redaction::credential_fields(&text))
            .filter(|fields| !fields.is_empty())
        else {
            continue;
        };
        let settings = fields
            .iter()
            .map(|field| format!("`{}`", field.setting))
            .collect::<Vec<_>>()
            .join(", ");
        errors.push(format!(
            "{}: committed workspace configuration contains explicit credentials in {settings}; committed credentials leak, so remove them and use a credential source instead (`aws_profile` or the default AWS chain for Bedrock, `api_key_env` for Ollama), keeping literal credentials only in an untracked file",
            display_path(root, file)
        ));
    }
}

/// Untracked files in a Git worktree, outside the discovery exclusions, that
/// contain references discovery would otherwise find; empty outside Git.
// {impl}`requirements.cli.status`
pub(crate) fn untracked_reference_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let root = fs::canonicalize(root)?;
    let md_root = book_source_dir(&root, &mut Vec::new());
    let untracked = crate::authoring::access::DiscoverySources::load(&root)?.untracked()?;
    Ok(untracked
        .into_iter()
        .filter(|file| {
            fs::read_to_string(file).is_ok_and(|content| {
                !content.contains('\0')
                    && file_references(file, &content, file.starts_with(&md_root))
                        .iter()
                        .any(|reference| reference.kind != ReferenceKind::Term)
            })
        })
        .collect())
}

// {impl}`requirements.references.discovery`
// {impl}`requirements.authoring.progress`
// {impl}`requirements.ci.errors`
pub(crate) fn collect_references_with_progress(
    root: &Path,
    files: &[PathBuf],
    errors: &mut Vec<String>,
    progress: &mut impl FnMut(InspectionProgress),
) -> Vec<SourceReference> {
    // Book configuration errors are reported by inspection itself.
    let md_root = book_source_dir(root, &mut Vec::new());

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
        references.extend(file_references(file, &content, file.starts_with(&md_root)));
    }
    progress(InspectionProgress {
        stage: "reference source files",
        completed: files.len(),
        total: Some(files.len()),
    });
    references
}

/// The inline and fenced references in one text file, in line order.
// {impl}`requirements.references.discovery`
fn file_references(file: &Path, content: &str, in_book: bool) -> Vec<SourceReference> {
    let markdown = crate::outline::is_markdown_path(file);
    let mut references = Vec::new();
    for role in reference_roles(content, markdown) {
        let line = content[..role.start]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1;
        references.push(SourceReference {
            name: role.name.to_owned(),
            kind: role.kind,
            file: file.to_owned(),
            line,
            commentary: None,
            closed: true,
            in_book,
            in_declaration: false,
        });
    }
    let fences = crate::fenced::source_fences(file, content);
    // Declarations are found only where a `{fixme}` needs them.
    let declarations = if markdown && fences.iter().any(|f| f.kind == ReferenceKind::Fixme) {
        declaration_line_ranges(content)
    } else {
        Vec::new()
    };
    for fence in fences {
        references.push(SourceReference {
            in_declaration: declarations
                .iter()
                .any(|range| range.0 < fence.line && fence.line < range.1),
            name: fence.name,
            kind: fence.kind,
            file: file.to_owned(),
            line: fence.line,
            commentary: Some(fence.body),
            closed: fence.closed,
            in_book,
        });
    }
    references.sort_by_key(|reference| reference.line);
    references
}

/// The one-based opening and closing fence lines of each requirement or
/// example declaration in Markdown text.
// {impl}`requirements.references.roles`
fn declaration_line_ranges(content: &str) -> Vec<(usize, usize)> {
    let line_of = |offset: usize| content[..offset].bytes().filter(|b| *b == b'\n').count() + 1;
    definition_blocks(content)
        .into_iter()
        .filter(|block| block.kind != DefinitionKind::Term)
        .map(|block| {
            let close = if block.closed {
                line_of(block.body_end)
            } else {
                usize::MAX
            };
            (line_of(block.start), close)
        })
        .collect()
}

/// Unreadable records and unresolvable inputs contribute no ranges: their
/// own checks report them, and coverage checking continues.
// {impl}`requirements.references.evidence_coverage`
// {impl}`requirements.ci.errors`
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
pub(crate) mod tests {
    /// Validation errors other than citation reviews, for fixtures that test
    /// other checks.
    // {test}`requirements.references.citation_review`
    pub(crate) fn without_citation_reviews(errors: Vec<String>) -> Vec<String> {
        errors
            .into_iter()
            .filter(|error| !error.starts_with("evidence/citations/"))
            .collect()
    }

    use std::fs;
    use std::path::Path;

    use serde_json::{Value, json};
    use tempfile::TempDir;

    /// Records the current reference-set digest in every evidence record for
    /// a defined requirement or example that has none, so fixtures that
    /// write records by hand need not compute it.
    // {test}`requirements.ci.reference_set`
    pub(crate) fn stamp_reference_sets(root: &Path) {
        let Ok(entries) = fs::read_dir(root.join("evidence")) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix(".json"))
            else {
                continue;
            };
            let Some(Value::Object(mut record)) = fs::read_to_string(&path)
                .ok()
                .and_then(|content| serde_json::from_str(&content).ok())
            else {
                continue;
            };
            if record.contains_key("references") {
                continue;
            }
            let Ok(digest) = super::reference_set_digest_for(root, name) else {
                continue;
            };
            record.insert("references".to_owned(), Value::String(digest));
            fs::write(&path, Value::Object(record).to_string()).unwrap();
        }
    }

    /// Validates after stamping hand-written records with their current
    /// reference-set digests.
    // {test}`requirements.integration.report_api`
    // {test}`requirements.ci.reference_set`
    fn validate_repository(root: &Path) -> std::io::Result<super::EvidenceReport> {
        stamp_reference_sets(root);
        super::validate_repository(root)
    }

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

    // {test}`requirements.authoring.credentials`
    #[test]
    fn committed_workspace_credentials_are_reported_but_untracked_ones_are_not() {
        let temp = fixture();
        let root = temp.path();
        fs::create_dir_all(root.join("tools")).unwrap();
        let config = "version = 1\n[authoring.configs.cloud]\nbackend = 'bedrock'\nmodel = 'm'\n[authoring.configs.cloud.credentials]\naccess_key_id = 'PRIVATE_ACCESS'\nsecret_access_key = 'PRIVATE_SECRET'\n";
        fs::write(root.join("habeas.toml"), config).unwrap();
        fs::write(root.join("tools/habeas.toml"), config).unwrap();
        crate::authoring::access::tests::git(root, &["init", "--quiet"]);
        crate::authoring::access::tests::git(root, &["add", "book.toml", "src", "tools"]);
        let committed = |errors: &[String]| {
            errors
                .iter()
                .filter(|error| error.contains("contains explicit credentials"))
                .cloned()
                .collect::<Vec<_>>()
        };
        let report = super::validate_repository(root).unwrap();
        let found = committed(&report.errors);
        assert_eq!(found.len(), 1, "{:#?}", report.errors);
        assert!(found[0].starts_with("tools/habeas.toml: "), "{}", found[0]);
        assert!(found[0].contains("`authoring.configs.cloud.credentials.access_key_id`, `authoring.configs.cloud.credentials.secret_access_key`"), "{}", found[0]);
        assert!(found[0].contains("aws_profile") && found[0].contains("api_key_env"));
        assert!(!found[0].contains("PRIVATE_"), "{}", found[0]);
        // Staging the workspace file reports it too; a credential-free one is fine.
        crate::authoring::access::tests::git(root, &["add", "habeas.toml"]);
        assert_eq!(
            committed(&super::validate_repository(root).unwrap().errors).len(),
            2
        );
        fs::write(root.join("tools/habeas.toml"), "version = 1\n[authoring.configs.cloud]\nbackend = 'bedrock'\nmodel = 'm'\naws_profile = 'dev'\n").unwrap();
        let found = committed(&super::validate_repository(root).unwrap().errors);
        assert_eq!(found.len(), 1);
        assert!(found[0].starts_with("habeas.toml: "));
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
            stamp_reference_sets(root);
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
    fn citation_review_validation_reports_counted_progress() {
        let temp = fixture();
        let root = temp.path();
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
        fs::create_dir_all(root.join("evidence/citations/src")).unwrap();
        fs::write(root.join("evidence/citations/src/gone.json"), "{}").unwrap();
        let mut updates = Vec::new();
        let inspected =
            super::inspect_repository_with_progress(root, |update| updates.push(update)).unwrap();
        assert_eq!(
            inspected.report.errors,
            super::inspect_repository(root).unwrap().report.errors
        );
        let phase: Vec<_> = updates
            .iter()
            .filter(|update| update.stage == "citation reviews")
            .map(|update| (update.completed, update.total))
            .collect();
        // Discovery first, then two items and one orphaned review file.
        assert_eq!(
            phase,
            [
                (0, None),
                (0, Some(3)),
                (1, Some(3)),
                (2, Some(3)),
                (3, Some(3))
            ]
        );
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
        for stage in ["evidence files", "reference coverage", "citation reviews"] {
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
    // {test}`requirements.configuration.loading`
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

    // {test}`requirements.ci.errors`
    #[test]
    fn failed_checks_do_not_hide_later_independent_checks() {
        let temp = fixture();
        let root = temp.path();
        let fixme = ["{", "fixme}`requirements.missing`"].concat();
        fs::create_dir_all(root.join("md/bad dir")).unwrap();
        // Invalid and reserved names still have their fence and text checked.
        fs::write(root.join("md/bad dir/rules.md"), ":::{req} good\n:::\n").unwrap();
        fs::write(
            root.join("md/requirements.md"),
            format!(
                ":::{{req}} habeas:none\n:::\n\n:::{{req}} one\nFirst.\n:::\n\nSee {fixme}.\n\n:::{{fixme}} requirements.one\n:::\n"
            ),
        )
        .unwrap();
        fs::write(
            root.join("evidence/requirements.one.json"),
            json!({
                "requirement": "First.",
                "determination": true,
                "rationale": "Checked.",
                "inputs": [
                    {"definition": "requirements.missing", "text": [1, 2]},
                    {"requirements": "requirements.none", "matches": ["requirements.one", "requirements.one", 3], "text": ["x"]},
                    {"file": "src/gone.rs", "item": "", "hash": "ABC"},
                ]
            })
            .to_string(),
        )
        .unwrap();
        let errors = validate_repository(root).unwrap().errors;
        let all = errors.join("\n");
        for detail in [
            "md/bad dir/rules.md:1: invalid requirement name \"bad dir.rules.good\"",
            "md/bad dir/rules.md:1: requirement bad dir.rules.good has no text",
            "md/requirements.md:1: invalid requirement name \"requirements.habeas:none\": names beginning with habeas: are reserved",
            "md/requirements.md:1: requirement requirements.habeas:none has no text",
            // Every element of `text` is checked, and the definition is
            // still resolved.
            "inputs[0]: text[0] must be a string",
            "inputs[0]: text[1] must be a string",
            "inputs[0]: requirements.missing: no matching named definition",
            // Duplicate matches are reported beside a non-string entry, and
            // the selection is still resolved.
            "inputs[1]: matches[2] must be a string",
            "inputs[1]: matches must have distinct definition names",
            "inputs[1]: requirements glob \"requirements.none\" matches no requirement",
            // A malformed hash does not hide a missing file.
            "inputs[2]: hash must be a lowercase hexadecimal SHA-256 digest",
            "inputs[2]: cannot open src/gone.rs",
            // An inline `{fixme}` in the book is reported for its form, its
            // placement, and its unknown name.
            "md/requirements.md:8: inline {fixme} reference to requirements.missing must use the fenced form",
            "md/requirements.md:8: a named {fixme} to requirements.missing must appear in a source outside the book",
            &format!("md/requirements.md:8: {fixme} has no matching requirement or example"),
            // A misplaced named `{fixme}` still has its body checked.
            "md/requirements.md:10: a named {fixme} to requirements.one must appear in a source outside the book",
            "md/requirements.md:10: {fixme} to requirements.one has an empty body",
        ] {
            assert!(all.contains(detail), "missing {detail:?} in:\n{all}");
        }
        // No failure is reported twice.
        let mut unique = errors.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), errors.len(), "{all}");
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

        let errors = without_citation_reviews(validate_repository(temp.path()).unwrap().errors);
        assert!(errors.is_empty(), "{errors:?}");
    }

    // {test}`requirements.references.discovery`
    // {test}`requirements.references.markdown_literals`
    #[test]
    fn every_markdown_extension_keeps_escaped_and_code_roles_literal() {
        let temp = fixture();
        let role = ["{", "cite}`code.role`"].concat();
        let fence = [":::", "{cite} fenced.role\nBody.\n:::"].concat();
        let notes = format!(
            "`{role}` and \\{role}\n\n```text\n{role}\n{fence}\n```\n\n<span title=\"{role}\"></span>\n"
        );
        fs::create_dir_all(temp.path().join("docs")).unwrap();
        for file in [
            "docs/a.md",
            "docs/b.markdown",
            "docs/C.MD",
            "docs/d.Markdown",
        ] {
            fs::write(temp.path().join(file), &notes).unwrap();
        }
        // The same text in a plain-text file is not Markdown.
        fs::write(temp.path().join("docs/e.txt"), &notes).unwrap();
        let mut errors = Vec::new();
        let references = super::collect_references(temp.path(), &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
        let files = references
            .iter()
            .map(|reference| super::display_path(temp.path(), &reference.file))
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            files.into_iter().collect::<Vec<_>>(),
            ["docs/e.txt"],
            "{references:#?}"
        );
    }

    // {test}`requirements.references.discovery`
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

    // {test}`requirements.ci.requirements_inputs`
    // {test}`requirements.references.definition_glob_cases`
    // {test}`requirements.references.definition_globs`
    #[test]
    fn requirements_inputs_track_matched_declarations_in_full_name_order() {
        let temp = fixture();
        let bar = ":::{req} a.bar\nBar.\n:::\n";
        let nested =
            ":::{example} a.b.bar\nNested. CITE`foo.a.bar`\n:::\n".replace("CITE", "{cite}");
        let other = ":::{req} other\nOther.\n:::\n";
        let write = |declarations: &[&str]| {
            fs::write(
                temp.path().join("md/foo.md"),
                format!("# Foo\n\n{}", declarations.join("\n")),
            )
            .unwrap();
        };
        let lines = |declaration: &str| declaration.lines().map(str::to_owned).collect::<Vec<_>>();
        let mut text = lines(&nested);
        text.push(String::new());
        text.extend(lines(bar));
        let record = |glob: &str| {
            json!({
                "requirement": "Other.",
                "determination": true,
                "rationale": "Both matches were examined.",
                "inputs": [{
                    "requirements": glob,
                    "matches": ["foo.a.b.bar", "foo.a.bar"],
                    "text": text,
                }]
            })
            .to_string()
        };
        fs::write(
            temp.path().join("evidence/foo.other.json"),
            record("foo.*.bar"),
        )
        .unwrap();
        let errors = || {
            validate_repository(temp.path())
                .unwrap()
                .errors
                .into_iter()
                .filter(|error| error.starts_with("evidence/foo.other.json"))
                .collect::<Vec<_>>()
        };

        // `*` crosses dots, the example sorts before the requirement by full
        // name, and the stored text is both complete fenced declarations.
        write(&[bar, &nested, other]);
        assert!(errors().is_empty(), "{:?}", errors());
        // Moving unchanged declarations does not invalidate the input.
        write(&[other, "Some prose.\n", &nested, "More prose.\n", bar]);
        assert!(errors().is_empty(), "{:?}", errors());

        write(&[&bar.replace("Bar.", "Changed."), &nested, other]);
        let changed = errors().join("\n");
        assert!(
            changed.contains(
                "inputs[0]: input text changed for Requirements { requirements: \"foo.*.bar\" }; changed definitions [\"foo.a.bar\"]\n- Bar.\n+ Changed."
            ),
            "{changed}"
        );

        let added = ":::{req} z.bar\nAdded.\n:::\n";
        write(&[bar, &nested, other, added]);
        assert!(
            errors().join("\n").contains(
                "inputs[0]: requirements glob \"foo.*.bar\" matches changed; added [\"foo.z.bar\"], removed []"
            ),
            "{:?}",
            errors()
        );
        write(&[bar, other]);
        assert!(
            errors().join("\n").contains(
                "requirements glob \"foo.*.bar\" matches changed; added [], removed [\"foo.a.b.bar\"]"
            ),
            "{:?}",
            errors()
        );
        write(&[&bar.replace("a.bar", "c.bar"), &nested, other]);
        assert!(
            errors().join("\n").contains(
                "requirements glob \"foo.*.bar\" matches changed; added [\"foo.c.bar\"], removed [\"foo.a.bar\"]"
            ),
            "{:?}",
            errors()
        );

        write(&[bar, &nested, other]);
        fs::write(
            temp.path().join("evidence/foo.other.json"),
            record("foo.*.none"),
        )
        .unwrap();
        assert_eq!(
            errors(),
            [
                "evidence/foo.other.json: inputs[0]: requirements glob \"foo.*.none\" matches no requirement or example"
            ]
        );

        // A selection that is not a valid definition glob is rejected.
        for (glob, problem) in [
            ("foo.[a].bar", "unsupported character '['"),
            ("", "definition glob must not be empty"),
        ] {
            fs::write(temp.path().join("evidence/foo.other.json"), record(glob)).unwrap();
            let found = errors();
            assert!(
                found.len() == 1 && found[0].contains("inputs[0]") && found[0].contains(problem),
                "{glob:?}: {found:?}"
            );
        }
    }

    // {test}`requirements.ci.errors`
    #[test]
    fn configuration_and_dependency_failures_do_not_stop_later_checks() {
        let temp = fixture();
        fs::write(
            temp.path().join("md/rules.md"),
            ":::{req} ready\nUse TERM`rules.missing` and TERM`rules.absent`.\n:::\n\n:::{req} other\nOther.\n:::\n"
                .replace("TERM", "{term}"),
        )
        .unwrap();
        fs::write(
            temp.path().join("evidence/rules.ready.json"),
            json!({
                "requirement": "Use TERM`rules.missing` and TERM`rules.absent`.".replace("TERM", "{term}"),
                "determination": true,
                "rationale": "Examined.",
                "inputs": [{"definition": "rules.gone", "text": ["x"]}]
            })
            .to_string(),
        )
        .unwrap();
        // Every unresolvable term is reported, and the record's other checks
        // and other records' checks still run.
        let errors = validate_repository(temp.path()).unwrap().errors.join("\n");
        for detail in [
            "evidence/rules.ready.json: definition dependencies: term rules.absent: no matching named definition",
            "evidence/rules.ready.json: definition dependencies: term rules.missing: no matching named definition",
            "evidence/rules.ready.json: inputs[0]: rules.gone: no matching named definition",
            "evidence/rules.other.json: missing for requirement",
        ] {
            assert!(errors.contains(detail), "missing {detail:?} in:\n{errors}");
        }

        // Invalid workspace and book configuration are each reported, and
        // checks that do not depend on them still run.
        fs::write(temp.path().join("habeas.toml"), "version = 2\n").unwrap();
        fs::write(temp.path().join("book.toml"), "[book]\nsrc = 'md'\n[").unwrap();
        fs::write(temp.path().join("evidence/ghost.json"), "{").unwrap();
        let unknown = ["{", "impl}`missing.name`"].concat();
        fs::write(temp.path().join("src/main.rs"), format!("// {unknown}\n")).unwrap();
        let errors = validate_repository(temp.path()).unwrap().errors.join("\n");
        for detail in [
            "habeas.toml: version: expected version = 1".to_owned(),
            "book.toml: invalid TOML".to_owned(),
            "evidence/ghost.json: invalid JSON".to_owned(),
            format!("src/main.rs:1: {unknown} has no matching requirement"),
        ] {
            assert!(errors.contains(&detail), "missing {detail:?} in:\n{errors}");
        }
    }

    // {test}`requirements.ci.errors`
    #[test]
    fn git_index_failure_is_reported_once_and_later_checks_still_run() {
        let temp = fixture();
        fs::write(
            temp.path().join("md/rules.md"),
            ":::{req} ready\nReady.\n:::\n",
        )
        .unwrap();
        // A `.git` file naming a missing Git directory makes the Git index
        // unreadable, so reference discovery fails.
        fs::write(
            temp.path().join(".git"),
            format!("gitdir: {}\n", temp.path().join("missing").display()),
        )
        .unwrap();
        fs::write(temp.path().join("evidence/ghost.json"), "{").unwrap();
        let errors = validate_repository(temp.path()).unwrap().errors;
        let discovery = errors
            .iter()
            .filter(|error| error.starts_with("cannot discover reference sources:"))
            .count();
        assert_eq!(discovery, 1, "{errors:#?}");
        let errors = errors.join("\n");
        for detail in [
            "evidence/rules.ready.json: missing for requirement",
            "evidence/ghost.json: invalid JSON",
        ] {
            assert!(errors.contains(detail), "missing {detail:?} in:\n{errors}");
        }
    }

    // {test}`requirements.ci.schema`
    #[test]
    fn schema_rejects_each_malformed_field_and_input_form() {
        let temp = fixture();
        fs::write(
            temp.path().join("md/rules.md"),
            "# Rules\n\n:::{req} ready\nReady.\n:::\n\n:::{req} blank\nBlank.\n:::\n\n:::{req} listed\nListed.\n:::\n\n:::{definition} term\n:phrase: term\n\nA term.\n:::\n",
        )
        .unwrap();
        // A whitespace-only definition text and a top-level value that is not
        // an object are each rejected.
        fs::write(
            temp.path().join("evidence/rules.blank.json"),
            json!({
                "requirement": " \n",
                "determination": true,
                "rationale": "Examined.",
                "references": "0".repeat(64),
                "inputs": []
            })
            .to_string(),
        )
        .unwrap();
        fs::write(temp.path().join("evidence/rules.listed.json"), "[]").unwrap();
        let term = [
            ":::{definition} term",
            ":phrase: term",
            "",
            "A term.",
            ":::",
        ];
        fs::write(
            temp.path().join("evidence/rules.ready.json"),
            json!({
                "requirement": "Ready.",
                "determination": true,
                "rationale": " \n",
                "inputs": [
                    {"definition": "rules.term", "text": term},
                    {"definition": 7, "text": term},
                    {"definition": "rules.term", "text": []},
                    {"definition": "rules.term", "requirements": "rules.*", "text": term},
                    {"requirements": "rules.ready", "matches": ["rules.ready", "rules.ready"], "text": ["x"]},
                    {"requirements": "rules.ready", "matches": [1], "text": ["x"]},
                    "src/main.rs",
                    {"file": "src/main.rs", "item": "", "hash": "a".repeat(63)},
                    {"file": "src/main.rs", "item": "", "hash": "g".repeat(64)},
                    {"file": "src/main.rs", "item": "", "hash": "0".repeat(64), "text": ["// first line"]},
                    {"matches": ["rules.ready"], "text": ["x"]},
                    {"requirements": "rules.ready", "text": ["x"]},
                    {"requirements": "rules.ready", "matches": "rules.ready", "text": ["x"]}
                ]
            })
            .to_string(),
        )
        .unwrap();
        let report = validate_repository(temp.path()).unwrap();
        let errors = report.errors.join("\n");
        let label = "evidence/rules.ready.json";
        for detail in [
            format!("{label}: rationale must not be empty"),
            format!("{label}: inputs[1]: definition must be a string"),
            format!("{label}: inputs[2]: text must be a nonempty array"),
            format!("{label}: inputs[3]: unexpected field \"requirements\""),
            format!("{label}: inputs[4]: matches must have distinct definition names"),
            format!("{label}: inputs[5]: matches[0] must be a string"),
            format!("{label}: inputs[6]: expected a JSON object"),
            format!("{label}: inputs[7]: hash must be a lowercase hexadecimal SHA-256 digest"),
            format!("{label}: inputs[8]: hash must be a lowercase hexadecimal SHA-256 digest"),
            format!("{label}: inputs[9]: unexpected field \"text\""),
            format!("{label}: inputs[10]: missing field \"requirements\""),
            format!("{label}: inputs[11]: missing field \"matches\""),
            format!("{label}: inputs[12]: matches must be an array"),
            "evidence/rules.blank.json: requirement must not be empty".to_owned(),
            "evidence/rules.listed.json: expected a JSON object".to_owned(),
        ] {
            assert!(errors.contains(&detail), "missing {detail:?} in:\n{errors}");
        }
        // The well-formed definition input is accepted.
        assert!(!errors.contains("inputs[0]"), "{errors}");
    }

    // {test}`requirements.ci.determination`
    #[test]
    fn a_false_determination_fails_only_its_own_record_with_its_rationale() {
        let temp = fixture();
        let chapter = "# Rules\n\n:::{req} ready\nReady.\n:::\n\n:::{req} refined\nRefines IMPL`rules.ready` and CITE`rules.ready`.\n:::\n"
            .replace("IMPL", "{impl}")
            .replace("CITE", "{cite}");
        fs::write(temp.path().join("md/rules.md"), &chapter).unwrap();
        fs::write(
            temp.path().join("evidence/rules.ready.json"),
            json!({
                "requirement": "Ready.",
                "determination": true,
                "rationale": "The refining requirement was examined.",
                "inputs": crate::inputs::outline_inputs_for_lines(temp.path(), "md/rules.md", 8, 8)
                    .unwrap()
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            temp.path().join("evidence/rules.refined.json"),
            json!({
                "requirement": chapter.lines().nth(7).unwrap(),
                "determination": false,
                "rationale": "  Work remains: the refinement is untested.  ",
                "inputs": []
            })
            .to_string(),
        )
        .unwrap();
        let report = validate_repository(temp.path()).unwrap();
        assert_eq!(
            report.errors,
            [
                "evidence/rules.refined.json: determination is false: Work remains: the refinement is untested."
            ],
            "the implemented and cited requirement is unaffected"
        );
    }

    // {test}`requirements.ci.diagnostics`
    #[test]
    fn changed_requirement_text_names_the_record_and_location_with_a_diff() {
        let temp = fixture();
        fs::write(
            temp.path().join("md/requirements.md"),
            "# Requirements\n\n:::{req} one\nHabeas MUST check input lines.\n:::\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("evidence/requirements.one.json"),
            json!({
                "requirement": "Habeas SHOULD check input lines.",
                "determination": true,
                "rationale": "Examined.",
                "inputs": []
            })
            .to_string(),
        )
        .unwrap();
        let report = validate_repository(temp.path()).unwrap();
        assert_eq!(
            report.errors,
            [
                "evidence/requirements.one.json: requirement text changed (md/requirements.md:3)\n- Habeas SHOULD check input lines.\n+ Habeas MUST check input lines."
            ]
        );
    }

    // {test}`requirements.ci.diagnostics`
    #[test]
    fn compact_diffs_elide_common_lines_and_truncate_long_runs() {
        let diff = |old: &[&str], new: &[&str]| super::compact_diff(old, new);
        assert_eq!(
            diff(&["a", "b", "c", "d"], &["a", "B", "C", "d"]),
            "\n- b\n- c\n+ B\n+ C"
        );
        assert_eq!(diff(&["a", "c"], &["a", "b", "c"]), "\n+ b");
        assert_eq!(diff(&["a", "b", "c"], &["a", "c"]), "\n- b");
        assert_eq!(diff(&["same"], &["same"]), "");
        let old = (0..10).map(|n| format!("old {n}")).collect::<Vec<_>>();
        let old = old.iter().map(String::as_str).collect::<Vec<_>>();
        let truncated = diff(&old, &["new"]);
        assert_eq!(truncated.matches("\n- old").count(), 8, "{truncated}");
        assert!(
            truncated.contains("\n- old 7\n- ... 2 more lines\n+ new"),
            "{truncated}"
        );
        assert!(!truncated.contains("old 8"), "{truncated}");
    }

    // {test}`requirements.ci.span`
    #[test]
    fn requirement_bodies_are_compared_without_surrounding_whitespace() {
        let temp = fixture();
        fs::write(
            temp.path().join("md/requirements.md"),
            "# Requirements\n\n:::{req} one\n\n   Body MUST hold.  \n\n:::\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("evidence/requirements.one.json"),
            json!({
                "requirement": "Body MUST hold.",
                "determination": true,
                "rationale": "Examined.",
                "inputs": []
            })
            .to_string(),
        )
        .unwrap();
        let report = validate_repository(temp.path()).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
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
            errors
                .contains("md/bad dir/rules.md:3: invalid requirement name \"bad dir.rules.good\""),
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

    // {test}`requirements.references.discovery`
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
            errors.contains(
                "requirements glob \"rules.*\" matches changed; added [\"rules.added\"], removed []"
            ),
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
    // {test}`requirements.references.roles`
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
        let errors = without_citation_reviews(validate_repository(temp.path()).unwrap().errors);
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
        let errors = without_citation_reviews(validate_repository(temp.path()).unwrap().errors);
        assert!(errors.is_empty(), "{errors:?}");
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
        let chapter = ":::{req} ready\nReady.\n:::\n\n:::{example} tested\nGiven ready. TEST`rules.ready`\n:::\n\n:::{example} cited\nGiven ready. CITE`rules.ready`\n:::\n\n:::{example} nested\nLike the tested case. CITE`rules.tested`\n:::\n\n:::{example} implemented\nGiven ready. IMPL`rules.ready`\n:::\n\n:::{example} literal\nGiven ready. `CITE`rules.ready`` and \\CITE`rules.ready`\n:::\n\n:::{example} unknown\nGiven ready. CITE`rules.absent`\n:::\n"
            .replace("CITE", "{cite}")
            .replace("TEST", "{test}")
            .replace("IMPL", "{impl}");
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
        // Only `{cite}` or `{test}` counts, and only as a valid reference.
        for name in ["implemented", "literal", "unknown"] {
            assert!(
                errors.contains(&format!(
                    "example rules.{name} must reference at least one requirement with {{cite}} or {{test}}"
                )),
                "{name}: {errors}"
            );
        }
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

    // {test}`requirements.ci.reference_set`
    // {test}`requirements.references.outline_comments`
    #[test]
    fn reference_set_digest_tracks_impl_and_test_edges_by_node() {
        let temp = fixture();
        let root = temp.path();
        let role = |kind: &str| format!("{{{kind}}}`requirements.ready`");
        let declaration = |options: &str| {
            format!(
                ":::{{req}} ready\n{options}Ready MUST hold.\n:::\n\n:::{{req}} other\nOther MUST hold.\n:::\n"
            )
        };
        fs::write(root.join("md/requirements.md"), declaration("")).unwrap();
        let digest = |lib: &str, notes: &str, guide: &str| {
            fs::write(root.join("src/lib.rs"), lib).unwrap();
            fs::write(root.join("notes.txt"), notes).unwrap();
            fs::write(root.join("md/guide.md"), guide).unwrap();
            super::reference_set_digest_for(root, "requirements.ready").unwrap()
        };
        let lib = format!(
            "// {}\nfn run() {{\n    work();\n}}\n\nfn other() {{}}\n",
            role("impl")
        );
        let notes = format!("{}\n", role("test"));
        let guide = format!("# Guide\n\nIntro.\n\n## Usage\n\nSee {}.\n", role("impl"));
        let base = digest(&lib, &notes, &guide);
        assert_eq!(base.len(), 64);
        assert!(super::is_sha256_hex(&base));
        assert_eq!(
            digest(&lib, &notes, &guide),
            base,
            "the digest is deterministic"
        );

        // Line moves within the reference's node, unrelated lines, and
        // `{cite}` references leave the digest unchanged.
        for (lib, notes, guide) in [
            (
                format!("// unrelated\n\n{lib}"),
                notes.clone(),
                guide.clone(),
            ),
            (
                format!(
                    "fn run() {{\n    // {}\n    work();\n}}\n\nfn other() {{}}\n",
                    role("impl")
                ),
                notes.clone(),
                guide.clone(),
            ),
            (
                lib.clone(),
                format!("first\n\nsecond\n{notes}"),
                guide.clone(),
            ),
            (
                lib.clone(),
                notes.clone(),
                guide.replace("See ", "Intro line.\n\nSee "),
            ),
            (
                format!("{lib}// {}\nfn cited() {{}}\n", role("cite")),
                format!("{notes}{}\n", role("cite")),
                guide.clone(),
            ),
        ] {
            assert_eq!(
                digest(&lib, &notes, &guide),
                base,
                "{lib}\n{notes}\n{guide}"
            );
        }

        // Adding, removing, re-roling, or moving a reference to another node
        // changes it.
        let mut seen = std::collections::BTreeSet::from([base.clone()]);
        for (lib, notes, guide) in [
            (
                format!("{lib}// {}\nfn more() {{}}\n", role("impl")),
                notes.clone(),
                guide.clone(),
            ),
            (
                format!(
                    "fn run() {{\n    // {}\n    // {}\n    work();\n}}\n\nfn other() {{}}\n",
                    role("impl"),
                    role("impl")
                ),
                notes.clone(),
                guide.clone(),
            ),
            (
                lib.replace(&role("impl"), "removed"),
                notes.clone(),
                guide.clone(),
            ),
            (lib.replace("impl", "test"), notes.clone(), guide.clone()),
            (
                format!(
                    "fn run() {{\n    work();\n}}\n\n// {}\nfn other() {{}}\n",
                    role("impl")
                ),
                notes.clone(),
                guide.clone(),
            ),
            (lib.clone(), String::new(), guide.clone()),
            (
                lib.clone(),
                notes.clone(),
                guide.replace("Intro.", &format!("Intro {}.", role("impl"))),
            ),
        ] {
            let changed = digest(&lib, &notes, &guide);
            assert!(seen.insert(changed), "{lib}\n{notes}\n{guide}");
        }

        // Adding an anchor changes it, as does changing the anchored node.
        digest(&lib, &notes, &guide);
        for options in [
            ":impl: notes.txt\n",
            ":test: notes.txt\n",
            ":impl: src/lib.rs#fn[run]\n",
        ] {
            fs::write(root.join("md/requirements.md"), declaration(options)).unwrap();
            let changed = super::reference_set_digest_for(root, "requirements.ready").unwrap();
            assert!(seen.insert(changed), "{options}");
        }
    }

    // {test}`requirements.ci.reference_set`
    // {test}`requirements.ci.schema`
    #[test]
    fn validation_requires_a_current_reference_set_digest() {
        let temp = fixture();
        let root = temp.path();
        fs::write(
            root.join("md/requirements.md"),
            ":::{req} ready\nReady MUST hold.\n:::\n",
        )
        .unwrap();
        let impl_role = "IMPL`requirements.ready`".replace("IMPL", "{impl}");
        let cite_role = "CITE`requirements.ready`".replace("CITE", "{cite}");
        fs::write(
            root.join("src/lib.rs"),
            format!(
                "// {impl_role}\nfn run() {{}}\n\n// {cite_role}\nfn other() {{\n    work();\n}}\n"
            ),
        )
        .unwrap();
        let path = root.join("evidence/requirements.ready.json");
        let write = |references: Option<Value>| {
            let mut record = json!({
                "requirement": "Ready MUST hold.",
                "determination": true,
                "rationale": "Checked.",
                "inputs": [outline(root, "src/lib.rs", "")],
            });
            if let Some(references) = references {
                record["references"] = references;
            }
            fs::write(&path, record.to_string()).unwrap();
        };
        let errors = || without_citation_reviews(super::validate_repository(root).unwrap().errors);

        let current = super::reference_set_digest_for(root, "requirements.ready").unwrap();
        write(Some(json!(current)));
        assert!(errors().is_empty(), "{:?}", errors());

        for (references, expected) in [
            (
                None,
                "evidence/requirements.ready.json: missing field \"references\"",
            ),
            (
                Some(json!(7)),
                "evidence/requirements.ready.json: references must be a string",
            ),
            (
                Some(json!(current.to_uppercase())),
                "evidence/requirements.ready.json: references must be a lowercase hexadecimal SHA-256 digest",
            ),
            (
                Some(json!(&current[1..])),
                "evidence/requirements.ready.json: references must be a lowercase hexadecimal SHA-256 digest",
            ),
        ] {
            write(references);
            assert_eq!(errors(), [expected]);
        }

        // A new reference inside a node the inputs already cover changes no
        // recorded hash, but makes the evidence stale.
        write(Some(json!(current)));
        fs::write(
            root.join("src/lib.rs"),
            format!(
                "// {impl_role}\nfn run() {{}}\n\n// {cite_role}\nfn other() {{\n    // {impl_role}\n    work();\n}}\n"
            ),
        )
        .unwrap();
        let reported = errors();
        assert_eq!(reported.len(), 1, "{reported:?}");
        assert!(
            reported[0].starts_with("evidence/requirements.ready.json: reference set changed"),
            "{reported:?}"
        );
    }
}

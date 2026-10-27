use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde_json::{Map, Value};
use toml_edit::DocumentMut;

use crate::inputs::{InputSelector, ResolvedInput};
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

pub(crate) struct Declaration {
    pub(crate) kind: DefinitionKind,
    pub(crate) text: String,
    pub(crate) source: String,
    pub(crate) file: String,
    pub(crate) line_start: usize,
    pub(crate) line_end: usize,
    pub(crate) raw_lines: Vec<String>,
}

// {impl}`requirements.integration.report_api`
pub fn validate_repository(root: &Path) -> io::Result<EvidenceReport> {
    Ok(inspect_repository(root)?.report)
}

pub(crate) struct RepositoryInspection {
    pub(crate) definitions: BTreeMap<String, Declaration>,
    pub(crate) report: EvidenceReport,
}

// {impl}`requirements.integration.report_api`
pub(crate) fn inspect_repository(root: &Path) -> io::Result<RepositoryInspection> {
    let root = fs::canonicalize(root)?;
    let mut errors = Vec::new();
    let md_root = book_source_dir(&root, &mut errors);
    let definitions = collect_definitions(&root, &md_root, &mut errors);
    let evidence_root = root.join("evidence");
    let evidence_files = if evidence_root.is_dir() {
        files_with_extension(&root, &evidence_root, "json", &mut errors)
    } else {
        Vec::new()
    };
    let mut present = BTreeSet::new();

    // {impl}`requirements.ci.orphans`
    for file in &evidence_files {
        let relative = file
            .strip_prefix(&evidence_root)
            .expect("evidence file is inside evidence directory");
        if relative.starts_with("rust") && relative.to_string_lossy().ends_with(".rs.json") {
            continue;
        }
        let label = display_path(&root, file);
        let name = if relative.components().count() == 1 {
            relative
                .file_name()
                .and_then(|part| part.to_str())
                .and_then(|part| part.strip_suffix(".json"))
        } else {
            None
        };
        let expected = name.and_then(|name| definitions.get(name));

        if let Some(name) = name.filter(|_| expected.is_some()) {
            present.insert(name.to_owned());
        } else {
            errors.push(format!("{label}: no matching requirement or example"));
        }
        validate_evidence_file(&root, file, expected, &definitions, &mut errors);
    }

    // {impl}`requirements.ci.coverage`
    for (name, definition) in &definitions {
        if !present.contains(name) {
            errors.push(format!(
                "evidence/{name}.json: missing for {} at {}",
                definition.kind.noun(),
                definition.source
            ));
        }
    }

    validate_references(&root, &definitions, &mut errors);
    // {impl}`requirements.references.rust_function_coverage`
    crate::rust_coverage::validate_rust_functions(&root, &definitions, &mut errors);
    // {impl}`requirements.references.rust_function_attestations`
    validate_rust_attestations(&root, &definitions, &mut errors);

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
    })
}

// {impl}`requirements.ci.book_source`
fn book_source_dir(root: &Path, errors: &mut Vec<String>) -> PathBuf {
    let path = root.join("book.toml");
    let source = match fs::read_to_string(&path) {
        Ok(content) => match content.parse::<DocumentMut>() {
            Ok(book) => book["book"]["src"].as_str().unwrap_or("src").to_owned(),
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
        for block in definition_blocks(&content) {
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
            let text = content[block.body_start..block.body_end].trim().to_owned();
            if text.is_empty() {
                errors.push(format!(
                    "{source}: {} {name} has no text",
                    block.kind.noun()
                ));
            }

            match definitions.entry(name.clone()) {
                Entry::Vacant(slot) => {
                    let raw_lines = content[block.start..block.end]
                        .lines()
                        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
                        .collect::<Vec<_>>();
                    slot.insert(Declaration {
                        kind: block.kind,
                        text,
                        source,
                        file: label.clone(),
                        line_start: line,
                        line_end: line + raw_lines.len().saturating_sub(1),
                        raw_lines,
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
                    reference.kind == ReferenceKind::Cite
                        && definitions
                            .get(reference.name)
                            .is_some_and(|target| target.kind == DefinitionKind::Requirement)
                })
        {
            errors.push(format!(
                "{}: example {name} must cite at least one requirement",
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
fn validate_evidence_file(
    root: &Path,
    path: &Path,
    expected: Option<&Declaration>,
    definitions: &BTreeMap<String, Declaration>,
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

    let determination = boolean_field(object, "determination", &label, errors);
    let rationale = string_field(object, "rationale", &label, errors);
    if let Some(rationale) = rationale
        && rationale.trim().is_empty()
    {
        errors.push(format!("{label}: rationale must not be empty"));
    }
    // {impl}`requirements.ci.determination`
    if determination == Some(false) {
        errors.push(format!(
            "{label}: determination is false{}",
            rationale
                .filter(|text| !text.trim().is_empty())
                .map_or(String::new(), |text| format!(": {}", text.trim()))
        ));
    }

    if let Some(inputs) = array_field(object, "inputs", &label, errors) {
        for (index, input) in inputs.iter().enumerate() {
            validate_input(
                root,
                definitions,
                input,
                &format!("{label}: inputs[{index}]"),
                errors,
            );
        }
    }
}

// {impl}`requirements.ci.schema`
// {impl}`requirements.ci.input_containment`
// {impl}`requirements.ci.rust_item_inputs`
// {impl}`requirements.ci.requirements_inputs`
fn validate_input(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    input: &Value,
    label: &str,
    errors: &mut Vec<String>,
) -> Option<ResolvedInput> {
    let Some(object) = input.as_object() else {
        errors.push(format!("{label}: expected a JSON object"));
        return None;
    };
    if object.contains_key("rust_file") || object.contains_key("rust_item") {
        check_keys(object, &["rust_file", "rust_item", "text"], label, errors);
        let rust_file = string_field(object, "rust_file", label, errors);
        let rust_item = string_field(object, "rust_item", label, errors);
        let text = text_field(object, label, errors);
        let (Some(rust_file), Some(rust_item), Some(text)) = (rust_file, rust_item, text) else {
            return None;
        };
        let selector = InputSelector::RustItem {
            rust_file: rust_file.to_owned(),
            rust_item: rust_item.to_owned(),
        };
        return validate_resolved_input(root, definitions, &selector, &text, None, label, errors);
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
            &selector,
            &text,
            Some(&matches),
            label,
            errors,
        );
    }
    validate_file_range_input(root, definitions, object, label, errors)
}

// {impl}`requirements.ci.schema`
// {impl}`requirements.ci.inputs`
fn validate_file_range_input(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    object: &Map<String, Value>,
    label: &str,
    errors: &mut Vec<String>,
) -> Option<ResolvedInput> {
    check_keys(
        object,
        &["file", "line_start", "line_end", "text"],
        label,
        errors,
    );

    let file = string_field(object, "file", label, errors).and_then(|file| {
        let relative = Path::new(file);
        if file.is_empty()
            || !relative
                .components()
                .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
        {
            errors.push(format!(
                "{label}: file must be a path relative to the repository"
            ));
            None
        } else {
            Some(file)
        }
    });
    let start = positive_integer_field(object, "line_start", label, errors);
    let end = positive_integer_field(object, "line_end", label, errors);
    let text = text_field(object, label, errors);

    if let (Some(start), Some(end)) = (start, end)
        && end < start
    {
        errors.push(format!("{label}: line_end must be at least line_start"));
        return None;
    }
    let (Some(file), Some(start), Some(end), Some(text)) = (file, start, end, text) else {
        return None;
    };
    validate_input_lines(
        root,
        file,
        start,
        end,
        &text.iter().map(String::as_str).collect::<Vec<_>>(),
        label,
        errors,
    );
    InputSelector::FileRange {
        file: file.to_owned(),
        line_start: start,
        line_end: end,
    }
    .resolve(root, definitions)
    .ok()
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

// {impl}`requirements.ci.rust_item_inputs`
// {impl}`requirements.ci.requirements_inputs`
fn validate_resolved_input(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    selector: &InputSelector,
    text: &[String],
    matches: Option<&[String]>,
    label: &str,
    errors: &mut Vec<String>,
) -> Option<ResolvedInput> {
    let resolved = match selector.resolve(root, definitions) {
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

// {impl}`requirements.ci.inputs`
// {impl}`requirements.ci.input_containment`
fn validate_input_lines(
    root: &Path,
    file: &str,
    start: usize,
    end: usize,
    text: &[&str],
    label: &str,
    errors: &mut Vec<String>,
) {
    let path = match fs::canonicalize(root.join(file)) {
        Ok(path) => path,
        Err(error) => {
            errors.push(format!("{label}: cannot open {file}: {error}"));
            return;
        }
    };
    if !path.starts_with(root) {
        errors.push(format!("{label}: {file} resolves outside the repository"));
        return;
    }
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) => {
            errors.push(format!("{label}: cannot read {file}: {error}"));
            return;
        }
    };
    let lines: Vec<_> = content
        .lines()
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    if end > lines.len() {
        errors.push(format!(
            "{label}: {file}:{start}-{end} exceeds the file's {} lines",
            lines.len()
        ));
        return;
    }
    let expected_count = end - start + 1;
    if text.len() != expected_count {
        errors.push(format!(
            "{label}: text has {} lines, but {file}:{start}-{end} has {expected_count}",
            text.len()
        ));
    }
    let actual = &lines[start - 1..end];
    if text != actual {
        errors.push(format!(
            "{label}: input text changed at {file}:{start}-{end}{}",
            compact_diff(text, actual)
        ));
    }
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
fn boolean_field(
    object: &Map<String, Value>,
    key: &str,
    label: &str,
    errors: &mut Vec<String>,
) -> Option<bool> {
    match object.get(key) {
        Some(Value::Bool(value)) => Some(*value),
        Some(_) => {
            errors.push(format!("{label}: {key} must be a boolean"));
            None
        }
        None => {
            errors.push(format!("{label}: missing field {key:?}"));
            None
        }
    }
}

// {impl}`requirements.ci.schema`
fn positive_integer_field(
    object: &Map<String, Value>,
    key: &str,
    label: &str,
    errors: &mut Vec<String>,
) -> Option<usize> {
    match object.get(key) {
        Some(value) => match value.as_u64().and_then(|value| usize::try_from(value).ok()) {
            Some(value) if value > 0 => Some(value),
            _ => {
                errors.push(format!("{label}: {key} must be a positive integer"));
                None
            }
        },
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

struct SourceReference {
    name: String,
    kind: ReferenceKind,
    file: PathBuf,
    line: usize,
}

struct InputRange {
    file: PathBuf,
    line_start: usize,
    line_end: usize,
}

// {impl}`requirements.references.evidence_coverage`
// {impl}`requirements.references.targets`
// {impl}`requirements.examples.evidence`
fn validate_references(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    errors: &mut Vec<String>,
) {
    let mut by_definition: BTreeMap<String, Vec<SourceReference>> = BTreeMap::new();
    for reference in collect_references(root, errors) {
        if definitions.contains_key(&reference.name) {
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

    for (name, definition) in definitions {
        let evidence_file = root.join("evidence").join(format!("{name}.json"));
        let references = by_definition.remove(name).unwrap_or_default();
        if definition.kind == DefinitionKind::Example
            && !references
                .iter()
                .any(|reference| reference.kind == ReferenceKind::Test)
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
        let inputs = evidence_input_ranges(root, &evidence_file, definitions);
        for reference in references {
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
    }
}

// {impl}`requirements.references.targets`
fn collect_references(root: &Path, errors: &mut Vec<String>) -> Vec<SourceReference> {
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
                let excluded = directory == root
                    && matches!(
                        entry.file_name().to_str(),
                        Some(".git" | "target" | "book" | "evidence")
                    );
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
    for file in files {
        let content = match fs::read_to_string(&file) {
            Ok(content) => content,
            Err(error) if error.kind() == io::ErrorKind::InvalidData => continue,
            Err(error) => {
                errors.push(format!(
                    "{}: cannot read reference source: {error}",
                    display_path(root, &file)
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
    references
}

// {impl}`requirements.references.evidence_coverage`
fn evidence_input_ranges(
    root: &Path,
    path: &Path,
    definitions: &BTreeMap<String, Declaration>,
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
        .filter_map(|selector| selector.resolve(root, definitions).ok())
        .flat_map(|resolved| {
            resolved
                .ranges
                .into_iter()
                .filter_map(|range| {
                    Some(InputRange {
                        file: fs::canonicalize(root.join(range.file)).ok()?,
                        line_start: range.line_start,
                        line_end: range.line_end,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

// {impl}`requirements.references.rust_function_attestations`
// {impl}`requirements.ci.orphans`
fn validate_rust_attestations(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    errors: &mut Vec<String>,
) {
    let mut expected_files = BTreeSet::new();
    for file in crate::rust_coverage::rust_files(root, errors) {
        let rust_file = display_path(root, &file);
        let source = match fs::read_to_string(&file) {
            Ok(source) => source,
            Err(error) => {
                errors.push(format!("{rust_file}: cannot read Rust source: {error}"));
                continue;
            }
        };
        let functions = match crate::rust_items::function_citations(&source, definitions) {
            Ok(functions) => functions,
            Err(_) => continue, // the Rust citation check reports this parse failure
        };
        if functions.is_empty() {
            continue;
        }
        let evidence_path = root.join("evidence/rust").join(format!("{rust_file}.json"));
        expected_files.insert(evidence_path.clone());
        validate_rust_attestation_file(
            root,
            &evidence_path,
            &rust_file,
            &functions,
            definitions,
            errors,
        );
    }
    let evidence_root = root.join("evidence/rust");
    if evidence_root.is_dir() {
        for file in files_with_extension(root, &evidence_root, "json", errors) {
            if !expected_files.contains(&file) {
                errors.push(format!(
                    "{}: orphaned Rust attestation file",
                    display_path(root, &file)
                ));
            }
        }
    }
}

// {impl}`requirements.references.rust_function_attestations`
// {impl}`requirements.ci.schema`
// {impl}`requirements.ci.determination`
fn validate_rust_attestation_file(
    root: &Path,
    path: &Path,
    rust_file: &str,
    functions: &[(crate::rust_items::RustItem, BTreeSet<(String, String)>)],
    definitions: &BTreeMap<String, Declaration>,
    errors: &mut Vec<String>,
) {
    let label = display_path(root, path);
    let purpose =
        "review each function and confirm it cites every applicable requirement or named example";
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            for (function, _) in functions {
                errors.push(format!(
                    "{label}: {rust_file}:{} {}: missing Rust function attestation file; {purpose}",
                    function.declaration_line, function.selector
                ));
            }
            return;
        }
        Err(error) => {
            errors.push(format!(
                "{label}: cannot read Rust attestation: {error}; {purpose}"
            ));
            return;
        }
    };
    let value: Value = match serde_json::from_str(&source) {
        Ok(value) => value,
        Err(error) => {
            errors.push(format!("{label}: invalid JSON: {error}; {purpose}"));
            return;
        }
    };
    let Some(object) = value.as_object() else {
        errors.push(format!("{label}: expected a JSON object; {purpose}"));
        return;
    };
    check_keys(object, &["rust_file", "functions"], &label, errors);
    if let Some(recorded_file) = string_field(object, "rust_file", &label, errors)
        && recorded_file != rust_file
    {
        errors.push(format!(
            "{label}: rust_file is {recorded_file:?}, expected {rust_file:?}"
        ));
    }
    let Some(entries) = array_field(object, "functions", &label, errors) else {
        return;
    };
    let expected = functions
        .iter()
        .map(|(function, citations)| (function.selector.as_str(), (function, citations)))
        .collect::<BTreeMap<_, _>>();
    let mut present = BTreeSet::new();
    for (index, entry) in entries.iter().enumerate() {
        let entry_label = format!("{label}: functions[{index}]");
        let Some(entry) = entry.as_object() else {
            errors.push(format!("{entry_label}: expected a JSON object"));
            continue;
        };
        check_keys(
            entry,
            &[
                "rust_item",
                "citations",
                "determination",
                "rationale",
                "inputs",
            ],
            &entry_label,
            errors,
        );
        let Some(selector) = string_field(entry, "rust_item", &entry_label, errors) else {
            continue;
        };
        let current = expected.get(selector);
        let location = current.map_or_else(
            || format!("{rust_file} {selector}"),
            |(function, _)| {
                format!(
                    "{rust_file}:{} {selector} ({})",
                    function.declaration_line, function.name
                )
            },
        );
        let entry_label = format!("{entry_label} ({location}; {purpose})");
        if !present.insert(selector.to_owned()) {
            errors.push(format!(
                "{entry_label}: duplicate Rust function attestation"
            ));
        }
        if current.is_none() {
            errors.push(format!("{entry_label}: orphaned Rust function attestation"));
        }
        let recorded_citations =
            array_field(entry, "citations", &entry_label, errors).map(|values| {
                let mut citations = BTreeSet::new();
                for (citation_index, value) in values.iter().enumerate() {
                    let citation_label = format!("{entry_label}: citations[{citation_index}]");
                    let Some(value) = value.as_object() else {
                        errors.push(format!("{citation_label}: expected a JSON object"));
                        continue;
                    };
                    check_keys(value, &["role", "name"], &citation_label, errors);
                    let role = string_field(value, "role", &citation_label, errors);
                    let name = string_field(value, "name", &citation_label, errors);
                    if let (Some(role), Some(name)) = (role, name) {
                        if !matches!(role, "impl" | "cite" | "test") {
                            errors
                                .push(format!("{citation_label}: invalid citation role {role:?}"));
                        }
                        if !definitions.contains_key(name) {
                            errors.push(format!("{citation_label}: unknown definition {name:?}"));
                        }
                        if !citations.insert((role.to_owned(), name.to_owned())) {
                            errors.push(format!("{citation_label}: duplicate citation"));
                        }
                    }
                }
                citations
            });
        if let (Some(recorded), Some((_, current))) = (recorded_citations.as_ref(), current)
            && recorded != *current
        {
            errors.push(format!(
                "{entry_label}: citation set changed; recorded {recorded:?}, current {current:?}"
            ));
        }
        if recorded_citations.as_ref().is_some_and(BTreeSet::is_empty) {
            errors.push(format!("{entry_label}: citations must not be empty"));
        }
        let determination = boolean_field(entry, "determination", &entry_label, errors);
        let rationale = string_field(entry, "rationale", &entry_label, errors);
        if rationale.is_some_and(|value| value.trim().is_empty()) {
            errors.push(format!("{entry_label}: rationale must not be empty"));
        }
        if determination == Some(false) {
            errors.push(format!(
                "{entry_label}: determination is false: {}",
                rationale.unwrap_or("")
            ));
        }
        let Some(inputs) = array_field(entry, "inputs", &entry_label, errors) else {
            continue;
        };
        let mut self_inputs = 0;
        let mut definition_inputs = 0;
        let mut matched_definitions = BTreeSet::new();
        for (input_index, input) in inputs.iter().enumerate() {
            let input_label = format!("{entry_label}: inputs[{input_index}]");
            let input_selector = InputSelector::from_input(input);
            let resolved = validate_input(root, definitions, input, &input_label, errors);
            match input_selector {
                Some(InputSelector::RustItem {
                    rust_file: selected_file,
                    rust_item,
                }) if selected_file == rust_file && rust_item == selector => {
                    self_inputs += 1;
                }
                Some(InputSelector::Requirements { .. }) => {
                    definition_inputs += 1;
                    if let Some(resolved) = resolved {
                        matched_definitions.extend(resolved.matches);
                    }
                }
                _ => {}
            }
        }
        if self_inputs != 1 {
            errors.push(format!(
                "{entry_label}: expected exactly one Rust-item input selecting this function, found {self_inputs}"
            ));
        }
        if definition_inputs == 0 {
            errors.push(format!(
                "{entry_label}: expected at least one requirements input"
            ));
        }
        if let Some((_, citations)) = current {
            for (_, name) in citations.iter() {
                if !matched_definitions.contains(name) {
                    errors.push(format!(
                        "{entry_label}: requirements inputs do not cover citation {name}"
                    ));
                }
            }
        }
    }
    for (selector, (function, _)) in expected {
        if !present.contains(selector) {
            errors.push(format!(
                "{label}: {rust_file}:{} {selector}: missing Rust function attestation",
                function.declaration_line
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use tempfile::TempDir;

    use super::validate_repository;

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

    // {test}`requirements.integration.report_api`
    // {test}`requirements.ci.book_source`
    // {test}`requirements.ci.coverage`
    // {test}`requirements.ci.requirement_text`
    // {test}`requirements.ci.inputs`
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
                "inputs": [{
                    "file": "src/main.rs",
                    "line_start": 1,
                    "line_end": 2,
                    "text": ["// first line", "// second line"]
                }]
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
                        "line_start": 1,
                        "line_end": 1,
                        "text": ["changed line"]
                    },
                    {
                        "file": "../outside",
                        "line_start": 0,
                        "line_end": 2,
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
            "input text changed",
            "- changed line",
            "+ // first line",
            "requirement must be a string",
            "missing field \"determination\"",
            "inputs must be an array",
            "file must be a path relative to the repository",
            "line_start must be a positive integer",
            "text[0] must be a string",
            "duplicate requirement requirements.one; first declared at md/requirements.md:3",
            "requirement requirements.empty has no text",
        ] {
            assert!(errors.contains(detail), "missing {detail:?} in:\n{errors}");
        }
    }

    // {test}`requirements.references.evidence_coverage`
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
                    {
                        "file": "src/main.rs",
                        "line_start": 1,
                        "line_end": 2,
                        "text": source.lines().collect::<Vec<_>>()
                    },
                    {
                        "file": "md/notes.md",
                        "line_start": 3,
                        "line_end": 3,
                        "text": [test]
                    }
                ]
            })
            .to_string(),
        )
        .unwrap();

        let report = validate_repository(temp.path()).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
    }

    // {test}`requirements.references.targets`
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
        fs::write(temp.path().join("src/main.rs"), &source).unwrap();
        fs::write(
            temp.path().join("evidence/foo.bar.ready.json"),
            json!({
                "requirement": "Ready.",
                "determination": true,
                "rationale": "Only the first line was examined.",
                "inputs": [{
                    "file": "src/main.rs",
                    "line_start": 1,
                    "line_end": 1,
                    "text": [source.lines().next().unwrap()]
                }]
            })
            .to_string(),
        )
        .unwrap();

        let report = validate_repository(temp.path()).unwrap();
        let errors = report.errors.join("\n");
        assert_eq!(report.errors.len(), 2, "{errors}");
        assert!(
            errors.contains(&format!(
                "src/main.rs:3: {unknown} has no matching requirement"
            )),
            "{errors}"
        );
        assert!(
            errors.contains(&format!("no input covers {known} at src/main.rs:2")),
            "{errors}"
        );
    }

    // {test}`requirements.ci.span`
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
    // {test}`requirements.references.attestation_reassessment`
    // {test}`requirements.ci.requirements_inputs`
    // {test}`requirements.ci.rust_item_inputs`
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
                "inputs": [{
                    "file": "md/rules.md",
                    "line_start": 8,
                    "line_end": 8,
                    "text": [chapter.lines().nth(7).unwrap()]
                }]
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
                "inputs": [{
                    "file": "src/main.rs",
                    "line_start": 1,
                    "line_end": 3,
                    "text": test_source.lines().collect::<Vec<_>>()
                }]
            })
            .to_string(),
        )
        .unwrap();
        let rust_evidence = temp.path().join("evidence/rust/src/main.rs.json");
        fs::create_dir_all(rust_evidence.parent().unwrap()).unwrap();
        fs::write(
            &rust_evidence,
            json!({
                "rust_file": "src/main.rs",
                "functions": [{
                    "rust_item": "fn[checks_case]",
                    "citations": [{"role": "test", "name": "rules.case"}],
                    "determination": true,
                    "rationale": "The sole function is a test of the named case. Its test citation names every behavior it asserts; the case cites the ready requirement.",
                    "inputs": [
                        {
                            "rust_file": "src/main.rs",
                            "rust_item": "fn[checks_case]",
                            "text": test_source.lines().collect::<Vec<_>>()
                        },
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
                }]
            })
            .to_string(),
        )
        .unwrap();

        let report = validate_repository(temp.path()).unwrap();
        assert_eq!(report.requirements, 1);
        assert_eq!(report.examples, 1);
        assert_eq!(report.evidence_files, 3);
        assert!(report.errors.is_empty(), "{:?}", report.errors);

        let rust_original = fs::read_to_string(&rust_evidence).unwrap();
        let mut bad_citations: serde_json::Value = serde_json::from_str(&rust_original).unwrap();
        bad_citations["functions"][0]["citations"] = json!([]);
        fs::write(&rust_evidence, bad_citations.to_string()).unwrap();
        let errors = validate_repository(temp.path()).unwrap().errors.join("\n");
        assert!(errors.contains("citation set changed"), "{errors}");
        assert!(errors.contains("citations must not be empty"), "{errors}");
        let mut duplicate: serde_json::Value = serde_json::from_str(&rust_original).unwrap();
        let copy = duplicate["functions"][0].clone();
        duplicate["functions"].as_array_mut().unwrap().push(copy);
        fs::write(&rust_evidence, duplicate.to_string()).unwrap();
        let errors = validate_repository(temp.path()).unwrap().errors.join("\n");
        assert!(
            errors.contains("duplicate Rust function attestation"),
            "{errors}"
        );
        fs::remove_file(&rust_evidence).unwrap();
        let errors = validate_repository(temp.path()).unwrap().errors.join("\n");
        assert!(
            errors.contains("missing Rust function attestation file"),
            "{errors}"
        );
        fs::write(&rust_evidence, rust_original).unwrap();
        let orphan = temp.path().join("evidence/rust/src/removed.rs.json");
        fs::write(&orphan, "{}").unwrap();
        let errors = validate_repository(temp.path()).unwrap().errors.join("\n");
        assert!(
            errors.contains("orphaned Rust attestation file"),
            "{errors}"
        );
        fs::remove_file(orphan).unwrap();

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
            errors.contains("input text changed for RustItem"),
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
            "example rules.empty must cite at least one requirement",
            "example rules.unclosed is missing a closing colon fence",
            "example rules.case has a true determination but no {test} reference",
        ] {
            assert!(errors.contains(detail), "missing {detail:?} in:\n{errors}");
        }
    }
}

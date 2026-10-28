use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::Path;

use serde_json::{Value, json};

use crate::DefinitionKind;
use crate::evidence::{Declaration, inspect_repository};
use crate::inputs::{InputSelector, read_project_file};
use crate::rust_items::{function_citations, parse_items};

// {impl}`requirements.cli.add_input`
pub enum InputTarget {
    Requirement(String),
    Example(String),
    RustFunction {
        rust_file: String,
        rust_item: String,
    },
}

// {impl}`requirements.cli.add_input`
pub enum InputMode {
    FileRange(String),
    RustItemsAtLine(String),
    Requirements(String),
}

// {impl}`requirements.cli.add_input`
// {impl}`requirements.cli.add_input_file_range`
// {impl}`requirements.cli.add_input_rust_item`
// {impl}`requirements.cli.add_input_requirements`
pub fn add_input(root: &Path, target: InputTarget, mode: InputMode) -> io::Result<String> {
    let root = fs::canonicalize(root)?;
    let inspection = inspect_repository(&root)?;
    let definitions = &inspection.definitions;
    let selectors = selectors_for_mode(&root, mode)?;
    let mut inputs = Vec::new();
    for selector in selectors {
        let resolved = selector
            .resolve(&root, definitions)
            .map_err(invalid_input)?;
        inputs.push((
            selector.clone(),
            selector.to_input(&resolved),
            resolved.matches,
        ));
    }

    match target {
        InputTarget::Requirement(name) => add_definition_inputs(
            &root,
            definitions,
            &name,
            DefinitionKind::Requirement,
            inputs,
        ),
        InputTarget::Example(name) => {
            add_definition_inputs(&root, definitions, &name, DefinitionKind::Example, inputs)
        }
        InputTarget::RustFunction {
            rust_file,
            rust_item,
        } => add_function_inputs(&root, definitions, &rust_file, &rust_item, inputs),
    }
}

// {impl}`requirements.cli.add_input`
// {impl}`requirements.cli.add_input_file_range`
// {impl}`requirements.cli.add_input_rust_item`
// {impl}`requirements.cli.add_input_requirements`
fn selectors_for_mode(root: &Path, mode: InputMode) -> io::Result<Vec<InputSelector>> {
    match mode {
        InputMode::FileRange(specification) => {
            let (file, range) = specification.rsplit_once(':').ok_or_else(|| {
                invalid_input("file range must have the form <file>:<start>-<end>")
            })?;
            let (start, end) = range.split_once('-').ok_or_else(|| {
                invalid_input("file range must have the form <file>:<start>-<end>")
            })?;
            let line_start = start
                .parse::<usize>()
                .map_err(|_| invalid_input("file range start must be a positive integer"))?;
            let line_end = end
                .parse::<usize>()
                .map_err(|_| invalid_input("file range end must be a positive integer"))?;
            Ok(vec![InputSelector::FileRange {
                file: file.to_owned(),
                line_start,
                line_end,
            }])
        }
        InputMode::RustItemsAtLine(specification) => {
            let (rust_file, line) = specification.rsplit_once(':').ok_or_else(|| {
                invalid_input("Rust item location must have the form <rust-file>:<line>")
            })?;
            let line = line
                .parse::<usize>()
                .map_err(|_| invalid_input("Rust item line must be a positive integer"))?;
            if !rust_file.ends_with(".rs") {
                return Err(invalid_input("Rust item location requires a .rs file"));
            }
            let (path, source) = read_project_file(root, rust_file).map_err(invalid_input)?;
            let rust_file = path
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .ok_or_else(|| invalid_input("Rust file path must be UTF-8"))?;
            let items = parse_items(&source).map_err(|error| {
                invalid_input(format!("{rust_file}: cannot parse Rust source: {error}"))
            })?;
            let selected = items
                .into_iter()
                .filter(|item| item.declaration_line == line)
                .map(|item| InputSelector::RustItem {
                    rust_file: rust_file.to_owned(),
                    rust_item: item.selector,
                })
                .collect::<Vec<_>>();
            if selected.is_empty() {
                return Err(invalid_input(format!(
                    "{rust_file}:{line}: no Rust item is declared on this line"
                )));
            }
            Ok(selected)
        }
        InputMode::Requirements(requirements) => {
            Ok(vec![InputSelector::Requirements { requirements }])
        }
    }
}

// {impl}`requirements.cli.add_input`
fn add_definition_inputs(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    name: &str,
    kind: DefinitionKind,
    inputs: Vec<(InputSelector, Value, Vec<String>)>,
) -> io::Result<String> {
    let Some(definition) = definitions.get(name) else {
        return Err(invalid_input(format!("unknown {} {name:?}", kind.noun())));
    };
    if definition.kind != kind {
        return Err(invalid_input(format!(
            "{name:?} is an {}, not a {}",
            definition.kind.noun(),
            kind.noun()
        )));
    }
    let path = root.join("evidence").join(format!("{name}.json"));
    let mut record = read_or_new(
        &path,
        json!({
            kind.noun(): definition.text,
            "determination": false,
            "rationale": "Review pending: inspect the definition, behavior, and supporting inputs.",
            "inputs": [],
        }),
    )?;
    let output = update_inputs(&mut record, inputs)?;
    if output.1 {
        fs::create_dir_all(root.join("evidence"))?;
        fs::write(
            &path,
            format!("{}\n", serde_json::to_string_pretty(&record)?),
        )?;
    }
    Ok(format!("evidence/{name}.json:\n{}", output.0))
}

// {impl}`requirements.cli.add_input`
// {impl}`requirements.references.rust_function_attestations`
// {impl}`requirements.cli.function_attestation_guidance`
fn add_function_inputs(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    rust_file: &str,
    rust_item: &str,
    inputs: Vec<(InputSelector, Value, Vec<String>)>,
) -> io::Result<String> {
    let (path, source) = read_project_file(root, rust_file).map_err(invalid_input)?;
    let rust_file = path
        .strip_prefix(root)
        .unwrap()
        .to_str()
        .ok_or_else(|| invalid_input("Rust file path must be UTF-8"))?;
    let functions = function_citations(&source, definitions).map_err(|error| {
        invalid_input(format!("{rust_file}: cannot parse Rust source: {error}"))
    })?;
    let Some((_, citations)) = functions
        .into_iter()
        .find(|(item, _)| item.selector == rust_item)
    else {
        return Err(invalid_input(format!(
            "{rust_file}: no function matches Rust item {rust_item:?}"
        )));
    };
    let path = root.join("evidence/rust").join(format!("{rust_file}.json"));
    let mut sidecar = read_or_new(&path, json!({"rust_file": rust_file, "functions": []}))?;
    let Some(entries) = sidecar.get_mut("functions").and_then(Value::as_array_mut) else {
        return Err(invalid_input(format!(
            "{}: functions must be an array",
            path.display()
        )));
    };
    let existing_index = entries
        .iter()
        .position(|entry| entry.get("rust_item").and_then(Value::as_str) == Some(rust_item));
    let new_entry = existing_index.is_none();
    let index = existing_index.unwrap_or_else(|| {
        entries.push(json!({
            "rust_item": rust_item,
            "citations": citation_values(&citations),
            "determination": false,
            "rationale": "Review pending: inspect this function and every applicable requirement or example; confirm that its citations are complete and its behavior follows their combined texts.",
            "inputs": [],
        }));
        entries.len() - 1
    });
    let (output, changed) = update_inputs(&mut entries[index], inputs)?;
    if changed || new_entry {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(
            &path,
            format!("{}\n", serde_json::to_string_pretty(&sidecar)?),
        )?;
    }
    Ok(format!(
        "evidence/rust/{rust_file}.json {rust_item}:\n{output}\nThis attestation remains pending until an agent reviews the function and applicable requirement and example texts, confirms every applicable citation is present, and explains how the cited texts account for the function's behavior."
    ))
}

// {impl}`requirements.cli.add_input`
fn citation_values(citations: &BTreeSet<(String, String)>) -> Vec<Value> {
    citations
        .iter()
        .map(|(role, name)| json!({ "role": role, "name": name }))
        .collect()
}

// {impl}`requirements.cli.add_input`
fn update_inputs(
    record: &mut Value,
    inputs: Vec<(InputSelector, Value, Vec<String>)>,
) -> io::Result<(String, bool)> {
    let Some(object) = record.as_object_mut() else {
        return Err(invalid_input("evidence record must be a JSON object"));
    };
    let Some(existing) = object.get_mut("inputs").and_then(Value::as_array_mut) else {
        return Err(invalid_input("evidence inputs must be an array"));
    };
    let mut output = String::new();
    let mut changed = false;
    for (selector, input, matches) in inputs {
        let position = existing
            .iter()
            .position(|old| InputSelector::from_input(old).as_ref() == Some(&selector));
        let action = if let Some(position) = position {
            if existing[position] == input {
                "unchanged"
            } else {
                existing[position] = input;
                changed = true;
                "refreshed"
            }
        } else {
            existing.push(input);
            changed = true;
            "added"
        };
        output.push_str(&format!("- {action}: {selector:?}"));
        if !matches.is_empty() {
            output.push_str(&format!(" matching {}", matches.join(", ")));
        }
        output.push('\n');
    }
    if changed {
        object.insert("determination".to_owned(), Value::Bool(false));
        object.insert(
            "rationale".to_owned(),
            Value::String(
                "Review pending: reassess the changed input, all applicable definitions and citations, and the behavior they jointly specify.".to_owned(),
            ),
        );
    }
    Ok((output, changed))
}

// {impl}`requirements.cli.add_input`
fn read_or_new(path: &Path, fallback: Value) -> io::Result<Value> {
    match fs::read_to_string(path) {
        Ok(source) => serde_json::from_str(&source)
            .map_err(|error| invalid_input(format!("{}: invalid JSON: {error}", path.display()))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(fallback),
        Err(error) => Err(error),
    }
}

// {impl}`requirements.cli.add_input`
fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

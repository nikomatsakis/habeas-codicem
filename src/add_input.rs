use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use serde_json::{Value, json};

use crate::DefinitionKind;
use crate::evidence::{Declaration, inspect_repository};
use crate::inputs::{InputSelector, OutlineCache};

// {impl}`requirements.cli.add_input`
pub enum InputTarget {
    Requirement(String),
    Example(String),
}

// {impl}`requirements.cli.add_input`
pub enum InputMode {
    /// `<file>:<line>`: every outline node declared on that line.
    ItemsAtLine(String),
    Requirements(String),
}

// {impl}`requirements.cli.add_input`
// {impl}`requirements.cli.add_input_item`
// {impl}`requirements.cli.add_input_requirements`
pub fn add_input(root: &Path, target: InputTarget, mode: InputMode) -> io::Result<String> {
    let root = fs::canonicalize(root)?;
    let inspection = inspect_repository(&root)?;
    let definitions = &inspection.definitions;
    let outlines = OutlineCache::default();
    let selectors = selectors_for_mode(&root, &outlines, mode)?;
    let mut inputs = Vec::new();
    for selector in selectors {
        let resolved = selector
            .resolve(&root, definitions, &outlines)
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
            &outlines,
            &name,
            DefinitionKind::Requirement,
            inputs,
        ),
        InputTarget::Example(name) => add_definition_inputs(
            &root,
            definitions,
            &outlines,
            &name,
            DefinitionKind::Example,
            inputs,
        ),
    }
}

// {impl}`requirements.cli.add_input`
// {impl}`requirements.cli.add_input_item`
// {impl}`requirements.cli.add_input_requirements`
// {impl}`requirements.configuration.evidence_paths`
// {impl}`requirements.ci.schema`
fn selectors_for_mode(
    root: &Path,
    outlines: &OutlineCache,
    mode: InputMode,
) -> io::Result<Vec<InputSelector>> {
    match mode {
        InputMode::ItemsAtLine(specification) => {
            let (file, line) = specification
                .rsplit_once(':')
                .ok_or_else(|| invalid_input("item location must have the form <file>:<line>"))?;
            let line = line
                .parse::<usize>()
                .map_err(|_| invalid_input("item line must be a positive integer"))?;
            let parsed = outlines.parse(root, file);
            let parsed = parsed.as_ref().as_ref().map_err(invalid_input)?;
            let file = parsed
                .path
                .strip_prefix(root)
                .unwrap_or(&parsed.path)
                .to_str()
                .ok_or_else(|| invalid_input("file path must be UTF-8"))?;
            let selected = parsed
                .outline
                .declared_on_line(line)
                .into_iter()
                .map(|index| InputSelector::Outline {
                    file: file.to_owned(),
                    item: parsed.outline.nodes[index].selector.clone(),
                })
                .collect::<Vec<_>>();
            if selected.is_empty() {
                return Err(invalid_input(format!(
                    "{file}:{line}: no outline node is declared on this line"
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
// {impl}`requirements.definitions.capture`
fn add_definition_inputs(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    outlines: &OutlineCache,
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
    let output = update_inputs(
        &mut record,
        inputs,
        root,
        definitions,
        outlines,
        Some(&definition.text),
    )?;
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
// {impl}`requirements.definitions.capture`
// {impl}`requirements.cli.add_input_requirements`
// {impl}`requirements.definitions.dependencies`
// {impl}`requirements.cli.add_input_item`
fn update_inputs(
    record: &mut Value,
    inputs: Vec<(InputSelector, Value, Vec<String>)>,
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    outlines: &OutlineCache,
    subject: Option<&str>,
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
    let required = crate::terms::dependencies(root, definitions, outlines, object, subject)
        .map_err(invalid_input)?;
    let existing = object
        .get_mut("inputs")
        .and_then(Value::as_array_mut)
        .unwrap();
    for definition in required {
        let selector = InputSelector::Definition {
            definition: definition.clone(),
        };
        let resolved = selector
            .resolve(root, definitions, outlines)
            .map_err(invalid_input)?;
        let input = selector.to_input(&resolved);
        let positions: Vec<_> = existing
            .iter()
            .enumerate()
            .filter_map(|(i, old)| {
                (old.get("definition").and_then(Value::as_str) == Some(definition.as_str()))
                    .then_some(i)
            })
            .collect();
        if positions.len() != 1 || existing[positions[0]] != input {
            existing.retain(|old| {
                old.get("definition").and_then(Value::as_str) != Some(definition.as_str())
            });
            existing.push(input);
            changed = true;
            output.push_str(&format!("- captured: definition {definition}\n"));
        }
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

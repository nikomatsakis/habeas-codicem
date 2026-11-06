use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Map, Value};

use crate::evidence::Declaration;
use crate::inputs::{InputSelector, OutlineCache};
use crate::{DefinitionKind, ReferenceKind, reference_roles};

// {impl}`requirements.definitions.declaration`
pub(crate) fn declaration(text: &str) -> Result<(&str, &str), String> {
    let text = text.trim_start();
    let (option, body) = text.split_once('\n').unwrap_or((text, ""));
    let phrase = option
        .trim()
        .strip_prefix(":phrase:")
        .map(str::trim)
        .filter(|phrase| !phrase.is_empty())
        .ok_or_else(|| "expected one nonempty :phrase: option before the body".to_owned())?;
    let body = body.trim();
    if body.is_empty() {
        return Err("definition has no explanatory body".to_owned());
    }
    if body
        .lines()
        .next()
        .is_some_and(|line| line.trim_start().starts_with(":phrase:"))
    {
        return Err("definition has duplicate :phrase: options".to_owned());
    }
    Ok((phrase, body))
}

// {impl}`requirements.definitions.dependencies`
// {impl}`requirements.definitions.references`
fn referenced_terms(text: &str, markdown: bool, names: &mut BTreeSet<String>) {
    names.extend(
        reference_roles(text, markdown)
            .into_iter()
            .filter(|r| r.kind == ReferenceKind::Term)
            .map(|r| r.name.to_owned()),
    );
}

// {impl}`requirements.definitions.dependencies`
// {impl}`requirements.definitions.selection`
pub(crate) fn dependencies(
    root: &Path,
    declarations: &BTreeMap<String, Declaration>,
    outlines: &OutlineCache,
    record: &Map<String, Value>,
    subject: Option<&str>,
) -> Result<BTreeSet<String>, String> {
    let pending = direct_references(root, declarations, outlines, record, subject);
    let (required, problems) = close_dependencies(declarations, pending);
    match problems.into_iter().next() {
        Some(problem) => Err(problem),
        None => Ok(required),
    }
}

/// Terms referenced directly by the subject text or the record's inputs.
// {impl}`requirements.definitions.dependencies`
// {impl}`requirements.definitions.selection`
fn direct_references(
    root: &Path,
    declarations: &BTreeMap<String, Declaration>,
    outlines: &OutlineCache,
    record: &Map<String, Value>,
    subject: Option<&str>,
) -> BTreeSet<String> {
    let mut pending = BTreeSet::new();
    if let Some(text) = subject {
        referenced_terms(text, true, &mut pending);
    }
    if let Some(inputs) = record.get("inputs").and_then(Value::as_array) {
        for input in inputs {
            let Some(selector) = InputSelector::from_input(input) else {
                continue;
            };
            // Recorded outline inputs carry no text; read the node's current source.
            let text = match input.get("text").and_then(Value::as_array) {
                Some(lines) => lines
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n"),
                None => match selector.resolve(root, declarations, outlines) {
                    Ok(resolved) if matches!(selector, InputSelector::Outline { .. }) => {
                        resolved.text.join("\n")
                    }
                    _ => continue,
                },
            };
            match selector {
                InputSelector::Definition { definition } => {
                    pending.insert(definition);
                }
                InputSelector::Outline { file, .. } if file.ends_with(".rs") => {
                    for range in crate::rust_coverage::comments(&text) {
                        referenced_terms(&text[range], false, &mut pending);
                    }
                }
                InputSelector::Outline { file, .. } => {
                    referenced_terms(&text, file.ends_with(".md"), &mut pending)
                }
                InputSelector::Requirements { .. } => {
                    // Scan bodies, not the surrounding colon fences: Markdown code
                    // protection must be evaluated in the declaration's own context.
                    for block in crate::definition_blocks(&text) {
                        referenced_terms(
                            &text[block.body_start..block.body_end],
                            true,
                            &mut pending,
                        );
                    }
                }
            }
        }
    }
    pending
}

/// Expands referenced terms to a fixed point. Returns the resolvable
/// definitions and one problem per unresolvable term, continuing past each
/// problem so every one is found.
// {impl}`requirements.definitions.dependencies`
fn close_dependencies(
    declarations: &BTreeMap<String, Declaration>,
    mut pending: BTreeSet<String>,
) -> (BTreeSet<String>, Vec<String>) {
    let mut visited = BTreeSet::new();
    let mut required = BTreeSet::new();
    let mut problems = Vec::new();
    while let Some(name) = pending.pop_first() {
        if !visited.insert(name.clone()) {
            continue;
        }
        let Some(definition) = declarations
            .get(&name)
            .filter(|d| d.kind == DefinitionKind::Term)
        else {
            problems.push(format!("term {name}: no matching named definition"));
            continue;
        };
        match declaration(&definition.text) {
            Ok((_, body)) => referenced_terms(body, true, &mut pending),
            Err(problem) => problems.push(problem),
        }
        required.insert(name);
    }
    (required, problems)
}

// {impl}`requirements.definitions.selection`
// {impl}`requirements.definitions.dependencies`
pub(crate) fn validate_dependencies(
    root: &Path,
    declarations: &BTreeMap<String, Declaration>,
    outlines: &OutlineCache,
    record: &Map<String, Value>,
    subject: Option<&str>,
    label: &str,
    errors: &mut Vec<String>,
) {
    let pending = direct_references(root, declarations, outlines, record, subject);
    let (required, problems) = close_dependencies(declarations, pending);
    for problem in problems {
        errors.push(format!("{label}: definition dependencies: {problem}"));
    }
    for name in required {
        let count = record
            .get("inputs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|input| input.get("definition").and_then(Value::as_str) == Some(name.as_str()))
            .count();
        if count != 1 {
            errors.push(format!(
                "{label}: expected exactly one definition input for {name}, found {count}"
            ));
        }
    }
}

// {impl}`requirements.definitions.references`
// {impl}`requirements.definitions.declaration`
pub(crate) fn escaped_phrase(phrase: &str) -> String {
    let mut output = String::new();
    for ch in phrase.chars() {
        match ch {
            '*' | '_' | '`' | '[' | ']' | '\\' | '~' | '{' | '}' => {
                output.push_str(&format!("&#{};", u32::from(ch)))
            }
            _ => output.push_str(&crate::escape_html(&ch.to_string())),
        }
    }
    output
}

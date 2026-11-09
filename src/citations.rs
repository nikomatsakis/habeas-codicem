//! Citation reviews: an assessment, for every source item outside the book
//! that carries a `{cite}` reference, of whether its citations are sensible.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::ReferenceKind;
use crate::evidence::{
    Declaration, InspectionProgress, SourceReference, array_field, check_keys, display_path,
    string_field, validate_input,
};
use crate::inputs::{InputSelector, OutlineCache};
use crate::judgment::Judgment;

/// The directory under `evidence/` holding citation reviews.
// {impl}`requirements.references.citation_review`
pub(crate) const CITATIONS_DIR: &str = "citations";

/// The inspection phase that validates citation reviews.
// {impl}`requirements.authoring.progress`
const CITATION_STAGE: &str = "citation reviews";

/// One citation of a reviewed item: its role, name, and fenced commentary,
/// which is empty for an inline reference.
// {impl}`requirements.references.citation_review`
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Citation {
    pub(crate) role: &'static str,
    pub(crate) name: String,
    pub(crate) commentary: String,
}

impl Citation {
    // {impl}`requirements.references.citation_review`
    pub(crate) fn to_value(&self) -> Value {
        json!({"role": self.role, "name": self.name, "commentary": self.commentary})
    }
}

/// A source item outside the book carrying at least one `{cite}` reference.
// {impl}`requirements.references.citation_review`
#[derive(Clone, Debug)]
pub(crate) struct CitationItem {
    /// Project-relative path.
    pub(crate) file: String,
    /// Outline selector; empty for the root node.
    pub(crate) item: String,
    /// Line of the item's first citation.
    pub(crate) line: usize,
    /// Citations in source order.
    pub(crate) citations: Vec<Citation>,
}

impl CitationItem {
    /// The review's path relative to the project root.
    // {impl}`requirements.references.citation_review`
    pub(crate) fn review_path(&self) -> String {
        review_path(&self.file, &self.item)
    }

    /// The selector as displayed, `<root>` for the root node.
    // {impl}`requirements.references.citation_review`
    // {impl}`requirements.authoring.citation_prompt`
    pub(crate) fn selector_label(&self) -> &str {
        if self.item.is_empty() {
            "<root>"
        } else {
            &self.item
        }
    }

    /// The citations as JSON, in source order.
    // {impl}`requirements.references.citation_review`
    // {impl}`requirements.authoring.citation_prompt`
    pub(crate) fn citations_value(&self) -> Value {
        Value::Array(self.citations.iter().map(Citation::to_value).collect())
    }

    /// The citations as the model receives them, with each commentary redacted.
    // {impl}`requirements.authoring.citation_prompt`
    // {impl}`requirements.authoring.redaction`
    pub(crate) fn model_citations_value(&self) -> Value {
        let mut value = self.citations_value();
        for citation in value.as_array_mut().into_iter().flatten() {
            if let Some(commentary) = citation.get_mut("commentary")
                && let Some(text) = commentary.as_str()
            {
                *commentary = Value::String(crate::redaction::redact_text(text).into_owned());
            }
        }
        value
    }
}

/// `evidence/citations/<file>/<selector>.json`, with `/` and `%` in the
/// selector percent-encoded and `_root` for the root node.
// {impl}`requirements.references.citation_review`
pub(crate) fn review_path(file: &str, item: &str) -> String {
    format!(
        "evidence/{CITATIONS_DIR}/{file}/{}.json",
        encoded_selector(item)
    )
}

/// A selector as a review file name: `/` and `%` percent-encoded, and
/// `_root` for the root node.
// {impl}`requirements.references.citation_review`
// {impl}`requirements.references.site_review`
pub(crate) fn encoded_selector(item: &str) -> String {
    if item.is_empty() {
        "_root".to_owned()
    } else {
        item.replace('%', "%25").replace('/', "%2F")
    }
}

/// The single-subject authoring name of a citation review,
/// `citations:<file>#<selector>`.
// {impl}`requirements.references.citation_review`
pub(crate) fn subject_label(file: &str, item: &str) -> String {
    format!("citations:{file}#{item}")
}

/// The source items outside the book whose citations need review, keyed by
/// file and selector. Citations that do not resolve are reported by
/// reference validation and are not reviewed.
// {impl}`requirements.references.citation_review`
// {impl}`requirements.references.outline_comments`
// {impl}`requirements.references.builtin_definitions`
// {impl}`requirements.ci.errors`
pub(crate) fn citation_items(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    references: &[SourceReference],
    outlines: &OutlineCache,
) -> BTreeMap<(String, String), CitationItem> {
    let mut items = BTreeMap::<(String, String), CitationItem>::new();
    for reference in references {
        if reference.kind != ReferenceKind::Cite || reference.in_book || !reference.closed {
            continue;
        }
        let resolves = crate::builtins::builtin(&reference.name).is_some_and(|builtin| {
            builtin
                .check(reference.kind, reference.commentary.as_deref())
                .is_ok()
        }) || definitions
            .get(&reference.name)
            .is_some_and(|definition| definition.kind != crate::DefinitionKind::Term);
        if !resolves {
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
            // outlined is reported elsewhere and is reviewed as a whole file.
            _ => String::new(),
        };
        let entry = items
            .entry((file.clone(), item.clone()))
            .or_insert_with(|| CitationItem {
                file,
                item,
                line: reference.line,
                citations: Vec::new(),
            });
        entry.citations.push(Citation {
            role: reference.kind.label(),
            name: reference.name.clone(),
            commentary: reference.commentary.clone().unwrap_or_default(),
        });
    }
    items
}

/// Whether two citation lists are the same, comparing commentary by its
/// words so reflowing it does not change the review.
// {impl}`requirements.references.citation_review`
fn same_citations(recorded: &[Citation], current: &[Citation]) -> bool {
    let key = |citations: &[Citation]| {
        let mut keys = citations
            .iter()
            .map(|citation| {
                (
                    citation.role.to_owned(),
                    citation.name.clone(),
                    citation
                        .commentary
                        .split_whitespace()
                        .map(str::to_owned)
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        keys.sort();
        keys
    };
    key(recorded) == key(current)
}

/// Validates citation reviews: each current item needs a current, accepted
/// review, and every review file must belong to a current item. `files` are
/// the review files already listed with the rest of `evidence/`, so a listing
/// failure there is reported once.
/// Reports progress as the "citation reviews" phase: an activity display
/// while items are discovered, then one check per item and review file.
// {impl}`requirements.references.citation_review`
// {impl}`requirements.ci.outcomes`
// {impl}`requirements.ci.determination`
// {impl}`requirements.authoring.progress`
// {impl}`requirements.ci.errors`
pub(crate) fn validate(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    references: &[SourceReference],
    outlines: &OutlineCache,
    files: &[PathBuf],
    errors: &mut Vec<String>,
    progress: &mut impl FnMut(InspectionProgress),
) {
    progress(InspectionProgress {
        stage: CITATION_STAGE,
        completed: 0,
        total: None,
    });
    let items = citation_items(root, definitions, references, outlines);
    let total = items.len() + files.len();
    let mut completed = 0;
    let mut advance = |progress: &mut dyn FnMut(InspectionProgress)| {
        progress(InspectionProgress {
            stage: CITATION_STAGE,
            completed,
            total: Some(total),
        });
        completed += 1;
    };
    advance(progress);
    let mut expected = BTreeSet::new();
    for item in items.values() {
        let path = item.review_path();
        expected.insert(root.join(&path));
        if root.join(&path).exists() {
            validate_review(root, &path, item, definitions, outlines, errors);
        } else {
            errors.push(format!(
                "{path}: missing citation review for {} {} at {}:{}",
                item.file,
                item.selector_label(),
                item.file,
                item.line
            ));
        }
        advance(progress);
    }
    for file in files {
        if !expected.contains(file) {
            errors.push(format!(
                "{}: orphaned citation review: no source item outside the book with {{cite}} references matches it",
                display_path(root, file)
            ));
        }
        advance(progress);
    }
}

/// Validates one review file against its current item.
// {impl}`requirements.references.citation_review`
// {impl}`requirements.ci.outcomes`
// {impl}`requirements.ci.diagnostics`
// {impl}`requirements.ci.errors`
fn validate_review(
    root: &Path,
    label: &str,
    item: &CitationItem,
    definitions: &BTreeMap<String, Declaration>,
    outlines: &OutlineCache,
    errors: &mut Vec<String>,
) {
    let content = match fs::read_to_string(root.join(label)) {
        Ok(content) => content,
        Err(error) => {
            errors.push(format!("{label}: cannot read citation review: {error}"));
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
    check_keys(
        object,
        &[
            "file",
            "item",
            "citations",
            "determination",
            "rationale",
            "inputs",
        ],
        label,
        errors,
    );
    if let Some(file) = string_field(object, "file", label, errors)
        && file != item.file
    {
        errors.push(format!("{label}: file must be {:?}", item.file));
    }
    if let Some(selector) = string_field(object, "item", label, errors)
        && selector != item.item
    {
        errors.push(format!("{label}: item must be {:?}", item.item));
    }
    if let Some(recorded) = array_field(object, "citations", label, errors) {
        let parsed = recorded
            .iter()
            .enumerate()
            .map(|(index, citation)| {
                parse_citation(citation, &format!("{label}: citations[{index}]"), errors)
            })
            .collect::<Option<Vec<_>>>();
        if let Some(parsed) = parsed
            && !same_citations(&parsed, &item.citations)
        {
            errors.push(format!(
                "{label}: citations changed for {} {}: recorded {}, now {}",
                item.file,
                item.selector_label(),
                Value::Array(parsed.iter().map(Citation::to_value).collect()),
                item.citations_value()
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
    let rationale = string_field(object, "rationale", label, errors);
    if rationale.is_some_and(|text| text.trim().is_empty()) {
        errors.push(format!("{label}: rationale must not be empty"));
    }
    // A citation review judges citations, which no `{fixme}` acknowledges.
    if matches!(determination, Some(Judgment::Fixme(_))) {
        errors.push(format!(
            "{label}: determination is FIXME, but a citation review cannot accept a {{fixme}}; it must be true or false"
        ));
    }
    if determination == Some(Judgment::False) {
        errors.push(format!(
            "{label}: determination is false{}",
            rationale
                .filter(|text| !text.trim().is_empty())
                .map_or(String::new(), |text| format!(": {}", text.trim()))
        ));
    }
    let Some(inputs) = array_field(object, "inputs", label, errors) else {
        return;
    };
    let mut selects_item = false;
    let mut cited = BTreeSet::new();
    for (index, input) in inputs.iter().enumerate() {
        validate_input(
            root,
            definitions,
            outlines,
            input,
            &format!("{label}: inputs[{index}]"),
            errors,
        );
        match InputSelector::from_input(input) {
            Some(InputSelector::Outline {
                file,
                item: selector,
            }) => {
                selects_item |= file == item.file && selector == item.item;
            }
            Some(InputSelector::Requirements { .. }) => {
                cited.extend(
                    input
                        .get("matches")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_owned),
                );
            }
            _ => {}
        }
    }
    if !selects_item {
        errors.push(format!(
            "{label}: inputs must select the reviewed item {} {}",
            item.file,
            item.selector_label()
        ));
    }
    for citation in &item.citations {
        if crate::builtins::builtin(&citation.name).is_none() && !cited.contains(&citation.name) {
            errors.push(format!(
                "{label}: inputs must include the cited declaration {}",
                citation.name
            ));
        }
    }
}

/// Parses one recorded citation.
// {impl}`requirements.references.citation_review`
// {impl}`requirements.ci.errors`
fn parse_citation(value: &Value, label: &str, errors: &mut Vec<String>) -> Option<Citation> {
    let Some(object) = value.as_object() else {
        errors.push(format!("{label}: expected a JSON object"));
        return None;
    };
    check_keys(object, &["role", "name", "commentary"], label, errors);
    let role = string_field(object, "role", label, errors).and_then(|role| match role {
        "cite" => Some("cite"),
        "impl" => Some("impl"),
        "test" => Some("test"),
        other => {
            errors.push(format!("{label}: unknown role {other:?}"));
            None
        }
    });
    // Every field is checked, so one record reports each malformed field.
    let name = string_field(object, "name", label, errors);
    let commentary = string_field(object, "commentary", label, errors);
    Some(Citation {
        role: role?,
        name: name?.to_owned(),
        commentary: commentary?.to_owned(),
    })
}

/// Whether a path lies in the citation review directory.
// {impl}`requirements.references.citation_review`
// {impl}`requirements.ci.orphans`
pub(crate) fn is_review_path(evidence_root: &Path, path: &Path) -> bool {
    path.strip_prefix(evidence_root)
        .ok()
        .and_then(|relative| relative.components().next())
        .is_some_and(|first| first.as_os_str() == CITATIONS_DIR)
}

/// The `habeas:none` citations outside the book, as file, selector, line,
/// and justification.
// {impl}`requirements.references.no_requirement`
pub(crate) fn none_citations(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    references: &[SourceReference],
    outlines: &OutlineCache,
) -> Vec<(CitationItem, String)> {
    let mut found = Vec::new();
    for item in citation_items(root, definitions, references, outlines).into_values() {
        for citation in &item.citations {
            if citation.name == "habeas:none" {
                found.push((
                    item.clone(),
                    crate::fenced::justification(&citation.commentary),
                ));
            }
        }
    }
    found.sort_by(|left, right| (&left.0.file, left.0.line).cmp(&(&right.0.file, right.0.line)));
    found
}

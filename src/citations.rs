//! Citation reviews of source items and opted-in explanatory book passages.

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
pub(crate) const CITATIONS_DIR: &str = "citations";

/// The inspection phase that validates citation reviews.
// {impl}`requirements.authoring.progress`
const CITATION_STAGE: &str = "citation reviews";

// {impl}`requirements.references.citation_review`
/// One citation of a reviewed item: its role, name, and fenced commentary,
/// which is empty for an inline reference.
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

// {cite}`habeas:util`
/// A source item or book passage carrying citations that need review.
#[derive(Clone, Debug)]
pub(crate) struct CitationItem {
    pub(crate) review_kind: ReviewKind,
    /// Project-relative path.
    pub(crate) file: String,
    /// Outline selector; empty for the root node.
    pub(crate) item: String,
    /// Line of the item's first citation.
    pub(crate) line: usize,
    /// Citations in source order.
    pub(crate) citations: Vec<Citation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReviewKind {
    SourceItem,
    BookCommentary,
}

impl ReviewKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::SourceItem => "source_item",
            Self::BookCommentary => "book_commentary",
        }
    }
}

impl CitationItem {
    // {cite}`habeas:util`
    /// The review's path relative to the project root.
    pub(crate) fn review_path(&self) -> String {
        review_path(&self.file, &self.item)
    }

    // {cite}`habeas:util`
    /// The selector as displayed, `<root>` for the root node.
    pub(crate) fn selector_label(&self) -> &str {
        if self.item.is_empty() {
            "<root>"
        } else {
            &self.item
        }
    }

    // {impl}`requirements.references.citation_review`
    /// The citations as JSON, in source order.
    pub(crate) fn citations_value(&self) -> Value {
        Value::Array(self.citations.iter().map(Citation::to_value).collect())
    }

    // {cite}`habeas:util`
    /// The citations as the model receives them, with each commentary redacted.
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

// {impl}`requirements.references.citation_review`
/// `evidence/citations/<file>/<selector>.json`, with `/` and `%` in the
/// selector percent-encoded and `_root` for the root node.
pub(crate) fn review_path(file: &str, item: &str) -> String {
    format!(
        "evidence/{CITATIONS_DIR}/{file}/{}.json",
        encoded_selector(item)
    )
}

// {impl}`requirements.references.citation_review`
/// A selector as a review file name: `/` and `%` percent-encoded, and
/// `_root` for the root node.
pub(crate) fn encoded_selector(item: &str) -> String {
    if item.is_empty() {
        "_root".to_owned()
    } else {
        item.replace('%', "%25").replace('/', "%2F")
    }
}

/// The single-subject authoring name of a citation review,
/// `citations:<file>#<selector>`.
// {cite}`habeas:util`
pub(crate) fn subject_label(file: &str, item: &str) -> String {
    format!("citations:{file}#{item}")
}

/// Discover source items and the outermost opted-in book commentary fences.
/// Unresolvable citations are reported by reference validation.
// {impl}`requirements.references.outline_comments`
// {impl}`requirements.references.builtin_definitions`
// {impl}`requirements.references.util`
// {impl}`requirements.references.citation_review.book_commentary`
pub(crate) fn citation_items(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    references: &[SourceReference],
    outlines: &OutlineCache,
) -> BTreeMap<(String, String), CitationItem> {
    let resolves = |reference: &SourceReference| {
        crate::builtins::builtin(&reference.name)
            .is_some_and(|builtin| builtin.check(reference.kind).is_ok())
            || definitions
                .get(&reference.name)
                .is_some_and(|definition| definition.kind != crate::DefinitionKind::Term)
    };
    // Most book citations live in declarations or ordinary unfenced prose.
    // Outline only files containing a possible opted-in commentary fence.
    let commentary_files = references
        .iter()
        .filter(|r| {
            r.in_book
                && !r.in_declaration
                && r.closed
                && r.kind == ReferenceKind::Cite
                && r.commentary.is_some()
        })
        .map(|r| &r.file)
        .collect::<BTreeSet<_>>();
    let mut items = BTreeMap::<(String, String), CitationItem>::new();
    // One outline read per file. Each range names a whole commentary container.
    let mut passages = BTreeMap::<String, Vec<(usize, usize, String)>>::new();
    for reference in references {
        if reference.kind != ReferenceKind::Cite
            || !reference.closed
            || !resolves(reference)
            || (reference.in_book
                && (reference.in_declaration || !commentary_files.contains(&reference.file)))
        {
            continue;
        }
        let file = display_path(root, &reference.file);
        let parsed = outlines.parse(root, &file);
        let (item, review_kind) = if reference.in_book {
            let ranges = passages.entry(file.clone()).or_insert_with(|| {
                let Ok(parsed) = parsed.as_ref() else {
                    return Vec::new();
                };
                let declarations = crate::definition_blocks(&parsed.source);
                crate::fenced::markdown_fences(&parsed.source, true)
                    .into_iter()
                    .filter(|fence| fence.closed && fence.kind == ReferenceKind::Cite)
                    .filter(|fence| {
                        !declarations.iter().any(|block| {
                            block.kind != crate::DefinitionKind::Term
                                && block.start < fence.start
                                && fence.start < block.body_end
                        })
                    })
                    .filter_map(|fence| {
                        let end = parsed.source[..fence.end]
                            .bytes()
                            .filter(|&b| b == b'\n')
                            .count()
                            + 1;
                        // Utility-only commentary is exempt, including nested citations.
                        let eligible = references.iter().any(|r| {
                            r.file == reference.file
                                && r.kind == ReferenceKind::Cite
                                && r.closed
                                && !r.in_declaration
                                && r.line >= fence.line
                                && r.line < end
                                && r.name != "habeas:util"
                                && resolves(r)
                        });
                        eligible.then(|| {
                            let owner = parsed.reference_owner(fence.line, fence.column);
                            (
                                fence.line,
                                end,
                                parsed.outline.nodes[owner].selector.clone(),
                            )
                        })
                    })
                    .collect()
            });
            // The earliest containing opener owns nested commentary citations.
            let Some((_, _, item)) = ranges
                .iter()
                .filter(|(start, end, _)| *start <= reference.line && reference.line < *end)
                .min_by_key(|(start, _, _)| *start)
            else {
                continue;
            };
            (item.clone(), ReviewKind::BookCommentary)
        } else {
            let item = match parsed.as_ref() {
                Ok(parsed) if parsed.outline.language != "plain-text" => {
                    let index = parsed.reference_owner(reference.line, reference.column);
                    parsed.outline.nodes[index].selector.clone()
                }
                _ => String::new(),
            };
            (item, ReviewKind::SourceItem)
        };
        let entry = items
            .entry((file.clone(), item.clone()))
            .or_insert_with(|| CitationItem {
                review_kind,
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
    items.retain(|_, item| {
        item.citations
            .iter()
            .any(|citation| citation.name != "habeas:util")
    });
    items
}

/// Whether two citation lists are the same, comparing commentary by its
/// words so reflowing it does not change the review.
// {cite}`habeas:util`
pub(crate) fn same_citations(recorded: &[Citation], current: &[Citation]) -> bool {
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
// {impl}`requirements.ci.outcomes`
// {impl}`requirements.ci.determination`
// {impl}`requirements.ci.errors`
pub(crate) fn validate(
    domain: &crate::sdk::HabeasDomain,
    references: &[SourceReference],
    files: &[PathBuf],
    errors: &mut Vec<String>,
    progress: &mut impl FnMut(InspectionProgress),
) {
    let root = domain.root.as_ref();
    let definitions = domain.definitions.as_ref();
    let outlines = &domain.outlines;
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
            validate_review(domain, &path, item, errors);
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
                "{}: orphaned citation review: no reviewed source item or book passage with {{cite}} references matches it",
                display_path(root, file)
            ));
        }
        advance(progress);
    }
}

/// Validates one review file against its current item.
// {impl}`requirements.ci.errors`
pub(crate) fn validate_review(
    domain: &crate::sdk::HabeasDomain,
    label: &str,
    item: &CitationItem,
    errors: &mut Vec<String>,
) {
    let root = domain.root.as_ref();
    let definitions = domain.definitions.as_ref();
    let outlines = &domain.outlines;
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
    let subject =
        crate::sdk::HabeasNode::review_subject(&crate::authoring::AuthoringSubject::Citation {
            file: item.file.clone(),
            item: item.item.clone(),
        });
    crate::evidence_format::check_native(&value, &subject, &domain, &label, errors);
    let value = match crate::evidence_format::application_fields(&value) {
        Ok(value) => value,
        Err(error) => {
            errors.push(format!("{label}: invalid evidence format: {error}"));
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
            // Parse every entry before combining validity: collecting
            // directly into Option would hide errors after the first None.
            .collect::<Vec<_>>()
            .into_iter()
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
// {cite}`habeas:util`
pub(crate) fn is_review_path(evidence_root: &Path, path: &Path) -> bool {
    path.strip_prefix(evidence_root)
        .ok()
        .and_then(|relative| relative.components().next())
        .is_some_and(|first| first.as_os_str() == CITATIONS_DIR)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::{collect_references, load_definitions};

    fn project() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("md")).unwrap();
        fs::write(root.path().join("book.toml"), "[book]\nsrc='md'\n").unwrap();
        fs::write(
            root.path().join("md/requirements.md"),
            ":::{req} ready\nThe project is ready.\n:::\n",
        )
        .unwrap();
        root
    }

    // {test}`requirements.references.citation_review.book_commentary`
    // {test}`requirements.references.outline_comments`
    // {test}`requirements.references.util`
    #[test]
    fn book_commentary_groups_nested_citations_but_excludes_declarations_and_literals() {
        let root = project();
        let cite = |name: &str| format!("{{{}}}`{name}`", "cite");
        let commentary = format!(
            "# Notes\n\nUnfenced {}.\n\n:::::{{cite}} habeas:util\nOuter passage. {}\n\n::::{{cite}} requirements.ready\nNested passage.\n::::\n\n::::{{req}} nested\nNested requirement. {}\n::::\n\n::::{{example}} illustration\n{}\nNested example.\n::::\n\n```text\n:::{{cite}} requirements.ready\nLiteral.\n:::\n```\n\n\\{}\n{}\n{}\n{}\n:::::\n\n:::{{cite}} habeas:util\nUtility-only passage.\n:::\n\n:::{{cite}} requirements.missing\nUnresolved.\n:::\n\n:::{{req}} outer\n:::{{cite}} requirements.ready\nDeclaration commentary.\n:::\n:::\n\n:::{{cite}} requirements.ready\nUnclosed.\n",
            cite("requirements.ready"),
            cite("requirements.ready"),
            cite("requirements.ready"),
            cite("requirements.ready"),
            cite("requirements.ready"),
            cite("notes.nested"),
            cite("notes.illustration"),
            cite("habeas:coverage"),
        );
        fs::write(root.path().join("md/notes.md"), &commentary).unwrap();
        // The same fence outside the book is an ordinary source-item review.
        fs::write(
            root.path().join("notes.md"),
            ":::{cite} requirements.ready\nSource commentary.\n:::\n",
        )
        .unwrap();
        let definitions = load_definitions(root.path()).unwrap();
        let mut errors = Vec::new();
        let references = collect_references(root.path(), &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
        let outlines = OutlineCache::default();
        let items = citation_items(root.path(), &definitions, &references, &outlines);
        let book = items
            .values()
            .filter(|item| item.file == "md/notes.md")
            .collect::<Vec<_>>();
        assert_eq!(book.len(), 1, "{items:#?}");
        let book = book[0];
        assert_eq!(book.review_kind, ReviewKind::BookCommentary);
        assert_eq!(book.line, 5);
        assert_eq!(
            book.citations
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            [
                "habeas:util",
                "requirements.ready",
                "requirements.ready",
                "notes.nested",
                "notes.illustration",
                "habeas:coverage"
            ]
        );
        let parsed = outlines.parse(root.path(), &book.file);
        let parsed = parsed.as_ref().as_ref().unwrap();
        let owner = parsed.resolve(&book.file, &book.item).unwrap();
        let node = &parsed.outline.nodes[owner];
        assert_eq!(node.kind, "block");
        let text = &parsed.source[node.byte_start..node.byte_end];
        assert!(text.starts_with(":::::{cite} habeas:util"), "{text}");
        assert!(text.contains("Outer passage") && text.contains("Nested passage"));
        assert!(!text.contains("Unfenced") && !text.contains("Utility-only passage"));
        assert_eq!(
            items
                .values()
                .find(|item| item.file == "notes.md")
                .unwrap()
                .review_kind,
            ReviewKind::SourceItem
        );
    }

    // {test}`requirements.references.citation_review.book_commentary`
    #[test]
    fn separate_closed_passages_have_independent_subjects_and_empty_book_cites_stay_exempt() {
        let root = project();
        fs::write(root.path().join("md/notes.md"), "# Notes\n\n:::{cite} requirements.ready\nFirst passage.\n:::\n\n:::{cite} requirements.ready\nSecond passage.\n:::\n\n<div>\n:::{cite} requirements.ready\nHTML literal.\n:::\n</div>\n").unwrap();
        let definitions = load_definitions(root.path()).unwrap();
        let references = collect_references(root.path(), &mut Vec::new());
        let items = citation_items(
            root.path(),
            &definitions,
            &references,
            &OutlineCache::default(),
        );
        let passages = items.values().collect::<Vec<_>>();
        assert_eq!(passages.len(), 2, "{items:#?}");
        assert_ne!(passages[0].item, passages[1].item);
        assert_ne!(passages[0].review_path(), passages[1].review_path());
        assert!(passages.iter().all(
            |item| item.review_kind == ReviewKind::BookCommentary && item.citations.len() == 1
        ));
        fs::write(
            root.path().join("md/notes.md"),
            "Only an inline citation: {cite}`requirements.ready`.\n",
        )
        .unwrap();
        let references = collect_references(root.path(), &mut Vec::new());
        assert!(
            citation_items(
                root.path(),
                &definitions,
                &references,
                &OutlineCache::default()
            )
            .is_empty()
        );
    }
}

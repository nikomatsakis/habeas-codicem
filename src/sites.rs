//! Distributed requirements and site reviews: the queries a requirement
//! declares, the sites they select, and an assessment of each site.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::evidence::{
    Declaration, InspectionProgress, SourceReference, array_field, check_keys, display_path,
    string_field, validate_input,
};
use crate::inputs::{InputSelector, OutlineCache};
use crate::judgment::Judgment;
use crate::structural::{DeclaredQuery, QueryKind};
use crate::{DefinitionKind, ReferenceKind};

/// The directory under `evidence/` holding site reviews.
// {impl}`requirements.references.site_review`
pub(crate) const SITES_DIR: &str = "sites";

/// The inspection phase that validates site reviews.
// {impl}`requirements.authoring.progress`
const SITE_STAGE: &str = "site reviews";

/// One query match at a site.
// {impl}`requirements.references.site_review`
// {impl}`requirements.authoring.site_prompt`
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SiteMatch {
    pub(crate) kind: QueryKind,
    pub(crate) query: String,
    pub(crate) line_start: usize,
    pub(crate) line_end: usize,
    pub(crate) text: String,
}

/// One site of a distributed requirement: the innermost outline node
/// containing a match of one of its queries.
// {impl}`requirements.references.distributed`
// {impl}`requirements.references.site_review`
#[derive(Clone, Debug)]
pub(crate) struct SiteItem {
    pub(crate) requirement: String,
    /// Project-relative path.
    pub(crate) file: String,
    /// Outline selector; empty for the root node.
    pub(crate) item: String,
    /// Inclusive lines of the site's node.
    pub(crate) line_start: usize,
    pub(crate) line_end: usize,
    /// The matches at the site, in query declaration and source order.
    pub(crate) matches: Vec<SiteMatch>,
}

impl SiteItem {
    /// The review's path relative to the project root.
    // {impl}`requirements.references.site_review`
    pub(crate) fn review_path(&self) -> String {
        review_path(&self.requirement, &self.file, &self.item)
    }

    /// The selector as displayed, `<root>` for the root node.
    // {impl}`requirements.references.site_review`
    // {impl}`requirements.authoring.site_prompt`
    pub(crate) fn selector_label(&self) -> &str {
        if self.item.is_empty() {
            "<root>"
        } else {
            &self.item
        }
    }

    /// The sorted names of the queries with a match at the site.
    // {impl}`requirements.references.site_review`
    pub(crate) fn queries(&self) -> Vec<String> {
        self.matches
            .iter()
            .map(|found| found.query.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// The matches as the model receives them: each query's kind and name
    /// and the match's lines, and, for a violation match, its redacted text.
    // {impl}`requirements.authoring.site_prompt`
    // {impl}`requirements.authoring.redaction`
    pub(crate) fn matches_value(&self) -> Value {
        Value::Array(
            self.matches
                .iter()
                .map(|found| {
                    let mut value = json!({
                        "kind": found.kind.label(),
                        "query": found.query,
                        "line_start": found.line_start,
                        "line_end": found.line_end,
                    });
                    if found.kind == QueryKind::Violations {
                        value["text"] =
                            Value::String(crate::redaction::redact_text(&found.text).into_owned());
                    }
                    value
                })
                .collect(),
        )
    }

    /// Whether a reference lies inside the site's node.
    // {impl}`requirements.references.site_review`
    pub(crate) fn contains(&self, root: &Path, reference: &SourceReference) -> bool {
        display_path(root, &reference.file) == self.file
            && self.line_start <= reference.line
            && reference.line <= self.line_end
    }

    /// The `{fixme}` references naming the requirement inside the site,
    /// which acknowledge gaps at this site only.
    // {impl}`requirements.references.site_review`
    pub(crate) fn fixmes<'a>(
        &self,
        root: &Path,
        references: &'a [SourceReference],
    ) -> Vec<&'a SourceReference> {
        references
            .iter()
            .filter(|reference| {
                reference.kind == ReferenceKind::Fixme
                    && reference.name == self.requirement
                    && reference.is_fixme_edge()
                    && reference.closed
                    && self.contains(root, reference)
            })
            .collect()
    }
}

/// The sites of each distributed requirement, by full name.
// {impl}`requirements.references.distributed`
pub(crate) type Sites = BTreeMap<String, Vec<SiteItem>>;

/// Whether a reference is a `{fixme}` naming a distributed requirement from
/// inside one of its sites, so it belongs to that site's review alone.
// {impl}`requirements.references.site_review`
pub(crate) fn in_own_site(root: &Path, sites: &Sites, reference: &SourceReference) -> bool {
    reference.kind == ReferenceKind::Fixme
        && sites
            .get(&reference.name)
            .is_some_and(|sites| sites.iter().any(|site| site.contains(root, reference)))
}

/// `evidence/sites/<full-name>/<file>/<selector>.json`, with the selector
/// encoded as for citation reviews.
// {impl}`requirements.references.site_review`
pub(crate) fn review_path(requirement: &str, file: &str, item: &str) -> String {
    format!(
        "evidence/{SITES_DIR}/{requirement}/{file}/{}.json",
        crate::citations::encoded_selector(item)
    )
}

/// `sites:<full-name>/<file>#<selector>`.
// {impl}`requirements.references.site_review`
pub(crate) fn subject_label(requirement: &str, file: &str, item: &str) -> String {
    format!("sites:{requirement}/{file}#{item}")
}

/// Whether a path lies in the site review directory.
// {impl}`requirements.references.site_review`
// {impl}`requirements.ci.orphans`
pub(crate) fn is_review_path(evidence_root: &Path, path: &Path) -> bool {
    path.strip_prefix(evidence_root)
        .ok()
        .and_then(|relative| relative.components().next())
        .is_some_and(|first| first.as_os_str() == SITES_DIR)
}

/// The sites of one distributed requirement's well-formed queries, without
/// validating the queries; a requirement without queries has none.
// {impl}`requirements.references.distributed`
pub(crate) fn requirement_sites(
    root: &Path,
    name: &str,
    declaration: &Declaration,
    files: &[PathBuf],
    outlines: &OutlineCache,
) -> Result<Vec<SiteItem>, String> {
    let (queries, _) = crate::structural::parse_queries(declaration);
    let mut matches = Vec::new();
    for query in &queries {
        let found = crate::structural::query_matches(root, files, outlines, &queries, &query.name)
            .map_err(|error| format!("{name}: query {}: {error}", query.name))?;
        matches.push((query, found));
    }
    Ok(collect_sites(root, name, outlines, matches))
}

/// Groups query matches by site, in path and selector order.
// {impl}`requirements.references.distributed`
fn collect_sites(
    root: &Path,
    name: &str,
    outlines: &OutlineCache,
    matches: Vec<(&DeclaredQuery, Vec<crate::structural::QueryMatch>)>,
) -> Vec<SiteItem> {
    let mut sites = BTreeMap::<(String, String), SiteItem>::new();
    for (query, found) in matches {
        for found in found {
            let site = sites
                .entry((found.site.file.clone(), found.site.item.clone()))
                .or_insert_with(|| {
                    let (line_start, line_end) = outlines
                        .parse(root, &found.site.file)
                        .as_ref()
                        .as_ref()
                        .ok()
                        .and_then(|parsed| {
                            let index = parsed.outline.resolve(&found.site.item).ok()?;
                            let node = &parsed.outline.nodes[index];
                            Some((node.line_start, node.line_end))
                        })
                        .unwrap_or((found.line_start, found.line_end));
                    SiteItem {
                        requirement: name.to_owned(),
                        file: found.site.file.clone(),
                        item: found.site.item.clone(),
                        line_start,
                        line_end,
                        matches: Vec::new(),
                    }
                });
            site.matches.push(SiteMatch {
                kind: query.kind,
                query: query.name.clone(),
                line_start: found.line_start,
                line_end: found.line_end,
                text: found.text,
            });
        }
    }
    sites.into_values().collect()
}

/// Validates every requirement's query directives and finds each
/// distributed requirement's sites. A query that is malformed, or whose
/// `within` query is, finds no sites.
// {impl}`requirements.references.distributed`
// {impl}`requirements.ci.errors`
pub(crate) fn discover(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    files: &[PathBuf],
    outlines: &OutlineCache,
    errors: &mut Vec<String>,
) -> Sites {
    let mut sites = Sites::new();
    for (name, declaration) in definitions {
        if declaration.kind != DefinitionKind::Requirement
            || !crate::structural::has_query_directive(declaration)
        {
            continue;
        }
        let (queries, problems) = crate::structural::parse_queries(declaration);
        errors.extend(problems);
        let runnable = validate_queries(name, declaration, &queries, errors);
        let mut matches = Vec::new();
        for query in runnable {
            let location = format!("{}:{}", declaration.file, query.line);
            match crate::structural::query_matches(root, files, outlines, &queries, &query.name) {
                Ok(found) => {
                    if query.kind == QueryKind::Sites && found.is_empty() {
                        errors.push(format!(
                            "{location}: site query {} of {name} matches nothing in the project",
                            query.name
                        ));
                    }
                    matches.push((query, found));
                }
                Err(error) => errors.push(format!(
                    "{location}: query {} of {name} cannot run: {error}",
                    query.name
                )),
            }
        }
        sites.insert(name.clone(), collect_sites(root, name, outlines, matches));
    }
    sites
}

/// Reports the problems of a requirement's well-formed queries and returns
/// the names of those that can run: each name must be a unique identifier
/// segment, a violation query needs a site query to lie `within`, each rule
/// must compile, and each `match` snippet must contain a match and each
/// `no_match` snippet none.
// {impl}`requirements.references.distributed`
// {impl}`requirements.ci.errors`
fn validate_queries<'a>(
    name: &str,
    declaration: &Declaration,
    queries: &'a [DeclaredQuery],
    errors: &mut Vec<String>,
) -> Vec<&'a DeclaredQuery> {
    let mut valid = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut runnable = Vec::new();
    let has_sites = queries.iter().any(|query| query.kind == QueryKind::Sites);
    for query in queries {
        let location = format!("{}:{}", declaration.file, query.line);
        let mut problems = Vec::new();
        if !crate::valid_name(&query.name) || query.name.contains('.') {
            problems.push(format!(
                "query name {:?} must be a single identifier segment of ASCII letters, digits, underscores, or hyphens",
                query.name
            ));
        }
        if !seen.insert(query.name.clone()) {
            problems.push(format!(
                "query name {} is declared more than once in {name}",
                query.name
            ));
        }
        if query.kind == QueryKind::Violations && !has_sites {
            problems.push(format!(
                "{name} has a violation query, so it must also have a site query"
            ));
        }
        if let Err(error) = crate::structural::within_query(queries, query) {
            problems.push(error);
        }
        if let Err(error) = crate::structural::StructuralRule::compile(
            &query.body.language,
            &query.body.rule_core(),
        ) {
            problems.push(error);
        }
        for (field, snippets) in [
            ("match", &query.body.matches),
            ("no_match", &query.body.no_match),
        ] {
            if snippets.is_empty() {
                problems.push(format!("{field} must be a nonempty list of snippets"));
            }
        }
        for problem in &problems {
            errors.push(format!(
                "{location}: query {} of {name}: {problem}",
                query.name
            ));
        }
        if problems.is_empty() {
            valid.insert(query.name.clone());
            runnable.push(query);
        }
    }
    // A violation query runs only when the site query it lies within does.
    runnable.retain(|query| {
        query
            .body
            .within
            .as_ref()
            .is_none_or(|within| valid.contains(within))
    });
    for query in &runnable {
        let location = format!("{}:{}", declaration.file, query.line);
        for (field, snippets, expect_match) in [
            ("match", &query.body.matches, true),
            ("no_match", &query.body.no_match, false),
        ] {
            for (index, snippet) in snippets.iter().enumerate() {
                let problem = match crate::structural::snippet_matches(queries, query, snippet) {
                    Err(error) => Some(error),
                    Ok(0) if expect_match => Some("contains no match of the query".to_owned()),
                    Ok(count) if !expect_match && count > 0 => {
                        Some(format!("contains {count} matches of the query"))
                    }
                    Ok(_) => None,
                };
                if let Some(problem) = problem {
                    errors.push(format!(
                        "{location}: query {} of {name}: {field}[{index}] snippet {problem}",
                        query.name
                    ));
                }
            }
        }
    }
    runnable
}

/// Reports query directives outside a `{req}` body in one chapter. Text in
/// code blocks stays literal.
// {impl}`requirements.references.distributed`
// {impl}`requirements.ci.errors`
pub(crate) fn validate_query_placement(
    label: &str,
    content: &str,
    blocks: &[crate::DefinitionBlock],
    errors: &mut Vec<String>,
) {
    let code = crate::code_block_ranges(content);
    let mut offset = 0;
    for (index, line) in content.split_inclusive('\n').enumerate() {
        let start = offset;
        offset += line.len();
        let text = line.trim_end_matches(['\n', '\r']);
        let Some((_, kind, name)) = crate::structural::query_fence(text.trim_start_matches(' '))
        else {
            continue;
        };
        if code.iter().any(|range| range.contains(&start)) {
            continue;
        }
        let inside = blocks.iter().any(|block| {
            block.kind == DefinitionKind::Requirement
                && block.body_start <= start
                && start < block.body_end
        });
        if !inside {
            errors.push(format!(
                "{label}:{}: {{{}}} query {name} must appear inside a {{req}} body",
                index + 1,
                kind.label()
            ));
        }
    }
}

/// The declarations a requirement cites, which every site review receives.
// {impl}`requirements.references.site_review`
pub(crate) fn cited_declarations(
    declaration: &Declaration,
    definitions: &BTreeMap<String, Declaration>,
) -> Vec<String> {
    let mut cited = Vec::new();
    for (kind, name) in crate::text_references(&declaration.text, true) {
        if kind == ReferenceKind::Cite
            && definitions
                .get(&name)
                .is_some_and(|definition| definition.kind != DefinitionKind::Term)
            && !cited.contains(&name)
        {
            cited.push(name);
        }
    }
    cited
}

/// One acknowledgment inside a site as recorded: the outline node it
/// belongs to and its body.
// {impl}`requirements.references.site_review`
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SiteFixme {
    pub(crate) item: String,
    pub(crate) commentary: String,
}

impl SiteFixme {
    // {impl}`requirements.references.site_review`
    pub(crate) fn to_value(&self) -> Value {
        json!({"item": self.item, "commentary": self.commentary})
    }

    /// The comparison key: the node and the body's words, so reflowing the
    /// body does not make the review stale.
    // {impl}`requirements.references.site_review`
    fn key(&self) -> (String, Vec<String>) {
        (
            self.item.clone(),
            self.commentary
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
        )
    }
}

/// The `{fixme}` references naming the requirement inside a site, in source
/// order, as recorded in its review.
// {impl}`requirements.references.site_review`
pub(crate) fn site_fixmes(
    root: &Path,
    site: &SiteItem,
    references: &[SourceReference],
    outlines: &OutlineCache,
) -> Vec<SiteFixme> {
    let parsed = outlines.parse(root, &site.file);
    site.fixmes(root, references)
        .into_iter()
        .map(|reference| SiteFixme {
            item: parsed
                .as_ref()
                .as_ref()
                .ok()
                .filter(|parsed| parsed.outline.profile.language.is_some())
                .map(|parsed| {
                    parsed.outline.nodes[parsed.outline.owner_at_line(reference.line)]
                        .selector
                        .clone()
                })
                .unwrap_or_default(),
            commentary: reference.commentary.clone().unwrap_or_default(),
        })
        .collect()
}

/// Whether two lists of site `{fixme}` references are the same, by node and
/// body words.
// {impl}`requirements.references.site_review`
fn same_fixmes(recorded: &[SiteFixme], current: &[SiteFixme]) -> bool {
    let key = |fixmes: &[SiteFixme]| {
        let mut keys = fixmes.iter().map(SiteFixme::key).collect::<Vec<_>>();
        keys.sort();
        keys
    };
    key(recorded) == key(current)
}

/// Validates site reviews: each current site needs a current, accepted
/// review, and every review file must name a current site. `files` are the
/// review files already listed with the rest of `evidence/`.
// {impl}`requirements.references.site_review`
// {impl}`requirements.ci.outcomes`
// {impl}`requirements.authoring.progress`
// {impl}`requirements.ci.errors`
#[allow(clippy::too_many_arguments)]
pub(crate) fn validate(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    references: &[SourceReference],
    sites: &Sites,
    outlines: &OutlineCache,
    files: &[PathBuf],
    errors: &mut Vec<String>,
    progress: &mut impl FnMut(InspectionProgress),
) {
    let total = sites.values().map(Vec::len).sum::<usize>() + files.len();
    let mut completed = 0;
    let mut advance = |progress: &mut dyn FnMut(InspectionProgress)| {
        progress(InspectionProgress {
            stage: SITE_STAGE,
            completed,
            total: Some(total),
        });
        completed += 1;
    };
    advance(progress);
    let mut expected = BTreeSet::new();
    for site in sites.values().flatten() {
        let path = site.review_path();
        expected.insert(root.join(&path));
        if root.join(&path).exists() {
            validate_review(root, &path, site, definitions, references, outlines, errors);
        } else {
            errors.push(format!(
                "{path}: missing site review of {} for {} {} at {}:{}",
                site.requirement,
                site.file,
                site.selector_label(),
                site.file,
                site.line_start
            ));
        }
        advance(progress);
    }
    for file in files {
        if !expected.contains(file) {
            errors.push(format!(
                "{}: orphaned site review: its file and selector name no site of a distributed requirement",
                display_path(root, file)
            ));
        }
        advance(progress);
    }
}

/// Validates one review file against its current site.
// {impl}`requirements.references.site_review`
// {impl}`requirements.ci.outcomes`
// {impl}`requirements.ci.errors`
fn validate_review(
    root: &Path,
    label: &str,
    site: &SiteItem,
    definitions: &BTreeMap<String, Declaration>,
    references: &[SourceReference],
    outlines: &OutlineCache,
    errors: &mut Vec<String>,
) {
    let content = match fs::read_to_string(root.join(label)) {
        Ok(content) => content,
        Err(error) => {
            errors.push(format!("{label}: cannot read site review: {error}"));
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
            "requirement",
            "file",
            "item",
            "queries",
            "applies",
            "fixmes",
            "determination",
            "rationale",
            "inputs",
        ],
        label,
        errors,
    );
    for (field, expected) in [
        ("requirement", &site.requirement),
        ("file", &site.file),
        ("item", &site.item),
    ] {
        if let Some(recorded) = string_field(object, field, label, errors)
            && recorded != expected
        {
            errors.push(format!("{label}: {field} must be {expected:?}"));
        }
    }
    if let Some(recorded) = array_field(object, "queries", label, errors) {
        let queries = site.queries();
        let current = queries.iter().map(|query| json!(query)).collect::<Vec<_>>();
        if *recorded != current {
            errors.push(format!(
                "{label}: queries changed for {} {}: recorded {}, now {}",
                site.file,
                site.selector_label(),
                Value::Array(recorded.clone()),
                Value::Array(current)
            ));
        }
    }
    if let Some(recorded) = array_field(object, "fixmes", label, errors) {
        let current = site_fixmes(root, site, references, outlines);
        let parsed = recorded
            .iter()
            .map(|fixme| {
                Some(SiteFixme {
                    item: fixme.get("item")?.as_str()?.to_owned(),
                    commentary: fixme.get("commentary")?.as_str()?.to_owned(),
                })
            })
            .collect::<Option<Vec<_>>>();
        match parsed {
            None => errors.push(format!(
                "{label}: fixmes must be an array of objects with string item and commentary"
            )),
            Some(parsed) if !same_fixmes(&parsed, &current) => errors.push(format!(
                "{label}: {{fixme}} references to {} inside {} {} changed: recorded {}, now {}",
                site.requirement,
                site.file,
                site.selector_label(),
                Value::Array(parsed.iter().map(SiteFixme::to_value).collect()),
                Value::Array(current.iter().map(SiteFixme::to_value).collect())
            )),
            Some(_) => {}
        }
    }
    let applies = match object.get("applies") {
        Some(Value::Bool(applies)) => Some(*applies),
        Some(_) => {
            errors.push(format!("{label}: applies must be a Boolean"));
            None
        }
        None => {
            errors.push(format!("{label}: missing field \"applies\""));
            None
        }
    };
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
    if applies == Some(false)
        && determination
            .as_ref()
            .is_some_and(|judgment| *judgment != Judgment::True)
    {
        errors.push(format!(
            "{label}: applies is false, so the determination must be true"
        ));
    }
    match &determination {
        Some(Judgment::False) => errors.push(format!(
            "{label}: determination is false{}",
            rationale
                .filter(|text| !text.trim().is_empty())
                .map_or(String::new(), |text| format!(": {}", text.trim()))
        )),
        Some(Judgment::Fixme(entries)) => crate::evidence::validate_fixme_entries(
            root,
            label,
            &site.requirement,
            entries,
            &site.fixmes(root, references),
            errors,
        ),
        _ => {}
    }
    let Some(inputs) = array_field(object, "inputs", label, errors) else {
        return;
    };
    let mut selects_site = false;
    let mut declared = BTreeSet::new();
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
            Some(InputSelector::Outline { file, item }) => {
                selects_site |= file == site.file && item == site.item;
            }
            Some(InputSelector::Requirements { .. }) => declared.extend(
                input
                    .get("matches")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned),
            ),
            _ => {}
        }
    }
    if !selects_site {
        errors.push(format!(
            "{label}: inputs must select the site {} {}",
            site.file,
            site.selector_label()
        ));
    }
    let mut needed = vec![site.requirement.clone()];
    if let Some(declaration) = definitions.get(&site.requirement) {
        needed.extend(cited_declarations(declaration, definitions));
    }
    for name in needed {
        if !declared.contains(&name) {
            errors.push(format!(
                "{label}: inputs must include the declaration of {name}"
            ));
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A requirement with a site query selecting `check_` functions and a
    /// violation query for panics within them.
    // {test}`requirements.references.distributed`
    pub(crate) const RULE: &str = r#":::::{req} rule
Each checker MUST report problems instead of panicking.

::::{sites} checkers
```yaml
language: rust
paths: [src]
rule:
  kind: function_item
  has: { field: name, regex: "^check_" }
match:
  - "fn check_one() {}"
no_match:
  - "fn other() {}"
```
::::

::::{violations} panics
```yaml
language: rust
within: checkers
rule: { kind: macro_invocation, regex: "^panic" }
match:
  - "fn check_one() { panic!(); }"
no_match:
  - "fn other() { panic!(); }"
```
::::
:::::
"#;

    /// Sources with three checker sites, one panicking directly and one
    /// through a nested function, and a panic outside every site.
    // {test}`requirements.references.distributed`
    pub(crate) const SOURCE: &str = "fn check_one() { panic!(\"x\"); }\nfn check_two() {}\nfn other() { panic!(\"y\"); }\nmod inner {\n    fn check_three() {\n        fn nested() { panic!(\"z\"); }\n    }\n}\n";

    /// A project outside Git whose book declares `chapter` and whose
    /// `src/lib.rs` is `source`.
    // {test}`requirements.references.distributed`
    pub(crate) fn project(chapter: &str, source: &str) -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("md")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("evidence")).unwrap();
        fs::write(root.join("book.toml"), "[book]\nsrc = \"md\"\n").unwrap();
        fs::write(root.join("md/requirements.md"), chapter).unwrap();
        fs::write(root.join("src/lib.rs"), source).unwrap();
        temp
    }

    /// Validation errors and the sites of every distributed requirement.
    // {test}`requirements.references.distributed`
    pub(crate) fn inspect(root: &Path) -> (Vec<String>, Sites) {
        let inspection = crate::evidence::inspect_repository(root).unwrap();
        (inspection.report.errors, inspection.sites)
    }

    // {test}`requirements.references.distributed`
    #[test]
    fn sites_are_the_innermost_nodes_of_site_matches_and_violations_within_them() {
        let temp = project(RULE, SOURCE);
        let (errors, sites) = inspect(temp.path());
        let sites = &sites["requirements.rule"];
        let found = sites
            .iter()
            .map(|site| (site.item.as_str(), site.queries()))
            .collect::<Vec<_>>();
        assert_eq!(
            found,
            [
                (
                    "fn[check_one]",
                    vec!["checkers".to_owned(), "panics".to_owned()]
                ),
                ("fn[check_two]", vec!["checkers".to_owned()]),
                (
                    "mod[inner]/fn[check_three]",
                    vec!["checkers".to_owned(), "panics".to_owned()]
                ),
            ]
        );
        let nested = &sites[2];
        assert_eq!((nested.line_start, nested.line_end), (5, 7));
        let violation = nested
            .matches
            .iter()
            .find(|found| found.kind == QueryKind::Violations)
            .unwrap();
        assert_eq!(violation.text, "panic!(\"z\")");
        assert_eq!(violation.line_start, 6);
        assert_eq!(
            nested.matches_value()[1],
            json!({"kind": "violations", "query": "panics", "line_start": 6, "line_end": 6, "text": "panic!(\"z\")"})
        );
        // No query problems; each site lacks a review, and being a site is
        // not a reference for Rust item coverage.
        assert!(
            !errors.iter().any(|error| error.contains("query")),
            "{errors:?}"
        );
        for item in [
            "fn[check_one]",
            "fn[check_two]",
            "mod[inner]/fn[check_three]",
        ] {
            let path = review_path("requirements.rule", "src/lib.rs", item);
            assert!(
                errors
                    .iter()
                    .any(|error| error.starts_with(&format!("{path}: missing site review"))),
                "{item}: {errors:?}"
            );
        }
        assert!(
            errors
                .iter()
                .any(|error| error.contains("function check_two has no requirement")),
            "{errors:?}"
        );
    }

    // {test}`requirements.references.distributed`
    #[test]
    fn malformed_queries_are_reported_each_on_its_own() {
        let chapter = r#":::::{req} rule
Text.

::::{sites} bad.name
```yaml
language: rust
rule: { kind: function_item }
match: ["fn a() {}"]
no_match: ["fn a() {}"]
```
::::

::::{sites} nothing
```yaml
language: rust
rule: { kind: function_item, regex: "^fn never_defined" }
match: ["struct S;"]
no_match: ["fn"]
```
::::

::::{sites} uncompiled
```yaml
language: rust
rule: { kind: no_such_kind }
match: ["fn a() {}"]
no_match: ["fn b() {}"]
```
::::

::::{violations} loose
```yaml
language: rust
rule: { kind: function_item }
match: ["fn a() {}"]
no_match: ["fn b() {}"]
```
::::

::::{violations} astray
```yaml
language: rust
within: missing
rule: { kind: function_item }
match: ["fn a() {}"]
no_match: []
```
::::

::::{sites} keyed
```yaml
language: rust
within: nothing
rule: { kind: function_item }
match: ["fn a() {}"]
no_match: ["struct S;"]
```
::::

::::{sites} extra
```yaml
language: rust
rule: { kind: function_item }
fix: ""
match: ["fn a() {}"]
no_match: ["struct S;"]
```
::::

::::{sites} twice
```yaml
language: rust
rule: { kind: struct_item }
match: ["struct S;"]
no_match: ["fn a() {}"]
```
::::

::::{sites} twice
```yaml
language: rust
rule: { kind: struct_item }
match: ["struct S;"]
no_match: ["fn a() {}"]
```
::::

::::{sites} prose
Not a code block.
::::
:::::

::::{sites} outside
```yaml
language: rust
rule: { kind: function_item }
match: ["fn a() {}"]
no_match: ["struct S;"]
```
::::

```text
::::{sites} literal
```
"#;
        let temp = project(chapter, "fn run() {}\n");
        let (errors, sites) = inspect(temp.path());
        let errors = errors
            .into_iter()
            .filter(|error| error.starts_with("md/"))
            .collect::<Vec<_>>();
        let expect = |line: usize, text: &str| {
            assert!(
                errors.iter().any(|error| {
                    error.starts_with(&format!("md/requirements.md:{line}:"))
                        && error.contains(text)
                }),
                "{line}: {text}: {errors:#?}"
            );
        };
        expect(4, "single identifier segment");
        // The first matching snippet has no match, the no_match snippet does
        // not parse, and nothing in the project matches.
        expect(13, "match[0] snippet contains no match");
        expect(13, "no_match[0] snippet does not parse as rust");
        expect(
            13,
            "site query nothing of requirements.rule matches nothing in the project",
        );
        expect(22, "invalid ast-grep rule");
        expect(31, "violation query loose must have within");
        expect(40, "within names no site query \"missing\"");
        expect(40, "no_match must be a nonempty list");
        expect(50, "site query keyed must not have within");
        expect(60, "unknown field `fix`");
        expect(79, "query name twice is declared more than once");
        expect(70, "site query twice of requirements.rule matches nothing");
        assert!(
            !errors
                .iter()
                .any(|error| error.starts_with("md/requirements.md:79:")
                    && error.contains("matches nothing")),
            "{errors:#?}"
        );
        expect(88, "must contain exactly one fenced yaml code block");
        expect(93, "{sites} query outside must appear inside a {req} body");
        assert!(
            !errors.iter().any(|error| error.contains("literal")),
            "{errors:#?}"
        );
        // Only the first `twice` runs, and it selects nothing.
        assert!(sites["requirements.rule"].is_empty());
    }

    // {test}`requirements.references.distributed`
    #[test]
    fn snippets_must_have_the_expected_matches_counting_violations_within_sites() {
        let chapter = RULE
            .replace(
                "  - \"fn other() { panic!(); }\"",
                "  - \"fn check_other() { panic!(); }\"",
            )
            .replace(
                "  - \"fn check_one() { panic!(); }\"",
                "  - \"fn one() { panic!(); }\"",
            );
        let temp = project(&chapter, SOURCE);
        let (errors, _) = inspect(temp.path());
        for text in [
            "query panics of requirements.rule: match[0] snippet contains no match of the query",
            "query panics of requirements.rule: no_match[0] snippet contains 1 matches of the query",
        ] {
            assert!(
                errors.iter().any(|error| error.contains(text)),
                "{text}: {errors:#?}"
            );
        }
    }

    // {test}`requirements.references.distributed`
    #[test]
    fn a_violation_query_needs_a_site_query() {
        let chapter = ":::{req} rule\nText.\n\n::::{violations} only\n```yaml\nlanguage: rust\nwithin: only\nrule: { kind: function_item }\nmatch: [\"fn a() {}\"]\nno_match: [\"struct S;\"]\n```\n::::\n:::\n"
            .replace(":::{req}", ":::::{req}")
            .replace("\n:::\n", "\n:::::\n");
        let temp = project(&chapter, "fn run() {}\n");
        let (errors, _) = inspect(temp.path());
        assert!(
            errors.iter().any(|error| error.contains(
                "requirements.rule has a violation query, so it must also have a site query"
            )),
            "{errors:#?}"
        );
    }

    /// The inputs a current review of a site records: the site's outline
    /// node and the requirement's declaration.
    // {test}`requirements.references.site_review`
    pub(crate) fn site_inputs(root: &Path, item: &str) -> Vec<Value> {
        let definitions = crate::evidence::load_definitions(root).unwrap();
        let outlines = OutlineCache::default();
        let selector = InputSelector::Requirements {
            requirements: "requirements.rule".to_owned(),
        };
        let resolved = selector.resolve(root, &definitions, &outlines).unwrap();
        vec![
            crate::inputs::outline_input(root, "src/lib.rs", item).unwrap(),
            selector.to_input(&resolved),
        ]
    }

    /// Writes a site review of `requirements.rule` for an item of
    /// `src/lib.rs`.
    // {test}`requirements.references.site_review`
    pub(crate) fn write_review(root: &Path, item: &str, fields: Value) {
        let mut record = json!({
            "requirement": "requirements.rule",
            "file": "src/lib.rs",
            "item": item,
            "queries": ["checkers"],
            "applies": true,
            "fixmes": [],
            "determination": true,
            "rationale": "The site reports its problems.",
            "inputs": site_inputs(root, item),
        });
        for (key, value) in fields.as_object().unwrap() {
            record[key] = value.clone();
        }
        let path = root.join(review_path("requirements.rule", "src/lib.rs", item));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, record.to_string()).unwrap();
    }

    /// The validation errors of one site review.
    // {test}`requirements.references.site_review`
    fn review_errors(errors: &[String], item: &str) -> Vec<String> {
        let prefix = format!("{}:", review_path("requirements.rule", "src/lib.rs", item));
        errors
            .iter()
            .filter(|error| error.starts_with(&prefix))
            .cloned()
            .collect()
    }

    // {test}`requirements.references.site_review`
    // {test}`requirements.ci.orphans`
    #[test]
    fn each_site_needs_a_current_accepted_review_and_orphans_are_reported() {
        let temp = project(RULE, SOURCE);
        let root = temp.path();
        write_review(root, "fn[check_two]", json!({}));
        write_review(
            root,
            "fn[check_one]",
            json!({"applies": false, "determination": false, "rationale": "It panics."}),
        );
        write_review(root, "mod[inner]/fn[check_three]", json!({"inputs": []}));
        write_review(root, "fn[other]", json!({}));
        let (errors, _) = inspect(root);
        assert_eq!(
            review_errors(&errors, "fn[check_two]"),
            Vec::<String>::new()
        );
        let one = review_errors(&errors, "fn[check_one]").join("\n");
        for text in [
            "queries changed for src/lib.rs fn[check_one]: recorded [\"checkers\"], now [\"checkers\",\"panics\"]",
            "applies is false, so the determination must be true",
            "determination is false: It panics.",
        ] {
            assert!(one.contains(text), "{text}: {one}");
        }
        let three = review_errors(&errors, "mod[inner]/fn[check_three]").join("\n");
        for text in [
            "inputs must select the site src/lib.rs mod[inner]/fn[check_three]",
            "inputs must include the declaration of requirements.rule",
        ] {
            assert!(three.contains(text), "{text}: {three}");
        }
        assert_eq!(
            review_errors(&errors, "fn[other]"),
            [format!(
                "{}: orphaned site review: its file and selector name no site of a distributed requirement",
                review_path("requirements.rule", "src/lib.rs", "fn[other]")
            )]
        );
        assert!(
            !errors
                .iter()
                .any(|error| error.contains("no matching requirement or example")
                    && error.starts_with("evidence/sites/")),
            "{errors:#?}"
        );
        // A changed site makes its review stale.
        fs::write(
            root.join("src/lib.rs"),
            SOURCE.replace("fn check_two() {}", "fn check_two() { let _ = 1; }"),
        )
        .unwrap();
        let (errors, _) = inspect(root);
        assert!(
            review_errors(&errors, "fn[check_two]")
                .iter()
                .any(|error| error.contains("hash")),
            "{errors:#?}"
        );
    }

    // {test}`requirements.references.site_review`
    // {test}`requirements.references.evidence_coverage`
    // {test}`requirements.ci.reference_set`
    #[test]
    fn a_fixme_inside_a_site_belongs_to_that_sites_review_only() {
        let fixme = |issue: &str| {
            format!(
                "    // :::{{fixme}} requirements.rule\n    // :github: o/r#{issue}\n    // Panics instead of reporting.\n    // :::\n"
            )
        };
        let source = SOURCE
            .replace(
                "fn check_two() {}",
                &format!("fn check_two() {{\n{}}}", fixme("1")),
            )
            .replace(
                "fn other() { panic!(\"y\"); }",
                &format!("fn other() {{\n{}}}", fixme("2")),
            );
        let temp = project(RULE, &source);
        let root = temp.path();
        fs::write(
            root.join("evidence/requirements.rule.json"),
            json!({"requirement": "x", "determination": true, "rationale": "Fine.", "inputs": []})
                .to_string(),
        )
        .unwrap();
        crate::evidence::tests::stamp_reference_sets(root);
        write_review(
            root,
            "fn[check_two]",
            json!({"determination": {"FIXME": ["o/r#1"]}}),
        );
        let (errors, _) = inspect(root);
        let coverage = errors
            .iter()
            .filter(|error| error.contains("no input covers {fixme}"))
            .collect::<Vec<_>>();
        assert_eq!(coverage.len(), 1, "{errors:#?}");
        assert!(coverage[0].ends_with("src/lib.rs:9"), "{coverage:?}");
        let two = review_errors(&errors, "fn[check_two]").join("\n");
        assert!(
            two.contains("{fixme} references to requirements.rule inside src/lib.rs fn[check_two] changed: recorded [], now [{\"item\":\"fn[check_two]\",\"commentary\":\":github: o/r#1\\n Panics instead of reporting.\"}]"),
            "{two}"
        );
        assert!(!two.contains("FIXME entry"), "{two}");

        // Editing the in-site `{fixme}` leaves the requirement's reference
        // set unchanged; editing the other changes it.
        let digest = crate::evidence::reference_set_digest_for(root, "requirements.rule").unwrap();
        fs::write(
            root.join("src/lib.rs"),
            source.replacen("Panics instead", "Still panics instead", 1),
        )
        .unwrap();
        assert_eq!(
            crate::evidence::reference_set_digest_for(root, "requirements.rule").unwrap(),
            digest
        );
        fs::write(
            root.join("src/lib.rs"),
            source.replace(
                "// Panics instead of reporting.\n    // :::\n}\nmod",
                "// Edited.\n    // :::\n}\nmod",
            ),
        )
        .unwrap();
        assert_ne!(
            crate::evidence::reference_set_digest_for(root, "requirements.rule").unwrap(),
            digest
        );
    }

    // {test}`requirements.references.site_review`
    #[test]
    fn status_and_info_report_site_reviews_by_requirement_file_and_selector() {
        let temp = project(RULE, SOURCE);
        let root = temp.path();
        write_review(
            root,
            "fn[check_two]",
            json!({"applies": false, "rationale": "A helper, not a checker."}),
        );
        write_review(
            root,
            "mod[inner]/fn[check_three]",
            json!({"queries": ["checkers", "panics"], "determination": false, "rationale": "It panics."}),
        );
        let status = crate::status_markdown(root, false).unwrap();
        for row in [
            "| `requirements.rule` | `src/lib.rs` | `fn[check_one]` | Missing |",
            "| `requirements.rule` | `src/lib.rs` | `mod[inner]/fn[check_three]` | Determination false |",
        ] {
            assert!(status.contains(row), "{row}\n{status}");
        }
        assert!(!status.contains("`fn[check_two]` | Valid"), "{status}");
        assert!(
            status.contains("### `sites:requirements.rule/src/lib.rs#mod[inner]/fn[check_three]`"),
            "{status}"
        );
        assert!(
            status.contains("determination is false: It panics."),
            "{status}"
        );
        let all = crate::status_markdown(root, true).unwrap();
        assert!(
            all.contains("| `requirements.rule` | `src/lib.rs` | `fn[check_two]` | Valid |"),
            "{all}"
        );

        let info = crate::info_markdown(root, "requirements.rule").unwrap();
        for text in [
            "## Sites\n\n3 sites; the requirement does not apply to 1 of them.",
            "| `src/lib.rs` | `fn[check_one]` | unknown | `none` | Missing |",
            "| `src/lib.rs` | `fn[check_two]` | no | `true` | Valid |",
            "| `src/lib.rs` | `mod[inner]/fn[check_three]` | yes | `false` | Determination false |",
        ] {
            assert!(info.contains(text), "{text}\n{info}");
        }
    }
}

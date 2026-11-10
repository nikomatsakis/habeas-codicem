use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::access::SourceAccess;
use crate::citations::CitationItem;
use crate::evidence::{
    Declaration, SourceReference, collect_references, discovered_files, load_definitions,
};
use crate::github::GithubIssues;
use crate::inputs::{InputSelector, OutlineCache, ParsedSource, recorded_input};
use crate::sites::{SiteFixme, SiteItem};
use crate::{DefinitionKind, ReferenceKind};

pub(crate) enum AssessmentKind {
    /// A requirement or example, and whether it is distributed.
    Definition(DefinitionKind, bool),
    Citation(CitationItem),
    /// A site of a distributed requirement and the `{fixme}` references
    /// naming the requirement inside it.
    Site(SiteItem, Vec<SiteFixme>),
    /// An item a replaced record covered that is no longer covered.
    CoverageLoss(super::coverage_loss::LostItem),
}

impl AssessmentKind {
    // {impl}`requirements.authoring.initial_prompt`
    pub(crate) fn noun(&self) -> &'static str {
        match self {
            Self::Definition(kind, _) => kind.noun(),
            Self::Citation(_) => "citation review",
            Self::Site(..) => "site review",
            Self::CoverageLoss(_) => "coverage-loss review",
        }
    }
}

// {impl}`requirements.authoring.mandatory_inputs`
// {impl}`requirements.authoring.delivered_inputs`
pub(crate) struct AssessmentSources {
    pub(crate) root: PathBuf,
    pub(crate) access: SourceAccess,
    pub(crate) subject_name: String,
    pub(crate) subject_kind: AssessmentKind,
    pub(crate) subject_text: String,
    /// Parsed outlines, cached for this assessment.
    pub(crate) outlines: OutlineCache,
    /// Digest of the subject's reference set, recorded with the judgment.
    pub(crate) reference_set: String,
    mandatory: Vec<Value>,
    /// The state of each issue a `{fixme}` in each mandatory input names,
    /// supplied to the model with the input but never recorded.
    issue_states: Vec<Vec<(String, &'static str)>>,
    /// GitHub reads for the `github` tool and the states above.
    pub(crate) github: GithubIssues,
    recorded: Vec<Value>,
}

impl AssessmentSources {
    // {impl}`requirements.authoring.invocation`
    // {impl}`requirements.authoring.mandatory_inputs`
    // {impl}`requirements.authoring_input`
    // {impl}`requirements.examples.authoring_inputs`
    // {impl}`requirements.references.authoring_inputs`
    // {impl}`requirements.definitions.authoring_inputs`
    // {impl}`requirements.authoring.source_tools`
    // {impl}`requirements.references.anchors`
    // {impl}`requirements.ci.reference_set`
    // {impl}`requirements.references.builtin_definitions`
    pub(crate) fn prepare(root: &Path, subject_name: &str) -> io::Result<Self> {
        let root = fs::canonicalize(root)?;
        let access = SourceAccess::load(&root)?;
        let definitions = load_definitions(&root)?;
        let subject = definitions
            .get(subject_name)
            .filter(|definition| definition.kind != DefinitionKind::Term)
            .ok_or_else(|| {
                io::Error::other(format!(
                    "{subject_name}: no matching requirement or named example"
                ))
            })?;
        let mut selectors = vec![InputSelector::Requirements {
            requirements: subject_name.to_owned(),
        }];

        let mut builtins = Vec::new();
        for (kind, name) in crate::text_references(&subject.text, true) {
            // A `{fixme}` in a declaration is a validation error when named
            // and is not an edge when unnamed.
            if matches!(kind, ReferenceKind::Term | ReferenceKind::Fixme) {
                continue;
            }
            // A built-in has no declaration; its fixed text is supplied instead.
            if let Some(builtin) = crate::builtins::builtin(&name) {
                if kind == ReferenceKind::Cite && !builtins.contains(&builtin.name) {
                    builtins.push(builtin.name);
                }
                continue;
            }
            if !definitions
                .get(&name)
                .is_some_and(|definition| definition.kind != DefinitionKind::Term)
            {
                return Err(io::Error::other(format!(
                    "{subject_name}: referenced subject {name} does not exist"
                )));
            }
            // Only `{cite}` leaves the subject; its `{impl}` and `{test}`
            // references point into it from the definitions it implements or tests.
            if kind != ReferenceKind::Cite {
                continue;
            }
            insert_selector(
                &mut selectors,
                InputSelector::Requirements { requirements: name },
            );
        }

        let outlines = OutlineCache::default();
        let mut errors = Vec::new();
        let files = discovered_files(&root, &mut errors);
        let references = crate::evidence::collect_references_with_progress(
            &root,
            &files,
            &mut errors,
            &mut |_| {},
        );
        if !errors.is_empty() {
            return Err(io::Error::other(errors.join("\n")));
        }
        let sites =
            crate::evidence::definition_sites(&root, subject_name, subject, &files, &outlines)?;
        // The same edges determine the reference contributions below.
        let reference_set = crate::evidence::reference_set_digest(
            &root,
            &outlines,
            subject_name,
            subject,
            &references,
            &sites,
        );
        // `{impl}`, `{test}`, and `{fixme}` point from the subject to their
        // sources; `{cite}` sites point into the subject and are discoverable
        // with `references`. A `{fixme}` inside one of the subject's sites
        // belongs to that site's review instead.
        for reference in references.iter().filter(|reference| {
            reference.name == subject_name
                && !crate::sites::in_own_site(&root, &sites, reference)
                && reference.is_fixme_edge()
                && matches!(
                    reference.kind,
                    ReferenceKind::Impl | ReferenceKind::Test | ReferenceKind::Fixme
                )
        }) {
            // Generated evidence, including symlink aliases, is omitted by
            // default rather than denied.
            if access.omit_generated_evidence(&root, &reference.file) {
                continue;
            }
            access.file(&root, &reference.file)?;
            insert_selector(
                &mut selectors,
                reference_selector(&root, &definitions, &outlines, reference)?,
            );
        }

        // {impl}`requirements.references.anchors`
        for anchor in &subject.anchors {
            if access.omit_generated_evidence(&root, Path::new(&anchor.file)) {
                continue;
            }
            let selector = access
                .file(&root, Path::new(&anchor.file))
                .and_then(|_| {
                    anchor
                        .resolve(&root, &outlines)
                        .map(|_| anchor.selector())
                        .map_err(io::Error::other)
                })
                .map_err(|error| {
                    io::Error::new(
                        error.kind(),
                        format!(
                            "{subject_name}: cannot read :{}: anchor {}: {error}",
                            anchor.kind.label(),
                            anchor.label()
                        ),
                    )
                })?;
            insert_selector(&mut selectors, selector);
        }

        // Existing evidence only decides whether to reassess; its inputs are
        // not carried forward.
        let mut mandatory = Vec::new();
        for selector in selectors {
            if !generated_evidence_selector(&root, &access, &selector) {
                mandatory.push(source_input(
                    &root,
                    &access,
                    &definitions,
                    &outlines,
                    &selector,
                )?);
            }
        }
        let dependencies = definition_inputs(
            &root,
            &access,
            &definitions,
            &outlines,
            &mandatory,
            Some(&subject.text),
        )?;
        for input in dependencies {
            if !mandatory.contains(&input) {
                mandatory.push(input);
            }
        }
        mandatory.extend(builtins.into_iter().map(builtin_input));
        let github = GithubIssues::from_env(&root);
        let issue_states = crate::github::input_issue_states(
            &root,
            &github,
            &references,
            &definitions,
            &mandatory,
        );
        Ok(Self {
            root,
            access,
            subject_name: subject_name.to_owned(),
            subject_kind: AssessmentKind::Definition(
                subject.kind,
                crate::structural::has_query_directive(subject),
            ),
            subject_text: subject.text.clone(),
            outlines,
            reference_set,
            mandatory,
            issue_states,
            github,
            recorded: Vec::new(),
        })
    }

    /// Prepares a citation review of one source item outside the book. Its
    /// mandatory inputs are the item's outline node, each cited declaration
    /// or built-in text, and the terminology they use; the citations and
    /// their commentary are supplied in the prompt.
    // {impl}`requirements.authoring.citation_prompt`
    // {impl}`requirements.authoring.mandatory_inputs`
    pub(crate) fn prepare_citation(root: &Path, file: &str, item: &str) -> io::Result<Self> {
        let root = fs::canonicalize(root)?;
        let access = SourceAccess::load(&root)?;
        let definitions = load_definitions(&root)?;
        let outlines = OutlineCache::default();
        let mut errors = Vec::new();
        let references = collect_references(&root, &mut errors);
        if !errors.is_empty() {
            return Err(io::Error::other(errors.join("\n")));
        }
        let subject_name = crate::citations::subject_label(file, item);
        let citation = crate::citations::citation_items(
            &root,
            &definitions,
            &references,
            &outlines,
        )
        .remove(&(file.to_owned(), item.to_owned()))
        .ok_or_else(|| {
            io::Error::other(format!(
                "{subject_name}: no source item outside the book with {{cite}} references matches"
            ))
        })?;
        let mut mandatory = vec![source_input(
            &root,
            &access,
            &definitions,
            &outlines,
            &InputSelector::Outline {
                file: file.to_owned(),
                item: item.to_owned(),
            },
        )?];
        let mut builtins = Vec::new();
        let mut selectors = Vec::new();
        for cited in &citation.citations {
            if crate::builtins::builtin(&cited.name).is_some() {
                if !builtins.contains(&cited.name) {
                    builtins.push(cited.name.clone());
                }
            } else {
                insert_selector(
                    &mut selectors,
                    InputSelector::Requirements {
                        requirements: cited.name.clone(),
                    },
                );
            }
        }
        for selector in selectors {
            mandatory.push(source_input(
                &root,
                &access,
                &definitions,
                &outlines,
                &selector,
            )?);
        }
        for input in definition_inputs(&root, &access, &definitions, &outlines, &mandatory, None)? {
            if !mandatory.contains(&input) {
                mandatory.push(input);
            }
        }
        mandatory.extend(builtins.iter().map(|name| builtin_input(name)));
        let github = GithubIssues::from_env(&root);
        let issue_states = crate::github::input_issue_states(
            &root,
            &github,
            &references,
            &definitions,
            &mandatory,
        );
        Ok(Self {
            root,
            access,
            subject_name,
            subject_kind: AssessmentKind::Citation(citation),
            subject_text: String::new(),
            outlines,
            reference_set: String::new(),
            mandatory,
            issue_states,
            github,
            recorded: Vec::new(),
        })
    }

    /// Prepares the review of one site of a distributed requirement. Its
    /// mandatory inputs are the site's outline node, the requirement's
    /// complete declaration, each declaration or built-in text it cites, and
    /// the terminology they use; a `{fixme}` naming the requirement inside
    /// the site is part of the site's node.
    // {impl}`requirements.authoring.site_prompt`
    // {impl}`requirements.authoring.mandatory_inputs`
    // {impl}`requirements.references.builtin_definitions`
    pub(crate) fn prepare_site(
        root: &Path,
        requirement: &str,
        file: &str,
        item: &str,
    ) -> io::Result<Self> {
        let root = fs::canonicalize(root)?;
        let access = SourceAccess::load(&root)?;
        let definitions = load_definitions(&root)?;
        let subject_name = crate::sites::subject_label(requirement, file, item);
        let declaration = definitions
            .get(requirement)
            .filter(|definition| definition.kind == DefinitionKind::Requirement)
            .ok_or_else(|| {
                io::Error::other(format!(
                    "{subject_name}: no requirement named {requirement}"
                ))
            })?;
        let outlines = OutlineCache::default();
        let mut errors = Vec::new();
        let files = discovered_files(&root, &mut errors);
        let references = crate::evidence::collect_references_with_progress(
            &root,
            &files,
            &mut errors,
            &mut |_| {},
        );
        if !errors.is_empty() {
            return Err(io::Error::other(errors.join("\n")));
        }
        let site =
            crate::sites::requirement_sites(&root, requirement, declaration, &files, &outlines)
                .map_err(io::Error::other)?
                .into_iter()
                .find(|site| site.file == file && site.item == item)
                .ok_or_else(|| {
                    io::Error::other(format!("{subject_name}: no site of {requirement} matches"))
                })?;
        let mut selectors = vec![
            InputSelector::Outline {
                file: file.to_owned(),
                item: item.to_owned(),
            },
            InputSelector::Requirements {
                requirements: requirement.to_owned(),
            },
        ];
        let mut builtins = Vec::new();
        for (kind, name) in crate::text_references(&declaration.text, true) {
            if kind != ReferenceKind::Cite {
                continue;
            }
            if crate::builtins::builtin(&name).is_some() {
                if !builtins.contains(&name) {
                    builtins.push(name);
                }
            } else if definitions
                .get(&name)
                .is_some_and(|definition| definition.kind != DefinitionKind::Term)
            {
                insert_selector(
                    &mut selectors,
                    InputSelector::Requirements { requirements: name },
                );
            }
        }
        let mut mandatory = Vec::new();
        for selector in selectors {
            mandatory.push(source_input(
                &root,
                &access,
                &definitions,
                &outlines,
                &selector,
            )?);
        }
        for input in definition_inputs(&root, &access, &definitions, &outlines, &mandatory, None)? {
            if !mandatory.contains(&input) {
                mandatory.push(input);
            }
        }
        mandatory.extend(builtins.iter().map(|name| builtin_input(name)));
        let github = GithubIssues::from_env(&root);
        let issue_states = crate::github::input_issue_states(
            &root,
            &github,
            &references,
            &definitions,
            &mandatory,
        );
        let fixmes = crate::sites::site_fixmes(&root, &site, &references, &outlines);
        Ok(Self {
            root,
            access,
            subject_name,
            subject_kind: AssessmentKind::Site(site, fixmes),
            subject_text: String::new(),
            outlines,
            reference_set: String::new(),
            mandatory,
            issue_states,
            github,
            recorded: Vec::new(),
        })
    }

    /// Prepares the coverage-loss review of an item. Its mandatory inputs
    /// are the item's outline node, the declarations of the requirements and
    /// examples whose replaced records read it, and the terminology they use;
    /// those records' rationales are supplied in the prompt.
    // {impl}`requirements.authoring.mandatory_inputs`
    pub(crate) fn prepare_coverage_loss(
        root: &Path,
        lost: super::coverage_loss::LostItem,
    ) -> io::Result<Self> {
        let root = fs::canonicalize(root)?;
        let access = SourceAccess::load(&root)?;
        let definitions = load_definitions(&root)?;
        let outlines = OutlineCache::default();
        let mut selectors = vec![InputSelector::Outline {
            file: lost.file.clone(),
            item: lost.item.clone(),
        }];
        for reader in &lost.readers {
            insert_selector(
                &mut selectors,
                InputSelector::Requirements {
                    requirements: reader.definition.clone(),
                },
            );
        }
        let mut mandatory = Vec::new();
        for selector in selectors {
            mandatory.push(source_input(
                &root,
                &access,
                &definitions,
                &outlines,
                &selector,
            )?);
        }
        for input in definition_inputs(&root, &access, &definitions, &outlines, &mandatory, None)? {
            if !mandatory.contains(&input) {
                mandatory.push(input);
            }
        }
        let mut errors = Vec::new();
        let references = collect_references(&root, &mut errors);
        if !errors.is_empty() {
            return Err(io::Error::other(errors.join("\n")));
        }
        let github = GithubIssues::from_env(&root);
        let issue_states = crate::github::input_issue_states(
            &root,
            &github,
            &references,
            &definitions,
            &mandatory,
        );
        Ok(Self {
            root,
            access,
            subject_name: lost.label(),
            subject_kind: AssessmentKind::CoverageLoss(lost),
            subject_text: String::new(),
            outlines,
            reference_set: String::new(),
            mandatory,
            issue_states,
            github,
            recorded: Vec::new(),
        })
    }

    // {impl}`requirements.authoring.mandatory_inputs`
    pub(crate) fn mandatory_inputs(&self) -> &[Value] {
        &self.mandatory
    }

    /// The mandatory inputs as supplied to the model: each input containing
    /// a `{fixme}` that names issues also lists their states, `open` or
    /// `closed`, under `github_issues`.
    // {impl}`requirements.authoring.github_reads`
    // {impl}`requirements.authoring.mandatory_inputs`
    pub(crate) fn model_mandatory_inputs(&self) -> Vec<Value> {
        self.mandatory
            .iter()
            .zip(
                self.issue_states
                    .iter()
                    .chain(std::iter::repeat(&Vec::new())),
            )
            .map(|(input, states)| {
                let mut input = input.clone();
                if let (false, Some(object)) = (states.is_empty(), input.as_object_mut()) {
                    object.insert(
                        "github_issues".to_owned(),
                        states
                            .iter()
                            .map(|(issue, state)| json!({"issue": issue, "state": state}))
                            .collect(),
                    );
                }
                input
            })
            .collect()
    }

    /// Inputs recorded as evidence: outline inputs keep only their file,
    /// selector, and hash.
    // {impl}`requirements.authoring.delivered_inputs`
    pub(crate) fn recorded_inputs(&self) -> &[Value] {
        &self.recorded
    }

    // {impl}`requirements.authoring.delivered_inputs`
    // {impl}`requirements.ci.outline_inputs`
    pub(crate) fn record_inputs(&mut self, inputs: &[Value]) {
        // A built-in's fixed text is supplied to the model but is not an
        // evidence input form.
        for input in inputs.iter().filter(|input| input.get("builtin").is_none()) {
            let input = recorded_input(input);
            if !self.recorded.contains(&input) {
                self.recorded.push(input);
            }
        }
    }

    /// Reads a selector as a model-facing input, recording a project-local
    /// absolute path relative to the project.
    pub(crate) fn read_selector(&self, selector: &InputSelector) -> io::Result<Value> {
        let definitions = load_definitions(&self.root)?;
        let relative;
        let selector = match selector {
            InputSelector::Outline { file, item } => {
                relative = InputSelector::Outline {
                    file: crate::inputs::project_relative(&self.root, file),
                    item: item.clone(),
                };
                &relative
            }
            _ => selector,
        };
        source_input(
            &self.root,
            &self.access,
            &definitions,
            &self.outlines,
            selector,
        )
    }

    /// Parses a permitted file, aborting on access denial.
    // {impl}`requirements.authoring.outline_tool`
    pub(crate) fn parse_file(
        &self,
        file: &str,
    ) -> io::Result<std::sync::Arc<Result<ParsedSource, String>>> {
        self.access.file(&self.root, Path::new(file))?;
        Ok(self.outlines.parse(&self.root, file))
    }

    /// Reads the innermost outline nodes covering an inclusive line range,
    /// as model-facing outline inputs. With `comments`, comment lines select
    /// their comments' owners.
    // {impl}`requirements.ci.outline_inputs`
    pub(crate) fn read_lines(
        &self,
        file: &str,
        line_start: usize,
        line_end: Option<usize>,
        comments: bool,
    ) -> io::Result<Vec<Value>> {
        let parsed = self.parse_file(file)?;
        let parsed = parsed
            .as_ref()
            .as_ref()
            .map_err(|error| io::Error::other(error.clone()))?;
        let count = parsed.source.lines().count();
        let line_end = line_end.unwrap_or(count);
        if line_start == 0 || line_end < line_start || line_end > count {
            return Err(io::Error::other(format!(
                "{file}:{line_start}-{line_end} is not an inclusive range within the file's {count} lines"
            )));
        }
        parsed
            .nodes_for_range(line_start, line_end, comments)
            .into_iter()
            .map(|index| {
                self.read_selector(&InputSelector::Outline {
                    file: file.to_owned(),
                    item: parsed.outline.nodes[index].selector.clone(),
                })
            })
            .collect()
    }

    /// The additional definitions each input needs on its own, so a page of
    /// inputs can carry the definitions that accompany them.
    pub(crate) fn definitions_each(&self, inputs: &[Value]) -> io::Result<Vec<Vec<Value>>> {
        let definitions = load_definitions(&self.root)?;
        inputs
            .iter()
            .map(|input| {
                Ok(definition_inputs(
                    &self.root,
                    &self.access,
                    &definitions,
                    &self.outlines,
                    std::slice::from_ref(input),
                    None,
                )?
                .into_iter()
                .filter(|definition| definition != input && !self.recorded.contains(definition))
                .collect())
            })
            .collect()
    }

    pub(crate) fn additional_definitions(&self, inputs: &[Value]) -> io::Result<Vec<Value>> {
        let definitions = load_definitions(&self.root)?;
        Ok(definition_inputs(
            &self.root,
            &self.access,
            &definitions,
            &self.outlines,
            inputs,
            None,
        )?
        .into_iter()
        .filter(|input| !inputs.contains(input) && !self.recorded.contains(input))
        .collect())
    }
}

// {impl}`requirements.authoring.mandatory_inputs`
// {impl}`requirements.authoring.file_discovery`
pub(crate) fn generated_evidence_path(root: &Path, path: &Path) -> bool {
    let path = root.join(path);
    let evidence = root.join("evidence");
    path.starts_with(&evidence)
        || fs::canonicalize(&path)
            .is_ok_and(|path| path.starts_with(fs::canonicalize(&evidence).unwrap_or(evidence)))
}

// {impl}`requirements.authoring.mandatory_inputs`
// {impl}`requirements.configuration.evidence_paths`
fn generated_evidence_selector(
    root: &Path,
    access: &SourceAccess,
    selector: &InputSelector,
) -> bool {
    match selector {
        InputSelector::Outline { file, .. } => {
            access.omit_generated_evidence(root, Path::new(file))
        }
        InputSelector::Requirements { .. } | InputSelector::Definition { .. } => false,
    }
}

// {impl}`requirements.authoring.mandatory_inputs`
// {impl}`requirements.configuration.evidence_paths`
// {impl}`requirements.references.definition_globs`
fn source_input(
    root: &Path,
    access: &SourceAccess,
    definitions: &BTreeMap<String, Declaration>,
    outlines: &OutlineCache,
    selector: &InputSelector,
) -> io::Result<Value> {
    match selector {
        InputSelector::Outline { file, .. } => {
            access.file(root, Path::new(file))?;
        }
        InputSelector::Definition { definition } => {
            if let Some(definition) = definitions.get(definition) {
                access.file(root, Path::new(&definition.file))?;
            }
        }
        InputSelector::Requirements { requirements } => {
            for (name, definition) in definitions {
                if definition.kind != DefinitionKind::Term
                    && crate::definition_glob::matches(requirements, name)
                {
                    access.file(root, Path::new(&definition.file))?;
                }
            }
        }
    }
    let resolved = selector
        .resolve(root, definitions, outlines)
        .map_err(io::Error::other)?;
    for range in &resolved.ranges {
        access.file(root, Path::new(&range.file))?;
    }
    Ok(selector.to_model_input(&resolved))
}

/// The model-facing input carrying a built-in's fixed text.
// {impl}`requirements.authoring.mandatory_inputs`
pub(crate) fn builtin_input(name: &str) -> Value {
    let text = crate::builtins::builtin(name).map_or("", |builtin| builtin.text());
    json!({ "builtin": name, "text": [text] })
}

// {impl}`requirements.authoring.mandatory_inputs`
fn insert_selector(selectors: &mut Vec<InputSelector>, selector: InputSelector) {
    if !selectors.contains(&selector) {
        selectors.push(selector);
    }
}

/// The input a `{impl}` or `{test}` reference contributes: the enclosing
/// Markdown declaration, otherwise the smallest outline node the reference
/// belongs to, or the whole file in a plain-text source.
// {impl}`requirements.references.outline_comments`
fn reference_selector(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    outlines: &OutlineCache,
    reference: &SourceReference,
) -> io::Result<InputSelector> {
    let file = reference
        .file
        .strip_prefix(root)
        .map_err(|_| io::Error::other("reference is outside project root"))?
        .to_str()
        .ok_or_else(|| io::Error::other("reference path is not UTF-8"))?
        .to_owned();
    if crate::outline::is_markdown_path(&reference.file)
        && let Some((name, declaration)) = definitions
            .iter()
            .filter(|(_, declaration)| {
                declaration.file == file
                    && declaration.line_start <= reference.line
                    && reference.line <= declaration.line_end
            })
            .min_by_key(|(_, declaration)| declaration.line_end - declaration.line_start)
    {
        return Ok(if declaration.kind == DefinitionKind::Term {
            InputSelector::Definition {
                definition: name.clone(),
            }
        } else {
            InputSelector::Requirements {
                requirements: name.clone(),
            }
        });
    }
    let parsed = outlines.parse(root, &file);
    let parsed = parsed
        .as_ref()
        .as_ref()
        .map_err(|error| io::Error::other(format!("cannot outline referenced source: {error}")))?;
    let item = if parsed.outline.profile.language.is_some() {
        let index = parsed.outline.owner_at_line(reference.line);
        parsed.outline.nodes[index].selector.clone()
    } else {
        String::new()
    };
    Ok(InputSelector::Outline { file, item })
}

// {impl}`requirements.definitions.selection`
// {impl}`requirements.authoring.mandatory_inputs`
fn definition_inputs(
    root: &Path,
    access: &SourceAccess,
    definitions: &BTreeMap<String, Declaration>,
    outlines: &OutlineCache,
    inputs: &[Value],
    subject: Option<&str>,
) -> io::Result<Vec<Value>> {
    let record = json!({ "inputs": inputs });
    crate::terms::dependencies(
        root,
        definitions,
        outlines,
        record.as_object().unwrap(),
        subject,
    )
    .map_err(io::Error::other)?
    .into_iter()
    .map(|definition| {
        let selector = InputSelector::Definition { definition };
        source_input(root, access, definitions, outlines, &selector)
    })
    .collect()
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::authoring::access::tests::git;

    // {test}`requirements.authoring.mandatory_inputs`
    pub(crate) fn role(kind: &str, name: &str) -> String {
        format!("{{{kind}}}`{name}`")
    }

    // {test}`requirements.authoring.mandatory_inputs`
    pub(crate) fn project() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("md")).unwrap();
        fs::create_dir_all(root.path().join("src")).unwrap();
        fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::write(root.path().join("book.toml"), "[book]\nsrc='md'\n").unwrap();
        fs::write(
            root.path().join("md/requirements.md"),
            ":::{req} ready\nThe project MUST be ready.\n:::\n",
        )
        .unwrap();
        root
    }

    // {test}`requirements.authoring.complete_initial_bundle`
    // {test}`requirements.authoring.mandatory_inputs`
    // {test}`requirements.authoring_input`
    // {test}`requirements.examples.authoring_inputs`
    // {test}`requirements.references.authoring_inputs`
    // {test}`requirements.definitions.authoring_inputs`
    // {test}`requirements.ci.authoring_inputs`
    // {test}`requirements.ci.reference_set`
    #[test]
    fn bundle_combines_direct_references_terms_and_current_existing_selectors() {
        let root = project();
        let terms = format!(
            ":::{{definition}} unit\n:phrase: unit\nA {}.\n:::\n\n:::{{definition}} value\n:phrase: value\nA {}.\n:::\n",
            role("term", "terms.value"),
            role("term", "terms.unit")
        );
        fs::write(root.path().join("md/terms.md"), terms).unwrap();
        let declarations = format!(
            ":::{{req}} ready\nReady uses {} and {}.\n:::\n\n:::{{req}} base\nBase cites {}.\n:::\n\n:::{{req}} distant\nDistant MUST work.\n:::\n\n:::{{example}} case\nReady example. {} {}\n:::\n",
            role("cite", "requirements.base"),
            role("term", "terms.unit"),
            role("cite", "requirements.distant"),
            role("cite", "requirements.ready"),
            role("test", "requirements.ready")
        );
        fs::write(root.path().join("md/requirements.md"), declarations).unwrap();
        fs::write(root.path().join("src/lib.rs"), format!("struct Thing;\nimpl Thing {{\n    // {}\n    fn run(&self) {{}}\n    fn unrelated(&self) {{}}\n}}\n// {}\nfn test_ready() {{}}\n", role("impl", "requirements.ready"), role("test", "requirements.ready"))).unwrap();
        fs::write(root.path().join("notes.txt"), "new source text\n").unwrap();
        fs::create_dir(root.path().join("evidence")).unwrap();
        fs::write(
            root.path().join("evidence/requirements.ready.json"),
            json!({
                "determination": false, "rationale": "old secret judgment",
                "inputs": [{"file":"notes.txt", "line_start":1, "line_end":1, "text":["old text"]}]
            })
            .to_string(),
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        // The recorded digest covers the same `{impl}` and `{test}` edges.
        assert_eq!(
            sources.reference_set,
            crate::evidence::reference_set_digest_for(root.path(), "requirements.ready").unwrap()
        );
        let inputs = sources.mandatory_inputs();
        assert!(sources.recorded_inputs().is_empty());
        assert!(
            inputs
                .iter()
                .any(|input| input["item"] == "impl[Thing]/fn[run]")
        );
        assert!(inputs.iter().any(|input| input["item"] == "fn[test_ready]"));
        assert!(!inputs.iter().any(|input| input["item"] == "impl[Thing]"));
        assert!(
            inputs
                .iter()
                .any(|input| input["requirements"] == "requirements.case")
        );
        assert!(
            inputs
                .iter()
                .any(|input| input["requirements"] == "requirements.base")
        );
        assert!(
            !inputs
                .iter()
                .any(|input| input["requirements"] == "requirements.distant")
        );
        assert_eq!(
            inputs
                .iter()
                .filter(|input| input.get("definition").is_some())
                .count(),
            2
        );
        // Earlier evidence inputs are not carried forward.
        assert!(!inputs.iter().any(|input| input["file"] == "notes.txt"));
        assert!(
            !serde_json::to_string(inputs)
                .unwrap()
                .contains("old secret judgment")
        );
        let example = AssessmentSources::prepare(root.path(), "requirements.case").unwrap();
        assert!(matches!(
            example.subject_kind,
            AssessmentKind::Definition(DefinitionKind::Example, false)
        ));
        assert_eq!(
            example.mandatory_inputs()[0]["requirements"],
            "requirements.case"
        );
    }

    // {test}`requirements.authoring.mandatory_inputs`
    // {test}`requirements.ci.authoring_inputs`
    // {test}`requirements.authoring_input`
    #[test]
    fn unresolvable_earlier_inputs_do_not_block_reassessment() {
        let root = project();
        fs::create_dir(root.path().join("evidence")).unwrap();
        fs::write(
            root.path().join("evidence/requirements.ready.json"),
            json!({
                "inputs": [
                    {"file":"missing.txt", "line_start":1, "line_end":1},
                    {"rust_file":"src/lib.rs", "rust_item":"fn[removed]", "text":["fn removed() {}"]},
                    {"requirements":"requirements.removed", "matches":["requirements.removed"], "text":["gone"]}
                ]
            })
            .to_string(),
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        assert_eq!(
            sources.mandatory_inputs(),
            &[json!({
                "requirements": "requirements.ready",
                "matches": ["requirements.ready"],
                "text": [":::{req} ready", "The project MUST be ready.", ":::"]
            })]
        );
        assert!(AssessmentSources::prepare(root.path(), "missing").is_err());
    }

    // {test}`requirements.authoring.mandatory_inputs`
    // {test}`requirements.ci.authoring_inputs`
    #[test]
    fn earlier_evidence_inputs_and_judgments_are_not_carried_forward() {
        let root = project();
        fs::write(root.path().join("notes.txt"), "current source\n").unwrap();
        fs::create_dir_all(root.path().join("evidence")).unwrap();
        let old = json!({
            "requirement": "The project MUST be ready.",
            "determination": false,
            "rationale": "old secret judgment",
            "inputs": [
                {"file":"evidence/requirements.ready.json", "line_start":1, "line_end":1, "text":["old judgment"]},
                {"file":"notes.txt", "line_start":1, "line_end":1, "text":["old source"]},
            ]
        });
        let path = root.path().join("evidence/requirements.ready.json");
        fs::write(&path, old.to_string()).unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let supplied = serde_json::to_string(sources.mandatory_inputs()).unwrap();
        assert!(!supplied.contains("notes.txt"), "{supplied}");
        assert!(!supplied.contains("evidence/"), "{supplied}");
        assert!(!supplied.contains("old secret judgment"), "{supplied}");
        assert_eq!(
            serde_json::from_str::<Value>(&fs::read_to_string(path).unwrap()).unwrap(),
            old
        );
    }

    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.mandatory_inputs`
    // {test}`requirements.definitions.authoring_inputs`
    // {test}`requirements.authoring_input`
    // {test}`requirements.references.authoring_inputs`
    // {test}`requirements.authoring.read_denial`
    // {test}`requirements.references.discovery`
    #[test]
    fn mandatory_git_inputs_require_staging_declarations_sources_and_terms() {
        let root = project();
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["add", "Cargo.toml", "book.toml"]);
        let error = AssessmentSources::prepare(root.path(), "requirements.ready")
            .err()
            .unwrap();
        assert!(
            error.to_string().contains("md/requirements.md")
                && error.to_string().contains("git add"),
            "{error}"
        );
        git(root.path(), &["add", "md"]);
        fs::write(
            root.path().join("src/lib.rs"),
            format!(
                "// {}\nfn ready() {{}}\n",
                role("impl", "requirements.ready")
            ),
        )
        .unwrap();
        // Discovery does not consider an untracked source, so its reference is
        // not yet an input; staging it makes the source mandatory.
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let supplied = serde_json::to_string(sources.mandatory_inputs()).unwrap();
        assert!(!supplied.contains("src/lib.rs"), "{supplied}");
        git(root.path(), &["add", "src"]);
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let supplied = serde_json::to_string(sources.mandatory_inputs()).unwrap();
        assert!(supplied.contains("src/lib.rs"), "{supplied}");
        fs::write(
            root.path().join("md/terms.md"),
            ":::{definition} ready\n:phrase: ready\nReady to proceed.\n:::\n",
        )
        .unwrap();
        fs::write(
            root.path().join("src/lib.rs"),
            format!(
                "// {} {}\nfn ready() {{}}\n",
                role("impl", "requirements.ready"),
                role("term", "terms.ready")
            ),
        )
        .unwrap();
        let error = AssessmentSources::prepare(root.path(), "requirements.ready")
            .err()
            .unwrap();
        assert!(
            error.to_string().contains("md/terms.md") && error.to_string().contains("git add"),
            "{error}"
        );
        git(root.path(), &["add", "md/terms.md"]);
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        assert!(
            sources
                .mandatory_inputs()
                .iter()
                .any(|input| input["definition"] == "terms.ready")
        );
    }

    // {test}`requirements.authoring.read_denial`
    #[cfg(unix)]
    #[test]
    fn mandatory_inputs_through_symlinks_leaving_the_project_abort_preparation() {
        let root = project();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.rs"), "fn secret() {}\n").unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("secret.rs"),
            root.path().join("src/escape.rs"),
        )
        .unwrap();
        fs::write(
            root.path().join("md/requirements.md"),
            ":::{req} ready\n:impl: src/escape.rs\nThe project MUST be ready.\n:::\n",
        )
        .unwrap();
        let error = AssessmentSources::prepare(root.path(), "requirements.ready")
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
        assert!(error.to_string().contains("src/escape.rs"), "{error}");
        assert!(error.to_string().contains("outside the project"), "{error}");
    }

    // {test}`requirements.configuration.evidence_paths`
    // {test}`requirements.authoring.mandatory_inputs`
    // {test}`requirements.authoring.read_denial`
    #[test]
    fn disallowed_mandatory_inputs_abort_preparation() {
        let root = project();
        fs::write(
            root.path().join("habeas.toml"),
            "version = 1\n[evidence]\ndisallow = ['md/requirements.md']\n",
        )
        .unwrap();
        let error = AssessmentSources::prepare(root.path(), "requirements.ready")
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
        assert!(error.to_string().contains("md/requirements.md"), "{error}");
    }

    // {test}`requirements.authoring.mandatory_inputs`
    // {test}`requirements.authoring.coverage_loss`
    #[test]
    fn coverage_loss_inputs_combine_the_item_reader_declarations_and_terms_once() {
        use crate::authoring::coverage_loss::{LostItem, Reader};
        let root = project();
        fs::write(
            root.path().join("md/terms.md"),
            ":::{definition} unit\n:phrase: unit\nA unit.\n:::\n",
        )
        .unwrap();
        fs::write(
            root.path().join("md/requirements.md"),
            format!(
                ":::{{req}} ready\nReady uses {}.\n:::\n\n:::{{req}} other\nOther uses {}.\n:::\n",
                role("term", "terms.unit"),
                role("term", "terms.unit")
            ),
        )
        .unwrap();
        fs::write(root.path().join("src/lib.rs"), "fn lost() {}\n").unwrap();
        let reader = |definition: &str| Reader {
            definition: definition.to_owned(),
            rationale: "Read it.".to_owned(),
        };
        let lost = |readers: Vec<Reader>| LostItem {
            file: "src/lib.rs".to_owned(),
            item: "fn[lost]".to_owned(),
            readers,
        };
        let sources = AssessmentSources::prepare_coverage_loss(
            root.path(),
            lost(vec![
                reader("requirements.ready"),
                reader("requirements.other"),
                reader("requirements.ready"),
            ]),
        )
        .unwrap();
        let inputs = sources.mandatory_inputs();
        // The item first, each reader's declaration once, and the shared term
        // once.
        assert_eq!(inputs[0]["item"], "fn[lost]", "{inputs:#?}");
        let count =
            |key: &str, value: &str| inputs.iter().filter(|input| input[key] == value).count();
        assert_eq!(
            count("requirements", "requirements.ready"),
            1,
            "{inputs:#?}"
        );
        assert_eq!(
            count("requirements", "requirements.other"),
            1,
            "{inputs:#?}"
        );
        assert_eq!(count("definition", "terms.unit"), 1, "{inputs:#?}");
        assert_eq!(inputs.len(), 4, "{inputs:#?}");

        // An unresolvable mandatory input aborts preparation.
        fs::write(
            root.path().join("habeas.toml"),
            "version = 1\n[evidence]\ndisallow = ['md/terms.md']\n",
        )
        .unwrap();
        let error = AssessmentSources::prepare_coverage_loss(
            root.path(),
            lost(vec![reader("requirements.ready")]),
        )
        .err()
        .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
        assert!(error.to_string().contains("md/terms.md"), "{error}");
    }

    // {test}`requirements.authoring.mandatory_inputs`
    // {test}`requirements.references.citation_review`
    #[test]
    fn citation_review_inputs_combine_item_declarations_terms_and_built_ins_once() {
        let root = project();
        fs::write(
            root.path().join("md/terms.md"),
            ":::{definition} unit\n:phrase: unit\nA unit.\n:::\n",
        )
        .unwrap();
        fs::write(
            root.path().join("md/requirements.md"),
            format!(
                ":::{{req}} ready\nReady uses {}.\n:::\n\n:::{{req}} other\nOther uses {}.\n:::\n",
                role("term", "terms.unit"),
                role("term", "terms.unit")
            ),
        )
        .unwrap();
        let none = ["// :::", "{cite} habeas:none\n// Glue.\n// :::\n"].concat();
        fs::write(
            root.path().join("src/lib.rs"),
            format!(
                "// {}\n// {}\n// {}\n{none}{none}fn helper() {{}}\n",
                role("cite", "requirements.ready"),
                role("cite", "requirements.ready"),
                role("cite", "requirements.other"),
            ),
        )
        .unwrap();
        let sources =
            AssessmentSources::prepare_citation(root.path(), "src/lib.rs", "fn[helper]").unwrap();
        let inputs = sources.mandatory_inputs();
        // The item, each cited declaration once, the shared term once, and the
        // built-in text once.
        assert_eq!(inputs[0]["item"], "fn[helper]");
        let count =
            |key: &str, value: &str| inputs.iter().filter(|input| input[key] == value).count();
        assert_eq!(
            count("requirements", "requirements.ready"),
            1,
            "{inputs:#?}"
        );
        assert_eq!(
            count("requirements", "requirements.other"),
            1,
            "{inputs:#?}"
        );
        assert_eq!(count("definition", "terms.unit"), 1, "{inputs:#?}");
        assert_eq!(count("builtin", "habeas:none"), 1, "{inputs:#?}");
        assert_eq!(inputs.len(), 5, "{inputs:#?}");

        // An unresolvable mandatory input aborts preparation.
        fs::write(
            root.path().join("habeas.toml"),
            "version = 1\n[evidence]\ndisallow = ['md/terms.md']\n",
        )
        .unwrap();
        let error = AssessmentSources::prepare_citation(root.path(), "src/lib.rs", "fn[helper]")
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
        assert!(error.to_string().contains("md/terms.md"), "{error}");
    }

    // {test}`requirements.references.authoring_inputs`
    // {test}`requirements.references.roles`
    // {test}`requirements.authoring.mandatory_inputs`
    #[test]
    fn only_edges_leaving_the_subject_contribute_inputs() {
        let root = project();
        let declarations = format!(
            ":::{{req}} ready\nReady relies on {} and refines {}.\n:::\n\n:::{{req}} base\nBase MUST hold.\n:::\n\n:::{{req}} parent\nParent MUST hold.\n:::\n\n:::{{example}} citer\nA case relying on ready. {}\n:::\n\n:::{{example}} tester\nA ready case. {} {}\n:::\n",
            role("cite", "requirements.base"),
            role("impl", "requirements.parent"),
            role("cite", "requirements.ready"),
            role("cite", "requirements.base"),
            role("test", "requirements.ready")
        );
        fs::write(root.path().join("md/requirements.md"), declarations).unwrap();
        fs::write(
            root.path().join("src/lib.rs"),
            format!(
                "// {}\nfn run() {{}}\n// {}\nfn helper() {{}}\n// {}\nfn check() {{}}\n",
                role("impl", "requirements.ready"),
                role("cite", "requirements.ready"),
                role("test", "requirements.ready")
            ),
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let inputs = sources.mandatory_inputs();
        let has_requirements =
            |name: &str| inputs.iter().any(|input| input["requirements"] == name);
        let has_item = |item: &str| inputs.iter().any(|input| input["item"] == item);
        assert!(has_requirements("requirements.ready"));
        assert!(has_requirements("requirements.base"), "{inputs:#?}");
        assert!(has_requirements("requirements.tester"), "{inputs:#?}");
        assert!(has_item("fn[run]"), "{inputs:#?}");
        assert!(has_item("fn[check]"), "{inputs:#?}");
        assert!(!has_item("fn[helper]"), "{inputs:#?}");
        assert!(!has_requirements("requirements.citer"), "{inputs:#?}");
        assert!(!has_requirements("requirements.parent"), "{inputs:#?}");

        let parent = AssessmentSources::prepare(root.path(), "requirements.parent").unwrap();
        let inputs = parent.mandatory_inputs();
        assert!(
            inputs
                .iter()
                .any(|input| input["requirements"] == "requirements.ready"),
            "{inputs:#?}"
        );
        assert!(
            !inputs
                .iter()
                .any(|input| input["requirements"] == "requirements.base"),
            "{inputs:#?}"
        );
    }

    // {test}`requirements.references.anchors`
    // {test}`requirements.references.authoring_inputs`
    // {test}`requirements.authoring.mandatory_inputs`
    // {test}`requirements.authoring_input`
    #[test]
    fn anchored_files_contribute_root_node_inputs() {
        let root = project();
        fs::create_dir_all(root.path().join("skills")).unwrap();
        fs::write(root.path().join("skills/ready.md"), "# Skill\nBe ready.\n").unwrap();
        fs::write(
            root.path().join("md/requirements.md"),
            ":::{req} ready\n:impl: skills/ready.md\n:test: skills/ready.md\nThe project MUST be ready.\n:::\n",
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let inputs = sources.mandatory_inputs();
        let hash =
            crate::inputs::outline_input(root.path(), "skills/ready.md", "").unwrap()["hash"]
                .clone();
        assert_eq!(
            inputs,
            &[
                json!({
                    "requirements": "requirements.ready",
                    "matches": ["requirements.ready"],
                    "text": [":::{req} ready", "The project MUST be ready.", ":::"]
                }),
                json!({
                    "file": "skills/ready.md",
                    "item": "",
                    "hash": hash,
                    "kind": "file",
                    "line_start": 1,
                    "line_end": 2,
                    "text": ["# Skill", "Be ready."]
                }),
            ]
        );
        assert_eq!(sources.subject_text, "The project MUST be ready.");
    }

    // {test}`requirements.references.anchors`
    // {test}`requirements.authoring.read_denial`
    #[test]
    fn unreadable_or_denied_anchors_fail_before_inference() {
        let root = project();
        fs::write(
            root.path().join("md/requirements.md"),
            ":::{req} ready\n:impl: skills/missing.md\nThe project MUST be ready.\n:::\n",
        )
        .unwrap();
        let error = AssessmentSources::prepare(root.path(), "requirements.ready")
            .err()
            .unwrap();
        assert!(
            error
                .to_string()
                .contains("requirements.ready: cannot read :impl: anchor skills/missing.md"),
            "{error}"
        );

        fs::create_dir_all(root.path().join("private")).unwrap();
        fs::write(root.path().join("private/skill.md"), "secret\n").unwrap();
        fs::write(
            root.path().join("md/requirements.md"),
            ":::{req} ready\n:impl: private/skill.md\nThe project MUST be ready.\n:::\n",
        )
        .unwrap();
        fs::write(
            root.path().join("habeas.toml"),
            "version = 1\n[evidence]\ndisallow = ['private']\n",
        )
        .unwrap();
        let error = AssessmentSources::prepare(root.path(), "requirements.ready")
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
        assert!(error.to_string().contains("private/skill.md"), "{error}");
    }

    // {test}`requirements.references.authoring_inputs`
    // {test}`requirements.references.builtin_definitions`
    #[test]
    fn cited_built_ins_contribute_their_text_and_fixmes_in_own_sites_do_not() {
        let project = crate::authoring::tests::citing_site_project();
        let root = project.path();
        let fixme = |issue: &str| {
            format!(
                "    // :::{{fixme}} requirements.rule\n    // :github: o/r#{issue}\n    // Panics instead of reporting.\n    // :::\n"
            )
        };
        let source = crate::sites::tests::SOURCE
            .replace(
                "fn check_two() {}",
                &format!("fn check_two() {{\n{}}}", fixme("1")),
            )
            .replace(
                "fn other() { panic!(\"y\"); }",
                &format!("fn other() {{\n{}}}", fixme("2")),
            );
        fs::write(root.join("src/lib.rs"), source).unwrap();
        let sources = AssessmentSources::prepare(root, "requirements.rule").unwrap();
        let mandatory = sources.mandatory_inputs();
        let items = mandatory
            .iter()
            .filter_map(|input| input["item"].as_str())
            .collect::<Vec<_>>();
        // The `{fixme}` outside every site contributes its node; the one
        // inside the subject's own site belongs to that site's review.
        assert_eq!(items, ["fn[other]"], "{mandatory:#?}");
        // A cited built-in contributes its fixed text once, and a cited
        // declaration its own declaration.
        assert_eq!(
            mandatory
                .iter()
                .filter(|input| input["builtin"] == "habeas:none")
                .collect::<Vec<_>>(),
            [&builtin_input("habeas:none")]
        );
        assert!(
            mandatory
                .iter()
                .any(|input| input["requirements"] == "requirements.other"),
            "{mandatory:#?}"
        );
    }

    // {test}`requirements.references.authoring_inputs`
    // {test}`requirements.references.outline_comments`
    // {test}`requirements.references.anchors`
    // {test}`requirements.authoring.mandatory_inputs`
    #[test]
    fn references_contribute_their_owning_outline_nodes() {
        let root = project();
        fs::create_dir_all(root.path().join("docs")).unwrap();
        fs::write(
            root.path().join("md/requirements.md"),
            ":::{req} ready\n:impl: src/lib.rs#mod[inner]\nThe project MUST be ready.\n:::\n",
        )
        .unwrap();
        // A `{fixme}` contributes the node its comment belongs to; an
        // unnamed one is not an edge and contributes nothing.
        fs::write(
            root.path().join("src/lib.rs"),
            "mod inner {\n    fn unrelated() {}\n}\n\n// :::{fixme} requirements.ready\n// The gap.\n// :::\nfn gap() {}\n\n// :::{fixme}\n// :github: example/app#1\n// :::\nfn tracked() {}\n",
        )
        .unwrap();
        fs::write(
            root.path().join("docs/guide.md"),
            format!(
                "# Intro\n\nText.\n\n# Usage\n\nSee {}.\n",
                role("impl", "requirements.ready")
            ),
        )
        .unwrap();
        fs::write(
            root.path().join("notes.txt"),
            format!("first\n{}\n", role("test", "requirements.ready")),
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let items = sources
            .mandatory_inputs()
            .iter()
            .filter(|input| input.get("item").is_some())
            .map(|input| {
                (
                    input["file"].as_str().unwrap(),
                    input["item"].as_str().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            items,
            [
                ("docs/guide.md", "section[Usage]"),
                // A plain-text file contributes the complete file.
                ("notes.txt", ""),
                ("src/lib.rs", "fn[gap]"),
                ("src/lib.rs", "mod[inner]"),
            ]
        );
    }

    // {test}`requirements.references.authoring_inputs`
    #[test]
    fn references_in_terminology_declarations_contribute_that_declaration() {
        let root = project();
        fs::write(
            root.path().join("md/requirements.md"),
            format!(
                ":::{{req}} ready\nThe project MUST be ready.\n:::\n\n:::{{definition}} widget\n:phrase: widget\n\nA part that is ready, as {} shows.\n:::\n",
                role("test", "requirements.ready")
            ),
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let definitions = sources
            .mandatory_inputs()
            .iter()
            .filter_map(|input| input.get("definition"))
            .collect::<Vec<_>>();
        assert_eq!(definitions, [&json!("requirements.widget")]);
        // The declaration contributes as a whole, not as a Markdown section.
        assert!(
            !sources
                .mandatory_inputs()
                .iter()
                .any(|input| input["file"] == "md/requirements.md"),
            "{:?}",
            sources.mandatory_inputs()
        );
    }

    // {test}`requirements.authoring.mandatory_inputs`
    // {test}`requirements.authoring.read_denial`
    #[test]
    fn generated_evidence_anchors_are_omitted_unless_allowed_and_abort_when_disallowed() {
        let root = project();
        fs::create_dir_all(root.path().join("evidence")).unwrap();
        fs::write(root.path().join("evidence/report.md"), "# Report\n").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("evidence", root.path().join("alias")).unwrap();
        let declaration = |anchor: &str| {
            fs::write(
                root.path().join("md/requirements.md"),
                format!(":::{{req}} ready\n:impl: {anchor}\nThe project MUST be ready.\n:::\n"),
            )
            .unwrap();
        };
        let has_file = |sources: &AssessmentSources, file: &str| {
            sources
                .mandatory_inputs()
                .iter()
                .any(|input| input["file"] == file)
        };

        // Default exclusion omits the anchor instead of aborting.
        declaration("evidence/report.md");
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        assert!(
            !has_file(&sources, "evidence/report.md"),
            "{:#?}",
            sources.mandatory_inputs()
        );
        assert_eq!(sources.mandatory_inputs().len(), 1);

        // A symlink alias into `evidence/` is omitted too.
        #[cfg(unix)]
        {
            declaration("alias/report.md");
            let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
            assert!(
                !has_file(&sources, "alias/report.md"),
                "{:#?}",
                sources.mandatory_inputs()
            );
            assert_eq!(sources.mandatory_inputs().len(), 1);
        }

        // An explicit allow permits the selection.
        declaration("evidence/report.md");
        fs::write(
            root.path().join("habeas.toml"),
            "version = 1\n[evidence]\nallow = ['evidence/report.md']\n",
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        assert!(
            has_file(&sources, "evidence/report.md"),
            "{:#?}",
            sources.mandatory_inputs()
        );

        // An explicit disallow aborts before inference.
        fs::write(
            root.path().join("habeas.toml"),
            "version = 1\n[evidence]\ndisallow = ['evidence']\n",
        )
        .unwrap();
        let error = AssessmentSources::prepare(root.path(), "requirements.ready")
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
        assert!(error.to_string().contains("evidence/report.md"), "{error}");
    }

    // {test}`requirements.authoring.mandatory_inputs`
    #[test]
    fn generated_evidence_references_are_omitted_by_default() {
        let root = project();
        fs::create_dir_all(root.path().join("evidence")).unwrap();
        fs::write(
            root.path().join("evidence/notes.md"),
            format!("# Notes\n\n{}\n", role("impl", "requirements.ready")),
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        assert!(
            !sources
                .mandatory_inputs()
                .iter()
                .any(|input| input["file"] == "evidence/notes.md"),
            "{:#?}",
            sources.mandatory_inputs()
        );
    }
}

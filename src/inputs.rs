use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::definition_glob;
use crate::evidence::Declaration;
use crate::outline_items::EvidenceOutline;

// {cite}`habeas:util`
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum InputSelector {
    Outline { file: String, item: String },
    Definition { definition: String },
    Requirements { requirements: String },
}

// {impl}`requirements.index.detail_format`
#[derive(Clone, Debug)]
pub(crate) struct SourceRange {
    pub(crate) file: String,
    pub(crate) line_start: usize,
    pub(crate) line_end: usize,
    pub(crate) name: Option<String>,
}

/// The outline node an outline input currently selects.
// {impl}`requirements.index.detail_format`
#[derive(Clone, Debug)]
pub(crate) struct ResolvedNode {
    /// Canonical path of the parsed file.
    pub(crate) path: PathBuf,
    pub(crate) index: usize,
    pub(crate) kind: String,
    pub(crate) hash: String,
}

// {cite}`habeas:util`
pub(crate) struct ResolvedInput {
    pub(crate) ranges: Vec<SourceRange>,
    /// Current source text; recorded only for requirements and definition inputs.
    pub(crate) text: Vec<String>,
    pub(crate) matches: Vec<String>,
    pub(crate) node: Option<ResolvedNode>,
}

// {cite}`habeas:util`
/// A parsed source file.
pub(crate) struct ParsedSource {
    /// Canonical path.
    pub(crate) path: PathBuf,
    pub(crate) snapshot: trust_but_verify::outline::OutlineContent,
    pub(crate) source: String,
    /// The source as models receive it and Habeas displays it, with the same
    /// lines as `source`.
    pub(crate) redacted: String,
    pub(crate) outline: EvidenceOutline,
}

#[cfg(test)]
mod ownership_tests {
    use super::*;

    // {test}`requirements.references.outline_comments`
    // {test}`requirements.references.authoring_inputs`
    // {test}`requirements.authoring.reference_index`
    #[test]
    fn markdown_references_select_the_nearest_directive_or_standalone_paragraph() {
        let source = "# H\nAn ordinary citation.\n\n::::{note}\nOuter citation.\n\n:::{tip}\nInner citation.\n:::\n::::\n";
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.md"), source).unwrap();
        let cache = OutlineCache::default();
        let parsed = cache.parse(dir.path(), "a.md");
        let parsed = parsed.as_ref().as_ref().unwrap();
        let ordinary = parsed.reference_owner(2, 1);
        assert_eq!(parsed.outline.nodes[ordinary].kind, "paragraph");
        let outer = parsed.reference_owner(5, 1);
        let inner = parsed.reference_owner(8, 1);
        assert_eq!(parsed.outline.nodes[outer].kind, "block");
        assert_eq!(parsed.outline.nodes[inner].kind, "block");
        assert_eq!(parsed.outline.nodes[inner].parent, Some(outer));
        assert_eq!(parsed.outline.nodes[inner].line_start, 7);
        assert_eq!(
            parsed.outline.nodes[parsed.outline.ownership.owner_at(8, 1)].kind,
            "paragraph"
        );
        let rust = "fn run() {}\n";
        fs::write(dir.path().join("a.rs"), rust).unwrap();
        let parsed = cache.parse(dir.path(), "a.rs");
        let parsed = parsed.as_ref().as_ref().unwrap();
        assert_eq!(
            parsed.reference_owner(1, 1),
            parsed.outline.ownership.owner_at(1, 1)
        );
    }
}

// {cite}`habeas:util`
pub(crate) type SourceCache = trust_but_verify::domain::ValidationCache<crate::sdk::HabeasDomain>;

/// Parsed outlines, cached per file for one validation or authoring run.
/// Clones share entries populated by any caller in that run.
#[derive(Default)]
pub(crate) struct OutlineCache {
    pub(crate) validation: SourceCache,
    files: Arc<Mutex<HashMap<PathBuf, Arc<Result<ParsedSource, String>>>>>,
    /// Content to parse in place of a file's, by project-relative path.
    overrides: Arc<HashMap<String, String>>,
    catalogs: Arc<Mutex<HashMap<PathBuf, Arc<Result<ReferenceCatalog, String>>>>>,
    /// Source visibility for agent reads. Saved evidence uses its recorded-input resolver.
    project_sources: Option<Arc<crate::authoring::access::SourceAccess>>,
}

// {cite}`habeas:util`
pub(crate) struct ReferenceCatalog {
    pub references: Vec<crate::evidence::SourceReference>,
    pub files: Vec<PathBuf>,
}

impl Clone for OutlineCache {
    // {cite}`habeas:util`
    fn clone(&self) -> Self {
        Self {
            validation: self.validation.clone(),
            files: self.files.clone(),
            overrides: self.overrides.clone(),
            catalogs: self.catalogs.clone(),
            project_sources: self.project_sources.clone(),
        }
    }
}

impl OutlineCache {
    // {cite}`habeas:util`
    /// A cache that parses `overrides` in place of those files' contents.
    pub(crate) fn with_overrides(overrides: HashMap<String, String>) -> Self {
        Self {
            overrides: Arc::new(overrides),
            ..Self::default()
        }
    }

    /// Start a model-facing view sharing the domain's retained SDK outlines.
    pub(crate) fn for_authoring(
        &self,
        access: Arc<crate::authoring::access::SourceAccess>,
    ) -> Self {
        Self {
            project_sources: Some(access),
            files: Default::default(),
            ..self.clone()
        }
    }

    fn check_project_file(&self, root: &Path, file: &str) -> Result<PathBuf, String> {
        match &self.project_sources {
            Some(sources) => sources.file(root, Path::new(file)),
            None => crate::authoring::access::resolve_evidence_file(root, file),
        }
        .map_err(|error| error.to_string())
    }

    /// Repository relationships share one discovery snapshot per inspection.
    pub(crate) fn reference_catalog(&self, root: &Path) -> Arc<Result<ReferenceCatalog, String>> {
        if let Some(catalog) = self
            .catalogs
            .lock()
            .expect("reference catalog lock")
            .get(root)
        {
            return catalog.clone();
        }
        // A discovery failure aborts; this is not a validation site.
        let mut failures = Vec::new();
        let files = crate::evidence::discovered_files(root, &mut failures);
        let references = crate::evidence::collect_references_with_progress(
            &self.validation,
            root,
            &files,
            &mut failures,
            &mut |_| {},
        );
        let catalog = Arc::new(if failures.is_empty() {
            Ok(ReferenceCatalog { references, files })
        } else {
            Err(failures.join("\n"))
        });
        self.catalogs
            .lock()
            .expect("reference catalog lock")
            .entry(root.into())
            .or_insert(catalog)
            .clone()
    }

    /// The retained SDK outline for a permitted file or supplied refactoring snapshot.
    pub(crate) fn snapshot(
        &self,
        root: &Path,
        file: &str,
    ) -> Result<trust_but_verify::outline::OutlineContent, String> {
        use trust_but_verify::outline::OutlineContent;
        let snapshot = if let Some(sources) = &self.project_sources {
            let (path, file) = sources
                .read(root, Path::new(file))
                .map_err(|error| error.to_string())?;
            OutlineContent::from_source(
                &self.validation,
                path,
                file.text().map_err(|error| error.to_string())?.into(),
            )
        } else {
            let path = self.check_project_file(root, file)?;
            match self.overrides.get(file) {
                Some(text) => {
                    OutlineContent::from_source(&self.validation, &path, text.as_str().into())
                }
                None => OutlineContent::from_path(&self.validation, &path),
            }
        }
        .map_err(|error| format!("cannot read {file}: {error}"))?;
        if let Some(error) = &snapshot.outline().diagnostic {
            let language = snapshot.language();
            return Err(format!("{file}: cannot parse {language} source: {error}"));
        }
        Ok(snapshot)
    }

    // {impl}`requirements.authoring.outline_tool`
    /// Reads and parses a permitted file, or returns the cached result.
    pub(crate) fn parse(&self, root: &Path, file: &str) -> Arc<Result<ParsedSource, String>> {
        let key = root.join(file);
        if let Some(parsed) = self.files.lock().expect("outline cache lock").get(&key) {
            return parsed.clone();
        }
        let parsed = Arc::new(self.snapshot(root, file).and_then(|snapshot| {
            let path = snapshot.id().path.clone();
            let source = snapshot.text().to_owned();
            let public = snapshot.outline();
            let items = crate::outline_items::OutlineItems::new(public, &source);
            let outline = EvidenceOutline::new(public, items, snapshot.language(), &source);
            Ok(ParsedSource {
                redacted: snapshot
                    .source_span_redacted(public.items[0].span)
                    .expect("validated file-root span")
                    .into_owned(),
                snapshot,
                path: fs::canonicalize(&path).unwrap_or(path),
                source,
                outline,
            })
        }));
        self.files
            .lock()
            .expect("outline cache lock")
            .insert(key, parsed.clone());
        parsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // {test}`requirements.references.outline_comments`
    #[test]
    fn evidence_preserves_attachment_lines_and_declaration_positions() {
        let dir = tempfile::tempdir().unwrap();
        let source = "/// Docs.\n#[derive(Debug)]\nstruct One; struct Two;\n";
        fs::write(dir.path().join("a.rs"), source).unwrap();
        let cache = OutlineCache::default();
        let parsed = cache.parse(dir.path(), "a.rs");
        let parsed = parsed.as_ref().as_ref().unwrap();
        let first = parsed.resolve("a.rs", "struct[One]").unwrap();
        let second = parsed.resolve("a.rs", "struct[Two]").unwrap();
        let node = &parsed.outline.nodes[first];
        assert_eq!(
            (node.line_start, node.declaration_line, node.line_end),
            (1, 3, 3)
        );
        assert_eq!(parsed.reference_owner(3, 13), second);
        assert_eq!(parsed.nodes_for_range(3, 3, false), [first, second]);
        assert_eq!(parsed.reference_owner(1, 1), first);
    }

    // {test}`requirements.references.outline_comments`
    #[test]
    fn references_in_sibling_comments_on_one_line_use_their_byte_columns() {
        let dir = tempfile::tempdir().unwrap();
        let source = "/// docs\r\nstruct Café; /* {impl}`one` */ struct Two; /* :::{cite} two\r\n * body\r\n * ::: */\r\n";
        let file = dir.path().join("a.rs");
        fs::write(&file, source).unwrap();
        let cache = OutlineCache::default();
        let references = crate::evidence::file_references(&cache.validation, &file, source, false);
        let parsed = cache.parse(dir.path(), "a.rs");
        let parsed = parsed.as_ref().as_ref().unwrap();
        let line = source.lines().nth(1).unwrap();
        assert_eq!(references.len(), 2);
        for (reference, (marker, selector)) in
            references.iter().zip([("{impl}", "Café"), (":::", "Two")])
        {
            assert_eq!(
                (reference.line, reference.column),
                (2, line.find(marker).unwrap() + 1)
            );
            let owner = parsed.reference_owner(reference.line, reference.column);
            assert_eq!(
                crate::outline_items::display_name(&parsed.outline.nodes[owner].selector)
                    .as_deref(),
                Some(selector)
            );
            assert!(reference.closed);
        }
    }

    // {test}`requirements.ci.outline_inputs`
    #[test]
    fn an_empty_file_root_covers_line_one() {
        let dir = tempfile::tempdir().unwrap();
        let file = "empty.yaml";
        fs::write(dir.path().join(file), "").unwrap();
        let parsed = OutlineCache::default().parse(dir.path(), file);
        let parsed = parsed.as_ref().as_ref().unwrap();
        let root = parsed.resolve(file, "").unwrap();
        let (range, lines) = parsed.node_range(file, root);
        assert_eq!((range.line_start, range.line_end), (1, 1));
        assert!(lines.is_empty());
    }

    // {test}`requirements.ci.outline_inputs`
    // {test}`requirements.references.outline_comments`
    #[test]
    fn yaml_evidence_ranges_follow_comment_ownership_between_sibling_entries() {
        let dir = tempfile::tempdir().unwrap();
        let file = "jobs.yml";
        let source =
            "jobs:\n  first:\n    run: a # inline\n  # for second\n  second: b\n# file note\n";
        fs::write(dir.path().join(file), source).unwrap();
        let parsed = OutlineCache::default().parse(dir.path(), file);
        let parsed = parsed.as_ref().as_ref().unwrap();
        let first = parsed.resolve(file, "pair[jobs]/pair[first]").unwrap();
        let second = parsed.resolve(file, "pair[jobs]/pair[second]").unwrap();
        let (range, lines) = parsed.node_range(file, first);
        assert_eq!((range.line_start, range.line_end), (2, 3));
        assert_eq!(lines, ["  first:", "    run: a # inline"]);
        let (range, lines) = parsed.node_range(file, second);
        assert_eq!((range.line_start, range.line_end), (4, 5));
        assert_eq!(lines, ["  # for second", "  second: b"]);
        assert_eq!(parsed.nodes_for_range(4, 4, false), [second]);
        assert_eq!(parsed.reference_owner(4, 3), second);
        assert_eq!(parsed.reference_owner(6, 1), 0);
    }

    // {test}`requirements.references.outline_comments`
    // {test}`requirements.cli.fixme_issues`
    #[test]
    fn yaml_sequence_entry_reports_its_dash_as_the_declaration_line() {
        let dir = tempfile::tempdir().unwrap();
        let file = "steps.yaml";
        fs::write(
            dir.path().join(file),
            "steps:\n  # first note\n  # second note\n  - build\n",
        )
        .unwrap();
        let parsed = OutlineCache::default().parse(dir.path(), file);
        let parsed = parsed.as_ref().as_ref().unwrap();
        let owner = parsed.reference_owner(2, 3);
        let item = &parsed.outline.nodes[owner];
        assert_eq!(item.kind, "item");
        assert_eq!(
            (item.line_start, item.declaration_line, item.line_end),
            (2, 4, 4)
        );
    }

    // {test}`requirements.authoring.credentials`
    // {test}`requirements.references.structural_hash`
    #[test]
    fn configuration_source_uses_sdk_redaction_for_text_and_hashes() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("fixtures")).unwrap();
        let observations = |value: &str| {
            let source = format!("[authoring.configs.local]\napi_key = '{value}'\n");
            fs::write(dir.path().join("fixtures/habeas.toml"), &source).unwrap();
            let parsed = OutlineCache::default().parse(dir.path(), "fixtures/habeas.toml");
            let parsed = parsed.as_ref().as_ref().unwrap();
            assert_eq!(parsed.redacted, crate::redaction::redact_text(&source));
            parsed
                .outline
                .nodes
                .iter()
                .map(|node| (node.selector.clone(), node.structural_hash.clone()))
                .collect::<Vec<_>>()
        };
        assert_ne!(observations("first"), observations("a longer value"));
        let token = crate::redaction::tests::github_token();
        assert_eq!(
            observations(&token),
            observations(&token.replace("16C7", "43D9"))
        );
    }

    // {test}`requirements.authoring.outline_tool`
    #[test]
    fn clones_share_sources_loaded_after_cloning_but_new_runs_read_changes() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("a.rs");
        fs::write(&file, "fn original() {}\n").unwrap();
        let outlines = OutlineCache::default();
        let clone = outlines.clone();
        let snapshot =
            trust_but_verify::outline::OutlineContent::from_path(&outlines.validation, &file)
                .unwrap();
        assert_eq!(snapshot.text(), "fn original() {}\n");
        fs::write(&file, "fn changed() {}\n").unwrap();
        let original = clone.parse(temp.path(), "a.rs");
        assert_eq!(
            original.as_ref().as_ref().unwrap().source,
            "fn original() {}\n"
        );

        let cached = outlines.parse(temp.path(), "a.rs");
        assert_eq!(
            cached.as_ref().as_ref().unwrap().source,
            "fn original() {}\n"
        );
        assert!(Arc::ptr_eq(&original, &cached));

        let fresh = OutlineCache::default().parse(temp.path(), "a.rs");
        assert_eq!(fresh.as_ref().as_ref().unwrap().source, "fn changed() {}\n");
    }

    // {test}`requirements.authoring.outline_tool`
    #[test]
    fn clones_share_overrides_and_failed_reads() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("a.rs"), "fn disk() {}\n").unwrap();
        let outlines = OutlineCache::with_overrides(HashMap::from([(
            "a.rs".to_owned(),
            "fn supplied() {}\n".to_owned(),
        )]));
        let clone = outlines.clone();
        let supplied = clone.parse(temp.path(), "a.rs");
        assert_eq!(
            supplied.as_ref().as_ref().unwrap().source,
            "fn supplied() {}\n"
        );
        assert!(Arc::ptr_eq(&supplied, &outlines.parse(temp.path(), "a.rs")));

        let missing = clone.parse(temp.path(), "missing.rs");
        assert!(missing.is_err());
        fs::write(temp.path().join("missing.rs"), "fn created() {}\n").unwrap();
        assert!(Arc::ptr_eq(
            &missing,
            &outlines.parse(temp.path(), "missing.rs")
        ));
        assert!(
            OutlineCache::default()
                .parse(temp.path(), "missing.rs")
                .is_ok()
        );
    }
}

impl ParsedSource {
    /// Habeas attaches Markdown references to the nearest directive, or to
    /// their paragraph outside directives. SDK line selection still exposes
    /// the innermost paragraph.
    // {impl}`requirements.references.outline_comments`
    pub(crate) fn reference_owner(&self, line: usize, column: usize) -> usize {
        let owner = self.outline.ownership.owner_at(line, column);
        if self.outline.language != "markdown" {
            return owner;
        }
        let mut current = Some(owner);
        while let Some(index) = current {
            let node = &self.outline.nodes[index];
            if node.kind == "block"
                && self.source[node.byte_start..node.byte_end]
                    .trim_start()
                    .starts_with(":::")
            {
                return index;
            }
            current = node.parent;
        }
        owner
    }

    /// Resolves a selector to exactly one node.
    // {impl}`requirements.references.outline_selectors`
    pub(crate) fn resolve(&self, file: &str, item: &str) -> Result<usize, String> {
        self.outline
            .resolve(item)
            .map_err(|error| format!("{file}: {error}"))
    }

    /// The innermost nodes covering the requested lines of an inclusive
    /// range. Blank lines between nodes would otherwise select their common
    /// ancestor, so they are skipped unless every requested line is blank.
    /// With `comments`, comment lines select the comments' owners.
    // {impl}`requirements.references.outline_comments`
    pub(crate) fn nodes_for_range(
        &self,
        line_start: usize,
        line_end: usize,
        comments: bool,
    ) -> Vec<usize> {
        let lines = self
            .source
            .lines()
            .enumerate()
            .map(|(index, line)| (index + 1, line))
            .skip(line_start.saturating_sub(1))
            .take((line_end + 1).saturating_sub(line_start))
            .collect::<Vec<_>>();
        let nonblank = lines
            .iter()
            .filter(|(_, line)| !line.trim().is_empty())
            .map(|(number, _)| *number)
            .collect::<Vec<_>>();
        if nonblank.is_empty() {
            self.outline
                .nodes_for_lines(lines.iter().map(|(number, _)| *number), comments)
        } else {
            self.outline.nodes_for_lines(nonblank, comments)
        }
    }

    // {impl}`requirements.ci.outline_inputs`
    /// A node's current inclusive line range and redacted source text.
    pub(crate) fn node_range(&self, file: &str, index: usize) -> (SourceRange, Vec<String>) {
        let node = &self.outline.nodes[index];
        (
            SourceRange {
                file: file.to_owned(),
                line_start: node.line_start,
                line_end: node.line_end.max(node.line_start),
                name: None,
            },
            self.redacted
                .lines()
                .skip(node.line_start - 1)
                .take(node.line_end + 1 - node.line_start)
                .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
                .collect(),
        )
    }
}

impl InputSelector {
    // {cite}`habeas:util`
    pub(crate) fn from_input(input: &Value) -> Option<Self> {
        let object = input.as_object()?;
        if let Some(item) = object.get("item").and_then(Value::as_str) {
            Some(Self::Outline {
                file: object.get("file")?.as_str()?.to_owned(),
                item: item.to_owned(),
            })
        } else if let Some(definition) = object.get("definition").and_then(Value::as_str) {
            Some(Self::Definition {
                definition: definition.to_owned(),
            })
        } else {
            Some(Self::Requirements {
                requirements: object.get("requirements")?.as_str()?.to_owned(),
            })
        }
    }

    /// The recorded evidence input: selectors and hashes, never outline
    /// source text or line numbers.
    // {impl}`requirements.definitions.inputs`
    pub(crate) fn to_input(&self, resolved: &ResolvedInput) -> Value {
        match self {
            Self::Outline { file, item } => json!({
                "file": file,
                "item": item,
                "hash": resolved.node.as_ref().map(|node| node.hash.as_str()),
            }),
            Self::Definition { definition } => {
                json!({ "definition": definition, "text": resolved.text })
            }
            Self::Requirements { requirements } => json!({
                "requirements": requirements,
                "matches": resolved.matches,
                "text": resolved.text,
            }),
        }
    }

    /// The input as shown to the model: the recorded input plus, for an
    /// outline input, the node's kind and source text, without line numbers.
    // {impl}`requirements.authoring.mandatory_inputs`
    // {impl}`requirements.authoring.no_line_numbers`
    pub(crate) fn to_model_input(&self, resolved: &ResolvedInput) -> Value {
        let mut input = self.to_input(resolved);
        if let (Self::Outline { .. }, Some(node), Some(object)) =
            (self, &resolved.node, input.as_object_mut())
        {
            object.insert("kind".to_owned(), json!(node.kind));
            object.insert("text".to_owned(), json!(resolved.text));
        }
        input
    }

    pub(crate) fn resolve(
        &self,
        root: &Path,
        definitions: &BTreeMap<String, Declaration>,
        outlines: &OutlineCache,
    ) -> Result<ResolvedInput, String> {
        match self {
            Self::Outline { file, item } => {
                let parsed = outlines.parse(root, file);
                let parsed = parsed.as_ref().as_ref().map_err(Clone::clone)?;
                let index = parsed.resolve(file, item)?;
                let (range, text) = parsed.node_range(file, index);
                let node = &parsed.outline.nodes[index];
                Ok(ResolvedInput {
                    ranges: vec![range],
                    text,
                    matches: Vec::new(),
                    node: Some(ResolvedNode {
                        path: parsed.path.clone(),
                        index,
                        kind: if index == 0 {
                            "file".to_owned()
                        } else {
                            node.kind.clone()
                        },
                        hash: node.structural_hash.clone(),
                    }),
                })
            }
            Self::Definition { definition } => {
                let target = definitions
                    .get(definition)
                    .filter(|d| d.kind == crate::DefinitionKind::Term)
                    .ok_or_else(|| format!("{definition}: no matching named definition"))?;
                outlines.check_project_file(root, &target.file)?;
                Ok(ResolvedInput {
                    ranges: vec![SourceRange {
                        file: target.file.clone(),
                        line_start: target.line_start,
                        line_end: target.line_end,
                        name: Some(definition.clone()),
                    }],
                    text: crate::redaction::redact_lines(&target.raw_lines),
                    matches: Vec::new(),
                    node: None,
                })
            }
            Self::Requirements { requirements } => {
                definition_glob::validate(requirements)
                    .map_err(|error| format!("requirements glob {requirements:?}: {error}"))?;
                let mut ranges = Vec::new();
                let mut text = Vec::new();
                let mut matches = Vec::new();
                for (name, definition) in definitions {
                    if definition.kind == crate::DefinitionKind::Term
                        || !definition_glob::matches(requirements, name)
                    {
                        continue;
                    }
                    outlines.check_project_file(root, &definition.file)?;
                    if !text.is_empty() {
                        text.push(String::new());
                    }
                    text.extend(crate::redaction::redact_lines(&definition.raw_lines));
                    ranges.push(SourceRange {
                        file: definition.file.clone(),
                        line_start: definition.line_start,
                        line_end: definition.line_end,
                        name: Some(name.clone()),
                    });
                    matches.push(name.clone());
                }
                if matches.is_empty() {
                    return Err(format!(
                        "requirements glob {requirements:?} matches no requirement or example"
                    ));
                }
                Ok(ResolvedInput {
                    ranges,
                    text,
                    matches,
                    node: None,
                })
            }
        }
    }
}

/// The recorded form of a model-facing input: outline inputs keep only
/// `file`, `item`, and `hash`.
// {impl}`requirements.authoring.delivered_inputs`
pub(crate) fn recorded_input(input: &Value) -> Value {
    match input.as_object() {
        Some(object) if object.contains_key("item") => json!({
            "file": object.get("file"),
            "item": object.get("item"),
            "hash": object.get("hash"),
        }),
        _ => input.clone(),
    }
}

// {impl}`requirements.ci.schema`
/// The recorded form of a file path: an absolute path inside the project at
/// the canonical `root` becomes relative to the project for portability. Relative paths, and
/// paths that leave the project, are kept as given.
pub(crate) fn project_relative(root: &Path, file: &str) -> String {
    let path = Path::new(file);
    if !path.is_absolute() {
        return file.to_owned();
    }
    let Some(base) = path
        .ancestors()
        .find(|ancestor| fs::canonicalize(ancestor).is_ok_and(|ancestor| ancestor == root))
    else {
        return file.to_owned();
    };
    let parts = path
        .strip_prefix(base)
        .expect("ancestor is a prefix of its path")
        .components()
        .map(|part| match part {
            std::path::Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>();
    match parts {
        Some(parts) if !parts.is_empty() => parts.join("/"),
        _ => file.to_owned(),
    }
}

/// Resolves the outline node `item` in `file`, relative to the project
/// `root`, and returns the evidence input recording it: `{file, item, hash}`.
/// The empty selector selects the whole file. Intended for tools and tests
/// that construct evidence records.
// {impl}`requirements.ci.schema`
pub fn outline_input(root: &Path, file: &str, item: &str) -> std::io::Result<Value> {
    cached_outline_input(
        &OutlineCache::default(),
        &fs::canonicalize(root)?,
        file,
        item,
    )
}

fn cached_outline_input(
    outlines: &OutlineCache,
    root: &Path,
    file: &str,
    item: &str,
) -> std::io::Result<Value> {
    let selector = InputSelector::Outline {
        file: file.to_owned(),
        item: item.to_owned(),
    };
    let resolved = selector
        .resolve(root, &BTreeMap::new(), outlines)
        .map_err(std::io::Error::other)?;
    Ok(selector.to_input(&resolved))
}

/// Returns the evidence inputs for the innermost outline nodes covering an
/// inclusive, one-based line range of `file`, as the `habeas` authoring tool
/// records them.
// {impl}`requirements.ci.outline_inputs`
// {impl}`requirements.authoring.source_tools`
pub fn outline_inputs_for_lines(
    root: &Path,
    file: &str,
    line_start: usize,
    line_end: usize,
) -> std::io::Result<Vec<Value>> {
    let root = fs::canonicalize(root)?;
    let outlines = OutlineCache::default();
    let parsed = outlines.parse(&root, file);
    let parsed = parsed
        .as_ref()
        .as_ref()
        .map_err(|error| std::io::Error::other(error.clone()))?;
    parsed
        .nodes_for_range(line_start, line_end, false)
        .into_iter()
        .map(|index| {
            cached_outline_input(
                &outlines,
                &root,
                file,
                &parsed.outline.nodes[index].selector,
            )
        })
        .collect()
}

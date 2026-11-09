use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use mdbook_preprocessor::book::{Book, BookItem, Chapter, SectionNumber};
use mdbook_preprocessor::config::Config;
use serde_json::Value;

use crate::evidence::{Declaration, SourceReference, inspect_repository};
use crate::inputs::{InputSelector, OutlineCache, ResolvedInput, SourceRange};
use crate::markdown::render_evidence_table;
use crate::{
    Definition, DefinitionKind, EvidenceReport, ReferenceKind, chapter_href, colon_fence,
    definition_blocks, escape_html, overlaps_protected, protected_ranges, shield_badge,
};

/// Where generated evidence detail pages and the optional index go.
// {impl}`requirements.index.detail_pages`
// {impl}`requirements.index.chapter`
pub(crate) struct DetailPlan {
    index_path: Option<PathBuf>,
    detail_paths: BTreeMap<String, PathBuf>,
    /// Defining chapter paths, each with its definitions in definition order.
    chapters: Vec<(PathBuf, Vec<String>)>,
}

// {impl}`requirements.index.source_links`
pub(crate) struct SourceLinks {
    blob_prefix: String,
}

// {impl}`requirements.index.detail_format`
pub(crate) struct RenderSources<'a> {
    pub(crate) root: &'a Path,
    pub(crate) definitions: &'a BTreeMap<String, Declaration>,
    /// Current `{impl}`, `{cite}`, and `{test}` references in project sources.
    pub(crate) references: &'a [SourceReference],
    pub(crate) source_links: Option<&'a SourceLinks>,
    /// Parsed outlines, cached while rendering.
    pub(crate) outlines: &'a OutlineCache,
}

impl SourceLinks {
    // {impl}`requirements.index.source_links`
    fn from_config(config: &Config) -> io::Result<Option<Self>> {
        let repository = config
            .get::<String>("output.html.git-repository-url")
            .map_err(io::Error::other)?;
        let edit_template = config
            .get::<String>("output.html.edit-url-template")
            .map_err(io::Error::other)?;
        let (Some(repository), Some(edit_template)) = (repository, edit_template) else {
            return Ok(None);
        };
        let repository = repository.trim_end_matches('/');
        let Some(repository_path) = repository.strip_prefix("https://github.com/") else {
            return Ok(None);
        };
        let mut parts = repository_path.split('/');
        let (Some(owner), Some(project), None) = (parts.next(), parts.next(), parts.next()) else {
            return Ok(None);
        };
        if owner.is_empty()
            || project.is_empty()
            || !owner
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || !project
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        {
            return Ok(None);
        }
        let Some(branch) = edit_template
            .strip_prefix(&format!("{repository}/edit/"))
            .and_then(|suffix| suffix.strip_suffix("/{path}"))
            .filter(|branch| !branch.is_empty())
        else {
            return Ok(None);
        };
        Ok(Some(Self {
            blob_prefix: format!("{repository}/blob/{branch}"),
        }))
    }

    // {impl}`requirements.index.source_links`
    fn input_url(&self, file: &str, start: u64, end: u64) -> Option<String> {
        if start == 0 || end < start {
            return None;
        }
        let mut segments = Vec::new();
        for component in Path::new(file).components() {
            match component {
                Component::Normal(segment) => segments.push(encode_url_segment(segment.to_str()?)),
                Component::CurDir => {}
                _ => return None,
            }
        }
        if segments.is_empty() {
            return None;
        }
        Some(format!(
            "{}/{}#L{start}-L{end}",
            self.blob_prefix,
            segments.join("/")
        ))
    }
}

// {impl}`requirements.index.source_links`
fn encode_url_segment(segment: &str) -> String {
    let mut encoded = String::new();
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(char::from(byte))
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

// {impl}`requirements.index.state`
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum EvidenceState {
    Valid,
    Missing,
    DeterminationFalse,
    Fixme,
    Invalid,
}

// {impl}`requirements.index.detail_format`
pub(crate) struct LoadedDetailEvidence {
    state: EvidenceState,
    record: Option<String>,
    errors: Vec<String>,
    fixme_entries: Vec<String>,
}

// {impl}`requirements.index.detail_format`
struct DetailEvidence<'a> {
    state: EvidenceState,
    record: Option<&'a str>,
    errors: &'a [String],
    fixme_entries: &'a [String],
}

impl LoadedDetailEvidence {
    // {impl}`requirements.index.state`
    // {impl}`requirements.ci.outcomes`
    // {impl}`requirements.cli.fixme_summary`
    pub(crate) fn load(root: &Path, name: &str, report: &EvidenceReport) -> Self {
        Self::load_path(root, &format!("evidence/{name}.json"), report)
    }

    /// Loads the record at a project-relative path, with the validation
    /// errors reported for it, such as a citation review.
    // {impl}`requirements.index.state`
    // {impl}`requirements.references.citation_review`
    // {impl}`requirements.ci.outcomes`
    pub(crate) fn load_path(root: &Path, relative: &str, report: &EvidenceReport) -> Self {
        let evidence_path = root.join(relative);
        let prefix = format!("{relative}:");
        let mut errors = report
            .errors
            .iter()
            .filter(|error| error.starts_with(&prefix))
            .cloned()
            .collect::<Vec<_>>();
        // {impl}`requirements.index.invalid_detail`
        let record = match fs::read_to_string(&evidence_path) {
            Ok(record) => Some(record),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                // Validation usually reported the same failure already.
                let failure = format!("{prefix} cannot read evidence:");
                if !errors.iter().any(|reported| reported.starts_with(&failure)) {
                    errors.push(format!("{failure} {error}"));
                }
                None
            }
        };
        let judgment = record
            .as_deref()
            .and_then(|record| serde_json::from_str::<Value>(record).ok())
            .and_then(|record| {
                record
                    .get("determination")
                    .and_then(|value| crate::judgment::Judgment::from_value(value).ok())
            });
        let fixme_entries = judgment
            .as_ref()
            .map(|judgment| judgment.fixme_entries().to_vec())
            .unwrap_or_default();
        let state = if record.is_none() && !evidence_path.exists() {
            EvidenceState::Missing
        } else if errors.is_empty() {
            match judgment {
                Some(crate::judgment::Judgment::True) => EvidenceState::Valid,
                Some(crate::judgment::Judgment::False) => EvidenceState::DeterminationFalse,
                Some(crate::judgment::Judgment::Fixme(_)) => EvidenceState::Fixme,
                None => EvidenceState::Invalid,
            }
        } else if errors
            .iter()
            .all(|error| error.starts_with(&format!("{prefix} determination is false")))
        {
            EvidenceState::DeterminationFalse
        } else {
            EvidenceState::Invalid
        };
        Self {
            state,
            record,
            errors,
            fixme_entries,
        }
    }

    // {impl}`requirements.index.state`
    pub(crate) fn state(&self) -> EvidenceState {
        self.state
    }

    // {impl}`requirements.index.state`
    pub(crate) fn errors(&self) -> &[String] {
        &self.errors
    }

    /// The recorded determination's kind, when the record has a valid
    /// determination, and whether its rationale states that its assessments
    /// disagreed under the `confirm` policy.
    // {impl}`requirements.cli.status`
    pub(crate) fn disagreement(&self) -> Option<&'static str> {
        let record = serde_json::from_str::<Value>(self.record.as_deref()?).ok()?;
        let rationale = record.get("rationale")?.as_str()?;
        if !rationale.contains(crate::authoring::DISAGREEMENT_NOTE) {
            return None;
        }
        let judgment = crate::judgment::Judgment::from_value(record.get("determination")?).ok()?;
        Some(judgment.kind())
    }

    // {impl}`requirements.cli.status`
    // {impl}`requirements.index.detail_format`
    // {impl}`requirements.cli.fixme_summary`
    pub(crate) fn fixme_entries(&self) -> &[String] {
        &self.fixme_entries
    }

    // {impl}`requirements.index.detail_format`
    // {impl}`requirements.cli.info`
    fn borrowed(&self) -> DetailEvidence<'_> {
        DetailEvidence {
            state: self.state,
            record: self.record.as_deref(),
            errors: &self.errors,
            fixme_entries: &self.fixme_entries,
        }
    }
}

impl EvidenceState {
    // {impl}`requirements.index.state`
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Valid => "Valid",
            Self::Missing => "Missing",
            Self::DeterminationFalse => "Determination false",
            Self::Fixme => "FIXME",
            Self::Invalid => "Invalid",
        }
    }

    // {impl}`requirements.index.state`
    fn color(self) -> &'static str {
        match self {
            Self::Valid => "237346",
            Self::Missing => "5f6670",
            Self::DeterminationFalse => "ae5d14",
            Self::Fixme => "ae5d14",
            Self::Invalid => "ad3028",
        }
    }
}

impl DetailPlan {
    // {impl}`requirements.index.chapter`
    // {impl}`requirements.examples.index`
    // {impl}`requirements.index.detail_pages`
    // {impl}`requirements.index.path_conflicts`
    pub(crate) fn find(
        book: &Book,
        definitions: &BTreeMap<String, Definition>,
        order: &[String],
    ) -> io::Result<Self> {
        let mut index_path = None;
        for chapter in book.chapters() {
            for _ in marker_ranges(&chapter.content)? {
                if index_path.is_some() {
                    return Err(invalid_index(
                        "a book may contain only one {requirements-index} directive",
                    ));
                }
                index_path = chapter.path.clone();
            }
        }
        let mut detail_paths = BTreeMap::new();
        let mut chapters: Vec<(PathBuf, Vec<String>)> = Vec::new();
        for name in order {
            let Some(definition) = definitions.get(name) else {
                continue;
            };
            let path = detail_path(&definition.path, name);
            detail_paths.insert(name.clone(), path);
            match chapters
                .iter_mut()
                .find(|(chapter, _)| *chapter == definition.path)
            {
                Some((_, names)) => names.push(name.clone()),
                None => chapters.push((definition.path.clone(), vec![name.clone()])),
            }
        }
        let existing_paths = book
            .chapters()
            .filter_map(|chapter| chapter.path.as_ref())
            .collect::<BTreeSet<_>>();
        // {impl}`requirements.index.path_conflicts`
        let mut generated = BTreeSet::new();
        for path in detail_paths.values() {
            if existing_paths.contains(path) || !generated.insert(path) {
                return Err(invalid_index(&format!(
                    "generated evidence page {} conflicts with a book chapter",
                    path.display()
                )));
            }
        }
        Ok(Self {
            index_path,
            detail_paths,
            chapters,
        })
    }

    // {impl}`requirements.index.navigation`
    pub(crate) fn detail_path(&self, name: &str) -> Option<&Path> {
        self.detail_paths.get(name).map(PathBuf::as_path)
    }

    // {impl}`requirements.index.navigation`
    pub(crate) fn is_detail_path(&self, path: &Path) -> bool {
        self.detail_paths.values().any(|detail| detail == path)
    }

    // {impl}`requirements.index.state`
    // {impl}`requirements.examples.index`
    // {impl}`requirements.index.chapter`
    // {impl}`requirements.index.detail_pages`
    pub(crate) fn populate(
        &self,
        book: &mut Book,
        root: &Path,
        definitions: &BTreeMap<String, Definition>,
    ) -> io::Result<()> {
        if self.detail_paths.is_empty() && self.index_path.is_none() {
            return Ok(());
        }
        let config = Config::from_disk(root.join("book.toml")).map_err(io::Error::other)?;
        let source_links = SourceLinks::from_config(&config)?;
        let inspection = inspect_repository(root)?;
        let report = &inspection.report;
        let outlines = OutlineCache::default();
        let sources = RenderSources {
            root,
            definitions: &inspection.definitions,
            references: &inspection.references,
            source_links: source_links.as_ref(),
            outlines: &outlines,
        };
        let mut rows = Vec::new();
        let mut pages = BTreeMap::new();
        for (name, detail_path) in &self.detail_paths {
            let evidence_path = root.join("evidence").join(format!("{name}.json"));
            let evidence = LoadedDetailEvidence::load(root, name, report);
            let definition = definitions
                .get(name)
                .expect("every detail page has a definition");
            if let Some(index_path) = &self.index_path {
                let detail_href = chapter_href(index_path, detail_path, None);
                rows.push((
                    format!("[`{name}`]({detail_href}) ({})", definition.kind.noun()),
                    evidence.state().label(),
                ));
            }
            let content = render_detail_page(
                name,
                detail_path,
                self.index_path.as_deref(),
                definition,
                evidence.borrowed(),
                &sources,
            );
            // {impl}`requirements.index.source_links`
            let source_path = if evidence_path.exists() {
                evidence_edit_path(&config.book.src, name)
            } else {
                definition.path.clone()
            };
            pages.insert(name.clone(), (detail_path.clone(), content, source_path));
        }

        // {impl}`requirements.cli.shared_status_rendering`
        let rows = render_evidence_table(&rows);

        // {impl}`requirements.index.navigation`
        let mut index_found = self.index_path.is_none();
        let mut chapters_found = 0;
        book.for_each_chapter_mut(|chapter| {
            let Some(path) = chapter.path.clone() else {
                return;
            };
            if self.index_path.as_deref() == Some(path.as_path()) {
                index_found = true;
                let ranges = marker_ranges(&chapter.content)
                    .expect("the index directive was checked before the book was changed");
                let range = ranges
                    .into_iter()
                    .next()
                    .expect("the planned index directive is still present");
                chapter.content.replace_range(range, &rows);
            }
            let Some((_, names)) = self.chapters.iter().find(|(chapter, _)| *chapter == path)
            else {
                return;
            };
            chapters_found += 1;
            let mut parent_names = chapter.parent_names.clone();
            parent_names.push(chapter.name.clone());
            let first_number = chapter
                .sub_items
                .iter()
                .filter_map(|item| match item {
                    BookItem::Chapter(child) => child.number.as_ref()?.last().copied(),
                    _ => None,
                })
                .max()
                .unwrap_or(0)
                + 1;
            for (offset, name) in names.iter().enumerate() {
                let (path, content, source_path) = &pages[name];
                let mut page = Chapter::new(name, content.clone(), path, parent_names.clone());
                page.source_path = Some(source_path.clone());
                page.number = chapter.number.as_ref().map(|number| {
                    let mut number = number.to_vec();
                    number.push(first_number + offset as u32);
                    SectionNumber::new(number)
                });
                chapter.sub_items.push(BookItem::Chapter(page));
            }
        });
        if !index_found {
            return Err(invalid_index("the requirements index chapter disappeared"));
        }
        if chapters_found != self.chapters.len() {
            return Err(invalid_index("a defining chapter disappeared"));
        }
        Ok(())
    }
}

/// The generated page for `name`, in a directory named after its chapter.
// {impl}`requirements.index.detail_pages`
fn detail_path(chapter: &Path, name: &str) -> PathBuf {
    chapter.with_extension("").join(format!("{name}.md"))
}

// {impl}`requirements.index.source_links`
fn evidence_edit_path(book_src: &Path, name: &str) -> PathBuf {
    let mut path = PathBuf::new();
    for component in book_src.components() {
        if matches!(component, Component::Normal(_)) {
            path.push("..");
        }
    }
    path.push("evidence");
    path.push(format!("{name}.json"));
    path
}

// {impl}`requirements.index.detail_format`
// {impl}`requirements.index.invalid_detail`
// {impl}`requirements.examples.index`
// {impl}`requirements.index.navigation`
fn render_detail_page(
    name: &str,
    detail_path: &Path,
    index_path: Option<&Path>,
    definition: &Definition,
    detail_evidence: DetailEvidence<'_>,
    sources: &RenderSources<'_>,
) -> String {
    let definition_href = chapter_href(
        detail_path,
        &definition.path,
        Some(&definition.kind.anchor(name)),
    );
    let mut header = format!("# `{name}`\n\n");
    if let Some(index_path) = index_path {
        let index_href = chapter_href(detail_path, index_path, None);
        header.push_str(&format!("[Requirements index]({index_href}) · "));
    }
    header.push_str(&format!("[View definition]({definition_href})\n\n"));
    render_detail_sections(
        header,
        name,
        &definition.text,
        definition.kind,
        detail_evidence,
        sources,
    )
}

// {impl}`requirements.cli.info`
pub(crate) fn render_cli_detail_page(
    name: &str,
    source: &str,
    text: &str,
    kind: DefinitionKind,
    evidence: &LoadedDetailEvidence,
    sources: &RenderSources<'_>,
) -> String {
    render_detail_sections(
        format!("# `{name}`\n\nDefinition: `{source}`\n\n"),
        name,
        text,
        kind,
        evidence.borrowed(),
        sources,
    )
}

/// The groups detail pages sort inputs into, in display order.
// {impl}`requirements.index.detail_format`
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum InputGroup {
    Implementation,
    Tests,
    ReferencedDefinitions,
    OtherSources,
}

impl InputGroup {
    const ALL: [Self; 4] = [
        Self::Implementation,
        Self::Tests,
        Self::ReferencedDefinitions,
        Self::OtherSources,
    ];

    // {impl}`requirements.index.detail_format`
    fn heading(self) -> &'static str {
        match self {
            Self::Implementation => "Implementation",
            Self::Tests => "Tests",
            Self::ReferencedDefinitions => "Referenced definitions",
            Self::OtherSources => "Other sources",
        }
    }

    // {impl}`requirements.index.detail_format`
    fn id(self) -> &'static str {
        match self {
            Self::Implementation => "habeas-inputs-implementation",
            Self::Tests => "habeas-inputs-tests",
            Self::ReferencedDefinitions => "habeas-inputs-referenced-definitions",
            Self::OtherSources => "habeas-inputs-other-sources",
        }
    }
}

/// Current edges from one definition, used to group its evidence inputs.
// {impl}`requirements.index.detail_format`
#[derive(Default)]
struct DefinitionEdges {
    /// `{impl}` and `{test}` reference sites as canonical file and line.
    sites: Vec<(ReferenceKind, PathBuf, usize)>,
    /// Anchored nodes as canonical paths and current line ranges.
    anchors: Vec<(ReferenceKind, PathBuf, usize, usize)>,
    /// Definitions the subject cites with `{cite}`.
    cited: BTreeSet<String>,
}

impl DefinitionEdges {
    // {impl}`requirements.index.detail_format`
    // {impl}`requirements.references.anchors`
    fn current(name: &str, sources: &RenderSources<'_>) -> Self {
        let mut edges = Self::default();
        for reference in sources.references {
            if reference.name == name
                && matches!(reference.kind, ReferenceKind::Impl | ReferenceKind::Test)
            {
                let file = fs::canonicalize(&reference.file).unwrap_or(reference.file.clone());
                edges.sites.push((reference.kind, file, reference.line));
            }
        }
        if let Some(declaration) = sources.definitions.get(name) {
            for anchor in &declaration.anchors {
                if let Ok(target) = anchor.resolve(sources.root, sources.outlines) {
                    edges.anchors.push((
                        anchor.kind,
                        target.path,
                        target.line_start,
                        target.line_end,
                    ));
                }
            }
            edges.cited = crate::text_references(&declaration.text, true)
                .into_iter()
                .filter(|(kind, _)| *kind == ReferenceKind::Cite)
                .map(|(_, name)| name)
                .collect();
        }
        edges
    }

    // {impl}`requirements.index.detail_format`
    fn group(
        &self,
        root: &Path,
        selector: Option<&InputSelector>,
        ranges: &[SourceRange],
    ) -> InputGroup {
        let all_cited = !ranges.is_empty()
            && ranges.iter().all(|range| {
                range
                    .name
                    .as_ref()
                    .is_some_and(|name| self.cited.contains(name))
            });
        let ranges = ranges
            .iter()
            .map(|range| {
                let file = root.join(&range.file);
                (
                    fs::canonicalize(&file).unwrap_or(file),
                    range.line_start,
                    range.line_end,
                )
            })
            .collect::<Vec<_>>();
        for (group, kind) in [
            (InputGroup::Implementation, ReferenceKind::Impl),
            (InputGroup::Tests, ReferenceKind::Test),
        ] {
            let site = self.sites.iter().any(|(site_kind, file, line)| {
                *site_kind == kind
                    && ranges
                        .iter()
                        .any(|(path, start, end)| path == file && start <= line && line <= end)
            });
            let anchored =
                self.anchors
                    .iter()
                    .any(|(anchor_kind, file, anchor_start, anchor_end)| {
                        *anchor_kind == kind
                            && ranges.iter().any(|(path, start, end)| {
                                path == file && start <= anchor_end && anchor_start <= end
                            })
                    });
            if site || anchored {
                return group;
            }
        }
        match selector {
            Some(InputSelector::Definition { .. }) => InputGroup::ReferencedDefinitions,
            Some(InputSelector::Requirements { .. }) if all_cited => {
                InputGroup::ReferencedDefinitions
            }
            _ => InputGroup::OtherSources,
        }
    }
}

// {impl}`requirements.index.detail_format`
// {impl}`requirements.index.invalid_detail`
// {impl}`requirements.cli.info`
// {impl}`requirements.index.state`
// {impl}`requirements.examples.index`
// {impl}`requirements.authoring.redaction`
fn render_detail_sections(
    mut content: String,
    name: &str,
    definition_text: &str,
    kind: DefinitionKind,
    detail_evidence: DetailEvidence<'_>,
    sources: &RenderSources<'_>,
) -> String {
    content.push_str(&shield_badge(
        "evidence",
        detail_evidence.state.label(),
        detail_evidence.state.color(),
    ));
    content.push_str(&format!(
        "\n\n## {}\n\n",
        if kind == DefinitionKind::Example {
            "Example"
        } else {
            "Requirement"
        }
    ));
    for line in crate::redaction::redact_text(definition_text).lines() {
        content.push_str("> ");
        content.push_str(line);
        content.push('\n');
    }

    if !detail_evidence.errors.is_empty() {
        content.push_str("\n## Validation findings\n\n<pre><code>");
        content.push_str(&escape_html(&crate::redaction::redact_text(
            &detail_evidence.errors.join("\n\n"),
        )));
        content.push_str("</code></pre>\n");
    }

    let parsed = detail_evidence
        .record
        .and_then(|record| serde_json::from_str::<Value>(record).ok());
    let evidence = parsed.as_ref().and_then(Value::as_object);
    content.push_str("\n## Rationale\n\n");
    match evidence
        .and_then(|evidence| evidence.get("rationale"))
        .and_then(Value::as_str)
    {
        Some(rationale) if !rationale.trim().is_empty() => content.push_str(rationale.trim()),
        _ => content.push_str("No rationale recorded."),
    }
    if !detail_evidence.fixme_entries.is_empty() {
        content.push_str("\n\n## FIXME references\n\n<pre><code>");
        content.push_str(&escape_html(&detail_evidence.fixme_entries.join("\n")));
        content.push_str("</code></pre>\n");
    }
    content.push_str("\n\n## Inputs\n\n");
    match evidence
        .and_then(|evidence| evidence.get("inputs"))
        .and_then(Value::as_array)
    {
        Some(inputs) if !inputs.is_empty() => {
            let edges = DefinitionEdges::current(name, sources);
            let mut grouped: BTreeMap<InputGroup, Vec<(usize, &Value, InputLocation)>> =
                BTreeMap::new();
            for (index, input) in inputs.iter().enumerate() {
                let location = InputLocation::resolve(input, sources);
                let group = edges.group(
                    sources.root,
                    location.selector.as_ref(),
                    match &location.ranges {
                        Some(Ok(ranges)) => ranges,
                        _ => &[],
                    },
                );
                grouped
                    .entry(group)
                    .or_default()
                    .push((index + 1, input, location));
            }
            for group in InputGroup::ALL {
                let Some(inputs) = grouped.get(&group) else {
                    continue;
                };
                content.push_str(&format!(
                    "<details class=\"habeas-input-group\" open>\n<summary><h3 id=\"{}\" style=\"display: inline\">{}</h3></summary>\n\n",
                    group.id(),
                    group.heading()
                ));
                for (index, input, location) in inputs {
                    render_input(&mut content, *index, input, location, sources);
                }
                content.push_str("</details>\n\n");
            }
        }
        _ => content.push_str("No inputs recorded.\n"),
    }
    if evidence.is_none()
        && let Some(record) = detail_evidence.record
    {
        content.push_str("\n## Unreadable evidence record\n\n");
        push_code_block(&mut content, &crate::redaction::redact_text(record), "json");
    }
    content
}

/// An input's selector and what it currently resolves to.
// {impl}`requirements.index.detail_format`
struct InputLocation {
    selector: Option<InputSelector>,
    resolved: Option<Result<ResolvedInput, String>>,
    ranges: Option<Result<Vec<SourceRange>, String>>,
}

impl InputLocation {
    // {impl}`requirements.index.detail_format`
    // {impl}`requirements.definitions.inputs`
    // {impl}`requirements.ci.outline_inputs`
    fn resolve(input: &Value, sources: &RenderSources<'_>) -> Self {
        let selector = InputSelector::from_input(input);
        let resolved = selector
            .as_ref()
            .map(|selector| selector.resolve(sources.root, sources.definitions, sources.outlines));
        let ranges = resolved.as_ref().map(|resolved| match resolved {
            Ok(resolved) => Ok(resolved.ranges.clone()),
            Err(error) => Err(error.clone()),
        });
        Self {
            selector,
            resolved,
            ranges,
        }
    }
}

// {impl}`requirements.index.source_links`
// {impl}`requirements.index.detail_format`
// {impl}`requirements.definitions.inputs`
// {impl}`requirements.ci.outline_inputs`
// {impl}`requirements.authoring.redaction`
fn render_input(
    content: &mut String,
    index: usize,
    input: &Value,
    location: &InputLocation,
    sources: &RenderSources<'_>,
) {
    content.push_str(&format!(
        "<details class=\"habeas-input\">\n<summary id=\"habeas-input-{index}\"><strong>Input {index}</strong>"
    ));
    if let Some(selector) = &location.selector {
        let label = match selector {
            InputSelector::Outline { file, item } if item.is_empty() => {
                format!("outline {file} (whole file)")
            }
            InputSelector::Outline { file, item } => format!("outline {file} {item}"),
            InputSelector::Definition { definition } => format!("definition {definition}"),
            InputSelector::Requirements { requirements } => {
                format!("requirements selection {requirements}")
            }
        };
        content.push_str(": <code>");
        content.push_str(&escape_html(&label));
        content.push_str("</code>");
    }
    content.push_str("</summary>\n\n");
    match &location.resolved {
        Some(Ok(resolved)) => {
            if let (Some(InputSelector::Outline { file, item }), Some(node)) =
                (&location.selector, &resolved.node)
            {
                content.push_str(&format!(
                    "- File: <code>{}</code>\n- Selector: <code>{}</code>\n- Kind: <code>{}</code>\n",
                    escape_html(file),
                    escape_html(if item.is_empty() { "(root)" } else { item }),
                    escape_html(node.kind)
                ));
            }
            for range in &resolved.ranges {
                render_source_location(content, range, sources.source_links);
            }
            content.push('\n');
            if let Some(node) = &resolved.node {
                if input.get("hash").and_then(Value::as_str) != Some(node.hash.as_str()) {
                    content.push_str(
                        "**The recorded hash no longer matches this source; the input is stale.**\n\n",
                    );
                }
                let file = match &location.selector {
                    Some(InputSelector::Outline { file, .. }) => Some(file.as_str()),
                    _ => None,
                };
                push_code_block(content, &resolved.text.join("\n"), code_language(file));
                content.push_str("\n</details>\n\n");
                return;
            }
        }
        Some(Err(error)) => {
            content.push_str(&format!("Unresolved selector: {}\n\n", escape_html(error)));
        }
        None => {}
    }

    if let Some(lines) = input
        .get("text")
        .and_then(Value::as_array)
        .and_then(|lines| lines.iter().map(Value::as_str).collect::<Option<Vec<_>>>())
    {
        let file = match &location.selector {
            Some(InputSelector::Requirements { .. } | InputSelector::Definition { .. }) => {
                Some("requirements.md")
            }
            _ => None,
        };
        let text = crate::redaction::redact_text(&lines.join("\n")).into_owned();
        push_code_block(content, &text, code_language(file));
    } else {
        let raw = serde_json::to_string_pretty(input).expect("JSON values can be serialized");
        push_code_block(content, &crate::redaction::redact_text(&raw), "json");
    }
    content.push_str("\n</details>\n\n");
}

// {impl}`requirements.index.source_links`
// {impl}`requirements.index.detail_format`
fn render_source_location(
    content: &mut String,
    range: &SourceRange,
    source_links: Option<&SourceLinks>,
) {
    let location = format!("{}:{}–{}", range.file, range.line_start, range.line_end);
    let url = source_links.and_then(|links| {
        links.input_url(&range.file, range.line_start as u64, range.line_end as u64)
    });
    content.push_str("- ");
    if let Some(name) = &range.name {
        content.push_str("<code>");
        content.push_str(&escape_html(name));
        content.push_str("</code>: ");
    }
    if let Some(url) = url {
        content.push_str("<a href=\"");
        content.push_str(&escape_html(&url));
        content.push_str("\"><code>");
        content.push_str(&escape_html(&location));
        content.push_str("</code></a>\n");
    } else {
        content.push_str("<code>");
        content.push_str(&escape_html(&location));
        content.push_str("</code>\n");
    }
}

// {impl}`requirements.index.detail_format`
fn push_code_block(content: &mut String, text: &str, language: &str) {
    let longest = text
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    content.push_str(&fence);
    content.push_str(language);
    content.push('\n');
    content.push_str(text);
    content.push('\n');
    content.push_str(&fence);
    content.push('\n');
}

// {impl}`requirements.index.detail_format`
fn code_language(file: Option<&str>) -> &'static str {
    match file
        .and_then(|file| Path::new(file).extension())
        .and_then(|extension| extension.to_str())
    {
        Some("rs") => "rust,noplayground",
        Some("md") => "markdown",
        Some("json") => "json",
        Some("toml") => "ini",
        Some("java") => "java",
        Some("js" | "jsx" | "mjs" | "cjs") => "javascript",
        Some("ts" | "tsx" | "mts" | "cts") => "typescript",
        Some("py") => "python",
        _ => "plaintext",
    }
}

// {impl}`requirements.index.chapter`
fn marker_ranges(content: &str) -> io::Result<Vec<Range<usize>>> {
    let protected = protected_ranges(content);
    let definitions = definition_blocks(content);
    let mut lines = Vec::new();
    let mut position = 0;
    for segment in content.split_inclusive('\n') {
        let end = position + segment.len();
        lines.push((
            position,
            end,
            segment.trim_end_matches('\n').trim_end_matches('\r'),
        ));
        position = end;
    }

    let mut ranges = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let (start, body_start, line) = lines[index];
        let Some((suffix, fence_length)) = colon_fence(line) else {
            index += 1;
            continue;
        };
        if suffix.trim_end() != "{requirements-index}"
            || overlaps_protected(start, body_start, &protected)
            || definitions
                .iter()
                .any(|block| block.start <= start && start < block.end)
        {
            index += 1;
            continue;
        }
        let Some(close) = (index + 1..lines.len()).find(|&candidate| {
            !overlaps_protected(lines[candidate].0, lines[candidate].1, &protected)
                && colon_fence(lines[candidate].2).is_some_and(|(suffix, count)| {
                    count >= fence_length && suffix.trim().is_empty()
                })
        }) else {
            return Err(invalid_index(
                "{requirements-index} is missing a closing colon fence",
            ));
        };
        if !content[body_start..lines[close].0].trim().is_empty() {
            return Err(invalid_index(
                "{requirements-index} must have an empty body",
            ));
        }
        ranges.push(start..lines[close].1);
        index = close + 1;
    }
    Ok(ranges)
}

// {impl}`requirements.index.chapter`
fn invalid_index(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;

    use mdbook_preprocessor::book::{Book, BookItem, Chapter, SectionNumber};
    use mdbook_preprocessor::config::Config;
    use serde_json::json;
    use tempfile::TempDir;

    use super::{
        DetailEvidence, DetailPlan, EvidenceState, RenderSources, SourceLinks, code_language,
        evidence_edit_path, marker_ranges, render_detail_page,
    };
    use crate::{Definition, DefinitionKind, preprocess_book};

    // {test}`requirements.index.chapter`
    #[test]
    fn marker_accepts_long_fences_and_ignores_examples() {
        let content = "```text\n:::{requirements-index}\n:::\n```\n\n::::{req} example\n:::{requirements-index}\n:::\n::::\n\n::::{requirements-index}\n::::\n";
        let ranges = marker_ranges(content).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(
            &content[ranges[0].clone()],
            "::::{requirements-index}\n::::\n"
        );
        assert!(marker_ranges(":::{requirements-index}\n").is_err());
        assert!(marker_ranges(":::{requirements-index}\nUnexpected.\n:::").is_err());
        assert_eq!(
            marker_ranges("::::{requirements-index}\n:::::\n")
                .unwrap()
                .len(),
            1
        );
    }

    // {test}`requirements.index.chapter`
    // {test}`requirements.index.state`
    // {test}`requirements.index.navigation`
    // {test}`requirements.index.source_links`
    // {test}`requirements.cli.shared_status_rendering`
    // {test}`requirements.index.detail_format`
    // {test}`requirements.index.invalid_detail`
    #[test]
    fn index_shows_evidence_states_and_links_generated_chapters() {
        let temp = TempDir::new().unwrap();
        fs::create_dir_all(temp.path().join("md/requirements")).unwrap();
        fs::create_dir_all(temp.path().join("evidence")).unwrap();
        fs::create_dir_all(temp.path().join("src")).unwrap();
        fs::write(temp.path().join("book.toml"), "[book]\nsrc = \"md\"\n").unwrap();
        let requirements = [
            ("ready", "Ready MUST work."),
            ("pending", "Pending MUST work."),
            ("changed", "Changed MUST work."),
            ("absent", "Absent MUST work."),
            ("uncovered", "Uncovered MUST work."),
        ];
        let mut source = String::from("# Requirements\n\n");
        for (name, body) in requirements {
            source.push_str(&format!(":::{{req}} {name}\n{body}\n:::\n\n"));
        }
        fs::write(temp.path().join("md/requirements/core.md"), &source).unwrap();
        fs::write(
            temp.path().join("src/main.rs"),
            ["// {", "impl}`requirements.core.uncovered`\n"].concat(),
        )
        .unwrap();
        for (name, body, determination) in [
            ("ready", "Ready MUST work.", true),
            ("pending", "Pending MUST work.", false),
            ("changed", "Old requirement.", true),
            ("uncovered", "Uncovered MUST work.", true),
        ] {
            fs::write(
                temp.path()
                    .join(format!("evidence/requirements.core.{name}.json")),
                json!({
                    "requirement": body,
                    "determination": determination,
                    "rationale": "The fixture records the evidence state.",
                    "inputs": []
                })
                .to_string(),
            )
            .unwrap();
        }

        let mut book = Book::new();
        let mut definition_chapter =
            Chapter::new("Requirements", source, "requirements/core.md", Vec::new());
        definition_chapter.number = Some(SectionNumber::new(vec![3]));
        book.push_item(definition_chapter);
        let mut index_chapter = Chapter::new(
            "Requirements index",
            "# Requirements index\n\n:::{requirements-index}\n:::\n".to_owned(),
            "appendix/requirements-index.md",
            Vec::new(),
        );
        index_chapter.number = Some(SectionNumber::new(vec![4]));
        book.push_item(index_chapter);
        crate::evidence::tests::stamp_reference_sets(temp.path());
        preprocess_book(&mut book, temp.path()).unwrap();

        let definition = book.chapters().next().unwrap();
        assert!(definition.content.contains(
            "id=\"habeas-req-requirements.core.ready\" href=\"core/requirements.core.ready.html\""
        ));
        let index = book
            .chapters()
            .find(|chapter| chapter.name == "Requirements index")
            .unwrap();
        assert!(!index.content.contains("{requirements-index}"));
        for (name, state) in [
            ("ready", "Valid"),
            ("pending", "Determination false"),
            ("changed", "Invalid"),
            ("absent", "Missing"),
            ("uncovered", "Invalid"),
        ] {
            let link = format!(
                "| [`requirements.core.{name}`](../requirements/core/requirements.core.{name}.html)"
            );
            let row = index
                .content
                .lines()
                .find(|line| line.starts_with(&link))
                .unwrap_or_else(|| panic!("missing {link} in:\n{}", index.content));
            assert_eq!(row.split('|').nth(2).unwrap().trim(), state);
        }
        let table = index
            .content
            .lines()
            .skip_while(|line| !line.starts_with("| "))
            .take_while(|line| line.starts_with("| "))
            .collect::<Vec<_>>();
        let pipes = table[0]
            .match_indices('|')
            .map(|(position, _)| position)
            .collect::<Vec<_>>();
        assert_eq!(pipes.len(), 3);
        assert!(table.iter().all(|line| {
            line.match_indices('|')
                .map(|(position, _)| position)
                .collect::<Vec<_>>()
                == pipes
        }));
        for column in 0..2 {
            let widest = table
                .iter()
                .map(|line| line.split('|').nth(column + 1).unwrap().trim().len())
                .max()
                .unwrap();
            assert_eq!(pipes[column + 1] - pipes[column] - 3, widest);
        }
        assert!(index.sub_items.is_empty());
        assert_eq!(definition.sub_items.len(), 5);
        let ready = definition
            .sub_items
            .iter()
            .find_map(|item| match item {
                BookItem::Chapter(chapter) if chapter.name == "requirements.core.ready" => {
                    Some(chapter)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(
            ready.source_path.as_deref(),
            Some(Path::new("../evidence/requirements.core.ready.json"))
        );
        assert_eq!(
            ready.number.as_deref().map(Vec::as_slice),
            Some(&[3, 1][..])
        );
        assert_eq!(
            ready.path.as_deref(),
            Some(Path::new("requirements/core/requirements.core.ready.md"))
        );
        assert!(
            ready
                .content
                .contains("[View definition](../core.html#habeas-req-requirements.core.ready)")
        );
        assert!(ready.content.contains(
            "[Requirements index](../../appendix/requirements-index.html) · [View definition]"
        ));
        assert!(
            ready
                .content
                .contains("img.shields.io/badge/evidence-Valid-237346")
        );
        assert!(ready.content.contains("> Ready MUST work."));
        assert!(!ready.content.contains("## Evidence record"));
        let changed = definition
            .sub_items
            .iter()
            .find_map(|item| match item {
                BookItem::Chapter(chapter) if chapter.name == "requirements.core.changed" => {
                    Some(chapter)
                }
                _ => None,
            })
            .unwrap();
        assert!(changed.content.contains("requirement text changed"));
        let absent = definition
            .sub_items
            .iter()
            .find_map(|item| match item {
                BookItem::Chapter(chapter) if chapter.name == "requirements.core.absent" => {
                    Some(chapter)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(
            absent.source_path.as_deref(),
            Some(Path::new("requirements/core.md"))
        );
    }

    // {test}`requirements.ci.errors`
    // {test}`requirements.index.invalid_detail`
    #[test]
    fn an_unreadable_record_is_reported_once() {
        let temp = tempfile::tempdir().unwrap();
        // A directory where the record belongs cannot be read as one.
        fs::create_dir_all(temp.path().join("evidence/requirements.ready.json")).unwrap();
        let reading = |reported: Vec<String>| {
            let report = crate::EvidenceReport {
                requirements: 1,
                examples: 0,
                evidence_files: 1,
                errors: reported,
            };
            let loaded =
                super::LoadedDetailEvidence::load(temp.path(), "requirements.ready", &report);
            assert!(loaded.state == EvidenceState::Invalid);
            loaded
                .errors
                .iter()
                .filter(|error| error.contains("cannot read evidence"))
                .count()
        };
        assert_eq!(reading(Vec::new()), 1);
        // Validation already reported it.
        assert_eq!(
            reading(vec![
                "evidence/requirements.ready.json: cannot read evidence: Is a directory".into()
            ]),
            1
        );
    }

    // {test}`requirements.authoring.redaction`
    // {test}`requirements.index.detail_format`
    // {test}`requirements.cli.info`
    #[test]
    fn detail_pages_and_info_display_redacted_source() {
        let token = crate::redaction::tests::github_token();
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("md")).unwrap();
        fs::create_dir_all(root.join("evidence")).unwrap();
        fs::write(root.join("book.toml"), "[book]\nsrc = 'md'\n").unwrap();
        fs::write(
            root.join("md/requirements.md"),
            format!(":::{{req}} ready\nTokens such as {token} MUST be hidden.\n:::\n"),
        )
        .unwrap();
        fs::write(
            root.join("src/main.rs"),
            format!("fn main() {{\n    println!(\"{token}\");\n}}\n"),
        )
        .unwrap();
        fs::write(
            root.join("habeas.toml"),
            "version = 1\n[authoring.configs.local]\nbackend = 'ollama'\nmodel = 'm'\napi_key = 'plain ollama key'\n",
        )
        .unwrap();
        let record = json!({
            "requirement": format!("Tokens such as {token} MUST be hidden."),
            "determination": true,
            "rationale": "Checked.",
            "inputs": [
                crate::inputs::outline_input(root, "src/main.rs", "fn[main]").unwrap(),
                crate::inputs::outline_input(root, "habeas.toml", "").unwrap(),
                {"requirements": "requirements.ready", "matches": ["requirements.ready"], "text": [format!("Tokens such as {token} MUST be hidden.")]},
            ]
        });
        fs::write(
            root.join("evidence/requirements.ready.json"),
            record.to_string(),
        )
        .unwrap();
        let definition = Definition {
            path: Path::new("requirements.md").to_path_buf(),
            text: format!("Tokens such as {token} MUST be hidden."),
            kind: DefinitionKind::Requirement,
        };
        let record = record.to_string();
        let page = render_detail_page(
            "requirements.ready",
            Path::new("appendix/requirements-index/requirements.ready.md"),
            None,
            &definition,
            DetailEvidence {
                state: EvidenceState::Valid,
                record: Some(&record),
                errors: &[],
                fixme_entries: &[],
            },
            &RenderSources {
                root,
                definitions: &BTreeMap::new(),
                references: &[],
                source_links: None,
                outlines: &crate::inputs::OutlineCache::default(),
            },
        );
        let info = crate::commands::info_markdown(root, "requirements.ready").unwrap();
        for page in [page, info] {
            assert!(!page.contains(&token), "{page}");
            assert!(!page.contains("plain ollama key"), "{page}");
            assert!(
                page.contains("Tokens such as [REDACTED:GitHub Token] MUST be hidden."),
                "{page}"
            );
            assert!(
                page.contains("    println!(\"[REDACTED:GitHub Token]\");"),
                "{page}"
            );
            assert!(page.contains("api_key = '[REDACTED:api_key]'"), "{page}");
        }

        // Validation findings, an unresolved input shown as raw JSON, and an
        // unreadable record are redacted too.
        let unresolved = json!({
            "requirement": "Ready.",
            "determination": true,
            "rationale": "Checked.",
            "inputs": [{"file": "src/gone.rs", "item": "fn[gone]", "hash": token}],
        })
        .to_string();
        let unreadable = format!("{{broken {token}");
        let errors = [format!("evidence/requirements.ready.json: saw {token}")];
        for (record, shown) in [
            (&unresolved, "\"hash\": \"[REDACTED:GitHub Token]\""),
            (&unreadable, "{broken [REDACTED:GitHub Token]"),
        ] {
            let page = render_detail_page(
                "requirements.ready",
                Path::new("appendix/requirements-index/requirements.ready.md"),
                None,
                &definition,
                DetailEvidence {
                    state: EvidenceState::Invalid,
                    record: Some(record),
                    errors: &errors,
                    fixme_entries: &[],
                },
                &RenderSources {
                    root,
                    definitions: &BTreeMap::new(),
                    references: &[],
                    source_links: None,
                    outlines: &crate::inputs::OutlineCache::default(),
                },
            );
            assert!(!page.contains(&token), "{page}");
            assert!(page.contains("saw [REDACTED:GitHub Token]"), "{page}");
            assert!(page.contains(shown), "{page}");
        }
    }

    // {test}`requirements.index.detail_format`
    #[test]
    fn detail_page_renders_markdown_and_highlighted_input_sections() {
        let definition = Definition {
            path: Path::new("requirements/core.md").to_path_buf(),
            text: "Ready MUST work.\n\n## Example\n\nUse the CLI.".to_owned(),
            kind: DefinitionKind::Requirement,
        };
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("md")).unwrap();
        fs::write(
            root.join("src/main.rs"),
            "// lead\nfn main() {\n    println!(\"ready\");\n}\n",
        )
        .unwrap();
        fs::write(root.join("md/notes.md"), "```rust\nfn x() {}\n```\n").unwrap();
        fs::write(root.join("book.toml"), "[book]\n").unwrap();
        let record = json!({
            "requirement": "Stale text.",
            "determination": true,
            "rationale": "**Checked** against the source.\n\n- First check\n- Second check",
            "inputs": [
                crate::inputs::outline_input(root, "src/main.rs", "fn[main]").unwrap(),
                crate::inputs::outline_input(root, "md/notes.md", "").unwrap(),
                crate::inputs::outline_input(root, "book.toml", "").unwrap(),
            ]
        })
        .to_string();
        // The page shows the current source, flagging a changed hash.
        fs::write(
            root.join("src/main.rs"),
            "// lead\nfn main() {\n    println!(\"changed\");\n}\n",
        )
        .unwrap();
        let page = render_detail_page(
            "requirements.core.ready",
            Path::new("appendix/requirements-index/requirements.core.ready.md"),
            Some(Path::new("appendix/requirements-index.md")),
            &definition,
            DetailEvidence {
                state: EvidenceState::Valid,
                record: Some(&record),
                errors: &[],
                fixme_entries: &[],
            },
            &RenderSources {
                root,
                definitions: &BTreeMap::new(),
                references: &[],
                source_links: None,
                outlines: &crate::inputs::OutlineCache::default(),
            },
        );
        assert!(page.contains("img.shields.io/badge/evidence-Valid-237346"));
        assert!(page.contains("## Requirement\n\n> Ready MUST work.\n> \n> ## Example"));
        assert!(page.contains("## Rationale\n\n**Checked** against the source."));
        assert!(page.contains(
            "<summary id=\"habeas-input-1\"><strong>Input 1</strong>: <code>outline src/main.rs fn[main]</code></summary>"
        ));
        assert!(page.contains(
            "- File: <code>src/main.rs</code>\n- Selector: <code>fn[main]</code>\n- Kind: <code>fn</code>\n- <code>src/main.rs:1–4</code>\n"
        ), "{page}");
        assert!(
            page.contains(
                "```rust,noplayground\n// lead\nfn main() {\n    println!(\"changed\");\n}\n```"
            ),
            "{page}"
        );
        assert_eq!(
            page.matches("The recorded hash no longer matches").count(),
            1,
            "{page}"
        );
        assert!(
            page.contains("<code>outline md/notes.md (whole file)</code>"),
            "{page}"
        );
        assert!(page.contains("- Kind: <code>file</code>"), "{page}");
        assert!(
            page.contains("````markdown\n```rust\nfn x() {}\n```\n````"),
            "{page}"
        );
        assert!(page.contains("```ini\n[book]\n```"), "{page}");
        assert!(!page.contains("Stale text."));
    }

    // {test}`requirements.index.source_links`
    #[test]
    fn source_links_use_configured_repository_and_branch() {
        let config: Config = "[output.html]\ngit-repository-url = \"https://github.com/example/project\"\nedit-url-template = \"https://github.com/example/project/edit/release/{path}\"\n"
            .parse()
            .unwrap();
        let links = SourceLinks::from_config(&config).unwrap().unwrap();
        assert_eq!(
            links.input_url("src/evidence.rs", 26, 37).as_deref(),
            Some("https://github.com/example/project/blob/release/src/evidence.rs#L26-L37")
        );
        assert_eq!(
            links.input_url("src/a b#c.rs", 1, 1).as_deref(),
            Some("https://github.com/example/project/blob/release/src/a%20b%23c.rs#L1-L1")
        );
        assert!(links.input_url("../outside.rs", 1, 2).is_none());
        assert!(links.input_url("src/main.rs", 0, 2).is_none());
        assert_eq!(
            evidence_edit_path(Path::new("docs/md"), "requirements.core.ready"),
            Path::new("../../evidence/requirements.core.ready.json")
        );

        let definition = Definition {
            path: Path::new("requirements/core.md").to_path_buf(),
            text: "Ready MUST work.".to_owned(),
            kind: DefinitionKind::Requirement,
        };
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("src")).unwrap();
        fs::write(
            temp.path().join("src/evidence.rs"),
            format!(
                "{}pub fn validate_repository() {{\n{}}}\n",
                "\n".repeat(25),
                "    let _ = ();\n".repeat(10)
            ),
        )
        .unwrap();
        let record = json!({
            "rationale": "Checked.",
            "inputs": [crate::inputs::outline_input(temp.path(), "src/evidence.rs", "fn[validate_repository]").unwrap()]
        })
        .to_string();
        let page = render_detail_page(
            "requirements.core.ready",
            Path::new("appendix/requirements-index/requirements.core.ready.md"),
            Some(Path::new("appendix/requirements-index.md")),
            &definition,
            DetailEvidence {
                state: EvidenceState::Valid,
                record: Some(&record),
                errors: &[],
                fixme_entries: &[],
            },
            &RenderSources {
                root: temp.path(),
                definitions: &BTreeMap::new(),
                references: &[],
                source_links: Some(&links),
                outlines: &crate::inputs::OutlineCache::default(),
            },
        );
        assert!(page.contains(
            "<a href=\"https://github.com/example/project/blob/release/src/evidence.rs#L26-L37\"><code>src/evidence.rs:26–37</code></a>"
        ));

        let other_config: Config = "[output.html]\ngit-repository-url = \"https://github.com/example/project\"\nedit-url-template = \"https://github.com/other/project/edit/main/{path}\"\n"
            .parse()
            .unwrap();
        assert!(SourceLinks::from_config(&other_config).unwrap().is_none());
        let unsafe_config: Config = "[output.html]\ngit-repository-url = \"javascript:alert(1)\"\nedit-url-template = \"javascript:alert(1)/edit/main/{path}\"\n"
            .parse()
            .unwrap();
        assert!(SourceLinks::from_config(&unsafe_config).unwrap().is_none());
    }

    // {test}`requirements.index.detail_format`
    #[test]
    fn common_input_extensions_select_highlighting_languages() {
        for (file, expected) in [
            ("readme.md", "markdown"),
            ("record.json", "json"),
            ("book.toml", "ini"),
            ("Main.java", "java"),
            ("app.js", "javascript"),
            ("app.ts", "typescript"),
            ("script.py", "python"),
            ("notes.txt", "plaintext"),
        ] {
            assert_eq!(code_language(Some(file)), expected, "{file}");
        }
    }

    // {test}`requirements.index.chapter`
    #[test]
    fn repeated_index_markers_fail_with_a_clear_error() {
        let mut book = Book::new();
        for path in ["one.md", "two.md"] {
            book.push_item(Chapter::new(
                "Index",
                ":::{requirements-index}\n:::\n".to_owned(),
                path,
                Vec::new(),
            ));
        }
        let error = preprocess_book(&mut book, Path::new(".")).unwrap_err();
        assert!(error.to_string().contains("only one"));
    }

    // {test}`requirements.index.path_conflicts`
    #[test]
    fn generated_detail_path_cannot_replace_a_chapter() {
        let mut book = Book::new();
        book.push_item(Chapter::new(
            "Index",
            ":::{requirements-index}\n:::\n".to_owned(),
            "requirements-index.md",
            Vec::new(),
        ));
        book.push_item(Chapter::new(
            "Existing",
            String::new(),
            "requirements/requirements.ready.md",
            Vec::new(),
        ));
        let definitions = BTreeMap::from([(
            "requirements.ready".to_owned(),
            Definition {
                path: Path::new("requirements.md").to_path_buf(),
                text: "Ready MUST work.".to_owned(),
                kind: DefinitionKind::Requirement,
            },
        )]);
        let error = match DetailPlan::find(&book, &definitions, &["requirements.ready".to_owned()])
        {
            Err(error) => error,
            Ok(_) => panic!("a generated detail page replaced a chapter"),
        };
        assert!(error.to_string().contains("conflicts with a book chapter"));
    }

    // {test}`requirements.index.invalid_detail`
    #[test]
    fn invalid_evidence_detail_remains_readable() {
        let definition = Definition {
            path: Path::new("requirements.md").to_path_buf(),
            text: "Ready MUST work.".to_owned(),
            kind: DefinitionKind::Requirement,
        };
        let detail = Path::new("requirements-index/requirements.ready.md");
        let index = Path::new("requirements-index.md");
        let findings = ["evidence/requirements.ready.json: invalid JSON"];
        let page = render_detail_page(
            "requirements.ready",
            detail,
            Some(index),
            &definition,
            DetailEvidence {
                state: EvidenceState::Invalid,
                record: Some("{broken"),
                errors: &findings.map(str::to_owned),
                fixme_entries: &[],
            },
            &RenderSources {
                root: Path::new("."),
                definitions: &BTreeMap::new(),
                references: &[],
                source_links: None,
                outlines: &crate::inputs::OutlineCache::default(),
            },
        );
        assert!(page.contains("> Ready MUST work."));
        assert!(page.contains("## Validation findings"));
        assert!(page.contains("invalid JSON"));
        assert!(page.contains("No rationale recorded."));
        assert!(page.contains("No inputs recorded."));
        assert!(page.contains("## Unreadable evidence record"));
        assert!(page.contains("```json\n{broken\n```"));

        let missing = render_detail_page(
            "requirements.ready",
            detail,
            Some(index),
            &definition,
            DetailEvidence {
                state: EvidenceState::Missing,
                record: None,
                errors: &[],
                fixme_entries: &[],
            },
            &RenderSources {
                root: Path::new("."),
                definitions: &BTreeMap::new(),
                references: &[],
                source_links: None,
                outlines: &crate::inputs::OutlineCache::default(),
            },
        );
        assert!(missing.contains("> Ready MUST work."));
        assert!(missing.contains("No rationale recorded."));
        assert!(missing.contains("No inputs recorded."));
    }

    // {test}`requirements.index.state`
    #[test]
    fn findings_make_false_and_fixme_records_invalid() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        for dir in ["md", "evidence", "src"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        fs::write(root.join("book.toml"), "[book]\nsrc = \"md\"\n").unwrap();
        let names = [
            "false_clean",
            "false_stale",
            "false_malformed",
            "false_uncovered",
            "fixme_clean",
            "fixme_stale",
        ];
        let mut source = String::new();
        for name in names {
            source.push_str(&format!(":::{{req}} {name}\nThe {name} rule.\n:::\n\n"));
        }
        fs::write(root.join("md/rules.md"), &source).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "// IMPL`rules.false_uncovered`\nfn uncovered() {}\n\n// :::{fixme} rules.fixme_clean\n// :github: example/rules#9\n// The gap.\n// :::\nfn gap() {}\n"
                .replace("IMPL", "{impl}"),
        )
        .unwrap();
        let gap = crate::inputs::outline_input(root, "src/lib.rs", "fn[gap]").unwrap();
        for name in names {
            let determination = if name.starts_with("fixme") {
                json!({"FIXME": ["example/rules#9"]})
            } else {
                json!(false)
            };
            let text = if name.ends_with("stale") {
                "An older rule.".to_owned()
            } else {
                format!("The {name} rule.")
            };
            let mut record = json!({
                "requirement": text,
                "determination": determination,
                "rationale": format!("The {name} fixture."),
                "inputs": if name == "fixme_clean" { json!([gap]) } else { json!([]) },
            });
            if name == "false_malformed" {
                record["unexpected"] = json!(true);
            }
            fs::write(
                root.join(format!("evidence/rules.{name}.json")),
                record.to_string(),
            )
            .unwrap();
        }

        let mut book = Book::new();
        book.push_item(Chapter::new("Rules", source, "rules.md", Vec::new()));
        book.push_item(Chapter::new(
            "Index",
            ":::{requirements-index}\n:::\n".to_owned(),
            "requirements-index.md",
            Vec::new(),
        ));
        crate::evidence::tests::stamp_reference_sets(root);
        preprocess_book(&mut book, root).unwrap();
        let index = book
            .chapters()
            .find(|chapter| chapter.name == "Index")
            .unwrap();
        let rules = book.chapters().next().unwrap();
        for (name, state) in [
            ("false_clean", "Determination false"),
            ("false_stale", "Invalid"),
            ("false_malformed", "Invalid"),
            ("false_uncovered", "Invalid"),
            ("fixme_clean", "FIXME"),
            ("fixme_stale", "Invalid"),
        ] {
            let link = format!("| [`rules.{name}`](rules/rules.{name}.html)");
            let row = index
                .content
                .lines()
                .find(|line| line.starts_with(&link))
                .unwrap_or_else(|| panic!("missing {link} in:\n{}", index.content));
            assert_eq!(row.split('|').nth(2).unwrap().trim(), state, "{row}");
            let page = rules
                .sub_items
                .iter()
                .find_map(|item| match item {
                    BookItem::Chapter(chapter) if chapter.name == format!("rules.{name}") => {
                        Some(chapter)
                    }
                    _ => None,
                })
                .unwrap();
            // Every page with findings shows them, whatever the determination.
            assert_eq!(
                page.content.contains("Validation findings"),
                state != "FIXME",
                "{name}: {}",
                page.content
            );
        }
    }

    // {test}`requirements.examples.index`
    #[test]
    fn index_and_detail_pages_show_each_example_evidence_state() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        for dir in ["md", "evidence", "src"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        fs::write(root.join("book.toml"), "[book]\nsrc = \"md\"\n").unwrap();
        let body =
            |name: &str| format!("The {name} case. CITE`rules.ready`").replace("CITE", "{cite}");
        let names = ["valid", "pending", "fixme", "changed", "missing"];
        let mut source = ":::{req} ready\nReady.\n:::\n".to_owned();
        for name in names {
            source.push_str(&format!("\n:::{{example}} {name}\n{}\n:::\n", body(name)));
        }
        fs::write(root.join("md/rules.md"), &source).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "// TEST`rules.valid`\nfn valid_case() {}\n\n// :::{fixme} rules.fixme\n// :github: example/rules#7\n// The case is untested.\n// :::\nfn fixme_case() {}\n"
                .replace("TEST", "{test}"),
        )
        .unwrap();
        let test_input =
            crate::inputs::outline_input(root, "src/lib.rs", "fn[valid_case]").unwrap();
        let fixme_input =
            crate::inputs::outline_input(root, "src/lib.rs", "fn[fixme_case]").unwrap();
        for (name, determination, text, inputs) in [
            ("valid", json!(true), body("valid"), json!([test_input])),
            ("pending", json!(false), body("pending"), json!([])),
            (
                "fixme",
                json!({"FIXME": ["example/rules#7"]}),
                body("fixme"),
                json!([fixme_input]),
            ),
            (
                "changed",
                json!(true),
                "An older case text.".to_owned(),
                json!([]),
            ),
        ] {
            fs::write(
                root.join(format!("evidence/rules.{name}.json")),
                json!({
                    "example": text,
                    "determination": determination,
                    "rationale": format!("The {name} fixture."),
                    "inputs": inputs,
                })
                .to_string(),
            )
            .unwrap();
        }

        let mut book = Book::new();
        book.push_item(Chapter::new("Rules", source, "rules.md", Vec::new()));
        book.push_item(Chapter::new(
            "Index",
            ":::{requirements-index}\n:::\n".to_owned(),
            "requirements-index.md",
            Vec::new(),
        ));
        crate::evidence::tests::stamp_reference_sets(root);
        preprocess_book(&mut book, root).unwrap();
        let index = book
            .chapters()
            .find(|chapter| chapter.name == "Index")
            .unwrap();
        let rules = book.chapters().next().unwrap();
        for (name, state) in [
            ("valid", "Valid"),
            ("pending", "Determination false"),
            ("fixme", "FIXME"),
            ("changed", "Invalid"),
            ("missing", "Missing"),
        ] {
            let link = format!("| [`rules.{name}`](rules/rules.{name}.html) (example)");
            let row = index
                .content
                .lines()
                .find(|line| line.starts_with(&link))
                .unwrap_or_else(|| panic!("missing {link} in:\n{}", index.content));
            assert_eq!(row.split('|').nth(2).unwrap().trim(), state, "{row}");
            let page = rules
                .sub_items
                .iter()
                .find_map(|item| match item {
                    BookItem::Chapter(chapter) if chapter.name == format!("rules.{name}") => {
                        Some(chapter)
                    }
                    _ => None,
                })
                .unwrap();
            assert!(
                page.content.contains(&format!("alt=\"evidence {state}\"")),
                "{name}: {}",
                page.content
            );
        }
    }

    // {test}`requirements.examples.index`
    // {test}`requirements.index.detail_format`
    // {test}`requirements.index.invalid_detail`
    #[test]
    fn index_lists_examples_and_links_their_detail_pages() {
        let temp = TempDir::new().unwrap();
        fs::create_dir_all(temp.path().join("md")).unwrap();
        fs::create_dir_all(temp.path().join("evidence")).unwrap();
        fs::write(temp.path().join("book.toml"), "[book]\nsrc = \"md\"\n").unwrap();
        let source = ":::{req} ready\nReady.\n:::\n\n:::{example} pending\nPending case. CITE`rules.ready`\n:::\n\n:::{example} missing\nMissing case. CITE`rules.ready`\n:::\n"
            .replace("CITE", "{cite}");
        fs::write(temp.path().join("md/rules.md"), &source).unwrap();
        fs::write(
            temp.path().join("evidence/rules.pending.json"),
            json!({
                "example": "Pending case. CITE`rules.ready`".replace("CITE", "{cite}"),
                "determination": false,
                "rationale": "The case still needs a test.",
                "inputs": []
            })
            .to_string(),
        )
        .unwrap();

        let mut book = Book::new();
        book.push_item(Chapter::new(
            "Rules",
            source.to_owned(),
            "rules.md",
            Vec::new(),
        ));
        book.push_item(Chapter::new(
            "Index",
            ":::{requirements-index}\n:::\n".to_owned(),
            "requirements-index.md",
            Vec::new(),
        ));
        preprocess_book(&mut book, temp.path()).unwrap();

        let rules = book.chapters().next().unwrap();
        assert!(
            rules
                .content
                .contains("id=\"habeas-example-rules.pending\" href=\"rules/rules.pending.html\"")
        );
        let index = book
            .chapters()
            .find(|chapter| chapter.name == "Index")
            .unwrap();
        assert!(
            index
                .content
                .contains("| [`rules.pending`](rules/rules.pending.html) (example)")
        );
        assert!(
            index
                .content
                .contains("| [`rules.missing`](rules/rules.missing.html) (example)")
        );
        let pending = rules
            .sub_items
            .iter()
            .find_map(|item| match item {
                BookItem::Chapter(chapter) if chapter.name == "rules.pending" => Some(chapter),
                _ => None,
            })
            .unwrap();
        assert!(pending.content.contains("## Example\n\n> Pending case."));
        assert!(
            pending
                .content
                .contains("[View definition](../rules.html#habeas-example-rules.pending)")
        );
        assert!(pending.content.contains("The case still needs a test."));

        let missing = rules
            .sub_items
            .iter()
            .find_map(|item| match item {
                BookItem::Chapter(chapter) if chapter.name == "rules.missing" => Some(chapter),
                _ => None,
            })
            .unwrap();
        assert!(missing.content.contains("## Example\n\n> Missing case."));
        assert!(missing.content.contains("## Validation findings"));
        assert!(missing.content.contains("missing for example"));
        assert!(missing.content.contains("No rationale recorded."));
    }

    // {test}`requirements.index.detail_format`
    // {test}`requirements.references.anchors`
    #[test]
    fn detail_inputs_are_grouped_by_current_edges_in_disclosures() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        for dir in ["md", "evidence", "src", "skills"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        fs::write(root.join("book.toml"), "[book]\nsrc = \"md\"\n").unwrap();
        let source = ":::{req} ready\n:impl: skills/ready.md\nReady relies on CITE`rules.base`.\n:::\n\n:::{req} base\nBase.\n:::\n\n:::{example} case\nA case. TEST`rules.ready`\n:::\n"
            .replace("CITE", "{cite}")
            .replace("TEST", "{test}");
        fs::write(root.join("md/rules.md"), &source).unwrap();
        fs::write(root.join("skills/ready.md"), "# Skill\n").unwrap();
        fs::write(root.join("notes.txt"), "Notes.\n").unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "// IMPL`rules.ready`\nfn run() {}\n\n// TEST`rules.ready`\nfn check() {}\n"
                .replace("IMPL", "{impl}")
                .replace("TEST", "{test}"),
        )
        .unwrap();
        let outline =
            |file: &str, item: &str| crate::inputs::outline_input(root, file, item).unwrap();
        let inputs = json!([
            {"requirements": "rules.ready", "matches": ["rules.ready"], "text": ["ready"]},
            outline("src/lib.rs", "fn[check]"),
            outline("notes.txt", ""),
            {"requirements": "rules.base", "matches": ["rules.base"], "text": ["base"]},
            outline("skills/ready.md", "section[Skill]"),
            {"requirements": "rules.case", "matches": ["rules.case"], "text": ["case"]},
            outline("src/lib.rs", "fn[run]"),
        ]);
        fs::write(
            root.join("evidence/rules.ready.json"),
            json!({
                "requirement": "Ready relies on CITE`rules.base`.".replace("CITE", "{cite}"),
                "determination": true,
                "rationale": "Checked.",
                "inputs": inputs,
            })
            .to_string(),
        )
        .unwrap();

        let mut book = Book::new();
        book.push_item(Chapter::new("Rules", source, "rules.md", Vec::new()));
        preprocess_book(&mut book, root).unwrap();
        let page = book
            .chapters()
            .find(|chapter| chapter.name == "rules.ready")
            .unwrap();
        let content = &page.content;
        let position = |needle: &str| {
            content
                .find(needle)
                .unwrap_or_else(|| panic!("missing {needle:?} in:\n{content}"))
        };
        let groups = [
            ("Implementation", [5, 7].as_slice()),
            ("Tests", &[2, 6]),
            ("Referenced definitions", &[4]),
            ("Other sources", &[1, 3]),
        ];
        let starts = groups
            .iter()
            .map(|(heading, _)| position(&format!("style=\"display: inline\">{heading}</h3>")))
            .collect::<Vec<_>>();
        assert!(starts.windows(2).all(|pair| pair[0] < pair[1]), "{content}");
        for (group, (_, inputs)) in groups.iter().enumerate() {
            let end = starts.get(group + 1).copied().unwrap_or(content.len());
            for input in *inputs {
                let at = position(&format!("<summary id=\"habeas-input-{input}\">"));
                assert!(starts[group] < at && at < end, "input {input}:\n{content}");
            }
        }
        assert_eq!(
            content
                .matches("<details class=\"habeas-input-group\" open>")
                .count(),
            4
        );
        assert_eq!(
            content.matches("<details class=\"habeas-input\">").count(),
            7
        );
        assert_eq!(content.matches("</details>").count(), 11);
        assert!(content.contains("<code>requirements selection rules.base</code>"));
        assert!(!content.contains("Requirements index"));
    }

    // {test}`requirements.index.detail_format`
    // {test}`requirements.definitions.inputs`
    // {test}`requirements.ci.requirements_inputs`
    #[test]
    fn detail_page_shows_recorded_text_locations_and_first_matching_group() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        for dir in ["md", "evidence", "src"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        fs::write(root.join("book.toml"), "[book]\nsrc = \"md\"\n").unwrap();
        let source = ":::{req} ready\nReady uses TERM`rules.term`.\n:::\n\n:::{req} base\nBase.\n:::\n\n:::{definition} term\n:phrase: term\n\nA term.\n:::\n"
            .replace("TERM", "{term}");
        fs::write(root.join("md/rules.md"), &source).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "// IMPL`rules.ready`\n// TEST`rules.ready`\nfn both() {}\n"
                .replace("IMPL", "{impl}")
                .replace("TEST", "{test}"),
        )
        .unwrap();
        let inputs = json!([
            {"requirements": "rules.*", "matches": ["rules.base", "rules.ready"], "text": ["recorded base", "", "recorded ready"]},
            {"definition": "rules.term", "text": ["recorded term"]},
            {"requirements": "missing.*", "matches": ["missing.x"], "text": ["recorded missing"]},
            crate::inputs::outline_input(root, "src/lib.rs", "fn[both]").unwrap(),
        ]);
        fs::write(
            root.join("evidence/rules.ready.json"),
            json!({
                "requirement": "Ready uses TERM`rules.term`.".replace("TERM", "{term}"),
                "determination": true,
                "rationale": "Checked.",
                "inputs": inputs,
            })
            .to_string(),
        )
        .unwrap();

        let mut book = Book::new();
        book.push_item(Chapter::new("Rules", source, "rules.md", Vec::new()));
        preprocess_book(&mut book, root).unwrap();
        let page = book
            .chapters()
            .find(|chapter| chapter.name == "rules.ready")
            .unwrap();
        let content = &page.content;
        let position = |needle: &str| {
            content
                .find(needle)
                .unwrap_or_else(|| panic!("missing {needle:?} in:\n{content}"))
        };

        // The state badge has alt text naming the state.
        let badge =
            position("<img class=\"habeas-shield\" src=\"https://img.shields.io/badge/evidence-");
        assert!(
            content[badge..].contains("alt=\"evidence Invalid\""),
            "{content}"
        );

        // A requirements input lists each match's name, file, and inclusive
        // range, then its recorded text.
        let selection = position("<code>requirements selection rules.*</code>");
        let base = position("- <code>rules.base</code>: <code>md/rules.md:5–7</code>\n");
        let ready = position("- <code>rules.ready</code>: <code>md/rules.md:1–3</code>\n");
        let recorded = position("```markdown\nrecorded base\n\nrecorded ready\n```");
        assert!(
            selection < base && base < ready && ready < recorded,
            "{content}"
        );

        // A definition input shows its name, location, and recorded text.
        let definition = position("<code>definition rules.term</code>");
        let location = position("- <code>rules.term</code>: <code>md/rules.md:9–13</code>\n");
        let text = position("```markdown\nrecorded term\n```");
        assert!(definition < location && location < text, "{content}");

        // An unresolved selector is reported and its recorded text kept.
        let unresolved = position("Unresolved selector: ");
        let missing = position("```markdown\nrecorded missing\n```");
        assert!(unresolved < missing, "{content}");

        // A source reached by both an implementation and a test edge appears
        // once, in the first group that applies.
        let implementation = position("style=\"display: inline\">Implementation</h3>");
        let both = position("<code>outline src/lib.rs fn[both]</code>");
        assert!(implementation < both, "{content}");
        assert!(
            !content.contains("style=\"display: inline\">Tests</h3>"),
            "{content}"
        );
        assert_eq!(
            content.matches("outline src/lib.rs fn[both]").count(),
            1,
            "{content}"
        );
    }
}

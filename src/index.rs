use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use mdbook_preprocessor::book::{Book, BookItem, Chapter, SectionNumber};
use mdbook_preprocessor::config::Config;
use serde_json::Value;

use crate::markdown::render_evidence_table;
use crate::{
    Definition, DefinitionKind, EvidenceReport, chapter_href, colon_fence, definition_blocks,
    escape_html, overlaps_protected, protected_ranges, shield_badge, validate_repository,
};

pub(crate) struct IndexPlan {
    chapter_path: PathBuf,
    detail_paths: BTreeMap<String, PathBuf>,
}

// {impl}`requirements.index.source_links`
struct SourceLinks {
    blob_prefix: String,
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum EvidenceState {
    Valid,
    Missing,
    DeterminationFalse,
    Invalid,
}

pub(crate) struct LoadedDetailEvidence {
    state: EvidenceState,
    record: Option<String>,
    errors: Vec<String>,
}

struct DetailEvidence<'a> {
    state: EvidenceState,
    record: Option<&'a str>,
    errors: &'a [String],
}

impl LoadedDetailEvidence {
    pub(crate) fn load(root: &Path, name: &str, report: &EvidenceReport) -> Self {
        let evidence_path = root.join("evidence").join(format!("{name}.json"));
        let prefix = format!("evidence/{name}.json:");
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
                errors.push(format!("{prefix} cannot read evidence: {error}"));
                None
            }
        };
        let state = if record.is_none() && !evidence_path.exists() {
            EvidenceState::Missing
        } else if errors.is_empty() {
            EvidenceState::Valid
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

    // {impl}`requirements.index.detail_format`
    // {impl}`requirements.cli.info`
    fn borrowed(&self) -> DetailEvidence<'_> {
        DetailEvidence {
            state: self.state,
            record: self.record.as_deref(),
            errors: &self.errors,
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
            Self::Invalid => "Invalid",
        }
    }

    // {impl}`requirements.index.state`
    fn color(self) -> &'static str {
        match self {
            Self::Valid => "237346",
            Self::Missing => "5f6670",
            Self::DeterminationFalse => "ae5d14",
            Self::Invalid => "ad3028",
        }
    }
}

impl IndexPlan {
    // {impl}`requirements.index.chapter`
    // {impl}`requirements.examples.index`
    pub(crate) fn find(
        book: &Book,
        definitions: &BTreeMap<String, Definition>,
    ) -> io::Result<Option<Self>> {
        let mut chapter_path = None;
        for chapter in book.chapters() {
            for _ in marker_ranges(&chapter.content)? {
                if chapter_path.is_some() {
                    return Err(invalid_index(
                        "a book may contain only one {requirements-index} directive",
                    ));
                }
                chapter_path = chapter.path.clone();
            }
        }
        let Some(chapter_path) = chapter_path else {
            return Ok(None);
        };
        let detail_dir = chapter_path.with_extension("");
        let detail_paths = definitions
            .keys()
            .map(|name| (name.clone(), detail_dir.join(format!("{name}.md"))))
            .collect::<BTreeMap<_, _>>();
        let existing_paths = book
            .chapters()
            .filter_map(|chapter| chapter.path.as_ref())
            .collect::<BTreeSet<_>>();
        // {impl}`requirements.index.path_conflicts`
        for path in detail_paths.values() {
            if existing_paths.contains(path) {
                return Err(invalid_index(&format!(
                    "generated evidence page {} conflicts with a book chapter",
                    path.display()
                )));
            }
        }
        Ok(Some(Self {
            chapter_path,
            detail_paths,
        }))
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
    pub(crate) fn populate(
        &self,
        book: &mut Book,
        root: &Path,
        definitions: &BTreeMap<String, Definition>,
    ) -> io::Result<()> {
        let config = Config::from_disk(root.join("book.toml")).map_err(io::Error::other)?;
        let source_links = SourceLinks::from_config(&config)?;
        let report = validate_repository(root)?;
        let mut rows = Vec::new();
        let mut pages = Vec::new();
        for (name, detail_path) in &self.detail_paths {
            let evidence_path = root.join("evidence").join(format!("{name}.json"));
            let evidence = LoadedDetailEvidence::load(root, name, &report);
            let detail_href = chapter_href(&self.chapter_path, detail_path, None);
            let definition = definitions
                .get(name)
                .expect("every detail page has a definition");
            rows.push((
                format!("[`{name}`]({detail_href}) ({})", definition.kind.noun()),
                evidence.state().label(),
            ));
            let content = render_detail_page(
                name,
                detail_path,
                &self.chapter_path,
                definition,
                evidence.borrowed(),
                source_links.as_ref(),
            );
            // {impl}`requirements.index.source_links`
            let source_path = if evidence_path.exists() {
                evidence_edit_path(&config.book.src, name)
            } else {
                definition.path.clone()
            };
            pages.push((name.clone(), detail_path.clone(), content, source_path));
        }

        // {impl}`requirements.cli.shared_status_rendering`
        let rows = render_evidence_table(&rows);

        // {impl}`requirements.index.navigation`
        let mut found = false;
        book.for_each_chapter_mut(|chapter| {
            if chapter.path.as_deref() != Some(self.chapter_path.as_path()) {
                return;
            }
            found = true;
            let ranges = marker_ranges(&chapter.content)
                .expect("the index directive was checked before the book was changed");
            let range = ranges
                .into_iter()
                .next()
                .expect("the planned index directive is still present");
            chapter.content.replace_range(range, &rows);
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
            for (offset, (name, path, content, source_path)) in pages.iter().enumerate() {
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
        if !found {
            return Err(invalid_index("the requirements index chapter disappeared"));
        }
        Ok(())
    }
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
fn render_detail_page(
    name: &str,
    detail_path: &Path,
    index_path: &Path,
    definition: &Definition,
    detail_evidence: DetailEvidence<'_>,
    source_links: Option<&SourceLinks>,
) -> String {
    let definition_href = chapter_href(
        detail_path,
        &definition.path,
        Some(&definition.kind.anchor(name)),
    );
    let index_href = chapter_href(detail_path, index_path, None);
    render_detail_sections(
        format!(
            "# `{name}`\n\n[Requirements index]({index_href}) · [View definition]({definition_href})\n\n"
        ),
        &definition.text,
        definition.kind,
        detail_evidence,
        source_links,
    )
}

// {impl}`requirements.cli.info`
pub(crate) fn render_cli_detail_page(
    name: &str,
    source: &str,
    text: &str,
    evidence: &LoadedDetailEvidence,
) -> String {
    render_detail_sections(
        format!("# `{name}`\n\nDefinition: `{source}`\n\n"),
        text,
        DefinitionKind::Requirement,
        evidence.borrowed(),
        None,
    )
}

// {impl}`requirements.index.detail_format`
// {impl}`requirements.index.invalid_detail`
// {impl}`requirements.cli.info`
fn render_detail_sections(
    mut content: String,
    definition_text: &str,
    kind: DefinitionKind,
    detail_evidence: DetailEvidence<'_>,
    source_links: Option<&SourceLinks>,
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
    for line in definition_text.lines() {
        content.push_str("> ");
        content.push_str(line);
        content.push('\n');
    }

    if !detail_evidence.errors.is_empty() {
        content.push_str("\n## Validation findings\n\n<pre><code>");
        content.push_str(&escape_html(&detail_evidence.errors.join("\n\n")));
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
    content.push_str("\n\n## Inputs\n\n");
    match evidence
        .and_then(|evidence| evidence.get("inputs"))
        .and_then(Value::as_array)
    {
        Some(inputs) if !inputs.is_empty() => {
            for (index, input) in inputs.iter().enumerate() {
                render_input(&mut content, index + 1, input, source_links);
            }
        }
        _ => content.push_str("No inputs recorded.\n"),
    }
    if evidence.is_none()
        && let Some(record) = detail_evidence.record
    {
        content.push_str("\n## Unreadable evidence record\n\n");
        push_code_block(&mut content, record, "json");
    }
    content
}

// {impl}`requirements.index.source_links`
// {impl}`requirements.index.detail_format`
fn render_input(
    content: &mut String,
    index: usize,
    input: &Value,
    source_links: Option<&SourceLinks>,
) {
    let file = input.get("file").and_then(Value::as_str);
    let start = input.get("line_start").and_then(Value::as_u64);
    let end = input.get("line_end").and_then(Value::as_u64);
    content.push_str(&format!("<h3 id=\"habeas-input-{index}\">Input {index}"));
    if let (Some(file), Some(start), Some(end)) = (file, start, end) {
        let url = source_links.and_then(|links| links.input_url(file, start, end));
        if let Some(url) = url.as_deref() {
            content.push_str(": <a href=\"");
            content.push_str(&escape_html(url));
            content.push_str("\"><code>");
        } else {
            content.push_str(": <code>");
        }
        content.push_str(&escape_html(&format!("{file}:{start}–{end}")));
        content.push_str("</code>");
        if url.is_some() {
            content.push_str("</a>");
        }
    }
    content.push_str("</h3>\n\n");

    if let Some(lines) = input
        .get("text")
        .and_then(Value::as_array)
        .and_then(|lines| lines.iter().map(Value::as_str).collect::<Option<Vec<_>>>())
    {
        push_code_block(content, &lines.join("\n"), code_language(file));
    } else {
        let raw = serde_json::to_string_pretty(input).expect("JSON values can be serialized");
        push_code_block(content, &raw, "json");
    }
    content.push('\n');
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
        DetailEvidence, EvidenceState, IndexPlan, SourceLinks, code_language, evidence_edit_path,
        marker_ranges, render_detail_page,
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
            format!("// {{impl}}`requirements.core.uncovered`\n"),
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
        book.push_item(Chapter::new(
            "Requirements",
            source,
            "requirements/core.md",
            Vec::new(),
        ));
        let mut index_chapter = Chapter::new(
            "Requirements index",
            "# Requirements index\n\n:::{requirements-index}\n:::\n".to_owned(),
            "appendix/requirements-index.md",
            Vec::new(),
        );
        index_chapter.number = Some(SectionNumber::new(vec![4]));
        book.push_item(index_chapter);
        preprocess_book(&mut book, temp.path()).unwrap();

        let definition = book.chapters().next().unwrap();
        assert!(definition.content.contains(
            "id=\"habeas-req-requirements.core.ready\" href=\"../appendix/requirements-index/requirements.core.ready.html\""
        ));
        let index = book.chapters().nth(1).unwrap();
        assert!(!index.content.contains("{requirements-index}"));
        for (name, state) in [
            ("ready", "Valid"),
            ("pending", "Determination false"),
            ("changed", "Invalid"),
            ("absent", "Missing"),
            ("uncovered", "Invalid"),
        ] {
            let link = format!(
                "| [`requirements.core.{name}`](requirements-index/requirements.core.{name}.html)"
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
        assert_eq!(index.sub_items.len(), 5);
        let ready = index
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
            Some(&[4, 4][..])
        );
        assert_eq!(
            ready.path.as_deref(),
            Some(Path::new(
                "appendix/requirements-index/requirements.core.ready.md"
            ))
        );
        assert!(ready.content.contains(
            "[View definition](../../requirements/core.html#habeas-req-requirements.core.ready)"
        ));
        assert!(
            ready
                .content
                .contains("[Requirements index](../requirements-index.html)")
        );
        assert!(
            ready
                .content
                .contains("img.shields.io/badge/evidence-Valid-237346")
        );
        assert!(ready.content.contains("> Ready MUST work."));
        assert!(!ready.content.contains("## Evidence record"));
        let changed = index
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
        let absent = index
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

    // {test}`requirements.index.detail_format`
    #[test]
    fn detail_page_renders_markdown_and_highlighted_input_sections() {
        let definition = Definition {
            path: Path::new("requirements/core.md").to_path_buf(),
            text: "Ready MUST work.\n\n## Example\n\nUse the CLI.".to_owned(),
            kind: DefinitionKind::Requirement,
        };
        let record = json!({
            "requirement": "Stale text.",
            "determination": true,
            "rationale": "**Checked** against the source.\n\n- First check\n- Second check",
            "inputs": [
                {
                    "file": "src/main.rs",
                    "line_start": 2,
                    "line_end": 3,
                    "text": ["fn main() {", "    println!(\"ready\");"]
                },
                {
                    "file": "md/requirements.md",
                    "line_start": 8,
                    "line_end": 8,
                    "text": ["```rust"]
                },
                {
                    "file": "book.toml",
                    "line_start": 1,
                    "line_end": 1,
                    "text": ["[book]"]
                }
            ]
        })
        .to_string();
        let page = render_detail_page(
            "requirements.core.ready",
            Path::new("appendix/requirements-index/requirements.core.ready.md"),
            Path::new("appendix/requirements-index.md"),
            &definition,
            DetailEvidence {
                state: EvidenceState::Valid,
                record: Some(&record),
                errors: &[],
            },
            None,
        );
        assert!(page.contains("img.shields.io/badge/evidence-Valid-237346"));
        assert!(page.contains("## Requirement\n\n> Ready MUST work.\n> \n> ## Example"));
        assert!(page.contains("## Rationale\n\n**Checked** against the source."));
        assert!(
            page.contains("<h3 id=\"habeas-input-1\">Input 1: <code>src/main.rs:2–3</code></h3>")
        );
        assert!(page.contains("```rust,noplayground\nfn main()"));
        assert!(page.contains("````markdown\n```rust\n````"));
        assert!(page.contains("```ini\n[book]\n```"));
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
        let record = json!({
            "rationale": "Checked.",
            "inputs": [{
                "file": "src/evidence.rs",
                "line_start": 26,
                "line_end": 37,
                "text": ["pub fn validate_repository() {}"]
            }]
        })
        .to_string();
        let page = render_detail_page(
            "requirements.core.ready",
            Path::new("appendix/requirements-index/requirements.core.ready.md"),
            Path::new("appendix/requirements-index.md"),
            &definition,
            DetailEvidence {
                state: EvidenceState::Valid,
                record: Some(&record),
                errors: &[],
            },
            Some(&links),
        );
        assert!(page.contains(
            "<h3 id=\"habeas-input-1\">Input 1: <a href=\"https://github.com/example/project/blob/release/src/evidence.rs#L26-L37\"><code>src/evidence.rs:26–37</code></a></h3>"
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
            "requirements-index/requirements.ready.md",
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
        let error = match IndexPlan::find(&book, &definitions) {
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
            index,
            &definition,
            DetailEvidence {
                state: EvidenceState::Invalid,
                record: Some("{broken"),
                errors: &findings.map(str::to_owned),
            },
            None,
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
            index,
            &definition,
            DetailEvidence {
                state: EvidenceState::Missing,
                record: None,
                errors: &[],
            },
            None,
        );
        assert!(missing.contains("> Ready MUST work."));
        assert!(missing.contains("No rationale recorded."));
        assert!(missing.contains("No inputs recorded."));
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

        let chapters = book.chapters().collect::<Vec<_>>();
        assert!(chapters[0].content.contains(
            "id=\"habeas-example-rules.pending\" href=\"requirements-index/rules.pending.html\""
        ));
        let index = chapters[1];
        assert!(
            index
                .content
                .contains("| [`rules.pending`](requirements-index/rules.pending.html) (example)")
        );
        assert!(
            index
                .content
                .contains("| [`rules.missing`](requirements-index/rules.missing.html) (example)")
        );
        let pending = index
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

        let missing = index
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
}

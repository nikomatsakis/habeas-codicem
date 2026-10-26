use std::collections::BTreeMap;
use std::io;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use mdbook_preprocessor::book::Book;
use pulldown_cmark::{Event, Parser, Tag};

mod evidence;
mod index;

pub use evidence::{EvidenceReport, validate_repository};

/// Validate the current Cargo and mdBook project for use in a Cargo test.
///
/// Panics with all validation errors so an integration test can simply call
/// `habeas::validate()`.
#[track_caller]
pub fn validate() {
    // {impl}`requirements.integration.validate`
    let cwd = std::env::current_dir().expect("cannot determine current directory");
    let root = cwd
        .ancestors()
        .find(|path| path.join("Cargo.toml").is_file() && path.join("book.toml").is_file())
        .expect("cannot find Cargo and mdBook project root");
    let report = validate_repository(root).expect("cannot validate requirement evidence");
    if !report.errors.is_empty() {
        panic!(
            "Habeas validation failed ({} requirements, {} evidence files, {} errors):\n- {}",
            report.requirements,
            report.evidence_files,
            report.errors.len(),
            report.errors.join("\n- ")
        );
    }
}

pub(crate) struct RequirementBlock {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) body_start: usize,
    pub(crate) body_end: usize,
    pub(crate) name: String,
    pub(crate) closed: bool,
}

pub(crate) struct Definition {
    pub(crate) path: PathBuf,
    pub(crate) text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReferenceKind {
    Impl,
    Cite,
    Test,
}

// {impl}`requirements.references.book_badges`
impl ReferenceKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Impl => "impl",
            Self::Cite => "cite",
            Self::Test => "test",
        }
    }

    fn color(self) -> &'static str {
        match self {
            Self::Impl => "237346",
            Self::Cite => "7151a1",
            Self::Test => "ae5d14",
        }
    }
}

pub(crate) struct ReferenceMatch<'a> {
    pub(crate) start: usize,
    end: usize,
    pub(crate) name: &'a str,
    pub(crate) kind: ReferenceKind,
}

// {impl}`requirements.cli.mdbook`
pub fn preprocess_book(book: &mut Book, root: &Path) -> io::Result<()> {
    let mut definitions = BTreeMap::new();
    for chapter in book.chapters() {
        let Some(source_path) = chapter.source_path.as_deref().or(chapter.path.as_deref()) else {
            continue;
        };
        let Some(render_path) = chapter.path.as_deref() else {
            continue;
        };
        let namespace = chapter_namespace(source_path);
        for block in requirement_blocks(&chapter.content) {
            if !block.closed {
                continue;
            }
            let name = full_requirement_name(&namespace, &block.name);
            definitions.entry(name).or_insert_with(|| Definition {
                path: render_path.to_path_buf(),
                text: chapter.content[block.body_start..block.body_end]
                    .trim()
                    .to_owned(),
            });
        }
    }
    let index = index::IndexPlan::find(book, &definitions)?;
    if let Some(plan) = index.as_ref() {
        plan.populate(book, root, &definitions)?;
    }

    // {impl}`requirements.index.source_links`
    book.for_each_chapter_mut(|chapter| {
        if chapter.source_path.is_none()
            || chapter
                .path
                .as_deref()
                .is_some_and(|path| index.as_ref().is_some_and(|plan| plan.is_detail_path(path)))
        {
            return;
        }
        let namespace = chapter
            .source_path
            .as_deref()
            .or(chapter.path.as_deref())
            .map(chapter_namespace)
            .unwrap_or_default();
        chapter.content = replace_requirements(
            &chapter.content,
            &namespace,
            chapter.path.as_deref(),
            index.as_ref(),
        );
        if let Some(path) = chapter.path.as_deref() {
            chapter.content = replace_references(&chapter.content, path, &definitions);
        }
    });
    Ok(())
}

// {impl}`requirements.ci.identifiers`
fn chapter_namespace(path: &Path) -> String {
    let path = path.with_extension("");
    let mut parts: Vec<_> = path
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();

    if parts.len() > 1 && parts[0] == "reqs" {
        parts.remove(0);
    }
    if parts.len() > 1 && matches!(parts.last().map(String::as_str), Some("index" | "README")) {
        parts.pop();
    }

    parts.join(".")
}

// {impl}`requirements.index.navigation`
// {impl}`requirements.references.book_badges`
fn replace_requirements(
    content: &str,
    namespace: &str,
    current_path: Option<&Path>,
    index: Option<&index::IndexPlan>,
) -> String {
    let mut result = String::with_capacity(content.len());
    let mut cursor = 0;

    for block in requirement_blocks(content)
        .into_iter()
        .filter(|block| block.closed)
    {
        result.push_str(&content[cursor..block.start]);
        let full_name = full_requirement_name(namespace, &block.name);
        // {impl}`requirements.references.safe_badges`
        let escaped_name = escape_html(&full_name);
        let badge = shield_badge("req", &full_name, "006ba8");
        let detail_href = current_path
            .zip(index)
            .and_then(|(current, plan)| plan.detail_path(&full_name).map(|path| (current, path)))
            .map(|(current, path)| escape_html(&chapter_href(current, path, None)));
        let tag = if detail_href.is_some() { "a" } else { "span" };
        result.push('<');
        result.push_str(tag);
        result.push_str(" class=\"habeas-req-badge\" id=\"habeas-req-");
        result.push_str(&escaped_name);
        result.push('"');
        if let Some(href) = detail_href {
            result.push_str(" href=\"");
            result.push_str(&href);
            result.push('"');
        }
        result.push('>');
        result.push_str(&badge);
        result.push_str("</");
        result.push_str(tag);
        result.push_str(">\n\n");
        result.push_str(&content[block.body_start..block.body_end]);
        result.push('\n');
        cursor = block.end;
    }

    result.push_str(&content[cursor..]);
    result
}

// {impl}`requirements.references.book_badges`
fn replace_references(
    content: &str,
    current_path: &Path,
    definitions: &BTreeMap<String, Definition>,
) -> String {
    let mut result = String::with_capacity(content.len());
    let mut cursor = 0;

    for reference in reference_roles(content, true) {
        result.push_str(&content[cursor..reference.start]);
        if let Some(target) = definitions.get(reference.name) {
            let kind = reference.kind.label();
            let badge = shield_badge(kind, reference.name, reference.kind.color());
            // {impl}`requirements.references.safe_badges`
            let href = escape_html(&chapter_href(
                current_path,
                &target.path,
                Some(&format!("habeas-req-{}", reference.name)),
            ));
            result.push_str("<a class=\"habeas-req-badge habeas-req-ref-");
            result.push_str(kind);
            result.push_str("\" href=\"");
            result.push_str(&href);
            result.push_str("\">");
            result.push_str(&badge);
            result.push_str("</a>");
        } else {
            result.push_str(&content[reference.start..reference.end]);
        }
        cursor = reference.end;
    }

    result.push_str(&content[cursor..]);
    result
}

// {impl}`requirements.references.book_badges`
// {impl}`requirements.references.safe_badges`
pub(crate) fn shield_badge(label: &str, message: &str, color: &str) -> String {
    let url = format!(
        "https://img.shields.io/badge/{}-{}-{color}?style=flat-square&labelColor=555",
        shield_path_component(label),
        shield_path_component(message)
    );
    format!(
        "<img class=\"habeas-shield\" src=\"{}\" alt=\"{} {}\" loading=\"lazy\">",
        escape_html(&url),
        escape_html(label),
        escape_html(message)
    )
}

// {impl}`requirements.references.safe_badges`
fn shield_path_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' => encoded.push(char::from(byte)),
            b' ' => encoded.push('_'),
            b'_' => encoded.push_str("__"),
            b'-' => encoded.push_str("--"),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

// {impl}`requirements.index.navigation`
pub(crate) fn chapter_href(current: &Path, target: &Path, anchor: Option<&str>) -> String {
    let current_dir = current
        .parent()
        .into_iter()
        .flat_map(Path::components)
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let target_html = target
        .with_extension("html")
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let shared = current_dir
        .iter()
        .zip(&target_html)
        .take_while(|(left, right)| left == right)
        .count();
    let mut href = "../".repeat(current_dir.len() - shared);
    href.push_str(&target_html[shared..].join("/"));
    if let Some(anchor) = anchor {
        href.push('#');
        href.push_str(anchor);
    }
    // mdBook interprets ".md" inside a generated URL as a chapter suffix.
    href.replace(".md", "%2Emd")
}

// {impl}`requirements.ci.identifiers`
fn full_requirement_name(namespace: &str, name: &str) -> String {
    if namespace.is_empty() {
        name.to_owned()
    } else {
        format!("{namespace}.{name}")
    }
}

// {impl}`requirements.ci.span`
// {impl}`requirements.ci.identifiers`
// {impl}`requirements.references.markdown_literals`
pub(crate) fn requirement_blocks(content: &str) -> Vec<RequirementBlock> {
    let code_ranges = Parser::new(content)
        .into_offset_iter()
        .filter_map(|(event, range)| {
            matches!(event, Event::Start(Tag::CodeBlock(_))).then_some(range)
        })
        .collect::<Vec<_>>();
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

    let mut blocks = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let (start, body_start, line) = lines[index];
        let Some((suffix, fence_length)) = colon_fence(line) else {
            index += 1;
            continue;
        };
        let Some(rest) = suffix.strip_prefix("{req}") else {
            index += 1;
            continue;
        };
        if overlaps_protected(start, body_start, &code_ranges)
            || !rest.chars().next().is_some_and(char::is_whitespace)
            || !valid_name(rest.trim())
        {
            index += 1;
            continue;
        }
        let name = rest.trim();

        let close_index = (index + 1..lines.len()).find(|&candidate| {
            !overlaps_protected(lines[candidate].0, lines[candidate].1, &code_ranges)
                && colon_fence(lines[candidate].2).is_some_and(|(suffix, count)| {
                    count >= fence_length && suffix.trim().is_empty()
                })
        });
        let (end, body_end, closed) = if let Some(close_index) = close_index {
            (lines[close_index].1, lines[close_index].0, true)
        } else {
            (content.len(), content.len(), false)
        };
        blocks.push(RequirementBlock {
            start,
            end,
            body_start,
            body_end,
            name: name.to_owned(),
            closed,
        });
        index = close_index.map_or(lines.len(), |close_index| close_index + 1);
    }
    blocks
}

// {impl}`requirements.ci.span`
fn colon_fence(line: &str) -> Option<(&str, usize)> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let length = trimmed.bytes().take_while(|byte| *byte == b':').count();
    (length >= 3).then_some((&trimmed[length..], length))
}

// {impl}`requirements.references.markdown_literals`
fn overlaps_protected(start: usize, end: usize, ranges: &[Range<usize>]) -> bool {
    ranges
        .iter()
        .any(|range| start < range.end && range.start < end)
}

// {impl}`requirements.references.targets`
// {impl}`requirements.ci.identifiers`
// {impl}`requirements.references.markdown_literals`
pub(crate) fn reference_roles(content: &str, markdown: bool) -> Vec<ReferenceMatch<'_>> {
    let protected = if markdown {
        protected_ranges(content)
    } else {
        Vec::new()
    };
    let mut references = Vec::new();

    for (start, _) in content.match_indices('{') {
        if is_escaped(content, start) || is_protected(start, &protected) {
            continue;
        }
        let remaining = &content[start..];
        let (kind, prefix) = if remaining.starts_with("{impl}`") {
            (ReferenceKind::Impl, "{impl}`")
        } else if remaining.starts_with("{cite}`") {
            (ReferenceKind::Cite, "{cite}`")
        } else if remaining.starts_with("{test}`") {
            (ReferenceKind::Test, "{test}`")
        } else {
            continue;
        };
        let name_start = start + prefix.len();
        let Some(name_end_offset) = content[name_start..].find('`') else {
            continue;
        };
        let name_end = name_start + name_end_offset;
        let name = &content[name_start..name_end];
        if valid_name(name) {
            references.push(ReferenceMatch {
                start,
                end: name_end + 1,
                name,
                kind,
            });
        }
    }

    references
}

// {impl}`requirements.references.markdown_literals`
fn protected_ranges(content: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();

    for (event, range) in Parser::new(content).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_)) => ranges.push(range),
            Event::Code(_) | Event::Html(_) | Event::InlineHtml(_) => ranges.push(range),
            _ => {}
        }
    }

    ranges
}

// {impl}`requirements.references.markdown_literals`
fn is_protected(position: usize, ranges: &[Range<usize>]) -> bool {
    ranges
        .iter()
        .any(|range| range.start <= position && position < range.end)
}

// {impl}`requirements.references.markdown_literals`
fn is_escaped(content: &str, position: usize) -> bool {
    content[..position]
        .bytes()
        .rev()
        .take_while(|byte| *byte == b'\\')
        .count()
        % 2
        == 1
}

// {impl}`requirements.ci.identifiers`
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        })
}

// {impl}`requirements.references.safe_badges`
fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::{
        chapter_href, chapter_namespace, preprocess_book, reference_roles, replace_requirements,
        requirement_blocks, shield_badge,
    };
    use mdbook_preprocessor::book::{Book, Chapter};
    use std::path::Path;

    // {test}`requirements.ci.identifiers`
    #[test]
    fn chapter_path_prefixes_the_requirement_name() {
        assert_eq!(
            chapter_namespace(Path::new("requirements.md")),
            "requirements"
        );
        assert_eq!(chapter_namespace(Path::new("reqs/foo/bar.md")), "foo.bar");
        assert_eq!(chapter_namespace(Path::new("reqs/foo/README.md")), "foo");
        assert_eq!(chapter_namespace(Path::new("reqs/foo/index.md")), "foo");

        let rendered = replace_requirements(
            ":::{req} tui.table\nThe table MUST be visible.\n:::",
            "requirements",
            None,
            None,
        );
        assert!(rendered.contains("habeas-req-badge"));
        assert!(rendered.contains("alt=\"req requirements.tui.table\""));
        assert!(!rendered.contains("{req}"));
    }

    // {test}`requirements.index.navigation`
    #[test]
    fn links_preserve_requirement_names_containing_md() {
        let detail = chapter_href(
            Path::new("requirements.md"),
            Path::new("requirements-index/requirements.cli.mdbook.md"),
            None,
        );
        assert_eq!(detail, "requirements-index/requirements.cli%2Emdbook.html");

        let definition = chapter_href(
            Path::new("requirements-index/requirements.cli.mdbook.md"),
            Path::new("requirements.md"),
            Some("habeas-req-requirements.cli.mdbook"),
        );
        assert_eq!(
            definition,
            "../requirements.html#habeas-req-requirements.cli%2Emdbook"
        );
    }

    // {test}`requirements.references.markdown_literals`
    #[test]
    fn code_examples_and_legacy_roles_remain_literal() {
        let source = "`{req}`inline``\n\n```text\n   :::{req} block\n```\n\n{req}`legacy`\n\n:::{req} visible\nVisible.\n:::";
        let rendered = replace_requirements(source, "requirements", None, None);
        assert!(rendered.contains("{req}`inline`"));
        assert!(rendered.contains(":::{req} block"));
        assert!(rendered.contains("{req}`legacy`"));
        assert!(rendered.contains("alt=\"req requirements.visible\""));
    }

    // {test}`requirements.ci.identifiers`
    // {test}`requirements.references.safe_badges`
    #[test]
    fn invalid_names_remain_literal_and_html_is_escaped() {
        let rendered = replace_requirements(
            ":::{req} bad name\nInvalid.\n:::\n\n:::{req} good\nGood.\n:::",
            "a&b",
            None,
            None,
        );
        assert!(rendered.contains(":::{req} bad name"));
        assert!(rendered.contains("alt=\"req a&amp;b.good\""));
        assert!(reference_roles("{impl}`bad name`", false).is_empty());
    }

    // {test}`requirements.references.book_badges`
    // {test}`requirements.references.safe_badges`
    #[test]
    fn shields_badge_urls_escape_requirement_names() {
        let badge = shield_badge("req", "requirements.foo_bar-baz", "006ba8");
        assert!(badge.contains(
            "https://img.shields.io/badge/req-requirements.foo__bar--baz-006ba8?style=flat-square&amp;labelColor=555"
        ));
        assert!(badge.contains("alt=\"req requirements.foo_bar-baz\""));

        let status = shield_badge("evidence", "Determination false", "ae5d14");
        assert!(status.contains("badge/evidence-Determination_false-ae5d14"));
    }

    // {test}`requirements.references.book_badges`
    #[test]
    fn references_in_nested_chapters_link_to_the_requirement_definition() {
        let mut book = Book::new();
        book.push_item(Chapter::new(
            "Requirements",
            ":::{req} ready\nReady.\n:::".to_owned(),
            "requirements.md",
            Vec::new(),
        ));
        let references = ["impl", "cite", "test"]
            .iter()
            .map(|kind| format!("{{{kind}}}`requirements.ready`"))
            .collect::<Vec<_>>()
            .join(" ");
        book.push_item(Chapter::new(
            "Guide",
            references,
            "guides/deep.md",
            Vec::new(),
        ));

        preprocess_book(&mut book, Path::new(".")).unwrap();
        let chapters = book.chapters().collect::<Vec<_>>();
        assert!(
            chapters[0]
                .content
                .contains("id=\"habeas-req-requirements.ready\"")
        );
        for (kind, color) in [("impl", "237346"), ("cite", "7151a1"), ("test", "ae5d14")] {
            assert!(chapters[1].content.contains(&format!(
                "class=\"habeas-req-badge habeas-req-ref-{kind}\" href=\"../requirements.html#habeas-req-requirements.ready\""
            )));
            assert!(chapters[1].content.contains(&format!(
                "img.shields.io/badge/{kind}-requirements.ready-{color}"
            )));
        }
    }

    // {test}`requirements.references.markdown_literals`
    #[test]
    fn markdown_examples_and_escaped_references_stay_literal() {
        let marker = format!("{{impl}}`requirements.ready`");
        let content = format!("`{marker}`\n\n```text\n{marker}\n```\n\n\\{marker}");
        let mut book = Book::new();
        book.push_item(Chapter::new(
            "Requirements",
            ":::{req} ready\nReady.\n:::".to_owned(),
            "requirements.md",
            Vec::new(),
        ));
        book.push_item(Chapter::new("Guide", content, "guide.md", Vec::new()));

        preprocess_book(&mut book, Path::new(".")).unwrap();
        let guide = book.chapters().nth(1).unwrap();
        assert_eq!(guide.content.matches(&marker).count(), 3);
        assert!(!guide.content.contains("habeas-req-ref-impl"));
    }

    // {test}`requirements.references.book_badges`
    #[test]
    fn colon_directives_render_markdown_bodies_and_legacy_roles_stay_literal() {
        let source = "An inline {req}`inline` role.\n\n:::{req} defined\nDefinition.\n\n## Details\n\n```text\nexample\n```\n:::\n\nAfter.";
        let rendered = replace_requirements(source, "requirements", None, None);
        assert!(rendered.contains("An inline {req}`inline` role."));
        assert!(!rendered.contains("habeas-req-requirements.inline"));
        assert!(rendered.contains("id=\"habeas-req-requirements.defined\""));
        assert!(rendered.contains("## Details"));
        assert!(rendered.contains("```text\nexample\n```"));
        assert!(rendered.contains("After."));
        assert!(!rendered.contains(":::{req} defined"));
    }

    // {test}`requirements.references.targets`
    #[test]
    fn references_in_requirement_bodies_are_scanned_as_markdown() {
        let known = format!("{{cite}}`requirements.ready`");
        let example = format!("{{cite}}`missing.example`");
        let content = format!(":::{{req}} ready\nSee {known}.\n\n```text\n{example}\n```\n:::");
        let references = reference_roles(&content, true);
        assert_eq!(references.len(), 1);
        assert_eq!(references[0].name, "requirements.ready");
    }

    // {test}`requirements.ci.span`
    #[test]
    fn a_longer_colon_fence_contains_a_shorter_nested_directive() {
        let content = "::::{req} ready\nIntro.\n\n:::{note}\nNested.\n:::\n::::\n\nOutside.";
        let blocks = requirement_blocks(content);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].closed);
        assert_eq!(
            &content[blocks[0].body_start..blocks[0].body_end],
            "Intro.\n\n:::{note}\nNested.\n:::\n"
        );
        assert!(content[blocks[0].end..].contains("Outside."));
    }
}

mod commands;
mod markdown;
pub use commands::{info_markdown, status_markdown};

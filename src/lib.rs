use std::collections::BTreeMap;
use std::io;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use mdbook_preprocessor::book::Book;
use pulldown_cmark::{Event, Parser, Tag};

mod add_input;
pub mod authoring;
pub mod config;
mod definition_glob;
mod evidence;
mod fixme;
mod index;
mod inputs;
pub mod judgment;
mod rust_coverage;
mod rust_items;
mod terms;

pub use add_input::{InputMode, InputTarget, add_input};
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
            "Habeas validation failed ({} requirements, {} examples, {} evidence files, {} errors):\n- {}\n\nRun `habeas author --all` to author missing or outdated evidence using the configured model, then address any reported gaps and rerun the tests. Do not write evidence judgments or snapshots by hand.",
            report.requirements,
            report.examples,
            report.evidence_files,
            report.errors.len(),
            report.errors.join("\n- ")
        );
    }
}

// {impl}`requirements.examples.definition`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DefinitionKind {
    Requirement,
    Example,
    Term,
}

impl DefinitionKind {
    // {impl}`requirements.ci.errors`
    // {impl}`requirements.examples.evidence`
    // {impl}`requirements.definitions.declaration`
    pub(crate) fn noun(self) -> &'static str {
        match self {
            Self::Requirement => "requirement",
            Self::Example => "example",
            Self::Term => "definition",
        }
    }

    // {impl}`requirements.references.book_badges`
    // {impl}`requirements.examples.index`
    // {impl}`requirements.definitions.declaration`
    pub(crate) fn badge(self) -> &'static str {
        match self {
            Self::Requirement => "req",
            Self::Example => "example",
            Self::Term => "definition",
        }
    }

    // {impl}`requirements.references.book_badges`
    // {impl}`requirements.examples.index`
    // {impl}`requirements.definitions.references`
    // {impl}`requirements.index.navigation`
    pub(crate) fn anchor(self, name: &str) -> String {
        format!("habeas-{}-{name}", self.badge())
    }
}

// {impl}`requirements.references.anchors`
pub(crate) struct DefinitionBlock {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) body_start: usize,
    /// Start of the body after any option lines.
    pub(crate) text_start: usize,
    pub(crate) body_end: usize,
    pub(crate) name: String,
    pub(crate) kind: DefinitionKind,
    pub(crate) closed: bool,
    pub(crate) options: Vec<DefinitionOption>,
}

/// An option line such as `:impl: src/skill.md` immediately after the opening
/// fence of a `{req}` or `{example}` directive.
// {impl}`requirements.references.anchors`
pub(crate) struct DefinitionOption {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) line: usize,
    pub(crate) name: String,
    pub(crate) value: String,
}

pub(crate) struct Definition {
    pub(crate) path: PathBuf,
    pub(crate) text: String,
    pub(crate) kind: DefinitionKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReferenceKind {
    Impl,
    Cite,
    Test,
    Term,
}

// {impl}`requirements.references.book_badges`
impl ReferenceKind {
    // {impl}`requirements.references.book_badges`
    // {impl}`requirements.definitions.references`
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Impl => "impl",
            Self::Cite => "cite",
            Self::Test => "test",
            Self::Term => "term",
        }
    }

    // {impl}`requirements.references.book_badges`
    // {impl}`requirements.definitions.references`
    fn color(self) -> &'static str {
        match self {
            Self::Impl => "237346",
            Self::Cite => "7151a1",
            Self::Test => "ae5d14",
            Self::Term => "006ba8",
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
// {impl}`requirements.examples.definition`
// {impl}`requirements.definitions.declaration`
// {impl}`requirements.definitions.references`
// {impl}`requirements.definitions.selection`
// {impl}`requirements.index.detail_pages`
// {impl}`requirements.references.anchors`
pub fn preprocess_book(book: &mut Book, root: &Path) -> io::Result<()> {
    let mut definitions = BTreeMap::new();
    let mut order = Vec::new();
    for chapter in book.chapters() {
        let Some(source_path) = chapter.source_path.as_deref().or(chapter.path.as_deref()) else {
            continue;
        };
        let Some(render_path) = chapter.path.as_deref() else {
            continue;
        };
        let namespace = chapter_namespace(source_path);
        let blocks = definition_blocks(&chapter.content);
        for block in &blocks {
            if !block.closed {
                continue;
            }
            let name = full_requirement_name(&namespace, &block.name);
            if definitions.contains_key(&name) {
                continue;
            }
            if block.kind != DefinitionKind::Term {
                order.push(name.clone());
            }
            definitions.insert(
                name,
                Definition {
                    path: render_path.to_path_buf(),
                    text: without_options(
                        &chapter.content,
                        block.body_start..block.body_end,
                        &blocks,
                    )
                    .trim()
                    .to_owned(),
                    kind: block.kind,
                },
            );
        }
    }
    let evidence_definitions = order
        .iter()
        .map(|name| {
            let d = &definitions[name];
            (
                name.clone(),
                Definition {
                    path: d.path.clone(),
                    text: d.text.clone(),
                    kind: d.kind,
                },
            )
        })
        .collect();
    let plan = index::DetailPlan::find(book, &evidence_definitions, &order)?;
    plan.populate(book, root, &evidence_definitions)?;

    // {impl}`requirements.index.source_links`
    book.for_each_chapter_mut(|chapter| {
        if chapter.source_path.is_none()
            || chapter
                .path
                .as_deref()
                .is_some_and(|path| plan.is_detail_path(path))
        {
            if let Some(path) = chapter.path.as_deref() {
                chapter.content = replace_references(&chapter.content, path, &definitions);
            }
            return;
        }
        let namespace = chapter
            .source_path
            .as_deref()
            .or(chapter.path.as_deref())
            .map(chapter_namespace)
            .unwrap_or_default();
        chapter.content = replace_definitions(
            &chapter.content,
            &namespace,
            chapter.path.as_deref(),
            Some(&plan),
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
// {impl}`requirements.examples.definition`
// {impl}`requirements.definitions.declaration`
// {impl}`requirements.definitions.references`
// {impl}`requirements.examples.index`
// {impl}`requirements.references.anchors`
fn replace_definitions(
    content: &str,
    namespace: &str,
    current_path: Option<&Path>,
    plan: Option<&index::DetailPlan>,
) -> String {
    let mut result = String::with_capacity(content.len());
    let mut cursor = 0;

    for block in definition_blocks(content)
        .into_iter()
        .filter(|block| block.closed)
    {
        if block.start < cursor {
            continue;
        }
        result.push_str(&content[cursor..block.start]);
        let full_name = full_requirement_name(namespace, &block.name);
        // {impl}`requirements.references.safe_badges`
        let escaped_name = escape_html(&full_name);
        if block.kind == DefinitionKind::Term {
            if let Ok((phrase, body)) =
                terms::declaration(&content[block.body_start..block.body_end])
            {
                result.push_str(&format!(
                    "<strong id=\"{}\">{}</strong>\n\n",
                    escape_html(&block.kind.anchor(&full_name)),
                    terms::escaped_phrase(phrase)
                ));
                result.push_str(body);
                result.push('\n');
            } else {
                result.push_str(&content[block.start..block.end]);
            }
            cursor = block.end;
            continue;
        }
        let badge = shield_badge(
            block.kind.badge(),
            &full_name,
            if block.kind == DefinitionKind::Example {
                "5b62a8"
            } else {
                "006ba8"
            },
        );
        let detail_href = current_path
            .zip(plan)
            .and_then(|(current, plan)| plan.detail_path(&full_name).map(|path| (current, path)))
            .map(|(current, path)| escape_html(&chapter_href(current, path, None)));
        let tag = if detail_href.is_some() { "a" } else { "span" };
        result.push('<');
        result.push_str(tag);
        result.push_str(" class=\"habeas-req-badge\" id=\"habeas-");
        result.push_str(block.kind.badge());
        result.push('-');
        result.push_str(&escaped_name);
        result.push('"');
        if let Some(href) = &detail_href {
            result.push_str(" href=\"");
            result.push_str(href);
            result.push('"');
        }
        result.push('>');
        result.push_str(&badge);
        result.push_str("</");
        result.push_str(tag);
        result.push_str(">\n\n");
        // Option lines such as anchors are metadata, not body text.
        result.push_str(&replace_definitions(
            &content[block.text_start..block.body_end],
            namespace,
            current_path,
            plan,
        ));
        if let Some(href) = &detail_href {
            result.push_str("\n<p class=\"habeas-see-evidence\"><a href=\"");
            result.push_str(href);
            result.push_str("\">See evidence</a></p>\n");
        }
        result.push('\n');
        cursor = block.end;
    }

    result.push_str(&content[cursor..]);
    result
}

// {impl}`requirements.references.book_badges`
// {impl}`requirements.definitions.references`
// {impl}`requirements.index.navigation`
// {impl}`requirements.examples.index`
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
            if reference.kind == ReferenceKind::Term {
                if target.kind == DefinitionKind::Term
                    && let Ok((phrase, _)) = terms::declaration(&target.text)
                {
                    let href = chapter_href(
                        current_path,
                        &target.path,
                        Some(&target.kind.anchor(reference.name)),
                    );
                    result.push_str(&format!(
                        "<a href=\"{}\">{}</a>",
                        escape_html(&href),
                        terms::escaped_phrase(phrase)
                    ));
                } else {
                    result.push_str(&content[reference.start..reference.end]);
                }
                cursor = reference.end;
                continue;
            }
            let kind = reference.kind.label();
            let badge = shield_badge(kind, reference.name, reference.kind.color());
            // {impl}`requirements.references.safe_badges`
            let href = escape_html(&chapter_href(
                current_path,
                &target.path,
                Some(&target.kind.anchor(reference.name)),
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
// {impl}`requirements.ci.coverage`
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
// {impl}`requirements.examples.definition`
// {impl}`requirements.definitions.declaration`
// {impl}`requirements.ci.fence_lengths`
// {impl}`requirements.references.anchors`
pub(crate) fn definition_blocks(content: &str) -> Vec<DefinitionBlock> {
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
        let (kind, rest) = if let Some(rest) = suffix.strip_prefix("{req}") {
            (DefinitionKind::Requirement, rest)
        } else if let Some(rest) = suffix.strip_prefix("{example}") {
            (DefinitionKind::Example, rest)
        } else if let Some(rest) = suffix.strip_prefix("{definition}") {
            (DefinitionKind::Term, rest)
        } else {
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
        // {impl}`requirements.references.anchors`
        let mut options = Vec::new();
        let mut text_start = body_start;
        if kind != DefinitionKind::Term {
            let body_lines = close_index.unwrap_or(lines.len());
            for (offset, &(option_start, option_end, option)) in
                lines[index + 1..body_lines].iter().enumerate()
            {
                let Some((option_name, value)) = option_line(option) else {
                    break;
                };
                options.push(DefinitionOption {
                    start: option_start,
                    end: option_end,
                    line: index + offset + 2,
                    name: option_name.to_owned(),
                    value: value.to_owned(),
                });
                text_start = option_end;
            }
        }
        blocks.push(DefinitionBlock {
            start,
            end,
            body_start,
            text_start,
            body_end,
            name: name.to_owned(),
            kind,
            closed,
            options,
        });
        index += 1;
    }
    blocks
}

/// Parse an option line of the form `:name: value`.
// {impl}`requirements.references.anchors`
fn option_line(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim_start_matches(' ').strip_prefix(':')?;
    let (name, value) = rest.split_once(':')?;
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        || !(value.is_empty() || value.starts_with(char::is_whitespace))
    {
        return None;
    }
    Some((name, value.trim()))
}

/// The text of `range` without the option lines of any definition block in it.
// {impl}`requirements.references.anchors`
// {impl}`requirements.ci.requirement_text`
pub(crate) fn without_options(
    content: &str,
    range: Range<usize>,
    blocks: &[DefinitionBlock],
) -> String {
    let mut options = blocks
        .iter()
        .flat_map(|block| &block.options)
        .filter(|option| range.start <= option.start && option.end <= range.end)
        .map(|option| option.start..option.end)
        .collect::<Vec<_>>();
    options.sort_by_key(|option| option.start);
    let mut text = String::with_capacity(range.len());
    let mut cursor = range.start;
    for option in options {
        if option.start >= cursor {
            text.push_str(&content[cursor..option.start]);
            cursor = option.end;
        }
    }
    text.push_str(&content[cursor..range.end]);
    text
}

// {impl}`requirements.ci.span`
// {impl}`requirements.ci.fence_lengths`
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
// {impl}`requirements.definitions.references`
// {impl}`requirements.definitions.declaration`
pub(crate) fn reference_roles(content: &str, markdown: bool) -> Vec<ReferenceMatch<'_>> {
    let protected = if markdown {
        let mut ranges = protected_ranges(content);
        for block in definition_blocks(content)
            .into_iter()
            .filter(|b| b.kind == DefinitionKind::Term)
        {
            let body = &content[block.body_start..block.body_end];
            let option_start = block.body_start + body.len() - body.trim_start().len();
            let option_end = content[option_start..block.body_end]
                .find('\n')
                .map_or(block.body_end, |offset| option_start + offset);
            ranges.push(option_start..option_end);
        }
        ranges
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
        } else if remaining.starts_with("{term}`") {
            (ReferenceKind::Term, "{term}`")
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
// {impl}`requirements.references.targets`
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
        DefinitionKind, chapter_href, chapter_namespace, definition_blocks, preprocess_book,
        reference_roles, replace_definitions, shield_badge,
    };
    use mdbook_preprocessor::book::{Book, BookItem, Chapter};
    use std::path::Path;

    // {test}`requirements.index.detail_pages`
    fn book_root() -> tempfile::TempDir {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(root.path().join("md")).unwrap();
        std::fs::write(root.path().join("book.toml"), "[book]\nsrc = 'md'\n").unwrap();
        root
    }

    // {test}`requirements.references.anchors`
    #[test]
    fn anchor_options_are_metadata_not_body_text() {
        let source = "::::{req} ready\n:impl: skills/ready.md\n:test: tests/ready.sh\nReady.\n\n:::{example} case\n:test: tests/case.sh\nA case.\n:::\n::::\n\n:::{req} plain\nText :impl: inline.\n\n:test: later.md\n:::\n";
        let blocks = definition_blocks(source);
        assert_eq!(blocks.len(), 3);
        let options = |index: usize| {
            blocks[index]
                .options
                .iter()
                .map(|option| (option.name.as_str(), option.value.as_str(), option.line))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            options(0),
            [
                ("impl", "skills/ready.md", 2),
                ("test", "tests/ready.sh", 3)
            ]
        );
        assert_eq!(options(1), [("test", "tests/case.sh", 7)]);
        // Only lines immediately after the opening fence are options.
        assert!(options(2).is_empty());
        assert_eq!(
            super::without_options(source, blocks[0].body_start..blocks[0].body_end, &blocks)
                .trim(),
            "Ready.\n\n:::{example} case\nA case.\n:::"
        );

        let rendered = replace_definitions(source, "requirements", None, None);
        assert!(!rendered.contains(":impl: skills"), "{rendered}");
        assert!(!rendered.contains("skills/ready.md"), "{rendered}");
        assert!(!rendered.contains("tests/case.sh"), "{rendered}");
        assert!(rendered.contains("Text :impl: inline."), "{rendered}");
        assert!(rendered.contains(":test: later.md"), "{rendered}");
    }

    // {test}`requirements.index.detail_pages`
    // {test}`requirements.index.navigation`
    // {test}`requirements.examples.index`
    // {test}`requirements.references.anchors`
    #[test]
    fn every_definition_gets_a_nested_detail_page_without_an_index() {
        let mut book = Book::new();
        let mut chapter = Chapter::new(
            "Rules",
            "# Rules\n\n:::{req} zeta\n:impl: notes.txt\nZeta.\n:::\n\n:::{example} alpha\nAlpha case. CITE`rules.zeta`\n:::\n\n:::{req} cli.mdbook\nMdbook.\n:::\n".replace("CITE", "{cite}"),
            "rules.md",
            Vec::new(),
        );
        chapter.number = Some(mdbook_preprocessor::book::SectionNumber::new(vec![2]));
        chapter.sub_items.push(BookItem::Chapter(Chapter::new(
            "Child",
            "Child.".to_owned(),
            "rules/child.md",
            vec!["Rules".to_owned()],
        )));
        book.push_item(chapter);
        let root = book_root();
        preprocess_book(&mut book, root.path()).unwrap();

        let rules = book.chapters().next().unwrap();
        let pages = rules
            .sub_items
            .iter()
            .filter_map(|item| match item {
                BookItem::Chapter(chapter) => Some(chapter),
                _ => None,
            })
            .collect::<Vec<_>>();
        // Definition order, after existing subchapters, titled by full name.
        assert_eq!(
            pages
                .iter()
                .map(|page| page.name.as_str())
                .collect::<Vec<_>>(),
            ["Child", "rules.zeta", "rules.alpha", "rules.cli.mdbook"]
        );
        assert_eq!(
            pages[1].path.as_deref(),
            Some(Path::new("rules/rules.zeta.md"))
        );
        assert_eq!(pages[1].parent_names, ["Rules"]);
        assert_eq!(
            pages[2].number.as_deref().map(Vec::as_slice),
            Some(&[2, 2][..])
        );
        for (kind, name) in [("req", "zeta"), ("example", "alpha"), ("req", "cli.mdbook")] {
            let href = format!("rules/rules.{name}.html").replace(".md", "%2Emd");
            assert!(
                rules.content.contains(&format!(
                    "id=\"habeas-{kind}-rules.{name}\" href=\"{href}\""
                )),
                "{}",
                rules.content
            );
            assert!(
                rules.content.contains(&format!(
                    "<p class=\"habeas-see-evidence\"><a href=\"{href}\">See evidence</a></p>"
                )),
                "{}",
                rules.content
            );
        }
        assert_eq!(rules.content.matches("See evidence").count(), 3);
        assert!(!rules.content.contains("notes.txt"));
        assert!(pages[1].content.contains("> Zeta.\n"));
        assert!(!pages[1].content.contains(":impl:"));
        assert!(
            pages[1]
                .content
                .contains("[View definition](../rules.html#habeas-req-rules.zeta)")
        );
        assert!(!pages[1].content.contains("Requirements index"));
        assert!(
            pages[3]
                .content
                .contains("[View definition](../rules.html#habeas-req-rules.cli%2Emdbook)")
        );
    }

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

        let rendered = replace_definitions(
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
        let rendered = replace_definitions(source, "requirements", None, None);
        assert!(rendered.contains("{req}`inline`"));
        assert!(rendered.contains(":::{req} block"));
        assert!(rendered.contains("{req}`legacy`"));
        assert!(rendered.contains("alt=\"req requirements.visible\""));
    }

    // {test}`requirements.ci.identifiers`
    // {test}`requirements.references.safe_badges`
    #[test]
    fn invalid_names_remain_literal_and_html_is_escaped() {
        let rendered = replace_definitions(
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

        let root = book_root();
        preprocess_book(&mut book, root.path()).unwrap();
        // Generated detail pages are titled by full name.
        let chapters = book
            .chapters()
            .filter(|chapter| !chapter.name.contains('.'))
            .collect::<Vec<_>>();
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
    // {test}`requirements.references.targets`
    #[test]
    fn markdown_examples_and_escaped_references_stay_literal() {
        let marker = ["{", "impl}`requirements.ready`"].concat();
        let content = format!("`{marker}`\n\n```text\n{marker}\n```\n\n\\{marker}");
        let mut book = Book::new();
        book.push_item(Chapter::new(
            "Requirements",
            ":::{req} ready\nReady.\n:::".to_owned(),
            "requirements.md",
            Vec::new(),
        ));
        book.push_item(Chapter::new("Guide", content, "guide.md", Vec::new()));

        let root = book_root();
        preprocess_book(&mut book, root.path()).unwrap();
        let guide = book
            .chapters()
            .find(|chapter| chapter.name == "Guide")
            .unwrap();
        assert_eq!(guide.content.matches(&marker).count(), 3);
        assert!(!guide.content.contains("habeas-req-ref-impl"));
    }

    // {test}`requirements.references.book_badges`
    // {test}`requirements.references.markdown_literals`
    #[test]
    fn colon_directives_render_markdown_bodies_and_legacy_roles_stay_literal() {
        let source = "An inline {req}`inline` role.\n\n:::{req} defined\nDefinition.\n\n## Details\n\n```text\nexample\n```\n:::\n\nAfter.";
        let rendered = replace_definitions(source, "requirements", None, None);
        assert!(rendered.contains("An inline {req}`inline` role."));
        assert!(!rendered.contains("habeas-req-requirements.inline"));
        assert!(rendered.contains("id=\"habeas-req-requirements.defined\""));
        assert!(rendered.contains("## Details"));
        assert!(rendered.contains("```text\nexample\n```"));
        assert!(rendered.contains("After."));
        assert!(!rendered.contains(":::{req} defined"));
    }

    // {test}`requirements.references.targets`
    // {test}`requirements.references.markdown_literals`
    #[test]
    fn references_in_requirement_bodies_are_scanned_as_markdown() {
        let known = ["{", "cite}`requirements.ready`"].concat();
        let example = ["{", "cite}`missing.example`"].concat();
        let content = format!(":::{{req}} ready\nSee {known}.\n\n```text\n{example}\n```\n:::");
        let references = reference_roles(&content, true);
        assert_eq!(references.len(), 1);
        assert_eq!(references[0].name, "requirements.ready");
    }

    // {test}`requirements.ci.span`
    // {test}`requirements.ci.fence_lengths`
    #[test]
    fn a_longer_colon_fence_contains_a_shorter_nested_directive() {
        let content = "::::{req} ready\nIntro.\n\n:::{note}\nNested.\n:::\n::::\n\nOutside.";
        let blocks = definition_blocks(content);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].closed);
        assert_eq!(blocks[0].kind, DefinitionKind::Requirement);
        assert_eq!(
            &content[blocks[0].body_start..blocks[0].body_end],
            "Intro.\n\n:::{note}\nNested.\n:::\n"
        );
        assert!(content[blocks[0].end..].contains("Outside."));
    }

    // {test}`requirements.examples.definition`
    // {test}`requirements.references.book_badges`
    // {test}`requirements.ci.identifiers`
    #[test]
    fn examples_are_distinct_definitions_and_tests_link_to_them() {
        let source = "::::{req} ready\nReady.\n\n:::{example} something.foo\nGiven ready, it succeeds. CITE`requirements.ready`\n:::\n::::\n\n:::{example} bad name\nInvalid.\n:::\n\n```text\n:::{example} hidden\nLiteral.\n:::\n```"
            .replace("CITE", "{cite}");
        let blocks = definition_blocks(&source);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].kind, DefinitionKind::Requirement);
        assert_eq!(blocks[1].kind, DefinitionKind::Example);

        let mut book = Book::new();
        book.push_item(Chapter::new(
            "Requirements",
            source,
            "requirements.md",
            Vec::new(),
        ));
        book.push_item(Chapter::new(
            "Guide",
            "TEST`requirements.something.foo`".replace("TEST", "{test}"),
            "guides/deep.md",
            Vec::new(),
        ));
        let root = book_root();
        preprocess_book(&mut book, root.path()).unwrap();
        // Generated detail pages are titled by full name.
        let chapters = book
            .chapters()
            .filter(|chapter| !chapter.name.contains('.'))
            .collect::<Vec<_>>();
        assert!(
            chapters[0]
                .content
                .contains("id=\"habeas-req-requirements.ready\"")
        );
        assert!(
            chapters[0]
                .content
                .contains("id=\"habeas-example-requirements.something.foo\"")
        );
        assert!(
            chapters[0]
                .content
                .contains("alt=\"example requirements.something.foo\"")
        );
        assert!(chapters[0].content.contains(":::{example} bad name"));
        assert!(chapters[0].content.contains(":::{example} hidden"));
        assert!(
            chapters[1].content.contains(
                "href=\"../requirements.html#habeas-example-requirements.something.foo\""
            )
        );
    }
}

mod commands;
mod markdown;
pub use commands::{
    info_markdown, status_markdown, status_markdown_filtered, status_markdown_with_options,
};

use std::collections::BTreeMap;
use std::io;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use mdbook_preprocessor::book::Book;
use pulldown_cmark::{Event, Parser, Tag};

pub mod authoring;
mod builtins;
mod citations;
pub mod config;
mod definition_glob;
mod evidence;
mod fenced;
mod fixme;
mod github;
mod index;
mod inputs;
pub mod judgment;
mod outline;
mod redaction;
mod refactor;
mod rust_coverage;
mod sites;
mod structural;
mod terms;

pub use evidence::{EvidenceReport, reference_set_digest_for, validate_repository};
pub use github::GithubIssues;
pub use inputs::{outline_input, outline_inputs_for_lines};
pub use refactor::{RefactorError, refactor};

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
    // {impl}`requirements.examples.evidence`
    pub(crate) fn noun(self) -> &'static str {
        match self {
            Self::Requirement => "requirement",
            Self::Example => "example",
            Self::Term => "definition",
        }
    }

    // {impl}`requirements.examples.index`
    pub(crate) fn badge(self) -> &'static str {
        match self {
            Self::Requirement => "req",
            Self::Example => "example",
            Self::Term => "definition",
        }
    }

    // {impl}`requirements.examples.index`
    // {impl}`requirements.index.navigation`
    pub(crate) fn anchor(self, name: &str) -> String {
        format!("habeas-{}-{name}", self.badge())
    }
}

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
pub(crate) struct DefinitionOption {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) line: usize,
    pub(crate) name: String,
    pub(crate) value: String,
}

// {cite}`habeas:util`
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
    /// An acknowledged gap; only the fenced form is valid, and an unnamed
    /// fence ties its place to the issues it names without being an edge.
    Fixme,
}

// {impl}`requirements.references.book_badges`
impl ReferenceKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Impl => "impl",
            Self::Cite => "cite",
            Self::Test => "test",
            Self::Term => "term",
            Self::Fixme => "fixme",
        }
    }

    fn color(self) -> &'static str {
        match self {
            Self::Impl => "237346",
            Self::Cite => "7151a1",
            Self::Test => "ae5d14",
            Self::Term => "006ba8",
            Self::Fixme => "b60205",
        }
    }
}

pub(crate) struct ReferenceMatch<'a> {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) name: &'a str,
    pub(crate) kind: ReferenceKind,
}

// {impl}`requirements.examples.definition`
// {impl}`requirements.definitions.declaration`
// {impl}`requirements.definitions.references`
// {impl}`requirements.definitions.selection`
// {impl}`requirements.index.detail_pages`
// {impl}`requirements.references.anchors`
// {impl}`requirements.ci.identifiers`
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
            // A directive whose full name is invalid, such as one in a
            // chapter whose path gives an invalid namespace, defines nothing.
            if !valid_name(&name) || definitions.contains_key(&name) {
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

fn chapter_namespace(path: &Path) -> String {
    let path = path.with_extension("");
    let mut parts: Vec<_> = path
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();

    let nested = parts.len() > 1;
    if nested && parts[0] == "reqs" {
        parts.remove(0);
    }
    if nested && matches!(parts.last().map(String::as_str), Some("index" | "README")) {
        parts.pop();
    }

    parts.join(".")
}

// {impl}`requirements.index.navigation`
// {impl}`requirements.references.book_badges`
// {impl}`requirements.examples.definition`
// {impl}`requirements.examples.index`
fn replace_definitions(
    content: &str,
    namespace: &str,
    current_path: Option<&Path>,
    plan: Option<&index::DetailPlan>,
) -> String {
    let mut result = String::with_capacity(content.len());
    let mut cursor = 0;

    // A directive whose full name is invalid remains literal.
    for block in definition_blocks(content)
        .into_iter()
        .filter(|block| block.closed && valid_name(&full_requirement_name(namespace, &block.name)))
    {
        if block.start < cursor {
            continue;
        }
        result.push_str(&content[cursor..block.start]);
        let full_name = full_requirement_name(namespace, &block.name);
        // Valid names contain no HTML-special characters, so this escaping
        // only guards the inserted HTML against future name rules.
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
        let body = &content[block.text_start..block.body_end];
        let body = if block.kind == DefinitionKind::Requirement {
            replace_query_directives(body)
        } else {
            body.to_owned()
        };
        result.push_str(&replace_definitions(&body, namespace, current_path, plan));
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

/// Renders each query directive in a requirement body as a badge naming its
/// kind and name, followed by its code block. Directives in code blocks
/// stay literal.
// {impl}`requirements.references.distributed`
fn replace_query_directives(body: &str) -> String {
    let code = code_block_ranges(body);
    let mut result = String::with_capacity(body.len());
    let mut closing: Option<String> = None;
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let text = line.trim_end_matches(['\n', '\r']);
        if code.iter().any(|range| range.contains(&start)) {
            result.push_str(line);
            continue;
        }
        if closing.as_deref() == Some(text.trim_end()) {
            closing = None;
            continue;
        }
        if closing.is_none()
            && let Some((fence, kind, name)) = structural::query_fence(text)
        {
            result.push_str("<span class=\"habeas-query-badge\">");
            result.push_str(&shield_badge(kind.label(), name, "6c4f8a"));
            result.push_str("</span>\n\n");
            closing = Some(fence.to_owned());
            continue;
        }
        result.push_str(line);
    }
    result
}

/// Renders fenced references as their badge followed by their body, and
/// inline references as badges. A fenced reference inside a definition needs
/// a longer outer fence, so its closing fence does not end the definition.
// {impl}`requirements.references.book_badges`
// {impl}`requirements.references.builtin_definitions`
fn replace_references(
    content: &str,
    current_path: &Path,
    definitions: &BTreeMap<String, Definition>,
) -> String {
    let mut result = String::with_capacity(content.len());
    let mut cursor = 0;
    for fence in fenced::markdown_fences(content, true) {
        if fence.start < cursor || !fence.closed {
            continue;
        }
        let body = &content[fence.body.clone()];
        let Some(badge) = reference_badge(fence.kind, &fence.name, current_path, definitions)
        else {
            continue;
        };
        result.push_str(&replace_inline_references(
            &content[cursor..fence.start],
            current_path,
            definitions,
        ));
        result.push_str("<div class=\"habeas-fenced-ref\">\n\n<p>");
        result.push_str(&badge);
        result.push_str("</p>\n\n");
        result.push_str(&replace_references(body, current_path, definitions));
        result.push_str("\n\n</div>\n\n");
        cursor = fence.end;
    }
    result.push_str(&replace_inline_references(
        &content[cursor..],
        current_path,
        definitions,
    ));
    result
}

/// The badge for a valid `{impl}`, `{cite}`, or `{test}` reference: a link to
/// a project definition, or an unlinked badge whose hover text gives a
/// built-in's text. Returns `None` for a reference that must remain literal.
// {impl}`requirements.references.safe_badges`
// {impl}`requirements.index.navigation`
// {impl}`requirements.examples.index`
fn reference_badge(
    kind: ReferenceKind,
    name: &str,
    current_path: &Path,
    definitions: &BTreeMap<String, Definition>,
) -> Option<String> {
    // A `{fixme}` has no badge; a named one is invalid in the book and an
    // unnamed one is not a reference.
    if kind == ReferenceKind::Fixme {
        return None;
    }
    let label = kind.label();
    let badge = shield_badge(label, name, kind.color());
    if let Some(builtin) = builtins::builtin(name) {
        builtin.check(kind).ok()?;
        return Some(format!(
            "<span class=\"habeas-req-badge habeas-req-ref-{label} habeas-builtin\" title=\"{}\">{badge}</span>",
            escape_html(builtin.text())
        ));
    }
    let target = definitions.get(name)?;
    if target.kind == DefinitionKind::Term {
        return None;
    }
    let href = escape_html(&chapter_href(
        current_path,
        &target.path,
        Some(&target.kind.anchor(name)),
    ));
    Some(format!(
        "<a class=\"habeas-req-badge habeas-req-ref-{label}\" href=\"{href}\">{badge}</a>"
    ))
}

// {impl}`requirements.index.navigation`
// {impl}`requirements.examples.index`
fn replace_inline_references(
    content: &str,
    current_path: &Path,
    definitions: &BTreeMap<String, Definition>,
) -> String {
    let mut result = String::with_capacity(content.len());
    let mut cursor = 0;

    for reference in reference_roles(content, true) {
        result.push_str(&content[cursor..reference.start]);
        if reference.kind == ReferenceKind::Term {
            match definitions.get(reference.name) {
                Some(target)
                    if target.kind == DefinitionKind::Term
                        && let Ok((phrase, _)) = terms::declaration(&target.text) =>
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
                }
                _ => result.push_str(&content[reference.start..reference.end]),
            }
        } else if let Some(badge) =
            reference_badge(reference.kind, reference.name, current_path, definitions)
        {
            result.push_str(&badge);
        } else {
            result.push_str(&content[reference.start..reference.end]);
        }
        cursor = reference.end;
    }

    result.push_str(&content[cursor..]);
    result
}

// {impl}`requirements.index.detail_format`
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
// Relative chapter links, including `../` from nested chapters.
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

fn full_requirement_name(namespace: &str, name: &str) -> String {
    if namespace.is_empty() {
        name.to_owned()
    } else {
        format!("{namespace}.{name}")
    }
}

// {impl}`requirements.examples.definition`
// {impl}`requirements.ci.fence_lengths`
pub(crate) fn definition_blocks(content: &str) -> Vec<DefinitionBlock> {
    let code_ranges = code_block_ranges(content);
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
        // A reserved built-in name is recognized so validation can report it.
        if overlaps_protected(start, body_start, &code_ranges)
            || !rest.chars().next().is_some_and(char::is_whitespace)
            || !(valid_name(rest.trim()) || builtins::is_builtin_name(rest.trim()))
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
pub(crate) fn option_line(line: &str) -> Option<(&str, &str)> {
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

/// The byte ranges of a Markdown text's code blocks, whose contents stay
/// literal.
pub(crate) fn code_block_ranges(content: &str) -> Vec<Range<usize>> {
    Parser::new(content)
        .into_offset_iter()
        .filter_map(|(event, range)| {
            matches!(event, Event::Start(Tag::CodeBlock(_))).then_some(range)
        })
        .collect()
}

fn colon_fence(line: &str) -> Option<(&str, usize)> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let length = trimmed.bytes().take_while(|byte| *byte == b':').count();
    (length >= 3).then_some((&trimmed[length..], length))
}

fn overlaps_protected(start: usize, end: usize, ranges: &[Range<usize>]) -> bool {
    ranges
        .iter()
        .any(|range| start < range.end && range.start < end)
}

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
        } else if remaining.starts_with("{fixme}`") {
            // Recognized so validation can report the inline form.
            (ReferenceKind::Fixme, "{fixme}`")
        } else {
            continue;
        };
        let name_start = start + prefix.len();
        let Some(name_end_offset) = content[name_start..].find('`') else {
            continue;
        };
        let name_end = name_start + name_end_offset;
        let name = &content[name_start..name_end];
        let builtin = kind != ReferenceKind::Term && builtins::is_builtin_name(name);
        if valid_name(name) || builtin {
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

/// Every reference in text, inline or fenced, as its role and name, in
/// source order.
pub(crate) fn text_references(content: &str, markdown: bool) -> Vec<(ReferenceKind, String)> {
    let mut references = reference_roles(content, markdown)
        .into_iter()
        .map(|reference| (reference.start, reference.kind, reference.name.to_owned()))
        .collect::<Vec<_>>();
    references.extend(
        fenced::markdown_fences(content, markdown)
            .into_iter()
            .map(|fence| (fence.start, fence.kind, fence.name)),
    );
    references.sort_by_key(|(start, _, _)| *start);
    references
        .into_iter()
        .map(|(_, kind, name)| (kind, name))
        .collect()
}

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

fn is_protected(position: usize, ranges: &[Range<usize>]) -> bool {
    ranges
        .iter()
        .any(|range| range.start <= position && position < range.end)
}

fn is_escaped(content: &str, position: usize) -> bool {
    content[..position]
        .bytes()
        .rev()
        .take_while(|byte| *byte == b'\\')
        .count()
        % 2
        == 1
}

pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        })
}

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

    // {cite}`habeas:util`
    fn book_root() -> tempfile::TempDir {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(root.path().join("md")).unwrap();
        std::fs::write(root.path().join("book.toml"), "[book]\nsrc = 'md'\n").unwrap();
        root
    }

    // {test}`requirements.references.book_badges`
    // {test}`requirements.references.builtin_definitions`
    #[test]
    fn fenced_references_render_their_badge_and_body_and_built_ins_are_unlinked() {
        let mut book = Book::new();
        let content = [
            "::::{req} ready\nReady MUST hold.\n\n:::{cite} rules.other\nThe *other* rule.\n:::\n::::\n\n",
            ":::{req} other\nOther.\n:::\n\n",
            ":::{cite} habeas:util\nServes its caller.\n:::\n\n",
            ":::{cite} habeas:none\nRemoved.\n:::\n\n",
            "Inline CITE`habeas:none` stays literal.\n\n",
            "Inline CITE`habeas:util` is a badge.\n\n",
            ":::{cite} rules.missing\nUnknown.\n:::\n\n",
            "```text\n:::{cite} rules.other\n:::\n```\n",
        ]
        .concat()
        .replace("CITE", "{cite}");
        book.push_item(Chapter::new("Rules", content, "rules.md", Vec::new()));
        let root = book_root();
        preprocess_book(&mut book, root.path()).unwrap();
        let rendered = book.chapters().next().unwrap().content.clone();

        // The project reference links to its definition, then its body.
        let linked = "<div class=\"habeas-fenced-ref\">\n\n<p><a class=\"habeas-req-badge habeas-req-ref-cite\" href=\"rules.html#habeas-req-rules.other\">";
        let start = rendered.find(linked).expect(&rendered);
        let after = &rendered[start..];
        assert!(
            after.find("alt=\"cite rules.other\"").unwrap()
                < after.find("The *other* rule.").unwrap(),
            "{rendered}"
        );
        assert!(
            !rendered.contains(":::{cite} rules.other\nThe"),
            "{rendered}"
        );
        // The outer requirement still contains it and closes at its own fence.
        assert!(rendered.contains("See evidence"), "{rendered}");

        // A built-in has no link; its hover text is its full text.
        let util = crate::builtins::builtin("habeas:util").unwrap();
        assert!(
            rendered.contains(&format!(
                "<span class=\"habeas-req-badge habeas-req-ref-cite habeas-builtin\" title=\"{}\"><img",
                super::escape_html(util.text())
            )),
            "{rendered}"
        );
        let badge = "<p><span class=\"habeas-req-badge habeas-req-ref-cite habeas-builtin\"";
        let fenced = rendered.find(badge).expect(&rendered);
        assert!(
            rendered[fenced..].find("Serves its caller.").is_some(),
            "{rendered}"
        );
        assert!(!rendered.contains("href=\"habeas"), "{rendered}");
        // Invalid forms, unknown names, and code examples remain literal.
        assert!(
            rendered.contains(":::{cite} habeas:none\nRemoved.\n:::"),
            "{rendered}"
        );
        assert!(
            rendered.contains(&"Inline CITE`habeas:none` stays literal.".replace("CITE", "{cite}"))
        );
        assert!(
            rendered.contains(":::{cite} rules.missing\nUnknown.\n:::"),
            "{rendered}"
        );
        assert!(
            rendered.contains("```text\n:::{cite} rules.other\n:::\n```"),
            "{rendered}"
        );
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
        assert_eq!(chapter_namespace(Path::new("foo/index.md")), "foo");
        assert_eq!(chapter_namespace(Path::new("reqs/index.md")), "");
        assert_eq!(chapter_namespace(Path::new("reqs/bar.md")), "bar");
        // Only nested paths drop these segments.
        assert_eq!(chapter_namespace(Path::new("index.md")), "index");
        assert_eq!(chapter_namespace(Path::new("reqs.md")), "reqs");
        assert_eq!(
            chapter_namespace(Path::new("other/reqs/a.md")),
            "other.reqs.a"
        );

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
    #[test]
    fn invalid_names_remain_literal() {
        let source = ":::{req} bad name\nInvalid.\n:::\n\n:::{req} good\nGood.\n:::";
        let rendered = replace_definitions(source, "rules", None, None);
        assert!(rendered.contains(":::{req} bad name"));
        assert!(rendered.contains("alt=\"req rules.good\""));
        // A valid local name in a chapter whose namespace is invalid gives an
        // invalid full name, so the directive stays literal too.
        let rendered = replace_definitions(source, "a&b", None, None);
        assert_eq!(rendered, source);

        for name in [
            "",
            "a..b",
            ".a",
            "a.",
            "a b",
            "caf\u{e9}",
            "a/b",
            "a&b",
            "a`b",
        ] {
            assert!(!super::valid_name(name), "{name:?} is invalid");
        }
        for name in ["a", "a_b.c-d", "A1.b2", "x.-_.y"] {
            assert!(super::valid_name(name), "{name:?} is valid");
        }
        for role in ["impl", "cite", "test", "term"] {
            for name in ["bad name", "a..b", "", "a."] {
                let text = format!("{{{role}}}`{name}`");
                assert!(reference_roles(&text, false).is_empty(), "{text}");
                assert!(reference_roles(&text, true).is_empty(), "{text}");
            }
            assert_eq!(
                reference_roles(&format!("{{{role}}}`a_b.c-d`"), false).len(),
                1
            );
        }
        for opener in [":::{req} a..b", ":::{example} .x", ":::{definition} a b"] {
            let source = format!("{opener}\n:phrase: p\n\nBody.\n:::\n");
            assert!(definition_blocks(&source).is_empty(), "{opener}");
        }
    }

    // {test}`requirements.ci.identifiers`
    #[test]
    fn chapters_with_invalid_namespaces_define_nothing_in_the_book() {
        let mut book = Book::new();
        book.push_item(Chapter::new(
            "Bad",
            ":::{req} good\nGood.\n:::".to_owned(),
            "bad dir/rules.md",
            Vec::new(),
        ));
        book.push_item(Chapter::new(
            "Guide",
            "See {impl}`bad dir.rules.good`.".to_owned(),
            "guide.md",
            Vec::new(),
        ));
        let root = book_root();
        preprocess_book(&mut book, root.path()).unwrap();
        let chapters = book.chapters().collect::<Vec<_>>();
        assert_eq!(chapters.len(), 2, "no detail page is generated");
        assert_eq!(chapters[0].content, ":::{req} good\nGood.\n:::");
        assert!(!chapters[0].content.contains("habeas-req-badge"));
        assert_eq!(chapters[1].content, "See {impl}`bad dir.rules.good`.");
    }

    // {test}`requirements.references.safe_badges`
    #[test]
    fn badge_urls_percent_encode_punctuation_and_html_is_escaped() {
        let badge = shield_badge("a/b?c", "x#y%z&w<v>\"u' caf\u{e9}", "006ba8");
        let url = badge
            .split("src=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap();
        assert_eq!(
            url,
            "https://img.shields.io/badge/a%2Fb%3Fc-x%23y%25z%26w%3Cv%3E%22u%27_caf%C3%A9-006ba8?style=flat-square&amp;labelColor=555"
        );
        assert!(
            badge.contains("alt=\"a/b?c x#y%z&amp;w&lt;v&gt;&quot;u&#39; caf\u{e9}\""),
            "{badge}"
        );
    }

    // {test}`requirements.references.safe_badges`
    #[test]
    fn reference_link_targets_are_escaped() {
        let mut book = Book::new();
        let mut target = Chapter::new(
            "Rules",
            ":::{req} ready\nReady.\n:::".to_owned(),
            "rules.md",
            Vec::new(),
        );
        // The rendered path, unlike the source path, may contain characters
        // that are special in HTML.
        target.path = Some("a&b \"q\" <x>.md".into());
        book.push_item(target);
        book.push_item(Chapter::new(
            "Guide",
            concat!("See {cite}", "`rules.ready`.").to_owned(),
            "guide.md",
            Vec::new(),
        ));
        let root = book_root();
        preprocess_book(&mut book, root.path()).unwrap();
        let guide = book
            .chapters()
            .find(|chapter| chapter.name == "Guide")
            .unwrap();
        assert!(
            guide
                .content
                .contains("href=\"a&amp;b &quot;q&quot; &lt;x&gt;.html#habeas-req-rules.ready\""),
            "{}",
            guide.content
        );
        assert!(!guide.content.contains("\"q\""), "{}", guide.content);
        assert!(!guide.content.contains("<x>"), "{}", guide.content);
    }

    /// The shipped stylesheet's rules, as selector lists and declarations.
    fn stylesheet_rules() -> Vec<(Vec<String>, Vec<String>)> {
        include_str!("../md/habeas.css")
            .split('}')
            .filter(|rule| !rule.trim().is_empty())
            .map(|rule| {
                let (selectors, body) = rule.split_once('{').unwrap();
                let selectors = selectors
                    .split(',')
                    .map(|selector| selector.trim().to_owned())
                    .collect();
                let declarations = body
                    .split(';')
                    .map(|declaration| declaration.split_whitespace().collect::<Vec<_>>().join(" "))
                    .filter(|declaration| !declaration.is_empty())
                    .collect();
                (selectors, declarations)
            })
            .collect()
    }

    // {test}`requirements.references.book_badges`
    #[test]
    fn badge_stylesheet_keeps_visited_links_legible_and_shows_focus() {
        let rules = stylesheet_rules();
        let declarations = |selector: &str| {
            rules
                .iter()
                .filter(|(selectors, _)| selectors.iter().any(|candidate| candidate == selector))
                .flat_map(|(_, declarations)| declarations.iter().map(String::as_str))
                .collect::<Vec<_>>()
        };
        // A visited badge link looks exactly like an unvisited one.
        assert_eq!(
            declarations("a.habeas-req-badge:visited"),
            declarations("a.habeas-req-badge")
        );
        assert!(declarations("a.habeas-req-badge").contains(&"color: inherit"));
        // Keyboard focus shows an outline.
        let focus = declarations("a.habeas-req-badge:focus-visible");
        assert!(
            focus
                .iter()
                .any(|declaration| declaration.starts_with("outline:")
                    && !declaration.contains("none")
                    && !declaration.contains(" 0 ")),
            "{focus:?}"
        );
        // Hover only brightens.
        assert_eq!(
            declarations("a.habeas-req-badge:hover"),
            ["filter: brightness(1.08)"]
        );
        // Every rule styles a Habeas badge or fenced reference; nothing else in
        // the book is restyled.
        let specified = [
            ".habeas-req-badge",
            ".habeas-shield",
            "a.habeas-req-badge",
            "a.habeas-req-badge:visited",
            "a.habeas-req-badge:hover",
            "a.habeas-req-badge:focus-visible",
            ".habeas-builtin",
            ".habeas-fenced-ref",
        ];
        for (selectors, _) in &rules {
            for selector in selectors {
                assert!(specified.contains(&selector.as_str()), "{selector}");
            }
        }
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
    // {test}`requirements.index.navigation`
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
    // {test}`requirements.references.discovery`
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
        // An escaped backslash does not escape the role.
        let content = format!("\\\\{marker} \\\\\\{marker}");
        let roles = reference_roles(&content, true);
        assert_eq!(roles.len(), 1);
        assert_eq!(roles[0].start, 2);
    }

    // {test}`requirements.references.markdown_literals`
    #[test]
    fn roles_in_raw_html_stay_literal() {
        let marker = ["{", "impl}`requirements.ready`"].concat();
        let content = format!(
            "<div>\n{marker}\n</div>\n\n<!-- {marker} -->\n\nText <!-- {marker} --> and <span title=\"{marker}\">tag</span>.\n\nBetween <em>{marker}</em> tags.\n"
        );
        let references = reference_roles(&content, true);
        // Only the role between inline tags is ordinary Markdown text.
        assert_eq!(references.len(), 1, "{content}");
        assert!(content[references[0].start..].starts_with(&format!("{marker}</em>")));
        // Outside Markdown, HTML-looking text does not protect roles.
        assert_eq!(reference_roles(&content, false).len(), 5);
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

    // {test}`requirements.references.distributed`
    #[test]
    fn query_directives_render_as_badges_followed_by_their_code_blocks() {
        let source = "::::{req} spread\nEach site MUST hold.\n\n:::{sites} checks\n```yaml\nlanguage: rust\n```\n:::\n\n:::{violations} risky\n```yaml\nwithin: checks\n```\n:::\n\n```text\n:::{sites} literal\n```\n::::\n";
        let rendered = replace_definitions(source, "requirements", None, None);
        assert!(
            rendered.contains("<span class=\"habeas-query-badge\"><img class=\"habeas-shield\" src=\"https://img.shields.io/badge/sites-checks-6c4f8a?style=flat-square&amp;labelColor=555\" alt=\"sites checks\" loading=\"lazy\"></span>\n\n```yaml\nlanguage: rust\n```\n\n<span class=\"habeas-query-badge\">"),
            "{rendered}"
        );
        assert!(rendered.contains("alt=\"violations risky\""), "{rendered}");
        assert!(!rendered.contains(":::{sites} checks"), "{rendered}");
        assert!(!rendered.contains("\n:::\n"), "{rendered}");
        assert!(
            rendered.contains("```text\n:::{sites} literal\n```"),
            "{rendered}"
        );
    }

    // {test}`requirements.references.discovery`
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

    // {test}`requirements.ci.span`
    #[test]
    fn closing_fences_allow_no_trailing_text_and_fences_allow_three_spaces_of_indent() {
        let content = ":::{req} ready\nBody.\n:::foo\nStill body.\n::: \n\nAfter.";
        let blocks = definition_blocks(content);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].closed);
        assert_eq!(
            &content[blocks[0].body_start..blocks[0].body_end],
            "Body.\n:::foo\nStill body.\n",
            "a colon line with trailing text does not close the body; trailing whitespace may"
        );

        let indented = "   :::{req} ready\nBody.\n   :::\n";
        let blocks = definition_blocks(indented);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].closed);
        assert_eq!(
            &indented[blocks[0].body_start..blocks[0].body_end],
            "Body.\n"
        );
        assert!(definition_blocks("    :::{req} ready\nBody.\n:::\n").is_empty());
        let unclosed = definition_blocks(":::{req} ready\nBody.\n    :::\n");
        assert_eq!(unclosed.len(), 1);
        assert!(
            !unclosed[0].closed,
            "a four-space-indented fence does not close"
        );
        assert!(definition_blocks(":::{req}ready\nBody.\n:::\n").is_empty());
        // An opening fence needs at least three colons immediately followed
        // by `{req}`.
        assert!(definition_blocks("::{req} ready\nBody.\n::\n").is_empty());
        assert!(definition_blocks("::: {req} ready\nBody.\n:::\n").is_empty());
    }

    // {test}`requirements.examples.definition`
    // {test}`requirements.references.book_badges`
    // {test}`requirements.ci.identifiers`
    // {test}`requirements.examples.index`
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
    info_markdown, status_markdown, status_markdown_filtered, status_markdown_with_github,
    status_markdown_with_options,
};

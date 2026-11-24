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
pub mod execution_coverage;
mod fenced;
mod fixme;
mod github;
mod index;
mod inputs;
pub mod judgment;
mod outline {
    pub(crate) use trust_but_verify::outline_engine::LegacyOutline as Outline;
    pub(crate) use trust_but_verify::outline_engine::ResolveError;
    pub(crate) use trust_but_verify::outline_engine::comment_ranges;
    pub(crate) use trust_but_verify::outline_engine::parse;
    pub(crate) use trust_but_verify::outline_engine::parse_redacted;
    pub(crate) use trust_but_verify::outline_engine::profile::profile_for_path;

    pub(crate) fn uses_language(
        path: &std::path::Path,
        language: impl trust_but_verify::outline::OutlineLang,
    ) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                language
                    .extensions()
                    .iter()
                    .any(|candidate| ext.eq_ignore_ascii_case(candidate))
            })
    }

    pub(crate) fn is_markdown_path(path: &std::path::Path) -> bool {
        uses_language(path, trust_but_verify::outline::languages::Markdown)
    }
}
mod evidence_format;
mod outline_items;
mod redaction;
mod refactor;
mod rust_coverage;
pub mod rustdocs;
mod sdk;
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

// {cite}`habeas:util`
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

// {cite}`habeas:util`
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

// {cite}`habeas:util`
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
    // {cite}`habeas:util`
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Impl => "impl",
            Self::Cite => "cite",
            Self::Test => "test",
            Self::Term => "term",
            Self::Fixme => "fixme",
        }
    }

    // {cite}`habeas:util`
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

// {cite}`habeas:util`
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
// {impl}`requirements.references.builtin_definitions`
// {impl}`requirements.index.render_only`
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
    execution_coverage::populate(book, root, plan.index_path())?;

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
        let content = chapter.path.as_deref().map_or_else(
            || chapter.content.clone(),
            |path| {
                replace_reviewed_citation_passages(&chapter.content, &namespace, path, &definitions)
            },
        );
        let content = omit_fenced_citation_paragraphs(&content, &namespace, &definitions);
        chapter.content =
            replace_definitions(&content, &namespace, chapter.path.as_deref(), Some(&plan));
        if let Some(path) = chapter.path.as_deref() {
            chapter.content = replace_references(&chapter.content, path, &definitions);
        }
    });
    Ok(())
}

/// Render review annotations separately from their prose. Collect from the
/// original source, including metadata later omitted from chapter presentation.
// {impl}`requirements.references.citation_review.book_commentary.display`
fn replace_reviewed_citation_passages(
    content: &str,
    namespace: &str,
    current_path: &Path,
    definitions: &BTreeMap<String, Definition>,
) -> String {
    let declarations = definition_blocks(content);
    let in_declaration = |offset| {
        declarations.iter().any(|block| {
            block.kind != DefinitionKind::Term && block.start < offset && offset < block.body_end
        })
    };
    let fences = fenced::markdown_fences(content, true);
    let mut citations = reference_roles(content, true)
        .into_iter()
        .filter(|reference| reference.kind == ReferenceKind::Cite)
        .map(|reference| (reference.start, reference.name.to_owned()))
        .collect::<Vec<_>>();
    citations.extend(
        fences
            .iter()
            .filter(|fence| fence.closed && fence.kind == ReferenceKind::Cite)
            .map(|fence| (fence.start, fence.name.clone())),
    );
    citations.sort_by_key(|(offset, _)| *offset);

    let mut result = String::with_capacity(content.len());
    let mut cursor = 0;
    for fence in &fences {
        if !fence.closed
            || fence.kind != ReferenceKind::Cite
            || fence.start < cursor
            || in_declaration(fence.start)
        {
            continue;
        }
        let mut names = Vec::new();
        let mut badges = Vec::new();
        for (offset, name) in &citations {
            if *offset < fence.start || *offset >= fence.end || in_declaration(*offset) {
                continue;
            }
            if !names.contains(name)
                && let Some(badge) =
                    reference_badge(ReferenceKind::Cite, name, current_path, definitions)
            {
                names.push(name.clone());
                badges.push(badge);
            }
        }
        if !names.iter().any(|name| name != "habeas:util") {
            continue;
        }

        // Keep the original annotation list, but omit citation-only metadata
        // from the visible prose using the same rules as other fenced sections.
        let fragment = omit_fenced_citation_paragraphs(
            &content[fence.start..fence.end],
            namespace,
            definitions,
        );
        let fragment_declarations = definition_blocks(&fragment);
        let fragment_fences = fenced::markdown_fences(&fragment, true);
        let mut removed = Vec::new();
        for nested in &fragment_fences {
            if !nested.closed
                || nested.kind != ReferenceKind::Cite
                || fragment_declarations.iter().any(|block| {
                    block.kind != DefinitionKind::Term
                        && block.start < nested.start
                        && nested.start < block.body_end
                })
                || (nested.start != 0
                    && reference_badge(
                        ReferenceKind::Cite,
                        &nested.name,
                        current_path,
                        definitions,
                    )
                    .is_none())
            {
                continue;
            }
            removed.push(nested.start..nested.body.start);
            removed.push(nested.body.end..nested.end);
        }
        removed.sort_by_key(|range| range.start);
        let mut body = String::new();
        let mut position = 0;
        for range in removed {
            if position < range.start {
                body.push_str(&fragment[position..range.start]);
            }
            position = position.max(range.end);
        }
        body.push_str(&fragment[position..]);

        result.push_str(&content[cursor..fence.start]);
        result.push_str(
            "<div class=\"habeas-citation-passage\">\n<details class=\"habeas-citation-list\">\n<summary aria-label=\"Show citations for this passage\" title=\"Show citations\"><span aria-hidden=\"true\">※</span></summary>\n<ul>\n",
        );
        for badge in badges {
            result.push_str(&format!("<li>{badge}</li>\n"));
        }
        result.push_str("</ul>\n</details>\n\n");
        result.push_str(&body);
        result.push_str("\n\n</div>\n\n");
        cursor = fence.end;
    }
    result.push_str(&content[cursor..]);
    result
}

/// Hide citation metadata only in authored chapter presentation. Definitions,
/// detail pages, discovery, and authoring keep the original source.
// {impl}`requirements.references.book_badges`
fn omit_fenced_citation_paragraphs(
    content: &str,
    namespace: &str,
    definitions: &BTreeMap<String, Definition>,
) -> String {
    use trust_but_verify::outline::{OutlineLang, languages::Markdown};

    if !content.contains("{cite}`") || !content.contains(":::") {
        return content.to_owned();
    }
    // Reuse the SDK's Markdown grammar for nesting, literal contexts, and
    // unclosed containers instead of recognizing another set of fence rules.
    let Ok(outline) = Markdown.outline(content) else {
        return content.to_owned();
    };
    let blocks = definition_blocks(content);
    let mut literal_definitions = blocks
        .iter()
        .filter(|block| {
            !block.closed || !valid_name(&full_requirement_name(namespace, &block.name))
        })
        .map(|block| block.start..block.end)
        .collect::<Vec<_>>();
    let mut masked = content.as_bytes().to_vec();
    let mut bodies = Vec::new();
    for block in &blocks {
        for option in &block.options {
            for byte in &mut masked[option.start..option.end] {
                if !matches!(*byte, b'\r' | b'\n') {
                    *byte = b' ';
                }
            }
        }
        if block.kind == DefinitionKind::Term {
            let body = &content[block.body_start..block.body_end];
            if terms::declaration(body).is_ok() {
                let start = block.body_start + body.len() - body.trim_start().len();
                let end = content[start..block.body_end]
                    .find('\n')
                    .map_or(block.body_end, |offset| start + offset);
                masked[start..end].fill(b' ');
            }
        }
    }
    for node in &outline.items {
        if node.kind != "block" {
            continue;
        }
        let source = &content[node.span.clone()];
        let opening = source.lines().next().unwrap_or_default();
        let Some((suffix, length)) = colon_fence(opening) else {
            continue;
        };
        if suffix.trim().is_empty() {
            continue;
        }
        if let Some(name) = ["{req}", "{example}", "{definition}"]
            .iter()
            .find_map(|kind| suffix.strip_prefix(kind))
            && (!name.starts_with(char::is_whitespace)
                || !valid_name(&full_requirement_name(namespace, name.trim())))
        {
            literal_definitions.push(node.span.clone());
            continue;
        }
        let body_start =
            node.span.start + source.find('\n').map_or(source.len(), |offset| offset + 1);
        let end = node.span.start + source.trim_end_matches(['\r', '\n']).len();
        let last_line = content[..end].rfind('\n').map_or(0, |offset| offset + 1);
        let closing = &content[last_line..end];
        let prefix = closing.len() - closing.trim_start_matches([' ', '\t', '>']).len();
        let body_end = if last_line >= body_start
            && colon_fence(&closing[prefix..])
                .is_some_and(|(suffix, count)| suffix.trim().is_empty() && count >= length)
        {
            // Preserve any Markdown list/quote prefix; mask only the fence.
            masked[last_line + prefix..end].fill(b' ');
            last_line
        } else {
            node.span.end
        };
        if body_start <= body_end {
            bodies.push(body_start..body_end);
        }
        // Blank delimiters let CommonMark identify top-level paragraphs while
        // retaining the original byte offsets and list/blockquote structure.
        for byte in &mut masked[node.span.start..body_start] {
            if !matches!(*byte, b'\r' | b'\n') {
                *byte = b' ';
            }
        }
    }
    let masked = String::from_utf8(masked).expect("only complete fence lines are blanked");
    let references = reference_roles(content, true);
    let mut omitted = Vec::new();
    let mut depth = 0;
    for (event, range) in Parser::new(&masked).into_offset_iter() {
        match event {
            Event::Start(tag) => {
                if tag == Tag::Paragraph
                    && depth == 0
                    && bodies
                        .iter()
                        .any(|body| body.start <= range.start && range.end <= body.end)
                    && !overlaps_protected(range.start, range.end, &literal_definitions)
                {
                    let mut cursor = range.start;
                    let mut count = 0;
                    for reference in references.iter().filter(|reference| {
                        range.start <= reference.start && reference.end <= range.end
                    }) {
                        if reference.kind != ReferenceKind::Cite
                            || !content[cursor..reference.start].trim().is_empty()
                            || reference_badge(
                                reference.kind,
                                reference.name,
                                Path::new("chapter.md"),
                                definitions,
                            )
                            .is_none()
                        {
                            count = 0;
                            break;
                        }
                        count += 1;
                        cursor = reference.end;
                    }
                    if count > 0 && content[cursor..range.end].trim().is_empty() {
                        omitted.push(range);
                    }
                }
                depth += 1;
            }
            Event::End(_) => depth -= 1,
            _ => {}
        }
    }
    let mut result = String::with_capacity(content.len());
    let mut cursor = 0;
    for range in omitted {
        result.push_str(&content[cursor..range.start]);
        cursor = range.end;
    }
    result.push_str(&content[cursor..]);
    result
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
// {impl}`requirements.index.declaration_sections`
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
                let anchor = escape_html(&block.kind.anchor(&full_name));
                let permalink = format!("#{anchor}").replace(".md", "%2Emd");
                result.push_str(&format!(
                    "<details class=\"habeas-declaration\" style=\"--habeas-declaration-color: #2e8555\" open>\n<summary><span class=\"habeas-definition-badge\"><a class=\"habeas-req-badge habeas-badge-kind\" href=\"{permalink}\" title=\"Link to {escaped_name} in this chapter\">definition</a><a class=\"habeas-req-badge habeas-badge-name\" id=\"{anchor}\" href=\"{permalink}\" title=\"Link to {escaped_name} in this chapter\">{}</a></span></summary>\n<div class=\"habeas-declaration-body\">\n\n",
                    terms::escaped_phrase(phrase)
                ));
                result.push_str(&replace_definitions(body, namespace, current_path, plan));
                result.push_str("\n</div>\n</details>\n\n");
            } else {
                result.push_str(&content[block.start..block.end]);
            }
            cursor = block.end;
            continue;
        }
        let color = if block.kind == DefinitionKind::Example {
            "7151a1"
        } else {
            "006ba8"
        };
        let detail_href = current_path
            .zip(plan)
            .and_then(|(current, plan)| plan.detail_path(&full_name).map(|path| (current, path)))
            .map(|(current, path)| escape_html(&chapter_href(current, path, None)));
        let anchor = format!("habeas-{}-{escaped_name}", block.kind.badge());
        let permalink = format!("#{anchor}").replace(".md", "%2Emd");
        let name_href = detail_href.as_deref().unwrap_or(&permalink);
        let name_title = if detail_href.is_some() {
            format!("View evidence for {escaped_name}")
        } else {
            format!("Link to {escaped_name} in this chapter")
        };
        let kind = block.kind.badge();
        result.push_str(&format!(
            "<details class=\"habeas-declaration\" style=\"--habeas-declaration-color: #{color}\" open>\n<summary><span class=\"habeas-definition-badge\"><a class=\"habeas-req-badge habeas-badge-kind\" id=\"{anchor}\" href=\"{permalink}\" title=\"Link to {escaped_name} in this chapter\" aria-label=\"{kind} {escaped_name}\">{kind}</a><a class=\"habeas-req-badge habeas-badge-name\" href=\"{name_href}\" title=\"{name_title}\">{escaped_name}</a></span></summary>\n<div class=\"habeas-declaration-body\">\n\n"
        ));
        // Option lines such as anchors are metadata, not body text.
        let body = &content[block.text_start..block.body_end];
        let body = if block.kind == DefinitionKind::Requirement {
            replace_query_directives(body)
        } else {
            body.to_owned()
        };
        result.push_str(&replace_definitions(&body, namespace, current_path, plan));
        result.push_str("\n</div>\n</details>\n\n");
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

// {cite}`habeas:util`
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
    let target_output = if target.file_name() == Some(std::ffi::OsStr::new("README.md")) {
        target.with_file_name("index.html")
    } else {
        target.with_extension("html")
    };
    let target_html = target_output
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

// {impl}`requirements.ci.requirement_text`
// {impl}`requirements.examples.definition`
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
    if !["{impl}`", "{cite}`", "{test}`", "{term}`", "{fixme}`"]
        .iter()
        .any(|prefix| content.contains(prefix))
    {
        return Vec::new();
    }
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

// {cite}`habeas:util`
fn is_protected(position: usize, ranges: &[Range<usize>]) -> bool {
    ranges
        .iter()
        .any(|range| range.start <= position && position < range.end)
}

// {cite}`habeas:util`
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
    // {test}`requirements.references.citation_review.book_commentary.display`
    // {test}`requirements.references.builtin_definitions`
    // {test}`requirements.index.navigation`
    // {test}`requirements.examples.authoring_inputs`
    #[test]
    fn citation_metadata_is_hidden_in_fenced_sections_and_preserved_in_details() {
        let citations = "{cite}`rules.ready` \t {cite}`rules.other`\n{cite}`habeas:util`";
        for (opening, options, definition) in [
            (":::{req} case", "", Some("guide.case")),
            (":::{req} case", ":test: md/rules.md\n", Some("guide.case")),
            (":::{example} case", "", Some("guide.case")),
            (":::{definition} case", ":phrase: case\n", None),
            (":::{note} Any prose container", "", None),
            (":::{cite} rules.ready", "", None),
            (":::{impl} rules.ready", "", None),
            (":::{test} rules.ready", "", None),
        ] {
            let root = book_root();
            let rules = ":::{req} ready\nReady.\n:::\n\n:::{req} other\nOther.\n:::\n";
            let original = format!("{opening}\n{options}{citations}\n\nA concrete case.\n:::\n");
            std::fs::write(root.path().join("md/rules.md"), rules).unwrap();
            std::fs::write(root.path().join("md/guide.md"), &original).unwrap();
            let mut book = Book::new();
            book.push_item(Chapter::new("Rules", rules.into(), "rules.md", Vec::new()));
            book.push_item(Chapter::new(
                "Guide",
                original.clone(),
                "guide.md",
                Vec::new(),
            ));
            let declarations = crate::evidence::load_definitions(root.path()).unwrap();
            if let Some(name) = definition {
                assert!(declarations[name].text.contains(citations), "{opening}");
                let sources =
                    crate::authoring::inputs::AssessmentSources::prepare(root.path(), name)
                        .unwrap();
                assert!(sources.subject_text.contains(citations), "{opening}");
                assert!(
                    sources.mandatory_inputs().iter().any(|input| {
                        input["text"].as_array().is_some_and(|lines| {
                            lines
                                .iter()
                                .filter_map(|line| line.as_str())
                                .collect::<Vec<_>>()
                                .join("\n")
                                .contains(citations)
                        })
                    }),
                    "{opening}"
                );
            }
            let references = super::text_references(&original, true);
            preprocess_book(&mut book, root.path()).unwrap();
            let guide = book
                .chapters()
                .find(|chapter| chapter.name == "Guide")
                .unwrap();
            assert!(guide.content.contains("A concrete case."), "{opening}");
            let visible_prose = if opening.starts_with(":::{cite}") {
                let (annotations, prose) = guide.content.split_once("</details>\n\n").unwrap();
                for name in ["rules.ready", "rules.other", "habeas:util"] {
                    assert!(
                        annotations.contains(&format!("alt=\"cite {name}\"")),
                        "{opening}: {annotations}"
                    );
                }
                prose
            } else {
                &guide.content
            };
            assert!(
                !visible_prose.contains("alt=\"cite rules.other\""),
                "{opening}"
            );
            assert!(
                !visible_prose.contains("alt=\"cite habeas:util\""),
                "{opening}"
            );
            assert_eq!(
                guide.content.matches("alt=\"cite rules.ready\"").count(),
                usize::from(opening.starts_with(":::{cite}")),
                "{opening}: {}",
                guide.content
            );
            if let Some(name) = definition {
                let detail = book
                    .chapters()
                    .find(|chapter| chapter.name == name)
                    .unwrap();
                assert!(
                    detail.content.contains("alt=\"cite rules.ready\""),
                    "{opening}"
                );
                assert!(
                    detail.content.contains("alt=\"cite rules.other\""),
                    "{opening}"
                );
                assert!(
                    detail.content.contains("alt=\"cite habeas:util\""),
                    "{opening}"
                );
                assert!(guide.content.contains("habeas-badge-name"), "{opening}");
                assert!(
                    guide
                        .content
                        .contains(&format!("href=\"requirements-index/{name}.html\"")),
                    "{opening}: {}",
                    guide.content
                );
            }
            assert_eq!(
                std::fs::read_to_string(root.path().join("md/guide.md")).unwrap(),
                original
            );
            assert_eq!(super::text_references(&original, true), references);
            for (name, declaration) in declarations {
                assert_eq!(
                    crate::evidence::load_definitions(root.path()).unwrap()[&name].text,
                    declaration.text
                );
            }
        }
    }

    // {test}`requirements.references.citation_review.book_commentary.display`
    #[test]
    fn reviewed_passages_show_one_gutter_list_and_preserve_prose_and_literal_contexts() {
        let rules = ":::{req} ready\nReady.\n:::\n\n:::{req} other\nOther.\n:::\n\n:::{req} inside\nInside.\n:::\n\n:::{req} term_rule\nTerminology.\n:::\n";
        let source = concat!(
            "Outside paragraph. {cite}`rules.inside`.\n\n",
            ":::::{cite} habeas:util\n",
            "{cite}`rules.other`\n\n",
            "The *ordinary* paragraph cites {cite}`rules.ready`.\n\n",
            "::::{cite} rules.ready\nNested prose.\n::::\n\n",
            "::::{req} nested\n{cite}`rules.inside`\nNested requirement.\n::::\n\n",
            "::::{example} sample\n{cite}`rules.inside`\nNested example.\n::::\n\n",
            "::::{definition} term\n:phrase: nested term\nDefinition prose. {cite}`rules.term_rule`\n::::\n\n",
            "```text\n:::{cite} rules.inside\nLiteral code.\n:::\n```\n\n",
            "Inline code ``{cite}`rules.inside` ``.\n",
            "\\{cite}`rules.inside`\n",
            ":::::\n\n",
            ":::{cite} habeas:util\nUtility-only.\n:::\n\n",
            ":::{cite} rules.missing\nUnresolved.\n:::\n\n",
            "<div>\n:::{cite} rules.ready\nHTML literal.\n:::\n</div>\n\n",
            ":::{cite} rules.ready\nUnclosed.\n",
        );
        let root = book_root();
        std::fs::write(root.path().join("md/guide.md"), source).unwrap();
        let original_references = super::text_references(source, true);
        let mut book = Book::new();
        book.push_item(Chapter::new("Rules", rules.into(), "rules.md", vec![]));
        book.push_item(Chapter::new("Guide", source.into(), "guide.md", vec![]));
        preprocess_book(&mut book, root.path()).unwrap();
        let guide = &book
            .chapters()
            .find(|chapter| chapter.name == "Guide")
            .unwrap()
            .content;
        assert_eq!(
            guide.matches("class=\"habeas-citation-passage\"").count(),
            1
        );
        assert_eq!(guide.matches("class=\"habeas-citation-list\"").count(), 1);
        let (before_prose, prose) = guide.split_once("</details>\n\n").unwrap();
        let (outside, list) = before_prose
            .split_once("<div class=\"habeas-citation-passage\">")
            .unwrap();
        assert!(outside.starts_with("Outside paragraph."));
        assert!(outside.contains("href=\"rules.html#habeas-req-rules.inside\""));
        assert!(outside.contains("alt=\"cite rules.inside\""));
        assert!(!outside.contains("habeas-citation-list"));
        assert!(list.contains("<summary aria-label=\"Show citations for this passage\""));
        assert!(
            list.contains("title=\"Show citations\"><span aria-hidden=\"true\">※</span></summary>")
        );
        assert_eq!(list.matches("<li>").count(), 4);
        for name in [
            "habeas:util",
            "rules.other",
            "rules.ready",
            "rules.term_rule",
        ] {
            assert_eq!(list.matches(&format!("alt=\"cite {name}\"")).count(), 1);
        }
        assert!(!list.contains("rules.inside"));
        assert!(list.contains("href=\"rules.html#habeas-req-rules.other\""));
        assert!(list.contains("habeas-builtin\" title="));
        assert!(!prose.contains("alt=\"cite rules.other\""));
        // Only the explicit inline citation remains, not a nested opening badge.
        assert_eq!(prose.matches("alt=\"cite rules.ready\"").count(), 1);
        let utility = prose
            .split("<div class=\"habeas-fenced-ref\">")
            .find(|part| part.contains("Utility-only."))
            .unwrap();
        assert!(utility.contains("habeas-req-ref-cite habeas-builtin\" title="));
        assert!(utility.contains("alt=\"cite habeas:util\""));
        for (name, kind) in [("guide.nested", "req"), ("guide.sample", "example")] {
            assert!(prose.contains(&format!("aria-label=\"{kind} {name}\"")));
            assert!(prose.contains(&format!("href=\"requirements-index/{name}.html\"")));
            let detail = book
                .chapters()
                .find(|chapter| chapter.name == name)
                .unwrap();
            assert!(detail.content.contains("alt=\"cite rules.inside\""));
        }
        assert!(!prose.contains("::::{cite} rules.ready\nNested"));
        for text in [
            "Nested prose.",
            "Nested requirement.",
            "Nested example.",
            "```text\n:::{cite} rules.inside\nLiteral code.\n:::\n```",
            ":::{cite} rules.missing\nUnresolved.\n:::",
            "<div>\n:::{cite} rules.ready\nHTML literal.\n:::\n</div>",
            ":::{cite} rules.ready\nUnclosed.",
        ] {
            assert!(prose.contains(text), "{text}: {prose}");
        }
        let mut html = String::new();
        pulldown_cmark::html::push_html(&mut html, pulldown_cmark::Parser::new(guide));
        assert!(html.contains("<p>The <em>ordinary</em> paragraph cites"));
        assert!(html.contains("<p>Nested prose.</p>"), "{html}");
        assert!(html.contains("<p>Utility-only.</p>"), "{html}");
        assert_eq!(
            std::fs::read_to_string(root.path().join("md/guide.md")).unwrap(),
            source
        );
        assert_eq!(super::text_references(source, true), original_references);

        let css = include_str!("../md/habeas.css");
        let rules = |selector: &str| {
            css.rsplit_once(&format!("{selector} {{"))
                .unwrap()
                .1
                .split('}')
                .next()
                .unwrap()
        };
        let passage = rules(".habeas-citation-passage");
        assert!(passage.contains("padding: 0;") && passage.contains("border: 0;"));
        let margin = passage
            .split(';')
            .find_map(|declaration| declaration.trim().strip_prefix("margin:"))
            .unwrap();
        let margins = margin.split_whitespace().collect::<Vec<_>>();
        let left = match margins.as_slice() {
            [all] => *all,
            [_, horizontal] | [_, horizontal, _] => *horizontal,
            [_, _, _, left] => *left,
            _ => panic!("invalid margin shorthand"),
        };
        assert_eq!(left, "0");
        assert!(passage.contains("position: relative;"));
        let marker = rules(".habeas-citation-list");
        assert!(marker.contains("position: absolute;"));
        assert!(marker.contains("right:") && !marker.contains("left:"));
        let panel = rules(".habeas-citation-list > ul");
        assert!(panel.contains("max-width:") && panel.contains("position: absolute;"));
        // The native disclosure contains annotations only; prose follows it
        // as a sibling, so opening or closing it leaves prose displayed.
        assert!(html.contains("</details>\n<p>The <em>ordinary</em> paragraph"));
        assert!(
            rules(".habeas-citation-list > summary:focus-visible")
                .contains("outline: 2px solid var(--links);")
        );
    }

    // {test}`requirements.references.book_badges`
    // {test}`requirements.references.markdown_literals`
    // {test}`requirements.ci.identifiers`
    #[test]
    fn citation_metadata_preserves_prose_other_roles_and_literal_contexts() {
        let definitions = std::collections::BTreeMap::from([(
            "rules.ready".into(),
            super::Definition {
                path: "rules.md".into(),
                text: "Ready.".into(),
                kind: DefinitionKind::Requirement,
            },
        )]);
        let cite = "{cite}`rules.ready`";
        for body in [
            format!("See {cite}."),
            format!("{cite} {{test}}`rules.ready`"),
            format!("{cite} {{impl}}`rules.ready`"),
            format!("{cite} {{term}}`terms.ready`"),
            format!("{cite} {{cite}}`rules.unknown`"),
            format!("{cite} {{cite}}`habeas:unknown`"),
            format!("{cite} {{cite}}`habeas:invalid name`"),
            format!("{cite} punctuation"),
            format!("\\{cite}"),
            format!("`{cite}`"),
            format!("    {cite}"),
            format!("```text\n{cite}\n```"),
            format!("~~~text\n{cite}\n~~~"),
            format!("> {cite}"),
            format!("- {cite}"),
            format!("- First.\n\n  {cite}"),
            format!("<div>\n{cite}\n</div>"),
            format!("<!-- {cite} -->"),
            format!("<span>{cite}</span>"),
        ] {
            let source = format!(":::{{note}} A section\n{body}\n:::\n");
            assert_eq!(
                super::omit_fenced_citation_paragraphs(&source, "guide", &definitions),
                source,
                "{body}"
            );
        }
        for source in [
            cite.into(),
            format!("Outside.\n\n{cite}\n\n:::{{note}} Empty.\n:::\n"),
            format!("::{{note}} Not a fence\n{cite}\n::\n"),
            format!(":::{{req}} invalid name\n{cite}\n:::\n"),
            format!(":::{{example}} invalid name\n{cite}\n:::\n"),
            format!(":::{{req}} unclosed\n{cite}\n"),
            format!("```text\n:::{{note}} Literal\n{cite}\n:::\n```"),
            format!("<div>\n:::{{note}} Literal\n{cite}\n:::\n</div>"),
            format!("> :::{{note}} Quoted\n> {cite}\n> :::\n"),
        ] {
            assert_eq!(
                super::omit_fenced_citation_paragraphs(&source, "guide", &definitions),
                source
            );
        }
        let source = format!(
            "{cite}\r\n\r\n::::{{req}} outer\r\n{cite}\r\n\r\n:::{{note}} Nésted\r\n{cite}\r\n\r\nNested.\r\n:::\r\n\r\nOuter.\r\n::::\r\n\r\n{cite}\r\n"
        );
        let rendered = super::omit_fenced_citation_paragraphs(&source, "guide", &definitions);
        assert_eq!(rendered.matches(cite).count(), 2, "{rendered}");
        assert!(rendered.contains("Nésted\r\n"), "{rendered}");
        assert!(rendered.contains("Nested.\r\n"), "{rendered}");
        assert!(rendered.contains("Outer.\r\n"), "{rendered}");
        let unclosed = format!(":::{{note}} Retained through EOF\n{cite}\n\nBody.\n");
        assert_eq!(
            super::omit_fenced_citation_paragraphs(&unclosed, "guide", &definitions),
            ":::{note} Retained through EOF\n\nBody.\n"
        );
    }

    // {test}`requirements.references.book_badges`
    // {test}`requirements.references.builtin_definitions`
    // {test}`requirements.test-coverage.check.exceptions.anchor`
    #[test]
    fn fenced_references_render_their_badge_and_body_and_built_ins_are_unlinked() {
        let mut book = Book::new();
        let content = [
            "::::{req} ready\nReady MUST hold.\n\n:::{cite} rules.other\nThe *other* rule.\n:::\n\n:::{impl} rules.other\nThe *implementation* rule.\n:::\n\n:::{test} rules.other\nThe *tested* rule.\n:::\n::::\n\n",
            ":::{req} other\nOther.\n:::\n\n",
            ":::{cite} habeas:util\nServes its caller.\n:::\n\n",
            ":::{cite} habeas:coverage\nA substantive coverage explanation.\n:::\n\n",
            ":::{cite} habeas:none\nRemoved.\n:::\n\n",
            "Inline {cite}`habeas:none` stays literal.\n\n",
            "Inline {cite}`habeas:util` is a badge.\n\n",
            ":::{cite} rules.missing\nUnknown.\n:::\n\n",
            "```text\n:::{cite} rules.other\n:::\n```\n",
        ]
        .concat();
        book.push_item(Chapter::new("Rules", content, "rules.md", Vec::new()));
        let root = book_root();
        preprocess_book(&mut book, root.path()).unwrap();
        let rendered = book.chapters().next().unwrap().content.clone();

        // The project reference links to its definition, then its body.
        let mut html = String::new();
        pulldown_cmark::html::push_html(&mut html, pulldown_cmark::Parser::new(&rendered));
        assert!(html.contains("aria-label=\"req rules.ready\""), "{html}");
        for (kind, color, word) in [
            ("cite", "7151a1", "other"),
            ("impl", "237346", "implementation"),
            ("test", "ae5d14", "tested"),
        ] {
            assert!(
                html.contains(&format!("<p>The <em>{word}</em> rule.</p>")),
                "{html}"
            );
            let linked = format!(
                "<div class=\"habeas-fenced-ref\">\n\n<p><a class=\"habeas-req-badge habeas-req-ref-{kind}\" href=\"rules.html#habeas-req-rules.other\">"
            );
            let start = rendered.find(&linked).expect(&rendered);
            let after = &rendered[start..];
            let badge_end = after.find("</a>").unwrap();
            assert!(
                after[..badge_end].contains(&format!("alt=\"{kind} rules.other\""))
                    && after[..badge_end]
                        .contains(&format!("img.shields.io/badge/{kind}-rules.other-{color}")),
                "{rendered}"
            );
            assert!(
                badge_end < after.find(&format!("The *{word}* rule.")).unwrap(),
                "{rendered}"
            );
            assert!(
                !rendered.contains(&format!(":::{kind} rules.other\nThe")),
                "{rendered}"
            );
        }
        // The outer requirement still contains it and closes at its own fence.
        assert!(!rendered.contains("See evidence"), "{rendered}");

        // A built-in has no link; its hover text is its full text.
        let util = crate::builtins::builtin("habeas:util").unwrap();
        let coverage = crate::builtins::builtin("habeas:coverage").unwrap();
        assert!(rendered.contains(&format!(
            "<span class=\"habeas-req-badge habeas-req-ref-cite habeas-builtin\" title=\"{}\"><img",
            super::escape_html(coverage.text())
        )), "{rendered}");
        assert!(
            rendered.contains(&format!(
                "<span class=\"habeas-req-badge habeas-req-ref-cite habeas-builtin\" title=\"{}\"><img",
                super::escape_html(util.text())
            )),
            "{rendered}"
        );
        let inline = rendered
            .split("Inline ")
            .find(|part| part.contains(" is a badge."))
            .unwrap();
        let inline = inline.split(" is a badge.").next().unwrap();
        assert!(inline.starts_with(&format!("<span class=\"habeas-req-badge habeas-req-ref-cite habeas-builtin\" title=\"{}\"><img", super::escape_html(util.text()))), "{inline}");
        assert!(
            inline.contains("habeas:util") && inline.ends_with("</span>"),
            "{inline}"
        );
        assert!(!inline.contains("<a "), "{inline}");
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
        assert!(rendered.contains("Inline {cite}`habeas:none` stays literal."));
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
    // {test}`requirements.ci.requirement_text`
    #[test]
    // {test}`requirements.ci.span`
    // {test}`requirements.examples.definition`
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
        assert_eq!(
            super::without_options(source, blocks[1].body_start..blocks[1].body_end, &blocks)
                .trim(),
            "A case."
        );

        let rendered = replace_definitions(source, "requirements", None, None);
        assert!(!rendered.contains(":impl: skills"), "{rendered}");
        assert!(!rendered.contains("skills/ready.md"), "{rendered}");
        assert!(!rendered.contains("tests/case.sh"), "{rendered}");
        assert!(rendered.contains("Text :impl: inline."), "{rendered}");
        assert!(rendered.contains(":test: later.md"), "{rendered}");
    }

    // {test}`requirements.test-coverage.index.navigation`
    // {test}`requirements.index.detail_pages`
    // {test}`requirements.index.chapter`
    // {test}`requirements.index.chapter.default`
    // {test}`requirements.index.navigation`
    // {test}`requirements.examples.index`
    // {test}`requirements.test-coverage.index`
    // {test}`requirements.references.anchors`
    #[test]
    fn definitions_without_a_marker_get_a_default_index_and_detail_pages() {
        let mut book = Book::new();
        let mut chapter = Chapter::new(
            "Rules",
            "# Rules\n\n:::{req} zeta\n:impl: notes.txt\nZeta.\n:::\n\n:::{example} alpha\nAlpha case. {cite}`rules.zeta`\n:::\n\n:::{req} cli.mdbook\nMdbook.\n:::\n".to_owned(),
            "rules.md",
            Vec::new(),
        );
        chapter.number = Some(mdbook_preprocessor::book::SectionNumber::new(vec![2]));
        chapter.sub_items.push(BookItem::Chapter(Chapter::new(
            "Child",
            ":::{req} nested\nNested.\n:::\n".to_owned(),
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
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].name, "Child");
        assert_eq!(book.items.len(), 2);
        assert_eq!(book.chapters().count(), 8);
        assert!(rules.content.contains("habeas-badge-name"));
        assert!(!rules.content.contains("See evidence"));
        assert!(!rules.content.contains("notes.txt"));
        for (kind, name) in [("req", "zeta"), ("example", "alpha"), ("req", "cli.mdbook")] {
            assert!(rules.content.contains(&format!(
                "id=\"habeas-{kind}-rules.{name}\" href=\"#habeas-{kind}-rules.{}\"",
                name.replace(".md", "%2Emd")
            )));
        }
        assert!(
            rules
                .content
                .contains("href=\"requirements-index/rules.cli%2Emdbook.html\"")
        );
        assert!(
            pages[0]
                .content
                .contains("href=\"../requirements-index/rules.child.nested.html\"")
        );
        let BookItem::Chapter(index) = book.items.last().unwrap() else {
            panic!("the default index must be a top-level chapter");
        };
        assert_eq!(index.name, "Requirements index");
        assert_eq!(
            index.path.as_deref(),
            Some(Path::new("requirements-index.md"))
        );
        assert!(index.source_path.is_none());
        assert!(index.parent_names.is_empty());
        assert_eq!(index.number.as_deref().map(Vec::as_slice), Some(&[3][..]));
        assert!(!index.content.contains("{requirements-index}"));
        assert!(index.content.contains("(requirement)"));
        assert!(index.content.contains("(example)"));
        assert!(index.content.contains("Missing"));
        let coverage = index
            .sub_items
            .iter()
            .filter_map(|item| match item {
                BookItem::Chapter(chapter) if chapter.name == "Documented coverage gaps" => {
                    Some(chapter)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(coverage.len(), 1);
        assert_eq!(
            coverage[0].path.as_deref(),
            Some(Path::new("requirements-index/coverage.md"))
        );
        assert_eq!(coverage[0].parent_names, ["Requirements index"]);
        assert!(
            index
                .content
                .contains("[Documented coverage gaps](requirements-index/coverage.html)")
        );
        let details = index
            .sub_items
            .iter()
            .filter_map(|item| match item {
                BookItem::Chapter(chapter) if chapter.name != "Documented coverage gaps" => {
                    Some(chapter)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            details
                .iter()
                .map(|page| page.name.as_str())
                .collect::<Vec<_>>(),
            [
                "rules.zeta",
                "rules.alpha",
                "rules.cli.mdbook",
                "rules.child.nested"
            ]
        );
        for (offset, page) in details.iter().enumerate() {
            assert_eq!(page.parent_names, ["Requirements index"]);
            assert_eq!(
                page.number.as_deref().map(Vec::as_slice),
                Some(&[3, offset as u32 + 2][..])
            );
            assert!(
                page.content
                    .contains("[Requirements index](../requirements-index.html)")
            );
        }
        assert!(
            details[2]
                .content
                .contains("[View definition](../rules.html#habeas-req-rules.cli%2Emdbook)")
        );
        assert!(
            details[3]
                .content
                .contains("[View definition](../rules/child.html#habeas-req-rules.child.nested)")
        );
        assert!(!root.path().join("md/requirements-index.md").exists());
    }

    // {test}`requirements.index.chapter`
    // {test}`requirements.index.chapter.default`
    // {test}`requirements.index.detail_pages`
    #[test]
    fn default_index_exists_even_without_definitions_or_numbered_chapters() {
        for with_prose in [false, true] {
            let mut book = Book::new();
            let prose = "An index marker in a code block is literal.\n\n```text\n:::{requirements-index}\n:::\n```\n";
            if with_prose {
                book.push_item(Chapter::new(
                    "Introduction",
                    prose.into(),
                    "intro.md",
                    vec![],
                ));
            }
            let root = book_root();
            preprocess_book(&mut book, root.path()).unwrap();
            assert_eq!(book.items.len(), usize::from(with_prose) + 1);
            if with_prose {
                let BookItem::Chapter(intro) = &book.items[0] else {
                    panic!("the original introduction must remain a chapter");
                };
                assert_eq!(intro.content, prose);
                assert!(intro.number.is_none());
            }
            let BookItem::Chapter(index) = book.items.last().unwrap() else {
                panic!("the default index must be appended");
            };
            assert_eq!(index.name, "Requirements index");
            assert_eq!(
                index.path.as_deref(),
                Some(Path::new("requirements-index.md"))
            );
            assert_eq!(index.number.as_deref().map(Vec::as_slice), Some(&[1][..]));
            assert!(index.source_path.is_none());
            assert_eq!(index.sub_items.len(), 1);
            let BookItem::Chapter(coverage) = &index.sub_items[0] else {
                unreachable!()
            };
            assert_eq!(coverage.name, "Documented coverage gaps");
            assert!(coverage.source_path.is_none());
            assert!(index.parent_names.is_empty());
            assert!(index.content.starts_with("# Requirements index\n\n"));
            assert!(index.content.contains("| Definition | Evidence |"));
            assert!(!index.content.contains("{requirements-index}"));
            assert!(!root.path().join("md/requirements-index.md").exists());
        }
    }

    // {test}`requirements.index.chapter.default`
    #[test]
    fn default_index_number_follows_top_level_chapters_across_parts_and_unnumbered_chapters() {
        let mut book = Book::new();
        book.push_item(BookItem::PartTitle("Part".into()));
        let mut first = Chapter::new("First", "First.".into(), "first.md", vec![]);
        first.number = Some(mdbook_preprocessor::book::SectionNumber::new(vec![2]));
        book.push_item(first);
        let mut chapter = Chapter::new("Chapter", "Text.".into(), "chapter.md", vec![]);
        chapter.number = Some(mdbook_preprocessor::book::SectionNumber::new(vec![4]));
        let mut child = Chapter::new("Child", "Child.".into(), "child.md", vec!["Chapter".into()]);
        child.number = Some(mdbook_preprocessor::book::SectionNumber::new(vec![4, 10]));
        chapter.sub_items.push(BookItem::Chapter(child));
        book.push_item(chapter);
        let mut last = Chapter::new("Last", "Last.".into(), "last.md", vec![]);
        last.number = Some(mdbook_preprocessor::book::SectionNumber::new(vec![3]));
        book.push_item(last);
        book.push_item(BookItem::Separator);
        book.push_item(Chapter::new(
            "Appendix",
            "Appendix.".into(),
            "appendix.md",
            vec![],
        ));
        let root = book_root();
        preprocess_book(&mut book, root.path()).unwrap();
        assert_eq!(book.items.len(), 7);
        let BookItem::Chapter(index) = book.items.last().unwrap() else {
            panic!("the default index must follow the appendix");
        };
        assert_eq!(index.number.as_deref().map(Vec::as_slice), Some(&[5][..]));
        assert_eq!(index.sub_items.len(), 1);
        assert!(
            matches!(&index.sub_items[0], BookItem::Chapter(chapter) if chapter.name == "Documented coverage gaps")
        );
    }

    // {test}`requirements.index.chapter`
    // {test}`requirements.index.detail_pages`
    // {test}`requirements.index.navigation`
    #[test]
    fn index_owns_details_in_book_order_with_badge_icons() {
        let mut book = Book::new();
        book.push_item(Chapter::new(
            "Rules",
            ":::{req} zeta\nZeta.\n:::\n".into(),
            "rules.md",
            vec![],
        ));
        book.push_item(Chapter::new(
            "More",
            ":::{example} cli.mdbook\nExample.\n:::\n".into(),
            "more.md",
            vec![],
        ));
        let mut index = Chapter::new(
            "Index",
            ":::{requirements-index}\n:::\n".into(),
            "appendix/index.md",
            vec![],
        );
        index.number = Some(mdbook_preprocessor::book::SectionNumber::new(vec![3]));
        let mut child = Chapter::new(
            "Existing",
            "Existing.".into(),
            "appendix/existing.md",
            vec!["Index".into()],
        );
        child.number = Some(mdbook_preprocessor::book::SectionNumber::new(vec![3, 1]));
        index.sub_items.push(BookItem::Chapter(child));
        book.push_item(index);
        let root = book_root();
        preprocess_book(&mut book, root.path()).unwrap();
        assert_eq!(
            book.items.len(),
            3,
            "the explicit index keeps its placement"
        );
        let chapters = book.chapters().collect::<Vec<_>>();
        for chapter in &chapters[..2] {
            assert!(chapter.sub_items.is_empty());
            assert!(chapter.content.contains("title=\"View evidence for "));
            assert!(chapter.content.contains("habeas-badge-name"));
            assert!(!chapter.content.contains("<svg"), "{}", chapter.content);
            assert!(!chapter.content.contains("See evidence"));
        }
        assert!(
            chapters[1]
                .content
                .contains("href=\"appendix/index/more.cli%2Emdbook.html\"")
        );
        let pages = chapters[2]
            .sub_items
            .iter()
            .filter_map(|item| match item {
                BookItem::Chapter(chapter) => Some(chapter),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            pages
                .iter()
                .map(|page| page.name.as_str())
                .collect::<Vec<_>>(),
            [
                "Documented coverage gaps",
                "Existing",
                "rules.zeta",
                "more.cli.mdbook"
            ]
        );
        assert_eq!(pages[3].parent_names, ["Index"]);
        assert_eq!(
            pages[3].number.as_deref().map(Vec::as_slice),
            Some(&[3, 4][..])
        );
        assert!(
            pages[3]
                .content
                .contains("[View definition](../../more.html#habeas-example-more.cli%2Emdbook)")
        );
    }

    // {test}`requirements.index.navigation`
    // {test}`requirements.index.declaration_sections`
    #[test]
    fn declaration_badges_and_margin_controls_share_kind_colors() {
        let mut book = Book::new();
        book.push_item(Chapter::new(
            "Rules",
            ":::{req} ready\nReady.\n:::\n\n:::{example} case\nA case. {cite}`rules.ready`\n:::\n\n:::{definition} word\n:phrase: a word\n\nA definition.\n:::\n"
                .into(),
            "rules.md",
            vec![],
        ));
        let root = book_root();
        preprocess_book(&mut book, root.path()).unwrap();
        let content = &book.chapters().next().unwrap().content;
        for (kind, name, color) in [
            ("req", "ready", "006ba8"),
            ("example", "case", "7151a1"),
            ("definition", "word", "2e8555"),
        ] {
            let segment = content
                .split("<details class=\"habeas-declaration\"")
                .find(|part| part.contains(&format!("id=\"habeas-{kind}-rules.{name}\"")))
                .unwrap()
                .split("</summary>")
                .next()
                .unwrap();
            assert!(
                segment.contains(&format!("style=\"--habeas-declaration-color: #{color}\"")),
                "{segment}"
            );
            assert!(segment.contains("habeas-badge-name"), "{segment}");
            assert!(!segment.contains("<svg"), "{segment}");
        }
    }

    // {test}`requirements.index.declaration_sections`
    // {test}`requirements.index.navigation`
    #[test]
    fn declaration_sections_contain_their_headers_and_markdown_bodies() {
        let mut book = Book::new();
        book.push_item(Chapter::new(
            "Rules",
            "Before.\n\n::::{req} ready\n:impl: notes.md\nA **requirement**.\n\n:::{example} case\nA *nested example*. {cite}`rules.ready`\n\n```rust\nlet answer = true;\n```\n:::\n\nThe requirement ends here.\n::::\n\n:::{definition} word\n:phrase: alpha & beta\n\nA named **definition**.\n:::\n\nAfter.\n"
                .into(),
            "rules.md",
            vec![],
        ));
        let root = book_root();
        preprocess_book(&mut book, root.path()).unwrap();
        let markdown = &book.chapters().next().unwrap().content;
        let mut html = String::new();
        pulldown_cmark::html::push_html(&mut html, pulldown_cmark::Parser::new(markdown));
        assert_eq!(
            html.matches("<details class=\"habeas-declaration\"")
                .count(),
            3,
            "{html}"
        );
        for declaration in html.split("<details class=\"habeas-declaration\"").skip(1) {
            let attributes = declaration.split('>').next().unwrap();
            assert!(
                attributes
                    .split_ascii_whitespace()
                    .any(|attribute| attribute == "open"),
                "Declaration should start expanded: {attributes}"
            );
        }
        assert_eq!(html.matches("</details>").count(), 3, "{html}");
        assert_eq!(html.matches("<summary>").count(), 3, "{html}");
        assert_eq!(html.matches("</summary>").count(), 3, "{html}");
        assert!(
            html.contains("<p>Before.</p>\n<details")
                && html.ends_with("</details>\n<p>After.</p>\n"),
            "{html}"
        );
        // Both navigational links belong to the header, not the hidden body.
        for (kind, name) in [("req", "ready"), ("example", "case")] {
            let header = html
                .split(&format!("id=\"habeas-{kind}-rules.{name}\""))
                .nth(1)
                .unwrap()
                .split("</summary>")
                .next()
                .unwrap();
            assert!(
                header.contains(&format!("aria-label=\"{kind} rules.{name}\"")),
                "{header}"
            );
            assert!(
                header.contains(&format!(
                    "href=\"#habeas-{kind}-rules.{name}\" title=\"Link to rules.{name} in this chapter\" aria-label=\"{kind} rules.{name}\">{kind}</a>"
                )),
                "{header}"
            );
            assert!(
                header.contains(&format!(
                    "class=\"habeas-req-badge habeas-badge-name\" href=\"requirements-index/rules.{name}.html\" title=\"View evidence for rules.{name}\">rules.{name}</a>"
                )),
                "{header}"
            );
            assert!(!header.contains("habeas-detail-icon"), "{header}");
            assert!(!header.contains("habeas-declaration-body"), "{header}");
        }
        let body = "<div class=\"habeas-declaration-body\">\n";
        assert!(
            html.contains(&format!(
                "{body}<p>A <strong>requirement</strong>.</p>\n<details"
            )),
            "{html}"
        );
        assert!(
            html.contains(&format!("{body}<p>A <em>nested example</em>.")),
            "{html}"
        );
        assert!(
            html.contains("<pre><code class=\"language-rust\">let answer = true;"),
            "{html}"
        );
        assert!(
            html.contains("</details>\n<p>The requirement ends here.</p>\n</div>\n</details>"),
            "{html}"
        );
        assert!(
            html.contains(">definition</a><a class=\"habeas-req-badge habeas-badge-name\" id=\"habeas-definition-rules.word\" href=\"#habeas-definition-rules.word\" title=\"Link to rules.word in this chapter\">alpha &amp; beta</a></span></summary>"),
            "{html}"
        );
        assert!(
            html.contains(&format!(
                "{body}<p>A named <strong>definition</strong>.</p>"
            )),
            "{html}"
        );
        assert!(!html.contains(":impl: notes.md"), "{html}");
    }

    // {test}`requirements.references.safe_badges`
    // {test}`requirements.index.navigation`
    #[test]
    fn definition_evidence_link_targets_are_escaped() {
        let mut book = Book::new();
        book.push_item(Chapter::new(
            "Rules",
            ":::{req} ready\nReady.\n:::\n".into(),
            "rules.md",
            vec![],
        ));
        book.push_item(Chapter::new(
            "Index",
            ":::{requirements-index}\n:::\n".into(),
            "a&b \"q\" <x>.md",
            vec![],
        ));
        let root = book_root();
        preprocess_book(&mut book, root.path()).unwrap();
        let content = &book.chapters().next().unwrap().content;
        let name_link = content
            .split("<a class=\"habeas-req-badge habeas-badge-name\"")
            .nth(1)
            .unwrap()
            .split("</a>")
            .next()
            .unwrap();
        assert!(
            name_link.contains("href=\"a&amp;b &quot;q&quot; &lt;x&gt;/rules.ready.html\""),
            "{name_link}"
        );
        assert!(name_link.contains("title=\"View evidence for rules.ready\""));
        assert!(!name_link.contains("a&b"));
        assert!(!name_link.contains("\"q\""));
        assert!(!name_link.contains("<x>"));
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
        assert!(rendered.contains("aria-label=\"req requirements.tui.table\""));
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

    // {test}`requirements.index.navigation`
    #[test]
    fn links_to_readme_chapters_use_the_index_output() {
        assert_eq!(
            chapter_href(
                Path::new("requirements-index/rules.book.md"),
                Path::new("requirements/index/README.md"),
                Some("habeas-req-rules.book"),
            ),
            "../requirements/index/index.html#habeas-req-rules.book"
        );
        assert_eq!(
            chapter_href(
                Path::new("requirements/index/README.md"),
                Path::new("requirements/index/README.md"),
                None,
            ),
            "index.html"
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
        assert!(rendered.contains("aria-label=\"req requirements.visible\""));
    }

    // {test}`requirements.ci.identifiers`
    #[test]
    fn invalid_names_remain_literal() {
        let source = ":::{req} bad name\nInvalid.\n:::\n\n:::{req} good\nGood.\n:::";
        let rendered = replace_definitions(source, "rules", None, None);
        assert!(rendered.contains(":::{req} bad name"));
        assert!(rendered.contains("aria-label=\"req rules.good\""));
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
        assert_eq!(
            chapters.len(),
            4,
            "an empty index and its coverage child are generated"
        );
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
            "See {cite}`rules.ready`.".to_owned(),
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

    // {test}`requirements.index.navigation`
    // {test}`requirements.index.declaration_sections`
    #[test]
    fn declaration_badge_stylesheet_keeps_split_links_in_one_row() {
        let rules = stylesheet_rules();
        let declarations = |selector: &str| {
            rules
                .iter()
                .filter(|(selectors, _)| selectors.iter().any(|candidate| candidate == selector))
                .flat_map(|(_, declarations)| declarations.iter().map(String::as_str))
                .collect::<Vec<_>>()
        };
        let wrapper = declarations(".habeas-definition-badge");
        for declaration in [
            "display: inline-flex",
            "align-items: center",
            "flex-wrap: nowrap",
            "white-space: nowrap",
        ] {
            assert!(wrapper.contains(&declaration), "{wrapper:?}");
        }
        let links = declarations(".habeas-definition-badge > a.habeas-req-badge");
        assert!(links.contains(&"flex-shrink: 0"), "{links:?}");
        assert!(
            declarations(".habeas-declaration > summary").contains(&"justify-content: flex-end")
        );
        assert!(
            declarations(".habeas-badge-name")
                .contains(&"background: var(--habeas-declaration-color)")
        );
    }

    // {test}`requirements.index.declaration_sections`
    #[test]
    fn declaration_styles_distinguish_disclosure_state_and_keyboard_focus() {
        let rules = stylesheet_rules();
        let declarations = |selector: &str| {
            rules
                .iter()
                .filter(|(selectors, _)| selectors.iter().any(|candidate| candidate == selector))
                .flat_map(|(_, declarations)| declarations.iter().map(String::as_str))
                .collect::<Vec<_>>()
        };
        let section = declarations(".habeas-declaration");
        assert!(
            section.contains(&"border-right: 3px solid transparent"),
            "{section:?}"
        );
        let bar = declarations(".habeas-declaration::before");
        for declaration in [
            "content: \"\"",
            "position: absolute",
            "top: 0",
            "bottom: 0",
            "right: -3px",
            "width: 3px",
            "border-radius: 2px",
            "background: var(--habeas-declaration-color)",
            "pointer-events: none",
        ] {
            assert!(bar.contains(&declaration), "{bar:?}");
        }
        assert!(
            section.contains(&"margin: 1.25em 0.75em 1.25em 0")
                && section.contains(&"padding-right: 1.25em")
                && declarations(".habeas-declaration-body").contains(&"padding: 0"),
            "{section:?}"
        );
        let control = declarations(".habeas-declaration > summary::after");
        for declaration in [
            "content: \"+\"",
            "flex-shrink: 0",
            "border-radius: 0 3px 3px 0",
            "background: var(--habeas-declaration-color)",
            "border: 1px solid var(--habeas-declaration-color)",
            "border-left-color: var(--bg)",
        ] {
            assert!(control.contains(&declaration), "{control:?}");
        }
        let width = control
            .iter()
            .find_map(|value| value.strip_prefix("width: "));
        let height = control
            .iter()
            .find_map(|value| value.strip_prefix("height: "));
        assert!(
            width.is_some(),
            "Disclosure needs a defined size: {control:?}"
        );
        assert_eq!(width, height, "Disclosure should be square: {control:?}");
        assert_eq!(height, Some("var(--habeas-badge-height)"), "{control:?}");
        let summary = declarations(".habeas-declaration > summary");
        assert!(summary.contains(&"gap: 0"), "{summary:?}");
        assert!(
            summary.contains(&"position: relative") && summary.contains(&"z-index: 1"),
            "The disclosure should remain above the vertical bar: {summary:?}"
        );
        assert!(
            summary
                .contains(&"margin-right: calc(-1.25em - var(--habeas-badge-height) / 2 - 1.5px)"),
            "The disclosure should sit over the top of the right-margin bar: {summary:?}"
        );
        // Links use 0.8em text, so 1.6em equals the control's 1.28em height.
        assert!(
            section.contains(&"--habeas-badge-height: 1.28em"),
            "{section:?}"
        );
        let badge = declarations(".habeas-definition-badge > a.habeas-req-badge");
        assert!(
            badge.contains(&"font-size: 0.8em")
                && badge.contains(&"height: 1.6em")
                && badge.contains(&"box-sizing: border-box")
                && badge.contains(&"padding: 0 0.5em"),
            "{badge:?}"
        );
        assert!(
            declarations(".habeas-badge-kind").contains(&"border-radius: 3px 0 0 3px"),
            "The kind segment should round only the badge's outer left corners"
        );
        assert!(
            declarations(".habeas-badge-name").contains(&"border-left: 1px solid var(--bg)")
                && declarations(".habeas-badge-name").contains(&"border-radius: 0"),
            "The middle segment should have straight joins and a matching thin divider"
        );
        assert!(
            declarations(".habeas-declaration[open] > summary::after").contains(&"content: \"−\"")
        );
        let focus = declarations(".habeas-declaration > summary:focus-visible::after");
        assert!(
            focus
                .iter()
                .any(|declaration| declaration.starts_with("outline: 2px")),
            "{focus:?}"
        );
        assert!(
            declarations(".habeas-declaration > summary > .habeas-definition-badge")
                .contains(&"overflow-x: auto")
        );
        assert!(
            declarations(".habeas-declaration > summary > .habeas-definition-badge")
                .contains(&"scrollbar-width: none"),
            "A horizontal scrollbar must not push the badge above its disclosure"
        );
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
        assert_eq!(
            declarations(".habeas-definition-badge > a.habeas-req-badge"),
            declarations(".habeas-definition-badge > a.habeas-req-badge:visited")
        );
        assert!(
            declarations(".habeas-definition-badge > a.habeas-req-badge:visited")
                .contains(&"color: white")
        );
        // Keyboard focus shows an outline.
        let focus = declarations("a.habeas-req-badge:focus-visible");
        assert!(
            focus.contains(&"outline: 2px solid var(--links)"),
            "{focus:?}"
        );
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
        // Every rule styles a Habeas declaration, badge, fenced reference, or citation
        // passage; nothing else in the book is restyled.
        let specified = [
            ".habeas-req-badge",
            ".habeas-shield",
            ".habeas-definition-badge",
            ".habeas-definition-badge > a.habeas-req-badge",
            ".habeas-definition-badge > a.habeas-req-badge:visited",
            ".habeas-badge-kind",
            ".habeas-badge-name",
            ".habeas-req-badge img.habeas-shield",
            "a.habeas-req-badge",
            "a.habeas-req-badge:visited",
            "a.habeas-req-badge:hover",
            "a.habeas-req-badge:focus-visible",
            ".habeas-builtin",
            ".habeas-fenced-ref",
            ".habeas-declaration",
            ".habeas-declaration::before",
            ".habeas-declaration > summary",
            ".habeas-declaration > summary::-webkit-details-marker",
            ".habeas-declaration > summary::after",
            ".habeas-declaration[open] > summary::after",
            ".habeas-declaration > summary > .habeas-definition-badge",
            ".habeas-declaration > summary:focus-visible::after",
            ".habeas-declaration-body",
            ".habeas-citation-passage",
            ".habeas-citation-list",
            ".habeas-citation-list > summary",
            ".habeas-citation-list > summary::-webkit-details-marker",
            ".habeas-citation-list > summary:hover",
            ".habeas-citation-list[open] > summary",
            ".habeas-citation-list > summary:focus-visible",
            ".habeas-citation-list > ul",
            ".habeas-citation-list > ul > li",
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
    // {test}`requirements.references.book_badges`
    #[test]
    fn markdown_examples_and_escaped_references_stay_literal() {
        let marker = "{impl}`requirements.ready`".to_owned();
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
    // {test}`requirements.references.book_badges`
    #[test]
    fn roles_in_raw_html_stay_literal() {
        let marker = "{impl}`requirements.ready`".to_owned();
        let content = format!(
            "<div>\n{marker}\n</div>\n\n<!-- {marker} -->\n\nText <!-- {marker} --> and <span title=\"{marker}\">tag</span>.\n\nBetween <em>{marker}</em> tags.\n"
        );
        let references = reference_roles(&content, true);
        // Only the role between inline tags is ordinary Markdown text.
        assert_eq!(references.len(), 1, "{content}");
        assert!(content[references[0].start..].starts_with(&format!("{marker}</em>")));
        // Outside Markdown, HTML-looking text does not protect roles.
        assert_eq!(reference_roles(&content, false).len(), 5);

        let mut book = Book::new();
        book.push_item(Chapter::new(
            "Requirements",
            ":::{req} ready\nReady.\n:::".into(),
            "requirements.md",
            Vec::new(),
        ));
        book.push_item(Chapter::new("Guide", content, "guide.md", Vec::new()));
        let root = book_root();
        preprocess_book(&mut book, root.path()).unwrap();
        let rendered = &book
            .chapters()
            .find(|chapter| chapter.name == "Guide")
            .unwrap()
            .content;
        assert!(rendered.contains(&format!("<div>\n{marker}\n</div>")));
        assert_eq!(rendered.matches(&format!("<!-- {marker} -->")).count(), 2);
        assert!(rendered.contains(&format!("<span title=\"{marker}\">tag</span>")));
        assert_eq!(rendered.matches(&marker).count(), 4);
        assert_eq!(rendered.matches("habeas-req-ref-impl").count(), 1);
        assert!(rendered.contains("Between <em><a class=\"habeas-req-badge"));
    }

    // {test}`requirements.references.book_badges`
    // {test}`requirements.references.markdown_literals`
    // {test}`requirements.index.declaration_sections`
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
        let known = "{cite}`requirements.ready`".to_owned();
        let example = "{cite}`missing.example`".to_owned();
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
        let source = "::::{req} ready\nReady.\n\n:::{example} something.foo\nGiven ready, it succeeds. {cite}`requirements.ready`\n:::\n::::\n\n:::{example} bad name\nInvalid.\n:::\n\n```text\n:::{example} hidden\nLiteral.\n:::\n```".to_owned();
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
            "{test}`requirements.something.foo`".to_owned(),
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
                .contains("aria-label=\"example requirements.something.foo\"")
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

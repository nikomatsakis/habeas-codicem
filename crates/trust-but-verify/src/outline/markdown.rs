//! Colon directives overlay the pinned Markdown block grammar. Temporary tilde
//! fences let that grammar determine containers and ranges; their bodies are
//! then parsed as Markdown, with all offsets mapped back to the original text.

use std::collections::HashSet;
use std::ops::Range;

use pulldown_cmark::{Event, Parser as CommonMark};
use tree_sitter::Node;

use super::{
    DraftComment, Encoder, HASH_DIGITS, Label, LegacyOutline, LineIndex, NameRule, OutlineError,
    OutlineNode, Profile, Redaction, Segment, assign_comments, check_errors, disambiguate,
    parse_tree, root_node,
};

/// A parsing view with a source offset for each byte boundary. Removing quote
/// and list continuations or expanding temporary fences never changes the
/// ranges returned to callers.
// {cite}`habeas:util`
struct View {
    text: String,
    offsets: Vec<usize>,
}

impl View {
    // {cite}`habeas:util`
    fn original(source: &str) -> Self {
        Self {
            text: source.to_owned(),
            offsets: (0..=source.len()).collect(),
        }
    }

    // {cite}`habeas:util`
    fn range(&self, range: Range<usize>) -> Range<usize> {
        self.offsets[range.start]..self.offsets[range.end]
    }

    // {cite}`habeas:util`
    fn without(&self, range: Range<usize>, excluded: &[Range<usize>]) -> Self {
        let mut text = Vec::new();
        let mut offsets = Vec::new();
        for byte in range.clone() {
            if !excluded.iter().any(|span| span.contains(&byte)) {
                text.push(self.text.as_bytes()[byte]);
                offsets.push(self.offsets[byte]);
            }
        }
        offsets.push(self.offsets[range.end]);
        Self {
            text: String::from_utf8(text).expect("only ASCII container prefixes are removed"),
            offsets,
        }
    }
}

/// The block grammar makes setext headings leaf blocks. Give its section
/// parser an ATX view of those headings while retaining every original byte
/// boundary, including the underline in the section's source span.
// {impl}`requirements.sdk.sources.markdown.sections`
fn setext_sections(view: &View, root: Node<'_>) -> Option<View> {
    let mut pending = vec![root];
    let mut headings = Vec::new();
    while let Some(node) = pending.pop() {
        if node.kind() == "setext_heading" {
            let content = node.child_by_field_name("heading_content")?;
            let mut cursor = node.walk();
            let underline = node.named_children(&mut cursor).find(|child| {
                matches!(child.kind(), "setext_h1_underline" | "setext_h2_underline")
            })?;
            let level = if underline.kind() == "setext_h1_underline" {
                1
            } else {
                2
            };
            // The grammar can include the next line's quote/list prefix in
            // this node. Keep that continuation for the following body line.
            let end = view.text[underline.end_byte()..node.end_byte()]
                .find('\n')
                .map_or(node.end_byte(), |newline| {
                    underline.end_byte() + newline + 1
                });
            let mut continuations = Vec::new();
            continuation_ranges(content, &mut continuations);
            headings.push((
                node.start_byte()..end,
                content.byte_range(),
                level,
                continuations,
            ));
        } else {
            let mut cursor = node.walk();
            pending.extend(node.named_children(&mut cursor));
        }
    }
    if headings.is_empty() {
        return None;
    }
    headings.sort_by_key(|(range, _, _, _)| range.start);
    let mut text = Vec::new();
    let mut offsets = Vec::new();
    let mut copied = 0;
    for (range, content, level, continuations) in headings {
        for byte in copied..range.start {
            text.push(view.text.as_bytes()[byte]);
            offsets.push(view.offsets[byte]);
        }
        for marker in std::iter::repeat_n(b'#', level).chain([b' ']) {
            text.push(marker);
            offsets.push(view.offsets[range.start]);
        }
        let body_view = view.without(content.clone(), &continuations);
        let body = &body_view.text;
        let mut consumed = 0;
        for (index, word) in body.split_whitespace().enumerate() {
            let start = consumed + body[consumed..].find(word).expect("word is in heading");
            if index != 0 {
                text.push(b' ');
                offsets.push(body_view.offsets[consumed]);
            }
            for byte in start..start + word.len() {
                text.push(body.as_bytes()[byte]);
                offsets.push(body_view.offsets[byte]);
            }
            consumed = start + word.len();
        }
        text.push(b'\n');
        offsets.push(view.offsets[content.end]);
        copied = range.end;
    }
    for byte in copied..view.text.len() {
        text.push(view.text.as_bytes()[byte]);
        offsets.push(view.offsets[byte]);
    }
    offsets.push(*view.offsets.last().expect("end boundary"));
    Some(View {
        text: String::from_utf8(text).expect("heading words retain their UTF-8 bytes"),
        offsets,
    })
}

/// An owned grammar node, including synthetic colon containers. Ownership is
/// recorded here so the shared identity rule can omit outline descendants.
// {cite}`habeas:util`
struct Syntax {
    kind: Option<String>,
    range: Range<usize>,
    children: Vec<usize>,
    owner: Option<usize>,
    comment: bool,
}

// {cite}`habeas:util`
struct Builder<'a> {
    profile: &'static Profile,
    grammar: fn() -> tree_sitter::Language,
    source: &'a str,
    redaction: &'a Redaction,
    lines: &'a LineIndex,
    syntax: Vec<Syntax>,
    nodes: Vec<OutlineNode>,
    cores: Vec<usize>,
    comments: Vec<DraftComment>,
}

// {impl}`requirements.sdk.sources.markdown.paragraphs`
// {impl}`requirements.sdk.sources.markdown.fenced_blocks`
// {impl}`requirements.sdk.sources.markdown.fenced_blocks.contents`
// {impl}`requirements.sdk.sources.markdown.fence_precedence`
pub(super) fn parse(
    profile: &'static Profile,
    grammar: fn() -> tree_sitter::Language,
    source: &str,
    redaction: &Redaction,
    lines: &LineIndex,
) -> Result<LegacyOutline, OutlineError> {
    let mut builder = Builder {
        profile,
        grammar,
        source,
        redaction,
        lines,
        syntax: Vec::new(),
        nodes: vec![root_node(source, lines, String::new(), String::new())],
        cores: vec![0],
        comments: Vec::new(),
    };
    builder.context(View::original(source), 0)?;
    for index in 0..builder.nodes.len() {
        let core = builder.cores[index];
        let mut structural = Encoder::new();
        builder.encode(&mut structural, core, index, false);
        let mut identity = Encoder::new();
        builder.encode(&mut identity, core, index, true);
        let node = &mut builder.nodes[index];
        node.structural_hash = structural.finish();
        node.identity_hash = identity.finish();
        if index != 0 {
            let label = match &node.name {
                Some(parts) => Label::Named(parts.clone()),
                None => Label::Hash(node.identity_hash[..HASH_DIGITS].to_owned()),
            };
            node.segment = Segment::base(node.kind, &label);
        }
    }
    disambiguate(&mut builder.nodes);
    let comments = assign_comments(&mut builder.nodes, builder.comments, source, lines);
    Ok(LegacyOutline {
        profile,
        nodes: builder.nodes,
        comments,
    })
}

impl Builder<'_> {
    // {cite}`habeas:util`
    fn context(&mut self, mut view: View, parent: usize) -> Result<usize, OutlineError> {
        // The block grammar requires a line ending after fence info. Give it
        // a virtual one at EOF without extending any original source range.
        if !view.text.is_empty() && !view.text.ends_with('\n') {
            view.text.push('\n');
            view.offsets
                .push(*view.offsets.last().expect("end boundary"));
        }
        let original = parse_tree(self.profile, self.grammar, &view.text)?;
        if !view.text.contains(":::") {
            if let Some(view) = setext_sections(&view, original.root_node()) {
                let tree = parse_tree(self.profile, self.grammar, &view.text)?;
                check_errors(&tree, &LineIndex::new(&view.text))?;
                return self.copy(tree.root_node(), &view, &HashSet::new(), parent);
            }
            check_errors(&original, &LineIndex::new(&view.text))?;
            return self.copy(original.root_node(), &view, &HashSet::new(), parent);
        }
        let (view, directives) = fences(view, original.root_node());
        let mut tree = parse_tree(self.profile, self.grammar, &view.text)?;
        let view = if let Some(converted) = setext_sections(&view, tree.root_node()) {
            tree = parse_tree(self.profile, self.grammar, &converted.text)?;
            converted
        } else {
            view
        };
        check_errors(&tree, &LineIndex::new(&view.text)).map_err(|mut error| {
            let local = LineIndex::new(&view.text).starts[error.line - 1];
            error.line = self.lines.line(view.offsets[local]);
            error
        })?;
        self.copy(tree.root_node(), &view, &directives, parent)
    }

    // {cite}`habeas:util`
    fn copy(
        &mut self,
        node: Node<'_>,
        view: &View,
        directives: &HashSet<usize>,
        parent: usize,
    ) -> Result<usize, OutlineError> {
        let syntax = self.syntax.len();
        let range = view.range(node.byte_range());
        let comment = (self.profile.is_comment)(node, &view.text);
        self.syntax.push(Syntax {
            kind: node.is_named().then(|| node.kind().to_owned()),
            range: range.clone(),
            children: Vec::new(),
            owner: None,
            comment,
        });
        if comment {
            self.comments.push(DraftComment {
                byte_start: range.start,
                byte_end: range.end,
                binds_to_container: false,
            });
            return Ok(syntax);
        }
        let mut cursor = node.walk();
        let children = node.children(&mut cursor).collect::<Vec<_>>();
        let colon = node.kind() == "fenced_code_block"
            && children.iter().any(|child| {
                child.kind() == "fenced_code_block_delimiter"
                    && directives.contains(&view.offsets[child.start_byte()])
            });
        let rule = self.profile.rule(node);
        let owner = if colon || rule.is_some() {
            let (kind, name, declaration) = if colon {
                ("block", None, range.start)
            } else {
                let rule = rule.expect("mapped node");
                let named = match rule.name {
                    NameRule::Custom(name) => name(node, &view.text),
                    NameRule::Anonymous => None,
                    NameRule::Field(field) => node.child_by_field_name(field).map(|child| {
                        (
                            vec![view.text[child.byte_range()].to_owned()],
                            child.start_byte(),
                        )
                    }),
                    NameRule::FirstChild(kinds) => children
                        .iter()
                        .find(|child| child.is_named() && kinds.contains(&child.kind()))
                        .map(|child| {
                            (
                                vec![view.text[child.byte_range()].to_owned()],
                                child.start_byte(),
                            )
                        }),
                };
                let (name, declaration) = named.map_or((None, range.start), |(name, byte)| {
                    (Some(name), view.offsets[byte])
                });
                (rule.kind, name, declaration)
            };
            if kind == "section" && name.is_none() && parent != 0 {
                parent
            } else {
                self.push(kind, name, declaration, range.clone(), syntax, parent)
            }
        } else {
            parent
        };
        if colon {
            self.syntax[syntax].kind = Some("colon_directive".to_owned());
            let opener = children
                .iter()
                .find(|child| child.kind() == "fenced_code_block_delimiter")
                .expect("a colon container has an opener");
            let body_start = view.text[opener.end_byte()..]
                .find('\n')
                .map_or(node.end_byte(), |byte| opener.end_byte() + byte + 1);
            let closer = children.iter().rev().find(|child| {
                child.kind() == "fenced_code_block_delimiter" && child.id() != opener.id()
            });
            let body_end = closer.map_or(node.end_byte(), |closer| {
                view.text[..closer.start_byte()]
                    .rfind('\n')
                    .map_or(0, |byte| byte + 1)
            });
            let mut continuations = Vec::new();
            continuation_ranges(node, &mut continuations);
            // Fence indentation is not part of the body indentation.
            let line_start = view.text[..opener.start_byte()]
                .rfind('\n')
                .map_or(0, |byte| byte + 1);
            let indent = view.text[line_start..opener.start_byte()]
                .bytes()
                .rev()
                .take_while(|byte| *byte == b' ')
                .count()
                .min(3);
            for start in LineIndex::new(&view.text).starts {
                if body_start <= start && start < body_end {
                    let spaces = view.text[start..]
                        .bytes()
                        .take(indent)
                        .take_while(|byte| *byte == b' ')
                        .count();
                    continuations.push(start..start + spaces);
                }
            }
            let body = view.without(body_start..body_end, &continuations);
            // Restore original fence text before recursively recognizing inner
            // directives. Temporary tilde runs are strictly parser machinery.
            let body = restore(body, self.source);
            let inner = self.context(body, owner)?;
            self.syntax[syntax].children.push(inner);
        } else {
            for child in children {
                let copied = self.copy(child, view, directives, owner)?;
                self.syntax[syntax].children.push(copied);
            }
        }
        Ok(syntax)
    }

    // {cite}`habeas:util`
    fn push(
        &mut self,
        kind: &'static str,
        name: Option<Vec<String>>,
        declaration: usize,
        range: Range<usize>,
        syntax: usize,
        parent: usize,
    ) -> usize {
        let index = self.nodes.len();
        let (line_start, line_end) = self.lines.range(range.start, range.end);
        self.nodes[parent].children.push(index);
        self.nodes.push(OutlineNode {
            kind,
            name,
            segment: String::new(),
            selector: String::new(),
            parent: Some(parent),
            children: Vec::new(),
            line_start,
            line_end,
            declaration_line: self.lines.line(declaration),
            byte_start: range.start,
            byte_end: range.end,
            source_start: range.start,
            structural_hash: String::new(),
            identity_hash: String::new(),
        });
        self.syntax[syntax].owner = Some(index);
        self.cores.push(syntax);
        index
    }

    // {cite}`habeas:util`
    fn encode(&self, encoder: &mut Encoder, syntax: usize, owner: usize, identity: bool) {
        let node = &self.syntax[syntax];
        if node.comment || (identity && node.owner.is_some_and(|other| other != owner)) {
            return;
        }
        if let Some(kind) = node.kind.as_deref() {
            encoder.kind(kind);
        }
        let mut position = node.range.start;
        for child in &node.children {
            let child_node = &self.syntax[*child];
            if position < child_node.range.start {
                encoder.words(
                    &self
                        .redaction
                        .slice(self.source, position..child_node.range.start),
                );
            }
            self.encode(encoder, *child, owner, identity);
            position = position.max(child_node.range.end);
        }
        if position < node.range.end {
            encoder.words(&self.redaction.slice(self.source, position..node.range.end));
        }
        if node.kind.is_some() {
            encoder.end();
        }
    }
}

// {cite}`habeas:util`
fn continuation_ranges(node: Node<'_>, ranges: &mut Vec<Range<usize>>) {
    if node.kind() == "block_continuation" {
        ranges.push(node.byte_range());
    } else {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            continuation_ranges(child, ranges);
        }
    }
}

/// Rebuild original bytes, omitting repeated offsets introduced by expanded
/// fences. Container-prefix removals remain removed.
// {cite}`habeas:util`
fn restore(view: View, source: &str) -> View {
    let mut text = Vec::new();
    let mut offsets = Vec::new();
    let end = *view.offsets.last().expect("end boundary");
    for byte in &view.offsets[..view.text.len()] {
        if *byte < end && offsets.last() != Some(byte) {
            text.push(source.as_bytes()[*byte]);
            offsets.push(*byte);
        }
    }
    offsets.push(end);
    View {
        text: String::from_utf8(text).expect("source text"),
        offsets,
    }
}

/// Recognize only block-line directives, outside CommonMark literal regions.
/// A long outer closer takes precedence over an unfinished shorter inner one.
fn fences(view: View, root: Node<'_>) -> (View, HashSet<usize>) {
    let mut shielded = Vec::new();
    literal_ranges(root, &mut shielded);
    shielded.extend(
        CommonMark::new(&view.text)
            .into_offset_iter()
            .filter_map(|(event, range)| {
                matches!(
                    event,
                    Event::Code(_) | Event::Html(_) | Event::InlineHtml(_)
                )
                .then_some(range)
            }),
    );
    let padding = view
        .text
        .split(|ch| ch != '~')
        .map(str::len)
        .max()
        .unwrap_or(0)
        + 1;
    let mut stack = Vec::new();
    let mut changes = Vec::new();
    let mut directives = HashSet::new();
    let mut line_start = 0;
    for line in view.text.split_inclusive('\n') {
        if let Some((start, count, opens)) = colon_line(line) {
            let start = line_start + start;
            if !shielded.iter().any(|range| range.contains(&start)) {
                if opens && stack.last().is_none_or(|outer| count < *outer) {
                    stack.push(count);
                    directives.insert(view.offsets[start]);
                    changes.push(start..start + count);
                } else if !opens && let Some(index) = stack.iter().position(|outer| count >= *outer)
                {
                    stack.truncate(index);
                    changes.push(start..start + count);
                }
            }
        }
        line_start += line.len();
    }
    let mut text = Vec::new();
    let mut offsets = Vec::new();
    let mut byte = 0;
    for change in changes {
        text.extend_from_slice(&view.text.as_bytes()[byte..change.start]);
        offsets.extend_from_slice(&view.offsets[byte..change.start]);
        text.extend(std::iter::repeat_n(b'~', padding + change.len()));
        offsets.extend(std::iter::repeat_n(view.offsets[change.start], padding));
        offsets.extend_from_slice(&view.offsets[change.clone()]);
        byte = change.end;
    }
    text.extend_from_slice(&view.text.as_bytes()[byte..]);
    offsets.extend_from_slice(&view.offsets[byte..]);
    (
        View {
            text: String::from_utf8(text).expect("only ASCII fences are changed"),
            offsets,
        },
        directives,
    )
}

fn literal_ranges(node: Node<'_>, ranges: &mut Vec<Range<usize>>) {
    if matches!(
        node.kind(),
        "fenced_code_block" | "indented_code_block" | "html_block"
    ) {
        ranges.push(node.byte_range());
    } else {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            literal_ranges(child, ranges);
        }
    }
}

fn colon_line(line: &str) -> Option<(usize, usize, bool)> {
    let start = line.len() - line.trim_start_matches([' ', '\t', '>']).len();
    let tail = &line[start..];
    let count = tail.bytes().take_while(|byte| *byte == b':').count();
    if count < 3 {
        return None;
    }
    let rest = tail[count..].trim();
    if rest.is_empty() {
        Some((start, count, false))
    } else if rest.starts_with('{')
        && rest
            .find('}')
            .is_some_and(|end| !rest[1..end].trim().is_empty())
    {
        Some((start, count, true))
    } else {
        None
    }
}

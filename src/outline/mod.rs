//! Source outlines: per-language trees of selectable nodes with structural
//! hashes, built from pinned tree-sitter grammars or the plain-text profile.
//!
//! [`parse`] selects a profile by file extension and returns an [`Outline`]:
//! a pre-order list of [`OutlineNode`]s whose first entry is the root, plus
//! every [`OutlineComment`] with the node it belongs to.

mod css;
mod hash;
mod markdown;
mod plain;
pub(crate) mod profile;
mod python;
mod rust;
mod selector;
mod toml;
mod typescript;
mod yaml;

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use tree_sitter::{Node, Parser, Tree};

use crate::redaction::Redaction;
use hash::Encoder;
use profile::{NameRule, text};
pub(crate) use profile::{Profile, is_markdown_path, profile_for_path};
pub(crate) use rust::PROFILE as RUST_PROFILE;
pub(crate) use selector::{HASH_DIGITS, Label, Segment, SelectorError, parse_selector};

/// One outline node. Line numbers are one-based and inclusive.
#[derive(Clone, Debug)]
pub(crate) struct OutlineNode {
    /// Short outline kind, such as `fn`; empty for the root.
    pub(crate) kind: &'static str,
    /// Name parts; `None` for an anonymous node. Most names have one part;
    /// a Rust trait impl has two, the self type and the trait.
    pub(crate) name: Option<Vec<String>>,
    /// This node's selector segment; empty for the root.
    pub(crate) segment: String,
    /// Full selector from the root; empty for the root.
    pub(crate) selector: String,
    pub(crate) parent: Option<usize>,
    pub(crate) children: Vec<usize>,
    /// First line, including attached siblings and comments belonging here.
    pub(crate) line_start: usize,
    /// Last line, including comments belonging here.
    pub(crate) line_end: usize,
    /// Line containing the name, or the node's first line when anonymous.
    pub(crate) declaration_line: usize,
    /// Byte range of the grammar node itself, without attached siblings; for
    /// a profile with `trailing_comments_outside`, it ends at the node's last
    /// non-comment token.
    pub(crate) byte_start: usize,
    pub(crate) byte_end: usize,
    /// SHA-256 structural hash, lowercase hex.
    pub(crate) structural_hash: String,
    /// SHA-256 identity hash, lowercase hex; excludes outline descendants.
    pub(crate) identity_hash: String,
}

impl OutlineNode {
    /// The name as displayed, with parts joined by `/`.
    // {impl}`requirements.authoring.outline_tool`
    pub(crate) fn display_name(&self) -> Option<String> {
        self.name.as_ref().map(|parts| parts.join("/"))
    }
}

/// One comment and the outline node it belongs to.
#[derive(Clone, Debug)]
pub(crate) struct OutlineComment {
    pub(crate) byte_start: usize,
    pub(crate) byte_end: usize,
    pub(crate) line_start: usize,
    pub(crate) line_end: usize,
    /// Index of the owning node in [`Outline::nodes`].
    pub(crate) owner: usize,
    /// Index of the comment run containing this comment; comments of one run
    /// share it, and runs are numbered in source order.
    pub(crate) run: usize,
}

/// A parsed outline.
#[derive(Clone, Debug)]
pub(crate) struct Outline {
    pub(crate) profile: &'static Profile,
    /// Nodes in source pre-order; index 0 is the root.
    pub(crate) nodes: Vec<OutlineNode>,
    /// Comments in source order.
    pub(crate) comments: Vec<OutlineComment>,
}

/// A source that its grammar cannot parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OutlineError {
    pub(crate) line: usize,
    pub(crate) message: String,
}

impl fmt::Display for OutlineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for OutlineError {}

/// A selector that does not resolve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ResolveError {
    Invalid(SelectorError),
    NotFound(String),
}

impl fmt::Display for ResolveError {
    // {impl}`requirements.references.outline_selectors`
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(error) => error.fmt(formatter),
            Self::NotFound(selector) => write!(formatter, "no outline node matches {selector:?}"),
        }
    }
}

impl std::error::Error for ResolveError {}

/// Parses a source with the profile its path selects. Hashes use the
/// source's redacted text pieces; the outline itself is built from the
/// original source.
// {impl}`requirements.references.outlines`
// {impl}`requirements.authoring.redaction`
#[cfg(test)]
pub(crate) fn parse(path: &std::path::Path, source: &str) -> Result<Outline, OutlineError> {
    parse_redacted(
        profile_for_path(path),
        source,
        &Redaction::of_file(path, source),
    )
}

/// Parses a source with a given profile, redacting by `secretshape` rules.
// {impl}`requirements.references.outlines`
// {impl}`requirements.authoring.redaction`
pub(crate) fn parse_with(profile: &'static Profile, source: &str) -> Result<Outline, OutlineError> {
    parse_redacted(profile, source, &Redaction::of_text(source))
}

/// Parses the original source with a given profile, hashing each text piece
/// that overlaps a redacted region in its redacted form.
// {impl}`requirements.references.structural_hash`
pub(crate) fn parse_redacted(
    profile: &'static Profile,
    source: &str,
    redaction: &Redaction,
) -> Result<Outline, OutlineError> {
    let lines = LineIndex::new(source);
    let Some(language) = profile.language else {
        return Ok(plain_text(profile, source, redaction, &lines));
    };
    let mut parser = Parser::new();
    parser
        .set_language(&language())
        .map_err(|error| OutlineError {
            line: 1,
            message: format!("cannot load {} grammar: {error}", profile.name),
        })?;
    let tree = parser.parse(source, None).ok_or_else(|| OutlineError {
        line: 1,
        message: "parser produced no tree".to_owned(),
    })?;
    check_errors(&tree, &lines)?;
    Ok(Builder::new(profile, source, redaction, &tree).finish(&lines))
}

/// Reports the first syntax error in a tree.
fn check_errors(tree: &Tree, lines: &LineIndex) -> Result<(), OutlineError> {
    let root = tree.root_node();
    if !root.has_error() {
        return Ok(());
    }
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.is_error() || node.is_missing() {
            return Err(OutlineError {
                line: lines.line(node.start_byte()),
                message: if node.is_missing() {
                    format!("syntax error: missing {}", node.kind())
                } else {
                    "syntax error".to_owned()
                },
            });
        }
        let mut cursor = node.walk();
        let children = node
            .children(&mut cursor)
            .filter(Node::has_error)
            .collect::<Vec<_>>();
        stack.extend(children.into_iter().rev());
    }
    Err(OutlineError {
        line: 1,
        message: "syntax error".to_owned(),
    })
}

/// Byte offsets of line starts.
struct LineIndex {
    starts: Vec<usize>,
    count: usize,
}

impl LineIndex {
    fn new(source: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(
            source
                .bytes()
                .enumerate()
                .filter(|(_, byte)| *byte == b'\n')
                .map(|(index, _)| index + 1),
        );
        let count = if source.is_empty() {
            1
        } else if source.ends_with('\n') {
            starts.len() - 1
        } else {
            starts.len()
        };
        Self { starts, count }
    }

    /// One-based line containing a byte offset.
    fn line(&self, byte: usize) -> usize {
        self.starts.partition_point(|start| *start <= byte).max(1)
    }

    /// Inclusive line range of a byte range.
    fn range(&self, start: usize, end: usize) -> (usize, usize) {
        (
            self.line(start),
            self.line(end.saturating_sub(1).max(start)),
        )
    }
}

/// The plain-text profile: one anonymous `line` node per non-blank line.
// {impl}`requirements.references.outline_selectors`
fn plain_text(
    profile: &'static Profile,
    source: &str,
    redaction: &Redaction,
    lines: &LineIndex,
) -> Outline {
    let mut root_hash = Encoder::new();
    let mut nodes = vec![root_node(source, lines, String::new(), String::new())];
    for (index, start) in lines.starts.iter().take(lines.count).enumerate() {
        let end = lines
            .starts
            .get(index + 1)
            .map_or(source.len(), |next| next - 1);
        if source[*start..end].trim().is_empty() {
            continue;
        }
        let redacted = redaction.slice(source, *start..end);
        let line = redacted.trim();
        root_hash.kind("line");
        root_hash.words(line);
        root_hash.end();
        let hash = hash::line(line);
        let segment = Segment::base("line", &Label::Hash(hash[..HASH_DIGITS].to_owned()));
        let node = nodes.len();
        nodes[0].children.push(node);
        nodes.push(OutlineNode {
            kind: "line",
            name: None,
            selector: segment.clone(),
            segment,
            parent: Some(0),
            children: Vec::new(),
            line_start: index + 1,
            line_end: index + 1,
            declaration_line: index + 1,
            byte_start: *start,
            byte_end: end,
            structural_hash: hash.clone(),
            identity_hash: hash,
        });
    }
    nodes[0].structural_hash = root_hash.finish();
    nodes[0].identity_hash = Encoder::new().finish();
    Outline {
        profile,
        nodes,
        comments: Vec::new(),
    }
}

/// The root node for the whole file; its segment and selector are empty.
fn root_node(
    source: &str,
    lines: &LineIndex,
    structural_hash: String,
    identity_hash: String,
) -> OutlineNode {
    OutlineNode {
        kind: "",
        name: None,
        segment: String::new(),
        selector: String::new(),
        parent: None,
        children: Vec::new(),
        line_start: 1,
        line_end: lines.count,
        declaration_line: 1,
        byte_start: 0,
        byte_end: source.len(),
        structural_hash,
        identity_hash,
    }
}

/// An outline node before hashing and selector assignment.
struct Draft<'tree> {
    kind: &'static str,
    name: Option<Vec<String>>,
    declaration_byte: usize,
    parent: Option<usize>,
    core: Node<'tree>,
    attached: Vec<Node<'tree>>,
}

/// A comment before ownership is assigned.
struct DraftComment {
    byte_start: usize,
    byte_end: usize,
    binds_to_container: bool,
}

/// Walks a grammar tree into outline drafts.
struct Builder<'tree, 'source> {
    profile: &'static Profile,
    source: &'source str,
    redaction: &'source Redaction,
    drafts: Vec<Draft<'tree>>,
    comments: Vec<DraftComment>,
    /// Grammar node id of each draft's core and attached siblings.
    owners: HashMap<usize, usize>,
}

impl<'tree, 'source> Builder<'tree, 'source> {
    fn new(
        profile: &'static Profile,
        source: &'source str,
        redaction: &'source Redaction,
        tree: &'tree Tree,
    ) -> Self {
        let root = tree.root_node();
        let mut builder = Self {
            profile,
            source,
            redaction,
            drafts: vec![Draft {
                kind: "",
                name: None,
                declaration_byte: 0,
                parent: None,
                core: root,
                attached: Vec::new(),
            }],
            comments: Vec::new(),
            owners: HashMap::from([(root.id(), 0)]),
        };
        builder.walk(root, 0);
        builder
    }

    fn is_comment(&self, node: Node<'_>) -> bool {
        (self.profile.is_comment)(node, self.source)
    }

    /// Whether a grammar node maps to an outline node.
    fn maps(&self, node: Node<'_>) -> bool {
        !self.is_comment(node) && self.profile.rule(node).is_some()
    }

    fn record_comment(&mut self, node: Node<'_>) {
        self.comments.push(DraftComment {
            byte_start: node.start_byte(),
            byte_end: node.end_byte(),
            binds_to_container: (self.profile.comment_binds_to_container)(node, self.source),
        });
    }

    /// Records comments anywhere under a node that is not walked as outline
    /// structure, such as an attached attribute.
    fn record_comments_within(&mut self, node: Node<'_>) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if self.is_comment(child) {
                self.record_comment(child);
            } else {
                self.record_comments_within(child);
            }
        }
    }

    /// Names a mapped node, returning its parts and the byte where the name
    /// is declared.
    fn name(&self, rule: NameRule, node: Node<'_>) -> Option<(Vec<String>, usize)> {
        let named = |child: Node<'_>| {
            (
                vec![text(child, self.source).to_owned()],
                child.start_byte(),
            )
        };
        match rule {
            NameRule::Anonymous => None,
            NameRule::Field(field) => node.child_by_field_name(field).map(named),
            NameRule::FirstChild(kinds) => {
                let mut cursor = node.walk();
                let child = node
                    .named_children(&mut cursor)
                    .find(|child| kinds.contains(&child.kind()));
                child.map(named)
            }
            NameRule::Custom(rule) => rule(node, self.source),
        }
    }

    fn walk(&mut self, node: Node<'tree>, parent: usize) {
        let mut cursor = node.walk();
        let children = node.children(&mut cursor).collect::<Vec<_>>();
        let mut attached_to = vec![None; children.len()];
        for (index, child) in children.iter().enumerate() {
            if !self.maps(*child) {
                continue;
            }
            for previous in (0..index).rev() {
                let sibling = children[previous];
                if (self.profile.attaches)(sibling, self.source) {
                    attached_to[previous] = Some(index);
                } else if !self.is_comment(sibling) {
                    break;
                }
            }
        }
        for (index, child) in children.iter().copied().enumerate() {
            if self.is_comment(child) {
                self.record_comment(child);
                continue;
            }
            if attached_to[index].is_some() {
                self.record_comments_within(child);
                continue;
            }
            if let Some(rule) = self.profile.rule(child) {
                let (name, declaration_byte) = match self.name(rule.name, child) {
                    Some((parts, byte)) => (Some(parts), byte),
                    None => (None, child.start_byte()),
                };
                let attached = children
                    .iter()
                    .zip(&attached_to)
                    .filter(|(_, target)| **target == Some(index))
                    .map(|(sibling, _)| *sibling)
                    .collect::<Vec<_>>();
                let draft = self.push(rule.kind, name, declaration_byte, parent, child, attached);
                self.walk(child, draft);
            } else {
                // Unmapped grammar nodes are transparent; source in no mapped
                // node belongs to the nearest outline ancestor, ultimately
                // the root.
                self.walk(child, parent);
            }
        }
    }

    fn push(
        &mut self,
        kind: &'static str,
        name: Option<Vec<String>>,
        declaration_byte: usize,
        parent: usize,
        core: Node<'tree>,
        attached: Vec<Node<'tree>>,
    ) -> usize {
        let index = self.drafts.len();
        self.owners.insert(core.id(), index);
        for sibling in &attached {
            self.owners.insert(sibling.id(), index);
        }
        self.drafts.push(Draft {
            kind,
            name,
            declaration_byte,
            parent: Some(parent),
            core,
            attached,
        });
        index
    }

    /// Computes a draft's structural or identity hash.
    fn hash(&self, index: usize, identity: bool) -> String {
        let draft = &self.drafts[index];
        let mut encoder = Encoder::new();
        for node in draft.attached.iter().chain([&draft.core]) {
            self.encode(&mut encoder, *node, index, identity);
        }
        encoder.finish()
    }

    fn encode(&self, encoder: &mut Encoder, node: Node<'_>, index: usize, identity: bool) {
        if identity
            && self
                .owners
                .get(&node.id())
                .is_some_and(|owner| *owner != index)
        {
            return;
        }
        if self.is_comment(node) {
            if (self.profile.hashes_comment)(node, self.source) {
                let body = self.piece(node.start_byte(), node.end_byte());
                let (group, prose) = (self.profile.comment_prose)(node, &body);
                encoder.prose(group, &prose);
            }
            return;
        }
        if node.is_named() {
            encoder.kind(node.kind());
        }
        if node.child_count() == 0 {
            encoder.words(&self.piece(node.start_byte(), node.end_byte()));
        } else {
            // Text inside a node that no child token covers, such as TOML
            // string contents, YAML block scalars, or Markdown inline text,
            // is a text piece of its own.
            let mut position = node.start_byte();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if position <= child.start_byte() {
                    encoder.words(&self.piece(position, child.start_byte()));
                }
                self.encode(encoder, child, index, identity);
                position = position.max(child.end_byte());
            }
            if position <= node.end_byte() {
                encoder.words(&self.piece(position, node.end_byte()));
            }
        }
        if node.is_named() {
            encoder.end();
        }
    }

    /// A text piece of the source in its redacted form.
    fn piece(&self, start: usize, end: usize) -> std::borrow::Cow<'source, str> {
        self.redaction.slice(self.source, start..end)
    }

    /// Builds the root and outline nodes, linking parents and children, and
    /// assigns hashes, selectors, line ranges including attached siblings,
    /// and comment owners.
    // {impl}`requirements.references.outline_selectors`
    fn finish(self, lines: &LineIndex) -> Outline {
        let mut nodes = Vec::with_capacity(self.drafts.len());
        nodes.push(root_node(
            self.source,
            lines,
            self.hash(0, false),
            self.hash(0, true),
        ));
        for (index, draft) in self.drafts.iter().enumerate().skip(1) {
            let identity_hash = self.hash(index, true);
            let label = match &draft.name {
                Some(parts) => Label::Named(parts.clone()),
                None => Label::Hash(identity_hash[..HASH_DIGITS].to_owned()),
            };
            let start = draft
                .attached
                .iter()
                .map(Node::start_byte)
                .chain([draft.core.start_byte()])
                .min()
                .unwrap_or(0);
            let (line_start, _) = lines.range(start, draft.core.end_byte());
            let (_, line_end) = lines.range(draft.core.start_byte(), draft.core.end_byte());
            let parent = draft.parent.unwrap_or(0);
            nodes[parent].children.push(index);
            nodes.push(OutlineNode {
                kind: draft.kind,
                name: draft.name.clone(),
                segment: Segment::base(draft.kind, &label),
                selector: String::new(),
                parent: Some(parent),
                children: Vec::new(),
                line_start,
                line_end,
                declaration_line: lines.line(draft.declaration_byte),
                byte_start: draft.core.start_byte(),
                byte_end: if self.profile.trailing_comments_outside {
                    content_end(draft.core)
                } else {
                    draft.core.end_byte()
                },
                structural_hash: self.hash(index, false),
                identity_hash,
            });
        }
        disambiguate(&mut nodes);
        let comments = assign_comments(&mut nodes, self.comments, self.source, lines);
        Outline {
            profile: self.profile,
            nodes,
            comments,
        }
    }
}

/// Adds `#N` to sibling segments that would otherwise be identical, except
/// `line` nodes, and builds full selectors.
fn disambiguate(nodes: &mut [OutlineNode]) {
    for parent in 0..nodes.len() {
        let mut groups = BTreeMap::<String, Vec<usize>>::new();
        for child in &nodes[parent].children {
            groups
                .entry(nodes[*child].segment.clone())
                .or_default()
                .push(*child);
        }
        for group in groups.values().filter(|group| group.len() > 1) {
            for (ordinal, child) in group.iter().enumerate() {
                if nodes[*child].kind != "line" {
                    nodes[*child].segment = format!("{}#{}", nodes[*child].segment, ordinal + 1);
                }
            }
        }
    }
    for index in 1..nodes.len() {
        let parent = nodes[index].parent.unwrap_or(0);
        nodes[index].selector = if parent == 0 {
            nodes[index].segment.clone()
        } else {
            format!("{}/{}", nodes[parent].selector, nodes[index].segment)
        };
    }
}

/// Groups comments into runs, assigns each run's owner under the comment
/// rules, and widens owners' line ranges to cover their comments.
fn assign_comments(
    nodes: &mut [OutlineNode],
    mut drafts: Vec<DraftComment>,
    source: &str,
    lines: &LineIndex,
) -> Vec<OutlineComment> {
    drafts.sort_by_key(|comment| comment.byte_start);
    let mut runs: Vec<Vec<usize>> = Vec::new();
    for (index, comment) in drafts.iter().enumerate() {
        let joins = runs.last().and_then(|run| run.last()).is_some_and(|last| {
            let previous = &drafts[*last];
            previous.binds_to_container == comment.binds_to_container
                && !trails_code(source, previous.byte_start)
                && !trails_code(source, comment.byte_start)
                && source
                    .get(previous.byte_end..comment.byte_start)
                    .is_some_and(|gap| gap.trim().is_empty())
                && lines.line(comment.byte_start)
                    <= lines.range(previous.byte_start, previous.byte_end).1 + 1
        });
        if joins {
            runs.last_mut().expect("run exists").push(index);
        } else {
            runs.push(vec![index]);
        }
    }
    let base = nodes
        .iter()
        .map(|node| (node.line_start, node.line_end))
        .collect::<Vec<_>>();
    let mut owners = vec![0; drafts.len()];
    let mut run_of = vec![0; drafts.len()];
    for (run_index, run) in runs.iter().enumerate() {
        for index in run {
            run_of[*index] = run_index;
        }
    }
    for run in &runs {
        let first = &drafts[run[0]];
        let last = &drafts[*run.last().expect("nonempty run")];
        let (start, _) = lines.range(first.byte_start, first.byte_end);
        let (_, end) = lines.range(last.byte_start, last.byte_end);
        let owner = if trails_code(source, first.byte_start) {
            trailing_owner(nodes, &base, start, first.byte_start)
        } else {
            run_owner(
                nodes,
                (first.byte_start, last.byte_end),
                first.binds_to_container,
            )
        };
        for index in run {
            owners[*index] = owner;
        }
        if owner != 0 {
            nodes[owner].line_start = nodes[owner].line_start.min(start);
            nodes[owner].line_end = nodes[owner].line_end.max(end);
        }
    }
    drafts
        .into_iter()
        .zip(owners)
        .zip(run_of)
        .map(|((comment, owner), run)| {
            let (line_start, line_end) = lines.range(comment.byte_start, comment.byte_end);
            OutlineComment {
                byte_start: comment.byte_start,
                byte_end: comment.byte_end,
                line_start,
                line_end,
                owner,
                run,
            }
        })
        .collect()
}

/// Whether a comment follows code on its first line.
fn trails_code(source: &str, byte: usize) -> bool {
    let line_start = source[..byte].rfind('\n').map_or(0, |index| index + 1);
    !source[line_start..byte].trim().is_empty()
}

/// The owner of a comment that follows code on `line`: the innermost node
/// containing that line, by the line ranges in `base`. When several sibling
/// nodes share the line, the last one starting before the comment wins.
fn trailing_owner(
    nodes: &[OutlineNode],
    base: &[(usize, usize)],
    line: usize,
    byte: usize,
) -> usize {
    let covers = |index: usize| base[index].0 <= line && line <= base[index].1;
    let mut current = 0;
    loop {
        let next = nodes[current]
            .children
            .iter()
            .copied()
            .rev()
            .find(|child| covers(*child) && nodes[*child].byte_start < byte);
        match next {
            Some(child) => current = child,
            None => return current,
        }
    }
}

/// The end of a node's last non-comment token, so trailing comments that a
/// grammar folds into the node do not count as inside it.
fn content_end(node: Node<'_>) -> usize {
    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.children(&mut cursor).collect();
    children
        .iter()
        .rev()
        .find(|child| !child.is_extra())
        .map_or(node.end_byte(), |child| content_end(*child))
}

/// The owner of a comment run spanning `bytes` that does not follow code on
/// its line: the next child starting after it within the smallest node
/// containing it, else that container. A run that binds to its container,
/// such as a Rust inner doc comment, always belongs to the container.
fn run_owner(nodes: &[OutlineNode], bytes: (usize, usize), binds_to_container: bool) -> usize {
    let mut container = 0;
    while let Some(child) = nodes[container]
        .children
        .iter()
        .copied()
        .find(|child| nodes[*child].byte_start <= bytes.0 && bytes.1 <= nodes[*child].byte_end)
    {
        container = child;
    }
    if binds_to_container {
        return container;
    }
    nodes[container]
        .children
        .iter()
        .copied()
        .find(|child| nodes[*child].byte_start >= bytes.1)
        .unwrap_or(container)
}

impl Outline {
    /// Resolves a selector to a node index; the empty selector is the root.
    /// A `line` segment resolves to the first line with that content.
    pub(crate) fn resolve(&self, selector: &str) -> Result<usize, ResolveError> {
        let segments = parse_selector(selector).map_err(ResolveError::Invalid)?;
        let mut current = 0;
        for segment in segments {
            let wanted = segment.to_string();
            current = self.nodes[current]
                .children
                .iter()
                .copied()
                .find(|child| self.nodes[*child].segment == wanted)
                .ok_or_else(|| ResolveError::NotFound(selector.to_owned()))?;
        }
        Ok(current)
    }

    /// The innermost nodes whose line ranges cover a line: covering nodes
    /// none of whose children cover it. Several siblings can share a line.
    // {impl}`requirements.references.outlines`
    pub(crate) fn innermost_at_line(&self, line: usize) -> Vec<usize> {
        let covers = |index: usize| {
            self.nodes[index].line_start <= line && line <= self.nodes[index].line_end
        };
        if !covers(0) {
            return Vec::new();
        }
        let mut found = Vec::new();
        let mut stack = vec![0];
        while let Some(index) = stack.pop() {
            let covering = self.nodes[index]
                .children
                .iter()
                .copied()
                .filter(|child| covers(*child))
                .collect::<Vec<_>>();
            if covering.is_empty() {
                found.push(index);
            } else {
                stack.extend(covering.into_iter().rev());
            }
        }
        found
    }

    /// The owners of comments touching a line.
    pub(crate) fn comment_owners_at_line(&self, line: usize) -> Vec<usize> {
        let mut owners = self
            .comments
            .iter()
            .filter(|comment| comment.line_start <= line && line <= comment.line_end)
            .map(|comment| comment.owner)
            .collect::<Vec<_>>();
        owners.dedup();
        owners
    }

    /// The smallest node a line's source belongs to: the owner of a comment
    /// on that line, otherwise the smallest innermost node covering it, and
    /// the root when no other node covers it.
    pub(crate) fn owner_at_line(&self, line: usize) -> usize {
        if let Some(owner) = self.comment_owners_at_line(line).first() {
            return *owner;
        }
        self.innermost_at_line(line)
            .into_iter()
            .min_by_key(|index| {
                let node = &self.nodes[*index];
                (
                    node.line_end - node.line_start,
                    node.byte_end - node.byte_start,
                )
            })
            .unwrap_or(0)
    }

    /// The innermost nodes for a set of lines, without nodes whose ancestor
    /// is also selected, in source order. With `comments`, a line in a
    /// comment belongs to the comment's owner.
    pub(crate) fn nodes_for_lines(
        &self,
        lines: impl IntoIterator<Item = usize>,
        comments: bool,
    ) -> Vec<usize> {
        let mut selected = Vec::new();
        for line in lines {
            let owners = if comments {
                self.comment_owners_at_line(line)
            } else {
                Vec::new()
            };
            if owners.is_empty() {
                selected.extend(self.innermost_at_line(line));
            } else {
                selected.extend(owners);
            }
        }
        selected.sort_unstable();
        selected.dedup();
        let chosen = selected.clone();
        selected.retain(|index| {
            let mut ancestor = self.nodes[*index].parent;
            while let Some(parent) = ancestor {
                if chosen.binary_search(&parent).is_ok() {
                    return false;
                }
                ancestor = self.nodes[parent].parent;
            }
            true
        });
        selected
    }

    /// A node's source lines, over its full line range, without line endings.
    // {impl}`requirements.references.outlines`
    pub(crate) fn node_lines(&self, source: &str, index: usize) -> Vec<String> {
        let node = &self.nodes[index];
        source
            .lines()
            .map(|line| line.strip_suffix('\r').unwrap_or(line))
            .skip(node.line_start - 1)
            .take(node.line_end + 1 - node.line_start)
            .map(str::to_owned)
            .collect()
    }
}

#[cfg(test)]
mod tests;

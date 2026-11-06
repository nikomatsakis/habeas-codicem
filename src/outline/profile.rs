//! Outline profile tables: how a grammar's node kinds map onto outline nodes.

use tree_sitter::{Language, Node};

/// How an outline node of a mapped kind is named.
// {impl}`requirements.references.outlines`
#[derive(Clone, Copy)]
pub(crate) enum NameRule {
    /// Always anonymous: the segment is `kind@<identity hash>`.
    Anonymous,
    /// Named by the source text of a field, as written.
    Field(&'static str),
    /// Named by the first named child whose kind is listed, as written.
    FirstChild(&'static [&'static str]),
    /// Named by a profile-specific rule returning the name's parts, which are
    /// joined by an unencoded `/` in the segment. `None` makes the node
    /// anonymous.
    Custom(fn(Node<'_>, &str) -> Option<Vec<String>>),
}

/// One row of a profile's kind table.
// {impl}`requirements.references.outlines`
pub(crate) struct KindRule {
    /// Grammar node kind.
    pub(crate) grammar: &'static str,
    /// Short outline kind.
    pub(crate) kind: &'static str,
    /// Naming rule.
    pub(crate) name: NameRule,
    /// Optional context condition; the row only applies when it holds.
    pub(crate) when: Option<fn(Node<'_>) -> bool>,
}

impl KindRule {
    // {impl}`requirements.references.outlines`
    pub(crate) const fn new(grammar: &'static str, kind: &'static str, name: NameRule) -> Self {
        Self {
            grammar,
            kind,
            name,
            when: None,
        }
    }

    // {impl}`requirements.references.outlines`
    pub(crate) const fn when(mut self, condition: fn(Node<'_>) -> bool) -> Self {
        self.when = Some(condition);
        self
    }
}

/// A language's outline profile.
// {impl}`requirements.references.outlines`
// {impl}`requirements.references.outline_comments`
// {impl}`requirements.references.structural_hash`
pub(crate) struct Profile {
    /// Profile name, such as `rust` or `plain-text`.
    pub(crate) name: &'static str,
    /// File extensions selecting this profile, without the dot.
    pub(crate) extensions: &'static [&'static str],
    /// Grammar; `None` for the plain-text profile.
    pub(crate) language: Option<fn() -> Language>,
    /// Kind table.
    pub(crate) kinds: &'static [KindRule],
    /// Whether a grammar node is a comment.
    pub(crate) is_comment: fn(Node<'_>, &str) -> bool,
    /// Whether a comment is part of structural hashes.
    pub(crate) hashes_comment: fn(Node<'_>, &str) -> bool,
    /// A hashed comment's prose group and its text without comment
    /// delimiters; adjacent comments of one group hash as one run of words.
    pub(crate) comment_prose: fn(Node<'_>, &str) -> (&'static str, String),
    /// Whether a comment always belongs to the node enclosing it, such as a
    /// Rust inner doc comment.
    pub(crate) comment_binds_to_container: fn(Node<'_>, &str) -> bool,
    /// Whether comments a grammar folds into the end of a node lie outside it,
    /// as in YAML, where a comment before a key at the same indentation
    /// trails the previous key's node.
    pub(crate) trailing_comments_outside: bool,
    /// Whether a preceding grammar sibling attaches to the next outline node.
    pub(crate) attaches: fn(Node<'_>, &str) -> bool,
}

impl std::fmt::Debug for Profile {
    // {impl}`requirements.references.outlines`
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.name)
    }
}

impl Profile {
    // {impl}`requirements.references.outlines`
    pub(crate) fn rule(&self, node: Node<'_>) -> Option<&KindRule> {
        self.kinds.iter().find(|rule| {
            rule.grammar == node.kind() && rule.when.is_none_or(|condition| condition(node))
        })
    }
}

// {impl}`requirements.references.outlines`
// {impl}`requirements.references.outline_comments`
pub(crate) fn kind_is_comment(node: Node<'_>, _source: &str) -> bool {
    node.kind() == "comment"
}

// {impl}`requirements.references.structural_hash`
pub(crate) fn never(_node: Node<'_>, _source: &str) -> bool {
    false
}

/// Comment prose for profiles that hash no comments: the whole text.
// {impl}`requirements.references.structural_hash`
pub(crate) fn whole_comment(node: Node<'_>, source: &str) -> (&'static str, String) {
    ("comment", text(node, source).to_owned())
}

// {impl}`requirements.references.outlines`
pub(crate) fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    source.get(node.byte_range()).unwrap_or("")
}

// {impl}`requirements.references.outlines`
pub(crate) fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every profile, in lookup order; the plain-text profile is the fallback.
// {impl}`requirements.references.outlines`
pub(crate) fn profiles() -> [&'static Profile; 8] {
    [
        &super::rust::PROFILE,
        &super::python::PROFILE,
        &super::typescript::PROFILE,
        &super::typescript::TSX_PROFILE,
        &super::css::PROFILE,
        &super::markdown::PROFILE,
        &super::toml::PROFILE,
        &super::yaml::PROFILE,
    ]
}

/// Selects the profile for a path by its extension.
// {impl}`requirements.references.outlines`
pub(crate) fn profile_for_path(path: &std::path::Path) -> &'static Profile {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return &super::plain::PROFILE;
    };
    let extension = extension.to_ascii_lowercase();
    profiles()
        .into_iter()
        .find(|profile| profile.extensions.contains(&extension.as_str()))
        .unwrap_or(&super::plain::PROFILE)
}

//! Outline profile tables: how a grammar's node kinds map onto outline nodes.

use tree_sitter::{Language, Node};

/// A profile-specific naming rule: the selector's name parts and the source to highlight.
pub type CustomName = fn(Node<'_>, &str) -> Option<(Vec<String>, std::ops::Range<usize>)>;

/// How an outline node of a mapped kind is named.
#[derive(Clone, Copy)]
// {impl}`requirements.references.outlines`
pub enum NameRule {
    /// Always anonymous: the segment is `kind@<identity hash>`.
    Anonymous,
    /// Named by the source text of a field, as written.
    Field(&'static str),
    /// Named by the first named child whose kind is listed, as written.
    FirstChild(&'static [&'static str]),
    /// Named by a profile-specific rule returning the name's parts, which are
    /// joined by an unencoded `/` in the segment, and the source range to highlight. `None` makes the node anonymous.
    Custom(CustomName),
}

impl NameRule {
    /// Resolve the selector name and the source range that supplies it.
    // {impl}`requirements.sdk.outlines.name_span`
    pub(crate) fn resolve(
        self,
        node: Node<'_>,
        source: &str,
    ) -> Option<(Vec<String>, std::ops::Range<usize>)> {
        let child = match self {
            Self::Anonymous => return None,
            Self::Custom(rule) => return rule(node, source),
            Self::Field(field) => node.child_by_field_name(field)?,
            Self::FirstChild(kinds) => node
                .named_children(&mut node.walk())
                .find(|child| kinds.contains(&child.kind()))?,
        };
        Some((vec![text(child, source).to_owned()], child.byte_range()))
    }
}

/// One row of a profile's kind table.
// {impl}`requirements.references.outlines`
pub struct KindRule {
    /// Grammar node kind.
    pub grammar: &'static str,
    /// Short outline kind.
    pub kind: &'static str,
    /// Naming rule.
    pub name: NameRule,
    /// Optional context condition; the row only applies when it holds.
    pub when: Option<fn(Node<'_>) -> bool>,
}

impl KindRule {
    // {cite}`habeas:util`
    pub const fn new(grammar: &'static str, kind: &'static str, name: NameRule) -> Self {
        Self {
            grammar,
            kind,
            name,
            when: None,
        }
    }

    // {cite}`habeas:util`
    pub const fn when(mut self, condition: fn(Node<'_>) -> bool) -> Self {
        self.when = Some(condition);
        self
    }
}

/// A language's outline profile.
// {impl}`requirements.references.outlines`
pub struct Profile {
    /// Character used to parse structural-search metavariables.
    pub expando_char: char,
    /// Profile name, such as `rust` or `plain-text`.
    pub name: &'static str,
    /// File extensions selecting this profile, without the dot.
    pub extensions: &'static [&'static str],
    /// Grammar; `None` for the plain-text profile.
    pub language: Option<fn() -> Language>,
    /// Kind table.
    pub kinds: &'static [KindRule],
    /// Whether a grammar node is a comment.
    pub is_comment: fn(Node<'_>, &str) -> bool,
    /// Whether a comment is part of structural hashes.
    pub hashes_comment: fn(Node<'_>, &str) -> bool,
    /// A hashed comment's prose group and its text without comment
    /// delimiters, given the comment node and its redacted text; adjacent
    /// comments of one group hash as one run of words.
    pub comment_prose: fn(Node<'_>, &str) -> (&'static str, String),
    /// Whether a comment always belongs to the node enclosing it, such as a
    /// Rust inner doc comment.
    pub comment_binds_to_container: fn(Node<'_>, &str) -> bool,
    /// Whether comments a grammar folds into the end of a node lie outside it,
    /// as in YAML, where a comment before a key at the same indentation
    /// trails the previous key's node.
    pub trailing_comments_outside: bool,
    /// Whether a preceding grammar sibling attaches to the next outline node.
    pub attaches: fn(Node<'_>, &str) -> bool,
}

impl std::fmt::Debug for Profile {
    // {impl}`requirements.references.outlines`
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.name)
    }
}

impl Profile {
    // {cite}`habeas:util`
    pub fn rule(&self, node: Node<'_>) -> Option<&KindRule> {
        self.kinds.iter().find(|rule| {
            rule.grammar == node.kind() && rule.when.is_none_or(|condition| condition(node))
        })
    }
}

// {cite}`habeas:util`
pub fn kind_is_comment(node: Node<'_>, _source: &str) -> bool {
    node.kind() == "comment"
}

// {cite}`habeas:util`
pub fn never(_node: Node<'_>, _source: &str) -> bool {
    false
}

/// Comment prose for profiles that hash no comments: the whole text.
// :::{cite} habeas:coverage
// This callback fills the Profile interface for every non-Rust profile.
// Those profiles all set hashes_comment to never, so Builder::encode skips
// comment_prose for every source input; plain text does not use the grammar
// encoder at all. Rust hashes doc comments through its separate doc_prose.
// The fallback therefore has no reachable behavior in the supported profiles.
// :::
pub fn whole_comment(_node: Node<'_>, body: &str) -> (&'static str, String) {
    ("comment", body.to_owned())
}

// {cite}`habeas:util`
pub fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    source.get(node.byte_range()).unwrap_or("")
}

// {cite}`habeas:util`
pub fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A language plugin supplies its grammar and outline interpretation. Providers
/// are process-lifetime registrations; they do not fetch or own source content.
// ANCHOR: language-provider
pub trait LanguageProvider: Sync {
    fn profile(&'static self) -> &'static Profile;
    fn grammar(&self) -> fn() -> Language;
}
// ANCHOR_END: language-provider

/// Select providers independently of parsing. Registration order breaks ties;
/// an application can override a default by placing its provider first.
// {cite}`habeas:util`
pub struct Registry {
    providers: Vec<&'static dyn LanguageProvider>,
}

impl Registry {
    // {cite}`habeas:util`
    pub fn new(providers: impl IntoIterator<Item = &'static dyn LanguageProvider>) -> Self {
        Self {
            providers: providers.into_iter().collect(),
        }
    }

    // {cite}`habeas:util`
    pub fn named(&self, name: &str) -> Option<&'static dyn LanguageProvider> {
        self.providers
            .iter()
            .copied()
            .find(|p| p.profile().name == name)
    }

    // {cite}`habeas:util`
    pub fn for_path(&self, path: &std::path::Path) -> Option<&'static dyn LanguageProvider> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        self.providers
            .iter()
            .copied()
            .find(|p| p.profile().extensions.contains(&extension.as_str()))
    }
}

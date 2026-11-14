//! Outline profile tables: how a grammar's node kinds map onto outline nodes.

use tree_sitter::{Language, Node};

/// A profile-specific naming rule: the name's parts and the byte where the
/// name begins.
pub type CustomName = fn(Node<'_>, &str) -> Option<(Vec<String>, usize)>;

/// How an outline node of a mapped kind is named.
#[derive(Clone, Copy)]
pub enum NameRule {
    /// Always anonymous: the segment is `kind@<identity hash>`.
    Anonymous,
    /// Named by the source text of a field, as written.
    Field(&'static str),
    /// Named by the first named child whose kind is listed, as written.
    FirstChild(&'static [&'static str]),
    /// Named by a profile-specific rule returning the name's parts, which are
    /// joined by an unencoded `/` in the segment, and the byte where the name
    /// begins in the source (the node's start when the name is not written
    /// there). `None` makes the node anonymous.
    Custom(CustomName),
}

/// One row of a profile's kind table.
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
    pub const fn new(grammar: &'static str, kind: &'static str, name: NameRule) -> Self {
        Self {
            grammar,
            kind,
            name,
            when: None,
        }
    }

    pub const fn when(mut self, condition: fn(Node<'_>) -> bool) -> Self {
        self.when = Some(condition);
        self
    }
}

/// A language's outline profile.
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
    pub fn rule(&self, node: Node<'_>) -> Option<&KindRule> {
        self.kinds.iter().find(|rule| {
            rule.grammar == node.kind() && rule.when.is_none_or(|condition| condition(node))
        })
    }
}

pub fn kind_is_comment(node: Node<'_>, _source: &str) -> bool {
    node.kind() == "comment"
}

pub fn never(_node: Node<'_>, _source: &str) -> bool {
    false
}

/// Comment prose for profiles that hash no comments: the whole text.
pub fn whole_comment(_node: Node<'_>, body: &str) -> (&'static str, String) {
    ("comment", body.to_owned())
}

pub fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    source.get(node.byte_range()).unwrap_or("")
}

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
pub struct Registry {
    providers: Vec<&'static dyn LanguageProvider>,
}

impl Registry {
    pub fn new(providers: impl IntoIterator<Item = &'static dyn LanguageProvider>) -> Self {
        Self {
            providers: providers.into_iter().collect(),
        }
    }

    pub fn named(&self, name: &str) -> Option<&'static dyn LanguageProvider> {
        self.providers
            .iter()
            .copied()
            .find(|p| p.profile().name == name)
    }

    pub fn for_path(&self, path: &std::path::Path) -> Option<&'static dyn LanguageProvider> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        self.providers
            .iter()
            .copied()
            .find(|p| p.profile().extensions.contains(&extension.as_str()))
    }
}

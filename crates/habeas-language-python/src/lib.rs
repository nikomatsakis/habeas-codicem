//! Python outline profile.

use tree_sitter::Node;

use habeas_language::{KindRule, NameRule, Profile, kind_is_comment, never};

/// Decorators attach to the decorated definition.
// {cite}`habeas:util`
fn attaches(node: Node<'_>, _source: &str) -> bool {
    node.kind() == "decorator"
}

const KINDS: &[KindRule] = &[
    KindRule::new("function_definition", "def", NameRule::Field("name")),
    KindRule::new("class_definition", "class", NameRule::Field("name")),
];

// {impl}`requirements.references.outline_comments`
// {impl}`requirements.references.structural_hash`
pub static PROFILE: Profile = Profile {
    name: "python",
    expando_char: 'µ',
    extensions: &["py", "pyi"],
    language: Some(|| tree_sitter_python::LANGUAGE.into()),
    kinds: KINDS,
    is_comment: kind_is_comment,
    hashes_comment: never,
    comment_prose: habeas_language::whole_comment,
    comment_binds_to_container: never,
    trailing_comments_outside: false,
    attaches,
};

/// Tree-sitter-backed python provider.
pub struct Provider;

impl habeas_language::LanguageProvider for Provider {
    fn profile(&'static self) -> &'static Profile {
        &PROFILE
    }
    fn grammar(&self) -> fn() -> tree_sitter::Language {
        PROFILE.language.expect("language grammar")
    }
}

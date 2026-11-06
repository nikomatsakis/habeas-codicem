//! Python outline profile.

use tree_sitter::Node;

use super::profile::{KindRule, NameRule, Profile, kind_is_comment, never};

/// Decorators attach to the decorated definition.
// {impl}`requirements.references.outlines`
fn attaches(node: Node<'_>, _source: &str) -> bool {
    node.kind() == "decorator"
}

// {impl}`requirements.references.outlines`
const KINDS: &[KindRule] = &[
    KindRule::new("function_definition", "def", NameRule::Field("name")),
    KindRule::new("class_definition", "class", NameRule::Field("name")),
];

// {impl}`requirements.references.outlines`
// {impl}`requirements.references.outline_comments`
pub(crate) static PROFILE: Profile = Profile {
    name: "python",
    extensions: &["py", "pyi"],
    language: Some(|| tree_sitter_python::LANGUAGE.into()),
    kinds: KINDS,
    is_comment: kind_is_comment,
    hashes_comment: never,
    comment_prose: super::profile::whole_comment,
    comment_binds_to_container: never,
    attaches,
};

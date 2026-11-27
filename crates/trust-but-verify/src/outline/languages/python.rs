//! Python outline profile.

use tree_sitter::Node;

use crate::outline_engine::language::AttachmentTarget;
use crate::outline_engine::language::KindRule;
use crate::outline_engine::language::NameRule;
use crate::outline_engine::language::Profile;
use crate::outline_engine::language::kind_is_comment;
use crate::outline_engine::language::never;

/// Decorators attach to the decorated definition.
// {cite}`habeas:util`
fn attachment_target(node: Node<'_>) -> Option<AttachmentTarget> {
    (node.kind() == "decorator").then_some(AttachmentTarget::FollowingItem)
}

const KINDS: &[KindRule] = &[
    KindRule::new("function_definition", "def", NameRule::Field("name")),
    KindRule::new("class_definition", "class", NameRule::Field("name")),
];

// {impl}`requirements.references.outline_comments`
// {impl}`requirements.references.structural_hash`
// {impl}`requirements.sdk.sources.python.attachments`
pub static PROFILE: Profile = Profile {
    name: "python",
    fingerprint_scheme: "python-structural-v1",
    expando_char: 'µ',
    extensions: &["py", "pyi"],
    language: Some(|| tree_sitter_python::LANGUAGE.into()),
    kinds: KINDS,
    is_comment: kind_is_comment,
    hashes_comment: never,
    comment_prose: crate::outline_engine::language::whole_comment,
    comment_binds_to_container: never,
    trailing_comments_outside: false,
    attachment_target,
};

/// Tree-sitter-backed python provider.
// {cite}`habeas:util`
pub struct Provider;

impl crate::outline_engine::language::LanguageProvider for Provider {
    // {cite}`habeas:util`
    fn profile(&'static self) -> &'static Profile {
        &PROFILE
    }
    // {cite}`habeas:util`
    fn grammar(&self) -> fn() -> tree_sitter::Language {
        PROFILE.language.expect("language grammar")
    }
}

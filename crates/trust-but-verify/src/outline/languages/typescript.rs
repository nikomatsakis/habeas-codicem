//! TypeScript and TSX outline profiles.

use crate::outline_engine::language::AttachmentTarget;
use crate::outline_engine::language::KindRule;
use crate::outline_engine::language::NameRule;
use crate::outline_engine::language::Profile;
use crate::outline_engine::language::kind_is_comment;
use crate::outline_engine::language::never;

// {impl}`requirements.sdk.sources.typescript.attachments.decorators`
fn attachment_target(node: tree_sitter::Node<'_>) -> Option<AttachmentTarget> {
    if node.kind() != "decorator" {
        return None;
    }
    match node.parent()?.kind() {
        "class_declaration" | "abstract_class_declaration" => Some(AttachmentTarget::Parent),
        "class_body" => Some(AttachmentTarget::FollowingItem),
        "export_statement" => Some(AttachmentTarget::SiblingField("declaration")),
        _ => None,
    }
}

const KINDS: &[KindRule] = &[
    KindRule::new("function_declaration", "function", NameRule::Field("name")),
    KindRule::new(
        "generator_function_declaration",
        "function",
        NameRule::Field("name"),
    ),
    KindRule::new("function_signature", "function", NameRule::Field("name")),
    KindRule::new("class_declaration", "class", NameRule::Field("name")),
    KindRule::new(
        "abstract_class_declaration",
        "class",
        NameRule::Field("name"),
    ),
    KindRule::new("method_definition", "method", NameRule::Field("name")),
    KindRule::new("method_signature", "method", NameRule::Field("name")),
    KindRule::new(
        "abstract_method_signature",
        "method",
        NameRule::Field("name"),
    ),
    KindRule::new(
        "interface_declaration",
        "interface",
        NameRule::Field("name"),
    ),
    KindRule::new("type_alias_declaration", "type", NameRule::Field("name")),
    KindRule::new("enum_declaration", "enum", NameRule::Field("name")),
    KindRule::new("internal_module", "namespace", NameRule::Field("name")),
    KindRule::new("module", "namespace", NameRule::Field("name")).when(|node| node.is_named()),
];

// {impl}`requirements.references.outline_comments`
// {impl}`requirements.sdk.sources.typescript.attachments`
// {impl}`requirements.sdk.sources.typescript.attachments.comments`
const TYPESCRIPT: Profile = Profile {
    name: "typescript",
    fingerprint_scheme: "typescript-structural-v2",
    expando_char: '$',
    extensions: &["ts", "mts", "cts"],
    language: Some(|| tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
    kinds: KINDS,
    is_comment: kind_is_comment,
    hashes_comment: never,
    comment_prose: crate::outline_engine::language::whole_comment,
    comment_binds_to_container: never,
    trailing_comments_outside: false,
    attachment_target,
};

pub static PROFILE: Profile = TYPESCRIPT;

// {impl}`requirements.references.outline_comments`
// {impl}`requirements.references.structural_hash`
pub static TSX_PROFILE: Profile = Profile {
    name: "tsx",
    fingerprint_scheme: "tsx-structural-v2",
    expando_char: '$',
    extensions: &["tsx"],
    language: Some(|| tree_sitter_typescript::LANGUAGE_TSX.into()),
    ..TYPESCRIPT
};

/// Tree-sitter-backed typescript provider.
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

/// Tree-sitter-backed typescript provider.
// {cite}`habeas:util`
pub struct TsxProvider;

impl crate::outline_engine::language::LanguageProvider for TsxProvider {
    // {cite}`habeas:util`
    fn profile(&'static self) -> &'static Profile {
        &TSX_PROFILE
    }
    // {cite}`habeas:util`
    fn grammar(&self) -> fn() -> tree_sitter::Language {
        TSX_PROFILE.language.expect("language grammar")
    }
}

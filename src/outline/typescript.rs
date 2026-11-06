//! TypeScript and TSX outline profiles.

use super::profile::{KindRule, NameRule, Profile, kind_is_comment, never};

// {impl}`requirements.references.outlines`
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
    KindRule::new("module", "namespace", NameRule::Field("name")),
];

// {impl}`requirements.references.outlines`
// {impl}`requirements.references.outline_comments`
// {impl}`requirements.references.structural_hash`
const TYPESCRIPT: Profile = Profile {
    name: "typescript",
    extensions: &["ts", "mts", "cts"],
    language: Some(|| tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
    kinds: KINDS,
    is_comment: kind_is_comment,
    hashes_comment: never,
    comment_prose: super::profile::whole_comment,
    comment_binds_to_container: never,
    attaches: never,
};

// {impl}`requirements.references.outlines`
pub(crate) static PROFILE: Profile = TYPESCRIPT;

// {impl}`requirements.references.outlines`
// {impl}`requirements.references.outline_comments`
pub(crate) static TSX_PROFILE: Profile = Profile {
    name: "tsx",
    extensions: &["tsx"],
    language: Some(|| tree_sitter_typescript::LANGUAGE_TSX.into()),
    ..TYPESCRIPT
};

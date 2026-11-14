//! TypeScript and TSX outline profiles.

use habeas_language::{KindRule, NameRule, Profile, kind_is_comment, never};

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

// {impl}`requirements.references.outline_comments`
const TYPESCRIPT: Profile = Profile {
    name: "typescript",
    expando_char: '$',
    extensions: &["ts", "mts", "cts"],
    language: Some(|| tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
    kinds: KINDS,
    is_comment: kind_is_comment,
    hashes_comment: never,
    comment_prose: habeas_language::whole_comment,
    comment_binds_to_container: never,
    trailing_comments_outside: false,
    attaches: never,
};

pub static PROFILE: Profile = TYPESCRIPT;

// {impl}`requirements.references.outline_comments`
// {impl}`requirements.references.structural_hash`
pub static TSX_PROFILE: Profile = Profile {
    name: "tsx",
    expando_char: '$',
    extensions: &["tsx"],
    language: Some(|| tree_sitter_typescript::LANGUAGE_TSX.into()),
    ..TYPESCRIPT
};

/// Tree-sitter-backed typescript provider.
pub struct Provider;

impl habeas_language::LanguageProvider for Provider {
    fn profile(&'static self) -> &'static Profile {
        &PROFILE
    }
    fn grammar(&self) -> fn() -> tree_sitter::Language {
        PROFILE.language.expect("language grammar")
    }
}

/// Tree-sitter-backed typescript provider.
pub struct TsxProvider;

impl habeas_language::LanguageProvider for TsxProvider {
    fn profile(&'static self) -> &'static Profile {
        &TSX_PROFILE
    }
    fn grammar(&self) -> fn() -> tree_sitter::Language {
        TSX_PROFILE.language.expect("language grammar")
    }
}

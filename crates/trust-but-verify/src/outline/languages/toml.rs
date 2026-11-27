//! TOML outline profile.

use crate::outline_engine::language::KindRule;
use crate::outline_engine::language::NameRule;
use crate::outline_engine::language::Profile;
use crate::outline_engine::language::kind_is_comment;
use crate::outline_engine::language::never;

const KEYS: &[&str] = &["bare_key", "quoted_key", "dotted_key"];

const KINDS: &[KindRule] = &[
    KindRule::new("table", "table", NameRule::FirstChild(KEYS)),
    KindRule::new("table_array_element", "table", NameRule::FirstChild(KEYS)),
    KindRule::new("pair", "pair", NameRule::FirstChild(KEYS)),
];

// {impl}`requirements.references.outline_comments`
// {impl}`requirements.references.structural_hash`
// {impl}`requirements.sdk.sources.toml.attachments`
pub static PROFILE: Profile = Profile {
    name: "toml",
    expando_char: '_',
    extensions: &["toml"],
    language: Some(|| tree_sitter_toml_ng::LANGUAGE.into()),
    kinds: KINDS,
    is_comment: kind_is_comment,
    hashes_comment: never,
    comment_prose: crate::outline_engine::language::whole_comment,
    comment_binds_to_container: never,
    trailing_comments_outside: false,
    attaches: never,
};

/// Tree-sitter-backed toml provider.
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

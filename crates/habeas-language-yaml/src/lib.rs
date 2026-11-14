//! YAML outline profile.

use habeas_language::{KindRule, NameRule, Profile, kind_is_comment, never};

const KINDS: &[KindRule] = &[
    KindRule::new("block_mapping_pair", "pair", NameRule::Field("key")),
    KindRule::new("flow_pair", "pair", NameRule::Field("key")),
    KindRule::new("block_sequence_item", "item", NameRule::Anonymous),
];

// {impl}`requirements.references.outline_comments`
// {impl}`requirements.references.structural_hash`
pub static PROFILE: Profile = Profile {
    name: "yaml",
    expando_char: '$',
    extensions: &["yml", "yaml"],
    language: Some(|| tree_sitter_yaml::LANGUAGE.into()),
    kinds: KINDS,
    is_comment: kind_is_comment,
    hashes_comment: never,
    comment_prose: habeas_language::whole_comment,
    comment_binds_to_container: never,
    trailing_comments_outside: true,
    attaches: never,
};

/// Tree-sitter-backed yaml provider.
pub struct Provider;

impl habeas_language::LanguageProvider for Provider {
    fn profile(&'static self) -> &'static Profile {
        &PROFILE
    }
    fn grammar(&self) -> fn() -> tree_sitter::Language {
        PROFILE.language.expect("language grammar")
    }
}

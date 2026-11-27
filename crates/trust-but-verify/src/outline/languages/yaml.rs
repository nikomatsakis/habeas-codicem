//! YAML outline profile.

use crate::outline_engine::language::KindRule;
use crate::outline_engine::language::NameRule;
use crate::outline_engine::language::Profile;
use crate::outline_engine::language::kind_is_comment;
use crate::outline_engine::language::never;

const KINDS: &[KindRule] = &[
    KindRule::new("block_mapping_pair", "pair", NameRule::Field("key")),
    KindRule::new("flow_pair", "pair", NameRule::Field("key")),
    KindRule::new("block_sequence_item", "item", NameRule::Anonymous),
];

// {impl}`requirements.references.outline_comments`
// {impl}`requirements.references.structural_hash`
// {impl}`requirements.sdk.sources.yaml.attachments`
pub static PROFILE: Profile = Profile {
    name: "yaml",
    expando_char: '$',
    extensions: &["yml", "yaml"],
    language: Some(|| tree_sitter_yaml::LANGUAGE.into()),
    kinds: KINDS,
    is_comment: kind_is_comment,
    hashes_comment: never,
    comment_prose: crate::outline_engine::language::whole_comment,
    comment_binds_to_container: never,
    trailing_comments_outside: true,
    attaches: never,
};

/// Tree-sitter-backed yaml provider.
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

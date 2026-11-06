//! YAML outline profile.

use super::profile::{KindRule, NameRule, Profile, kind_is_comment, never};

// {impl}`requirements.references.outlines`
const KINDS: &[KindRule] = &[
    KindRule::new("block_mapping_pair", "pair", NameRule::Field("key")),
    KindRule::new("flow_pair", "pair", NameRule::Field("key")),
    KindRule::new("block_sequence_item", "item", NameRule::Anonymous),
];

// {impl}`requirements.references.outlines`
pub(crate) static PROFILE: Profile = Profile {
    name: "yaml",
    extensions: &["yml", "yaml"],
    language: Some(|| tree_sitter_yaml::LANGUAGE.into()),
    kinds: KINDS,
    is_comment: kind_is_comment,
    hashes_comment: never,
    comment_prose: super::profile::whole_comment,
    comment_binds_to_container: never,
    attaches: never,
};

//! TOML outline profile.

use super::profile::{KindRule, NameRule, Profile, kind_is_comment, never};

// {impl}`requirements.references.outlines`
const KEYS: &[&str] = &["bare_key", "quoted_key", "dotted_key"];

// {impl}`requirements.references.outlines`
const KINDS: &[KindRule] = &[
    KindRule::new("table", "table", NameRule::FirstChild(KEYS)),
    KindRule::new("table_array_element", "table", NameRule::FirstChild(KEYS)),
    KindRule::new("pair", "pair", NameRule::FirstChild(KEYS)),
];

// {impl}`requirements.references.outlines`
pub(crate) static PROFILE: Profile = Profile {
    name: "toml",
    extensions: &["toml"],
    language: Some(|| tree_sitter_toml_ng::LANGUAGE.into()),
    kinds: KINDS,
    fallback: "item",
    is_comment: kind_is_comment,
    hashes_comment: never,
    comment_prose: super::profile::whole_comment,
    comment_binds_to_container: never,
    attaches: never,
    prose: false,
};

//! Plain-text profile: every non-blank line is an anonymous `line` node.
//!
//! :::{cite} habeas:coverage
//! This file declares a static profile. Plain text has no grammar and never
//! invokes the grammar-node attachment callback. LLVM emits no line counts
//! for the initializer. The public `plain_line_fingerprints_use_sha256_words_without_losing_boundaries`
//! test exercises this profile through `OutlineContent::from_source`, including
//! its fingerprint scheme, whitespace normalization, and distinct word boundaries.
//! :::

use super::profile::{Profile, kind_is_comment, never};

// {impl}`requirements.references.outlines`
pub static PROFILE: Profile = Profile {
    expando_char: '$',
    name: "plain-text",
    fingerprint_scheme: "plain-text-structural-v1",
    extensions: &[],
    language: None,
    kinds: &[],
    is_comment: kind_is_comment,
    hashes_comment: never,
    comment_prose: super::profile::whole_comment,
    comment_binds_to_container: never,
    trailing_comments_outside: false,
    attachment_target: |_| None,
};

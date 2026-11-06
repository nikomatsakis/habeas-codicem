//! Plain-text profile: every non-blank line is an anonymous `line` node.

use super::profile::{Profile, kind_is_comment, never};

// {impl}`requirements.references.outlines`
pub(crate) static PROFILE: Profile = Profile {
    name: "plain-text",
    extensions: &[],
    language: None,
    kinds: &[],
    is_comment: kind_is_comment,
    hashes_comment: never,
    comment_prose: super::profile::whole_comment,
    comment_binds_to_container: never,
    attaches: never,
};

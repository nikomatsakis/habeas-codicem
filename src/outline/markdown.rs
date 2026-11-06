//! Markdown outline profile, using the block grammar.

use tree_sitter::Node;

use super::profile::{KindRule, NameRule, Profile, never, normalized, text};

/// Names a section by its heading's normalised text; a section without a
/// heading, before the first heading, is anonymous.
// {impl}`requirements.references.outlines`
fn heading_name(node: Node<'_>, source: &str) -> Option<Vec<String>> {
    let mut cursor = node.walk();
    let heading = node
        .named_children(&mut cursor)
        .find(|child| matches!(child.kind(), "atx_heading" | "setext_heading"))?;
    let content = heading.child_by_field_name("heading_content")?;
    Some(vec![normalized(text(content, source))])
}

/// HTML comment blocks are Markdown's comments.
// {impl}`requirements.references.outline_comments`
fn is_comment(node: Node<'_>, source: &str) -> bool {
    let body = text(node, source).trim();
    node.kind() == "html_block" && body.starts_with("<!--") && body.ends_with("-->")
}

// {impl}`requirements.references.outlines`
const KINDS: &[KindRule] = &[KindRule::new(
    "section",
    "section",
    NameRule::Custom(heading_name),
)];

// {impl}`requirements.references.outlines`
pub(crate) static PROFILE: Profile = Profile {
    name: "markdown",
    extensions: &["md", "markdown"],
    language: Some(|| tree_sitter_md::LANGUAGE.into()),
    kinds: KINDS,
    is_comment,
    hashes_comment: never,
    comment_prose: super::profile::whole_comment,
    comment_binds_to_container: never,
    attaches: never,
};

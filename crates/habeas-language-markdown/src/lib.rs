//! Markdown outline profile, using the block grammar.

use tree_sitter::Node;

use habeas_language::{KindRule, NameRule, Profile, never, normalized, text};

/// Names a section by its heading's normalised text; a section without a
/// heading, before the first heading, is anonymous.
fn heading_name(node: Node<'_>, source: &str) -> Option<(Vec<String>, usize)> {
    let mut cursor = node.walk();
    let heading = node
        .named_children(&mut cursor)
        .find(|child| matches!(child.kind(), "atx_heading" | "setext_heading"))?;
    let content = heading.child_by_field_name("heading_content")?;
    Some((
        vec![normalized(text(content, source))],
        content.start_byte(),
    ))
}

/// HTML comment blocks are Markdown's comments.
// {impl}`requirements.references.outline_comments`
fn is_comment(node: Node<'_>, source: &str) -> bool {
    let body = text(node, source).trim();
    node.kind() == "html_block" && body.starts_with("<!--") && body.ends_with("-->")
}

const KINDS: &[KindRule] = &[KindRule::new(
    "section",
    "section",
    NameRule::Custom(heading_name),
)];

// {impl}`requirements.references.outline_comments`
// {impl}`requirements.references.structural_hash`
pub static PROFILE: Profile = Profile {
    name: "markdown",
    expando_char: '$',
    extensions: &["md", "markdown"],
    language: Some(|| tree_sitter_md::LANGUAGE.into()),
    kinds: KINDS,
    is_comment,
    hashes_comment: never,
    comment_prose: habeas_language::whole_comment,
    comment_binds_to_container: never,
    trailing_comments_outside: false,
    attaches: never,
};

/// Tree-sitter-backed markdown provider.
pub struct Provider;

impl habeas_language::LanguageProvider for Provider {
    fn profile(&'static self) -> &'static Profile {
        &PROFILE
    }
    fn grammar(&self) -> fn() -> tree_sitter::Language {
        PROFILE.language.expect("language grammar")
    }
}

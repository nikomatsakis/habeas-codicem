//! CSS outline profile.

use tree_sitter::Node;

use super::profile::{KindRule, NameRule, Profile, kind_is_comment, never, normalized, text};

/// Names an at-rule by its normalised prelude: the text between the at-keyword
/// and the block.
// {cite}`habeas:util`
fn prelude_name(node: Node<'_>, source: &str) -> Option<(Vec<String>, usize)> {
    let keyword = node.child(0)?;
    let mut cursor = node.walk();
    let block = node
        .children(&mut cursor)
        .find(|child| child.kind() == "block")?;
    let raw = source.get(keyword.end_byte()..block.start_byte())?;
    let start = keyword.end_byte() + (raw.len() - raw.trim_start().len());
    let prelude = normalized(raw);
    (!prelude.is_empty()).then(|| (vec![prelude], start))
}

// {impl}`requirements.references.outline_comments`
fn is_comment(node: Node<'_>, source: &str) -> bool {
    kind_is_comment(node, source) || node.kind() == "js_comment"
}

/// Names a rule set by its normalised selector list.
// {cite}`habeas:util`
fn selectors_name(node: Node<'_>, source: &str) -> Option<(Vec<String>, usize)> {
    let mut cursor = node.walk();
    let selectors = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "selectors")?;
    Some((
        vec![normalized(text(selectors, source))],
        selectors.start_byte(),
    ))
}

const KINDS: &[KindRule] = &[
    KindRule::new("rule_set", "rule", NameRule::Custom(selectors_name)),
    KindRule::new("media_statement", "media", NameRule::Custom(prelude_name)),
    KindRule::new(
        "supports_statement",
        "supports",
        NameRule::Custom(prelude_name),
    ),
    KindRule::new(
        "keyframes_statement",
        "keyframes",
        NameRule::FirstChild(&["keyframes_name"]),
    ),
];

// {impl}`requirements.references.outline_comments`
// {impl}`requirements.references.structural_hash`
pub(crate) static PROFILE: Profile = Profile {
    name: "css",
    extensions: &["css"],
    language: Some(|| tree_sitter_css::LANGUAGE.into()),
    kinds: KINDS,
    is_comment,
    hashes_comment: never,
    comment_prose: super::profile::whole_comment,
    comment_binds_to_container: never,
    trailing_comments_outside: false,
    attaches: never,
};

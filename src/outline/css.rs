//! CSS outline profile.

use tree_sitter::Node;

use super::profile::{KindRule, NameRule, Profile, kind_is_comment, never, normalized, text};

/// Names an at-rule by its normalised prelude: the text between the at-keyword
/// and the block.
// {impl}`requirements.references.outlines`
fn prelude_name(node: Node<'_>, source: &str) -> Option<Vec<String>> {
    let keyword = node.child(0)?;
    let mut cursor = node.walk();
    let block = node
        .children(&mut cursor)
        .find(|child| child.kind() == "block")?;
    let prelude = normalized(source.get(keyword.end_byte()..block.start_byte())?);
    (!prelude.is_empty()).then(|| vec![prelude])
}

// {impl}`requirements.references.outlines`
fn is_comment(node: Node<'_>, source: &str) -> bool {
    kind_is_comment(node, source) || node.kind() == "js_comment"
}

/// Names a rule set by its normalised selector list.
// {impl}`requirements.references.outlines`
fn selectors_name(node: Node<'_>, source: &str) -> Option<Vec<String>> {
    let mut cursor = node.walk();
    let selectors = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "selectors")?;
    Some(vec![normalized(text(selectors, source))])
}

// {impl}`requirements.references.outlines`
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

// {impl}`requirements.references.outlines`
pub(crate) static PROFILE: Profile = Profile {
    name: "css",
    extensions: &["css"],
    language: Some(|| tree_sitter_css::LANGUAGE.into()),
    kinds: KINDS,
    fallback: "stmt",
    is_comment,
    hashes_comment: never,
    comment_binds_to_container: never,
    attaches: never,
    prose: false,
};

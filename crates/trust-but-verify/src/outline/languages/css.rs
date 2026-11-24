//! CSS outline profile.

use tree_sitter::Node;

use crate::outline_engine::language::KindRule;
use crate::outline_engine::language::NameRule;
use crate::outline_engine::language::Profile;
use crate::outline_engine::language::kind_is_comment;
use crate::outline_engine::language::never;
use crate::outline_engine::language::normalized;
use crate::outline_engine::language::text;

/// Names an at-rule by its normalised prelude: the text between the at-keyword
/// and the block.
// {cite}`habeas:util`
fn prelude_name(node: Node<'_>, source: &str) -> Option<(Vec<String>, std::ops::Range<usize>)> {
    let keyword = node.child(0)?;
    let mut cursor = node.walk();
    let block = node
        .children(&mut cursor)
        .find(|child| child.kind() == "block")?;
    let raw = source.get(keyword.end_byte()..block.start_byte())?;
    let start = keyword.end_byte() + (raw.len() - raw.trim_start().len());
    let prelude = normalized(raw);
    (!prelude.is_empty()).then(|| (vec![prelude], start..start + raw.trim().len()))
}

// {impl}`requirements.references.outline_comments`
fn is_comment(node: Node<'_>, source: &str) -> bool {
    kind_is_comment(node, source) || node.kind() == "js_comment"
}

/// Names a rule set by its normalised selector list.
// {cite}`habeas:util`
fn selectors_name(node: Node<'_>, source: &str) -> Option<(Vec<String>, std::ops::Range<usize>)> {
    let mut cursor = node.walk();
    let selectors = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "selectors")?;
    Some((
        vec![normalized(text(selectors, source))],
        selectors.byte_range(),
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
// {impl}`requirements.sdk.sources.css.attachments`
pub static PROFILE: Profile = Profile {
    name: "css",
    expando_char: '_',
    extensions: &["css"],
    language: Some(|| tree_sitter_css::LANGUAGE.into()),
    kinds: KINDS,
    is_comment,
    hashes_comment: never,
    comment_prose: crate::outline_engine::language::whole_comment,
    comment_binds_to_container: never,
    trailing_comments_outside: false,
    attaches: never,
};

/// Tree-sitter-backed css provider.
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

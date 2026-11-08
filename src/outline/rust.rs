//! Rust outline profile.

use tree_sitter::Node;

use super::profile::{KindRule, NameRule, Profile, text};

/// Names an impl by its syntactic self type and optional trait path.
// {impl}`requirements.references.rust_item_selectors`
// {impl}`requirements.cli.add_input_item`
fn impl_name(node: Node<'_>, source: &str) -> Option<(Vec<String>, usize)> {
    let self_type = node.child_by_field_name("type")?;
    let name = text(self_type, source).to_owned();
    // The name begins at whichever of the trait and self type comes first.
    Some(match node.child_by_field_name("trait") {
        Some(path) => (
            vec![name, text(path, source).to_owned()],
            path.start_byte().min(self_type.start_byte()),
        ),
        None => (vec![name], self_type.start_byte()),
    })
}

/// Names an extern block by its ABI string, `Rust` when absent.
// {impl}`requirements.references.rust_item_selectors`
// {impl}`requirements.cli.add_input_item`
fn extern_name(node: Node<'_>, source: &str) -> Option<(Vec<String>, usize)> {
    let mut cursor = node.walk();
    let modifier = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "extern_modifier");
    let abi = modifier.and_then(|modifier| {
        let mut cursor = modifier.walk();
        let literal = modifier
            .named_children(&mut cursor)
            .find(|child| child.kind() == "string_literal")?;
        let mut cursor = literal.walk();
        let content = literal
            .named_children(&mut cursor)
            .find(|child| child.kind() == "string_content")
            .map_or_else(String::new, |content| text(content, source).to_owned());
        Some((content, literal.start_byte()))
    });
    // Without an ABI string the name is not written, so the block is declared
    // where it starts.
    Some(match abi {
        Some((abi, byte)) => (vec![abi], byte),
        None => (vec!["Rust".to_owned()], node.start_byte()),
    })
}

/// Whether a grammar node sits directly in an item list.
// {impl}`requirements.references.rust_item_selectors`
fn in_item_list(node: Node<'_>) -> bool {
    node.parent()
        .is_some_and(|parent| matches!(parent.kind(), "source_file" | "declaration_list"))
}

/// Whether a macro invocation is an item rather than an expression.
// {impl}`requirements.references.rust_item_selectors`
fn item_macro(node: Node<'_>) -> bool {
    in_item_list(node)
        || node
            .parent()
            .is_some_and(|parent| parent.kind() == "expression_statement" && in_item_list(parent))
}

// {impl}`requirements.references.outlines`
// {impl}`requirements.references.outline_comments`
fn is_comment(node: Node<'_>, _source: &str) -> bool {
    matches!(node.kind(), "line_comment" | "block_comment")
}

/// Whether a comment has the given doc comment marker child.
// {impl}`requirements.references.structural_hash`
fn has_marker(node: Node<'_>, marker: &str) -> bool {
    let mut cursor = node.walk();
    is_comment(node, "")
        && node
            .children(&mut cursor)
            .any(|child| child.kind() == marker)
}

/// Doc comments, outer and inner, are hashed.
// {impl}`requirements.references.structural_hash`
fn is_doc_comment(node: Node<'_>, _source: &str) -> bool {
    has_marker(node, "outer_doc_comment_marker") || has_marker(node, "inner_doc_comment_marker")
}

/// Inner doc comments document the item enclosing them.
// {impl}`requirements.references.outline_comments`
fn is_inner_doc_comment(node: Node<'_>, _source: &str) -> bool {
    has_marker(node, "inner_doc_comment_marker")
}

/// A doc comment's prose group, inner or outer, and its text without the
/// `///`, `//!`, `/**`, `/*!`, and `*/` delimiters or the leading `*` of a
/// block comment's continuation lines.
// {impl}`requirements.references.structural_hash`
fn doc_prose(node: Node<'_>, body: &str) -> (&'static str, String) {
    let group = if is_inner_doc_comment(node, body) {
        "inner_doc_comment"
    } else {
        "outer_doc_comment"
    };
    let prose = if let Some(line) = body.strip_prefix("//") {
        line.strip_prefix(['/', '!']).unwrap_or(line).to_owned()
    } else {
        let inner = body.strip_prefix("/*").unwrap_or(body);
        let inner = inner.strip_suffix("*/").unwrap_or(inner);
        let inner = inner.strip_prefix(['*', '!']).unwrap_or(inner);
        let mut lines = inner.lines();
        let first = lines.next().unwrap_or("").to_owned();
        lines.fold(first, |mut prose, line| {
            let line = line.trim_start();
            prose.push('\n');
            prose.push_str(line.strip_prefix('*').unwrap_or(line));
            prose
        })
    };
    (group, prose)
}

/// Attributes and outer doc comments attach to the following item.
// {impl}`requirements.references.outlines`
fn attaches(node: Node<'_>, _source: &str) -> bool {
    node.kind() == "attribute_item" || has_marker(node, "outer_doc_comment_marker")
}

// {impl}`requirements.references.rust_item_selectors`
// {impl}`requirements.references.outlines`
const KINDS: &[KindRule] = &[
    KindRule::new("function_item", "fn", NameRule::Field("name")),
    KindRule::new("function_signature_item", "fn", NameRule::Field("name")),
    KindRule::new("struct_item", "struct", NameRule::Field("name")),
    KindRule::new("enum_item", "enum", NameRule::Field("name")),
    KindRule::new("union_item", "union", NameRule::Field("name")),
    KindRule::new("trait_item", "trait", NameRule::Field("name")),
    KindRule::new("impl_item", "impl", NameRule::Custom(impl_name)),
    KindRule::new("mod_item", "mod", NameRule::Field("name")),
    KindRule::new("macro_definition", "macro", NameRule::Field("name")),
    KindRule::new("macro_invocation", "macro", NameRule::Anonymous).when(item_macro),
    KindRule::new("const_item", "const", NameRule::Field("name")),
    KindRule::new("static_item", "static", NameRule::Field("name")),
    KindRule::new("type_item", "type", NameRule::Field("name")),
    KindRule::new("associated_type", "type", NameRule::Field("name")),
    KindRule::new("foreign_mod_item", "extern", NameRule::Custom(extern_name)),
    KindRule::new(
        "extern_crate_declaration",
        "extern-crate",
        NameRule::Field("name"),
    ),
    KindRule::new("use_declaration", "use", NameRule::Anonymous),
];

// {impl}`requirements.references.outlines`
// {impl}`requirements.references.rust_item_selectors`
// {impl}`requirements.references.outline_comments`
// {impl}`requirements.references.structural_hash`
pub(crate) static PROFILE: Profile = Profile {
    name: "rust",
    extensions: &["rs"],
    language: Some(|| tree_sitter_rust::LANGUAGE.into()),
    kinds: KINDS,
    is_comment,
    hashes_comment: is_doc_comment,
    comment_prose: doc_prose,
    comment_binds_to_container: is_inner_doc_comment,
    trailing_comments_outside: false,
    attaches,
};

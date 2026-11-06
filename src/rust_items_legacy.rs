//! The syn-based Rust item collector that predates the outline engine, kept
//! only to verify that the Rust outline profile reproduces its selectors.

use std::collections::BTreeMap;
use std::ops::Range;

use proc_macro2::Span;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

use crate::rust_coverage::comments;

// {test}`requirements.references.rust_item_selectors`
#[derive(Clone, Debug)]
pub(crate) struct LegacyItem {
    pub(crate) selector: String,
    pub(crate) declaration_line: usize,
    pub(crate) line_start: usize,
    pub(crate) line_end: usize,
}

// {test}`requirements.references.rust_item_selectors`
struct ItemNode {
    parent: Option<usize>,
    base: String,
    ordinal_always: bool,
    declaration_line: usize,
    byte_start: usize,
    byte_end: usize,
}

// {test}`requirements.references.rust_item_selectors`
struct ItemCollector<'a> {
    source: &'a str,
    nodes: Vec<ItemNode>,
    parents: Vec<usize>,
}

impl ItemCollector<'_> {
    // {test}`requirements.references.rust_item_selectors`
    fn add(
        &mut self,
        base: String,
        span: Span,
        declaration: Span,
        _name: String,
        _is_function: bool,
        ordinal_always: bool,
    ) -> usize {
        let range = span.byte_range();
        let index = self.nodes.len();
        self.nodes.push(ItemNode {
            parent: self.parents.last().copied(),
            base,
            ordinal_always,
            declaration_line: declaration.start().line,
            byte_start: range.start,
            byte_end: range.end,
        });
        index
    }

    // {test}`requirements.references.rust_item_selectors`
    fn source_at(&self, span: Span) -> &str {
        self.source.get(span.byte_range()).unwrap_or("")
    }

    // {test}`requirements.references.rust_item_selectors`
    fn named(&self, kind: &str, name: &syn::Ident) -> String {
        format!("{kind}[{}]", escape_segment(&name.to_string()))
    }
}

impl<'ast> Visit<'ast> for ItemCollector<'_> {
    // {test}`requirements.references.rust_item_selectors`
    fn visit_item(&mut self, item: &'ast syn::Item) {
        use syn::Item;
        let (base, declaration, name, is_function, ordinal_always) = match item {
            Item::Const(node) => (
                self.named("const", &node.ident),
                node.ident.span(),
                node.ident.to_string(),
                false,
                false,
            ),
            Item::Enum(node) => (
                self.named("enum", &node.ident),
                node.ident.span(),
                node.ident.to_string(),
                false,
                false,
            ),
            Item::ExternCrate(node) => (
                self.named("extern-crate", &node.ident),
                node.ident.span(),
                node.ident.to_string(),
                false,
                false,
            ),
            Item::Fn(node) => (
                self.named("fn", &node.sig.ident),
                node.sig.ident.span(),
                node.sig.ident.to_string(),
                true,
                false,
            ),
            Item::ForeignMod(node) => {
                let abi = node
                    .abi
                    .name
                    .as_ref()
                    .map_or("Rust".to_owned(), syn::LitStr::value);
                (
                    format!("extern[{}]", escape_segment(&abi)),
                    node.abi.extern_token.span,
                    abi,
                    false,
                    false,
                )
            }
            Item::Impl(node) => {
                let self_type = self.source_at(node.self_ty.span());
                let trait_name = node
                    .trait_
                    .as_ref()
                    .map(|(_, path, _)| self.source_at(path.span()));
                let base = if let Some(trait_name) = trait_name {
                    format!(
                        "impl[{}/{}]",
                        escape_segment(self_type),
                        escape_segment(trait_name)
                    )
                } else {
                    format!("impl[{}]", escape_segment(self_type))
                };
                (base, node.impl_token.span, "impl".to_owned(), false, false)
            }
            Item::Macro(node) => {
                if let Some(name) = &node.ident {
                    (
                        self.named("macro", name),
                        name.span(),
                        name.to_string(),
                        false,
                        false,
                    )
                } else {
                    (
                        "macro".to_owned(),
                        node.mac.path.span(),
                        "macro".to_owned(),
                        false,
                        true,
                    )
                }
            }
            Item::Mod(node) => (
                self.named("mod", &node.ident),
                node.ident.span(),
                node.ident.to_string(),
                false,
                false,
            ),
            Item::Static(node) => (
                self.named("static", &node.ident),
                node.ident.span(),
                node.ident.to_string(),
                false,
                false,
            ),
            Item::Struct(node) => (
                self.named("struct", &node.ident),
                node.ident.span(),
                node.ident.to_string(),
                false,
                false,
            ),
            Item::Trait(node) => (
                self.named("trait", &node.ident),
                node.ident.span(),
                node.ident.to_string(),
                false,
                false,
            ),
            Item::TraitAlias(node) => (
                self.named("trait-alias", &node.ident),
                node.ident.span(),
                node.ident.to_string(),
                false,
                false,
            ),
            Item::Type(node) => (
                self.named("type", &node.ident),
                node.ident.span(),
                node.ident.to_string(),
                false,
                false,
            ),
            Item::Union(node) => (
                self.named("union", &node.ident),
                node.ident.span(),
                node.ident.to_string(),
                false,
                false,
            ),
            Item::Use(node) => (
                "use".to_owned(),
                node.use_token.span,
                "use".to_owned(),
                false,
                true,
            ),
            Item::Verbatim(_) => (
                "item".to_owned(),
                item.span(),
                "item".to_owned(),
                false,
                true,
            ),
            _ => (
                "item".to_owned(),
                item.span(),
                "item".to_owned(),
                false,
                true,
            ),
        };
        let index = self.add(
            base,
            item.span(),
            declaration,
            name,
            is_function,
            ordinal_always,
        );
        self.parents.push(index);
        visit::visit_item(self, item);
        self.parents.pop();
    }

    // {test}`requirements.references.rust_item_selectors`
    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        use syn::ImplItem;
        let (base, declaration, name, is_function, ordinal_always) = match item {
            ImplItem::Const(node) => (
                self.named("const", &node.ident),
                node.ident.span(),
                node.ident.to_string(),
                false,
                false,
            ),
            ImplItem::Fn(node) => (
                self.named("fn", &node.sig.ident),
                node.sig.ident.span(),
                node.sig.ident.to_string(),
                true,
                false,
            ),
            ImplItem::Type(node) => (
                self.named("type", &node.ident),
                node.ident.span(),
                node.ident.to_string(),
                false,
                false,
            ),
            ImplItem::Macro(node) => (
                "macro".to_owned(),
                node.mac.path.span(),
                "macro".to_owned(),
                false,
                true,
            ),
            _ => (
                "item".to_owned(),
                item.span(),
                "item".to_owned(),
                false,
                true,
            ),
        };
        let index = self.add(
            base,
            item.span(),
            declaration,
            name,
            is_function,
            ordinal_always,
        );
        self.parents.push(index);
        visit::visit_impl_item(self, item);
        self.parents.pop();
    }

    // {test}`requirements.references.rust_item_selectors`
    fn visit_trait_item(&mut self, item: &'ast syn::TraitItem) {
        use syn::TraitItem;
        let (base, declaration, name, is_function, ordinal_always) = match item {
            TraitItem::Const(node) => (
                self.named("const", &node.ident),
                node.ident.span(),
                node.ident.to_string(),
                false,
                false,
            ),
            TraitItem::Fn(node) => (
                self.named("fn", &node.sig.ident),
                node.sig.ident.span(),
                node.sig.ident.to_string(),
                true,
                false,
            ),
            TraitItem::Type(node) => (
                self.named("type", &node.ident),
                node.ident.span(),
                node.ident.to_string(),
                false,
                false,
            ),
            TraitItem::Macro(node) => (
                "macro".to_owned(),
                node.mac.path.span(),
                "macro".to_owned(),
                false,
                true,
            ),
            _ => (
                "item".to_owned(),
                item.span(),
                "item".to_owned(),
                false,
                true,
            ),
        };
        let index = self.add(
            base,
            item.span(),
            declaration,
            name,
            is_function,
            ordinal_always,
        );
        self.parents.push(index);
        visit::visit_trait_item(self, item);
        self.parents.pop();
    }

    // {test}`requirements.references.rust_item_selectors`
    fn visit_foreign_item(&mut self, item: &'ast syn::ForeignItem) {
        use syn::ForeignItem;
        let (base, declaration, name, is_function, ordinal_always) = match item {
            ForeignItem::Fn(node) => (
                self.named("fn", &node.sig.ident),
                node.sig.ident.span(),
                node.sig.ident.to_string(),
                true,
                false,
            ),
            ForeignItem::Static(node) => (
                self.named("static", &node.ident),
                node.ident.span(),
                node.ident.to_string(),
                false,
                false,
            ),
            ForeignItem::Type(node) => (
                self.named("type", &node.ident),
                node.ident.span(),
                node.ident.to_string(),
                false,
                false,
            ),
            ForeignItem::Macro(node) => (
                "macro".to_owned(),
                node.mac.path.span(),
                "macro".to_owned(),
                false,
                true,
            ),
            _ => (
                "item".to_owned(),
                item.span(),
                "item".to_owned(),
                false,
                true,
            ),
        };
        let index = self.add(
            base,
            item.span(),
            declaration,
            name,
            is_function,
            ordinal_always,
        );
        self.parents.push(index);
        visit::visit_foreign_item(self, item);
        self.parents.pop();
    }
}

// {test}`requirements.references.rust_item_selectors`
fn escape_segment(source: &str) -> String {
    let mut escaped = String::new();
    for character in source.chars() {
        if matches!(character, '%' | '[' | ']' | '/' | '#') || character.is_whitespace() {
            for byte in character.to_string().bytes() {
                escaped.push_str(&format!("%{byte:02X}"));
            }
        } else {
            escaped.push(character);
        }
    }
    escaped
}

// {test}`requirements.references.rust_item_selectors`
pub(crate) fn parse_items(source: &str) -> Result<Vec<LegacyItem>, syn::Error> {
    let syntax = syn::parse_file(source)?;
    let mut collector = ItemCollector {
        source,
        nodes: Vec::new(),
        parents: Vec::new(),
    };
    collector.visit_file(&syntax);
    let nodes = collector.nodes;
    let mut groups = BTreeMap::<(Option<usize>, String), Vec<usize>>::new();
    for (index, node) in nodes.iter().enumerate() {
        groups
            .entry((node.parent, node.base.clone()))
            .or_default()
            .push(index);
    }
    let mut segments = vec![String::new(); nodes.len()];
    for indices in groups.values_mut() {
        indices.sort_by_key(|index| nodes[*index].byte_start);
        let count = indices.len();
        for (ordinal, index) in indices.iter().enumerate() {
            let node = &nodes[*index];
            segments[*index] = if node.ordinal_always || count > 1 {
                format!("{}#{}", node.base, ordinal + 1)
            } else {
                node.base.clone()
            };
        }
    }
    let comments = comments(source);
    let mut selectors = vec![String::new(); nodes.len()];
    let mut items = Vec::with_capacity(nodes.len());
    for (index, node) in nodes.iter().enumerate() {
        selectors[index] = node.parent.map_or_else(
            || segments[index].clone(),
            |parent| format!("{}/{}", selectors[parent], segments[index]),
        );
        let byte_start = attached_comment_indices(source, &comments, node.byte_start)
            .into_iter()
            .map(|index| comments[index].start)
            .min()
            .unwrap_or(node.byte_start);
        let line_start = line_at(source, byte_start);
        let line_end = line_at(source, node.byte_end.saturating_sub(1));
        items.push(LegacyItem {
            selector: selectors[index].clone(),
            declaration_line: node.declaration_line,
            line_start,
            line_end,
        });
    }
    Ok(items)
}

// {test}`requirements.references.rust_item_selectors`
fn line_at(source: &str, byte: usize) -> usize {
    source.as_bytes()[..byte.min(source.len())]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1
}

// {test}`requirements.references.rust_item_selectors`
fn attached_comment_indices(
    source: &str,
    comments: &[Range<usize>],
    function_start: usize,
) -> Vec<usize> {
    let mut position = function_start;
    let mut indices = Vec::new();
    for (index, comment) in comments.iter().enumerate().rev() {
        if comment.end > position {
            continue;
        }
        if source[comment.clone()].starts_with("//!") || source[comment.clone()].starts_with("/*!")
        {
            break;
        }
        let line_start = source[..comment.start]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        if source[comment.clone()].starts_with("//")
            && !source[line_start..comment.start].trim().is_empty()
        {
            break;
        }
        let gap = &source[comment.end..position];
        if !gap.trim().is_empty() || gap.bytes().filter(|byte| *byte == b'\n').count() > 1 {
            break;
        }
        indices.push(index);
        position = comment.start;
    }
    indices
}

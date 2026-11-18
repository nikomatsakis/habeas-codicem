use super::{Outline, parse};
use std::path::Path;

fn outline(source: &str) -> Outline {
    parse(Path::new("a.md"), source).unwrap_or_else(|error| panic!("{source:?}: {error}"))
}

fn kinds(outline: &Outline) -> Vec<(&str, Option<usize>, usize, usize)> {
    outline.nodes[1..]
        .iter()
        .map(|node| (node.kind, node.parent, node.line_start, node.line_end))
        .collect()
}

// {test}`requirements.references.outlines`
#[test]
fn markdown_views_respect_profile_field_and_child_naming_rules() {
    use super::profile::{KindRule, NameRule, Profile};
    let base = super::profile_for_path(Path::new("a.md"));
    for missing in [false, true] {
        let rules = vec![
            KindRule::new(
                "atx_heading",
                "section",
                NameRule::Field(if missing {
                    "missing"
                } else {
                    "heading_content"
                }),
            ),
            KindRule::new(
                "paragraph",
                "paragraph",
                NameRule::FirstChild(if missing { &["missing"] } else { &["inline"] }),
            ),
        ];
        let profile = Box::leak(Box::new(Profile {
            kinds: Box::leak(rules.into_boxed_slice()),
            ..*base
        }));
        let outline = super::parse_with(profile, "# Heading\n\nWords.\n").unwrap();
        assert_eq!(outline.nodes.len(), 3);
        assert_eq!(outline.nodes[1].kind, "section");
        assert_eq!(outline.nodes[2].kind, "paragraph");
        if missing {
            assert!(outline.nodes[1..].iter().all(|node| node.name.is_none()));
        } else {
            assert_eq!(
                outline.nodes[1].name.as_deref(),
                Some(["Heading".to_owned()].as_slice())
            );
            assert_eq!(
                outline.nodes[2].name.as_deref(),
                Some(["Words.".to_owned()].as_slice())
            );
        }
    }
}

// {test}`requirements.sdk.sources.markdown.paragraphs`
// {test}`requirements.sdk.sources.markdown.fenced_blocks`
// {test}`requirements.sdk.sources.markdown.fenced_blocks.contents`
// {test}`requirements.references.outlines`
// {test}`requirements.references.outline_selectors`
#[test]
fn paragraphs_and_nested_directives_have_independent_source_ranges() {
    let source = "# Authentication\n\nTokens expire.\n\n::::{note}\nReject expired tokens.\n\n:::{tip}\nCheck the boundary too.\n:::\n::::\n\n```text\nhi\n```\n";
    let outline = outline(source);
    assert_eq!(
        kinds(&outline),
        [
            ("section", Some(0), 1, 15),
            ("paragraph", Some(1), 3, 3),
            ("block", Some(1), 5, 11),
            ("paragraph", Some(3), 6, 6),
            ("block", Some(3), 8, 10),
            ("paragraph", Some(5), 9, 9),
            ("block", Some(1), 13, 15),
        ]
    );
    for (index, node) in outline.nodes.iter().enumerate().skip(1) {
        assert_eq!(outline.resolve(&node.selector).unwrap(), index);
        if node.kind != "section" {
            assert!(node.segment.starts_with(&format!("{}@", node.kind)));
            assert_eq!(node.segment.len(), node.kind.len() + 1 + 12);
        }
        assert_eq!(
            outline.node_lines(source, index).join("\n"),
            source[node.byte_start..node.byte_end].trim_end_matches('\n')
        );
    }
    assert_eq!(outline.owner_at_line(9), 6);
    assert_eq!(outline.owner_at_line(14), 7);
}

// {test}`requirements.sdk.sources.markdown.paragraphs`
#[test]
fn paragraphs_in_transparent_lists_and_quotes_are_selectable() {
    let source = "Before heading.\n\n- List paragraph.\n\n> Quoted paragraph.\n\nTitle\n=====\n\nAfter heading.\n";
    let outline = outline(source);
    let paragraphs = outline
        .nodes
        .iter()
        .filter(|node| node.kind == "paragraph")
        .collect::<Vec<_>>();
    assert_eq!(paragraphs.len(), 4);
    assert_eq!(
        paragraphs
            .iter()
            .map(|node| node.line_start)
            .collect::<Vec<_>>(),
        [1, 3, 5, 10]
    );
    let named = outline.resolve("section[Title]").unwrap();
    assert_eq!(paragraphs[3].parent, Some(named));
    assert!(paragraphs[..3].iter().all(|node| node.parent == Some(1)));
    assert_eq!(outline.nodes[1].kind, "section");
}

// {test}`requirements.sdk.sources.markdown.fenced_blocks`
// {test}`requirements.sdk.sources.markdown.fenced_blocks.contents`
// {test}`requirements.sdk.sources.markdown.fence_precedence`
// {test}`requirements.references.outlines`
#[test]
fn unfinished_directives_retain_their_body_and_leave_the_outer_closer_outside() {
    for source in [
        ":::{note}\nBody",
        ":::{note}\nBody\n",
        ":::{note}\n# Heading\nBody",
        "# Outside\n:::{note}\nBody",
    ] {
        let outline = outline(source);
        let block = outline
            .nodes
            .iter()
            .find(|node| node.kind == "block")
            .unwrap();
        assert_eq!(block.byte_end, source.len());
        assert_eq!(block.parent, Some(1));
        assert_eq!(outline.nodes[1].kind, "section");
        if source.starts_with("# Outside") {
            assert_eq!(outline.nodes[1].selector, "section[Outside]");
        }
        assert!(outline.nodes.iter().any(|node| node.kind == "paragraph"));
    }
    let source = "::::::{note}\nOuter.\n\n:::{tip}\nInner.\n::::::\n\nAfter.\n";
    let outline = outline(source);
    let blocks = outline
        .nodes
        .iter()
        .filter(|node| node.kind == "block")
        .collect::<Vec<_>>();
    assert_eq!(blocks.len(), 2);
    assert_eq!((blocks[0].line_start, blocks[0].line_end), (1, 6));
    assert_eq!((blocks[1].line_start, blocks[1].line_end), (4, 5));
    assert_eq!(blocks[0].parent, Some(1));
    assert_eq!(
        blocks[1].parent,
        Some(outline.resolve(&blocks[0].selector).unwrap())
    );
    assert!(!source[blocks[1].byte_start..blocks[1].byte_end].contains("::::::"));
    assert_eq!(outline.nodes[outline.owner_at_line(8)].kind, "paragraph");
    assert_eq!(outline.nodes[outline.owner_at_line(8)].parent, Some(1));
}

// {test}`requirements.sdk.sources.markdown.fence_precedence`
// {test}`requirements.sdk.sources.markdown.fenced_blocks`
// {test}`requirements.sdk.sources.markdown.fenced_blocks.contents`
// {test}`requirements.references.outlines`
#[test]
fn literal_code_inline_code_and_html_shield_directive_looking_text() {
    for source in [
        "````text\n:::{note}\nBody.\n```\n:::\n````\n",
        "~~~~text\n:::{note}\nBody.\n~~~\n:::\n~~~~\n",
        "```{note}\n:::{tip}\nBody.\n:::\n```\n",
    ] {
        let outline = outline(source);
        assert_eq!(
            outline
                .nodes
                .iter()
                .filter(|node| node.kind == "block")
                .count(),
            1
        );
        assert!(!outline.nodes.iter().any(|node| node.kind == "paragraph"));
    }
    for source in [
        "`code\n:::{note}\nbody\n:::\ncode`\n",
        "<div>\n:::{note}\nbody\n:::\n</div>\n",
        "<!--\n:::{note}\nbody\n:::\n-->\n",
        "`:::{note}`\n",
    ] {
        let outline = outline(source);
        assert!(
            !outline.nodes.iter().any(|node| node.kind == "block"),
            "{source}"
        );
    }
}

// {test}`requirements.sdk.sources.markdown.fence_precedence`
// {test}`requirements.sdk.sources.markdown.fenced_blocks`
// {test}`requirements.sdk.sources.markdown.fenced_blocks.contents`
// {test}`requirements.references.outlines`
#[test]
fn nested_fence_lengths_and_closer_whitespace_preserve_the_right_containers() {
    let source = "::::::{note}\nOuter.\n\n::::{tip}\nInner.\n:::\n:::: trailing text\n::::   \n\n~~~text\n::::::\n:::{fake}\n~~~\n::::::\n";
    let outline = outline(source);
    let blocks = outline
        .nodes
        .iter()
        .filter(|node| node.kind == "block")
        .collect::<Vec<_>>();
    assert_eq!(blocks.len(), 3);
    assert_eq!((blocks[0].line_start, blocks[0].line_end), (1, 14));
    assert_eq!((blocks[1].line_start, blocks[1].line_end), (4, 8));
    assert_eq!((blocks[2].line_start, blocks[2].line_end), (10, 13));
    let outer = outline.resolve(&blocks[0].selector).unwrap();
    assert_eq!(blocks[1].parent, Some(outer));
    assert_eq!(blocks[2].parent, Some(outer));
    assert_eq!(outline.nodes[outline.owner_at_line(12)].kind, "block");
}

// {test}`requirements.sdk.sources.markdown.fenced_blocks`
// {test}`requirements.sdk.sources.markdown.fenced_blocks.contents`
#[test]
fn unfinished_code_fences_remain_literal_blocks_through_eof() {
    for source in [
        "# H\n\n```text\n:::{note}\nLiteral body.",
        "# H\n\n~~~text\n:::{note}\nLiteral body.\n",
    ] {
        let outline = outline(source);
        let blocks = outline
            .nodes
            .iter()
            .filter(|node| node.kind == "block")
            .collect::<Vec<_>>();
        assert_eq!(blocks.len(), 1);
        let block = blocks[0];
        assert_eq!(block.parent, Some(outline.resolve("section[H]").unwrap()));
        assert_eq!((block.line_start, block.line_end), (3, 5));
        assert_eq!(block.byte_end, source.len());
        assert!(
            source[block.byte_start..block.byte_end].starts_with("```text")
                || source[block.byte_start..block.byte_end].starts_with("~~~text")
        );
        assert!(source[block.byte_start..block.byte_end].contains("Literal body."));
        assert!(block.segment.starts_with("block@"));
        assert_eq!(block.segment.len(), "block@".len() + 12);
        assert!(block.children.is_empty());
        assert!(!outline.nodes.iter().any(|node| node.kind == "paragraph"));
    }
}

// {test}`requirements.sdk.sources.markdown.fenced_blocks`
// {test}`requirements.sdk.sources.markdown.fenced_blocks.contents`
// {test}`requirements.sdk.sources.markdown.fence_precedence`
#[test]
fn empty_long_closed_and_quoted_directives_keep_original_unicode_offsets() {
    for source in [":::{note}\n:::\n", ":::{note}\n:::::\n", ":::{note}"] {
        let outline = outline(source);
        let block = outline
            .nodes
            .iter()
            .find(|node| node.kind == "block")
            .unwrap();
        assert_eq!(block.byte_end, source.len());
        assert!(!outline.nodes.iter().any(|node| node.kind == "paragraph"));
    }
    let source = "# 日本語\r\n\r\n> ::::{note}\r\n> café ☃\r\n>\r\n> :::{tip}\r\n> Inner.\r\n> :::\r\n> ::::\r\n";
    let outline = outline(source);
    let blocks = outline
        .nodes
        .iter()
        .filter(|node| node.kind == "block")
        .collect::<Vec<_>>();
    assert_eq!(blocks.len(), 2);
    assert_eq!((blocks[0].line_start, blocks[0].line_end), (3, 9));
    assert_eq!((blocks[1].line_start, blocks[1].line_end), (6, 8));
    let prose = &outline.nodes[outline.owner_at_line(4)];
    assert_eq!(prose.kind, "paragraph");
    assert!(source[prose.byte_start..prose.byte_end].contains("café ☃"));
    assert_eq!(prose.parent, Some(2));
}

// {test}`requirements.sdk.sources.markdown.fence_precedence`
#[test]
fn two_colons_and_inline_directive_text_do_not_open_containers() {
    for source in [
        ":: {note}\nText.\n::\n",
        ":::{ }\nText.\n:::\n",
        "Text :::{note}\n",
        ":::\nText.\n",
    ] {
        let outline = outline(source);
        assert!(
            !outline.nodes.iter().any(|node| node.kind == "block"),
            "{source}"
        );
    }
}

// {test}`requirements.sdk.sources.markdown.paragraphs`
// {test}`requirements.sdk.sources.markdown.fenced_blocks`
// {test}`requirements.references.structural_hash`
// {test}`requirements.references.selector_duplicate_names`
#[test]
fn anonymous_identities_survive_reflow_and_sibling_edits_and_number_duplicates() {
    let original =
        outline("# H\n::::{note}\nFirst paragraph.\n\n:::{tip}\nInner words.\n:::\n::::\n");
    let reflowed =
        outline("# H\n::::{note}\nFirst\n paragraph.\n\n:::{tip}\nInner\nwords.\n:::\n::::\n");
    assert_eq!(
        original.nodes[1].structural_hash,
        reflowed.nodes[1].structural_hash
    );
    assert_eq!(
        original
            .nodes
            .iter()
            .map(|node| &node.selector)
            .collect::<Vec<_>>(),
        reflowed
            .nodes
            .iter()
            .map(|node| &node.selector)
            .collect::<Vec<_>>()
    );
    let changed = outline(
        "# H\n::::{note}\nA replacement paragraph.\n\n:::{tip}\nDifferent inner words.\n:::\n::::\n\nNew sibling.\n",
    );
    assert_eq!(original.nodes[2].selector, changed.nodes[2].selector);
    assert_eq!(original.nodes[4].selector, changed.nodes[4].selector);
    assert_ne!(
        original.nodes[2].structural_hash,
        changed.nodes[2].structural_hash
    );
    let duplicates =
        outline("# H\nSame.\n\nSame.\n\n:::{note}\nFirst.\n:::\n\n:::{note}\nSecond.\n:::\n");
    let paragraphs = duplicates
        .nodes
        .iter()
        .filter(|node| node.kind == "paragraph" && node.parent == Some(1))
        .collect::<Vec<_>>();
    assert!(paragraphs[0].segment.ends_with("#1"));
    assert!(paragraphs[1].segment.ends_with("#2"));
    let blocks = duplicates
        .nodes
        .iter()
        .filter(|node| node.kind == "block")
        .collect::<Vec<_>>();
    assert!(blocks[0].segment.ends_with("#1"));
    assert!(blocks[1].segment.ends_with("#2"));
}

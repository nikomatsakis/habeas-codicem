//! Tests of `habeas refactor`'s mapping and rewriting helpers.

use std::path::Path;

use super::*;

/// Mappings from `(kind, old, new)` triples.
// {cite}`habeas:util`
fn mappings(entries: &[(&str, &str, &str)]) -> Mappings {
    let mut mappings = Mappings::default();
    for (kind, old, new) in entries {
        let pair = ((*old).to_owned(), (*new).to_owned());
        match *kind {
            "name" => mappings.names.push(pair),
            "path" => mappings.paths.push(pair),
            _ => {
                let split = |value: &str| {
                    let (file, item) = value.split_once('#').unwrap_or((value, ""));
                    (file.to_owned(), item.to_owned())
                };
                mappings.nodes.push((split(old), split(new)));
            }
        }
    }
    mappings
}

// {test}`requirements.refactor.mappings`
#[test]
fn mappings_cover_names_paths_and_nodes_beneath_them() {
    let names = mappings(&[("name", "rules.cli", "rules.core")]);
    assert_eq!(names.name("rules.cli").as_deref(), Some("rules.core"));
    assert_eq!(
        names.name("rules.cli.check").as_deref(),
        Some("rules.core.check")
    );
    assert_eq!(names.name("rules.client"), None);
    assert_eq!(
        names.name_back("rules.core.check").as_deref(),
        Some("rules.cli.check")
    );
    assert_eq!(
        names.selection("rules.cli.*").as_deref(),
        Some("rules.core.*")
    );

    let paths = mappings(&[("path", "src/config", "src/settings")]);
    assert_eq!(
        paths.node("src/config/load.rs", "fn[load]"),
        Some(("src/settings/load.rs".to_owned(), "fn[load]".to_owned()))
    );
    assert_eq!(paths.path("src/configure.rs"), None);
    assert_eq!(
        paths.path_back("src/settings/a.rs").as_deref(),
        Some("src/config/a.rs")
    );

    let nodes = mappings(&[("node", "src/a.rs#mod[m]", "src/b.rs#mod[n]")]);
    assert_eq!(
        nodes.node("src/a.rs", "mod[m]/fn[f]"),
        Some(("src/b.rs".to_owned(), "mod[n]/fn[f]".to_owned()))
    );
    assert_eq!(nodes.node("src/a.rs", "mod[mm]"), None);
    assert_eq!(nodes.node("src/a.rs", ""), None);
    assert_eq!(
        nodes.node_back("src/b.rs", "mod[n]"),
        Some(("src/a.rs".to_owned(), "mod[m]".to_owned()))
    );
    assert_eq!(
        nodes.address("src/a.rs#mod[m]").as_deref(),
        Some("src/b.rs#mod[n]")
    );
    // A root mapping covers every node of the file.
    let root = mappings(&[("node", "src/a.rs", "src/b.rs#mod[a]")]);
    assert_eq!(
        root.node("src/a.rs", "fn[f]"),
        Some(("src/b.rs".to_owned(), "mod[a]/fn[f]".to_owned()))
    );
}

// {test}`requirements.refactor.mapped_back`
#[test]
fn mapped_back_text_replaces_whole_new_names_and_paths() {
    let map = mappings(&[
        ("name", "rules.cli.check", "rules.core.check"),
        ("path", "src/a.rs", "src/b.rs"),
        ("node", "src/c.rs#fn[f]", "src/d.rs#fn[f]"),
    ]);
    assert_eq!(
        map.text_back("{impl}`rules.core.check` and rules.core.check.case"),
        "{impl}`rules.cli.check` and rules.cli.check.case"
    );
    // Longer names and other paths are left alone.
    assert_eq!(map.text_back("rules.core.checker"), "rules.core.checker");
    assert_eq!(map.text_back("xrules.core.check"), "xrules.core.check");
    assert_eq!(map.text_back("see src/b.rs."), "see src/a.rs.");
    assert_eq!(map.text_back("src/b.rs.bak"), "src/b.rs.bak");
    assert_eq!(map.text_back("lib/src/b.rs"), "lib/src/b.rs");
    assert_eq!(map.text_back("`src/d.rs#fn[f]`"), "`src/c.rs#fn[f]`");
}

// {test}`requirements.refactor.reference_rewrite`
#[test]
fn sources_rewrite_references_to_names_and_anchors_to_paths() {
    let map = mappings(&[
        ("name", "rules.check", "core.check"),
        ("path", "data.txt", "info.txt"),
    ]);
    let rust = concat!(
        "// {impl}`rules.check`\n",
        "// {cite}`rules.checker`\n",
        "// :::{fixme} rules.check\n// A gap.\n// :::\n",
        "fn f() {}\n",
    );
    let (rewritten, edits) = rewrite_source(&map, Path::new("src/lib.rs"), rust);
    assert_eq!(edits, 2);
    assert_eq!(
        rewritten,
        concat!(
            "// {impl}`core.check`\n",
            "// {cite}`rules.checker`\n",
            "// :::{fixme} core.check\n// A gap.\n// :::\n",
            "fn f() {}\n",
        )
    );
    // In Markdown, anchors are rewritten too, but not the declaration's name,
    // and references in code blocks stay literal.
    let markdown = concat!(
        ":::{req} other\n:impl: data.txt\n:test: data.txt#line@abc\n",
        "Uses {term}`rules.check`.\n:::\n\n```text\n{cite}`rules.check`\n```\n",
    );
    let (rewritten, edits) = rewrite_source(&map, Path::new("md/rules.md"), markdown);
    assert_eq!(edits, 3, "{rewritten}");
    assert!(rewritten.contains(":impl: info.txt\n:test: info.txt#line@abc\n"));
    assert!(rewritten.contains("Uses {term}`core.check`."));
    assert!(rewritten.contains("```text\n{cite}`rules.check`\n```"));
    // Plain text is not an anchor outside Markdown.
    let (_, edits) = rewrite_source(&map, Path::new("notes.txt"), ":impl: data.txt\n");
    assert_eq!(edits, 0);
}

// {test}`requirements.refactor.rationale.names`
// {test}`requirements.refactor.rationale.paths`
// {test}`requirements.refactor.rationale.addresses`
// {test}`requirements.refactor.rationale.untouched`
#[test]
fn rationales_rewrite_exact_code_spans_and_list_other_mentions() {
    let map = mappings(&[
        ("name", "rules.cli", "rules.core"),
        ("path", "src/old", "src/new"),
        ("node", "src/a.rs#fn[f]", "src/b.rs#fn[f]"),
    ]);
    let (rewritten, mentions) = rewrite_rationale(
        &map,
        "`rules.cli.check` holds; `src/old/x.rs` and `src/a.rs#fn[f]` do it. \
         `src/a.rs` stays, as does `src/a.rs:3`, rules.cli.check, and `rules.client`.",
    );
    assert_eq!(
        rewritten,
        "`rules.core.check` holds; `src/new/x.rs` and `src/b.rs#fn[f]` do it. \
         `src/a.rs` stays, as does `src/a.rs:3`, rules.cli.check, and `rules.client`."
    );
    assert_eq!(mentions, ["`src/a.rs`", "`src/a.rs:3`"]);
}

// {test}`requirements.refactor.definition_inputs`
#[test]
fn opening_fences_map_their_short_names_back() {
    assert_eq!(
        opening_back(":::{req} verify", "rules.verify", "rules.check"),
        ":::{req} check"
    );
    assert_eq!(
        opening_back("::::{req} check", "core.check", "rules.check"),
        "::::{req} check"
    );
    assert_eq!(
        opening_back(
            ":::{req} line.detection",
            "a.line.detection",
            "b.lines.detection"
        ),
        ":::{req} lines.detection"
    );
}

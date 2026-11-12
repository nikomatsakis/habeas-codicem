use std::path::Path;

use super::{Outline, parse};

fn outline(path: &str, source: &str) -> Outline {
    parse(Path::new(path), source).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn selectors(outline: &Outline) -> Vec<String> {
    outline.nodes[1..]
        .iter()
        .map(|node| node.selector.clone())
        .collect()
}

fn summary(outline: &Outline) -> Vec<String> {
    outline.nodes[1..]
        .iter()
        .map(|node| {
            format!(
                "{} {}-{}",
                regex::Regex::new("@[0-9a-f]{12}")
                    .unwrap()
                    .replace_all(&node.selector, "@"),
                node.line_start,
                node.line_end
            )
        })
        .collect()
}

// {test}`requirements.references.outline_selectors`
// {test}`requirements.references.rust_item_selectors`
// {test}`requirements.references.selector_nested`
// {test}`requirements.references.selector_impl_and_trait`
// {test}`requirements.references.selector_inline_and_foreign`
// {test}`requirements.references.selector_duplicate_names`
#[test]
fn rust_selectors_follow_syntactic_ancestry() {
    let source = r#"
mod local { fn run() {} }
mod external;
struct Thing;
impl Thing { fn run() {} }
impl Trait for Thing { fn run() {} }
impl [u8; 4] { fn edge() {} }
trait Trait { fn read(&self); type Item; }
extern "C" { fn puts(); }
extern { fn other(); }
fn outer() { fn inner() {} }
#[cfg(a)] fn repeated() {}
#[cfg(b)] fn repeated() {}
macro_rules! make { () => {} }
union U { a: u8 }
enum E { A }
const C: u8 = 1;
static S: u8 = 1;
type Alias = u8;
extern crate core;
"#;
    let outline = outline("src/lib.rs", source);
    let selectors = selectors(&outline);
    for selector in [
        "mod[local]/fn[run]",
        "mod[external]",
        "struct[Thing]",
        "impl[Thing]/fn[run]",
        "impl[Thing/Trait]/fn[run]",
        "impl[%5Bu8;%204%5D]/fn[edge]",
        "trait[Trait]/fn[read]",
        "trait[Trait]/type[Item]",
        "extern[C]/fn[puts]",
        "extern[Rust]/fn[other]",
        "fn[outer]/fn[inner]",
        "fn[repeated]#1",
        "fn[repeated]#2",
        "macro[make]",
        "union[U]",
        "enum[E]",
        "const[C]",
        "static[S]",
        "type[Alias]",
        "extern-crate[core]",
    ] {
        assert!(
            selectors.contains(&selector.to_owned()),
            "{selector}: {selectors:?}"
        );
        assert_eq!(
            outline.nodes[outline.resolve(selector).unwrap()].selector,
            selector
        );
    }
    assert_eq!(outline.resolve("").unwrap(), 0);
    assert!(outline.resolve("fn[repeated]").is_err());
    assert!(outline.resolve("fn[missing]").is_err());
}

// {test}`requirements.references.structural_hash`
#[test]
fn identity_hashes_include_own_attached_siblings_but_not_descendants() {
    let hashes = |source: &str, selector: &str| {
        let outline = outline("src/lib.rs", source);
        let node = &outline.nodes[outline
            .resolve(selector)
            .unwrap_or_else(|error| panic!("{selector}: {error:?}"))];
        (node.structural_hash.clone(), node.identity_hash.clone())
    };

    // A descendant's attached doc comment and attribute are outside its
    // ancestor's identity hash, though inside its structural hash.
    let base = "mod m {\n    /// Doc.\n    #[inline]\n    fn a() {}\n}\n";
    let (structural, identity) = hashes(base, "mod[m]");
    for edited in [
        base.replace("Doc.", "Changed doc."),
        base.replace("inline", "cold"),
        base.replace("    /// Doc.\n", ""),
    ] {
        let (edited_structural, edited_identity) = hashes(&edited, "mod[m]");
        assert_ne!(edited_structural, structural, "{edited}");
        assert_eq!(edited_identity, identity, "{edited}");
    }

    // A node's own attached doc comment and attribute are part of its identity
    // hash, so they distinguish otherwise identical anonymous nodes.
    let own = |source: &str| {
        let outline = outline("src/lib.rs", source);
        outline.nodes[1].identity_hash.clone()
    };
    let plain = own("use a::A;\n");
    let documented = own("/// Doc.\nuse a::A;\n");
    let attributed = own("#[cfg(test)]\nuse a::A;\n");
    assert_ne!(plain, documented);
    assert_ne!(plain, attributed);
    assert_ne!(own("/// Other.\nuse a::A;\n"), documented);
    assert_ne!(own("#[cfg(not(test))]\nuse a::A;\n"), attributed);
    // The identity hash of a node without descendants is its structural hash.
    let (structural, identity) = hashes("#[cfg(test)]\nfn b() {}\n", "fn[b]");
    assert_eq!(structural, identity);
}

// {test}`requirements.references.selector_use`
// {test}`requirements.references.outline_selectors`
// {test}`requirements.references.structural_hash`
#[test]
fn anonymous_selectors_use_identity_hashes() {
    let before = outline(
        "src/lib.rs",
        "use alpha::A;\nuse beta::B;\nstruct Packet;\n",
    );
    let inserted = outline(
        "src/lib.rs",
        "use gamma::C;\n\nuse alpha::A;\nuse beta::B;\nstruct Packet;\n",
    );
    let changed = outline(
        "src/lib.rs",
        "use alpha::Z;\nuse beta::B;\nstruct Packet;\n",
    );
    let before_selectors = selectors(&before);
    assert!(before_selectors[0].starts_with("use@"));
    assert_eq!(before_selectors[0].len(), "use@".len() + 12);
    assert_eq!(before_selectors[2], "struct[Packet]");
    for selector in &before_selectors {
        assert!(inserted.resolve(selector).is_ok(), "{selector}");
    }
    assert!(changed.resolve(&before_selectors[0]).is_err());
    assert!(changed.resolve(&before_selectors[1]).is_ok());
    let twice = outline("src/lib.rs", "use a::A;\nuse a::A;\n");
    let twice = selectors(&twice);
    assert!(
        twice[0].ends_with("#1") && twice[1].ends_with("#2"),
        "{twice:?}"
    );
}

// {test}`requirements.references.structural_hash`
// {test}`requirements.references.selector_raw_lines`
#[test]
fn hashes_skip_whitespace_and_plain_comments_but_not_doc_comments() {
    let base = concat!(
        "/// Doc.\n#[inline]\n// {cite}",
        "`a.b`\nfn run() { let x = 1; }\n"
    );
    let hash = |source: &str| {
        let outline = outline("src/lib.rs", source);
        let node = &outline.nodes[outline.resolve("fn[run]").unwrap()];
        (node.structural_hash.clone(), node.line_start, node.line_end)
    };
    let (original, start, end) = hash(base);
    assert_eq!((start, end), (1, 4));
    let (moved, start, _) = hash(&format!("\n\n{base}"));
    assert_eq!((moved.as_str(), start), (original.as_str(), 3));
    assert_eq!(
        hash(concat!(
            "/// Doc.\n#[inline]\n// {cite}",
            "`c.d`\nfn run() {\n    let x  =  1;\n}\n"
        ))
        .0,
        original
    );
    assert_ne!(hash(&base.replace("Doc.", "Docs.")).0, original);
    assert_ne!(hash(&base.replace("inline", "cold")).0, original);
    assert_ne!(hash(&base.replace("= 1", "= 2")).0, original);

    let parent = |source: &str| {
        let outline = outline("src/lib.rs", source);
        let node = &outline.nodes[outline.resolve("mod[m]").unwrap()];
        (node.structural_hash.clone(), node.identity_hash.clone())
    };
    let (structural, identity) = parent("mod m { fn a() {} }");
    let (edited_structural, edited_identity) = parent("mod m { fn a() { 1; } }");
    assert_ne!(structural, edited_structural);
    assert_eq!(identity, edited_identity);
}

/// The structural hash of `fn[run]` in a Rust source.
// {cite}`habeas:util`
fn run_hash(source: &str) -> String {
    let outline = outline("src/lib.rs", source);
    outline.nodes[outline.resolve("fn[run]").unwrap()]
        .structural_hash
        .clone()
}

// {test}`requirements.references.structural_hash`
#[test]
fn reflowing_doc_comments_keeps_the_hash() {
    let original = run_hash("/// a\n/// b\nfn run() {}\n");
    assert_eq!(run_hash("/// a b\nfn run() {}\n"), original);
    assert_eq!(run_hash("///   a\n///\n///   b  \nfn run() {}\n"), original);
    assert_eq!(run_hash("/** a\n * b\n */\nfn run() {}\n"), original);
    assert_ne!(run_hash("/// a c\nfn run() {}\n"), original);
    assert_ne!(run_hash("/// b a\nfn run() {}\n"), original);
}

// {test}`requirements.references.structural_hash`
#[test]
fn prose_words_are_encoded_separately() {
    assert_ne!(
        run_hash("/// ab cd\nfn run() {}\n"),
        run_hash("/// abcd\nfn run() {}\n")
    );
    let section = |source: &str| {
        let outline = outline("a.md", source);
        outline.nodes[outline.resolve("section[H]").unwrap()]
            .structural_hash
            .clone()
    };
    assert_ne!(section("# H\nab cd\n"), section("# H\nabcd\n"));
    assert_eq!(section("# H\nab cd\n"), section("# H\n  ab\ncd  \n"));
}

// {test}`requirements.references.structural_hash`
#[test]
fn editing_a_doc_comment_word_changes_the_hash() {
    let original = run_hash("/// Returns the count.\nfn run() {}\n");
    assert_ne!(run_hash("/// Returns the total.\nfn run() {}\n"), original);
    assert_ne!(run_hash("/// Returns  the count\nfn run() {}\n"), original);
    assert_eq!(
        run_hash("/// Returns  the\n/// count.\nfn run() {}\n"),
        original
    );
}

// {test}`requirements.references.structural_hash`
#[test]
fn string_literals_hash_as_words() {
    assert_eq!(
        run_hash("fn run() { \"ab cd\"; }\n"),
        run_hash("fn run() { \"ab  cd\"; }\n")
    );
    assert_ne!(
        run_hash("fn run() { \"ab cd\"; }\n"),
        run_hash("fn run() { \"abcd\"; }\n")
    );
    let pair = |source: &str| {
        let outline = outline("a.toml", source);
        outline.nodes[outline.resolve("pair[a]").unwrap()]
            .structural_hash
            .clone()
    };
    assert_eq!(pair("a = \"a  b\"\n"), pair("a = \"a b\"\n"));
    assert_ne!(pair("a = \"a b\"\n"), pair("a = \"ab\"\n"));
    let block = |source: &str| {
        let outline = outline("a.yml", source);
        outline.nodes[outline.resolve("pair[a]").unwrap()]
            .structural_hash
            .clone()
    };
    let original = block("a: |\n  one two\n  three\n");
    assert_eq!(block("a: |\n  one   two\n    three\n"), original);
    assert_eq!(block("a: |\n  one\n  two three\n"), original);
    assert_ne!(block("a: |\n  onetwo\n  three\n"), original);
}

// {test}`requirements.references.structural_hash`
#[test]
fn reflowing_markdown_paragraphs_keeps_the_hash() {
    let section = |source: &str| {
        let outline = outline("a.md", source);
        outline.nodes[outline.resolve("section[H]").unwrap()]
            .structural_hash
            .clone()
    };
    let original = section("# H\n\nThe quick brown\nfox jumps.\n");
    assert_eq!(section("# H\n\nThe quick\nbrown fox jumps.\n"), original);
    assert_eq!(section("# H\n\nThe quick brown fox jumps.\n"), original);
    assert_ne!(section("# H\n\nThe quick brownfox jumps.\n"), original);
}

// {test}`requirements.references.structural_hash`
#[test]
fn reflowing_inner_and_block_doc_comments_keeps_the_hash() {
    let module = |source: &str| {
        let outline = outline("src/lib.rs", source);
        outline.nodes[outline.resolve("mod[m]").unwrap()]
            .structural_hash
            .clone()
    };
    let original = module("mod m {\n    //! one two\n    //! three\n}\n");
    assert_eq!(
        module("mod m {\n    //! one\n    //! two three\n}\n"),
        original
    );
    assert_eq!(module("mod m {\n    /*! one two three */\n}\n"), original);
    assert_ne!(module("mod m {\n    //! one two\n}\n"), original);
    assert_eq!(
        run_hash("/**\n * a b\n * c\n */\nfn run() {}\n"),
        run_hash("/// a\n/// b c\nfn run() {}\n")
    );
}

/// The selector of the node owning the comment containing `needle`.
// {cite}`habeas:util`
fn comment_owner(outline: &Outline, source: &str, needle: &str) -> String {
    let start = source.find(needle).unwrap();
    let comment = outline
        .comments
        .iter()
        .find(|comment| comment.byte_start <= start && start < comment.byte_end)
        .unwrap_or_else(|| panic!("no comment contains {needle:?}"));
    outline.nodes[comment.owner].selector.clone()
}

// {test}`requirements.references.outline_comments`
#[test]
fn comment_runs_belong_to_the_next_node_or_their_container() {
    let source = "\
//! Crate docs.
fn first() {}
// between first and second
fn second() {} // trailing second
// before third

fn third() {
    // inside third
}

// floating

mod m {
    //! Module docs.
    // above inner
    fn inner() {}

    // alone in m
}
// final
";
    let outline = outline("src/lib.rs", source);
    let owner = |needle: &str| comment_owner(&outline, source, needle);
    assert_eq!(owner("Crate docs"), "");
    assert_eq!(owner("between first"), "fn[second]");
    assert_eq!(owner("trailing second"), "fn[second]");
    assert_eq!(owner("before third"), "fn[third]");
    assert_eq!(owner("inside third"), "fn[third]");
    assert_eq!(owner("floating"), "mod[m]");
    assert_eq!(owner("Module docs"), "mod[m]");
    assert_eq!(owner("above inner"), "mod[m]/fn[inner]");
    assert_eq!(owner("alone in m"), "mod[m]");
    assert_eq!(owner("final"), "");
    let second = &outline.nodes[outline.resolve("fn[second]").unwrap()];
    assert_eq!((second.line_start, second.line_end), (3, 4));
    let third = &outline.nodes[outline.resolve("fn[third]").unwrap()];
    assert_eq!((third.line_start, third.line_end), (5, 9));
    assert_eq!(
        outline
            .innermost_at_line(3)
            .into_iter()
            .map(|index| outline.nodes[index].selector.clone())
            .collect::<Vec<_>>(),
        ["fn[second]"]
    );
}

// {test}`requirements.references.outline_comments`
#[test]
fn comment_between_two_fns_belongs_to_the_next() {
    let source = "fn a() {}\n// one\n// two\nfn b() {}\n";
    let outline = outline("src/lib.rs", source);
    assert_eq!(comment_owner(&outline, source, "one"), "fn[b]");
    assert_eq!(comment_owner(&outline, source, "two"), "fn[b]");
}

// {test}`requirements.references.outline_comments`
#[test]
fn trailing_comment_after_last_fn_in_mod_belongs_to_the_mod() {
    let source = "mod m {\n    fn a() {}\n    // tail\n}\nfn b() {}\n";
    let outline = outline("src/lib.rs", source);
    assert_eq!(comment_owner(&outline, source, "tail"), "mod[m]");
}

// {test}`requirements.references.outline_comments`
#[test]
fn top_level_comment_with_nothing_after_belongs_to_the_root() {
    let source = "fn a() {}\n// tail\n";
    let outline = outline("src/lib.rs", source);
    assert_eq!(comment_owner(&outline, source, "tail"), "");
    let a = &outline.nodes[outline.resolve("fn[a]").unwrap()];
    assert_eq!((a.line_start, a.line_end), (1, 1));
}

// {test}`requirements.references.outline_comments`
#[test]
fn comment_after_code_on_a_line_belongs_to_that_lines_node() {
    let source = "\
mod m {
    fn a() {} // after a
    // before b
    fn b() {
        let x = 1; // inside b
    } // closing b
} // closing m
fn c() {}
";
    let outline = outline("src/lib.rs", source);
    let owner = |needle: &str| comment_owner(&outline, source, needle);
    assert_eq!(owner("after a"), "mod[m]/fn[a]");
    assert_eq!(owner("before b"), "mod[m]/fn[b]");
    assert_eq!(owner("inside b"), "mod[m]/fn[b]");
    assert_eq!(owner("closing b"), "mod[m]/fn[b]");
    assert_eq!(owner("closing m"), "mod[m]");
}

// {test}`requirements.references.outlines`
#[test]
fn line_queries_find_innermost_nodes() {
    let source = "impl Thing { fn run() {} }\n\nfn a() {} fn b() {}\nfn outer() {\n    let x = 1;\n    fn inner() {}\n}\n";
    let outline = outline("src/lib.rs", source);
    let names = |indices: Vec<usize>| {
        indices
            .into_iter()
            .map(|index| outline.nodes[index].selector.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(outline.innermost_at_line(1)), ["impl[Thing]/fn[run]"]);
    assert_eq!(names(outline.innermost_at_line(2)), [""]);
    assert_eq!(names(outline.innermost_at_line(3)), ["fn[a]", "fn[b]"]);
    assert_eq!(names(outline.innermost_at_line(5)), ["fn[outer]"]);
    assert_eq!(names(outline.nodes_for_lines(4..=7, false)), ["fn[outer]"]);
    assert_eq!(
        names(outline.nodes_for_lines(5..=6, false)),
        ["fn[outer]"],
        "an ancestor absorbs its selected descendants"
    );
    assert!(outline.innermost_at_line(99).is_empty());
}

// {test}`requirements.references.outlines`
// {test}`requirements.references.outline_selectors`
#[test]
fn plain_text_lines_are_interchangeable_anonymous_nodes() {
    let outline = outline("notes.txt", "alpha\n\n  beta  \nalpha\n");
    let selectors = selectors(&outline);
    assert_eq!(selectors.len(), 3);
    assert!(
        selectors
            .iter()
            .all(|selector| selector.starts_with("line@"))
    );
    assert_eq!(selectors[0], selectors[2]);
    assert_eq!(outline.resolve(&selectors[0]).unwrap(), 1);
    assert_eq!(outline.nodes[2].structural_hash, super::hash::line("beta"));
    let spaced = super::parse(Path::new("notes.txt"), "two  words\n").unwrap();
    let single = super::parse(Path::new("notes.txt"), "two words\n").unwrap();
    let joined = super::parse(Path::new("notes.txt"), "twowords\n").unwrap();
    assert_eq!(spaced.nodes[1].selector, single.nodes[1].selector);
    assert_ne!(single.nodes[1].selector, joined.nodes[1].selector);
    assert_eq!(outline.innermost_at_line(2), [0]);
    assert_eq!(outline.innermost_at_line(3), [2]);
}

// {test}`requirements.references.outlines`
#[test]
fn reports_parse_failures_with_lines() {
    let error = parse(Path::new("src/lib.rs"), "fn ok() {}\nfn broken( {\n").unwrap_err();
    assert_eq!(error.line, 2, "{error}");
    assert!(parse(Path::new("a.py"), "def f(:\n").is_err());
    let late = parse(Path::new("a.toml"), "a = 1\nb = 2\nc = = 3\nd = [\n").unwrap_err();
    assert_eq!(late.line, 3, "the first error is reported: {late}");
    assert!(late.to_string().starts_with("line 3: "), "{late}");
}

// {test}`requirements.references.outlines`
#[test]
fn profiles_are_selected_by_extension_case_insensitively() {
    for (path, profile) in [
        ("src/LIB.RS", "rust"),
        ("a.Py", "python"),
        ("a.TSX", "tsx"),
        ("README.MD", "markdown"),
        ("Cargo.TOML", "toml"),
        ("notes", "plain-text"),
        ("notes.rs.txt", "plain-text"),
    ] {
        assert_eq!(
            super::profile_for_path(Path::new(path)).name,
            profile,
            "{path}"
        );
    }
}

// {test}`requirements.references.outlines`
#[test]
fn python_profile_outlines_defs_classes_and_statements() {
    let source = "import os\n# lead\n@dec\ndef f(x):\n    return x\nclass C(B):\n    def m(self):\n        pass\nif __name__ == \"__main__\":\n    f(1)\n";
    let outline = outline("a.py", source);
    assert_eq!(
        summary(&outline),
        ["def[f] 2-5", "class[C] 6-8", "class[C]/def[m] 7-8",]
    );
    assert_eq!(
        outline.owner_at_line(1),
        0,
        "a top-level import maps to the root"
    );
    assert_eq!(
        outline.owner_at_line(10),
        0,
        "a top-level `if` maps to the root"
    );
    assert_eq!(outline.innermost_at_line(9), [0]);
    assert_eq!(outline.nodes_for_lines(9..=10, false), [0]);
}

// {test}`requirements.references.outlines`
#[test]
fn typescript_profile_outlines_declarations() {
    let source = "export function f(a: number): void {}\nclass C { m(): void {} x = 1; }\ninterface I { m(): void; }\ntype T = string;\nenum E { A }\nnamespace N { function g() {} }\nconst h = () => 1;\n";
    let outline = outline("a.ts", source);
    assert_eq!(
        selectors(&outline)
            .into_iter()
            .map(|selector| selector.split('@').next().unwrap().to_owned())
            .collect::<Vec<_>>(),
        [
            "function[f]",
            "class[C]",
            "class[C]/method[m]",
            "interface[I]",
            "interface[I]/method[m]",
            "type[T]",
            "enum[E]",
            "namespace[N]",
            "namespace[N]/function[g]",
        ]
    );
    let tsx = super::parse(Path::new("a.tsx"), "function App() { return <div/>; }\n").unwrap();
    assert_eq!(selectors(&tsx), ["function[App]"]);
}

// {test}`requirements.references.outlines`
#[test]
fn css_profile_names_rules_by_normalized_selectors() {
    let source = "/* c */\na  >  b,\n.x { color: red; }\n@media (max-width: 600px) { a { b: c } }\n@keyframes spin { from { x: y } }\n@import \"x.css\";\n";
    let outline = outline("a.css", source);
    assert_eq!(
        summary(&outline),
        [
            "rule[a%20>%20b,%20.x] 1-3",
            "media[(max-width:%20600px)] 4-4",
            "media[(max-width:%20600px)]/rule[a] 4-4",
            "keyframes[spin] 5-5",
        ]
    );
}

// {test}`requirements.references.outlines`
#[test]
fn markdown_profile_nests_sections_by_heading() {
    let source = "---\ntitle: x\n---\nintro\n\n# Head  *one*\npara\n<!-- note -->\n## Sub\ntext\n";
    let outline = outline("a.md", source);
    assert_eq!(
        summary(&outline),
        [
            "section@ 4-5",
            "section[Head%20*one*] 6-10",
            "section[Head%20*one*]/section[Sub] 8-10"
        ]
    );
    assert_eq!(outline.comments.len(), 1);
    let reflowed = super::parse(
        Path::new("a.md"),
        &source.replace("Head  *one*\npara", "Head *one*\n\npara"),
    )
    .unwrap();
    let selector = "section[Head%20*one*]";
    assert_eq!(
        outline.nodes[outline.resolve(selector).unwrap()].structural_hash,
        reflowed.nodes[reflowed.resolve(selector).unwrap()].structural_hash
    );
}

// {test}`requirements.references.outlines`
#[test]
fn toml_and_yaml_profiles_name_keys() {
    let toml = outline(
        "a.toml",
        "# c\na = 1\n[t.u]\n\"q\" = { x = 1 }\n[[arr]]\nb = 2\n[[arr]]\nb = 3\n",
    );
    assert_eq!(
        selectors(&toml),
        [
            "pair[a]",
            "table[t.u]",
            "table[t.u]/pair[\"q\"]",
            "table[t.u]/pair[\"q\"]/pair[x]",
            "table[arr]#1",
            "table[arr]#1/pair[b]",
            "table[arr]#2",
            "table[arr]#2/pair[b]",
        ]
    );
    let yaml = outline(
        "a.yml",
        "# c\na: 1\n\"b\":\n  c: {d: 1}\n  e:\n    - f: 1\n    - 2\n",
    );
    assert_eq!(
        summary(&yaml),
        [
            "pair[a] 1-2",
            "pair[\"b\"] 3-7",
            "pair[\"b\"]/pair[c] 4-4",
            "pair[\"b\"]/pair[c]/pair[d] 4-4",
            "pair[\"b\"]/pair[e] 5-7",
            "pair[\"b\"]/pair[e]/item@ 6-6",
            "pair[\"b\"]/pair[e]/item@/pair[f] 6-6",
            "pair[\"b\"]/pair[e]/item@ 7-7",
        ]
    );
}

// {test}`requirements.references.rust_item_selectors`
// {test}`requirements.references.selector_nested`
// {test}`requirements.references.selector_impl_and_trait`
// {test}`requirements.references.selector_inline_and_foreign`
// {test}`requirements.references.selector_duplicate_names`
// {test}`requirements.references.selector_use`
#[test]
fn selectors_name_syntactic_ancestry_and_duplicates() {
    let source = r#"
use alpha::A;
use beta::B;
mod local { fn run() {} }
mod external;
struct Thing;
impl Thing { fn run() {} }
impl Trait for Thing { fn run() {} }
impl [u8; 4] { fn edge() {} }
trait Trait { fn read(&self); }
extern "C" { fn puts(); }
fn outer() { fn inner() {} }
#[cfg(a)] fn repeated() {}
#[cfg(b)] fn repeated() {}
"#;
    let selectors = selectors(&outline("lib.rs", source));
    assert!(selectors[0].starts_with("use@") && selectors[1].starts_with("use@"));
    assert_ne!(selectors[0], selectors[1]);
    for selector in [
        "mod[local]/fn[run]",
        "mod[external]",
        "struct[Thing]",
        "impl[Thing]/fn[run]",
        "impl[Thing/Trait]/fn[run]",
        "impl[%5Bu8;%204%5D]/fn[edge]",
        "trait[Trait]/fn[read]",
        "extern[C]/fn[puts]",
        "fn[outer]/fn[inner]",
        "fn[repeated]#1",
        "fn[repeated]#2",
    ] {
        assert!(
            selectors.contains(&selector.to_owned()),
            "{selector}: {selectors:?}"
        );
    }
}

// {test}`requirements.references.structural_hash`
#[test]
fn markdown_hashes_include_paragraph_text_between_punctuation_tokens() {
    let hash = |source: &str| outline("a.md", source).nodes[1].structural_hash.clone();
    assert_ne!(
        hash("# Skill\nUse ready.\n"),
        hash("# Skill\nUse ready now.\n")
    );
    assert_eq!(
        hash("# Skill\nUse ready.\n"),
        hash("# Skill\nUse   ready.\n"),
        "whitespace is not hashed"
    );
}

// {test}`requirements.references.outlines`
// {test}`requirements.references.outline_comments`
#[test]
fn file_level_inner_doc_comments_and_unmapped_code_belong_to_the_root() {
    let source = "//! Crate docs.\n//! More.\nfn a() {}\n";
    let lines = outline("src/lib.rs", source);
    assert_eq!(comment_owner(&lines, source, "Crate docs"), "");
    assert_eq!(comment_owner(&lines, source, "More"), "");
    assert_eq!(lines.owner_at_line(1), 0);
    let a = &lines.nodes[lines.resolve("fn[a]").unwrap()];
    assert_eq!((a.line_start, a.line_end), (3, 3));
    let source = "/*! Crate docs. */\nfn a() {}\n";
    let block = outline("src/lib.rs", source);
    assert_eq!(comment_owner(&block, source, "Crate docs"), "");
    let markdown = outline("a.md", "---\ntitle: x\n---\n# H\n");
    assert_eq!(
        markdown.owner_at_line(2),
        0,
        "front matter maps to the root"
    );
}

// {test}`requirements.references.outlines`
#[test]
fn no_profile_invents_fallback_kinds() {
    let sources = [
        ("src/lib.rs", "use a::b;\nfn f() {}\n"),
        ("a.py", "import os\nx = 1\nif x:\n    pass\n"),
        ("a.ts", "const h = () => 1;\nimport x from \"y\";\n"),
        ("a.css", "@import \"x.css\";\n@charset \"utf-8\";\n"),
        ("a.md", "---\ntitle: x\n---\n"),
        ("a.toml", "a = 1\n"),
        ("a.yml", "- 1\n- 2\n"),
        ("a.yml", "plain scalar\n"),
    ];
    for (path, source) in sources {
        let outline = outline(path, source);
        for node in &outline.nodes[1..] {
            assert!(
                !matches!(node.kind, "stmt" | "block" | "node")
                    && !(node.kind == "item" && !path.ends_with(".yml")),
                "{path}: fallback kind {:?}",
                node.selector
            );
        }
    }
    assert_eq!(outline("a.py", "x = 1\n").nodes.len(), 1);
    assert_eq!(outline("a.ts", "const h = 1;\n").nodes.len(), 1);
    assert_eq!(outline("a.yml", "plain scalar\n").nodes.len(), 1);
}

// {test}`requirements.references.outline_comments`
#[test]
fn comment_runs_follow_the_same_rules_in_every_language() {
    // Each case: path, source, and (needle, expected owner selector) pairs.
    // Every language has a run before a node, a trailing comment, a comment
    // after its container's last node, and a top-level tail comment that
    // belongs to the root.
    type Case = (
        &'static str,
        &'static str,
        &'static [(&'static str, &'static str)],
    );
    let cases: &[Case] = &[
        (
            "a.py",
            "# lead one\n# lead two\ndef f():\n    pass  # trailing f\nclass C:\n    def m(self):\n        pass\n    # before n\n    def n(self):\n        pass\n# tail\n",
            &[
                ("lead one", "def[f]"),
                ("lead two", "def[f]"),
                ("trailing f", "def[f]"),
                ("before n", "class[C]/def[n]"),
                ("tail", ""),
            ],
        ),
        (
            "a.ts",
            "// lead\nfunction f() {} // trailing f\nclass C {\n  m() {}\n  // after m\n}\n/* tail */\n",
            &[
                ("lead", "function[f]"),
                ("trailing f", "function[f]"),
                ("after m", "class[C]"),
                ("tail", ""),
            ],
        ),
        (
            "a.tsx",
            "// lead\nfunction App() { return <div/>; } // trailing\n// tail\n",
            &[
                ("lead", "function[App]"),
                ("trailing", "function[App]"),
                ("tail", ""),
            ],
        ),
        (
            "a.css",
            "/* lead */\na { color: red; } /* trailing a */\n@media print {\n  b { x: y }\n  /* after b */\n}\n/* tail */\n",
            &[
                ("lead", "rule[a]"),
                ("trailing a", "rule[a]"),
                ("after b", "media[print]"),
                ("tail", ""),
            ],
        ),
        (
            "a.toml",
            "# lead\na = 1 # trailing a\n[t]\nb = 2\n# after b\n",
            &[
                ("lead", "pair[a]"),
                ("trailing a", "pair[a]"),
                ("after b", "table[t]"),
            ],
        ),
        (
            "a.yml",
            "# lead\na: 1 # trailing a\n# before b\nb:\n  c: 1\n  # inner\n  d: 2\n",
            &[
                ("lead", "pair[a]"),
                ("trailing a", "pair[a]"),
                ("before b", "pair[b]"),
                ("inner", "pair[b]/pair[d]"),
            ],
        ),
        (
            "a.md",
            "# H\npara\n<!-- before Sub -->\n## Sub\ntext\n\n<!-- end of Sub -->\n",
            &[
                ("before Sub", "section[H]/section[Sub]"),
                ("end of Sub", "section[H]/section[Sub]"),
            ],
        ),
    ];
    for (path, source, expected) in cases {
        let outline = outline(path, source);
        for (needle, owner) in *expected {
            assert_eq!(
                comment_owner(&outline, source, needle),
                *owner,
                "{path}: owner of {needle:?}"
            );
        }
    }
}

// {test}`requirements.references.outline_comments`
#[test]
fn markdown_comments_are_html_comment_blocks_not_inline_comments() {
    let source = "# H\nText <!-- inline --> more.\n\n<!-- block -->\n## Sub\n";
    let outline = outline("a.md", source);
    assert_eq!(outline.comments.len(), 1, "only the block is a comment");
    assert_eq!(
        comment_owner(&outline, source, "block"),
        "section[H]/section[Sub]"
    );
    let inline = source.find("inline").unwrap();
    assert!(
        outline
            .comments
            .iter()
            .all(|comment| !(comment.byte_start <= inline && inline < comment.byte_end))
    );
    assert_eq!(
        outline.nodes[outline.owner_at_line(2)].selector,
        "section[H]",
        "an inline HTML comment is paragraph text of its section"
    );
}

// {test}`requirements.references.outline_comments`
#[test]
fn a_comment_after_another_comment_on_its_line_forms_its_own_run() {
    let source = "fn a() {}\n/* first */ /* second */\nfn b() {}\n";
    let outline = outline("src/lib.rs", source);
    assert_eq!(comment_owner(&outline, source, "first"), "fn[b]");
    // The second comment follows other text on its line, so it forms a run
    // by itself and belongs to the innermost node containing that line; no
    // item contains line 2, so that is the root.
    assert_eq!(comment_owner(&outline, source, "second"), "");
}

// {test}`requirements.references.selector_duplicate_names`
#[test]
fn duplicate_names_are_numbered_in_source_order_without_line_numbers() {
    let source = "#[cfg(a)]\nfn run() { a() }\n#[cfg(b)]\nfn run() { b() }\n";
    let first = outline("src/lib.rs", source);
    assert_eq!(selectors(&first), ["fn[run]#1", "fn[run]#2"]);
    let node = |outline: &Outline, selector: &str| {
        outline.nodes[outline.resolve(selector).unwrap()].clone()
    };
    assert_eq!(node(&first, "fn[run]#1").line_start, 1);
    assert_eq!(node(&first, "fn[run]#2").line_start, 3);
    assert!(
        source[node(&first, "fn[run]#1").byte_start..].starts_with("fn run() { a() }"),
        "#1 is the first declaration in the file"
    );
    let swapped = outline(
        "src/lib.rs",
        "#[cfg(b)]\nfn run() { b() }\n#[cfg(a)]\nfn run() { a() }\n",
    );
    assert_eq!(
        node(&swapped, "fn[run]#1").structural_hash,
        node(&first, "fn[run]#2").structural_hash,
        "swapping the declarations swaps their ordinals"
    );
    let moved = outline("src/lib.rs", &format!("\n\n\n{source}"));
    assert_eq!(selectors(&moved), ["fn[run]#1", "fn[run]#2"]);
    assert_eq!(node(&moved, "fn[run]#1").line_start, 4);
    assert_eq!(
        node(&moved, "fn[run]#1").structural_hash,
        node(&first, "fn[run]#1").structural_hash,
        "moving the declarations changes neither selector"
    );
}

// {test}`requirements.references.rust_item_selectors`
#[test]
fn item_macros_are_anonymous_nodes_and_expression_macros_are_not() {
    let source = "\
thread_local! { static X: u8 = 1; }
println!();
impl<T> Trait for Wrapper<T> {
    m!();
    fn run() { inner!(); let x = vec![1]; }
}
mod local { other! {} }
";
    let outline = outline("src/lib.rs", source);
    let selectors = selectors(&outline);
    let anonymous = regex::Regex::new("^macro@[0-9a-f]{12}$").unwrap();
    assert!(anonymous.is_match(&selectors[0]), "{selectors:?}");
    assert!(anonymous.is_match(&selectors[1]), "{selectors:?}");
    assert_ne!(selectors[0], selectors[1]);
    assert_eq!(selectors[2], "impl[Wrapper<T>/Trait]");
    assert!(
        selectors[3].starts_with("impl[Wrapper<T>/Trait]/macro@"),
        "{selectors:?}"
    );
    assert_eq!(selectors[4], "impl[Wrapper<T>/Trait]/fn[run]");
    assert!(
        selectors[5].starts_with("mod[local]") && selectors[6].starts_with("mod[local]/macro@"),
        "{selectors:?}"
    );
    assert_eq!(
        selectors.len(),
        7,
        "macros inside a function body are expressions, not nodes: {selectors:?}"
    );
}

// {test}`requirements.references.outline_comments`
#[test]
fn yaml_comments_before_a_key_belong_to_that_key_not_the_previous_one() {
    let source = "jobs:\n  first:\n    steps:\n      - run: a\n  # note for second\n  second:\n    steps:\n      - run: b\n";
    let outline = parse(std::path::Path::new("ci.yml"), source).unwrap();
    let owners = outline.comment_owners_at_line(5);
    assert_eq!(owners.len(), 1, "{owners:?}");
    assert_eq!(outline.nodes[owners[0]].selector, "pair[jobs]/pair[second]");
}

// {test}`requirements.references.selector_inline_and_foreign`
#[test]
fn functions_in_a_separate_module_file_are_selected_within_that_file() {
    // `src/lib.rs` declares `mod local;`; `src/local.rs` holds its body. The
    // function's selector is relative to `src/local.rs`, without the module
    // path that the declaration in another file implies.
    let declaring = selectors(&outline("lib.rs", "mod local;\n"));
    assert_eq!(declaring, vec!["mod[local]".to_owned()]);
    let module = selectors(&outline("local.rs", "pub fn run() {}\n"));
    assert_eq!(module, vec!["fn[run]".to_owned()]);
    assert!(
        !module
            .iter()
            .any(|selector| selector.contains("mod[local]"))
    );
}

/// The structural hash of the node at `selector` in a source at `path`.
// {cite}`habeas:util`
fn node_hash(path: &str, source: &str, selector: &str) -> String {
    let outline = outline(path, source);
    let index = outline
        .resolve(selector)
        .unwrap_or_else(|error| panic!("{path} {selector}: {error:?}"));
    outline.nodes[index].structural_hash.clone()
}

// {test}`requirements.references.structural_hash`
#[test]
fn comments_are_excluded_from_hashes_in_every_other_profile() {
    // Each case: path, selector, original source, sources differing from it
    // only in comments, and a source with a changed non-comment word.
    let cases: &[(&str, &str, &str, &[&str], &str)] = &[
        (
            "a.py",
            "def[f]",
            "def f(x):\n    # note\n    return x  # trailing\n",
            &[
                "def f(x):\n    # other note\n    return x  # changed\n",
                "def f(x):\n    return x\n",
            ],
            "def f(x):\n    # note\n    return y  # trailing\n",
        ),
        (
            "a.ts",
            "function[f]",
            "function f() {\n  // note\n  return /* inline */ 1;\n}\n",
            &[
                "function f() {\n  // other note\n  return /* changed */ 1;\n}\n",
                "function f() {\n  return 1;\n}\n",
            ],
            "function f() {\n  // note\n  return /* inline */ 2;\n}\n",
        ),
        (
            "a.css",
            "rule[a]",
            "a { /* note */ color: red; }\n",
            &[
                "a { /* other note */ color: red; }\n",
                "a { color: red; }\n",
            ],
            "a { /* note */ color: blue; }\n",
        ),
        (
            "a.md",
            "section[H]",
            "# H\n\n<!-- note -->\n\ntext\n",
            &["# H\n\n<!-- other note -->\n\ntext\n", "# H\n\ntext\n"],
            "# H\n\n<!-- note -->\n\nprose\n",
        ),
        (
            "a.toml",
            "table[t]",
            "[t]\n# note\na = 1 # trailing\n",
            &["[t]\n# other note\na = 1 # changed\n", "[t]\na = 1\n"],
            "[t]\n# note\na = 2 # trailing\n",
        ),
        (
            "a.yml",
            "pair[a]",
            "a:\n  # note\n  b: 1 # trailing\n",
            &["a:\n  # other note\n  b: 1 # changed\n", "a:\n  b: 1\n"],
            "a:\n  # note\n  b: 2 # trailing\n",
        ),
    ];
    for (path, selector, source, same, changed) in cases {
        let original = node_hash(path, source, selector);
        for edited in *same {
            assert_eq!(
                node_hash(path, edited, selector),
                original,
                "{path}: {edited}"
            );
        }
        assert_ne!(
            node_hash(path, changed, selector),
            original,
            "{path}: {changed}"
        );
    }
}

// {test}`requirements.references.structural_hash`
#[test]
fn python_decorators_attach_to_the_decorated_definition_for_hashing() {
    let source = "@cache\ndef f():\n    pass\n";
    let original = node_hash("a.py", source, "def[f]");
    assert_ne!(
        node_hash("a.py", "@other\ndef f():\n    pass\n", "def[f]"),
        original
    );
    assert_ne!(
        node_hash("a.py", "def f():\n    pass\n", "def[f]"),
        original
    );
    assert_eq!(
        node_hash("a.py", "@cache\n\n\ndef f():\n  pass\n", "def[f]"),
        original
    );

    // A method's decorator is in its class's structural hash but outside the
    // class's identity hash.
    let class = |source: &str| {
        let outline = outline("a.py", source);
        let node = &outline.nodes[outline.resolve("class[C]").unwrap()];
        (node.structural_hash.clone(), node.identity_hash.clone())
    };
    let (structural, identity) = class("class C:\n    @cache\n    def m(self):\n        pass\n");
    let (edited_structural, edited_identity) =
        class("class C:\n    @other\n    def m(self):\n        pass\n");
    assert_ne!(edited_structural, structural);
    assert_eq!(edited_identity, identity);
}

// {test}`requirements.references.structural_hash`
#[test]
fn plain_text_line_hashes_ignore_whitespace_but_not_word_breaks() {
    let line = |source: &str| {
        outline("notes.txt", source).nodes[1]
            .structural_hash
            .clone()
    };
    let original = line("two words\n");
    assert_eq!(original, super::hash::line("two words"));
    for edited in [
        "  two words\n",
        "two words  \n",
        "two \t  words\n",
        "\ttwo words \n",
    ] {
        assert_eq!(line(edited), original, "{edited:?}");
    }
    assert_ne!(line("twowords\n"), original);
    assert_ne!(line("two word\n"), original);
}

// {test}`requirements.references.structural_hash`
#[test]
fn inner_and_outer_doc_comments_hash_in_separate_groups() {
    let module = |source: &str| node_hash("src/lib.rs", source, "mod[m]");
    // Moving a word from a nested item's outer doc comment into the module's
    // adjacent inner doc comment changes the module's hash.
    assert_ne!(
        module("mod m {\n    //! one\n    /// two\n    fn f() {}\n}\n"),
        module("mod m {\n    //! one two\n    fn f() {}\n}\n")
    );
    // Within one group, splitting words across comments keeps the hash.
    assert_eq!(
        module("mod m {\n    //! one\n    //! two\n    fn f() {}\n}\n"),
        module("mod m {\n    //! one two\n    fn f() {}\n}\n")
    );
}

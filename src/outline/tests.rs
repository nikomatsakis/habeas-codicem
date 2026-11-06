use std::path::Path;

use super::{Outline, parse};

// {test}`requirements.references.outlines`
fn outline(path: &str, source: &str) -> Outline {
    parse(Path::new(path), source).unwrap_or_else(|error| panic!("{path}: {error}"))
}

// {test}`requirements.references.outlines`
fn selectors(outline: &Outline) -> Vec<String> {
    outline.nodes[1..]
        .iter()
        .map(|node| node.selector.clone())
        .collect()
}

// {test}`requirements.references.outlines`
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
    let base = "/// Doc.\n#[inline]\n// {cite}`a.b`\nfn run() { let x = 1; }\n";
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
        hash("/// Doc.\n#[inline]\n// {cite}`c.d`\nfn run() {\n    let x  =  1;\n}\n").0,
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

// {test}`requirements.references.outline_comments`
#[test]
fn comments_belong_to_containing_adjacent_or_next_nodes() {
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
    let owner = |needle: &str| {
        let start = source.find(needle).unwrap();
        let comment = outline
            .comments
            .iter()
            .find(|comment| comment.byte_start <= start && start < comment.byte_end)
            .unwrap();
        outline.nodes[comment.owner].selector.clone()
    };
    assert_eq!(owner("Crate docs"), "");
    assert_eq!(owner("between first"), "fn[second]");
    assert_eq!(owner("trailing second"), "fn[second]");
    assert_eq!(owner("before third"), "fn[second]");
    assert_eq!(owner("inside third"), "fn[third]");
    assert_eq!(owner("floating"), "mod[m]");
    assert_eq!(owner("Module docs"), "mod[m]");
    assert_eq!(owner("above inner"), "mod[m]/fn[inner]");
    assert_eq!(owner("alone in m"), "mod[m]");
    assert_eq!(owner("final"), "mod[m]");
    let second = &outline.nodes[outline.resolve("fn[second]").unwrap()];
    assert_eq!((second.line_start, second.line_end), (3, 5));
    assert_eq!(
        outline
            .innermost_at_line(3)
            .into_iter()
            .map(|index| outline.nodes[index].selector.clone())
            .collect::<Vec<_>>(),
        ["fn[second]"]
    );
}

// {test}`requirements.references.outlines`
// {test}`requirements.references.selector_same_line`
#[test]
fn line_queries_find_innermost_and_declared_nodes() {
    let source = "impl Thing { fn run() {} }\n\nfn a() {} fn b() {}\nfn outer() {\n    let x = 1;\n    fn inner() {}\n}\n";
    let outline = outline("src/lib.rs", source);
    let names = |indices: Vec<usize>| {
        indices
            .into_iter()
            .map(|index| outline.nodes[index].selector.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names(outline.declared_on_line(1)),
        ["impl[Thing]", "impl[Thing]/fn[run]"]
    );
    assert_eq!(names(outline.innermost_at_line(1)), ["impl[Thing]/fn[run]"]);
    assert_eq!(names(outline.innermost_at_line(2)), [""]);
    assert_eq!(names(outline.innermost_at_line(3)), ["fn[a]", "fn[b]"]);
    assert_eq!(names(outline.innermost_at_line(5)), ["fn[outer]"]);
    assert_eq!(names(outline.innermost_in_range(4, 7)), ["fn[outer]"]);
    assert_eq!(
        names(outline.innermost_in_range(5, 6)),
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
    assert_eq!(
        outline.nodes[2].structural_hash,
        super::hash::digest("beta")
    );
    assert_eq!(outline.innermost_at_line(2), [0]);
    assert_eq!(outline.innermost_at_line(3), [2]);
}

// {test}`requirements.references.outlines`
#[test]
fn reports_parse_failures_with_lines() {
    let error = parse(Path::new("src/lib.rs"), "fn ok() {}\nfn broken( {\n").unwrap_err();
    assert_eq!(error.line, 2, "{error}");
    assert!(parse(Path::new("a.py"), "def f(:\n").is_err());
}

// {test}`requirements.references.outlines`
#[test]
fn python_profile_outlines_defs_classes_and_statements() {
    let source = "import os\n# lead\n@dec\ndef f(x):\n    return x\nclass C(B):\n    def m(self):\n        pass\nif __name__ == \"__main__\":\n    f(1)\n";
    let outline = outline("a.py", source);
    assert_eq!(
        summary(&outline),
        [
            "stmt@ 1-1",
            "def[f] 2-5",
            "class[C] 6-8",
            "class[C]/def[m] 7-8",
            "stmt@ 9-10"
        ]
    );
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
            "stmt"
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
            "stmt@ 6-6"
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
            "block@ 1-3",
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

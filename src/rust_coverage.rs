use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::evidence::{Declaration, InspectionProgress};
use crate::outline::{self, OutlineError};
use crate::reference_roles;

/// The inspection phase label for Rust function coverage.
// {impl}`requirements.authoring.progress`
const RUST_COVERAGE_STAGE: &str = "Rust function coverage";

// {cite}`habeas:util`
fn quoted_end(bytes: &[u8], start: usize) -> usize {
    let mut position = start + 1;
    while position < bytes.len() {
        match bytes[position] {
            b'\\' => position = (position + 2).min(bytes.len()),
            b'"' => return position + 1,
            _ => position += 1,
        }
    }
    bytes.len()
}

// {cite}`habeas:util`
fn char_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut position = start + 1;
    if position >= bytes.len() {
        return None;
    }
    if bytes[position] == b'\\' {
        position += 1;
        if bytes.get(position) == Some(&b'u') && bytes.get(position + 1) == Some(&b'{') {
            position += 2;
            while bytes.get(position).is_some_and(|byte| *byte != b'}') {
                position += 1;
            }
            position += 1;
        } else if bytes.get(position) == Some(&b'x') {
            position += 3;
        } else {
            position += 1;
        }
    } else {
        position += source[position..].chars().next()?.len_utf8();
    }
    (bytes.get(position) == Some(&b'\'')).then_some(position + 1)
}

// {cite}`habeas:util`
fn raw_string_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut position = match bytes.get(start..) {
        Some([b'r', ..]) => start + 1,
        Some([b'b' | b'c', b'r', ..]) => start + 2,
        _ => return None,
    };
    let mut hashes = 0;
    while bytes.get(position) == Some(&b'#') {
        hashes += 1;
        position += 1;
    }
    if bytes.get(position) != Some(&b'"') {
        return None;
    }
    position += 1;
    while position < bytes.len() {
        if bytes[position] == b'"'
            && bytes
                .get(position + 1..position + 1 + hashes)
                .is_some_and(|suffix| suffix.iter().all(|byte| *byte == b'#'))
        {
            return Some(position + 1 + hashes);
        }
        position += 1;
    }
    Some(bytes.len())
}

// {cite}`habeas:util`
/// Byte ranges of the comments in Rust source, skipping string and character
/// literals; used to find term references in recorded Rust inputs.
pub(crate) fn comments(source: &str) -> Vec<Range<usize>> {
    let bytes = source.as_bytes();
    let mut ranges = Vec::new();
    let mut position = 0;
    while position < bytes.len() {
        match (bytes[position], bytes.get(position + 1)) {
            (b'/', Some(b'/')) => {
                let start = position;
                position += 2;
                while bytes.get(position).is_some_and(|byte| *byte != b'\n') {
                    position += 1;
                }
                ranges.push(start..position);
            }
            (b'/', Some(b'*')) => {
                let start = position;
                let mut depth = 1;
                position += 2;
                while position < bytes.len() && depth > 0 {
                    match (bytes[position], bytes.get(position + 1)) {
                        (b'/', Some(b'*')) => {
                            depth += 1;
                            position += 2;
                        }
                        (b'*', Some(b'/')) => {
                            depth -= 1;
                            position += 2;
                        }
                        _ => position += 1,
                    }
                }
                ranges.push(start..position);
            }
            (b'r' | b'b' | b'c', _) => {
                position = raw_string_end(bytes, position).unwrap_or(position + 1);
            }
            (b'"', _) => position = quoted_end(bytes, position),
            (b'\'', _) => {
                position = char_end(source, position).unwrap_or(position + 1);
            }
            _ => position += 1,
        }
    }
    ranges
}

// {cite}`habeas:util`
/// The outline inputs recorded by every evidence record, citation review,
/// and site review, as `file -> item -> hashes`, read once per validation.
/// An item is covered when its node or an enclosing node is recorded here
/// with its current hash.
#[derive(Default)]
pub(crate) struct EvidenceReads(HashMap<String, HashMap<String, HashSet<String>>>);

impl EvidenceReads {
    /// Reads the outline inputs of the given evidence files. A file that
    /// cannot be read or parsed contributes nothing; evidence validation
    /// reports it.
    pub(crate) fn load(files: &[PathBuf]) -> Self {
        let mut reads = Self::default();
        for file in files {
            let Ok(text) = fs::read_to_string(file) else {
                continue;
            };
            let Ok(record) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            reads.add_record(&record);
        }
        reads
    }

    /// Adds the outline inputs of one record.
    pub(crate) fn add_record(&mut self, record: &serde_json::Value) {
        for input in record
            .get("inputs")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            let field = |name: &str| input.get(name).and_then(serde_json::Value::as_str);
            if let (Some(file), Some(item), Some(hash)) =
                (field("file"), field("item"), field("hash"))
            {
                self.0
                    .entry(file.to_owned())
                    .or_default()
                    .entry(item.to_owned())
                    .or_default()
                    .insert(hash.to_owned());
            }
        }
    }

    /// Whether node `index` of `file`'s outline, or a node containing it, is
    /// recorded with its current hash.
    pub(crate) fn covers(&self, file: &str, outline: &outline::Outline, index: usize) -> bool {
        let Some(items) = self.0.get(file) else {
            return false;
        };
        let mut current = Some(index);
        while let Some(index) = current {
            let node = &outline.nodes[index];
            if items
                .get(&node.selector)
                .is_some_and(|hashes| hashes.contains(&node.structural_hash))
            {
                return true;
            }
            current = node.parent;
        }
        false
    }
}

// {cite}`habeas:util`
/// An item Rust item coverage reports: its file, outline selector, and the
/// validation error naming it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UncoveredItem {
    pub(crate) file: String,
    pub(crate) item: String,
    /// How the item is reported.
    pub(crate) error: String,
    /// Whether it lies inside a module named `tests`, which is not reported.
    pub(crate) in_tests: bool,
}

/// The validation error for an uncovered item.
fn uncovered_error(label: &str, line: usize, kind: &str, name: &str) -> String {
    format!(
        "{label}:{line}: {kind} {name} has no recent coverage: it has no requirement or example reference, and no recent evidence record or review read it"
    )
}

/// Lists functions, methods, structs, enums, and unions with no valid
/// reference in a comment belonging to them, as `(line, kind, name)`.
// {impl}`requirements.references.rust_item_coverage`
#[cfg(test)]
fn missing_function_references(
    source: &str,
    definitions: &BTreeMap<String, Declaration>,
) -> Result<Vec<(usize, &'static str, String)>, OutlineError> {
    let (_, missing) = unreferenced_items(source, definitions)?;
    Ok(missing
        .into_iter()
        .map(|(_, line, kind, name)| (line, kind, name))
        .collect())
}

/// Items without a valid reference, as `(node, line, kind, name)`.
type Unreferenced = Vec<(usize, usize, &'static str, String)>;

/// A test attribute: `#[test]`, or one whose path ends in `test`, such as
/// `#[tokio::test]`.
static TEST_ATTRIBUTE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?m)^\s*#\[\s*(?:[A-Za-z_][A-Za-z0-9_]*\s*::\s*)*test\s*[\](]")
        .expect("test attribute pattern")
});

/// Test functions of a Rust source without a valid `{test}` reference, as
/// `(line, name)`.
// {impl}`requirements.references.coverage.tests`
pub(crate) fn untested_tests(
    source: &str,
    outline: &outline::Outline,
    definitions: &BTreeMap<String, Declaration>,
) -> Vec<(usize, String)> {
    let counts = |kind: crate::ReferenceKind, name: &str| {
        kind == crate::ReferenceKind::Test
            && definitions
                .get(name)
                .is_some_and(|d| d.kind != crate::DefinitionKind::Term)
    };
    let mut tested = vec![false; outline.nodes.len()];
    for comment in &outline.comments {
        if reference_roles(&source[comment.byte_start..comment.byte_end], false)
            .iter()
            .any(|role| counts(role.kind, role.name))
        {
            tested[comment.owner] = true;
        }
    }
    for fence in crate::fenced::comment_fences(source, outline) {
        if let Some(owner) = fence.owner
            && fence.closed
            && counts(fence.kind, &fence.name)
        {
            tested[owner] = true;
        }
    }
    let lines: Vec<&str> = source.lines().collect();
    outline
        .nodes
        .iter()
        .enumerate()
        .filter(|(index, node)| node.kind == "fn" && !tested[*index])
        .filter(|(_, node)| {
            // Attributes are attached before the declaration line.
            let start = node.line_start.saturating_sub(1);
            let end = node.declaration_line.min(lines.len());
            lines
                .get(start..end)
                .is_some_and(|attached| TEST_ATTRIBUTE.is_match(&attached.join("\n")))
        })
        .filter_map(|(_, node)| Some((node.declaration_line, node.display_name()?)))
        .collect()
}

/// The functions, methods, structs, enums, and unions of a Rust source with
/// no valid reference in a comment belonging to them, with the source's
/// outline.
// {impl}`requirements.references.builtin_definitions`
// {impl}`requirements.references.procedure`
// {impl}`requirements.references.outline_comments`
// {impl}`requirements.references.roles`
// {impl}`requirements.definitions.references`
// {impl}`requirements.references.util`
pub(crate) fn unreferenced_items(
    source: &str,
    definitions: &BTreeMap<String, Declaration>,
) -> Result<(outline::Outline, Unreferenced), OutlineError> {
    let outline = outline::parse_with(&outline::RUST_PROFILE, source)?;
    let mut cited = vec![false; outline.nodes.len()];
    // A built-in counts only in a role it permits.
    let counts = |kind: crate::ReferenceKind, name: &str| {
        // A `{fixme}` acknowledges a gap; it does not say why the item exists.
        if matches!(
            kind,
            crate::ReferenceKind::Term | crate::ReferenceKind::Fixme
        ) {
            return false;
        }
        if let Some(builtin) = crate::builtins::builtin(name) {
            return builtin.check(kind).is_ok();
        }
        definitions.get(name).is_some_and(|d| match d.kind {
            crate::DefinitionKind::Requirement => true,
            // An example counts only when it traces to a
            // requirement itself.
            crate::DefinitionKind::Example => {
                crate::evidence::example_references_requirement(d, definitions)
            }
            crate::DefinitionKind::Term => false,
        })
    };
    for comment in &outline.comments {
        if reference_roles(&source[comment.byte_start..comment.byte_end], false)
            .iter()
            .any(|role| counts(role.kind, role.name))
        {
            cited[comment.owner] = true;
        }
    }
    for fence in crate::fenced::comment_fences(source, &outline) {
        if let Some(owner) = fence.owner
            && fence.closed
            && counts(fence.kind, &fence.name)
        {
            cited[owner] = true;
        }
    }
    let missing = outline
        .nodes
        .iter()
        .zip(cited)
        .enumerate()
        .filter_map(|(index, (node, cited))| {
            let kind = item_kind(node.kind)?;
            let name = node.display_name()?;
            (!cited).then_some((index, node.declaration_line, kind, name))
        })
        .collect();
    Ok((outline, missing))
}

/// The kind coverage reports for an outline node kind that must be covered:
/// functions and methods, structs, enums, and unions.
pub(crate) fn item_kind(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "fn" => "function",
        "struct" => "struct",
        "enum" => "enum",
        "union" => "union",
        _ => return None,
    })
}

/// The first-party Rust files among the sources reference discovery
/// considers: every `.rs` file outside the project-root `vendor` directory.
pub(crate) fn rust_files(root: &Path, sources: &[PathBuf]) -> Vec<PathBuf> {
    let vendor = root.join("vendor");
    sources
        .iter()
        .filter(|path| path.extension().is_some_and(|value| value == "rs"))
        .filter(|path| !path.starts_with(&vendor))
        .cloned()
        .collect()
}

/// The package's Cargo example targets: each `.rs` file directly under
/// `examples/`, each `examples/<name>/main.rs`, and each `[[example]]` path.
// {impl}`requirements.references.coverage.examples`
pub(crate) fn cargo_examples(root: &Path) -> Vec<PathBuf> {
    let mut targets = std::collections::BTreeSet::new();
    if let Ok(entries) = fs::read_dir(root.join("examples")) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|extension| extension == "rs") {
                targets.insert(path);
            } else if path.join("main.rs").is_file() {
                targets.insert(path.join("main.rs"));
            }
        }
    }
    let manifest = fs::read_to_string(root.join("Cargo.toml"))
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok());
    for example in manifest
        .as_ref()
        .and_then(|manifest| manifest.get("example"))
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(path) = example.get("path").and_then(toml::Value::as_str) {
            targets.insert(root.join(path));
        }
    }
    targets.into_iter().filter(|path| path.is_file()).collect()
}

/// Reports each Cargo example target that does not `{cite}` a named example
/// of the book.
// {impl}`requirements.references.coverage.examples`
pub(crate) fn validate_cargo_examples(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    references: &[crate::evidence::SourceReference],
    errors: &mut Vec<String>,
) {
    for target in cargo_examples(root) {
        let target = fs::canonicalize(&target).unwrap_or(target);
        let cites_example = references.iter().any(|reference| {
            reference.file == target
                && reference.kind == crate::ReferenceKind::Cite
                && definitions
                    .get(&reference.name)
                    .is_some_and(|d| d.kind == crate::DefinitionKind::Example)
        });
        if !cites_example {
            errors.push(format!(
                "{}: Cargo example does not {{cite}} a named example of the book",
                display_path(root, &target)
            ));
        }
    }
}

// {impl}`requirements.ci.errors`
// {impl}`requirements.references.procedure`
pub(crate) fn validate_rust_functions(
    root: &Path,
    sources: &[PathBuf],
    definitions: &BTreeMap<String, Declaration>,
    reads: &EvidenceReads,
    errors: &mut Vec<String>,
    discovery_errors: &mut Vec<String>,
    progress: &mut impl FnMut(InspectionProgress),
) -> Vec<UncoveredItem> {
    let mut uncovered = Vec::new();
    progress(InspectionProgress {
        stage: RUST_COVERAGE_STAGE,
        completed: 0,
        total: None,
    });
    let files = rust_files(root, sources);
    let total = files.len();
    for (completed, file) in files.into_iter().enumerate() {
        progress(InspectionProgress {
            stage: RUST_COVERAGE_STAGE,
            completed,
            total: Some(total),
        });
        let label = display_path(root, &file);
        let source = match fs::read_to_string(&file) {
            Ok(source) => source,
            Err(error) => {
                let error = format!("{label}: cannot read Rust source: {error}");
                discovery_errors.push(error.clone());
                errors.push(error);
                continue;
            }
        };
        match unreferenced_items(&source, definitions) {
            Ok((outline, missing)) => {
                // Only a test without `{test}` fails validation; other items
                // without coverage are reported, not errors.
                // {impl}`requirements.references.coverage.tests`
                for (line, name) in untested_tests(&source, &outline, definitions) {
                    errors.push(format!(
                        "{label}:{line}: test function {name} has no {{test}} reference to a requirement or example"
                    ));
                }
                for (index, line, kind, name) in missing {
                    if reads.covers(&label, &outline, index) {
                        continue;
                    }
                    uncovered.push(UncoveredItem {
                        file: label.clone(),
                        item: outline.nodes[index].selector.clone(),
                        error: uncovered_error(&label, line, kind, &name),
                        in_tests: crate::coverage_history::in_tests_module(&outline, index),
                    });
                }
            }
            Err(error) => {
                let error = format!(
                    "{label}:{}: cannot parse Rust source: {}",
                    error.line, error.message
                );
                discovery_errors.push(error.clone());
                errors.push(error);
            }
        }
    }
    progress(InspectionProgress {
        stage: RUST_COVERAGE_STAGE,
        completed: total,
        total: Some(total),
    });
    uncovered
}

// {cite}`habeas:util`
fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use tempfile::TempDir;

    use super::{comments, missing_function_references, validate_rust_functions};
    use crate::DefinitionKind;
    use crate::evidence::Declaration;

    /// The text of each comment `comments` finds in `source`.
    // {test}`requirements.definitions.dependencies`
    fn comment_texts(source: &str) -> Vec<&str> {
        comments(source)
            .into_iter()
            .map(|range| &source[range])
            .collect()
    }

    // {test}`requirements.definitions.dependencies`
    #[test]
    fn comment_scan_skips_string_and_character_literals() {
        for (source, expected) in [
            // Strings, including escaped quotes and comment markers inside them.
            (r#"let s = "// no"; // yes"#, vec!["// yes"]),
            (r#"let s = "a \" // no"; /* yes */"#, vec!["/* yes */"]),
            (r#"let s = b"// no"; // yes"#, vec!["// yes"]),
            (r#"let s = c"/* no */"; // yes"#, vec!["// yes"]),
            // Raw strings, with and without hashes, may contain quotes.
            (r#"let s = r"// no"; // yes"#, vec!["// yes"]),
            (r##"let s = r#"" // no "#; // yes"##, vec!["// yes"]),
            (r###"let s = br##"#" // no"##; // yes"###, vec!["// yes"]),
            (r#"let s = cr"// no"; // yes"#, vec!["// yes"]),
            // Character literals, including quote characters and escapes.
            (r#"let c = '"'; // yes"#, vec!["// yes"]),
            (r"let c = '\''; // yes", vec!["// yes"]),
            (r#"let c = ('\\', "// no"); // yes"#, vec!["// yes"]),
            (
                r#"let c = ('\u{1F600}', '\x41', "// no"); // yes"#,
                vec!["// yes"],
            ),
            (r#"let c = ('é', "// no"); // yes"#, vec!["// yes"]),
            // A lifetime is not a character literal.
            (
                r#"fn f<'a>(x: &'a str) -> &'a str { "// no" } // yes"#,
                vec!["// yes"],
            ),
            // Block comments nest, and line comments end at the newline.
            (
                "/* a /* b */ c */ d // e\nf",
                vec!["/* a /* b */ c */", "// e"],
            ),
            (
                "/** doc */ //! inner\n/// outer",
                vec!["/** doc */", "//! inner", "/// outer"],
            ),
            // Unterminated literals and comments extend to the end of the source.
            (r#"let s = "// no"#, vec![]),
            (r##"let s = r#"// no"##, vec![]),
            (
                "/* open /* nested */ still",
                vec!["/* open /* nested */ still"],
            ),
            ("let c = '", vec![]),
        ] {
            assert_eq!(comment_texts(source), expected, "{source}");
        }
    }

    // {cite}`habeas:util`
    fn definitions() -> BTreeMap<String, Declaration> {
        BTreeMap::from([
            (
                "requirements.example".to_owned(),
                Declaration {
                    kind: DefinitionKind::Requirement,
                    text: "Example.".to_owned(),
                    source: "md/requirements.md:1".to_owned(),
                    file: "md/requirements.md".to_owned(),
                    line_start: 1,
                    line_end: 1,
                    raw_lines: vec![],
                    anchors: vec![],
                },
            ),
            (
                "requirements.case".to_owned(),
                Declaration {
                    kind: DefinitionKind::Example,
                    text: "Given the example, it works. {cite}`requirements.example`".to_owned(),
                    source: "md/requirements.md:5".to_owned(),
                    file: "md/requirements.md".to_owned(),
                    line_start: 5,
                    line_end: 5,
                    raw_lines: vec![],
                    anchors: vec![],
                },
            ),
        ])
    }

    // {test}`requirements.references.rust_item_coverage`
    #[test]
    fn a_read_covers_an_item_only_with_its_current_hash() {
        let source = "fn read() {}\n";
        let outline = crate::outline::parse_with(&crate::outline::RUST_PROFILE, source).unwrap();
        let index = outline
            .nodes
            .iter()
            .position(|node| node.selector == "fn[read]")
            .unwrap();
        let record = |hash: &str| serde_json::json!({"inputs": [{"file": "src/lib.rs", "item": "fn[read]", "hash": hash}]});
        let mut current = super::EvidenceReads::default();
        current.add_record(&record(&outline.nodes[index].structural_hash));
        assert!(current.covers("src/lib.rs", &outline, index));
        let mut stale = super::EvidenceReads::default();
        stale.add_record(&record(&"0".repeat(64)));
        assert!(!stale.covers("src/lib.rs", &outline, index));
    }

    // {test}`requirements.references.coverage.tests`
    #[test]
    fn test_functions_need_a_test_reference() {
        let source = concat!(
            "// {test}`requirements.example`\n#[test]\nfn tested() {}\n\n",
            "#[test]\nfn tested_in_body() {\n    // {test}`requirements.example`\n}\n\n",
            "#[test]\nfn untested() {}\n\n",
            "// {cite}`requirements.example`\n#[tokio::test]\nasync fn only_cited() {}\n\n",
            "#[tokio::test(flavor = \"multi_thread\")]\nasync fn untested_async() {}\n\n",
            "#[cfg(test)]\nfn not_a_test() {}\n\n",
            "#[rstest]\nfn other_attribute() {}\n\n",
            "mod tests {\n    #[test]\n    fn nested_untested() {}\n}\n",
        );
        let outline = crate::outline::parse_with(&crate::outline::RUST_PROFILE, source).unwrap();
        let names: Vec<String> = super::untested_tests(source, &outline, &definitions())
            .into_iter()
            .map(|(_, name)| name)
            .collect();
        assert_eq!(
            names,
            [
                "untested",
                "only_cited",
                "untested_async",
                "nested_untested"
            ]
        );
    }

    // {test}`requirements.references.coverage.examples`
    #[test]
    fn cargo_examples_must_cite_a_book_example() {
        let project = TempDir::new().unwrap();
        let root = project.path();
        fs::create_dir_all(root.join("examples/demo")).unwrap();
        fs::create_dir_all(root.join("extra")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = 'x'\n\n[[example]]\nname = 'listed'\npath = 'extra/listed.rs'\n",
        )
        .unwrap();
        fs::write(
            root.join("examples/cites.rs"),
            "// {cite}`requirements.case`\nfn main() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("examples/requirement.rs"),
            "// {cite}`requirements.example`\nfn main() {}\n",
        )
        .unwrap();
        fs::write(root.join("examples/demo/main.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("extra/listed.rs"), "fn main() {}\n").unwrap();
        let root = fs::canonicalize(root).unwrap();
        let mut definitions = definitions();
        definitions.insert(
            "requirements.case".to_owned(),
            Declaration {
                kind: DefinitionKind::Example,
                ..definitions["requirements.example"].clone()
            },
        );
        let mut found = Vec::new();
        let references = crate::evidence::collect_references(&root, &mut found);
        let mut errors = Vec::new();
        super::validate_cargo_examples(&root, &definitions, &references, &mut errors);
        errors.sort();
        // A citation of a requirement, rather than an example, is not enough.
        assert_eq!(
            errors,
            [
                "examples/demo/main.rs: Cargo example does not {cite} a named example of the book",
                "examples/requirement.rs: Cargo example does not {cite} a named example of the book",
                "extra/listed.rs: Cargo example does not {cite} a named example of the book",
            ]
        );
    }

    // {test}`requirements.references.rust_item_coverage`
    // {test}`requirements.references.outline_comments`
    #[test]
    fn finds_uncited_functions_without_using_enclosing_or_string_references() {
        let source = r##"
// {impl}`requirements.example`
mod feature {
    // {impl}`requirements.example`
    impl Widget {
        fn missing_method() {}
    }

    struct Widget;
    impl Widget {
        // {impl}`requirements.example`
        fn cited_method() {}
    }

    fn containing_string() {
        let _ = "// {impl}`requirements.example`";
        let _ = r#"/* {impl}`requirements.example` */"#;
        let _ = '/';
    }

    fn unknown_reference() {
        // {impl}`requirements.unknown`
    }

    fn body_reference() {
        /* {impl}`requirements.example` */
    }

    // A comment run belongs to the next item even across a blank line.
    // {impl}`requirements.example`

    fn after_blank_line() {}

    // {impl}`requirements.example`
    fn previous() {} // {impl}`requirements.example`
    fn after_trailing_comment() {}
}

mod inner_docs {
    //! {impl}`requirements.example`
    fn after_inner_doc() {}
}

/// {impl}`requirements.example`
fn cited_by_outer_doc() {}
"##;
        let missing = missing_function_references(source, &definitions()).unwrap();
        assert_eq!(
            missing
                .into_iter()
                .map(|(_, _, name)| name)
                .collect::<Vec<_>>(),
            [
                "missing_method",
                "Widget",
                "containing_string",
                "unknown_reference",
                "after_trailing_comment",
                "after_inner_doc"
            ]
        );
    }

    // {test}`requirements.references.rust_item_coverage`
    #[test]
    fn checks_trait_test_helper_and_nested_functions_separately() {
        let source = r#"
trait Work {
    fn uncited_trait_method(&self);
    // {impl}`requirements.example`
    fn cited_trait_method(&self);
}
fn outer() {
    // {impl}`requirements.example`
    fn inner() {}
    inner();
}
// {test}`requirements.example`
#[test]
fn tagged_test() {}
// {test}`requirements.case`
#[test]
fn named_example_test() {}
fn test_helper() {}
"#;
        let missing = missing_function_references(source, &definitions()).unwrap();
        assert_eq!(
            missing
                .into_iter()
                .map(|(_, _, name)| name)
                .collect::<Vec<_>>(),
            ["uncited_trait_method", "outer", "test_helper"]
        );
    }

    // {test}`requirements.references.rust_item_coverage`
    #[test]
    fn an_example_counts_only_when_it_references_a_requirement() {
        let mut definitions = definitions();
        let example = |text: &str| Declaration {
            kind: DefinitionKind::Example,
            text: text.to_owned(),
            source: "md/requirements.md:9".to_owned(),
            file: "md/requirements.md".to_owned(),
            line_start: 9,
            line_end: 9,
            raw_lines: vec![],
            anchors: vec![],
        };
        definitions.insert(
            "requirements.tested".to_owned(),
            example("A tested case. {test}`requirements.example`"),
        );
        definitions.insert("requirements.orphan".to_owned(), example("No requirement."));
        definitions.insert(
            "requirements.chained".to_owned(),
            example("Only another example. {cite}`requirements.case`"),
        );
        definitions.insert(
            "terms.word".to_owned(),
            Declaration {
                kind: DefinitionKind::Term,
                ..example("A word.")
            },
        );
        definitions.insert(
            "requirements.termed".to_owned(),
            example("Only a term. {cite}`terms.word`"),
        );
        let source = r#"
// {test}`requirements.case`
fn cites_example() {}
// {test}`requirements.tested`
fn tests_example() {}
// {test}`requirements.orphan`
fn orphan_example() {}
// {test}`requirements.chained`
fn chained_example() {}
// {test}`requirements.termed`
fn termed_example() {}
// {impl}`terms.word`
fn names_a_term() {}
"#;
        let missing = missing_function_references(source, &definitions).unwrap();
        assert_eq!(
            missing
                .into_iter()
                .map(|(_, _, name)| name)
                .collect::<Vec<_>>(),
            [
                "orphan_example",
                "chained_example",
                "termed_example",
                "names_a_term"
            ]
        );
    }

    // {test}`requirements.references.rust_item_coverage`
    #[test]
    fn locates_functions_after_unicode_text() {
        let source = r#"
// {impl}`requirements.example`
fn outer() {
    let _ = "é";
    /* {impl}`requirements.example` */ fn cited_inner() {}
    fn missing_inner() {}
}
"#;
        let missing = missing_function_references(source, &definitions()).unwrap();
        assert_eq!(missing, [(6, "function", "missing_inner".to_owned())]);
    }

    // {test}`requirements.references.rust_item_coverage`
    #[test]
    fn reports_all_files_and_parse_failures() {
        let project = TempDir::new().unwrap();
        fs::create_dir_all(project.path().join("src")).unwrap();
        fs::create_dir_all(project.path().join("src/evidence")).unwrap();
        fs::create_dir_all(project.path().join("tests")).unwrap();
        fs::create_dir_all(project.path().join("target")).unwrap();
        fs::create_dir_all(project.path().join("evidence")).unwrap();
        fs::write(project.path().join("src/lib.rs"), "fn uncited() {}\n").unwrap();
        fs::write(
            project.path().join("src/evidence/check.rs"),
            "fn nested_source() {}\n",
        )
        .unwrap();
        fs::write(project.path().join("tests/broken.rs"), "fn unfinished( {\n").unwrap();
        fs::write(
            project.path().join("target/generated.rs"),
            "fn ignored() {}\n",
        )
        .unwrap();
        fs::write(
            project.path().join("evidence/fixture.rs"),
            "fn ignored() {}\n",
        )
        .unwrap();
        // Build output and repository metadata are skipped at any depth; the
        // book, evidence, and vendored sources only at the project root.
        for (file, source) in [
            ("crates/sub/target/debug/build.rs", "fn ignored() {}\n"),
            (".git/hooks/hook.rs", "fn ignored() {}\n"),
            ("vendor/dep/lib.rs", "fn ignored() {}\n"),
            ("book/listing.rs", "fn ignored() {}\n"),
            ("src/vendor/own.rs", "fn own_vendor() {}\n"),
            ("src/book/own.rs", "fn own_book() {}\n"),
        ] {
            let path = project.path().join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, source).unwrap();
        }

        let mut errors = Vec::new();
        let uncovered = validate_rust_functions(
            project.path(),
            &crate::evidence::discovered_files(project.path(), &mut Vec::new()),
            &definitions(),
            &super::EvidenceReads::default(),
            &mut errors,
            &mut Vec::new(),
            &mut |_| {},
        );
        // Uncovered items are reported; only the parse failure is an error.
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(uncovered.len(), 4, "{uncovered:?}");
        let errors: Vec<String> = errors
            .into_iter()
            .chain(uncovered.into_iter().map(|item| item.error))
            .collect();
        for expected in [
            "src/vendor/own.rs:1: function own_vendor",
            "src/book/own.rs:1: function own_book",
        ] {
            assert!(
                errors.iter().any(|error| error.contains(expected)),
                "{expected}: {errors:?}"
            );
        }
        assert!(errors.iter().any(|error| {
            error.contains("src/lib.rs:1: function uncited has no recent coverage: it has no requirement or example reference, and no recent evidence record or review read it")
        }));
        assert!(errors.iter().any(|error| {
            error.contains(
                "src/evidence/check.rs:1: function nested_source has no recent coverage: it has no requirement or example reference, and no recent evidence record or review read it",
            )
        }));
        assert!(
            errors
                .iter()
                .any(|error| error.contains("tests/broken.rs:1: cannot parse Rust source"))
        );
    }

    // {test}`requirements.authoring.progress`
    #[test]
    fn rust_function_coverage_reports_counted_progress_per_file() {
        let project = TempDir::new().unwrap();
        fs::create_dir_all(project.path().join("src")).unwrap();
        for name in ["a", "b", "c"] {
            fs::write(project.path().join(format!("src/{name}.rs")), "").unwrap();
        }
        let mut updates = Vec::new();
        validate_rust_functions(
            project.path(),
            &crate::evidence::discovered_files(project.path(), &mut Vec::new()),
            &definitions(),
            &super::EvidenceReads::default(),
            &mut Vec::new(),
            &mut Vec::new(),
            &mut |update| updates.push((update.stage, update.completed, update.total)),
        );
        let stage = "Rust function coverage";
        assert_eq!(
            updates,
            [
                (stage, 0, None),
                (stage, 0, Some(3)),
                (stage, 1, Some(3)),
                (stage, 2, Some(3)),
                (stage, 3, Some(3)),
            ]
        );
    }

    // {test}`requirements.references.rust_item_coverage`
    #[test]
    fn structs_enums_and_unions_need_references_attached_or_inside() {
        let source = r##"
// {impl}`requirements.example`
#[derive(Debug)]
struct Attached {
    field: u8,
}

struct Inside {
    // {impl}`requirements.example`
    #[allow(dead_code)]
    field: u8,
}

enum Variants {
    // {cite}`requirements.example`
    One,
}

struct Missing;

enum MissingEnum {
    One,
}

union MissingUnion {
    value: u8,
}

// {impl}`requirements.example`
fn outer() {
    struct Nested;
}
"##;
        let missing = missing_function_references(source, &definitions()).unwrap();
        assert_eq!(
            missing,
            vec![
                (19, "struct", "Missing".to_owned()),
                (21, "enum", "MissingEnum".to_owned()),
                (25, "union", "MissingUnion".to_owned()),
                (31, "struct", "Nested".to_owned()),
            ]
        );
    }
}

use std::collections::BTreeMap;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::evidence::{Declaration, InspectionProgress};
use crate::reference_roles;

/// The inspection phase label for Rust function coverage.
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

/// Recognize the final path segment of the outer attribute attached to this function.
fn is_test_attribute(source: &str) -> bool {
    use syn::ext::IdentExt;
    use syn::parse::Parser;

    syn::Attribute::parse_outer
        .parse_str(source)
        .is_ok_and(|attrs| {
            attrs.iter().any(|attr| {
                attr.path()
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident.unraw() == "test")
            })
        })
}

/// Test functions of a Rust source without a `{test}` annotation, as
/// `(node, line, name)`.
fn untested_tests(
    source: &str,
) -> Result<Vec<(usize, usize, String)>, trust_but_verify::outline::OutlineError> {
    use trust_but_verify::outline::{AttachmentKind, OutlineLang, languages::Rust};
    let public = Rust.outline(source)?;
    if let Some(error) = public.diagnostic {
        return Err(error);
    }
    let items = crate::outline_items::OutlineItems::new(&public, source);
    let mut tested = vec![false; public.items.len()];
    for comment in &items.comments {
        if reference_roles(&source[comment.span.clone()], false)
            .iter()
            .any(|role| role.kind == crate::ReferenceKind::Test)
        {
            tested[comment.owner] = true;
        }
    }
    for fence in crate::fenced::comment_fences(source, &items, true) {
        if let Some(owner) = fence.owner
            && fence.closed
            && fence.kind == crate::ReferenceKind::Test
        {
            tested[owner] = true;
        }
    }
    Ok(public
        .items
        .iter()
        .enumerate()
        .filter(|(index, item)| {
            item.kind == "fn"
                && !tested[*index]
                && item.attachments.iter().any(|attachment| {
                    attachment.kind == AttachmentKind::Annotation
                        && is_test_attribute(&source[attachment.span])
                })
        })
        .filter_map(|(index, item)| {
            let name = item.name_span?;
            Some((index, items.line_at(name.start), source[name].to_owned()))
        })
        .collect())
}

/// The first-party Rust files among the sources reference discovery
/// considers: every `.rs` file outside the project-root `vendor` directory.
// {cite}`habeas:util`
pub(crate) fn rust_files(root: &Path, sources: &[PathBuf]) -> Vec<PathBuf> {
    let vendor = root.join("vendor");
    sources
        .iter()
        .filter(|path| path.extension().is_some_and(|value| value == "rs"))
        .filter(|path| !path.starts_with(&vendor))
        .cloned()
        .collect()
}

/// The package's Cargo example targets among `files`, the sources reference
/// discovery considers: each `.rs` file directly under `examples/`, each
/// `examples/<name>/main.rs`, and each `[[example]]` path.
pub(crate) fn cargo_examples(root: &Path, files: &[PathBuf]) -> Vec<PathBuf> {
    let mut targets = std::collections::BTreeSet::new();
    let examples = root.join("examples");
    for file in files {
        let Ok(relative) = file.strip_prefix(&examples) else {
            continue;
        };
        let parts: Vec<_> = relative.iter().collect();
        let target = match parts.as_slice() {
            [name] => Path::new(name)
                .extension()
                .is_some_and(|extension| extension == "rs"),
            [_, main] => *main == "main.rs",
            _ => false,
        };
        if target {
            targets.insert(file.clone());
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
            let path = root.join(path);
            let path = fs::canonicalize(&path).unwrap_or(path);
            if files.contains(&path) {
                targets.insert(path);
            }
        }
    }
    targets.into_iter().collect()
}

/// Reports each Cargo example target that does not `{cite}` a named example
/// of the book.
pub(crate) fn validate_cargo_examples(
    root: &Path,
    files: &[PathBuf],
    definitions: &BTreeMap<String, Declaration>,
    references: &[crate::evidence::SourceReference],
    errors: &mut Vec<String>,
) {
    for target in cargo_examples(root, files) {
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

pub(crate) fn validate_rust_functions(
    root: &Path,
    sources: &[PathBuf],
    errors: &mut Vec<String>,
    discovery_errors: &mut Vec<String>,
    progress: &mut impl FnMut(InspectionProgress),
) {
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
        match untested_tests(&source) {
            Ok(tests) => {
                for (_, line, name) in tests {
                    errors.push(format!(
                        "{label}:{line}: test function {name} has no {{test}} annotation"
                    ));
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

    use super::{comments, validate_rust_functions};
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

    // {test}`requirements.references.coverage.tests`
    // {test}`requirements.references.procedure`
    // {test}`requirements.references.outline_comments`
    #[test]
    fn test_functions_need_a_test_reference() {
        let source = concat!(
            "// {test}`requirements.example`\n#[test]\nfn tested() {}\n\n",
            "#[test]\nfn tested_in_body() {\n    // {test}`requirements.example`\n}\n\n",
            "// :::{test} requirements.example\n// Closed fenced reference.\n// :::\n#[test]\nfn tested_with_fence() {}\n\n",
            "// :::{test} requirements.example\n// Unclosed fenced reference.\n#[test]\nfn unclosed_fence() {}\n\n",
            "// :::{test} requirements.missing\n// :::\n#[test]\nfn unknown_fenced_target() {}\n\n",
            "#[test]\nfn untested() {}\n\n",
            "// {cite}`requirements.example`\n#[tokio::test]\nasync fn only_cited() {}\n\n",
            "#[tokio::test(flavor = \"multi_thread\")]\nasync fn untested_async() {}\n\n",
            "#[cfg(test)]\nfn not_a_test() {}\n\n",
            "#[rstest]\nfn other_attribute() {}\n\n",
            "mod tests {\n    #[test]\n    fn nested_untested() {}\n}\n",
            "mod inline { #[cfg(test)] #[tokio::test] async fn inline_test() {} fn helper() {} }\n",
            "// {impl}`requirements.example`\n#[test] fn only_implemented() {}\n",
            "#[doc = \"a ] #[test] string\"] fn documented_helper() {}\n",
        );
        let multiline = "#[test]\nfn\n    café() {}";
        assert_eq!(
            super::untested_tests(multiline).unwrap(),
            [(1, 3, "café".into())]
        );
        let names: Vec<String> = super::untested_tests(source)
            .unwrap()
            .into_iter()
            .map(|(_, _, name)| name)
            .collect();
        assert_eq!(
            names,
            [
                "unclosed_fence",
                "untested",
                "only_cited",
                "untested_async",
                "nested_untested",
                "inline_test",
                "only_implemented"
            ]
        );
    }

    // {test}`requirements.references.coverage.tests`
    // {test}`requirements.references.procedure`
    #[test]
    fn test_reference_errors_ignore_unannotated_helpers() {
        let project = TempDir::new().unwrap();
        fs::create_dir_all(project.path().join("tests")).unwrap();
        fs::write(
            project.path().join("tests/foo.rs"),
            "#[test]\nfn bare() {}\n\nfn helper() {}\n",
        )
        .unwrap();
        let mut errors = Vec::new();
        validate_rust_functions(
            project.path(),
            &crate::evidence::discovered_files(project.path(), &mut Vec::new()),
            &mut errors,
            &mut Vec::new(),
            &mut |_| {},
        );
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].contains("tests/foo.rs:2: test function bare"),
            "{errors:?}"
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
        let files = crate::evidence::discovered_files(&root, &mut found);
        super::validate_cargo_examples(&root, &files, &definitions, &references, &mut errors);
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
        for file in [
            "examples/demo/main.rs",
            "extra/listed.rs",
            "examples/requirement.rs",
        ] {
            fs::write(
                root.join(file),
                "// {cite}`requirements.case`\nfn main() {}\n",
            )
            .unwrap();
        }
        for file in [
            "examples/demo/helper.rs",
            "examples/a/b/main.rs",
            "examples/data.txt",
        ] {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "fn helper() {}\n").unwrap();
        }
        let references = crate::evidence::collect_references(&root, &mut found);
        let files = crate::evidence::discovered_files(&root, &mut found);
        let mut errors = Vec::new();
        super::validate_cargo_examples(&root, &files, &definitions, &references, &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
    }

    // {test}`requirements.references.rust_item_coverage.parsing`
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
        fs::write(project.path().join("src/evidence/check.rs"), "fn {").unwrap();
        fs::write(project.path().join("tests/broken.rs"), "fn unfinished( {\n").unwrap();
        fs::write(project.path().join("target/generated.rs"), "fn {").unwrap();
        fs::write(project.path().join("evidence/fixture.rs"), "fn {").unwrap();
        // Build output and repository metadata are skipped at any depth; the
        // book, evidence, and vendored sources only at the project root.
        for (file, source) in [
            ("crates/sub/target/debug/build.rs", "fn {"),
            (".git/hooks/hook.rs", "fn {"),
            ("vendor/dep/lib.rs", "fn {"),
            ("book/listing.rs", "fn {"),
            ("src/vendor/own.rs", "fn {"),
            ("src/book/own.rs", "fn {"),
        ] {
            let path = project.path().join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, source).unwrap();
        }

        let mut errors = Vec::new();
        validate_rust_functions(
            project.path(),
            &crate::evidence::discovered_files(project.path(), &mut Vec::new()),
            &mut errors,
            &mut Vec::new(),
            &mut |_| {},
        );
        assert_eq!(errors.len(), 4, "{errors:?}");
        for file in [
            "tests/broken.rs",
            "src/evidence/check.rs",
            "src/vendor/own.rs",
            "src/book/own.rs",
        ] {
            assert!(
                errors
                    .iter()
                    .any(|error| error.contains(&format!("{file}:1: cannot parse Rust source"))),
                "{errors:?}"
            );
        }
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
}

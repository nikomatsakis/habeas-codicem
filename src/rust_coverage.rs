use std::collections::BTreeMap;
use std::fs;
use std::ops::Range;
use std::path::Path;

use crate::evidence::{Declaration, InspectionProgress};
use crate::outline::{self, OutlineError};
use crate::reference_roles;

/// The inspection phase label for Rust function coverage.
const RUST_COVERAGE_STAGE: &str = "Rust function coverage";

// {impl}`requirements.references.rust_item_coverage`
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

// {impl}`requirements.references.rust_item_coverage`
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

// {impl}`requirements.references.rust_item_coverage`
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

// {impl}`requirements.references.rust_item_coverage`
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

/// Lists functions, methods, structs, enums, and unions with no valid
/// reference in a comment belonging to them, as `(line, kind, name)`.
// {impl}`requirements.references.rust_item_coverage`
// {impl}`requirements.definitions.references`
fn missing_function_references(
    source: &str,
    definitions: &BTreeMap<String, Declaration>,
) -> Result<Vec<(usize, &'static str, String)>, OutlineError> {
    let outline = outline::parse_with(&outline::RUST_PROFILE, source)?;
    let mut cited = vec![false; outline.nodes.len()];
    for comment in &outline.comments {
        if reference_roles(&source[comment.byte_start..comment.byte_end], false)
            .iter()
            .any(|role| {
                role.kind != crate::ReferenceKind::Term
                    && definitions
                        .get(role.name)
                        .is_some_and(|d| d.kind != crate::DefinitionKind::Term)
            })
        {
            cited[comment.owner] = true;
        }
    }
    Ok(outline
        .nodes
        .iter()
        .zip(cited)
        .filter_map(|(node, cited)| {
            let kind = match node.kind {
                "fn" => "function",
                "struct" => "struct",
                "enum" => "enum",
                "union" => "union",
                _ => return None,
            };
            let name = node.display_name()?;
            (!cited).then_some((node.declaration_line, kind, name))
        })
        .collect())
}

// {impl}`requirements.references.rust_item_coverage`
pub(crate) fn rust_files(root: &Path, errors: &mut Vec<String>) -> Vec<std::path::PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = stack.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                errors.push(format!(
                    "{}: cannot scan Rust sources: {error}",
                    display_path(root, &directory)
                ));
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    errors.push(format!(
                        "{}: cannot read Rust source directory entry: {error}",
                        display_path(root, &directory)
                    ));
                    continue;
                }
            };
            let path = entry.path();
            let kind = match entry.file_type() {
                Ok(kind) => kind,
                Err(error) => {
                    errors.push(format!(
                        "{}: cannot inspect Rust source: {error}",
                        display_path(root, &path)
                    ));
                    continue;
                }
            };
            if kind.is_dir() {
                let name = entry.file_name();
                if !matches!(name.to_str(), Some(".git" | "target"))
                    && !(directory == root
                        && matches!(name.to_str(), Some("book" | "evidence" | "vendor")))
                {
                    stack.push(path);
                }
            } else if kind.is_file() && path.extension().is_some_and(|value| value == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

// {impl}`requirements.references.rust_item_coverage`
// {impl}`requirements.authoring.progress`
pub(crate) fn validate_rust_functions(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    errors: &mut Vec<String>,
    discovery_errors: &mut Vec<String>,
    progress: &mut impl FnMut(InspectionProgress),
) {
    progress(InspectionProgress {
        stage: RUST_COVERAGE_STAGE,
        completed: 0,
        total: None,
    });
    let initial_errors = errors.len();
    let files = rust_files(root, errors);
    discovery_errors.extend(errors[initial_errors..].iter().cloned());
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
        match missing_function_references(&source, definitions) {
            Ok(missing) => {
                for (line, kind, name) in missing {
                    errors.push(format!(
                        "{label}:{line}: {kind} {name} has no requirement or example citation"
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

// {impl}`requirements.references.rust_item_coverage`
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

    use super::{missing_function_references, validate_rust_functions};
    use crate::DefinitionKind;
    use crate::evidence::Declaration;

    // {test}`requirements.references.rust_item_coverage`
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
                    text: "Given the example, it works. CITE`requirements.example`"
                        .replace("CITE", "{cite}"),
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
    fn finds_uncited_functions_without_using_enclosing_or_string_references() {
        let source = r##"
// @impl@`requirements.example`
mod feature {
    // @impl@`requirements.example`
    impl Widget {
        fn missing_method() {}
    }

    struct Widget;
    impl Widget {
        // @impl@`requirements.example`
        fn cited_method() {}
    }

    fn containing_string() {
        let _ = "// @impl@`requirements.example`";
        let _ = r#"/* @impl@`requirements.example` */"#;
        let _ = '/';
    }

    fn unknown_reference() {
        // @impl@`requirements.unknown`
    }

    fn body_reference() {
        /* @impl@`requirements.example` */
    }

    // A comment run belongs to the next item even across a blank line.
    // @impl@`requirements.example`

    fn after_blank_line() {}

    // @impl@`requirements.example`
    fn previous() {} // @impl@`requirements.example`
    fn after_trailing_comment() {}
}

mod inner_docs {
    //! @impl@`requirements.example`
    fn after_inner_doc() {}
}

/// @impl@`requirements.example`
fn cited_by_outer_doc() {}
"##
        .replace("@impl@", "{impl}");
        let missing = missing_function_references(&source, &definitions()).unwrap();
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
    // @impl@`requirements.example`
    fn cited_trait_method(&self);
}
fn outer() {
    // @impl@`requirements.example`
    fn inner() {}
    inner();
}
// @test@`requirements.example`
#[test]
fn tagged_test() {}
// @test@`requirements.case`
#[test]
fn named_example_test() {}
fn test_helper() {}
"#
        .replace("@impl@", "{impl}")
        .replace("@test@", "{test}");
        let missing = missing_function_references(&source, &definitions()).unwrap();
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
    fn locates_functions_after_unicode_text() {
        let source = r#"
// @impl@`requirements.example`
fn outer() {
    let _ = "é";
    /* @impl@`requirements.example` */ fn cited_inner() {}
    fn missing_inner() {}
}
"#
        .replace("@impl@", "{impl}");
        let missing = missing_function_references(&source, &definitions()).unwrap();
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

        let mut errors = Vec::new();
        validate_rust_functions(
            project.path(),
            &definitions(),
            &mut errors,
            &mut Vec::new(),
            &mut |_| {},
        );
        assert_eq!(errors.len(), 3, "{errors:?}");
        assert!(errors.iter().any(|error| {
            error.contains("src/lib.rs:1: function uncited has no requirement or example citation")
        }));
        assert!(errors.iter().any(|error| {
            error.contains(
                "src/evidence/check.rs:1: function nested_source has no requirement or example citation",
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
            &definitions(),
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
// @impl@`requirements.example`
#[derive(Debug)]
struct Attached {
    field: u8,
}

struct Inside {
    // @impl@`requirements.example`
    #[allow(dead_code)]
    field: u8,
}

enum Variants {
    // @cite@`requirements.example`
    One,
}

struct Missing;

enum MissingEnum {
    One,
}

union MissingUnion {
    value: u8,
}

// @impl@`requirements.example`
fn outer() {
    struct Nested;
}
"##
        .replace('@', "");
        let source = source
            .replace("impl`", "{impl}`")
            .replace("cite`", "{cite}`");
        let missing = missing_function_references(&source, &definitions()).unwrap();
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

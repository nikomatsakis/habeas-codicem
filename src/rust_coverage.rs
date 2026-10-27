use std::collections::BTreeMap;
use std::fs;
use std::ops::Range;
use std::path::Path;

use proc_macro2::Span;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

use crate::evidence::Declaration;
use crate::reference_roles;

// {impl}`requirements.references.rust_function_coverage`
struct Function {
    name: String,
    line: usize,
    start: usize,
    end: usize,
}

// {impl}`requirements.references.rust_function_coverage`
struct FunctionCollector {
    functions: Vec<Function>,
}

impl FunctionCollector {
    // {impl}`requirements.references.rust_function_coverage`
    fn add(&mut self, name: &syn::Ident, span: Span) {
        let range = span.byte_range();
        self.functions.push(Function {
            name: name.to_string(),
            line: span.start().line,
            start: range.start,
            end: range.end,
        });
    }
}

impl<'ast> Visit<'ast> for FunctionCollector {
    // {impl}`requirements.references.rust_function_coverage`
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        self.add(&node.sig.ident, node.span());
        visit::visit_item_fn(self, node);
    }

    // {impl}`requirements.references.rust_function_coverage`
    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        self.add(&node.sig.ident, node.span());
        visit::visit_impl_item_fn(self, node);
    }

    // {impl}`requirements.references.rust_function_coverage`
    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        self.add(&node.sig.ident, node.span());
        visit::visit_trait_item_fn(self, node);
    }

    // {impl}`requirements.references.rust_function_coverage`
    fn visit_foreign_item_fn(&mut self, node: &'ast syn::ForeignItemFn) {
        self.add(&node.sig.ident, node.span());
        visit::visit_foreign_item_fn(self, node);
    }
}

// {impl}`requirements.references.rust_function_coverage`
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

// {impl}`requirements.references.rust_function_coverage`
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

// {impl}`requirements.references.rust_function_coverage`
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

// {impl}`requirements.references.rust_function_coverage`
fn comments(source: &str) -> Vec<Range<usize>> {
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

// {impl}`requirements.references.rust_function_coverage`
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

// {impl}`requirements.references.rust_function_coverage`
fn missing_function_references(
    source: &str,
    definitions: &BTreeMap<String, Declaration>,
) -> Result<Vec<(usize, String)>, syn::Error> {
    let syntax = syn::parse_file(source)?;
    let mut collector = FunctionCollector {
        functions: Vec::new(),
    };
    collector.visit_file(&syntax);
    let functions = collector.functions;
    let comments = comments(source);
    let mut owners = vec![None; comments.len()];

    for (function_index, function) in functions.iter().enumerate() {
        for comment_index in attached_comment_indices(source, &comments, function.start) {
            owners[comment_index] = Some(function_index);
        }
    }
    for (comment_index, comment) in comments.iter().enumerate() {
        if source[comment.clone()].starts_with("//!") || source[comment.clone()].starts_with("/*!")
        {
            continue;
        }
        if owners[comment_index].is_none() {
            owners[comment_index] = functions
                .iter()
                .enumerate()
                .filter(|(_, function)| {
                    function.start <= comment.start && comment.end <= function.end
                })
                .min_by_key(|(_, function)| function.end - function.start)
                .map(|(index, _)| index);
        }
    }

    let mut cited = vec![false; functions.len()];
    for (comment_index, owner) in owners.iter().enumerate() {
        let Some(function_index) = owner else {
            continue;
        };
        if reference_roles(&source[comments[comment_index].clone()], false)
            .iter()
            .any(|role| definitions.contains_key(role.name))
        {
            cited[*function_index] = true;
        }
    }
    Ok(functions
        .into_iter()
        .zip(cited)
        .filter_map(|(function, cited)| (!cited).then_some((function.line, function.name)))
        .collect())
}

// {impl}`requirements.references.rust_function_coverage`
pub(crate) fn validate_rust_functions(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    errors: &mut Vec<String>,
) {
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
    for file in files {
        let label = display_path(root, &file);
        let source = match fs::read_to_string(&file) {
            Ok(source) => source,
            Err(error) => {
                errors.push(format!("{label}: cannot read Rust source: {error}"));
                continue;
            }
        };
        match missing_function_references(&source, definitions) {
            Ok(missing) => {
                for (line, name) in missing {
                    errors.push(format!(
                        "{label}:{line}: function {name} has no requirement or example citation"
                    ));
                }
            }
            Err(error) => {
                errors.push(format!(
                    "{label}:{}: cannot parse Rust source: {error}",
                    error.span().start().line
                ));
            }
        }
    }
}

// {impl}`requirements.references.rust_function_coverage`
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

    // {test}`requirements.references.rust_function_coverage`
    fn definitions() -> BTreeMap<String, Declaration> {
        BTreeMap::from([
            (
                "requirements.example".to_owned(),
                Declaration {
                    kind: DefinitionKind::Requirement,
                    text: "Example.".to_owned(),
                    source: "md/requirements.md:1".to_owned(),
                },
            ),
            (
                "requirements.case".to_owned(),
                Declaration {
                    kind: DefinitionKind::Example,
                    text: "Given the example, it works. CITE`requirements.example`"
                        .replace("CITE", "{cite}"),
                    source: "md/requirements.md:5".to_owned(),
                },
            ),
        ])
    }

    // {test}`requirements.references.rust_function_coverage`
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

    // @impl@`requirements.example`

    fn distant_comment() {}

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
                .map(|(_, name)| name)
                .collect::<Vec<_>>(),
            [
                "missing_method",
                "containing_string",
                "unknown_reference",
                "distant_comment",
                "after_trailing_comment",
                "after_inner_doc"
            ]
        );
    }

    // {test}`requirements.references.rust_function_coverage`
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
                .map(|(_, name)| name)
                .collect::<Vec<_>>(),
            ["uncited_trait_method", "outer", "test_helper"]
        );
    }

    // {test}`requirements.references.rust_function_coverage`
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
        assert_eq!(missing, [(6, "missing_inner".to_owned())]);
    }

    // {test}`requirements.references.rust_function_coverage`
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
        validate_rust_functions(project.path(), &definitions(), &mut errors);
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
}

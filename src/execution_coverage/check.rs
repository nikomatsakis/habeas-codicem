//! Post-suite policy checks over the same saved selection used by the book.
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use crate::citations::{citation_items, validate_review};
use crate::inputs::OutlineCache;

use super::{book, target_directory, workspace_root};

// {impl}`requirements.test-coverage.check.report`
pub struct CheckReport {
    pub passed: bool,
    pub text: String,
}

// {impl}`requirements.test-coverage.test`
pub fn test(project: &Path) -> io::Result<i32> {
    let status = super::run(
        project,
        &["--workspace".into(), "--ignore-default-filter".into()],
    )?;
    if status != 0 {
        return Ok(status);
    }
    let report = check(project)?;
    print!("{}", report.text);
    Ok(i32::from(!report.passed))
}

// {impl}`requirements.test-coverage.check`
pub fn check(project: &Path) -> io::Result<CheckReport> {
    check_in(project, target_directory(project)?)
}

// {cite}`habeas:util`
struct Function {
    selector: String,
    line: u32,
    column: u32,
}

// {impl}`requirements.test-coverage.check.scope`
// {impl}`requirements.test-coverage.check.threshold`
fn inventory(source: &str, outline: &crate::outline::Outline) -> io::Result<(Vec<Function>, bool)> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .map_err(io::Error::other)?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| io::Error::other("cannot parse Rust source"))?;
    if tree.root_node().has_error() {
        return Err(io::Error::other("cannot parse Rust source"));
    }
    let mut functions = Vec::new();
    let mut generated = false;
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "function_item" if node.child_by_field_name("body").is_some() => {
                let item = outline
                    .nodes
                    .iter()
                    .find(|item| item.kind == "fn" && item.byte_start == node.start_byte())
                    .ok_or_else(|| io::Error::other("function has no source outline identity"))?;
                functions.push(Function {
                    selector: item.selector.clone(),
                    line: (node.start_position().row + 1)
                        .try_into()
                        .map_err(io::Error::other)?,
                    column: (node.start_position().column + 1)
                        .try_into()
                        .map_err(io::Error::other)?,
                });
            }
            // No recorded lines is not proof that macro-generated code has no
            // executable obligations. Be conservative about procedural attributes.
            "macro_invocation" | "macro_definition" | "closure_expression" => generated = true,
            "attribute_item" | "inner_attribute_item" => {
                let text = &source[node.byte_range()];
                let name = text
                    .trim_start_matches(['#', '!', '[', ' '])
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or("");
                if !matches!(
                    name,
                    "cfg"
                        | "cfg_attr"
                        | "doc"
                        | "allow"
                        | "warn"
                        | "deny"
                        | "forbid"
                        | "expect"
                        | "repr"
                        | "path"
                        | "non_exhaustive"
                ) {
                    generated = true;
                }
                // cfg_attr can apply a generating attribute, so its absence
                // from the current build alone is not an exemption.
                if name == "cfg_attr" {
                    generated = true;
                }
            }
            _ => {}
        }
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }
    functions.sort_by_key(|function| (function.line, function.column));
    let no_executable_obligations = functions.is_empty() && !generated;
    Ok((functions, no_executable_obligations))
}

// {impl}`requirements.test-coverage.check.exceptions.reviews`
// {impl}`requirements.test-coverage.check.exceptions.binding`
// {impl}`requirements.test-coverage.check.exceptions.author`
fn exceptions(
    root: &Path,
    outlines: &OutlineCache,
    errors: &mut Vec<String>,
) -> io::Result<BTreeMap<(String, String), bool>> {
    let definitions = crate::evidence::load_definitions(root)?;
    let references = crate::evidence::collect_references(root, errors);
    let items = citation_items(root, &definitions, &references, outlines);
    let mut accepted = BTreeMap::new();
    for (key, item) in items {
        if !item
            .citations
            .iter()
            .any(|citation| citation.name == "habeas:coverage")
        {
            continue;
        }
        let mut findings = Vec::new();
        let path = item.review_path();
        if root.join(&path).is_file() {
            validate_review(root, &path, &item, &definitions, outlines, &mut findings);
        } else {
            findings.push(format!(
                "{path}: missing citation review; run `habeas author {}`",
                crate::citations::subject_label(&item.file, &item.item)
            ));
        }
        accepted.insert(key, findings.is_empty());
        errors.extend(findings);
    }
    Ok(accepted)
}

// {impl}`requirements.test-coverage.check.threshold`
fn threshold(covered: usize, total: usize) -> bool {
    total > 0 && (covered as u128) * 5 >= (total as u128) * 4
}

// {impl}`requirements.test-coverage.check.report`
fn gaps(lines: impl Iterator<Item = u32>) -> String {
    let mut ranges = Vec::new();
    let mut current: Option<(u32, u32)> = None;
    for line in lines {
        match current {
            Some((start, end)) if line == end + 1 => current = Some((start, line)),
            Some((start, end)) => {
                ranges.push(if start == end {
                    start.to_string()
                } else {
                    format!("{start}-{end}")
                });
                current = Some((line, line));
            }
            None => current = Some((line, line)),
        }
    }
    if let Some((start, end)) = current {
        ranges.push(if start == end {
            start.to_string()
        } else {
            format!("{start}-{end}")
        });
    }
    ranges.join(", ")
}

// {impl}`requirements.test-coverage.check`
// {impl}`requirements.test-coverage.check.measurement`
// {impl}`requirements.test-coverage.check.functions`
// {impl}`requirements.test-coverage.check.exceptions`
// {impl}`requirements.test-coverage.check.report`
fn check_in(project: &Path, target: PathBuf) -> io::Result<CheckReport> {
    let root = workspace_root(project)?;
    let saved = book::load_saved(&root, Some(target), Path::new("coverage.md"));
    let mut errors = Vec::new();
    if saved.uncertain_history {
        errors.push("Coverage history is incomplete: cannot establish latest-per-test selection or active build configuration. Repair or remove corrupt build artifacts, then recollect coverage.".into());
        errors.extend(saved.diagnostics.iter().cloned());
    }
    if saved.selected.is_empty() {
        errors.push(
            "No usable coverage has been collected. Run `habeas coverage -- --workspace`.".into(),
        );
        errors.extend(saved.diagnostics.iter().cloned());
    }
    let selected: Vec<_> = saved
        .selected
        .iter()
        .map(|&index| &saved.records[index])
        .collect();
    for record in &selected {
        let label = format!("{} / {}", record.attempt.binary, record.attempt.test);
        if !record.fresh {
            errors.push(format!(
                "{label}: stale coverage; recollect after source or build-input changes."
            ));
        }
        if !book::readable(record) {
            errors.push(format!(
                "{label}: missing or unreadable coverage: {}.",
                record
                    .attempt
                    .diagnostics
                    .iter()
                    .chain(
                        record
                            .attempt
                            .processes
                            .iter()
                            .filter_map(|process| process.diagnostic.as_ref())
                    )
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        if record
            .attempt
            .processes
            .iter()
            .any(|process| process.functions.is_none())
        {
            errors.push(format!("{label}: coverage format lacks function attribution; recollect coverage with this Habeas version."));
        }
        for process in &record.attempt.processes {
            if process.functions.iter().flatten().any(|function| {
                function.line == 0
                    || function.column == 0
                    || !record.run.sources.contains_key(&function.file)
            }) {
                errors.push(format!(
                    "{label}: corrupt function attribution; recollect coverage."
                ));
            }
        }
    }
    let totals = book::union(selected.iter().copied());
    if totals.is_empty() && !selected.is_empty() {
        errors.push("No usable instrumented lines are available; recollect coverage.".into());
    }
    // Measurement failures cannot be waived, even by valid citation evidence.
    if !errors.is_empty() {
        return Ok(CheckReport {
            passed: false,
            text: format!(
                "Coverage check failed:\n{}\n",
                errors
                    .iter()
                    .map(|error| format!("- {error}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        });
    }
    let mut counters = BTreeMap::<(String, u32, u32), u64>::new();
    for record in &selected {
        for process in &record.attempt.processes {
            for function in process.functions.iter().flatten() {
                counters
                    .entry((function.file.clone(), function.line, function.column))
                    .and_modify(|count| *count = (*count).max(function.count))
                    .or_insert(function.count);
            }
        }
    }
    let outlines = OutlineCache::default();
    let accepted = exceptions(&root, &outlines, &mut errors)?;
    let files = crate::evidence::discovered_files(&root, &mut errors);
    let mut text = String::from("File execution coverage (80% target):\n");
    let mut used = Vec::new();
    for path in crate::rust_coverage::rust_files(&root, &files) {
        let file = path
            .strip_prefix(&root)
            .map_err(io::Error::other)?
            .to_string_lossy()
            .into_owned();
        let parsed = outlines.parse(&root, &file);
        let parsed = match parsed.as_ref() {
            Ok(parsed) => parsed,
            Err(error) => {
                errors.push(error.clone());
                continue;
            }
        };
        let (functions, no_obligations) = match inventory(&parsed.source, &parsed.outline) {
            Ok(inventory) => inventory,
            Err(error) => {
                errors.push(format!("{file}: {error}"));
                continue;
            }
        };
        let lines: Vec<_> = totals
            .range((file.clone(), 0)..=(file.clone(), u32::MAX))
            .map(|((_, line), count)| (*line, *count))
            .collect();
        let covered = lines.iter().filter(|(_, count)| *count > 0).count();
        let total = lines.len();
        let passes = threshold(covered, total);
        let exempt = total == 0 && no_obligations;
        let waiver = accepted.get(&(file.clone(), String::new())) == Some(&true);
        let label = if total == 0 {
            if exempt {
                "no executable obligations".to_owned()
            } else {
                "not measured".to_owned()
            }
        } else {
            format!(
                "{covered}/{total} ({:.1}%)",
                100.0 * covered as f64 / total as f64
            )
        };
        text.push_str(&format!(
            "- {file}: {label} — {}\n",
            if passes || exempt {
                "pass"
            } else if waiver {
                "accepted exception"
            } else {
                "FAIL"
            }
        ));
        if !passes && !exempt {
            if waiver {
                used.push(format!("{file}: file threshold ({label})"));
            } else {
                errors.push(format!("{file}: {} without a current accepted `habeas:coverage` citation on the file root.",
                    if total == 0 { "executable code is not measured".to_owned() } else { format!("below 80%; uncovered lines: {}", gaps(lines.iter().filter(|(_, count)| *count == 0).map(|(line, _)| *line))) }));
            }
        }
        for function in functions {
            let key = (file.clone(), function.line, function.column);
            if counters.get(&key).is_some_and(|&count| count > 0) {
                continue;
            }
            let state = if counters.contains_key(&key) {
                "zero execution"
            } else {
                "not measured"
            };
            if accepted.get(&(file.clone(), function.selector.clone())) == Some(&true) {
                used.push(format!("{file}#{}: {state}", function.selector));
            } else {
                errors.push(format!("{file}:{} #{}: {state}; requires a current accepted `habeas:coverage` citation on this function.", function.line, function.selector));
            }
        }
    }
    text.push_str(&format!(
        "\nSelected {} test attempts in the active build configuration.\n",
        selected.len()
    ));
    for record in &selected {
        if record.run.outcome != "success" || record.attempt.outcome != "success" {
            text.push_str(&format!(
                "Recorded outcome: {} / {}: run {}, attempt {}.\n",
                record.attempt.binary,
                record.attempt.test,
                record.run.outcome,
                record.attempt.outcome
            ));
        }
    }
    if !used.is_empty() {
        text.push_str("\nAccepted exceptions (do not increase execution coverage):\n");
        for exception in used {
            text.push_str(&format!("- {exception}\n"));
        }
    }
    if !errors.is_empty() {
        text.push_str("\nUnsatisfied obligations and invalid evidence:\n");
        for error in &errors {
            text.push_str(&format!("- {error}\n"));
        }
    }
    let passed = errors.is_empty();
    text.push_str(if passed {
        "\nCoverage check passed.\n"
    } else {
        "\nCoverage check failed.\n"
    });
    Ok(CheckReport { passed, text })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_coverage::{
        Attempt, FunctionExecution, Process, Run, input_hashes, load, save, workspace_inputs,
    };
    use serde_json::json;
    use std::fs;

    // {cite}`habeas:util`
    fn fixture(source: &str) -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("md")).unwrap();
        fs::write(
            temp.path().join("Cargo.toml"),
            "[package]\nname='check-fixture'\nversion='0.1.0'\n[lib]\npath='code.rs'\n",
        )
        .unwrap();
        fs::write(temp.path().join("book.toml"), "[book]\nsrc='md'\n").unwrap();
        fs::write(temp.path().join("md/SUMMARY.md"), "# Summary\n").unwrap();
        fs::write(temp.path().join("code.rs"), source).unwrap();
        temp
    }

    // {cite}`habeas:util`
    fn process(root: &Path, lines: &[(u32, u64)], count: u64) -> Process {
        let source = fs::read_to_string(root.join("code.rs")).unwrap();
        let parsed = crate::outline::parse_with(&crate::outline::RUST_PROFILE, &source).unwrap();
        Process {
            pid: 1,
            profiles: vec!["profile.profraw".into()],
            diagnostic: None,
            lines: BTreeMap::from([("code.rs".into(), lines.iter().copied().collect())]),
            functions: Some(
                inventory(&source, &parsed)
                    .unwrap()
                    .0
                    .into_iter()
                    .map(|function| FunctionExecution {
                        file: "code.rs".into(),
                        line: function.line,
                        column: function.column,
                        count,
                    })
                    .collect(),
            ),
        }
    }

    // {cite}`habeas:util`
    fn record(
        root: &Path,
        test: &str,
        started: u128,
        config: &str,
        processes: Vec<Process>,
    ) -> PathBuf {
        let dir = root
            .join("target/habeas/coverage")
            .join(uuid::Uuid::new_v4().to_string());
        let attempt_dir = dir.join("attempts").join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&attempt_dir).unwrap();
        fs::write(
            attempt_dir.join("profile.profraw"),
            "retained synthetic profile",
        )
        .unwrap();
        let run = Run {
            schema: 1,
            id: dir.file_name().unwrap().to_string_lossy().into_owned(),
            started,
            command: vec![],
            selection: vec![],
            configuration: config.into(),
            configuration_inputs: BTreeMap::new(),
            sources: ["code.rs", "driver.rs"]
                .into_iter()
                .filter_map(|file| {
                    fs::read_to_string(root.join(file))
                        .ok()
                        .map(|source| (file.into(), source))
                })
                .collect(),
            inputs: input_hashes(&workspace_inputs(root, &root.join("target")).unwrap()).unwrap(),
            environment_inputs: BTreeMap::new(),
            revision: None,
            dirty: None,
            outcome: "success".into(),
            diagnostics: vec![],
        };
        save(&dir.join("run.json"), &run).unwrap();
        save(
            &attempt_dir.join("attempt.json"),
            &Attempt {
                id: attempt_dir
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                binary: "binary".into(),
                test: test.into(),
                attempt: "1".into(),
                started,
                pid: 1,
                command: vec![],
                outcome: "success".into(),
                processes,
                diagnostics: vec![],
            },
        )
        .unwrap();
        attempt_dir
    }

    // {cite}`habeas:util`
    fn report(root: &Path) -> CheckReport {
        check_in(root, root.join("target")).unwrap()
    }

    // Synthetic fixture evidence only: production evidence is authored by a model.
    // {cite}`habeas:util`
    fn reviews(root: &Path, determination: serde_json::Value) {
        let outlines = OutlineCache::default();
        let mut errors = Vec::new();
        let references = crate::evidence::collect_references(root, &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
        for item in citation_items(root, &BTreeMap::new(), &references, &outlines).into_values() {
            let parsed = outlines.parse(root, &item.file);
            let parsed = parsed.as_ref().as_ref().unwrap();
            let node = parsed.resolve(&item.file, &item.item).unwrap();
            let path = root.join(item.review_path());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, serde_json::to_vec(&json!({
                "file": item.file, "item": item.item, "citations": item.citations_value(),
                "determination": determination, "rationale": "Synthetic accepted explanation for test fixture.",
                "inputs": [{"file": item.file, "item": item.item, "hash": parsed.outline.nodes[node].structural_hash}]
            })).unwrap()).unwrap();
        }
    }

    // {test}`requirements.test-coverage.check.threshold`
    #[test]
    fn exact_per_file_threshold_and_partial_functions() {
        assert!(!threshold(79_999, 100_000)); // displays 80.0%, still fails
        assert!(threshold(80_000, 100_000));
        assert!(!threshold(0, 0));
        let temp = fixture("fn a() {\n let x=1;\n let y=2;\n let z=3;\n}\n");
        let root = temp.path();
        record(
            root,
            "a",
            1,
            "same",
            vec![process(root, &[(1, 1), (2, 1), (3, 1), (4, 0), (5, 1)], 1)],
        );
        let result = report(root);
        assert!(result.passed, "{}", result.text);
        assert!(result.text.contains("4/5 (80.0%)"));
        // Another file cannot be rescued by the first one's higher percentage.
        fs::write(root.join("driver.rs"), "fn driver() {}").unwrap();
        let mut p = process(root, &[(1, 1), (2, 1), (3, 1), (4, 1), (5, 1)], 1);
        p.lines.insert("driver.rs".into(), BTreeMap::from([(1, 0)]));
        p.functions.as_mut().unwrap().push(FunctionExecution {
            file: "driver.rs".into(),
            line: 1,
            column: 1,
            count: 0,
        });
        record(root, "a", 2, "same", vec![p]);
        let result = report(root);
        assert!(!result.passed);
        assert!(result.text.contains("driver.rs: 0/1 (0.0%)"));
        assert!(result.text.contains("uncovered lines: 1"));
    }

    // {test}`requirements.test-coverage.check.threshold`
    // {test}`requirements.test-coverage.check.functions`
    #[test]
    fn children_and_filtered_runs_contribute_to_the_same_union() {
        let temp = fixture("fn a() {\n let x=1;\n let y=2;\n let z=3;\n}\n");
        let root = temp.path();
        let first = process(root, &[(1, 1), (2, 1), (3, 0), (4, 0), (5, 1)], 0);
        let mut child = process(root, &[(1, 0), (2, 0), (3, 1), (4, 1), (5, 0)], 1);
        child.pid = 2;
        record(root, "filtered-first", 1, "same", vec![first, child]);
        assert!(report(root).passed);
        record(
            root,
            "filtered-second",
            2,
            "same",
            vec![process(root, &[(1, 0), (2, 0), (3, 0), (4, 0), (5, 0)], 0)],
        );
        assert!(
            report(root).passed,
            "a zero second test does not erase the first test's execution"
        );
    }

    // {test}`requirements.test-coverage.check.functions`
    // {test}`requirements.test-coverage.check.scope`
    #[test]
    fn overlapping_nested_functions_and_closures_do_not_cover_the_parent() {
        let source = "fn outer() { fn inner() {} let f = || {}; }\n";
        let temp = fixture(source);
        let root = temp.path();
        let mut p = process(root, &[(1, 1)], 0);
        p.functions.as_mut().unwrap()[1].count = 1;
        p.functions.as_mut().unwrap().push(FunctionExecution {
            file: "code.rs".into(),
            line: 1,
            column: (source.find("||").unwrap() + 3) as u32,
            count: 1,
        });
        record(root, "nested", 1, "same", vec![p]);
        let result = report(root);
        assert!(!result.passed);
        assert!(result.text.contains("#fn[outer]: zero execution"));
        assert!(!result.text.contains("#fn[outer]/fn[inner]:"));
        let mut p = process(root, &[(1, 1)], 1);
        p.functions.as_mut().unwrap()[1].count = 0;
        record(root, "nested", 2, "same", vec![p]);
        let result = report(root);
        assert!(result.text.contains("#fn[outer]/fn[inner]: zero execution"));
        assert!(!result.text.contains("#fn[outer]:"));
    }

    // {test}`requirements.test-coverage.check.scope`
    // {test}`requirements.test-coverage.check.threshold`
    #[test]
    fn default_trait_bodies_and_cfg_exclusions_are_obligations_but_declarations_are_not() {
        let temp =
            fixture("trait T { fn declaration(); fn body() {} }\n#[cfg(any())] fn excluded() {}\n");
        let root = temp.path();
        let mut p = process(root, &[(1, 1)], 1);
        p.functions.as_mut().unwrap().retain(|f| f.line == 1);
        record(root, "traits", 1, "same", vec![p]);
        let result = report(root);
        assert!(!result.passed);
        assert!(result.text.contains("#fn[excluded]: not measured"));
        assert!(!result.text.contains("fn[declaration]"));
        let parsed = crate::outline::parse_with(
            &crate::outline::RUST_PROFILE,
            "struct S;\ntrait T { fn declaration(); }\n",
        )
        .unwrap();
        assert!(
            inventory("struct S;\ntrait T { fn declaration(); }\n", &parsed)
                .unwrap()
                .1
        );
        let parsed =
            crate::outline::parse_with(&crate::outline::RUST_PROFILE, "make_functions!();")
                .unwrap();
        assert!(!inventory("make_functions!();", &parsed).unwrap().1);
        let parsed =
            crate::outline::parse_with(&crate::outline::RUST_PROFILE, "#[derive(Clone)] struct S;")
                .unwrap();
        assert!(!inventory("#[derive(Clone)] struct S;", &parsed).unwrap().1);
    }

    // {test}`requirements.test-coverage.check.measurement`
    #[test]
    fn stale_missing_legacy_and_uncertain_history_cannot_use_older_success() {
        let temp = fixture("fn a() {}");
        let root = temp.path();
        assert!(!report(root).passed);
        record(root, "a", 1, "same", vec![process(root, &[(1, 1)], 1)]);
        assert!(report(root).passed);
        let mut old = process(root, &[(1, 1)], 1);
        old.functions = None;
        record(root, "old", 0, "other-config", vec![old]);
        assert!(
            report(root).passed,
            "excluded legacy data cannot invalidate a supported active configuration"
        );
        let current = record(root, "a", 2, "same", vec![process(root, &[(1, 1)], 1)]);
        fs::remove_file(current.join("profile.profraw")).unwrap();
        assert!(report(root).text.contains("missing or unreadable"));
        let mut legacy = process(root, &[(1, 1)], 1);
        legacy.functions = None;
        record(root, "a", 3, "same", vec![legacy]);
        assert!(report(root).text.contains("lacks function attribution"));
        record(root, "a", 4, "same", vec![process(root, &[(1, 1)], 1)]);
        fs::write(root.join("code.rs"), "fn a() { let changed=1; }").unwrap();
        assert!(report(root).text.contains("stale coverage"));
        record(root, "a", 5, "same", vec![process(root, &[(1, 1)], 1)]);
        assert!(report(root).passed);
        fs::create_dir(root.join("target/habeas/coverage/corrupt")).unwrap();
        fs::write(root.join("target/habeas/coverage/corrupt/run.json"), "{").unwrap();
        assert!(
            report(root)
                .text
                .contains("cannot establish latest-per-test")
        );
    }

    // {test}`requirements.test-coverage.check.exceptions`
    // {test}`requirements.test-coverage.check.exceptions.reviews`
    // {test}`requirements.test-coverage.check.exceptions.binding`
    // {test}`requirements.test-coverage.check.report`
    #[test]
    fn accepted_file_and_function_explanations_remain_separate_from_execution() {
        let temp = fixture(
            "//! :::{cite} habeas:coverage\n//! Generated platform glue lacks a harness.\n//! :::\n\nfn a() {}\n",
        );
        let root = temp.path();
        reviews(root, json!(true));
        record(root, "a", 1, "same", vec![process(root, &[(5, 0)], 0)]);
        let result = report(root);
        assert!(!result.passed, "file exception cannot waive its function");
        assert!(
            result
                .text
                .contains("code.rs: 0/1 (0.0%) — accepted exception")
        );
        assert!(result.text.contains("#fn[a]: zero execution"));
        fs::write(root.join("code.rs"),"//! :::{cite} habeas:coverage\n//! File rationale.\n//! :::\n\n// :::{cite} habeas:coverage\n// Function rationale, describing only a Linux condition.\n// :::\nfn a() {}\n").unwrap();
        reviews(root, json!(true));
        record(root, "a", 2, "linux", vec![process(root, &[(8, 0)], 0)]);
        let result = report(root);
        assert!(result.passed, "{}", result.text);
        assert!(result.text.contains("0/1 (0.0%)"));
        assert!(result.text.contains("Accepted exceptions (do not increase"));
        let mut absent = process(root, &[(8, 0)], 0);
        absent.functions = Some(vec![]);
        record(root, "a", 3, "windows", vec![absent]);
        assert!(
            report(root).passed,
            "same accepted citation covers unmeasured functions without conditional matching"
        );
        record(root, "a", 4, "covered", vec![process(root, &[(8, 1)], 1)]);
        assert!(
            report(root).passed,
            "citation may remain when no exception is needed"
        );
        reviews(root, json!(false));
        assert!(!report(root).passed);
        assert!(report(root).text.contains("determination is false"));
        reviews(root, json!({"FIXME":["acknowledgment"]}));
        assert!(report(root).text.contains("cannot accept a {fixme}"));
    }

    // {test}`requirements.test-coverage.check.exceptions.reviews`
    // {test}`requirements.test-coverage.check.exceptions.author`
    #[test]
    fn standard_inline_doc_and_fenced_commentary_reviews_track_their_explanations() {
        let source = "/// Defensive invariant abort, inaccessible through the API.\n// {cite}`habeas:coverage`\nfn a() {}\n";
        let temp = fixture(source);
        let root = temp.path();
        reviews(root, json!(true));
        let mut errors = Vec::new();
        assert!(
            exceptions(root, &OutlineCache::default(), &mut errors)
                .unwrap()
                .values()
                .all(|accepted| *accepted)
        );
        assert!(errors.is_empty());
        fs::write(
            root.join("code.rs"),
            source.replace("inaccessible", "reachable"),
        )
        .unwrap();
        let mut errors = Vec::new();
        assert!(
            exceptions(root, &OutlineCache::default(), &mut errors)
                .unwrap()
                .values()
                .all(|accepted| !*accepted)
        );
        assert!(!errors.is_empty());
        let source = "// :::{cite} habeas:coverage\n// A protected rationale.\n// :::\nfn a() {}\n";
        fs::write(root.join("code.rs"), source).unwrap();
        reviews(root, json!(true));
        fs::write(root.join("code.rs"), source.replace("protected", "changed")).unwrap();
        let mut errors = Vec::new();
        assert!(
            exceptions(root, &OutlineCache::default(), &mut errors)
                .unwrap()
                .values()
                .all(|accepted| !*accepted)
        );
        assert!(
            errors
                .iter()
                .any(|error| error.contains("citations changed"))
        );
        // Exception evidence uses project-relative identities in another checkout.
        fs::write(root.join("code.rs"), source).unwrap();
        reviews(root, json!(true));
        let other = fixture(source);
        let path = "evidence/citations/code.rs/fn[a].json";
        fs::create_dir_all(other.path().join("evidence/citations/code.rs")).unwrap();
        fs::copy(root.join(path), other.path().join(path)).unwrap();
        let mut errors = Vec::new();
        assert!(
            exceptions(other.path(), &OutlineCache::default(), &mut errors)
                .unwrap()
                .values()
                .all(|accepted| *accepted)
        );
        assert!(errors.is_empty());
    }

    // {test}`requirements.test-coverage.check.measurement`
    // {test}`requirements.test-coverage.check.scope`
    // {test}`requirements.test-coverage.check.threshold`
    #[test]
    fn unmeasured_executable_files_and_parse_failures_do_not_disappear() {
        let temp = fixture("struct S; trait T { fn declaration(); }");
        let root = temp.path();
        fs::write(root.join("driver.rs"), "fn driver() {}").unwrap();
        let mut p = process(root, &[], 1);
        p.lines.insert("driver.rs".into(), BTreeMap::from([(1, 1)]));
        p.functions.as_mut().unwrap().push(FunctionExecution {
            file: "driver.rs".into(),
            line: 1,
            column: 1,
            count: 1,
        });
        record(root, "driver", 1, "same", vec![p]);
        assert!(
            report(root).passed,
            "declaration-only file is proved to have no executable obligations"
        );
        fs::write(root.join("code.rs"), "fn missing() {}").unwrap();
        let mut p = process(root, &[], 1);
        p.functions = Some(vec![]);
        p.lines.insert("driver.rs".into(), BTreeMap::from([(1, 1)]));
        p.functions.as_mut().unwrap().push(FunctionExecution {
            file: "driver.rs".into(),
            line: 1,
            column: 1,
            count: 1,
        });
        record(root, "driver", 2, "same", vec![p.clone()]);
        let result = report(root);
        assert!(result.text.contains("code.rs: not measured — FAIL"));
        assert!(result.text.contains("#fn[missing]: not measured"));
        fs::write(root.join("code.rs"), "fn malformed(").unwrap();
        record(root, "driver", 3, "same", vec![p]);
        let result = report(root);
        assert!(!result.passed);
        assert!(
            result
                .text
                .to_lowercase()
                .contains("cannot parse rust source"),
            "{}",
            result.text
        );
    }

    // {test}`requirements.test-coverage.check.measurement`
    // {test}`requirements.test-coverage.check.report`
    #[test]
    fn readable_failed_execution_can_contribute_but_outcomes_remain_visible() {
        let temp = fixture("fn a() {}");
        let root = temp.path();
        let path = record(root, "a", 1, "same", vec![process(root, &[(1, 1)], 1)]);
        let mut attempt: Attempt = load(&path.join("attempt.json")).unwrap();
        attempt.outcome = "failed".into();
        save(&path.join("attempt.json"), &attempt).unwrap();
        let result = report(root);
        assert!(result.passed, "{}", result.text);
        assert!(result.text.contains("attempt failed"));
        attempt.processes[0].diagnostic = Some("broken coverage map".into());
        save(&path.join("attempt.json"), &attempt).unwrap();
        assert!(!report(root).passed);
    }
}

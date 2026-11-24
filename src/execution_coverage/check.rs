//! Post-suite policy checks over saved execution measurements.
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

// {cite}`habeas:util`
#[derive(Clone)]
struct Exception {
    accepted: bool,
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
    keyword: (u32, u32),
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
                let mut cursor = node.walk();
                let keyword = node
                    .children(&mut cursor)
                    .find(|child| child.kind() == "fn")
                    .ok_or_else(|| io::Error::other("function has no fn keyword"))?
                    .start_position();
                functions.push(Function {
                    selector: item.selector.clone(),
                    line: (node.start_position().row + 1)
                        .try_into()
                        .map_err(io::Error::other)?,
                    column: (node.start_position().column + 1)
                        .try_into()
                        .map_err(io::Error::other)?,
                    keyword: (
                        (keyword.row + 1).try_into().map_err(io::Error::other)?,
                        (keyword.column + 1).try_into().map_err(io::Error::other)?,
                    ),
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
) -> io::Result<BTreeMap<(String, String), Exception>> {
    let references = crate::evidence::collect_references(root, errors);
    let definitions = match crate::evidence::load_definitions(root) {
        Ok(definitions) => definitions,
        Err(error) => {
            errors.push(error.to_string());
            return Ok(BTreeMap::new());
        }
    };
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
        accepted.insert(
            key,
            Exception {
                accepted: findings.is_empty(),
            },
        );
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
    let saved = book::load_saved(&root, Some(target));
    check_saved(&root, &saved)
}

// {impl}`requirements.test-coverage.check.measurement`
// {impl}`requirements.test-coverage.check.threshold`
pub(super) fn check_saved(root: &Path, saved: &book::SavedCoverage) -> io::Result<CheckReport> {
    let mut errors = Vec::new();
    if saved.invalid_store {
        errors.push("Current coverage store is incomplete: cannot establish the current test set or active build configuration. Repair or remove corrupt build artifacts, then recollect coverage.".into());
    }
    if saved.selected.is_empty() {
        errors.push(
            "No usable coverage has been collected. Run `habeas coverage -- --workspace`.".into(),
        );
    }
    if saved.invalid_store || saved.selected.is_empty() {
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
    // Source discovery, parsing and citation checks remain independent of
    // measurement. No exception can clear a measurement failure.
    let measured = errors.is_empty();
    let mut counters = std::collections::HashMap::<(&str, u32, u32), u64>::new();
    for record in &selected {
        for process in &record.attempt.processes {
            for function in process.functions.iter().flatten() {
                counters
                    .entry((&function.file, function.line, function.column))
                    .and_modify(|count| *count = (*count).max(function.count))
                    .or_insert(function.count);
            }
        }
    }
    let outlines = OutlineCache::default();
    let accepted = exceptions(root, &outlines, &mut errors)?;
    let files = crate::evidence::discovered_files(root, &mut errors);
    let mut text = String::from("File execution coverage (80% target):\n");
    let mut used = Vec::new();
    for path in crate::rust_coverage::rust_files(root, &files) {
        let file = path
            .strip_prefix(root)
            .map_err(io::Error::other)?
            .to_string_lossy()
            .into_owned();
        let parsed = outlines.parse(root, &file);
        let inventory = match parsed.as_ref() {
            Ok(parsed) => inventory(&parsed.source, &parsed.outline)
                .map_err(|error| format!("{file}: {error}")),
            Err(error) => Err(error.clone()),
        };
        let inventory = match inventory {
            Ok(inventory) => Some(inventory),
            Err(error) => {
                errors.push(error);
                None
            }
        };
        if !measured {
            continue;
        }
        let lines: Vec<_> = totals
            .range((file.clone(), 0)..=(file.clone(), u32::MAX))
            .map(|((_, line), count)| (*line, *count))
            .collect();
        let covered = lines.iter().filter(|(_, count)| *count > 0).count();
        let total = lines.len();
        // Measured file thresholds do not depend on parsing. Inventory is
        // needed only for function attribution and zero-line exemptions.
        if inventory.is_none() && total == 0 {
            continue;
        }
        let (functions, no_obligations) = inventory.unwrap_or_default();
        let passes = threshold(covered, total);
        let exempt = total == 0 && no_obligations;
        let exception = accepted.get(&(file.clone(), String::new())).cloned();
        let waiver = exception
            .as_ref()
            .is_some_and(|exception| exception.accepted);
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
            // Attribute expansion can remove modifiers: tokio::test's wrapper
            // starts at the original fn keyword rather than the async token.
            // Both points belong to this declaration, never a nested body.
            let keys = [
                (file.as_str(), function.line, function.column),
                (file.as_str(), function.keyword.0, function.keyword.1),
            ];
            if keys
                .iter()
                .any(|key| counters.get(key).is_some_and(|&count| count > 0))
            {
                continue;
            }
            let state = if keys.iter().any(|key| counters.contains_key(key)) {
                "zero execution"
            } else {
                "not measured"
            };
            let exception = accepted
                .get(&(file.clone(), function.selector.clone()))
                .cloned();
            let waiver = exception
                .as_ref()
                .is_some_and(|exception| exception.accepted);
            if waiver {
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
    let mut seen = std::collections::BTreeSet::new();
    errors.retain(|error| seen.insert(error.clone()));
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
    use crate::execution_coverage::store::fixtures::{load, save};
    use crate::execution_coverage::{
        Attempt, FunctionExecution, Process, Run, input_hashes, workspace_inputs,
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
        let parsed = crate::outline::parse(Path::new("code.rs"), &source).unwrap();
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

    // Execute the handler emitted by the renderer, using its actual controls
    // and row IDs. Node is already part of this repository's CI tooling.
    // {cite}`habeas:util`
    fn exercise_file_click(html: &str) {
        let button = html
            .split("class=\"gap-file-button\"")
            .nth(1)
            .unwrap()
            .split('>')
            .next()
            .unwrap();
        let attribute = |name: &str| {
            button
                .split(&format!("{name}=\""))
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap()
                .to_owned()
        };
        let ids = html
            .split("class=\"gap-detail-row\" id=\"")
            .skip(1)
            .map(|part| part.split('"').next().unwrap().to_owned())
            .collect::<Vec<_>>();
        let data = json!({"handler": attribute("onclick"), "controls": attribute("aria-controls"), "ids": ids});
        let output = std::process::Command::new("node").arg("-e").arg(r#"
            const assert = require('node:assert/strict');
            const vm = require('node:vm');
            const data = JSON.parse(process.argv[1]);
            const rows = new Map(data.ids.map(id => [id, {hidden: true}]));
            const attributes = {'aria-controls': data.controls, 'aria-expanded': 'false'};
            const chevron = {textContent: '▸'};
            const button = {getAttribute: name => attributes[name],
                setAttribute: (name, value) => {attributes[name] = value},
                querySelector: name => {assert.equal(name, '.gap-chevron'); return chevron}};
            const document = {getElementById: id => rows.get(id)};
            const click = () => vm.runInNewContext('(function(){' + data.handler + '}).call(button)', {button, document});
            assert.ok(rows.has(data.controls), 'controls must identify a generated row');
            click();
            assert.equal(rows.get(data.controls).hidden, false);
            assert.equal(attributes['aria-expanded'], 'true');
            assert.equal(chevron.textContent, '▾');
            click();
            assert.equal(rows.get(data.controls).hidden, true);
            assert.equal(attributes['aria-expanded'], 'false');
            assert.equal(chevron.textContent, '▸');
        "#).arg(data.to_string()).output().expect("Node is required for coverage UI tests");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // {test}`requirements.test-coverage.index.gaps`
    // {test}`requirements.test-coverage.index.gaps.details`
    // {test}`requirements.test-coverage.index.gaps.details.initial`
    // {test}`requirements.test-coverage.index.gaps.explanations`
    // {test}`requirements.test-coverage.index.gaps.accepted`
    // {test}`requirements.test-coverage.index.file_map.combined`
    // {test}`requirements.test-coverage.index.file_map.combined.details`
    #[test]
    fn book_browses_all_proposals_while_cli_checks_reviews_and_measurements() {
        use mdbook_preprocessor::book::{Book, Chapter};
        let source = "fn a() {}\n\n/// Invariant fallback, unreachable through the public API.\n// {cite}`habeas:coverage`\nfn b() {}\n";
        let temp = fixture(source);
        let root = temp.path();
        let mut p = process(root, &[(1, 1), (2, 1), (3, 1), (4, 1), (5, 0)], 0);
        p.functions.as_mut().unwrap()[0].count = 0;
        let path = record(root, "both", 1, "same", vec![p]);
        let render = || {
            let mut book = Book::new();
            book.push_item(Chapter::new(
                "Requirements index",
                "# Requirements index\n".into(),
                "requirements-index.md",
                vec![],
            ));
            book::populate_in(
                &mut book,
                root,
                Some(root.join("target")),
                Path::new("requirements-index.md"),
            )
            .unwrap();
            book.chapters()
                .find(|chapter| chapter.name == "Documented coverage gaps")
                .unwrap()
                .content
                .clone()
        };
        let missing = render();
        exercise_file_click(&missing);
        assert!(!report(root).passed);
        assert!(missing.contains("Recorded review: <strong>Missing</strong>"));
        assert!(missing.contains("fn[b]"));
        assert!(!missing.contains("fn[a]"));
        assert!(missing.contains("Invariant fallback, unreachable through the public API."));
        assert!(missing.contains("code.rs:3"));
        assert!(missing.contains("1 documented source items"));
        assert!(!missing.contains("CI would"));
        for (determination, expected) in [
            (json!(true), "Recorded true"),
            (json!(false), "Recorded false"),
            (json!({"FIXME": ["platform gap"]}), "Recorded FIXME"),
        ] {
            reviews(root, determination);
            let rendered = render();
            assert!(rendered.contains(expected), "{rendered}");
            assert!(rendered.contains("data-file=\"code.rs\""));
            assert!(rendered.contains("fn[b]"));
            assert!(rendered.contains("1 documented source items"));
            assert!(!rendered.contains("CI would"));
        }
        reviews(root, json!(true));
        let mut attempt: Attempt = load(&path.join("attempt.json")).unwrap();
        for entry in attempt.processes[0].functions.as_mut().unwrap() {
            entry.count = 1;
        }
        save(&path.join("attempt.json"), &attempt).unwrap();
        assert!(report(root).passed);
        let covered = render();
        assert!(covered.contains("fn[b]"));
        assert!(covered.contains("Recorded true"));
        fs::remove_file(path.join("profile.profraw")).unwrap();
        assert!(!report(root).passed);
        assert_eq!(render(), covered);
        fs::write(
            root.join("code.rs"),
            "fn a() {}\n// {cite}`habeas:coverage`\nfn b() {}\n",
        )
        .unwrap();
        let changed = render();
        assert!(changed.contains("No tracked justification is present."));
        assert!(changed.contains("Recorded true"));
        assert!(!changed.contains("citations changed"));
    }

    // {test}`requirements.test-coverage.check.threshold`
    // {test}`requirements.test-coverage.check.report`
    #[test]
    fn measured_file_threshold_remains_checked_when_source_parsing_fails() {
        let temp = fixture("fn adapter() {}\n");
        let root = temp.path();
        fs::create_dir_all(root.join("src/nested")).unwrap();
        fs::write(root.join("src/nested/driver.rs"), "fn broken(").unwrap();
        let mut p = process(root, &[(1, 1)], 1);
        p.lines
            .insert("src/nested/driver.rs".into(), BTreeMap::from([(1, 0)]));
        let attempt = record(root, "adapter", 1, "same", vec![p]);
        let run_path = attempt.parent().unwrap().parent().unwrap().join("run.json");
        let mut run: Run = load(&run_path).unwrap();
        run.sources
            .insert("src/nested/driver.rs".into(), "fn broken(".into());
        save(&run_path, &run).unwrap();
        let result = report(root);
        assert!(!result.passed);
        assert!(
            result.text.contains("src/nested/driver.rs: below 80%"),
            "{}",
            result.text
        );
        assert!(
            result.text.contains("syntax error")
                || result.text.contains("cannot parse Rust source"),
            "{}",
            result.text
        );
    }

    // {test}`requirements.test-coverage.check.scope`
    #[test]
    fn post_suite_scope_excludes_root_vendor_but_includes_source_vendor() {
        let temp = fixture("fn main() {}\n");
        let root = temp.path();
        for file in ["vendor/foreign.rs", "src/vendor/first_party.rs"] {
            fs::create_dir_all(root.join(file).parent().unwrap()).unwrap();
            fs::write(root.join(file), "fn unmeasured() {}\n").unwrap();
        }
        record(root, "main", 1, "same", vec![process(root, &[(1, 1)], 1)]);
        let result = report(root);
        assert!(!result.passed);
        assert!(result.text.contains("src/vendor/first_party.rs"));
        assert!(!result.text.contains("- vendor/foreign.rs"));
    }

    // {test}`requirements.test-coverage.check.exceptions.reviews`
    #[test]
    fn proposals_on_modules_structs_and_impls_waive_neither_files_nor_functions() {
        for source in [
            "// :::{cite} habeas:coverage\n// Container commentary.\n// :::\nmod m { fn work() {} }\n",
            "// :::{cite} habeas:coverage\n// Container commentary.\n// :::\nstruct S; impl S { fn work() {} }\n",
            "struct S;\n// :::{cite} habeas:coverage\n// Container commentary.\n// :::\nimpl S { fn work() {} }\n",
        ] {
            let temp = fixture(source);
            let root = temp.path();
            reviews(root, json!(true));
            record(root, "work", 1, "same", vec![process(root, &[(4, 0)], 0)]);
            let result = report(root);
            assert!(!result.passed);
            assert!(result.text.contains(
                "without a current accepted `habeas:coverage` citation on the file root"
            ));
            assert!(result.text.contains(
                "requires a current accepted `habeas:coverage` citation on this function"
            ));
            assert!(!result.text.contains("Accepted exceptions ("));
        }
    }

    // {test}`requirements.test-coverage.check.functions`
    #[test]
    fn expanded_async_entries_match_the_fn_keyword_without_borrowing_closure_execution() {
        let source = "#[tokio::test]\nasync fn work() { let callback = || {}; callback(); }\n";
        let temp = fixture(source);
        let root = temp.path();
        let mut p = process(root, &[(2, 1)], 1);
        p.functions.as_mut().unwrap()[0].column = 7; // fn after async
        record(root, "work", 1, "same", vec![p.clone()]);
        let result = report(root);
        assert!(result.passed, "{}", result.text);
        // Executing only the closure must not count as entering this function.
        p.functions.as_mut().unwrap()[0].column =
            source.lines().nth(1).unwrap().find("||").unwrap() as u32 + 1;
        record(root, "work", 2, "same", vec![p]);
        let result = report(root);
        assert!(!result.passed);
        assert!(result.text.contains("#fn[work]: not measured"));
    }

    // {test}`requirements.test-coverage.check.threshold`
    // {test}`requirements.test-coverage.check.report`
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
        assert!(result.text.contains("4/5 (80.0%) — pass"));
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
        assert!(result.text.contains("driver.rs: 0/1 (0.0%) — FAIL"));
        assert!(result.text.contains("uncovered lines: 1"));
        fs::write(root.join("driver.rs"), "fn driver() {\n let a=1;\n let b=2;\n let c=3;\n let d=4;\n let e=5;\n let f=6;\n let g=7;\n let h=8;\n}\n").unwrap();
        let mut p = process(root, &[(1, 1), (2, 1), (3, 1), (4, 1), (5, 1)], 1);
        p.lines.insert(
            "driver.rs".into(),
            (1..=10).map(|line| (line, u64::from(line <= 7))).collect(),
        );
        p.functions.as_mut().unwrap().push(FunctionExecution {
            file: "driver.rs".into(),
            line: 1,
            column: 1,
            count: 1,
        });
        record(root, "a", 3, "same", vec![p]);
        let result = report(root);
        assert!(
            !result.passed,
            "the 85% average of 100% and 70% cannot rescue driver.rs"
        );
        assert!(result.text.contains("driver.rs: 7/10 (70.0%) — FAIL"));
        assert!(
            !result.text.contains("#fn[driver]:"),
            "the function did execute"
        );
    }

    // {test}`requirements.test-coverage.check.threshold`
    #[test]
    fn a_function_below_eighty_percent_needs_no_exception_when_its_file_passes() {
        let temp = fixture(
            "fn partly() {\n let x=1;\n}\nfn fully() {\n let a=1;\n let b=2;\n let c=3;\n let d=4;\n let e=5;\n let f=6;\n let g=7;\n}\n",
        );
        let root = temp.path();
        let lines = (1..=12)
            .map(|line| (line, u64::from(line != 2 && line != 3)))
            .collect::<Vec<_>>();
        record(root, "a", 1, "same", vec![process(root, &lines, 1)]);
        let result = report(root);
        assert!(
            result.passed,
            "partly is at 1/3 while its file passes: {}",
            result.text
        );
        assert!(result.text.contains("code.rs: 10/12 (83.3%) — pass"));
        assert!(!result.text.contains("Accepted exceptions"));
    }

    // {test}`requirements.test-coverage.index.summary`
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
        assert!(report(root).text.contains("code.rs: 5/5 (100.0%) — pass"));
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
        assert!(report(root).text.contains("code.rs: 5/5 (100.0%) — pass"));
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
    // {test}`requirements.test-coverage.check.functions`
    #[test]
    fn default_trait_bodies_and_cfg_exclusions_are_obligations_but_declarations_are_not() {
        let temp =
            fixture("trait T { fn declaration(); fn body() {} }\n#[cfg(any())] fn excluded() {}\n");
        let root = temp.path();
        let mut p = process(root, &[(1, 1)], 0);
        p.functions.as_mut().unwrap().retain(|f| f.line == 1);
        record(root, "traits", 1, "same", vec![p]);
        let result = report(root);
        assert!(!result.passed);
        assert!(result.text.contains("#fn[excluded]: not measured"));
        assert!(
            result.text.contains("#trait[T]/fn[body]: zero execution"),
            "{}",
            result.text
        );
        assert!(!result.text.contains("fn[declaration]"));
        let parsed = crate::outline::parse(
            Path::new("fixture.rs"),
            "struct S;\ntrait T { fn declaration(); }\n",
        )
        .unwrap();
        assert!(
            inventory("struct S;\ntrait T { fn declaration(); }\n", &parsed)
                .unwrap()
                .1
        );
        let parsed = crate::outline::parse(Path::new("fixture.rs"), "make_functions!();").unwrap();
        assert!(!inventory("make_functions!();", &parsed).unwrap().1);
        let parsed =
            crate::outline::parse(Path::new("fixture.rs"), "#[derive(Clone)] struct S;").unwrap();
        assert!(!inventory("#[derive(Clone)] struct S;", &parsed).unwrap().1);
    }

    // {test}`requirements.test-coverage.check.measurement`
    // {test}`requirements.test-coverage.check.threshold`
    #[test]
    fn stale_missing_legacy_and_invalid_store_cannot_use_older_success() {
        let temp = fixture("fn a() {}");
        let root = temp.path();
        assert!(!report(root).passed);
        record(root, "a", 1, "same", vec![process(root, &[(1, 1)], 1)]);
        assert!(report(root).passed);
        let mut old = process(root, &[(1, 1)], 1);
        old.functions = None;
        record(root, "old", 0, "other-config", vec![old]);
        assert!(
            !report(root).passed,
            "a newly activated build with missing attribution is unavailable"
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
        let current = record(root, "a", 5, "same", vec![process(root, &[(1, 1)], 1)]);
        assert!(report(root).passed);
        crate::execution_coverage::store::fixtures::corrupt(
            &current.parent().unwrap().parent().unwrap().join("run.json"),
        );
        assert!(
            report(root)
                .text
                .contains("cannot establish the current test set")
        );
        let database =
            rusqlite::Connection::open(root.join("target/habeas/coverage/current.sqlite")).unwrap();
        database.execute_batch("PRAGMA user_version=999").unwrap();
        let result = report(root);
        assert!(!result.passed);
        assert!(result.text.contains("recollect"), "{}", result.text);
    }

    // {test}`requirements.test-coverage.index.summary`
    // {test}`requirements.test-coverage.index.selection`
    // {test}`requirements.test-coverage.index.unavailable`
    // {test}`requirements.test-coverage.check.measurement`
    #[test]
    fn running_and_unreadable_entries_suppress_totals_even_alongside_fresh_measurements() {
        let temp = fixture("fn a() {}");
        let root = temp.path();
        record(root, "fresh", 1, "same", vec![process(root, &[(1, 1)], 1)]);
        let path = record(root, "other", 2, "same", vec![process(root, &[(1, 1)], 1)]);
        fs::remove_file(path.join("profile.profraw")).unwrap();
        let result = report(root);
        assert!(!result.passed);
        assert!(
            result
                .text
                .contains("binary / other: missing or unreadable")
        );
        assert!(
            !result
                .text
                .contains("binary / fresh: missing or unreadable")
        );
        assert!(!result.text.contains("100.0%"));
        assert!(!result.text.contains("code.rs:"));
        let path = record(root, "other", 3, "same", vec![process(root, &[(1, 1)], 1)]);
        let mut attempt: Attempt = load(&path.join("attempt.json")).unwrap();
        for outcome in ["pending", "running"] {
            attempt.outcome = outcome.into();
            save(&path.join("attempt.json"), &attempt).unwrap();
            let result = report(root);
            assert!(!result.passed);
            assert!(
                result
                    .text
                    .contains("binary / other: missing or unreadable")
            );
            assert!(result.text.contains(&format!("attempt {outcome}")));
            assert!(!result.text.contains("100.0%"));
            assert!(!result.text.contains("code.rs:"));
        }
    }

    // {test}`requirements.test-coverage.index.summary`
    // {test}`requirements.test-coverage.index.freshness`
    // {test}`requirements.test-coverage.storage.provenance`
    // {test}`requirements.test-coverage.check.measurement`
    #[test]
    fn focused_recollection_leaves_only_the_unselected_result_stale_with_its_original_sources() {
        let temp = fixture("fn a() {}");
        let root = temp.path();
        record(root, "rerun", 1, "same", vec![process(root, &[(1, 1)], 1)]);
        let old = record(
            root,
            "unselected",
            2,
            "same",
            vec![process(root, &[(1, 1)], 1)],
        );
        fs::write(root.join("code.rs"), "fn a() { let changed=1; }").unwrap();
        record(root, "rerun", 3, "same", vec![process(root, &[(1, 1)], 1)]);
        let result = report(root);
        assert!(!result.passed);
        assert!(result.text.contains("binary / unselected: stale coverage"));
        assert!(!result.text.contains("binary / rerun: stale coverage"));
        assert!(!result.text.contains("100.0%"));
        assert!(!result.text.contains("code.rs:"));
        let saved = book::load_saved(root, Some(root.join("target")));
        let preserved = saved
            .records
            .iter()
            .find(|r| r.attempt.test == "unselected")
            .unwrap();
        assert!(!preserved.fresh);
        assert_eq!(
            preserved.attempt.id,
            old.file_name().unwrap().to_str().unwrap()
        );
        assert_eq!(
            preserved.run.id,
            old.parent()
                .unwrap()
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
        );
        assert_eq!(preserved.run.sources["code.rs"], "fn a() {}");
        let rerun = saved
            .records
            .iter()
            .find(|r| r.attempt.test == "rerun")
            .unwrap();
        assert!(rerun.fresh);
        assert_eq!(rerun.run.sources["code.rs"], "fn a() { let changed=1; }");
    }

    // {test}`requirements.test-coverage.check.exceptions`
    // {test}`requirements.test-coverage.check.exceptions.reviews`
    // {test}`requirements.test-coverage.check.exceptions.binding`
    // {test}`requirements.test-coverage.check.report`
    // {test}`requirements.test-coverage.check.functions`
    // {test}`requirements.test-coverage.check.measurement`
    // {test}`requirements.test-coverage.check.threshold`
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
        let exceptions = result
            .text
            .split_once("\nAccepted exceptions (do not increase execution coverage):\n")
            .unwrap()
            .1;
        assert!(exceptions.contains("- code.rs: file threshold (0/1 (0.0%))"));
        assert!(exceptions.contains("- code.rs#fn[a]: zero execution"));
        let mut absent = process(root, &[(8, 0)], 0);
        absent.functions = Some(vec![]);
        record(root, "a", 3, "windows", vec![absent]);
        let result = report(root);
        assert!(
            result.passed,
            "same accepted citation covers unmeasured functions without conditional matching"
        );
        let exceptions = result
            .text
            .split_once("\nAccepted exceptions (do not increase execution coverage):\n")
            .unwrap()
            .1;
        assert!(exceptions.contains("- code.rs#fn[a]: not measured"));
        record(root, "a", 4, "covered", vec![process(root, &[(8, 1)], 1)]);
        assert!(
            report(root).passed,
            "citation may remain when no exception is needed"
        );
        fs::write(root.join("driver.rs"), "fn driver() {}").unwrap();
        let mut p = process(root, &[], 0);
        p.lines.insert("driver.rs".into(), BTreeMap::from([(1, 1)]));
        p.functions.as_mut().unwrap().push(FunctionExecution {
            file: "driver.rs".into(),
            line: 1,
            column: 1,
            count: 1,
        });
        record(root, "a", 5, "unmeasured-file", vec![p]);
        let result = report(root);
        assert!(result.passed, "{}", result.text);
        assert!(
            result
                .text
                .contains("code.rs: not measured — accepted exception"),
            "{}",
            result.text
        );
        assert!(!result.text.contains("code.rs: 0/"), "{}", result.text);
        reviews(root, json!(false));
        assert!(!report(root).passed);
        assert!(report(root).text.contains("determination is false"));
        reviews(root, json!({"FIXME":["acknowledgment"]}));
        assert!(report(root).text.contains("cannot accept a {fixme}"));
    }

    // {test}`requirements.test-coverage.check.exceptions.reviews`
    // {test}`requirements.test-coverage.check.exceptions.author`
    // {test}`requirements.test-coverage.check.exceptions`
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
                .all(|exception| exception.accepted)
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
                .all(|exception| !exception.accepted)
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
                .all(|exception| !exception.accepted)
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
                .all(|exception| exception.accepted)
        );
        assert!(errors.is_empty());
    }

    // {test}`requirements.test-coverage.check.measurement`
    // {test}`requirements.test-coverage.check.scope`
    // {test}`requirements.test-coverage.check.threshold`
    // {test}`requirements.test-coverage.check.report`
    // {test}`requirements.test-coverage.check.functions`
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
        let result = report(root);
        assert!(result.passed, "{}", result.text);
        assert!(
            result
                .text
                .contains("code.rs: no executable obligations — pass"),
            "{}",
            result.text
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
        for outcome in ["failed", "interrupted"] {
            attempt.outcome = outcome.into();
            save(&path.join("attempt.json"), &attempt).unwrap();
            let result = report(root);
            assert!(result.passed, "{}", result.text);
            assert!(result.text.contains(&format!("attempt {outcome}")));
        }
        attempt.processes[0].diagnostic = Some("broken coverage map".into());
        save(&path.join("attempt.json"), &attempt).unwrap();
        assert!(!report(root).passed);
    }

    // {test}`requirements.test-coverage.check.exceptions`
    // {test}`requirements.test-coverage.check.exceptions.reviews`
    // {test}`requirements.test-coverage.check.report`
    // {test}`requirements.test-coverage.check.threshold`
    #[test]
    fn missing_and_stale_explanations_fail_the_report() {
        let source = "//! :::{cite} habeas:coverage\n//! File rationale.\n//! :::\n// :::{cite} habeas:coverage\n// Function rationale.\n// :::\nfn a() {}\n";
        let temp = fixture(source);
        let root = temp.path();
        record(root, "a", 1, "same", vec![process(root, &[(7, 0)], 0)]);
        let result = report(root);
        assert!(!result.passed);
        assert!(
            result
                .text
                .contains("missing citation review; run `habeas author"),
            "{}",
            result.text
        );
        reviews(root, json!(true));
        assert!(report(root).passed);
        fs::write(
            root.join("code.rs"),
            source.replace("Function rationale", "Changed rationale"),
        )
        .unwrap();
        record(root, "a", 2, "same", vec![process(root, &[(7, 0)], 0)]);
        let result = report(root);
        assert!(!result.passed);
        assert!(result.text.contains("citations changed"), "{}", result.text);
        assert!(
            result.text.contains("#fn[a]: zero execution"),
            "{}",
            result.text
        );
        fs::write(root.join("code.rs"), source).unwrap();
        reviews(root, json!(true));
        fs::write(
            root.join("code.rs"),
            source.replace("fn a() {}", "fn a() { let changed = 1; }"),
        )
        .unwrap();
        record(root, "a", 3, "same", vec![process(root, &[(7, 0)], 0)]);
        let result = report(root);
        assert!(!result.passed);
        assert!(
            result.text.contains("outline input changed"),
            "{}",
            result.text
        );
        assert!(
            result.text.contains("code.rs:7 #fn[a]: zero execution"),
            "{}",
            result.text
        );
    }

    // {test}`requirements.test-coverage.check.exceptions`
    // {test}`requirements.test-coverage.check.exceptions.reviews`
    // {test}`requirements.test-coverage.check.threshold`
    #[test]
    fn function_explanations_never_transfer_to_other_functions_or_the_file() {
        for (source, waived, remaining) in [
            (
                "// :::{cite} habeas:coverage\n// Outer reason.\n// :::\nfn outer() {\n fn inner() {}\n}\nfn sibling() {}\n",
                "fn[outer]",
                ["fn[outer]/fn[inner]", "fn[sibling]"],
            ),
            (
                "fn outer() {\n // :::{cite} habeas:coverage\n // Inner reason.\n // :::\n fn inner() {}\n}\nfn sibling() {}\n",
                "fn[outer]/fn[inner]",
                ["fn[outer]", "fn[sibling]"],
            ),
        ] {
            let temp = fixture(source);
            let root = temp.path();
            reviews(root, json!(true));
            record(root, "a", 1, "same", vec![process(root, &[(1, 1)], 0)]);
            let result = report(root);
            assert!(!result.passed);
            assert!(
                result
                    .text
                    .contains(&format!("- code.rs#{waived}: zero execution")),
                "{}",
                result.text
            );
            for selector in remaining {
                assert!(
                    result
                        .text
                        .contains(&format!("#{selector}: zero execution; requires")),
                    "{}",
                    result.text
                );
            }
            assert!(
                !result
                    .text
                    .contains(&format!("#{waived}: zero execution; requires")),
                "{}",
                result.text
            );
            record(root, "a", 2, "same", vec![process(root, &[(1, 0)], 0)]);
            let result = report(root);
            assert!(
                result.text.contains("code.rs: 0/1 (0.0%) — FAIL"),
                "{}",
                result.text
            );
            fs::write(root.join("driver.rs"), "fn other_file() {}").unwrap();
            record(root, "a", 3, "same", vec![process(root, &[(1, 1)], 0)]);
            let result = report(root);
            assert!(result.text.contains("driver.rs: not measured — FAIL"));
            assert!(result.text.contains("#fn[other_file]: not measured"));
        }
    }

    // {test}`requirements.test-coverage.check.measurement`
    // {test}`requirements.ci.errors`
    #[test]
    fn accepted_exceptions_cannot_waive_invalid_measurement_and_checks_still_collect_errors() {
        let source = "//! :::{cite} habeas:coverage\n//! File rationale.\n//! :::\n// :::{cite} habeas:coverage\n// Function rationale.\n// :::\nfn a() {}\n";
        for failure in [
            "absent",
            "missing profile",
            "legacy",
            "corrupt counters",
            "stale",
            "corrupt history",
            "empty lines",
        ] {
            let temp = fixture(source);
            let root = temp.path();
            reviews(root, json!(true));
            if failure != "absent" {
                record(root, "a", 1, "same", vec![process(root, &[(7, 1)], 1)]);
                let mut p = process(root, &[(7, 0)], 0);
                if failure == "legacy" {
                    p.functions = None;
                }
                if failure == "corrupt counters" {
                    p.functions.as_mut().unwrap()[0].line = 0;
                }
                if failure == "empty lines" {
                    p.lines.clear();
                }
                let newest = record(root, "a", 2, "same", vec![p]);
                match failure {
                    "missing profile" => fs::remove_file(newest.join("profile.profraw")).unwrap(),
                    "stale" => fs::write(
                        root.join("code.rs"),
                        format!("{source}\n// changed build input\n"),
                    )
                    .unwrap(),
                    "corrupt history" => crate::execution_coverage::store::fixtures::corrupt(
                        &newest.parent().unwrap().parent().unwrap().join("run.json"),
                    ),
                    _ => {}
                }
            }
            let result = report(root);
            assert!(!result.passed, "{failure}: {}", result.text);
            assert!(
                !result.text.contains("accepted exception"),
                "{failure}: {}",
                result.text
            );
            assert!(
                !result.text.contains("0/1 (0.0%)"),
                "{failure}: {}",
                result.text
            );
            if failure == "legacy" {
                assert!(
                    result
                        .text
                        .contains("recollect coverage with this Habeas version")
                );
            }
            if failure == "corrupt counters" {
                assert!(result.text.contains("corrupt function attribution"));
            }
            if failure == "empty lines" {
                assert!(result.text.contains("No usable instrumented lines"));
            }
            fs::remove_file(root.join("evidence/citations/code.rs/fn[a].json")).unwrap();
            let result = report(root);
            assert!(
                result.text.contains("missing citation review"),
                "{failure}: {}",
                result.text
            );
            let mut seen = std::collections::BTreeSet::new();
            for line in result.text.lines().filter(|line| line.starts_with("- ")) {
                assert!(seen.insert(line), "duplicate finding: {line}");
            }
            fs::write(root.join("broken.rs"), "fn malformed(").unwrap();
            fs::write(root.join("book.toml"), "[book]\nsrc = 7\n").unwrap();
            let result = report(root);
            assert!(
                result
                    .text
                    .to_lowercase()
                    .contains("cannot parse rust source"),
                "{}",
                result.text
            );
            assert!(
                result.text.contains("book.src must be a string"),
                "{}",
                result.text
            );
        }
    }

    // {test}`requirements.test-coverage.check.scope`
    // {test}`requirements.test-coverage.check.functions`
    #[test]
    fn methods_tests_and_helpers_are_included_and_discovery_failures_are_reported() {
        let source = "struct S;\nimpl S { fn method(&self) {} }\n#[cfg(test)] mod tests {\n #[test] fn case() {}\n fn helper() {}\n}\n";
        let temp = fixture(source);
        let root = temp.path();
        record(root, "a", 1, "same", vec![process(root, &[(1, 1)], 0)]);
        let result = report(root);
        for selector in [
            "impl[S]/fn[method]",
            "mod[tests]/fn[case]",
            "mod[tests]/fn[helper]",
        ] {
            assert!(
                result
                    .text
                    .contains(&format!("#{selector}: zero execution")),
                "{}",
                result.text
            );
        }
        git2::Repository::init(root).unwrap();
        fs::write(root.join(".git/index"), "corrupt Git index").unwrap();
        let result = report(root);
        assert!(!result.passed);
        assert!(
            result.text.contains("cannot discover reference sources"),
            "{}",
            result.text
        );
    }

    // {test}`requirements.test-coverage.check.report`
    #[test]
    fn uncovered_ranges_and_function_locations_are_reported() {
        let temp = fixture("fn a() {\n let x = 1;\n let y = 2;\n let z = 3;\n}\n");
        let root = temp.path();
        record(
            root,
            "a",
            1,
            "same",
            vec![process(root, &[(1, 1), (2, 0), (3, 0), (4, 1), (5, 0)], 0)],
        );
        let result = report(root);
        assert!(!result.passed);
        assert!(
            result.text.contains("uncovered lines: 2-3, 5"),
            "{}",
            result.text
        );
        assert!(
            result
                .text
                .contains("code.rs:1 #fn[a]: zero execution; requires"),
            "{}",
            result.text
        );
    }

    // {test}`requirements.test-coverage.check.guidance`
    #[test]
    fn static_validation_never_uses_runtime_gaps_to_decide_citation_applicability() {
        let temp = fixture(
            "//! :::{cite} habeas:coverage\n//! File rationale.\n//! :::\n// :::{cite} habeas:coverage\n// Function rationale.\n// :::\nfn a() {}\n",
        );
        let root = temp.path();
        let (path, source) = crate::execution_coverage::configure(root).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
        reviews(root, json!(true));
        assert!(!root.join("target/habeas/coverage").exists());
        assert!(crate::validate_repository(root).unwrap().errors.is_empty());
        for (started, count) in [(1, 0), (2, 1)] {
            record(
                root,
                "a",
                started,
                "same",
                vec![process(root, &[(7, count)], count)],
            );
            let result = crate::validate_repository(root).unwrap();
            assert!(result.errors.is_empty(), "{:?}", result.errors);
        }
    }
}

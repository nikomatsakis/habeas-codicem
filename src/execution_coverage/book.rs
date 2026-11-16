use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mdbook_preprocessor::book::{Book, BookItem, Chapter};

use super::{Attempt, Run, fresh, load, target_directory, workspace_root};
use crate::{colon_fence, definition_blocks, overlaps_protected, protected_ranges};

// {impl}`requirements.test-coverage.index`
pub fn marker_ranges(content: &str) -> io::Result<Vec<Range<usize>>> {
    let protected = protected_ranges(content);
    let definitions = definition_blocks(content);
    let mut offset = 0;
    let lines: Vec<_> = content
        .split_inclusive('\n')
        .map(|line| {
            let start = offset;
            offset += line.len();
            (start, offset, line.trim_end_matches(['\n', '\r']))
        })
        .collect();
    let mut ranges = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let (start, end, line) = lines[index];
        let Some((suffix, length)) = colon_fence(line) else {
            index += 1;
            continue;
        };
        if !suffix.starts_with("{coverage-index}") || overlaps_protected(start, end, &protected) {
            index += 1;
            continue;
        }
        if suffix.trim() != "{coverage-index}"
            || definitions
                .iter()
                .any(|block| block.start <= start && start < block.end)
        {
            return Err(io::Error::other(
                "{coverage-index} must be a standalone directive with no arguments",
            ));
        }
        let close = (index + 1..lines.len())
            .find(|&next| {
                !overlaps_protected(lines[next].0, lines[next].1, &protected)
                    && colon_fence(lines[next].2)
                        .is_some_and(|(suffix, count)| count >= length && suffix.trim().is_empty())
            })
            .ok_or_else(|| io::Error::other("{coverage-index} is missing a closing colon fence"))?;
        if !content[end..lines[close].0].trim().is_empty() {
            return Err(io::Error::other("{coverage-index} must have an empty body"));
        }
        ranges.push(start..lines[close].1);
        index = close + 1;
    }
    Ok(ranges)
}

// {cite}`habeas:util`
fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
        .replace('|', "&#124;")
        .replace('\n', " ")
}

// {cite}`habeas:util`
fn display(text: &str) -> String {
    format!("<code>{}</code>", escaped(text))
}

struct Record {
    run: Arc<Run>,
    attempt: Attempt,
    fresh: bool,
    detail: PathBuf,
}

fn readable(record: &Record) -> bool {
    record.attempt.diagnostics.is_empty()
        && !record.attempt.processes.is_empty()
        && record.attempt.processes.iter().all(|process| {
            process.diagnostic.is_none()
                && process
                    .lines
                    .keys()
                    .all(|file| record.run.sources.contains_key(file))
        })
}

// {impl}`requirements.test-coverage.index.summary`
fn union<'a>(records: impl Iterator<Item = &'a Record>) -> BTreeMap<(String, u32), u64> {
    let mut result: BTreeMap<(String, u32), u64> = BTreeMap::new();
    for record in records {
        for process in &record.attempt.processes {
            for (file, lines) in &process.lines {
                for (&line, &count) in lines {
                    result
                        .entry((file.clone(), line))
                        .and_modify(|current| *current = (*current).max(count))
                        .or_insert(count);
                }
            }
        }
    }
    result
}

// {impl}`requirements.test-coverage.index`
// {impl}`requirements.test-coverage.index.selection`
// {impl}`requirements.test-coverage.index.freshness`
// {impl}`requirements.test-coverage.index.summary`
// {impl}`requirements.test-coverage.index.paths`
// {impl}`requirements.test-coverage.index.unavailable`
// {impl}`requirements.test-coverage.index.render_only`
// {impl}`requirements.test-coverage.index.details`
// {impl}`requirements.test-coverage.processes.reports`
// {impl}`requirements.test-coverage.storage.configuration`
pub(crate) fn populate(book: &mut Book, project: &Path) -> io::Result<()> {
    populate_in(book, project, None)
}

fn populate_in(book: &mut Book, project: &Path, target: Option<PathBuf>) -> io::Result<()> {
    let mut found = Vec::new();
    for chapter in book.chapters() {
        for range in marker_ranges(&chapter.content)? {
            found.push((
                chapter
                    .path
                    .clone()
                    .ok_or_else(|| io::Error::other("coverage index needs a chapter path"))?,
                range,
            ));
        }
    }
    if found.len() > 1 {
        return Err(io::Error::other(
            "only one {coverage-index} directive is permitted",
        ));
    }
    let Some((index_path, range)) = found.pop() else {
        return Ok(());
    };
    let mut diagnostics = Vec::new();
    let mut uncertain_history = false;
    let mut records = Vec::new();
    let root_and_target = workspace_root(project).and_then(|root| {
        Ok((
            target
                .map(Ok)
                .unwrap_or_else(|| target_directory(project))?,
            root,
        ))
    });
    match root_and_target {
        Err(error) => diagnostics.push(format!("Coverage storage is unavailable: {error}")),
        Ok((target, root)) => {
            let storage = target.join("habeas/coverage");
            if storage.is_dir() {
                let entries = match fs::read_dir(storage) {
                    Ok(entries) => entries.collect::<Vec<_>>(),
                    Err(error) => {
                        uncertain_history = true;
                        diagnostics.push(format!("Coverage storage is unreadable: {error}"));
                        Vec::new()
                    }
                };
                for entry in entries {
                    let dir = match entry {
                        Ok(entry) => entry.path(),
                        Err(error) => {
                            uncertain_history = true;
                            diagnostics.push(error.to_string());
                            continue;
                        }
                    };
                    if !dir.is_dir() {
                        continue;
                    }
                    let run: Run = match load(&dir.join("run.json")) {
                        Ok(run) => run,
                        Err(error) => {
                            uncertain_history = true;
                            diagnostics
                                .push(format!("{}: unavailable run data: {error}", dir.display()));
                            continue;
                        }
                    };
                    let current = fresh(&run, &root, &target).unwrap_or(false);
                    uncertain_history |= run.schema != 1;
                    for diagnostic in &run.diagnostics {
                        diagnostics.push(format!("Run {} ({}): {diagnostic}", run.id, run.outcome));
                    }
                    let attempts = match fs::read_dir(dir.join("attempts")) {
                        Ok(attempts) => attempts,
                        Err(error) => {
                            uncertain_history = true;
                            diagnostics.push(format!(
                                "Run {}: unavailable attempts directory {}: {error}",
                                run.id,
                                dir.join("attempts").display()
                            ));
                            continue;
                        }
                    };
                    let run = Arc::new(run);
                    for entry in attempts {
                        let attempt_dir = match entry {
                            Ok(entry) => entry.path(),
                            Err(error) => {
                                uncertain_history = true;
                                diagnostics.push(error.to_string());
                                continue;
                            }
                        };
                        let mut attempt: Attempt = match load(&attempt_dir.join("attempt.json")) {
                            Ok(attempt) => attempt,
                            Err(error) => {
                                uncertain_history = true;
                                diagnostics.push(format!(
                                    "{}: unavailable attempt data: {error}",
                                    attempt_dir.display()
                                ));
                                continue;
                            }
                        };
                        for process in &mut attempt.processes {
                            if process.profiles.iter().any(|profile| {
                                fs::File::open(attempt_dir.join(profile))
                                    .and_then(|file| file.metadata())
                                    .map_or(true, |metadata| metadata.len() == 0)
                            }) {
                                process.diagnostic = Some(
                                    "A retained raw profile is missing, empty or unreadable."
                                        .into(),
                                );
                            }
                            if process
                                .lines
                                .keys()
                                .any(|file| !run.sources.contains_key(file))
                            {
                                process.diagnostic = Some(
                                    "Matching captured source is missing; coverage is unavailable."
                                        .into(),
                                );
                            }
                        }
                        // IDs are generated by the collector, but disk data may be corrupt.
                        if ![&run.id, &attempt.id]
                            .iter()
                            .all(|id| uuid::Uuid::parse_str(id).is_ok())
                        {
                            diagnostics.push(format!(
                                "{}: invalid coverage identity",
                                attempt_dir.display()
                            ));
                            uncertain_history = true;
                            continue;
                        }
                        let detail = index_path
                            .parent()
                            .unwrap_or(Path::new(""))
                            .join("coverage-reports")
                            .join(format!("{}-{}.md", run.id, attempt.id));
                        records.push(Record {
                            run: Arc::clone(&run),
                            attempt,
                            fresh: current,
                            detail,
                        });
                    }
                }
            }
        }
    }
    records.sort_by(|a, b| {
        (a.attempt.started, &a.run.id, &a.attempt.id).cmp(&(
            b.attempt.started,
            &b.run.id,
            &b.attempt.id,
        ))
    });
    let active = records
        .last()
        .map(|record| record.run.configuration.clone());
    let mut selected = BTreeMap::new();
    for (index, record) in records.iter().enumerate() {
        if Some(&record.run.configuration) == active.as_ref() {
            selected.insert(
                (record.attempt.binary.clone(), record.attempt.test.clone()),
                index,
            );
        }
    }
    let selected: BTreeSet<_> = selected.into_values().collect();
    let totals = union(
        selected
            .iter()
            .map(|&index| &records[index])
            .filter(|record| !uncertain_history && record.fresh && readable(record)),
    );
    let covered = totals.values().filter(|&&count| count > 0).count();
    let mut summary = String::from("## Combined execution coverage\n\n");
    if uncertain_history {
        summary.push_str("Coverage history is incomplete: cannot establish latest-per-test selection. Combined totals are unavailable until the affected build artifacts are repaired or removed.\n\n");
    }
    if records.is_empty() {
        summary.push_str("No test coverage is available. Run `habeas coverage` (or `cargo habeas coverage` in this repository), then rebuild the book.\n\n");
    } else {
        let configuration = &records.last().unwrap().run.configuration_inputs;
        summary.push_str(&format!(
            "Build configuration: {}.\n\n",
            display(&serde_json::to_string(configuration)?)
        ));
        if totals.is_empty() {
            summary.push_str("Line coverage: **unavailable** (no fresh instrumented lines).\n\n");
        } else {
            summary.push_str(&format!("Line coverage: **{covered}/{} ({:.1}%)** from the union of instrumented file-and-line locations.\n\n", totals.len(), covered as f64 / totals.len() as f64 * 100.0));
        }
        summary.push_str("This combines saved attempts, including filtered runs. It does **not establish that all current tests were measured**. Failed, interrupted, stale and unavailable results are shown below; incomplete records do not contribute totals.\n\n");
        summary.push_str("| Test process | Attempt | Captured (Unix ns) | Run outcome / attempt outcome | Coverage | Originating run, revision and dirty state | Command / selection |\n| --- | --- | --- | --- | --- | --- | --- |\n");
        for (index, record) in records.iter().enumerate().rev() {
            let state = if uncertain_history {
                "unavailable: latest attempt history cannot be established"
            } else if Some(&record.run.configuration) != active.as_ref() {
                "excluded: historical build configuration"
            } else if !selected.contains(&index) {
                "excluded: superseded attempt"
            } else if !record.fresh {
                "stale: source inputs changed or unavailable"
            } else if !readable(record) {
                "unavailable: incomplete coverage"
            } else {
                "contributes"
            };
            let href = crate::chapter_href(&index_path, &record.detail, None);
            summary.push_str(&format!("| <a href=\"{}\">{} / {}</a> | {} | {} | {} / {} | {} | {}; revision {}; dirty {} | {} / {} |\n",
                escaped(&href), escaped(&record.attempt.binary), escaped(&record.attempt.test), escaped(&record.attempt.attempt),
                record.attempt.started, escaped(&record.run.outcome), escaped(&record.attempt.outcome), state, display(&record.run.id),
                display(record.run.revision.as_deref().unwrap_or("unknown")), record.run.dirty.map_or("unknown", |dirty| if dirty { "yes" } else { "no" }),
                display(&record.run.command.join(" ")), display(&record.run.selection.join(" "))));
        }
        summary.push('\n');
    }
    for diagnostic in diagnostics {
        summary.push_str(&format!(
            "**Coverage diagnostic:** {}\n\n",
            escaped(&diagnostic)
        ));
    }
    let existing: BTreeSet<_> = book
        .chapters()
        .filter_map(|chapter| chapter.path.clone())
        .collect();
    let mut generated = BTreeSet::new();
    let mut details = Vec::new();
    let mut sources = BTreeMap::new();
    for record in &records {
        if existing.contains(&record.detail) || !generated.insert(record.detail.clone()) {
            return Err(io::Error::other(format!(
                "coverage report path conflicts with an existing chapter: {}",
                record.detail.display()
            )));
        }
        let mut chapter = Chapter::new(
            &format!(
                "{} / {} #{}",
                record.attempt.binary, record.attempt.test, record.attempt.attempt
            ),
            detail(record, &index_path),
            record.detail.clone(),
            vec![],
        );
        chapter.source_path = None;
        details.push(BookItem::Chapter(chapter));
        sources
            .entry(record.run.id.clone())
            .or_insert_with(|| Arc::clone(&record.run));
    }
    for run in sources.into_values() {
        let path = source_path(&index_path, &run.id);
        if existing.contains(&path) || !generated.insert(path.clone()) {
            return Err(io::Error::other(format!(
                "coverage report path conflicts with an existing chapter: {}",
                path.display()
            )));
        }
        let mut chapter = Chapter::new(
            &format!("Captured source: {}", run.id),
            source_page(&run, &path, &index_path),
            path,
            vec![],
        );
        chapter.source_path = None;
        details.push(BookItem::Chapter(chapter));
    }
    book.for_each_chapter_mut(|chapter| {
        if chapter.path.as_ref() == Some(&index_path) {
            chapter.content.replace_range(range.clone(), &summary);
            chapter.sub_items.append(&mut details);
        }
    });
    Ok(())
}

// {impl}`requirements.test-coverage.index.details`
fn detail(record: &Record, index: &Path) -> String {
    let attempt = &record.attempt;
    let mut result = format!(
        "# Test process coverage\n\n[Back to coverage index]({})\n\nBinary: {}. Test: {}. Attempt: {}. Outcome: {}.\n\nRun: {} ({}). Captured at Unix ns {}.\n\n",
        crate::chapter_href(&record.detail, index, None),
        display(&attempt.binary),
        display(&attempt.test),
        display(&attempt.attempt),
        display(&attempt.outcome),
        display(&record.run.id),
        escaped(&record.run.outcome),
        attempt.started
    );
    for diagnostic in &attempt.diagnostics {
        result.push_str(&format!("**Unavailable:** {}\n\n", escaped(diagnostic)));
    }
    if attempt.processes.is_empty() {
        result.push_str("**Unavailable:** no process report has been collected.\n\n");
    }
    if !attempt.processes.is_empty() {
        let id = format!("habeas-process-{}-{}", record.run.id, attempt.id);
        let source = crate::chapter_href(&record.detail, &source_path(index, &record.run.id), None);
        let processes = attempt.processes.iter().map(|process| serde_json::json!({
            "pid": process.pid,
            "kind": if process.pid == attempt.pid { "Test" } else { "Child" },
            "profiles": process.profiles,
            "diagnostic": process.diagnostic,
            "lines": process.lines.iter().map(|(file, lines)| (file.clone(), lines.iter().map(|(line, count)| (line.to_string(), count.to_string())).collect::<BTreeMap<_, _>>())).collect::<BTreeMap<_, _>>(),
        })).collect::<Vec<_>>();
        let config = serde_json::json!({"target": id, "source": source, "run": record.run.id, "processes": processes});
        result.push_str(&format!("<div id=\"{id}\"></div>\n\n<script id=\"{id}-data\" type=\"application/json\">{}</script>\n\n<script>\n{}\nhabeasRenderCoverage(JSON.parse(document.getElementById('{id}-data').textContent));\n</script>\n\n<noscript>Enable JavaScript to view source lines with per-process execution counts.</noscript>\n\n", json_html(&config), include_str!("../../assets/coverage-viewer.js")));
    }
    result
}

fn source_path(index: &Path, run: &str) -> PathBuf {
    index
        .parent()
        .unwrap_or(Path::new(""))
        .join("coverage-reports")
        .join(format!("{run}-source.md"))
}

// {cite}`habeas:util`
fn json_html(value: &impl serde::Serialize) -> String {
    serde_json::to_string(value)
        .unwrap()
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
}

// {impl}`requirements.test-coverage.index.details`
fn source_page(run: &Run, path: &Path, index: &Path) -> String {
    let id = format!("habeas-source-{}", run.id);
    let mut result = format!(
        "# Captured source\n\n[Back to coverage index]({})\n\nRun: {}.\n\n<script id=\"{id}\" type=\"application/json\">{}</script>\n<script>\nif (window.parent !== window) window.parent.postMessage({{kind: 'habeas-coverage-source', run: '{}', sources: JSON.parse(document.getElementById('{id}').textContent)}}, '*');\n</script>\n\n",
        crate::chapter_href(path, index, None),
        display(&run.id),
        json_html(&run.sources),
        run.id
    );
    for (file, source) in &run.sources {
        result.push_str(&format!(
            "## {}\n\n<pre>{}</pre>\n\n",
            escaped(file),
            crate::escape_html(source)
        ));
    }
    result
}
#[cfg(test)]
mod tests {
    use super::super::{Process, save};
    use super::*;

    fn populate(book: &mut Book, root: &Path) -> io::Result<()> {
        super::populate_in(book, root, Some(root.join("target")))
    }

    // {test}`requirements.test-coverage.index`
    #[test]
    fn directive_fences_and_markdown_examples() {
        assert_eq!(
            marker_ranges("```text\n:::{coverage-index}\n:::\n```\n")
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            marker_ranges("::::{coverage-index}\n\n:::::\n")
                .unwrap()
                .len(),
            1
        );
        for source in [
            ":::{coverage-index}\n",
            ":::{coverage-index}\nbody\n:::\n",
            ":::{coverage-index} args\n:::\n",
        ] {
            assert!(marker_ranges(source).is_err(), "{source}");
        }
        let mut book = Book::new();
        book.push_item(Chapter::new(
            "One",
            ":::{coverage-index}\n:::\n".into(),
            "one.md",
            vec![],
        ));
        book.push_item(Chapter::new(
            "Two",
            ":::{coverage-index}\n:::\n".into(),
            "two.md",
            vec![],
        ));
        assert!(
            populate(&mut book, Path::new("/tmp"))
                .unwrap_err()
                .to_string()
                .contains("only one")
        );
    }

    fn fixture() -> (tempfile::TempDir, Book) {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(temp.path().join("code.rs"), "fn one() {}\nfn two() {}\n").unwrap();
        let mut book = Book::new();
        book.push_item(Chapter::new(
            "Coverage",
            ":::{coverage-index}\n:::\n".into(),
            "reports/coverage.md",
            vec![],
        ));
        (temp, book)
    }

    fn record(
        root: &Path,
        test: &str,
        start: u128,
        config: &str,
        counts: &[(u32, u64)],
    ) -> PathBuf {
        let dir = root
            .join("target/habeas/coverage")
            .join(uuid::Uuid::new_v4().to_string());
        let attempt_dir = dir.join("attempts").join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&attempt_dir).unwrap();
        fs::write(attempt_dir.join("1-1.profraw"), "retained profile fixture").unwrap();
        let run = Run {
            schema: 1,
            id: dir.file_name().unwrap().to_string_lossy().into_owned(),
            started: start,
            command: vec!["cargo nextest run".into()],
            selection: vec!["filtered".into()],
            configuration: config.into(),
            configuration_inputs: BTreeMap::new(),
            sources: BTreeMap::from([(
                "code.rs".into(),
                fs::read_to_string(root.join("code.rs")).unwrap(),
            )]),
            inputs: super::super::input_hashes(
                &super::super::workspace_inputs(root, &root.join("target")).unwrap(),
            )
            .unwrap(),
            environment_inputs: BTreeMap::new(),
            revision: Some("rev".into()),
            dirty: Some(true),
            outcome: "success".into(),
            diagnostics: vec![],
        };
        save(&dir.join("run.json"), &run).unwrap();
        let attempt = Attempt {
            id: attempt_dir
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            binary: "binary".into(),
            test: test.into(),
            attempt: "1".into(),
            started: start,
            pid: 1,
            command: vec![],
            outcome: "success".into(),
            diagnostics: vec![],
            processes: vec![Process {
                pid: 1,
                profiles: vec!["1-1.profraw".into()],
                diagnostic: None,
                lines: BTreeMap::from([("code.rs".into(), counts.iter().copied().collect())]),
            }],
        };
        save(&attempt_dir.join("attempt.json"), &attempt).unwrap();
        attempt_dir
    }

    // {test}`requirements.test-coverage.storage.provenance`
    // {test}`requirements.test-coverage.index.freshness`
    #[test]
    fn cargo_launch_metadata_does_not_invalidate_but_caller_build_environment_does() {
        let (temp, book) = fixture();
        let root = temp.path();
        let attempt = record(root, "one", 1, "build-a", &[(1, 1), (2, 0)]);
        let path = attempt.parent().unwrap().parent().unwrap().join("run.json");
        let mut run: Run = load(&path).unwrap();
        // An older report captured package-specific compiler values. Reading
        // through `cargo run` supplies values for Habeas instead of this package.
        for name in [
            "CARGO_MANIFEST_DIR",
            "CARGO_PKG_VERSION",
            "CARGO_CFG_TARGET_OS",
            "CARGO_FEATURE_ALPHA",
            "CARGO_BIN_EXE_fixture",
            "OUT_DIR",
        ] {
            run.environment_inputs
                .insert(name.into(), Some("different compiled package".into()));
        }
        save(&path, &run).unwrap();
        let mut rendered = book.clone();
        populate(&mut rendered, root).unwrap();
        assert!(
            rendered
                .chapters()
                .next()
                .unwrap()
                .content
                .contains("1/2 (50.0%)")
        );
        for name in [
            "CC",
            "RUSTFLAGS",
            "CARGO_PROFILE_TEST_DEBUG",
            "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER",
            "HABEAS_FIXTURE_BUILD_INPUT",
        ] {
            let changed = format!("changed-{}", uuid::Uuid::new_v4());
            assert_ne!(std::env::var(name).ok().as_deref(), Some(changed.as_str()));
            run.environment_inputs.insert(name.into(), Some(changed));
            save(&path, &run).unwrap();
            let mut rendered = book.clone();
            populate(&mut rendered, root).unwrap();
            assert!(
                rendered
                    .chapters()
                    .next()
                    .unwrap()
                    .content
                    .contains("stale: source inputs changed or unavailable"),
                "{name}"
            );
            run.environment_inputs.remove(name);
        }
    }

    // {test}`requirements.test-coverage.index`
    // {test}`requirements.test-coverage.processes.reports`
    // {test}`requirements.test-coverage.storage.configuration`
    // {test}`requirements.test-coverage.index.selection`
    // {test}`requirements.test-coverage.index.summary`
    // {test}`requirements.test-coverage.index.details`
    // {test}`requirements.test-coverage.index.freshness`
    // {test}`requirements.test-coverage.index.render_only`
    #[test]
    fn filtered_runs_union_latest_attempts_and_invalidate_with_saved_source() {
        let (temp, book) = fixture();
        let root = temp.path();
        record(root, "one", 1, "build-a", &[(1, 1), (2, 0)]);
        record(root, "two", 2, "build-a", &[(1, 0), (2, 1)]);
        let mut combined = book.clone();
        populate(&mut combined, root).unwrap();
        assert!(
            combined
                .chapters()
                .next()
                .unwrap()
                .content
                .contains("2/2 (100.0%)")
        );
        assert!(
            !combined
                .chapters()
                .next()
                .unwrap()
                .content
                .contains("{coverage-index}")
        );
        // A failing newest attempt replaces the earlier success, even with no data.
        let newest = record(root, "one", 3, "build-a", &[]);
        let mut attempt: Attempt = load(&newest.join("attempt.json")).unwrap();
        attempt.outcome = "failed".into();
        attempt.diagnostics.push("profile missing".into());
        save(&newest.join("attempt.json"), &attempt).unwrap();
        let mut combined = book.clone();
        populate(&mut combined, root).unwrap();
        assert!(
            combined
                .chapters()
                .next()
                .unwrap()
                .content
                .contains("1/2 (50.0%)")
        );
        assert!(
            combined
                .chapters()
                .next()
                .unwrap()
                .content
                .contains("unavailable: incomplete")
        );
        fs::write(root.join("code.rs"), "changed source\n").unwrap();
        let mut stale = book.clone();
        populate(&mut stale, root).unwrap();
        assert!(
            stale
                .chapters()
                .next()
                .unwrap()
                .content
                .contains("no fresh instrumented lines")
        );
        assert!(
            stale
                .chapters()
                .next()
                .unwrap()
                .content
                .contains("stale: source inputs changed or unavailable")
        );
        assert!(
            stale
                .chapters()
                .any(|chapter| chapter.content.contains("fn one() {}"))
        );
        record(root, "one", 4, "build-b", &[(1, 1)]);
        let mut other = book;
        populate(&mut other, root).unwrap();
        assert!(
            other
                .chapters()
                .next()
                .unwrap()
                .content
                .contains("historical build configuration")
        );
        assert!(
            other
                .chapters()
                .next()
                .unwrap()
                .content
                .contains("1/1 (100.0%)")
        );
    }

    // {test}`requirements.test-coverage.index.summary`
    // {test}`requirements.test-coverage.index.details`
    // {test}`requirements.test-coverage.processes.reports`
    #[test]
    fn reports_identify_attempts_and_union_child_counts_without_discarding_profiles() {
        let (temp, book) = fixture();
        let root = temp.path();
        let first = record(root, "one", 100, "a", &[(1, 1), (2, 0)]);
        fs::write(first.join("2-2.profraw"), "child profile fixture").unwrap();
        let mut attempt: Attempt = load(&first.join("attempt.json")).unwrap();
        attempt.processes.push(Process {
            pid: 2,
            profiles: vec!["2-2.profraw".into()],
            lines: BTreeMap::from([("code.rs".into(), BTreeMap::from([(1, 0), (2, 7)]))]),
            diagnostic: None,
        });
        save(&first.join("attempt.json"), &attempt).unwrap();
        let second = record(root, "two", 200, "a", &[(1, 1), (2, 0)]);
        let path = second.parent().unwrap().parent().unwrap().join("run.json");
        let mut run: Run = load(&path).unwrap();
        run.revision = None;
        run.dirty = None;
        save(&path, &run).unwrap();

        let mut combined = book.clone();
        populate(&mut combined, root).unwrap();
        let index = combined.chapters().next().unwrap();
        assert_eq!(index.sub_items.len(), 4); // two attempts and their source chapters
        for required in [
            "2/2 (100.0%)",
            "binary / one",
            "binary / two",
            "| 1 | 100 | success / success | contributes",
            "| 1 | 200 | success / success | contributes",
            "revision <code>rev</code>; dirty yes",
            "revision <code>unknown</code>; dirty unknown",
            "<code>cargo nextest run</code> / <code>filtered</code>",
            "not establish that all current tests were measured",
            &format!("<code>{}</code>", run.id),
        ] {
            assert!(
                index.content.contains(required),
                "summary omits {required}: {}",
                index.content
            );
        }
        for chapter in combined
            .chapters()
            .skip(1)
            .filter(|chapter| chapter.content.contains("# Test process coverage"))
        {
            let href = crate::chapter_href(
                index.path.as_ref().unwrap(),
                chapter.path.as_ref().unwrap(),
                None,
            );
            assert!(index.content.contains(&format!("href=\"{href}\"")));
            assert!(
                chapter
                    .content
                    .contains("[Back to coverage index](../coverage.html)")
            );
            for required in [
                "Binary: <code>binary</code>",
                "Attempt: <code>1</code>",
                "Outcome: <code>success</code>",
                "Captured at Unix ns",
            ] {
                assert!(
                    chapter.content.contains(required),
                    "detail omits {required}"
                );
            }
        }
        let detail = combined
            .chapters()
            .find(|chapter| chapter.content.contains("Test: <code>one</code>"))
            .unwrap();
        let first_run = first
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy();
        assert!(
            detail
                .content
                .contains(&format!("Run: <code>{first_run}</code>"))
        );
        assert!(detail.content.contains("\"kind\":\"Test\""));
        assert!(detail.content.contains("\"kind\":\"Child\""));
        assert!(detail.content.contains("\"1\":\"0\",\"2\":\"7\""));
        // Rebuilding/source editing cannot replace the saved source or delete profiles.
        fs::write(root.join("code.rs"), "rebuilt source\n").unwrap();
        let mut historical = book;
        populate(&mut historical, root).unwrap();
        assert!(
            historical
                .chapters()
                .any(|chapter| chapter.content.contains("fn one() {}"))
        );
        assert!(
            historical
                .chapters()
                .any(|chapter| chapter.content.contains("\"1\":\"0\",\"2\":\"7\""))
        );
        assert!(first.join("1-1.profraw").is_file());
        assert!(first.join("2-2.profraw").is_file());
    }

    // {test}`requirements.test-coverage.index.paths`
    // {test}`requirements.test-coverage.index.unavailable`
    #[test]
    fn unavailable_runs_are_readable_and_generated_paths_cannot_collide() {
        let (temp, mut book) = fixture();
        populate(&mut book, temp.path()).unwrap();
        assert!(
            book.chapters()
                .next()
                .unwrap()
                .content
                .contains("Run `habeas coverage`")
        );
        let (temp, mut book) = fixture();
        let attempt = record(temp.path(), "one", 1, "a", &[(1, 1)]);
        let id = attempt.file_name().unwrap().to_string_lossy();
        let run = attempt
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy();
        book.push_item(Chapter::new(
            "Collision",
            "".into(),
            format!("reports/coverage-reports/{run}-{id}.md"),
            vec![],
        ));
        assert!(
            populate(&mut book, temp.path())
                .unwrap_err()
                .to_string()
                .contains("conflicts")
        );
        // Captured-source paths also conflict with pre-existing chapters.
        let (temp, mut book) = fixture();
        let attempt = record(temp.path(), "one", 1, "a", &[(1, 1)]);
        let run_path = attempt.parent().unwrap().parent().unwrap();
        let run: Run = load(&run_path.join("run.json")).unwrap();
        book.push_item(Chapter::new(
            "Source collision",
            "".into(),
            source_path(Path::new("reports/coverage.md"), &run.id),
            vec![],
        ));
        assert!(
            populate(&mut book, temp.path())
                .unwrap_err()
                .to_string()
                .contains("conflicts")
        );
        // Corrupt duplicate identities must not create the same generated report twice.
        let (temp, mut book) = fixture();
        let first = record(temp.path(), "one", 1, "a", &[(1, 1)]);
        let duplicate = first
            .parent()
            .unwrap()
            .join(uuid::Uuid::new_v4().to_string());
        fs::create_dir(&duplicate).unwrap();
        fs::copy(first.join("attempt.json"), duplicate.join("attempt.json")).unwrap();
        fs::copy(first.join("1-1.profraw"), duplicate.join("1-1.profraw")).unwrap();
        assert!(
            populate(&mut book, temp.path())
                .unwrap_err()
                .to_string()
                .contains("conflicts")
        );
    }

    // {test}`requirements.test-coverage.index.selection`
    // {test}`requirements.test-coverage.index.unavailable`
    // {test}`requirements.test-coverage.index.freshness`
    // {test}`requirements.test-coverage.storage.outcomes`
    #[test]
    fn unreadable_history_cannot_reintroduce_an_older_success() {
        let (temp, mut book) = fixture();
        record(temp.path(), "one", 1, "a", &[(1, 1)]);
        let newest = record(temp.path(), "one", 2, "a", &[(1, 0)]);
        fs::write(newest.join("attempt.json"), "not JSON").unwrap();
        populate(&mut book, temp.path()).unwrap();
        let index = &book.chapters().next().unwrap().content;
        assert!(index.contains("cannot establish latest-per-test selection"));
        assert!(index.contains("no fresh instrumented lines"));
        assert!(!index.contains("1/1 (100.0%)"));
        let (temp, mut book) = fixture();
        let newest = record(temp.path(), "one", 1, "a", &[(1, 1)]);
        let path = newest.parent().unwrap().parent().unwrap().join("run.json");
        let mut run: Run = load(&path).unwrap();
        run.sources.clear();
        save(&path, &run).unwrap();
        populate(&mut book, temp.path()).unwrap();
        assert!(
            book.chapters()
                .next()
                .unwrap()
                .content
                .contains("unavailable: incomplete coverage")
        );
        assert!(
            book.chapters()
                .nth(1)
                .unwrap()
                .content
                .contains("Matching captured source is missing")
        );
        // Unsupported metadata cannot establish compatibility for combined totals.
        let (temp, mut book) = fixture();
        let attempt = record(temp.path(), "one", 1, "a", &[(1, 1)]);
        let path = attempt.parent().unwrap().parent().unwrap().join("run.json");
        let mut run: Run = load(&path).unwrap();
        run.schema = 99;
        save(&path, &run).unwrap();
        populate(&mut book, temp.path()).unwrap();
        let index = &book.chapters().next().unwrap().content;
        assert!(index.contains("unavailable: latest attempt history cannot be established"));
        assert!(index.contains("no fresh instrumented lines"));
        // A retained result with an unreadable/missing raw profile is unavailable.
        let (temp, mut book) = fixture();
        let attempt = record(temp.path(), "one", 1, "a", &[(1, 1)]);
        fs::remove_file(attempt.join("1-1.profraw")).unwrap();
        populate(&mut book, temp.path()).unwrap();
        assert!(
            book.chapters()
                .next()
                .unwrap()
                .content
                .contains("unavailable: incomplete coverage")
        );
        assert!(
            book.chapters()
                .next()
                .unwrap()
                .content
                .contains("no fresh instrumented lines")
        );
        assert!(
            book.chapters()
                .nth(1)
                .unwrap()
                .content
                .contains("A retained raw profile is missing")
        );
    }

    // {test}`requirements.test-coverage.index.details`
    #[test]
    fn rendered_detail_scripts_remain_literal_across_blank_lines() {
        let (temp, mut book) = fixture();
        record(temp.path(), "one", 1, "a", &[(1, 1), (2, 0)]);
        populate(&mut book, temp.path()).unwrap();
        let detail = &book.chapters().nth(1).unwrap().content;
        let mut html = String::new();
        pulldown_cmark::html::push_html(
            &mut html,
            pulldown_cmark::Parser::new_ext(detail, pulldown_cmark::Options::all()),
        );
        assert!(html.contains(include_str!("../../assets/coverage-viewer.js")));
        assert!(
            html.contains(
                "habeasRenderCoverage(JSON.parse(document.getElementById('habeas-process-"
            )
        );
        assert!(!html.contains("<p>const element"));
        assert!(html.contains("\"1\":\"1\",\"2\":\"0\""));
    }

    // {test}`requirements.test-coverage.index.unavailable`
    #[test]
    fn run_data_failures_render_diagnostics_and_leave_other_chapters_readable() {
        for case in [
            "missing run",
            "corrupt run",
            "missing attempts",
            "run diagnostic",
        ] {
            let (temp, mut book) = fixture();
            let attempt = record(temp.path(), "one", 1, "a", &[(1, 1)]);
            let run_dir = attempt.parent().unwrap().parent().unwrap();
            match case {
                "missing run" => fs::remove_file(run_dir.join("run.json")).unwrap(),
                "corrupt run" => fs::write(run_dir.join("run.json"), "not JSON").unwrap(),
                "missing attempts" => fs::remove_dir_all(run_dir.join("attempts")).unwrap(),
                "run diagnostic" => {
                    let mut run: Run = load(&run_dir.join("run.json")).unwrap();
                    run.outcome = "interrupted".into();
                    run.diagnostics.push("collection was interrupted".into());
                    save(&run_dir.join("run.json"), &run).unwrap();
                }
                _ => unreachable!(),
            }
            book.push_item(Chapter::new(
                "Other chapter",
                "Unrelated content survives.".into(),
                "other.md",
                vec![],
            ));
            populate(&mut book, temp.path()).unwrap();
            let index = &book.chapters().next().unwrap().content;
            assert!(
                index.contains("**Coverage diagnostic:**"),
                "{case}: {index}"
            );
            if case == "missing run" || case == "corrupt run" {
                assert!(index.contains("unavailable run data"), "{case}");
            } else if case == "missing attempts" {
                assert!(index.contains("attempts"), "{index}");
            } else {
                assert!(index.contains("(interrupted): collection was interrupted"));
            }
            assert!(
                book.chapters()
                    .any(|chapter| chapter.content == "Unrelated content survives.")
            );
        }
    }

    // {test}`requirements.test-coverage.index.unavailable`
    #[cfg(unix)]
    #[test]
    fn unreadable_storage_is_a_diagnostic_and_does_not_stop_book_rendering() {
        use std::os::unix::fs::PermissionsExt;
        let (temp, mut book) = fixture();
        record(temp.path(), "one", 1, "a", &[(1, 1)]);
        let storage = temp.path().join("target/habeas/coverage");
        fs::set_permissions(&storage, fs::Permissions::from_mode(0o000)).unwrap();
        let result = populate(&mut book, temp.path());
        fs::set_permissions(storage, fs::Permissions::from_mode(0o755)).unwrap();
        result.unwrap();
        let index = &book.chapters().next().unwrap().content;
        assert!(index.contains("**Coverage diagnostic:** Coverage storage is unreadable"));
        assert!(index.contains("No test coverage is available"));
    }
}

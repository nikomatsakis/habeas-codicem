use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mdbook_preprocessor::book::{Book, BookItem, Chapter, SectionNumber};

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

pub(super) struct Record {
    pub(super) run: Arc<Run>,
    pub(super) attempt: Attempt,
    pub(super) fresh: bool,
    detail: PathBuf,
}

pub(super) fn readable(record: &Record) -> bool {
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
// {impl}`requirements.test-coverage.index.file_map`
pub(super) fn union<'a>(records: impl Iterator<Item = &'a Record>) -> BTreeMap<(String, u32), u64> {
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

// {impl}`requirements.test-coverage.index.file_map`
#[derive(Clone, Copy)]
enum MapAvailability {
    Complete,
    Partial,
    Unavailable,
}

// {cite}`habeas:util`
struct FileCoverage {
    file: String,
    covered: usize,
    total: usize,
    state: &'static str,
}

#[derive(Default)]
// {cite}`habeas:util`
struct DirectoryCoverage {
    files: Vec<FileCoverage>,
    directories: BTreeMap<String, DirectoryCoverage>,
}

#[derive(Default)]
// {cite}`habeas:util`
struct DirectorySummary {
    minimum: Option<(usize, usize)>,
    files: usize,
    not_measured: usize,
    unavailable: usize,
}

impl DirectorySummary {
    // {cite}`habeas:util`
    fn include(&mut self, other: Self) {
        self.files += other.files;
        self.not_measured += other.not_measured;
        self.unavailable += other.unavailable;
        if let Some((covered, total)) = other.minimum
            && self.minimum.is_none_or(|(current, denominator)| {
                (covered as u128) * (denominator as u128) < (current as u128) * (total as u128)
            })
        {
            self.minimum = Some((covered, total));
        }
    }
}

impl DirectoryCoverage {
    fn summary(&self) -> DirectorySummary {
        let mut result = DirectorySummary::default();
        for file in &self.files {
            result.include(DirectorySummary {
                minimum: (file.state == "measured").then_some((file.covered, file.total)),
                files: 1,
                not_measured: usize::from(file.state == "not measured"),
                unavailable: usize::from(file.state == "unavailable"),
            });
        }
        for directory in self.directories.values() {
            result.include(directory.summary());
        }
        result
    }

    fn render(&self, result: &mut String, folder: &str, page: &Path, index: &Path) {
        if !self.files.is_empty() {
            result.push_str("<table class=\"habeas-coverage-table\"><thead><tr><th scope=\"col\">File</th><th scope=\"col\">Coverage</th><th scope=\"col\">Covered / instrumented lines</th></tr></thead><tbody>\n");
            let mut files: Vec<_> = self.files.iter().collect();
            files.sort_by_key(|file| &file.file);
            for file in files {
                let href = crate::chapter_href(page, &super::notes::path(index, &file.file), None);
                result.push_str(&format!(
                    "<tr class=\"habeas-coverage-file\" data-file=\"{}\" data-status=\"{}\"><td><a href=\"{}\"><code>{}</code></a></td>",
                    escaped(&file.file), file.state, escaped(&href), escaped(&file.file),
                ));
                if file.state == "measured" {
                    let percentage = 100.0 * file.covered as f64 / file.total as f64;
                    result.push_str(&format!(
                        "<td class=\"habeas-coverage-value\"><span class=\"habeas-coverage-bar\" role=\"meter\" aria-label=\"{} line coverage\" aria-valuemin=\"0\" aria-valuemax=\"100\" aria-valuenow=\"{percentage:.6}\"><span class=\"habeas-coverage-fill\" style=\"width:{percentage:.6}%;background:hsl({:.1},55%,45%)\"></span></span><strong>{}</strong></td><td aria-label=\"{}/{} instrumented lines covered\">{}/{}</td>",
                        escaped(&file.file), percentage * 1.2,
                        percentage_label(file.covered, file.total),
                        file.covered, file.total, file.covered, file.total,
                    ));
                } else {
                    result.push_str(&format!("<td>{}</td><td>—</td>", file.state));
                }
                result.push_str("</tr>\n");
            }
            result.push_str("</tbody></table>\n");
        }
        for (name, directory) in &self.directories {
            let path = if folder.is_empty() {
                name.clone()
            } else {
                format!("{folder}/{name}")
            };
            let summary = directory.summary();
            result.push_str(&format!(
                "<details class=\"habeas-coverage-directory\" data-directory=\"{}\"><summary><code>{}/</code><span>",
                escaped(&path), escaped(&path),
            ));
            if let Some((covered, total)) = summary.minimum {
                result.push_str(&format!(
                    "Minimum measured file coverage: <strong data-min-covered=\"{covered}\" data-min-instrumented=\"{total}\">{}</strong>",
                    percentage_label(covered, total),
                ));
            } else {
                result.push_str("No measured files");
            }
            result.push_str(&format!("</span><small>{} files", summary.files));
            if summary.not_measured > 0 {
                result.push_str(&format!(" · {} not measured", summary.not_measured));
            }
            if summary.unavailable > 0 {
                result.push_str(&format!(" · {} unavailable", summary.unavailable));
            }
            result.push_str("</small></summary><div class=\"habeas-coverage-directory-body\">\n");
            directory.render(result, &path, page, index);
            result.push_str("</div></details>\n");
        }
    }
}

// {cite}`habeas:util`
fn percentage_label(covered: usize, total: usize) -> String {
    let percentage = 100.0 * covered as f64 / total as f64;
    if covered > 0 && percentage < 0.1 {
        "&lt;0.1%".to_owned()
    } else {
        format!("{percentage:.1}%")
    }
}

// {impl}`requirements.test-coverage.index.file_map`
// {impl}`requirements.test-coverage.index.file_map.table`
// {impl}`requirements.test-coverage.index.file_map.directories`
// {impl}`requirements.test-coverage.index.file_map.directories.initial`
// {impl}`requirements.test-coverage.index.file_map.directories.data`
// {impl}`requirements.test-coverage.index.file_map.minimum`
// {impl}`requirements.test-coverage.index.file_map.minimum.statuses`
fn file_map(
    files: impl IntoIterator<Item = String>,
    totals: &BTreeMap<(String, u32), u64>,
    availability: MapAvailability,
    positive_only: bool,
    page: &Path,
    index: &Path,
) -> String {
    let mut counts = BTreeMap::<&str, (usize, usize)>::new();
    for ((file, _), count) in totals {
        let (covered, total) = counts.entry(file).or_default();
        *total += 1;
        *covered += usize::from(*count > 0);
    }
    let mut root = DirectoryCoverage::default();
    for file in files {
        let (covered, total) = counts.get(file.as_str()).copied().unwrap_or_default();
        if positive_only && covered == 0 {
            continue;
        }
        let state = match (availability, total) {
            (MapAvailability::Unavailable, _) | (MapAvailability::Partial, 0) => "unavailable",
            (_, 0) => "not measured",
            _ => "measured",
        };
        let mut directory = &mut root;
        if let Some((folder, _)) = file.rsplit_once('/') {
            for name in folder.split('/') {
                directory = directory.directories.entry(name.to_owned()).or_default();
            }
        }
        directory.files.push(FileCoverage {
            file,
            covered,
            total,
            state,
        });
    }
    let mut result = String::from(
        "<style>\n\
        .habeas-coverage-map { margin: 1.5em 0; }\n\
        .habeas-coverage-table { width: 100%; margin: .4em 0; }\n\
        .habeas-coverage-table td, .habeas-coverage-table th { padding: .3em .6em; text-align: left; }\n\
        .habeas-coverage-table td:first-child { width: 60%; overflow-wrap: anywhere; }\n\
        .habeas-coverage-table td:not(:first-child) { white-space: nowrap; }\n\
        .habeas-coverage-map code { background: transparent; padding: 0; overflow-wrap: anywhere; }\n\
        .habeas-coverage-value strong { display: inline-block; min-width: 4.5em; text-align: right; }\n\
        .habeas-coverage-bar { display: inline-block; vertical-align: middle; width: 48px; height: 6px; border-radius: 3px; background: var(--table-border, #ddd); overflow: hidden; margin-right: .5em; }\n\
        .habeas-coverage-fill { display: block; height: 100%; }\n\
        .habeas-coverage-directory { border-bottom: 1px solid var(--table-border, #888); }\n\
        .habeas-coverage-directory > summary { cursor: pointer; padding: .5em 0; }\n\
        .habeas-coverage-directory > summary > code { font-weight: bold; }\n\
        .habeas-coverage-directory > summary > span { margin-left: 1em; }\n\
        .habeas-coverage-directory > summary > small { margin-left: 1em; opacity: .8; }\n\
        .habeas-coverage-directory-body { padding-left: 1em; overflow-x: auto; }\n\
        .habeas-coverage-picker { width: 100%; padding: .5em; margin: .5em 0; color: var(--fg, inherit); background: var(--bg, inherit); }\n\
        .habeas-coverage-details > summary { cursor: pointer; font-weight: bold; padding: .7em 0; }\n\
        </style>\n\
        <section class=\"habeas-coverage-map\" aria-label=\"File coverage map\">\n",
    );
    result.push_str("<p>Percentages measure recorded instrumented lines. Expand a directory to browse files; select a file to read its current coverage explanations.</p>\n");
    if root.files.is_empty() && root.directories.is_empty() && positive_only {
        result.push_str("<p>No files covered by this test</p>\n");
    }
    root.render(&mut result, "", page, index);
    result.push_str("</section>\n\n");
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
// {impl}`requirements.test-coverage.index.file_map`
// {impl}`requirements.test-coverage.index.file_map.combined`
pub(crate) fn populate(book: &mut Book, project: &Path) -> io::Result<()> {
    populate_in(book, project, None)
}

// {cite}`habeas:util`
pub(super) struct SavedCoverage {
    pub(super) records: Vec<Record>,
    pub(super) selected: BTreeSet<usize>,
    pub(super) active: Option<String>,
    pub(super) diagnostics: Vec<String>,
    pub(super) uncertain_history: bool,
    current_files: BTreeSet<String>,
    discovery_failed: bool,
}

// {impl}`requirements.test-coverage.index.selection`
// {impl}`requirements.test-coverage.index.freshness`
// {impl}`requirements.test-coverage.check.measurement`
pub(super) fn load_saved(
    project: &Path,
    target: Option<PathBuf>,
    index_path: &Path,
) -> SavedCoverage {
    let mut diagnostics = Vec::new();
    let mut uncertain_history = false;
    let mut current_files = BTreeSet::new();
    let mut discovery_failed = false;
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
            let mut errors = Vec::new();
            let discovered = crate::evidence::discovered_files(&root, &mut errors);
            discovery_failed = !errors.is_empty();
            diagnostics.extend(errors);
            current_files.extend(
                crate::rust_coverage::rust_files(&root, &discovered)
                    .into_iter()
                    .filter_map(|file| {
                        file.strip_prefix(&root)
                            .ok()
                            .map(|relative| relative.to_string_lossy().into_owned())
                    }),
            );
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
                            if process.profiles.is_empty()
                                || process.profiles.iter().any(|profile| {
                                    fs::File::open(attempt_dir.join(profile))
                                        .and_then(|file| file.metadata())
                                        .map_or(true, |metadata| metadata.len() == 0)
                                })
                            {
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
                            if process.lines.values().any(|lines| lines.contains_key(&0))
                                || process.functions.iter().flatten().any(|function| {
                                    function.line == 0
                                        || function.column == 0
                                        || !run.sources.contains_key(&function.file)
                                })
                            {
                                process.diagnostic =
                                    Some("Corrupt normalized coverage locations.".into());
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
    SavedCoverage {
        records,
        selected,
        active,
        diagnostics,
        uncertain_history,
        current_files,
        discovery_failed,
    }
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
    let SavedCoverage {
        records,
        selected,
        active,
        diagnostics,
        uncertain_history,
        current_files,
        discovery_failed,
    } = load_saved(project, target, &index_path);
    let note_files = current_files
        .iter()
        .chain(records.iter().flat_map(|record| record.run.sources.keys()))
        .filter(|file| file.ends_with(".rs"))
        .cloned()
        .collect::<BTreeSet<_>>();
    let usable: Vec<_> = selected
        .iter()
        .map(|&index| &records[index])
        .filter(|record| !uncertain_history && record.fresh && readable(record))
        .collect();
    let totals = union(usable.iter().copied());
    let covered = totals.values().filter(|&&count| count > 0).count();
    let mut summary = String::from("## Combined execution coverage\n\n");
    if uncertain_history {
        summary.push_str("Coverage history is incomplete: cannot establish latest-per-test selection. Combined totals are unavailable until the affected build artifacts are repaired or removed.\n\n");
    }
    if records.is_empty() {
        summary.push_str("No test coverage is available. Run `habeas coverage` (or `cargo habeas coverage` in this repository), then rebuild the book.\n\n");
    } else {
        if totals.is_empty() {
            summary.push_str("Line coverage: **unavailable** (no fresh instrumented lines).\n\n");
        } else {
            summary.push_str(&format!("Line coverage: **{covered}/{} ({:.1}%)** from the union of instrumented file-and-line locations.\n\n", totals.len(), covered as f64 / totals.len() as f64 * 100.0));
        }
        summary.push_str("This combines saved attempts, including filtered runs. It does **not establish that all current tests were measured**. Failed, interrupted, stale and unavailable results are shown below; incomplete records do not contribute totals.\n\n");
    }
    let stale = selected
        .iter()
        .filter(|&&index| !records[index].fresh)
        .count();
    let incomplete = selected
        .iter()
        .filter(|&&index| records[index].fresh && !readable(&records[index]))
        .count();
    if stale > 0 || incomplete > 0 {
        summary.push_str(&format!(
            "**Excluded coverage:** {stale} stale test attempts; {incomplete} unavailable test attempts.\n\n"
        ));
    }
    if !selected.is_empty() {
        summary.push_str("<div>\n<label for=\"habeas-coverage-test\">View a test's coverage map</label>\n<select id=\"habeas-coverage-test\" class=\"habeas-coverage-picker\" onchange=\"if(this.value) window.location.href=this.value\"><option value=\"\">Combined coverage</option>\n");
        let mut choices: Vec<_> = selected.iter().map(|&index| &records[index]).collect();
        choices.sort_by_key(|record| (&record.attempt.binary, &record.attempt.test));
        for record in choices {
            summary.push_str(&format!(
                "<option value=\"{}\">{} / {}</option>\n",
                escaped(&crate::chapter_href(&index_path, &record.detail, None)),
                escaped(&record.attempt.binary),
                escaped(&record.attempt.test),
            ));
        }
        summary.push_str("</select>\n</div>\n\n");
    }
    let availability = if discovery_failed
        || uncertain_history
        || (!records.is_empty() && usable.is_empty())
        || (records.is_empty() && !diagnostics.is_empty())
    {
        MapAvailability::Unavailable
    } else if stale > 0 || incomplete > 0 {
        MapAvailability::Partial
    } else {
        MapAvailability::Complete
    };
    summary.push_str(&file_map(
        current_files,
        &totals,
        availability,
        false,
        &index_path,
        &index_path,
    ));
    if !records.is_empty() {
        summary.push_str("<details class=\"habeas-coverage-details\"><summary>Test runs and provenance</summary>\n\n");
        let configuration = &records.last().unwrap().run.configuration_inputs;
        summary.push_str(&format!(
            "Build configuration: {}.\n\n",
            display(&serde_json::to_string(configuration)?)
        ));
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
        summary.push_str("\n</details>\n\n");
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
    let notes_root = workspace_root(project).unwrap_or_else(|_| project.to_path_buf());
    for chapter in super::notes::pages(&notes_root, &note_files, &index_path) {
        let path = chapter
            .path
            .as_ref()
            .expect("file notes have a render path");
        if existing.contains(path) || !generated.insert(path.clone()) {
            return Err(io::Error::other(format!(
                "coverage report path conflicts with an existing chapter: {}",
                path.display()
            )));
        }
        details.push(BookItem::Chapter(chapter));
    }
    book.for_each_chapter_mut(|chapter| {
        if chapter.path.as_ref() == Some(&index_path) {
            chapter.content.replace_range(range.clone(), &summary);
            let mut parent_names = chapter.parent_names.clone();
            parent_names.push(chapter.name.clone());
            let first_number = chapter
                .sub_items
                .iter()
                .filter_map(|item| match item {
                    BookItem::Chapter(child) => child.number.as_ref()?.last().copied(),
                    _ => None,
                })
                .max()
                .unwrap_or(0)
                + 1;
            for (offset, item) in details.iter_mut().enumerate() {
                let BookItem::Chapter(page) = item else {
                    unreachable!("generated coverage reports are chapters");
                };
                page.parent_names = parent_names.clone();
                page.number = chapter.number.as_ref().map(|number| {
                    let mut number = number.to_vec();
                    number.push(first_number + offset as u32);
                    SectionNumber::new(number)
                });
            }
            chapter.sub_items.append(&mut details);
        }
    });
    Ok(())
}

// {impl}`requirements.test-coverage.index.details`
// {impl}`requirements.test-coverage.index.file_map`
// {impl}`requirements.test-coverage.index.file_map.test`
fn detail(record: &Record, index: &Path) -> String {
    let attempt = &record.attempt;
    let mut result = format!(
        "# Test process coverage\n\n[Back to coverage index]({})\n\n## {}\n\n",
        crate::chapter_href(&record.detail, index, None),
        display(&attempt.test),
    );
    if readable(record) {
        result.push_str(&file_map(
            record.run.sources.keys().cloned(),
            &union(std::iter::once(record)),
            MapAvailability::Complete,
            true,
            &record.detail,
            index,
        ));
    } else {
        result.push_str("**Coverage unavailable:** this test's coverage is missing, unreadable or incomplete.\n\n");
    }
    result.push_str(&format!(
        "<details class=\"habeas-coverage-details\"><summary>Run and attempt details</summary>\n\nBinary: {}. Test: {}. Attempt: {}. Outcome: {}.\n\nRun: {} ({}). Captured at Unix ns {}.\n\n</details>\n\n",
        display(&attempt.binary),
        display(&attempt.test),
        display(&attempt.attempt),
        display(&attempt.outcome),
        display(&record.run.id),
        escaped(&record.run.outcome),
        attempt.started
    ));
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
        result.push_str(&format!("<details id=\"{id}-details\" class=\"habeas-coverage-details\"><summary>Source lines and individual processes</summary>\n<div id=\"{id}\"></div>\n</details>\n\n<script id=\"{id}-data\" type=\"application/json\">{}</script>\n\n<script>\n{}\nhabeasRenderCoverage(JSON.parse(document.getElementById('{id}-data').textContent));\n</script>\n\n<noscript>Enable JavaScript to view source lines with per-process execution counts.</noscript>\n\n", json_html(&config), include_str!("../../assets/coverage-viewer.js")));
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
        for example in [
            "```text\n:::{coverage-index}\n:::\n```\n",
            "~~~text\n:::{coverage-index}\n:::\n~~~\n",
            "    :::{coverage-index}\n    :::\n",
            "`:::{coverage-index}`\n",
            "`\n:::{coverage-index}\n:::\n`\n",
        ] {
            assert!(marker_ranges(example).unwrap().is_empty(), "{example}");
        }
        assert_eq!(
            marker_ranges("::::{coverage-index}\n\n:::::\n")
                .unwrap()
                .len(),
            1
        );
        for source in [
            ":::{coverage-index}\n",
            "::::{coverage-index}\n:::\n",
            ":::{coverage-index}\nbody\n:::\n",
            ":::{coverage-index} args\n:::\n",
            "::::{req} outer\n:::{coverage-index}\n:::\n::::\n",
            "::::{example} outer\n:::{coverage-index}\n:::\n::::\n",
            "::::{definition} outer\n:::{coverage-index}\n:::\n::::\n",
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
                functions: None,
                pid: 1,
                profiles: vec!["1-1.profraw".into()],
                diagnostic: None,
                lines: BTreeMap::from([("code.rs".into(), counts.iter().copied().collect())]),
            }],
        };
        save(&attempt_dir.join("attempt.json"), &attempt).unwrap();
        attempt_dir
    }

    fn map_html(content: &str) -> &str {
        content
            .split("<section class=\"habeas-coverage-map\"")
            .nth(1)
            .unwrap()
            .split("</section>")
            .next()
            .unwrap()
    }

    fn file_card<'a>(content: &'a str, file: &str) -> &'a str {
        content
            .split(&format!("data-file=\"{}\"", escaped(file)))
            .nth(1)
            .unwrap()
            .split("</tr>")
            .next()
            .unwrap()
    }

    // {test}`requirements.test-coverage.index.details`
    #[test]
    fn generated_reports_inherit_the_index_hierarchy_after_existing_children() {
        for number in [Some(SectionNumber::new(vec![3, 2])), None] {
            let (temp, mut book) = fixture();
            let BookItem::Chapter(index) = &mut book.items[0] else {
                unreachable!();
            };
            index.parent_names = vec!["Reports".into()];
            index.number = number.clone();
            let mut child = Chapter::new(
                "Guide",
                "# Guide\n".into(),
                "reports/guide.md",
                vec!["Reports".into(), "Coverage".into()],
            );
            child.number = number.as_ref().map(|number| {
                let mut number = number.to_vec();
                number.push(4);
                SectionNumber::new(number)
            });
            index.sub_items.push(BookItem::Chapter(child.clone()));
            record(temp.path(), "one", 1, "a", &[(1, 1)]);
            record(temp.path(), "two", 2, "a", &[(2, 1)]);
            populate(&mut book, temp.path()).unwrap();
            let BookItem::Chapter(index) = &book.items[0] else {
                unreachable!();
            };
            assert_eq!(index.number, number);
            assert_eq!(index.sub_items[0], BookItem::Chapter(child));
            assert_eq!(index.sub_items.len(), 6);
            for (offset, item) in index.sub_items[1..].iter().enumerate() {
                let BookItem::Chapter(page) = item else {
                    unreachable!();
                };
                assert_eq!(page.parent_names, ["Reports", "Coverage"]);
                let expected = number
                    .as_ref()
                    .map(|_| SectionNumber::new(vec![3, 2, 5 + offset as u32]));
                assert_eq!(page.number, expected);
                assert!(page.source_path.is_none());
                assert!(page.content.contains("../coverage.html"));
            }
        }
    }

    // {test}`requirements.test-coverage.index.file_map`
    // {test}`requirements.test-coverage.index.file_map.table`
    // {test}`requirements.test-coverage.index.file_map.combined`
    // {test}`requirements.test-coverage.index.file_map.test`
    // {test}`requirements.test-coverage.index.summary`
    #[test]
    fn file_maps_union_process_locations_and_filter_unexecuted_test_files() {
        let (temp, mut book) = fixture();
        let root = temp.path();
        fs::write(
            root.join("code.rs"),
            "fn one() {}\nfn two() {}\nfn three() {}\n// not instrumented\n",
        )
        .unwrap();
        fs::write(root.join("zero.rs"), "fn zero() {}\n").unwrap();
        fs::write(root.join("unmeasured.rs"), "// no instrumentation\n").unwrap();
        let path = record(root, "one", 1, "a", &[(1, 2), (2, 0)]);
        let run_path = path.parent().unwrap().parent().unwrap().join("run.json");
        let mut run: Run = load(&run_path).unwrap();
        for file in ["zero.rs", "unmeasured.rs"] {
            run.sources
                .insert(file.into(), fs::read_to_string(root.join(file)).unwrap());
        }
        save(&run_path, &run).unwrap();
        let mut attempt: Attempt = load(&path.join("attempt.json")).unwrap();
        attempt.processes[0]
            .lines
            .insert("zero.rs".into(), BTreeMap::from([(1, 0)]));
        fs::write(path.join("2-child.profraw"), "child profile fixture").unwrap();
        attempt.processes.push(Process {
            functions: None,
            pid: 2,
            profiles: vec!["2-child.profraw".into()],
            diagnostic: None,
            lines: BTreeMap::from([("code.rs".into(), BTreeMap::from([(1, 0), (3, u64::MAX)]))]),
        });
        save(&path.join("attempt.json"), &attempt).unwrap();
        populate(&mut book, root).unwrap();
        let index = &book.chapters().next().unwrap().content;
        assert!(index.contains("Line coverage: **2/4 (50.0%)**"));
        let map = map_html(index);
        assert!(file_card(map, "code.rs").contains("2/3 instrumented lines covered"));
        assert!(file_card(map, "code.rs").contains("66.7%"));
        assert!(file_card(map, "zero.rs").contains("0.0%"));
        let unmeasured = file_card(map, "unmeasured.rs");
        assert!(unmeasured.contains("not measured"));
        assert!(!unmeasured.contains('%'));
        assert!(index.contains("View a test's coverage map"));
        // mdBook closes inline HTML tags at paragraph boundaries. Keep the
        // complete selector in a raw HTML block so its options stay inside it.
        let mut paragraph = false;
        let mut picker = false;
        for event in pulldown_cmark::Parser::new_ext(index, pulldown_cmark::Options::all()) {
            match event {
                pulldown_cmark::Event::Start(pulldown_cmark::Tag::Paragraph) => paragraph = true,
                pulldown_cmark::Event::End(pulldown_cmark::TagEnd::Paragraph) => paragraph = false,
                pulldown_cmark::Event::Html(text) | pulldown_cmark::Event::InlineHtml(text)
                    if text.contains("<select id=\"habeas-coverage-test\"") =>
                {
                    assert!(
                        !paragraph,
                        "the test selector must survive mdBook rendering"
                    );
                    picker = true;
                }
                _ => {}
            }
        }
        assert!(picker);
        let mut html = String::new();
        pulldown_cmark::html::push_html(
            &mut html,
            pulldown_cmark::Parser::new_ext(index, pulldown_cmark::Options::all()),
        );
        assert!(html.contains("<details class=\"habeas-coverage-details\">"));
        assert!(!html.contains("<details class=\"habeas-coverage-details\" open"));
        assert!(html.contains("<table>"));
        let select = html
            .split("<select id=\"habeas-coverage-test\"")
            .nth(1)
            .unwrap()
            .split("</select>")
            .next()
            .unwrap();
        assert_eq!(
            select.matches("<option value=\"coverage-reports/").count(),
            1
        );
        let detail = &book.chapters().nth(1).unwrap().content;
        let map = map_html(detail);
        assert!(file_card(map, "code.rs").contains("2/3 instrumented lines covered"));
        assert!(!map.contains("zero.rs"));
        assert!(!map.contains("unmeasured.rs"));
        assert!(detail.contains("Source lines and individual processes"));
        assert!(!detail.contains("class=\"habeas-coverage-details\" open"));
        assert!(detail.contains("\"kind\":\"Child\""));
        assert!(
            detail.find("habeas-coverage-map").unwrap()
                < detail.find("Run and attempt details").unwrap()
        );
    }

    // {test}`requirements.test-coverage.index.file_map`
    // {test}`requirements.test-coverage.index.file_map.table`
    // {test}`requirements.test-coverage.index.file_map.combined`
    // {test}`requirements.test-coverage.index.file_map.test`
    #[test]
    fn maps_distinguish_no_runs_zero_hits_stale_and_incomplete_coverage() {
        let (temp, book) = fixture();
        let root = temp.path();
        let mut no_runs = book.clone();
        populate(&mut no_runs, root).unwrap();
        let index = &no_runs.chapters().next().unwrap().content;
        assert!(file_card(map_html(index), "code.rs").contains("not measured"));
        assert!(index.contains("Run `habeas coverage`"));

        let path = record(root, "zero", 1, "a", &[(1, 0), (2, 0)]);
        let mut zero = book.clone();
        populate(&mut zero, root).unwrap();
        assert!(
            file_card(
                map_html(&zero.chapters().next().unwrap().content),
                "code.rs"
            )
            .contains("0.0%")
        );
        let detail = &zero.chapters().nth(1).unwrap().content;
        assert!(map_html(detail).contains("No files covered by this test"));
        assert!(!map_html(detail).contains("data-file="));

        let mut attempt: Attempt = load(&path.join("attempt.json")).unwrap();
        attempt.processes[0]
            .lines
            .get_mut("code.rs")
            .unwrap()
            .insert(1, 1);
        for failure in [
            "no processes",
            "attempt diagnostic",
            "process diagnostic",
            "missing source",
            "empty profile",
        ] {
            let mut missing = attempt.clone();
            match failure {
                "no processes" => missing.processes.clear(),
                "attempt diagnostic" => missing.diagnostics.push("export incomplete".into()),
                "process diagnostic" => {
                    missing.processes[0].diagnostic = Some("unreadable profile".into());
                }
                "missing source" => {
                    missing.processes[0]
                        .lines
                        .insert("uncaptured.rs".into(), BTreeMap::from([(1, 1)]));
                }
                "empty profile" => fs::write(path.join("1-1.profraw"), "").unwrap(),
                _ => unreachable!(),
            }
            save(&path.join("attempt.json"), &missing).unwrap();
            let mut unavailable = book.clone();
            populate(&mut unavailable, root).unwrap();
            let detail = &unavailable.chapters().nth(1).unwrap().content;
            assert!(detail.contains("**Coverage unavailable:**"), "{failure}");
            assert!(!detail.contains("habeas-coverage-map"), "{failure}");
            assert!(!detail.contains("instrumented lines covered"), "{failure}");
            assert!(
                !detail.contains("No files covered by this test"),
                "{failure}"
            );
            assert!(detail.contains("Run and attempt details"), "{failure}");
            fs::write(path.join("1-1.profraw"), "retained profile fixture").unwrap();
        }
        save(&path.join("attempt.json"), &attempt).unwrap();
        fs::write(root.join("code.rs"), "changed source\n").unwrap();
        fs::write(root.join("new.rs"), "fn newly_added() {}\n").unwrap();
        let mut stale = book.clone();
        populate(&mut stale, root).unwrap();
        let index = &stale.chapters().next().unwrap().content;
        for file in ["code.rs", "new.rs"] {
            let card = file_card(map_html(index), file);
            assert!(card.contains("unavailable"));
            assert!(!card.contains('%'));
        }
        assert!(index.contains("1 stale test attempts"));
        // The historical attempt's complete map uses its captured source.
        let detail = &stale.chapters().nth(1).unwrap().content;
        assert!(file_card(map_html(detail), "code.rs").contains("50.0%"));
        assert!(!map_html(detail).contains("new.rs"));

        attempt.processes.push(Process {
            functions: None,
            pid: 2,
            profiles: vec!["missing-child.profraw".into()],
            diagnostic: None,
            lines: BTreeMap::from([("code.rs".into(), BTreeMap::from([(2, 1)]))]),
        });
        save(&path.join("attempt.json"), &attempt).unwrap();
        let mut incomplete = book.clone();
        populate(&mut incomplete, root).unwrap();
        let detail = &incomplete.chapters().nth(1).unwrap().content;
        assert!(detail.contains("**Coverage unavailable:**"));
        assert!(!detail.contains("habeas-coverage-map"));
        assert!(!detail.contains("instrumented lines covered"));
        assert!(!detail.contains("No files covered by this test"));
        assert!(detail.contains("Source lines and individual processes"));
        assert!(detail.contains("\"kind\":\"Test\""));

        fs::write(root.join("code.rs"), "fn one() {}\nfn two() {}\n").unwrap();
        fs::remove_file(root.join("new.rs")).unwrap();
        let mut unreadable = book.clone();
        populate(&mut unreadable, root).unwrap();
        let index = &unreadable.chapters().next().unwrap().content;
        assert!(index.contains("1 unavailable test attempts"));
        let card = file_card(map_html(index), "code.rs");
        assert!(card.contains("unavailable"));
        assert!(!card.contains('%'));
        // Unreadable metadata cannot establish latest-per-test selection.
        fs::write(path.join("attempt.json"), "not JSON").unwrap();
        let mut uncertain = book;
        populate(&mut uncertain, root).unwrap();
        let index = &uncertain.chapters().next().unwrap().content;
        assert!(index.contains("Coverage history is incomplete"));
        assert!(file_card(map_html(index), "code.rs").contains("unavailable"));
    }

    // {test}`requirements.test-coverage.index.file_map`
    // {test}`requirements.test-coverage.index.file_map.table`
    // {test}`requirements.test-coverage.index.file_map.combined`
    // {test}`requirements.test-coverage.index.freshness`
    #[test]
    fn incomplete_attempts_do_not_label_their_files_as_unmeasured_in_a_partial_map() {
        let (temp, book) = fixture();
        let root = temp.path();
        fs::write(root.join("other.rs"), "fn other() {}\n").unwrap();
        let good = record(root, "good", 1, "a", &[(1, 1)]);
        let bad = record(root, "bad", 2, "a", &[]);
        let mut attempt: Attempt = load(&bad.join("attempt.json")).unwrap();
        attempt.processes[0].lines =
            BTreeMap::from([("other.rs".into(), BTreeMap::from([(1, 1)]))]);
        save(&bad.join("attempt.json"), &attempt).unwrap();
        for path in [&good, &bad] {
            let run_path = path.parent().unwrap().parent().unwrap().join("run.json");
            let mut run: Run = load(&run_path).unwrap();
            run.sources
                .insert("other.rs".into(), "fn other() {}\n".into());
            save(&run_path, &run).unwrap();
        }
        fs::remove_file(bad.join("1-1.profraw")).unwrap();
        let mut partial = book.clone();
        populate(&mut partial, root).unwrap();
        let index = &partial.chapters().next().unwrap().content;
        assert!(file_card(map_html(index), "code.rs").contains("100.0%"));
        let unavailable = file_card(map_html(index), "other.rs");
        assert!(unavailable.contains("unavailable"));
        assert!(!unavailable.contains("not measured"));
        assert!(!unavailable.contains('%'));
        assert!(index.contains("1 unavailable test attempts"));
        // A stale selected attempt leaves the same unknown per-file state.
        let run_path = bad.parent().unwrap().parent().unwrap().join("run.json");
        let mut run: Run = load(&run_path).unwrap();
        run.inputs
            .insert("missing-build-input".into(), "old-hash".into());
        save(&run_path, &run).unwrap();
        let mut stale = book;
        populate(&mut stale, root).unwrap();
        let index = &stale.chapters().next().unwrap().content;
        assert!(file_card(map_html(index), "code.rs").contains("100.0%"));
        assert!(file_card(map_html(index), "other.rs").contains("unavailable"));
        assert!(index.contains("1 stale test attempts"));
    }

    // {test}`requirements.test-coverage.index.file_map`
    // {test}`requirements.test-coverage.index.file_map.table`
    #[test]
    fn file_map_escapes_paths_and_does_not_round_positive_coverage_to_zero() {
        let file = "src/<unsafe>\".rs".to_owned();
        let totals = (1..=2000)
            .map(|line| ((file.clone(), line), u64::from(line == 1)))
            .collect();
        let map = file_map(
            [file],
            &totals,
            MapAvailability::Complete,
            true,
            Path::new("coverage.md"),
            Path::new("coverage.md"),
        );
        assert!(map.contains("src/&lt;unsafe&gt;&quot;.rs"));
        assert!(!map.contains("<unsafe>"));
        assert!(map.contains("&lt;0.1%"));
        assert!(map.contains("1/2000 instrumented lines covered"));
        assert!(!map.contains("<strong>0.0%</strong>"));
        assert!(map.contains("background:hsl("));
        assert!(map.contains("role=\"meter\""));
        let levels = file_map(
            ["zero.rs", "half.rs", "full.rs"].map(String::from),
            &BTreeMap::from([
                (("zero.rs".into(), 1), 0),
                (("half.rs".into(), 1), 1),
                (("half.rs".into(), 2), 0),
                (("full.rs".into(), 1), 1),
            ]),
            MapAvailability::Complete,
            false,
            Path::new("coverage.md"),
            Path::new("coverage.md"),
        );
        for (file, percentage) in [("zero.rs", 0.0), ("half.rs", 50.0), ("full.rs", 100.0)] {
            let card = file_card(&levels, file);
            assert!(card.contains(&format!("aria-valuenow=\"{percentage:.6}\"")));
            assert!(card.contains(&format!("style=\"width:{percentage:.6}%")));
        }
        let empty = file_map(
            Vec::new(),
            &BTreeMap::new(),
            MapAvailability::Complete,
            false,
            Path::new("coverage.md"),
            Path::new("coverage.md"),
        );
        assert!(!empty.contains("data-file="));
    }

    fn directory_summary<'a>(map: &'a str, directory: &str) -> &'a str {
        map.split(&format!("data-directory=\"{directory}\""))
            .nth(1)
            .unwrap()
            .split("</summary>")
            .next()
            .unwrap()
    }

    // {test}`requirements.test-coverage.index.file_map.table`
    // {test}`requirements.test-coverage.index.file_map.directories`
    // {test}`requirements.test-coverage.index.file_map.directories.initial`
    // {test}`requirements.test-coverage.index.file_map.directories.data`
    // {test}`requirements.test-coverage.index.file_map.minimum`
    // {test}`requirements.test-coverage.index.file_map.minimum.statuses`
    // {test}`requirements.test-coverage.index.file_map.minimum.descendants`
    #[test]
    fn directory_tables_preserve_rows_inside_independent_initially_closed_controls() {
        let files = ["root.rs", "src/a.rs", "src/nested/b.rs", "src/nested/c.rs"];
        let totals = BTreeMap::from([
            (("root.rs".into(), 1), 1),
            (("src/a.rs".into(), 1), 1),
            (("src/a.rs".into(), 2), 0),
            (("src/nested/b.rs".into(), 1), 1),
            (("src/nested/b.rs".into(), 2), 0),
            (("src/nested/b.rs".into(), 3), 0),
            (("src/nested/b.rs".into(), 4), 0),
        ]);
        let map = file_map(
            files.map(String::from),
            &totals,
            MapAvailability::Complete,
            false,
            Path::new("coverage.md"),
            Path::new("coverage.md"),
        );
        for directory in ["src", "src/nested"] {
            let summary = directory_summary(&map, directory);
            assert!(summary.contains(&format!("<code>{directory}/</code>")));
            assert!(summary.contains("Minimum measured file coverage:"));
            assert!(summary.contains("25.0%"));
            assert!(summary.contains("1 not measured"));
            assert!(!summary.contains("unavailable"));
        }
        assert!(directory_summary(&map, "src").contains("3 files"));
        assert!(directory_summary(&map, "src/nested").contains("2 files"));

        // Native disclosures hide their body, retaining all table data. Check
        // semantic nesting rather than relying on directory text order alone.
        let tags = regex::Regex::new(r"<[^>]+>").unwrap();
        let mut parents = Vec::new();
        let mut rows = BTreeMap::new();
        for tag in tags.find_iter(map_html(&map)).map(|tag| tag.as_str()) {
            if tag.starts_with("<details ") {
                assert!(!tag.contains(" open"), "directory must start collapsed");
                let directory = tag
                    .split("data-directory=\"")
                    .nth(1)
                    .unwrap()
                    .split('"')
                    .next()
                    .unwrap();
                parents.push(directory);
            } else if tag == "</details>" {
                parents.pop().unwrap();
            } else if tag.starts_with("<tr class=\"habeas-coverage-file\"") {
                let file = tag
                    .split("data-file=\"")
                    .nth(1)
                    .unwrap()
                    .split('"')
                    .next()
                    .unwrap();
                assert!(rows.insert(file, parents.clone()).is_none());
            }
        }
        assert!(parents.is_empty());
        assert_eq!(rows["root.rs"], Vec::<&str>::new());
        assert_eq!(rows["src/a.rs"], ["src"]);
        for file in ["src/nested/b.rs", "src/nested/c.rs"] {
            assert_eq!(rows[file], ["src", "src/nested"]);
        }
        assert_eq!(rows.len(), files.len());
        for file in files {
            assert!(file_card(&map, file).contains(&format!("<code>{file}</code>")));
        }
        assert!(!map.contains("<script")); // toggling has no data-mutating handler
        for heading in ["File", "Coverage", "Covered / instrumented lines"] {
            assert!(map.contains(&format!("<th scope=\"col\">{heading}</th>")));
        }
        assert!(file_card(&map, "src/a.rs").contains(">1/2</td>"));
        assert!(file_card(&map, "src/nested/b.rs").contains(">1/4</td>"));
        let unknown = file_card(&map, "src/nested/c.rs");
        assert!(unknown.contains("not measured"));
        assert!(!unknown.contains('%'));
    }

    // {test}`requirements.test-coverage.index.file_map.directories`
    // {test}`requirements.test-coverage.index.file_map.directories.initial`
    #[test]
    fn directories_with_only_descendant_files_keep_distinct_full_path_controls() {
        let files = ["a/shared/deep/one.rs", "b/shared/two.rs"];
        let totals = files
            .iter()
            .map(|file| (((*file).to_owned(), 1), 1))
            .collect();
        let map = file_map(
            files.map(String::from),
            &totals,
            MapAvailability::Complete,
            false,
            Path::new("coverage.md"),
            Path::new("coverage.md"),
        );
        assert_eq!(map.matches("<details ").count(), 5);
        for directory in ["a", "a/shared", "a/shared/deep", "b", "b/shared"] {
            assert!(map.contains(&format!("data-directory=\"{directory}\"><summary>")));
            let summary = directory_summary(&map, directory);
            assert!(summary.contains(&format!("<code>{directory}/</code>")));
            assert!(summary.contains("100.0%"));
            assert!(!summary.contains(" open"));
        }
        assert_eq!(map.matches("data-file=").count(), 2);
    }

    // {test}`requirements.test-coverage.index.file_map.minimum`
    // {test}`requirements.test-coverage.index.file_map.minimum.statuses`
    #[test]
    fn directory_minima_use_exact_fractions_and_only_listed_measured_files() {
        let files = [
            "src/higher.rs",
            "src/lower.rs",
            "src/zero.rs",
            "src/unknown.rs",
        ];
        let totals = files[..3]
            .iter()
            .flat_map(|file| {
                let (covered, total) = match *file {
                    "src/higher.rs" => (501, 1000),
                    "src/lower.rs" => (1001, 2000),
                    _ => (0, 10),
                };
                (1..=total)
                    .map(move |line| (((*file).to_owned(), line), u64::from(line <= covered)))
            })
            .collect();
        let render = |availability, positive_only| {
            file_map(
                files.map(String::from),
                &totals,
                availability,
                positive_only,
                Path::new("coverage.md"),
                Path::new("coverage.md"),
            )
        };
        let positive = render(MapAvailability::Complete, true);
        let summary = directory_summary(&positive, "src");
        assert!(summary.contains("data-min-covered=\"1001\" data-min-instrumented=\"2000\""));
        assert!(summary.contains("2 files"));
        assert!(!positive.contains("data-file=\"src/zero.rs\""));
        assert!(!positive.contains("data-file=\"src/unknown.rs\""));
        assert!(!summary.contains("not measured"));
        let complete = render(MapAvailability::Complete, false);
        assert!(directory_summary(&complete, "src").contains("0.0%"));
        assert!(directory_summary(&complete, "src").contains("1 not measured"));
        let partial = render(MapAvailability::Partial, false);
        assert!(directory_summary(&partial, "src").contains("0.0%"));
        assert!(directory_summary(&partial, "src").contains("1 unavailable"));
        let unavailable = render(MapAvailability::Unavailable, false);
        let summary = directory_summary(&unavailable, "src");
        assert!(summary.contains("4 unavailable"));
        assert!(summary.contains("No measured files"));
        assert!(!summary.contains('%'));
        assert!(!summary.contains("data-min-"));
        let unknown = file_map(
            ["src/unknown.rs".into()],
            &BTreeMap::new(),
            MapAvailability::Complete,
            false,
            Path::new("coverage.md"),
            Path::new("coverage.md"),
        );
        let summary = directory_summary(&unknown, "src");
        assert!(summary.contains("1 not measured"));
        assert!(!summary.contains('%'));
    }

    // {test}`requirements.test-coverage.index.file_map.notes`
    // {test}`requirements.test-coverage.index.file_map.notes.current`
    // {test}`requirements.test-coverage.index.file_map.notes.render_only`
    #[test]
    fn combined_and_historical_file_links_share_current_source_notes_without_collecting() {
        let (temp, mut book) = fixture();
        let root = temp.path();
        fs::create_dir(root.join("md")).unwrap();
        fs::write(root.join("book.toml"), "[book]\nsrc='md'\n").unwrap();
        fs::write(root.join("md/SUMMARY.md"), "# Summary\n").unwrap();
        fs::create_dir_all(root.join("src/nested")).unwrap();
        for file in ["src/a.rs", "src/nested/a.rs"] {
            fs::write(root.join(file), "fn another() {}\n").unwrap();
        }
        let attempt = record(root, "one", 1, "a", &[(1, 1)]);
        let run_path = attempt.parent().unwrap().parent().unwrap().join("run.json");
        let mut run: Run = load(&run_path).unwrap();
        let mut process: Attempt = load(&attempt.join("attempt.json")).unwrap();
        for file in ["src/a.rs", "src/nested/a.rs"] {
            run.sources
                .insert(file.into(), fs::read_to_string(root.join(file)).unwrap());
            process.processes[0]
                .lines
                .insert(file.into(), BTreeMap::from([(1, 1)]));
        }
        save(&run_path, &run).unwrap();
        save(&attempt.join("attempt.json"), &process).unwrap();
        let captured = fs::read(attempt.join("attempt.json")).unwrap();
        fs::write(root.join("code.rs"),
            "// :::{cite} habeas:coverage\n// Current explanation, added after capture.\n// :::\nfn current() {}\n").unwrap();
        populate(&mut book, root).unwrap();
        let index = book.chapters().next().unwrap();
        let notes_path = super::super::notes::path(index.path.as_ref().unwrap(), "code.rs");
        let notes = book
            .chapters()
            .find(|chapter| chapter.path.as_ref() == Some(&notes_path))
            .unwrap();
        assert!(
            notes
                .content
                .contains("Current explanation, added after capture.")
        );
        assert!(notes.content.contains("Current workspace source notes"));
        assert!(notes.content.contains("code.rs"));
        assert_eq!(
            book.chapters()
                .filter(|chapter| chapter.path.as_ref() == Some(&notes_path))
                .count(),
            1
        );
        let historical = book
            .chapters()
            .find(|chapter| chapter.content.starts_with("# Test process coverage"))
            .unwrap();
        let mut distinct = BTreeSet::new();
        for file in ["code.rs", "src/a.rs", "src/nested/a.rs"] {
            let path = super::super::notes::path(index.path.as_ref().unwrap(), file);
            assert!(
                distinct.insert(path.clone()),
                "files must have distinct views"
            );
            let notes = book
                .chapters()
                .find(|chapter| chapter.path.as_ref() == Some(&path))
                .unwrap();
            assert!(
                notes
                    .content
                    .contains(&format!("File: <code>{file}</code>"))
            );
            for chapter in [index, historical] {
                let href = crate::chapter_href(chapter.path.as_ref().unwrap(), &path, None);
                assert!(
                    file_card(map_html(&chapter.content), file)
                        .contains(&format!("href=\"{href}\""))
                );
            }
        }
        assert_eq!(fs::read(attempt.join("attempt.json")).unwrap(), captured);
        assert_eq!(
            fs::read_to_string(attempt.join("1-1.profraw")).unwrap(),
            "retained profile fixture"
        );
        assert!(!root.join("evidence").exists());
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

    // {test}`requirements.test-coverage.index.selection`
    #[test]
    fn selection_distinguishes_binaries_and_selects_latest_attempts_within_the_active_build() {
        let (temp, mut book) = fixture();
        for (binary, started, counts) in [
            ("first", 1, [(1, 1), (2, 0)]),
            ("second", 2, [(1, 0), (2, 1)]),
        ] {
            let dir = record(temp.path(), "same", started, "build-a", &counts);
            let path = dir.join("attempt.json");
            let mut attempt: Attempt = load(&path).unwrap();
            attempt.binary = binary.into();
            save(&path, &attempt).unwrap();
        }
        populate(&mut book, temp.path()).unwrap();
        let index = &book.chapters().next().unwrap().content;
        assert!(index.contains("2/2 (100.0%)"));
        assert!(index.contains("first / same"));
        assert!(index.contains("second / same"));

        let (temp, mut book) = fixture();
        // Write newest first; run-start and UUID ordering deliberately disagree
        // with attempt-start ordering.
        for (test, start, config, counts) in [
            ("two", 40, "build-a", [(2, 1)]),
            ("one", 30, "build-a", [(1, 1)]),
            ("one", 35, "build-b", [(1, 0)]),
            ("two", 10, "build-a", [(2, 0)]),
        ] {
            let attempt = record(temp.path(), test, start, config, &counts);
            let dir = attempt.parent().unwrap().parent().unwrap();
            let path = dir.join("run.json");
            let mut run: Run = load(&path).unwrap();
            run.started = if config == "build-b" { 1000 } else { start };
            run.id = uuid::Uuid::from_u128(100 - start).to_string();
            save(&path, &run).unwrap();
            fs::rename(dir, dir.parent().unwrap().join(&run.id)).unwrap();
        }
        populate(&mut book, temp.path()).unwrap();
        let index = &book.chapters().next().unwrap().content;
        assert!(index.contains("2/2 (100.0%)"));
        assert!(index.contains("binary / one"));
        assert!(index.contains("binary / two"));
        assert!(index.contains("excluded: historical build configuration"));

        // A retry in the same run must also use attempt-start ordering.
        let (temp, mut book) = fixture();
        let newest = record(temp.path(), "one", 20, "build-a", &[(1, 1)]);
        let mut older: Attempt = load(&newest.join("attempt.json")).unwrap();
        older.started = 10;
        older.id = uuid::Uuid::from_u128(u128::MAX).to_string();
        older.attempt = "older".into();
        older.processes[0].lines = BTreeMap::from([("code.rs".into(), BTreeMap::from([(1, 0)]))]);
        let older_dir = newest.parent().unwrap().join(&older.id);
        fs::create_dir(&older_dir).unwrap();
        fs::copy(newest.join("1-1.profraw"), older_dir.join("1-1.profraw")).unwrap();
        save(&older_dir.join("attempt.json"), &older).unwrap();
        populate(&mut book, temp.path()).unwrap();
        assert!(
            book.chapters()
                .next()
                .unwrap()
                .content
                .contains("1/1 (100.0%)")
        );
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
        attempt.diagnostics.push("profile missing".into());
        for outcome in ["failed", "interrupted"] {
            attempt.outcome = outcome.into();
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
        }
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
            functions: None,
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
        assert_eq!(index.sub_items.len(), 5); // attempts, captured sources and current file notes
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
        // Current file-note pages are generated reports too, even without runs.
        let (temp, mut book) = fixture();
        book.push_item(Chapter::new(
            "Notes collision",
            "".into(),
            super::super::notes::path(Path::new("reports/coverage.md"), "code.rs"),
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

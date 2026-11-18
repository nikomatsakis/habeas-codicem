use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mdbook_preprocessor::book::{Book, BookItem, Chapter, SectionNumber};

use super::{Attempt, Run, fresh, load, target_directory, workspace_root};

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

pub(super) struct Record {
    pub(super) run: Arc<Run>,
    pub(super) attempt: Attempt,
    pub(super) fresh: bool,
    attempt_dir: PathBuf,
}

impl Record {
    // {cite}`habeas:util`
    fn load_measurements(&mut self) -> io::Result<()> {
        let mut attempt: Attempt = load(&self.attempt_dir.join("attempt.json"))?;
        // Do not substitute another attempt if a collector replaced the file
        // between selection and loading.
        if attempt.id != self.attempt.id
            || attempt.binary != self.attempt.binary
            || attempt.test != self.attempt.test
            || attempt.started != self.attempt.started
        {
            return Err(io::Error::other(
                "Attempt changed during coverage selection.",
            ));
        }
        for process in &mut attempt.processes {
            if process.profiles.is_empty()
                || process.profiles.iter().any(|profile| {
                    fs::File::open(self.attempt_dir.join(profile))
                        .and_then(|file| file.metadata())
                        .map_or(true, |metadata| metadata.len() == 0)
                })
            {
                process.diagnostic =
                    Some("A retained raw profile is missing, empty or unreadable.".into());
            }
            if process
                .lines
                .keys()
                .any(|file| !self.run.sources.contains_key(file))
                || process
                    .functions
                    .iter()
                    .flatten()
                    .any(|function| !self.run.sources.contains_key(&function.file))
            {
                process.diagnostic =
                    Some("Matching captured source is missing; coverage is unavailable.".into());
            }
            if process.lines.values().any(|lines| lines.contains_key(&0))
                || process
                    .functions
                    .iter()
                    .flatten()
                    .any(|function| function.line == 0 || function.column == 0)
            {
                process.diagnostic = Some("Corrupt normalized coverage locations.".into());
            }
        }
        self.attempt = attempt;
        Ok(())
    }
}

// Validate the full JSON schema while discarding measurements during selection.
// Unlike IgnoredAny, these visitors still reject malformed counts and locations
// in historical attempts, without allocating a tree entry for every line.
// {cite}`habeas:util`
struct DiscardMap<K, V>(std::marker::PhantomData<(K, V)>);

impl<'de, K: serde::Deserialize<'de>, V: serde::Deserialize<'de>> serde::Deserialize<'de>
    for DiscardMap<K, V>
{
    // {cite}`habeas:util`
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct MapVisitor<K, V>(std::marker::PhantomData<(K, V)>);
        impl<'de, K: serde::Deserialize<'de>, V: serde::Deserialize<'de>> serde::de::Visitor<'de>
            for MapVisitor<K, V>
        {
            type Value = DiscardMap<K, V>;

            // {cite}`habeas:util`
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a coverage measurement map")
            }

            // {cite}`habeas:util`
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                while map.next_entry::<K, V>()?.is_some() {}
                Ok(DiscardMap(std::marker::PhantomData))
            }
        }
        deserializer.deserialize_map(MapVisitor(std::marker::PhantomData))
    }
}

// {cite}`habeas:util`
struct DiscardSeq<T>(std::marker::PhantomData<T>);

impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for DiscardSeq<T> {
    // {cite}`habeas:util`
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SeqVisitor<T>(std::marker::PhantomData<T>);
        impl<'de, T: serde::Deserialize<'de>> serde::de::Visitor<'de> for SeqVisitor<T> {
            type Value = DiscardSeq<T>;

            // {cite}`habeas:util`
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a coverage function list")
            }

            // {cite}`habeas:util`
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                while seq.next_element::<T>()?.is_some() {}
                Ok(DiscardSeq(std::marker::PhantomData))
            }
        }
        deserializer.deserialize_seq(SeqVisitor(std::marker::PhantomData))
    }
}

type AttemptMetadata = Attempt<
    super::Process<DiscardMap<String, DiscardMap<u32, u64>>, DiscardSeq<super::FunctionExecution>>,
>;

impl AttemptMetadata {
    // {cite}`habeas:util`
    fn metadata(self) -> Attempt {
        Attempt {
            id: self.id,
            binary: self.binary,
            test: self.test,
            attempt: self.attempt,
            started: self.started,
            pid: self.pid,
            command: self.command,
            outcome: self.outcome,
            processes: Vec::new(),
            diagnostics: self.diagnostics,
        }
    }
}

// {cite}`habeas:util`
#[derive(serde::Serialize, serde::Deserialize)]
struct CachedAttempt {
    stamp: Vec<String>,
    metadata: Result<Attempt, String>,
}

// {cite}`habeas:util`
fn attempt_stamp(path: &Path, now: std::time::SystemTime) -> io::Result<Vec<String>> {
    let metadata = fs::metadata(path)?;
    // Recent writes are reread, including on filesystems whose timestamps have
    // coarse resolution. Unix change time also detects in-place edits that
    // restore the original modification time.
    if now.duration_since(metadata.modified()?).unwrap_or_default()
        < std::time::Duration::from_secs(2)
    {
        return Err(io::Error::other("Attempt is still being written."));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let age = now
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as i128
            - (metadata.ctime() as i128 * 1_000_000_000 + metadata.ctime_nsec() as i128);
        if age < 2_000_000_000 {
            return Err(io::Error::other("Attempt was recently changed."));
        }
        Ok(vec![
            metadata.dev().to_string(),
            metadata.ino().to_string(),
            metadata.len().to_string(),
            metadata.mtime().to_string(),
            metadata.mtime_nsec().to_string(),
            metadata.ctime().to_string(),
            metadata.ctime_nsec().to_string(),
        ])
    }
    #[cfg(not(unix))]
    {
        Err(io::Error::other(
            "Reliable file change stamps are unavailable.",
        ))
    }
}

// {cite}`habeas:util`
fn load_metadata(
    path: &Path,
    cache: &mut BTreeMap<PathBuf, CachedAttempt>,
    changed: &mut bool,
    now: std::time::SystemTime,
) -> io::Result<Attempt> {
    let stamp = attempt_stamp(path, now).ok();
    if let Some(entry) = cache.get(path)
        && stamp.as_ref() == Some(&entry.stamp)
    {
        return entry.metadata.clone().map_err(io::Error::other);
    }
    let metadata = load::<AttemptMetadata>(path)
        .map(AttemptMetadata::metadata)
        .map_err(|error| error.to_string());
    if let Some(stamp) = stamp {
        cache.insert(
            path.to_path_buf(),
            CachedAttempt {
                stamp,
                metadata: metadata.clone(),
            },
        );
        *changed = true;
    } else if cache.remove(path).is_some() {
        *changed = true;
    }
    metadata.map_err(io::Error::other)
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
    let mut files: BTreeMap<&str, BTreeMap<u32, u64>> = BTreeMap::new();
    for record in records {
        for process in &record.attempt.processes {
            for (file, lines) in &process.lines {
                let totals = files.entry(file).or_default();
                for (&line, &count) in lines {
                    totals
                        .entry(line)
                        .and_modify(|current| *current = (*current).max(count))
                        .or_insert(count);
                }
            }
        }
    }
    files
        .into_iter()
        .flat_map(|(file, lines)| {
            lines
                .into_iter()
                .map(move |(line, count)| ((file.to_owned(), line), count))
        })
        .collect()
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

// {cite}`habeas:util`
#[derive(Default)]
struct GapDirectory<'a> {
    files: Vec<&'a super::check::FilePolicy>,
    directories: BTreeMap<String, GapDirectory<'a>>,
}

impl GapDirectory<'_> {
    // {cite}`habeas:util`
    fn count(&self) -> usize {
        self.files
            .iter()
            .flat_map(|file| &file.obligations)
            .filter(|gap| !gap.accepted())
            .count()
            + self.directories.values().map(Self::count).sum::<usize>()
    }

    fn render(
        &self,
        html: &mut String,
        folder: &str,
        policy: &super::check::PolicyReport,
        index: &Path,
        next: &mut usize,
    ) {
        if !self.files.is_empty() {
            html.push_str("<table class=\"gap-file-table\"><colgroup><col><col><col><col></colgroup><thead><tr><th>File</th><th>Coverage</th><th>Uncovered obligations</th><th>Explanation</th></tr></thead><tbody>\n");
            for file in &self.files {
                let id = format!("habeas-gap-file-{}", *next);
                *next += 1;
                let gaps: Vec<_> = file
                    .obligations
                    .iter()
                    .filter(|gap| !gap.accepted())
                    .collect();
                let threshold = gaps.iter().any(|gap| gap.selector.is_empty());
                let functions = gaps.len() - usize::from(threshold);
                let undocumented = gaps.iter().filter(|gap| gap.exception.is_none()).count();
                let description = match (threshold, functions) {
                    (true, 0) => "file below target".to_owned(),
                    (true, n) => format!("file + {n} functions"),
                    (false, 1) => "1 function".to_owned(),
                    (false, n) => format!("{n} functions"),
                };
                let explanation = if undocumented == gaps.len() {
                    "Missing"
                } else {
                    "Review needed"
                };
                let percentage = if file.total == 0 {
                    "not measured".to_owned()
                } else {
                    percentage_label(file.covered, file.total)
                };
                let indicator = if file.total == 0 {
                    String::new()
                } else {
                    format!(
                        "<span class=\"gap-coverage-bar\" aria-hidden=\"true\" style=\"--gap-covered: {:.6}%\"></span>",
                        100.0 * file.covered as f64 / file.total as f64
                    )
                };
                html.push_str(&format!("<tr class=\"gap-file-row habeas-coverage-file\" data-file=\"{}\" data-status=\"{}\"><td><button class=\"gap-file-button\" aria-expanded=\"false\" aria-controls=\"{id}\" onclick=\"const row=document.getElementById(this.getAttribute('aria-controls'));row.hidden=!row.hidden;this.setAttribute('aria-expanded',String(!row.hidden));this.querySelector('.gap-chevron').textContent=row.hidden?'▸':'▾'\"><span class=\"gap-chevron\" aria-hidden=\"true\">▸</span><code>{}</code></button></td><td class=\"gap-percentage\">{percentage}{indicator}</td><td>{description}</td><td class=\"gap-unresolved\">{explanation}</td></tr>\n", escaped(&file.file), if file.total == 0 { "not measured" } else { "measured" }, escaped(&file.file)));
                let notes =
                    crate::chapter_href(index, &super::notes::path(index, &file.file), None);
                html.push_str(&format!("<tr class=\"gap-detail-row\" id=\"{id}\" hidden><td colspan=\"4\"><p class=\"gap-detail-title\"><code>{}</code> · {}/{} instrumented lines covered · <a href=\"{}\">Current source notes</a></p><table class=\"gap-obligation-table\"><thead><tr><th>Source item</th><th>Measurement</th><th>Explanation</th></tr></thead><tbody>", escaped(&file.file), file.covered, file.total, escaped(&notes)));
                for gap in gaps {
                    html.push_str(&format!("<tr><td><code>{}</code><br><span class=\"gap-meta\">{}:{}</span></td><td>{}</td><td>{}</td></tr>\n", escaped(if gap.selector.is_empty() { "File root" } else { &gap.selector }), escaped(&file.file), gap.line, escaped(&gap.measurement), explanation_html(gap)));
                }
                html.push_str("</tbody></table></td></tr>\n");
            }
            html.push_str("</tbody></table>\n");
        }
        for (name, directory) in &self.directories {
            let path = if folder.is_empty() {
                name.clone()
            } else {
                format!("{folder}/{name}")
            };
            let prefix = format!("{path}/");
            let minimum = policy
                .files
                .iter()
                .filter(|file| file.file.starts_with(&prefix) && file.total > 0)
                .min_by(|a, b| {
                    ((a.covered as u128) * (b.total as u128))
                        .cmp(&((b.covered as u128) * (a.total as u128)))
                });
            let min = minimum.map_or_else(|| "No measured files".to_owned(), |file| format!("Minimum measured file coverage: <strong data-min-covered=\"{}\" data-min-instrumented=\"{}\">{}</strong>", file.covered, file.total, percentage_label(file.covered, file.total)));
            let not_measured = policy
                .files
                .iter()
                .filter(|file| {
                    file.file.starts_with(&prefix)
                        && file.total == 0
                        && file.obligations.iter().any(|gap| !gap.accepted())
                })
                .count();
            let counts = if policy.counts_complete {
                format!("{} uncovered obligations", directory.count())
            } else if policy.measured {
                format!(
                    "Gap counts incomplete: {} established uncovered obligations",
                    directory.count()
                )
            } else {
                "Gap counts unavailable".to_owned()
            };
            html.push_str(&format!("<details class=\"gap-directory\" data-directory=\"{}\" open><summary><code>{}/</code><span class=\"gap-meta\">{min} · {counts}{} </span></summary>\n", escaped(&path), escaped(&path), if not_measured > 0 { format!(" · {not_measured} not measured") } else { String::new() }));
            directory.render(html, &path, policy, index, next);
            html.push_str("</details>\n");
        }
    }
}

// {impl}`requirements.test-coverage.index.gaps.explanations`
fn explanation_html(gap: &super::check::Obligation) -> String {
    let Some(exception) = &gap.exception else {
        return "<span class=\"gap-unresolved\">Missing habeas:coverage proposal</span>".into();
    };
    let state = if exception.accepted {
        "Current and accepted"
    } else if exception.review_missing {
        "Review missing"
    } else {
        "Review not accepted"
    };
    let mut html = format!("<details class=\"gap-explanation\"><summary>{state}</summary>");
    if exception.justification.trim().is_empty() {
        html.push_str("<p>No tracked justification is present.</p>");
    } else {
        html.push_str(&format!(
            "<pre>{}</pre>",
            escaped(&crate::redaction::redact_text(&exception.justification))
        ));
    }
    for finding in &exception.findings {
        html.push_str(&format!("<p>{}</p>", escaped(finding)));
    }
    html.push_str("</details>");
    html
}

// {impl}`requirements.test-coverage.index.file_map.combined`
// {impl}`requirements.test-coverage.index.file_map.combined.measurement`
// {impl}`requirements.test-coverage.index.file_map.table`
// {impl}`requirements.test-coverage.index.file_map.directories`
// {impl}`requirements.test-coverage.index.file_map.directories.initial`
// {impl}`requirements.test-coverage.index.file_map.directories.data`
// {impl}`requirements.test-coverage.index.file_map.minimum`
// {impl}`requirements.test-coverage.index.file_map.minimum.statuses`
// {impl}`requirements.test-coverage.index.gaps`
// {impl}`requirements.test-coverage.index.gaps.summary`
// {impl}`requirements.test-coverage.index.gaps.details`
// {impl}`requirements.test-coverage.index.gaps.details.initial`
// {impl}`requirements.test-coverage.index.gaps.accepted`
// {impl}`requirements.test-coverage.index.file_map.notes`
fn gaps_map(policy: &super::check::PolicyReport, index: &Path, attempts: usize) -> String {
    let mut html = format!(
        "<style>{}</style>\n<section class=\"habeas-coverage-map habeas-coverage-gaps\" aria-label=\"Coverage gaps\">\n<p class=\"gap-meta\">Combined coverage · {attempts} selected test attempts</p><p class=\"gap-policy gap-meta\">File target: 80% of instrumented lines. Every function needs execution or an accepted explanation.</p>\n",
        include_str!("../../assets/coverage-gaps.css")
    );
    let unresolved: Vec<_> = policy
        .files
        .iter()
        .flat_map(|file| &file.obligations)
        .filter(|gap| !gap.accepted())
        .collect();
    let files = unresolved
        .iter()
        .filter(|gap| gap.selector.is_empty())
        .count();
    let functions = unresolved.len() - files;
    html.push_str(&format!(
        "<div class=\"gap-verdict\" role=\"status\"><strong>CI would {}</strong>",
        if policy.report.passed { "pass" } else { "fail" }
    ));
    if policy.counts_complete {
        html.push_str(&format!("<span>{} unresolved gaps</span><span class=\"gap-meta\">{files} file obligations · {functions} function obligations</span>", unresolved.len()));
    } else if policy.measured {
        html.push_str(&format!(
            "<span>Gap counts incomplete: {} established unresolved gaps</span>",
            unresolved.len()
        ));
    } else {
        html.push_str("<span>Coverage unavailable · gap counts unavailable</span>");
    }
    html.push_str("</div><p class=\"gap-section-label\">Unresolved coverage gaps</p>\n");
    let mut directory = GapDirectory::default();
    for file in &policy.files {
        if !file.obligations.iter().any(|gap| !gap.accepted()) {
            continue;
        }
        let mut dir = &mut directory;
        if let Some((folder, _)) = file.file.rsplit_once('/') {
            for part in folder.split('/') {
                dir = dir.directories.entry(part.into()).or_default();
            }
        }
        dir.files.push(file);
    }
    directory.render(&mut html, "", policy, index, &mut 0);
    if unresolved.is_empty() && policy.counts_complete {
        html.push_str("<p>No unsatisfied file or function obligations.</p>\n");
    }
    let accepted = policy
        .files
        .iter()
        .flat_map(|file| {
            file.obligations
                .iter()
                .filter(|gap| gap.accepted())
                .map(move |gap| (file, gap))
        })
        .collect::<Vec<_>>();
    html.push_str(&format!("<details class=\"gap-accepted-group\"><summary>Documented gaps — {} accepted exceptions</summary><p class=\"gap-meta\">Current workspace source notes. Accepted exceptions do not increase measured percentages.</p>\n", accepted.len()));
    for (file, gap) in accepted {
        let notes = crate::chapter_href(index, &super::notes::path(index, &file.file), None);
        html.push_str(&format!("<div class=\"gap-accepted-item\"><p><a href=\"{}\"><code>{}:{} #{}</code></a> · {} · <span class=\"gap-accepted-state\">current and accepted</span></p>{}</div>\n", escaped(&notes), escaped(&file.file), gap.line, escaped(if gap.selector.is_empty() { "File root" } else { &gap.selector }), escaped(&gap.measurement), explanation_html(gap)));
    }
    html.push_str("</details>\n");
    if !policy.findings.is_empty() {
        html.push_str(
            "<details class=\"gap-footer\"><summary>Coverage-check findings</summary><ul>",
        );
        for finding in &policy.findings {
            html.push_str(&format!("<li>{}</li>", escaped(finding)));
        }
        html.push_str("</ul></details>\n");
    }
    html.push_str("<p class=\"gap-footer gap-meta\">Coverage uses the latest compatible attempt for each test, including filtered runs. This does not establish that all current tests were measured. Saved measurements and current exception reviews determine the CI verdict.</p>\n</section>\n\n");
    html
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
// {impl}`requirements.test-coverage.index.file_map.combined.details`
pub(crate) fn populate(book: &mut Book, project: &Path, parent: &Path) -> io::Result<()> {
    populate_in(book, project, None, parent)
}

// {cite}`habeas:util`
fn reserve_path(
    existing: &BTreeSet<PathBuf>,
    generated: &mut BTreeSet<PathBuf>,
    path: &Path,
    kind: &str,
) -> io::Result<()> {
    if existing.contains(path) || !generated.insert(path.to_owned()) {
        return Err(io::Error::other(format!(
            "coverage {kind} path conflicts with an existing chapter: {}",
            path.display()
        )));
    }
    Ok(())
}

// {cite}`habeas:util`
pub(super) struct SavedCoverage {
    pub(super) records: Vec<Record>,
    pub(super) selected: BTreeSet<usize>,
    pub(super) diagnostics: Vec<String>,
    pub(super) uncertain_history: bool,
    current_files: BTreeSet<String>,
    pub(super) discovery_failed: bool,
}

// {impl}`requirements.test-coverage.index.selection`
// {impl}`requirements.test-coverage.index.freshness`
// {impl}`requirements.test-coverage.check.measurement`
pub(super) fn load_saved(project: &Path, target: Option<PathBuf>) -> SavedCoverage {
    let mut diagnostics = Vec::new();
    let mut uncertain_history = false;
    let mut current_files = BTreeSet::new();
    let mut discovery_failed = false;
    let mut records = Vec::new();
    let workspace = workspace_root(project);
    if let Ok(root) = &workspace {
        let mut errors = Vec::new();
        let discovered = crate::evidence::discovered_files(root, &mut errors);
        discovery_failed = !errors.is_empty();
        diagnostics.extend(errors);
        current_files.extend(
            crate::rust_coverage::rust_files(root, &discovered)
                .into_iter()
                .filter_map(|file| {
                    file.strip_prefix(root)
                        .ok()
                        .map(|relative| relative.to_string_lossy().into_owned())
                }),
        );
    }
    let root_and_target = workspace.and_then(|root| {
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
                let cache_path = storage.join("selection-cache.json");
                let mut metadata_cache =
                    load::<BTreeMap<PathBuf, CachedAttempt>>(&cache_path).unwrap_or_default();
                let mut cache_changed = false;
                let mut seen = BTreeSet::new();
                let entries = match fs::read_dir(&storage) {
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
                        let attempt_path = attempt_dir.join("attempt.json");
                        seen.insert(attempt_path.clone());
                        let attempt: Attempt = match load_metadata(
                            &attempt_path,
                            &mut metadata_cache,
                            &mut cache_changed,
                            std::time::SystemTime::now(),
                        ) {
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
                        records.push(Record {
                            run: Arc::clone(&run),
                            attempt,
                            fresh: current,
                            attempt_dir,
                        });
                    }
                }
                let before = metadata_cache.len();
                metadata_cache.retain(|path, _| seen.contains(path));
                if cache_changed || before != metadata_cache.len() {
                    // This cache is disposable and optional: a read-only target
                    // directory still gets the same validated coverage result.
                    let _ = super::save(&cache_path, &metadata_cache);
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
    for &index in &selected {
        let record = &mut records[index];
        if let Err(error) = record.load_measurements() {
            uncertain_history = true;
            diagnostics.push(format!(
                "{}: unavailable attempt data: {error}",
                record.attempt_dir.display()
            ));
        }
    }
    SavedCoverage {
        records,
        selected,
        diagnostics,
        uncertain_history,
        current_files,
        discovery_failed,
    }
}

// {impl}`requirements.test-coverage.index.navigation`
// {impl}`requirements.test-coverage.index.source`
// {impl}`requirements.test-coverage.index.guidance`
// {impl}`requirements.test-coverage.index.summary`
// {impl}`requirements.test-coverage.index.details`
// {impl}`requirements.test-coverage.index.file_map.test`
// {impl}`requirements.test-coverage.index.file_map.combined.details`
pub(super) fn populate_in(
    book: &mut Book,
    project: &Path,
    target: Option<PathBuf>,
    parent: &Path,
) -> io::Result<()> {
    let index_path = parent.with_extension("").join("coverage.md");
    let saved = load_saved(project, target);
    let root = workspace_root(project).unwrap_or_else(|_| project.to_path_buf());
    let policy = super::check::check_saved(&root, &saved)?;
    let SavedCoverage {
        records,
        selected,
        diagnostics,
        uncertain_history,
        current_files,
        discovery_failed: _,
    } = saved;
    let note_files = if policy.measured {
        policy
            .files
            .iter()
            .filter(|file| !file.obligations.is_empty())
            .map(|file| file.file.clone())
            .collect()
    } else {
        current_files
    };
    let mut summary = String::from("# Coverage gaps\n\n");
    summary.push_str(&gaps_map(&policy, &index_path, selected.len()));
    if policy.measured {
        let totals = union(selected.iter().map(|&index| &records[index]));
        let covered = totals.values().filter(|&&count| count > 0).count();
        summary.push_str(&format!("Line coverage: **{covered}/{} ({:.1}%)** from the union of instrumented file-and-line locations.\n\n", totals.len(), 100.0 * covered as f64 / totals.len() as f64));
    }
    if uncertain_history {
        summary.push_str("Coverage history is incomplete: cannot establish latest-per-test selection. Combined totals are unavailable until the affected build artifacts are repaired or removed.\n\n");
    }
    if records.is_empty() {
        summary.push_str("No test coverage is available. Run `habeas coverage` (or `cargo habeas coverage` in this repository), then rebuild the book.\n\n");
    }
    let stale = selected
        .iter()
        .filter(|&&index| !records[index].fresh)
        .count();
    let incomplete = selected
        .iter()
        .filter(|&&index| records[index].fresh && !readable(&records[index]))
        .count();
    let historical = records.last().map_or(0, |latest| {
        records
            .iter()
            .filter(|record| record.run.configuration != latest.run.configuration)
            .count()
    });
    if stale > 0 || incomplete > 0 || historical > 0 {
        summary.push_str(&format!(
            "**Excluded coverage:** {stale} stale test attempts; {incomplete} unavailable test attempts; {historical} attempts from a historical build configuration.\n\n"
        ));
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
    reserve_path(&existing, &mut generated, &index_path, "index")?;
    let mut details = Vec::new();
    let notes_root = workspace_root(project).unwrap_or_else(|_| project.to_path_buf());
    for chapter in super::notes::pages(&notes_root, &note_files, &index_path) {
        let path = chapter
            .path
            .as_ref()
            .expect("file notes have a render path");
        reserve_path(&existing, &mut generated, path, "report")?;
        details.push(BookItem::Chapter(chapter));
    }
    summary.push_str("## Collect coverage and check CI policy\n\n");
    summary.push_str(super::GUIDANCE);
    summary.push('\n');
    let mut coverage = Some(Chapter::new("Coverage gaps", summary, &index_path, vec![]));
    let mut found = false;
    book.for_each_chapter_mut(|chapter| {
        if chapter.path.as_deref() == Some(parent) {
            found = true;
            chapter.content.push_str(&format!(
                "\n\n[Coverage gaps]({}) — uncovered obligations and their explanations.\n",
                crate::chapter_href(parent, &index_path, None)
            ));
            let mut page = coverage.take().expect("requirements index occurs once");
            page.source_path = None;
            page.parent_names = chapter.parent_names.clone();
            page.parent_names.push(chapter.name.clone());
            page.number = chapter.number.as_ref().map(|number| {
                let mut parts = number.to_vec();
                parts.push(1);
                SectionNumber::new(parts)
            });
            for (offset, item) in details.iter_mut().enumerate() {
                let BookItem::Chapter(child) = item else {
                    unreachable!()
                };
                child.parent_names = page.parent_names.clone();
                child.parent_names.push(page.name.clone());
                child.number = page.number.as_ref().map(|number| {
                    let mut parts = number.to_vec();
                    parts.push(offset as u32 + 1);
                    SectionNumber::new(parts)
                });
            }
            page.sub_items.append(&mut details);
            fn renumber(chapter: &mut Chapter, number: Vec<u32>) {
                chapter.number = Some(SectionNumber::new(number.clone()));
                let mut next = 1;
                for item in &mut chapter.sub_items {
                    if let BookItem::Chapter(child) = item {
                        let mut parts = number.clone();
                        parts.push(next);
                        renumber(child, parts);
                        next += 1;
                    }
                }
            }
            if let Some(number) = &chapter.number {
                let mut next = 2;
                for item in &mut chapter.sub_items {
                    if let BookItem::Chapter(child) = item {
                        let mut parts = number.to_vec();
                        parts.push(next);
                        renumber(child, parts);
                        next += 1;
                    }
                }
            }
            chapter.sub_items.insert(0, BookItem::Chapter(page));
        }
    });
    if !found {
        return Err(io::Error::other(
            "requirements index chapter disappeared before coverage generation",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{Process, save};
    use super::*;

    // Test adapter: expose the generated coverage subtree as the root for
    // report-content fixtures. The navigation test exercises the full book.
    fn populate(book: &mut Book, root: &Path) -> io::Result<()> {
        if !root.join("book.toml").exists() {
            fs::create_dir_all(root.join("md"))?;
            fs::write(root.join("book.toml"), "[book]\nsrc='md'\n")?;
            fs::write(root.join("md/SUMMARY.md"), "# Summary\n")?;
        }
        let BookItem::Chapter(parent) = &mut book.items[0] else {
            unreachable!()
        };
        parent.path = Some("reports.md".into());
        super::populate_in(
            book,
            root,
            Some(root.join("target")),
            Path::new("reports.md"),
        )?;
        let BookItem::Chapter(parent) = &mut book.items[0] else {
            unreachable!()
        };
        let coverage = parent.sub_items.remove(0);
        book.items[0] = coverage;
        Ok(())
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
        let attempt: Attempt = Attempt {
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
                functions: Some(
                    counts
                        .iter()
                        .map(|&(line, count)| super::super::FunctionExecution {
                            file: "code.rs".into(),
                            line,
                            column: 1,
                            count,
                        })
                        .collect(),
                ),
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
            .split("<section class=\"habeas-coverage-map")
            .nth(1)
            .unwrap()
            .split("</section>")
            .next()
            .unwrap()
    }

    fn file_card<'a>(content: &'a str, file: &str) -> &'a str {
        let row = content
            .split(&format!("data-file=\"{}\"", escaped(file)))
            .nth(1)
            .unwrap();
        if row.contains("gap-file-button") {
            row.split("</tbody></table></td></tr>").next().unwrap()
        } else {
            row.split("</tr>").next().unwrap()
        }
    }

    // Return a complete native details group, accounting for nested groups and
    // explanation controls rather than stopping at the first closing tag.
    fn directory_group<'a>(html: &'a str, directory: &str) -> &'a str {
        let start = html
            .find(&format!(
                "<details class=\"gap-directory\" data-directory=\"{directory}\""
            ))
            .unwrap();
        let rest = &html[start..];
        let tags = regex::Regex::new(r"<details(?:\s[^>]*|)>|</details>").unwrap();
        let mut depth = 0;
        for tag in tags.find_iter(rest) {
            if tag.as_str() == "</details>" {
                depth -= 1;
                if depth == 0 {
                    return &rest[..tag.end()];
                }
            } else {
                depth += 1;
            }
        }
        panic!("unclosed directory {directory}");
    }

    fn assert_directory_contents(html: &str, directory: &str, children: &[&str], files: &[&str]) {
        let group = directory_group(html, directory);
        let (summary, body) = group.split_once("</summary>").unwrap();
        assert!(summary.contains(&format!("<code>{directory}/</code>")));
        assert!(!summary.contains("data-file="));
        assert!(!summary.contains("<table"));
        for child in children {
            assert!(body.contains(directory_group(html, child)));
        }
        for file in files {
            assert!(body.contains(&format!("data-file=\"{file}\"")));
        }
    }

    // {test}`requirements.test-coverage.index`
    // {test}`requirements.test-coverage.index.navigation`
    // {test}`requirements.test-coverage.index.source`
    // {test}`requirements.test-coverage.index.guidance`
    // {test}`requirements.test-coverage.check.guidance`
    // {test}`requirements.test-coverage.check.exceptions.setup`
    // {test}`requirements.test-coverage.index.details`
    #[test]
    fn generated_reports_inherit_the_requirements_index_hierarchy() {
        for number in [Some(SectionNumber::new(vec![3, 2])), None] {
            let (temp, mut book) = fixture();
            let BookItem::Chapter(parent) = &mut book.items[0] else {
                unreachable!()
            };
            parent.path = Some("reports.md".into());
            parent.name = "Requirements index".into();
            parent.content = "# Requirements index\n".into();
            parent.parent_names = vec!["Reports".into()];
            parent.number = number.clone();
            parent.sub_items.push(BookItem::Chapter(Chapter::new(
                "Guide",
                "# Guide\n".into(),
                "reports/guide.md",
                vec![],
            )));
            record(temp.path(), "one", 1, "a", &[(1, 1)]);
            // Build fixture configuration before invoking the production path.
            fs::create_dir(temp.path().join("md")).unwrap();
            fs::write(temp.path().join("book.toml"), "[book]\nsrc='md'\n").unwrap();
            fs::write(temp.path().join("md/SUMMARY.md"), "# Summary\n").unwrap();
            super::populate_in(
                &mut book,
                temp.path(),
                Some(temp.path().join("target")),
                Path::new("reports.md"),
            )
            .unwrap();
            let BookItem::Chapter(parent) = &book.items[0] else {
                unreachable!()
            };
            assert!(
                parent
                    .content
                    .contains("[Coverage gaps](reports/coverage.html)")
            );
            assert_eq!(parent.number, number);
            let BookItem::Chapter(coverage) = &parent.sub_items[0] else {
                unreachable!()
            };
            assert_eq!(coverage.parent_names, ["Reports", "Requirements index"]);
            assert_eq!(
                coverage.path.as_deref(),
                Some(Path::new("reports/coverage.md"))
            );
            assert!(coverage.source_path.is_none());
            assert_eq!(
                coverage.number,
                number.as_ref().map(|_| SectionNumber::new(vec![3, 2, 1]))
            );
            assert_eq!(parent.sub_items.len(), 2);
            for (offset, item) in coverage.sub_items.iter().enumerate() {
                let BookItem::Chapter(page) = item else {
                    unreachable!()
                };
                assert_eq!(
                    page.parent_names,
                    ["Reports", "Requirements index", "Coverage gaps"]
                );
                assert_eq!(
                    page.number,
                    number
                        .as_ref()
                        .map(|_| SectionNumber::new(vec![3, 2, 1, offset as u32 + 1]))
                );
                assert!(page.source_path.is_none());
                assert!(page.content.contains("../coverage.html"));
            }
            for guidance in [
                "habeas test",
                "habeas coverage -- --workspace",
                "habeas coverage-check",
                "static validation test needs no prior coverage",
                "80%",
                "habeas:coverage",
                "current accepted review",
                "No project declaration or special configuration",
                "after it succeeds,\nrun `habeas coverage-check` as a separate step",
                "`habeas test` collects coverage for the whole workspace,\nthen checks it",
                "substantive prose justification",
                "fenced commentary or a Rust doc comment",
                "citation review with `habeas author`",
                "Ordinary partly covered functions\nneed no exception when their file meets its target",
            ] {
                assert!(
                    coverage.content.contains(guidance),
                    "missing generated guidance: {guidance}"
                );
            }
            assert!(!coverage.content.contains(":::{coverage-index}"));
        }
    }

    // {test}`requirements.test-coverage.index.source`
    #[test]
    fn authored_coverage_directives_do_not_control_generated_placement() {
        for content in [
            "A plain chapter.\n",
            ":::{coverage-index}\n:::\n",
            ":::{coverage-index}\n:::\n\n:::{coverage-index}\n:::\n",
        ] {
            let (temp, mut book) = fixture();
            let root = temp.path();
            fs::create_dir(root.join("md")).unwrap();
            fs::write(root.join("book.toml"), "[book]\nsrc='md'\n").unwrap();
            fs::write(root.join("md/SUMMARY.md"), "# Summary\n").unwrap();
            let BookItem::Chapter(parent) = &mut book.items[0] else {
                unreachable!()
            };
            parent.name = "Requirements index".into();
            parent.content = "# Requirements index\n".into();
            parent.path = Some("reports.md".into());
            book.push_item(Chapter::new(
                "Authored notes",
                content.into(),
                "elsewhere.md",
                vec![],
            ));
            super::populate_in(
                &mut book,
                root,
                Some(root.join("target")),
                Path::new("reports.md"),
            )
            .unwrap();
            let coverage = book
                .chapters()
                .filter(|chapter| chapter.name == "Coverage gaps")
                .collect::<Vec<_>>();
            assert_eq!(coverage.len(), 1);
            assert_eq!(
                coverage[0].path.as_deref(),
                Some(Path::new("reports/coverage.md"))
            );
            assert_eq!(coverage[0].parent_names, ["Requirements index"]);
            assert!(coverage[0].source_path.is_none());
            assert!(!root.join("md/reports/coverage.md").exists());
            let BookItem::Chapter(parent) = &book.items[0] else {
                unreachable!()
            };
            assert!(
                matches!(&parent.sub_items[0], BookItem::Chapter(child) if child.name == "Coverage gaps")
            );
            let BookItem::Chapter(authored) = &book.items[1] else {
                unreachable!()
            };
            assert_eq!(authored.content, content);
            assert!(authored.sub_items.is_empty());
        }
    }

    // {test}`requirements.test-coverage.index.file_map.combined`
    // {test}`requirements.test-coverage.index.file_map.directories.initial`
    // {test}`requirements.test-coverage.index.file_map.directories.data`
    // {test}`requirements.test-coverage.index.file_map.minimum`
    // {test}`requirements.test-coverage.index.file_map.test`
    // {test}`requirements.test-coverage.index.gaps.summary`
    // {test}`requirements.test-coverage.index.file_map.directories`
    #[test]
    fn combined_directory_minimum_includes_hidden_passing_files() {
        let (temp, mut book) = fixture();
        let root = temp.path();
        fs::create_dir_all(root.join("src/nested/deep")).unwrap();
        fs::write(
            root.join("src/gap.rs"),
            format!("fn gap() {{\n{}}}\n", " let value = 1;\n".repeat(18)),
        )
        .unwrap();
        fs::write(
            root.join("src/nested/passing.rs"),
            "fn passing() {\n let a = 1;\n let b = 2;\n let c = 3;\n}\n",
        )
        .unwrap();
        fs::write(root.join("src/nested/deep/gap.rs"), "fn nested_gap() {}\n").unwrap();
        let path = record(root, "all", 1, "same", &[(1, 0), (2, 1)]);
        let run_path = path.parent().unwrap().parent().unwrap().join("run.json");
        let mut run: Run = load(&run_path).unwrap();
        let mut attempt: Attempt = load(&path.join("attempt.json")).unwrap();
        for (file, total, covered, count) in [
            ("src/gap.rs", 20, 19, 0),
            ("src/nested/passing.rs", 5, 4, 1),
            ("src/nested/deep/gap.rs", 1, 1, 0),
        ] {
            run.sources
                .insert(file.into(), fs::read_to_string(root.join(file)).unwrap());
            attempt.processes[0].lines.insert(
                file.into(),
                (1..=total)
                    .map(|line| (line, u64::from(line <= covered)))
                    .collect(),
            );
            attempt.processes[0].functions.as_mut().unwrap().push(
                super::super::FunctionExecution {
                    file: file.into(),
                    line: 1,
                    column: 1,
                    count,
                },
            );
        }
        save(&run_path, &run).unwrap();
        save(&path.join("attempt.json"), &attempt).unwrap();
        populate(&mut book, root).unwrap();
        let overview = &book.chapters().next().unwrap().content;
        let map = map_html(overview);
        assert!(map.contains("data-directory=\"src\" open"));
        assert!(map.contains("data-min-covered=\"4\" data-min-instrumented=\"5\">80.0%"));
        assert!(map.contains("2 uncovered obligations"));
        for (directory, expected) in [("src", 2), ("src/nested", 1), ("src/nested/deep", 1)] {
            let summary = directory_summary(map, directory);
            assert!(summary.contains(&format!("{expected} uncovered obligations")));
            assert!(summary.contains("Minimum measured file coverage:"));
        }
        assert_directory_contents(map, "src", &["src/nested"], &["src/gap.rs"]);
        assert_directory_contents(map, "src/nested", &["src/nested/deep"], &[]);
        assert_directory_contents(map, "src/nested/deep", &[], &["src/nested/deep/gap.rs"]);
        for directory in ["src", "src/nested", "src/nested/deep"] {
            assert!(map.contains(&format!("data-directory=\"{directory}\" open")));
        }
        assert!(map.contains("data-file=\"code.rs\""));
        assert!(
            map.find("data-file=\"code.rs\"").unwrap()
                < map.find("<details class=\"gap-directory\"").unwrap(),
            "root file rows are outside directory controls"
        );
        assert_eq!(map.matches("data-file=").count(), 3);
        assert!(map.contains("data-file=\"src/gap.rs\""));
        assert!(map.contains("95.0%"));
        assert!(file_card(map, "src/gap.rs").contains("<code>src/gap.rs</code>"));
        assert!(map.contains("zero execution"));
        assert!(!map.contains("data-file=\"src/nested/passing.rs\""));
        assert!(
            book.chapters()
                .all(|chapter| chapter.name != "File notes: src/nested/passing.rs")
        );
        assert!(overview.contains("1 file obligations · 3 function obligations"));
    }

    // {test}`requirements.test-coverage.index.paths`
    #[test]
    fn generated_paths_reject_index_report_and_report_report_collisions() {
        let existing = BTreeSet::from([PathBuf::from("authored.md")]);
        let mut generated = BTreeSet::new();
        let index = Path::new("reports/coverage.md");
        reserve_path(&existing, &mut generated, index, "index").unwrap();
        assert!(
            reserve_path(&existing, &mut generated, index, "report")
                .unwrap_err()
                .to_string()
                .contains("coverage report path conflicts")
        );
        let report = Path::new("reports/coverage/file.md");
        reserve_path(&existing, &mut generated, report, "report").unwrap();
        assert!(reserve_path(&existing, &mut generated, report, "report").is_err());
        assert!(
            reserve_path(
                &existing,
                &mut generated,
                Path::new("authored.md"),
                "report"
            )
            .is_err()
        );
        assert_eq!(
            generated,
            BTreeSet::from([index.to_owned(), report.to_owned()])
        );
    }

    // {test}`requirements.test-coverage.index.paths`
    // {test}`requirements.test-coverage.index.file_map`
    // {test}`requirements.test-coverage.index.file_map.table`
    // {test}`requirements.test-coverage.index.file_map.combined`
    // {test}`requirements.test-coverage.index.file_map.test`
    // {test}`requirements.test-coverage.index.summary`
    // {test}`requirements.test-coverage.index.file_map.combined.details`
    // {test}`requirements.test-coverage.index.file_map.notes`
    // {test}`requirements.test-coverage.index.details`
    #[test]
    fn gaps_union_process_locations_and_omit_per_test_reports() {
        let (temp, mut book) = fixture();
        let root = temp.path();
        fs::write(
            root.join("code.rs"),
            "fn one() {}\nfn two() {}\nfn three() {}\n// not instrumented\n",
        )
        .unwrap();
        fs::write(root.join("zero.rs"), "fn zero() {}\n").unwrap();
        fs::write(root.join("unmeasured.rs"), "// no instrumentation\n").unwrap();
        fs::write(root.join("executable.rs"), "fn unmeasured() {}\n").unwrap();
        let path = record(root, "one", 1, "a", &[(1, 2), (2, 0)]);
        let run_path = path.parent().unwrap().parent().unwrap().join("run.json");
        let mut run: Run = load(&run_path).unwrap();
        for file in ["zero.rs", "unmeasured.rs", "executable.rs"] {
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
            functions: Some(vec![]),
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
        assert!(file_card(map, "code.rs").contains("--gap-covered: 66.666667%"));
        let notes_path = super::super::notes::path(Path::new("reports/coverage.md"), "code.rs");
        let href = crate::chapter_href(Path::new("reports/coverage.md"), &notes_path, None);
        assert!(file_card(map, "code.rs").contains(&format!("href=\"{href}\"")));
        let notes = book
            .chapters()
            .find(|chapter| chapter.path.as_ref() == Some(&notes_path))
            .unwrap();
        assert!(notes.content.contains("File: <code>code.rs</code>"));
        assert!(file_card(map, "zero.rs").contains("0.0%"));
        assert!(file_card(map, "zero.rs").contains("--gap-covered: 0.000000%"));
        assert!(
            !map.contains("data-file=\"unmeasured.rs\""),
            "a non-executable file has no gap"
        );
        let unmeasured = file_card(map, "executable.rs")
            .split("</tr>")
            .next()
            .unwrap();
        assert!(unmeasured.contains("not measured"));
        assert!(!unmeasured.contains('%'));
        assert!(!unmeasured.contains("gap-coverage-bar"));
        assert!(map.contains("<th>File</th><th>Coverage</th>"));
        assert_eq!(
            map.matches("class=\"gap-file-row habeas-coverage-file\"")
                .count(),
            3
        );
        let mut html = String::new();
        pulldown_cmark::html::push_html(
            &mut html,
            pulldown_cmark::Parser::new_ext(index, pulldown_cmark::Options::all()),
        );
        assert!(html.contains("Unresolved coverage gaps"));
        assert!(!html.contains("habeas-coverage-test"));
        assert!(!html.contains("Test runs and provenance"));
        assert!(!html.contains("binary / one"));
        assert!(!html.contains("<select"));
        assert!(
            book.chapters()
                .skip(1)
                .all(|chapter| chapter.name.starts_with("File notes:"))
        );
    }

    // {test}`requirements.test-coverage.index.file_map`
    // {test}`requirements.test-coverage.index.file_map.table`
    // {test}`requirements.test-coverage.index.file_map.combined`
    // {test}`requirements.test-coverage.index.file_map.test`
    // {test}`requirements.test-coverage.index.file_map.combined.measurement`
    // {test}`requirements.test-coverage.index.summary`
    #[test]
    fn maps_distinguish_no_runs_zero_hits_stale_and_incomplete_coverage() {
        let (temp, book) = fixture();
        let root = temp.path();
        let mut no_runs = book.clone();
        populate(&mut no_runs, root).unwrap();
        let index = &no_runs.chapters().next().unwrap().content;
        assert!(index.contains("Coverage unavailable"));
        assert!(index.contains("gap counts unavailable"));
        assert!(!map_html(index).contains("data-file="));
        assert!(index.contains("Run `habeas coverage`"));

        let empty_path = record(root, "empty", 0, "empty", &[]);
        let mut empty = book.clone();
        populate(&mut empty, root).unwrap();
        let empty_index = &empty.chapters().next().unwrap().content;
        assert!(empty_index.contains("Coverage unavailable"));
        assert!(!empty_index.contains("Line coverage:"));
        assert!(!empty_index.contains("NaN"));
        assert!(!map_html(empty_index).contains("data-file="));
        assert!(empty_index.contains("No usable instrumented lines"));
        fs::remove_dir_all(empty_path.parent().unwrap().parent().unwrap()).unwrap();
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
            let index = &unavailable.chapters().next().unwrap().content;
            assert!(index.contains("Coverage unavailable"), "{failure}");
            assert!(!map_html(index).contains("data-file="), "{failure}");
            assert!(
                index.contains("missing or unreadable coverage"),
                "{failure}"
            );
            fs::write(path.join("1-1.profraw"), "retained profile fixture").unwrap();
        }
        save(&path.join("attempt.json"), &attempt).unwrap();
        fs::write(root.join("code.rs"), "changed source\n").unwrap();
        fs::write(root.join("new.rs"), "fn newly_added() {}\n").unwrap();
        let mut stale = book.clone();
        populate(&mut stale, root).unwrap();
        let index = &stale.chapters().next().unwrap().content;
        assert!(index.contains("Coverage unavailable"));
        assert!(!map_html(index).contains("data-file="));
        assert!(index.contains("1 stale test attempts"));
        attempt.processes.push(Process {
            functions: Some(vec![]),
            pid: 2,
            profiles: vec!["missing-child.profraw".into()],
            diagnostic: None,
            lines: BTreeMap::from([("code.rs".into(), BTreeMap::from([(2, 1)]))]),
        });
        save(&path.join("attempt.json"), &attempt).unwrap();
        let mut incomplete = book.clone();
        populate(&mut incomplete, root).unwrap();
        let index = &incomplete.chapters().next().unwrap().content;
        assert!(index.contains("Coverage unavailable"));
        assert!(!map_html(index).contains("data-file="));
        assert!(index.contains("missing or unreadable coverage"));

        fs::write(root.join("code.rs"), "fn one() {}\nfn two() {}\n").unwrap();
        fs::remove_file(root.join("new.rs")).unwrap();
        let mut unreadable = book.clone();
        populate(&mut unreadable, root).unwrap();
        let index = &unreadable.chapters().next().unwrap().content;
        assert!(index.contains("1 unavailable test attempts"));
        assert!(index.contains("Coverage unavailable"));
        assert!(!map_html(index).contains("data-file="));
        // Unreadable metadata cannot establish latest-per-test selection.
        fs::write(path.join("attempt.json"), "not JSON").unwrap();
        let mut uncertain = book;
        populate(&mut uncertain, root).unwrap();
        let index = &uncertain.chapters().next().unwrap().content;
        assert!(index.contains("Coverage history is incomplete"));
        assert!(index.contains("gap counts unavailable"));
        assert!(!map_html(index).contains("data-file="));
    }

    // {test}`requirements.test-coverage.index.file_map`
    // {test}`requirements.test-coverage.index.file_map.table`
    // {test}`requirements.test-coverage.index.file_map.combined`
    // {test}`requirements.test-coverage.index.file_map.combined.measurement`
    // {test}`requirements.test-coverage.index.freshness`
    // {test}`requirements.test-coverage.index.file_map.minimum`
    // {test}`requirements.test-coverage.index.file_map.minimum.statuses`
    #[test]
    fn incomplete_attempts_do_not_label_their_files_as_unmeasured_in_a_partial_map() {
        let (temp, book) = fixture();
        let root = temp.path();
        fs::create_dir_all(root.join("src/nested")).unwrap();
        fs::write(root.join("src/nested/other.rs"), "fn other() {}\n").unwrap();
        let good = record(root, "good", 1, "a", &[(1, 1)]);
        let bad = record(root, "bad", 2, "a", &[]);
        let mut attempt: Attempt = load(&bad.join("attempt.json")).unwrap();
        attempt.processes[0].lines =
            BTreeMap::from([("src/nested/other.rs".into(), BTreeMap::from([(1, 1)]))]);
        save(&bad.join("attempt.json"), &attempt).unwrap();
        for path in [&good, &bad] {
            let run_path = path.parent().unwrap().parent().unwrap().join("run.json");
            let mut run: Run = load(&run_path).unwrap();
            run.sources
                .insert("src/nested/other.rs".into(), "fn other() {}\n".into());
            save(&run_path, &run).unwrap();
        }
        fs::remove_file(bad.join("1-1.profraw")).unwrap();
        let mut partial = book.clone();
        populate(&mut partial, root).unwrap();
        let index = &partial.chapters().next().unwrap().content;
        assert!(index.contains("Coverage unavailable"));
        assert!(!map_html(index).contains("data-file="));
        assert!(!map_html(index).contains("not measured"));
        assert!(!map_html(index).contains("class=\"gap-directory\""));
        assert!(!map_html(index).contains("data-min-covered="));
        assert!(!map_html(index).contains("gap-coverage-bar\""));
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
        assert!(index.contains("Coverage unavailable"));
        assert!(!map_html(index).contains("data-file="));
        assert!(!map_html(index).contains("class=\"gap-directory\""));
        assert!(!map_html(index).contains("data-min-covered="));
        assert!(!map_html(index).contains("not measured"));
        assert!(index.contains("1 stale test attempts"));
    }

    // {test}`requirements.test-coverage.index.file_map`
    // {test}`requirements.test-coverage.index.file_map.table`
    #[test]
    fn gaps_escape_paths_and_do_not_round_positive_coverage_to_zero() {
        let policy = super::super::check::PolicyReport {
            report: super::super::check::CheckReport {
                passed: false,
                text: String::new(),
            },
            files: vec![super::super::check::FilePolicy {
                file: "src/<unsafe>\".rs".into(),
                covered: 1,
                total: 2000,
                obligations: vec![super::super::check::Obligation {
                    selector: String::new(),
                    line: 1,
                    measurement: "below target".into(),
                    exception: None,
                }],
            }],
            counts_complete: true,
            measured: true,
            findings: vec![],
        };
        let map = gaps_map(&policy, Path::new("coverage.md"), 1);
        assert!(map.contains("src/&lt;unsafe&gt;&quot;.rs"));
        assert!(!map.contains("<unsafe>"));
        assert!(map.contains("&lt;0.1%"));
        assert!(map.contains("1/2000 instrumented lines covered"));
        assert!(!map.contains(">0.0%<"));
    }

    // {test}`requirements.test-coverage.index.file_map`
    // {test}`requirements.test-coverage.index.file_map.table`
    #[test]
    fn coverage_bars_distinguish_measured_levels_alongside_percentage_text() {
        let mut policy = super::super::check::PolicyReport {
            report: super::super::check::CheckReport {
                passed: false,
                text: String::new(),
            },
            files: vec![],
            counts_complete: true,
            measured: true,
            findings: vec![],
        };
        for covered in [0, 1, 25, 75, 100] {
            policy.files.push(super::super::check::FilePolicy {
                file: format!("src/level-{covered}.rs"),
                covered,
                total: 100,
                obligations: vec![super::super::check::Obligation {
                    selector: "fn[unexecuted]".into(),
                    line: 1,
                    measurement: "zero execution".into(),
                    exception: None,
                }],
            });
        }
        let map = gaps_map(&policy, Path::new("coverage.md"), 1);
        for covered in [0, 1, 25, 75, 100] {
            let file = format!("src/level-{covered}.rs");
            let row = file_card(&map, &file).split("</tr>").next().unwrap();
            assert!(row.contains(&format!("<code>{file}</code>")));
            assert!(row.contains(&format!(">{covered}.0%")));
            assert!(row.contains(&format!("--gap-covered: {covered}.000000%")));
            assert!(row.contains("class=\"gap-coverage-bar\" aria-hidden=\"true\""));
            assert_eq!(map.matches(&format!("data-file=\"{file}\"")).count(), 1);
        }
        let css = include_str!("../../assets/coverage-gaps.css");
        assert!(css.contains(".gap-coverage-bar::before"));
        assert!(css.contains("width: var(--gap-covered)"));
        assert!(css.contains("background: var(--gap-link)"));
    }

    fn directory_summary<'a>(map: &'a str, directory: &str) -> &'a str {
        map.split(&format!("data-directory=\"{directory}\""))
            .nth(1)
            .unwrap()
            .split("</summary>")
            .next()
            .unwrap()
    }

    // {test}`requirements.test-coverage.index.file_map.minimum`
    // {test}`requirements.test-coverage.index.file_map.minimum.statuses`
    // {test}`requirements.test-coverage.index.file_map.minimum.descendants`
    // {test}`requirements.test-coverage.index.file_map.directories`
    // {test}`requirements.test-coverage.index.file_map.directories.data`
    #[test]
    fn directory_minima_use_exact_fractions_and_count_unmeasured_descendants() {
        let render = |files: &[(&str, usize, usize)]| {
            let mut policy = super::super::check::PolicyReport {
                report: super::super::check::CheckReport {
                    passed: false,
                    text: String::new(),
                },
                files: files
                    .iter()
                    .map(|&(file, covered, total)| super::super::check::FilePolicy {
                        file: file.into(),
                        covered,
                        total,
                        obligations: vec![super::super::check::Obligation {
                            selector: String::new(),
                            line: 1,
                            measurement: "below target".into(),
                            exception: None,
                        }],
                    })
                    .collect(),
                counts_complete: true,
                measured: true,
                findings: vec![],
            };
            // Unlisted files with no obligations or accepted exceptions must
            // not inflate the directory's not-measured count.
            for (file, accepted) in [
                ("src/nested/hidden-empty.rs", false),
                ("src/nested/hidden-accepted.rs", true),
            ] {
                policy.files.push(super::super::check::FilePolicy {
                    file: file.into(),
                    covered: 0,
                    total: 0,
                    obligations: if accepted {
                        vec![super::super::check::Obligation {
                            selector: String::new(),
                            line: 1,
                            measurement: "not measured".into(),
                            exception: Some(super::super::check::Exception {
                                accepted: true,
                                review_missing: false,
                                justification: "No instrumented obligations.".into(),
                                findings: vec![],
                            }),
                        }]
                    } else {
                        vec![]
                    },
                });
            }
            gaps_map(&policy, Path::new("coverage.md"), 1)
        };
        let map = render(&[("src/higher.rs", 501, 1000), ("src/lower.rs", 1001, 2000)]);
        assert!(
            directory_summary(&map, "src")
                .contains("data-min-covered=\"1001\" data-min-instrumented=\"2000\"")
        );
        let map = render(&[
            ("src/a.rs", 1, 2),
            ("src/nested/b.rs", 1, 4),
            ("src/nested/c.rs", 0, 0),
        ]);
        for directory in ["src", "src/nested"] {
            let summary = directory_summary(&map, directory);
            assert!(summary.contains("Minimum measured file coverage:"));
            assert!(summary.contains("25.0%"));
            assert!(summary.contains("1 not measured"));
            assert!(!summary.contains("0 unavailable"));
            assert!(!summary.contains("2 not measured"));
        }
        assert_directory_contents(&map, "src", &["src/nested"], &["src/a.rs"]);
        assert_directory_contents(
            &map,
            "src/nested",
            &[],
            &["src/nested/b.rs", "src/nested/c.rs"],
        );
        // Changing one native details control only changes its open attribute;
        // all file identities, measurements, summaries and sibling state remain.
        let opening = "<details class=\"gap-directory\" data-directory=\"src/nested\" open>";
        let closed_opening = opening.replace(" open>", ">");
        let closed = map.replace(opening, &closed_opening);
        assert_ne!(closed, map);
        assert!(closed.contains("data-directory=\"src\" open>"));
        assert!(closed.contains("data-directory=\"src/nested\">"));
        assert_eq!(
            directory_group(&closed, "src/nested")
                .split_once('>')
                .unwrap()
                .1,
            directory_group(&map, "src/nested")
                .split_once('>')
                .unwrap()
                .1
        );
        for file in ["src/a.rs", "src/nested/b.rs", "src/nested/c.rs"] {
            assert_eq!(file_card(&map, file), file_card(&closed, file));
        }
        assert_eq!(closed.replace(&closed_opening, opening), map);
        let map = render(&[("src/unknown.rs", 0, 0)]);
        let summary = directory_summary(&map, "src");
        assert!(summary.contains("No measured files"));
        assert!(summary.contains("1 not measured"));
        assert!(!summary.contains('%'));
        let map = render(&[
            ("src/measured-zero.rs", 0, 4),
            ("src/nested/unmeasured.rs", 0, 0),
        ]);
        let summary = directory_summary(&map, "src");
        assert!(summary.contains("Minimum measured file coverage:"));
        assert!(summary.contains("0.0%"));
        assert!(summary.contains("1 not measured"));
        assert!(!summary.contains("2 not measured"));
        let zero = file_card(&map, "src/measured-zero.rs");
        assert!(zero.contains("data-status=\"measured\""));
        assert!(zero.contains("0.0%"));
        assert!(zero.contains("0/4 instrumented lines covered"));
        assert!(!zero.contains("not measured"));
        let nested = directory_summary(&map, "src/nested");
        assert!(nested.contains("No measured files"));
        assert!(nested.contains("1 not measured"));
        assert!(!nested.contains('%'));
        let map = render(&[
            ("src/direct.rs", 0, 0),
            ("src/nested/a.rs", 0, 0),
            ("src/nested/deep/b.rs", 0, 0),
            ("src/nested/deep/c.rs", 0, 0),
            ("src/sibling/d.rs", 0, 0),
            ("src/measured-zero.rs", 0, 4),
            ("src/positive.rs", 1, 4),
            ("src-other/outside.rs", 0, 0),
        ]);
        for (directory, count) in [
            ("src", 5),
            ("src/nested", 3),
            ("src/nested/deep", 2),
            ("src/sibling", 1),
            ("src-other", 1),
        ] {
            let summary = directory_summary(&map, directory);
            assert!(
                summary.contains(&format!(" · {count} not measured")),
                "{summary}"
            );
        }
        assert!(directory_summary(&map, "src").contains("0.0%"));
        let closed = map.replace(
            "<details class=\"gap-directory\" data-directory=\"src/nested\" open>",
            "<details class=\"gap-directory\" data-directory=\"src/nested\">",
        );
        for directory in [
            "src",
            "src/nested",
            "src/nested/deep",
            "src/sibling",
            "src-other",
        ] {
            assert_eq!(
                directory_summary(&closed, directory)
                    .split_once('>')
                    .unwrap()
                    .1,
                directory_summary(&map, directory)
                    .split_once('>')
                    .unwrap()
                    .1
            );
        }
    }

    // {test}`requirements.test-coverage.index.file_map.notes`
    // {test}`requirements.test-coverage.index.file_map.notes.current`
    // {test}`requirements.test-coverage.index.file_map.notes.render_only`
    // {test}`requirements.test-coverage.index.paths`
    #[test]
    fn gap_notes_use_current_source_without_collecting() {
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
        let mut distinct = BTreeSet::new();
        for file in ["code.rs", "src/a.rs", "src/nested/a.rs"] {
            let path = super::super::notes::path(index.path.as_ref().unwrap(), file);
            assert_ne!(
                path,
                *index.path.as_ref().unwrap(),
                "report and index paths must differ"
            );
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
            assert!(index.content.contains("Coverage unavailable"));
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
                    .contains("stale coverage; recollect"),
                "{name}"
            );
            run.environment_inputs.remove(name);
        }
    }

    // {test}`requirements.test-coverage.index.selection`
    #[test]
    #[cfg(unix)]
    fn metadata_cache_preserves_selection_and_detects_edits_with_restored_mtime() {
        let (temp, _) = fixture();
        let dir = record(temp.path(), "one", 1, "build-a", &[(1, 1)]);
        let path = dir.join("attempt.json");
        let original_mtime = fs::metadata(&path).unwrap().modified().unwrap();
        let now = std::time::SystemTime::now() + std::time::Duration::from_secs(10);
        let mut cache = BTreeMap::new();
        let mut changed = false;
        assert_eq!(
            load_metadata(&path, &mut cache, &mut changed, now)
                .unwrap()
                .test,
            "one"
        );
        assert!(changed);
        changed = false;
        assert_eq!(
            load_metadata(&path, &mut cache, &mut changed, now)
                .unwrap()
                .test,
            "one"
        );
        assert!(!changed, "unchanged attempts use cached metadata");

        let original = fs::read_to_string(&path).unwrap();
        let modified = original.replace("\"test\": \"one\"", "\"test\": \"two\"");
        assert_eq!(original.len(), modified.len());
        // The artificial clock makes fresh files eligible for caching. Wait
        // for the filesystem's change timestamp to advance; real callers
        // instead reread all files changed within the past two seconds.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            fs::write(&path, &modified).unwrap();
            fs::File::open(&path)
                .unwrap()
                .set_times(std::fs::FileTimes::new().set_modified(original_mtime))
                .unwrap();
            if load_metadata(&path, &mut cache, &mut changed, now)
                .unwrap()
                .test
                == "two"
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "same-size edits with restored mtime must invalidate the cache"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            changed,
            "change time detects same-size edits with restored mtime"
        );

        fs::write(&path, "{").unwrap();
        assert!(load_metadata(&path, &mut cache, &mut changed, now).is_err());
        changed = false;
        assert!(load_metadata(&path, &mut cache, &mut changed, now).is_err());
        assert!(!changed, "cached corruption remains a selection failure");
        fs::write(&path, original).unwrap();
        assert_eq!(
            load_metadata(&path, &mut cache, &mut changed, now)
                .unwrap()
                .test,
            "one"
        );

        changed = false;
        load_metadata(
            &path,
            &mut cache,
            &mut changed,
            std::time::SystemTime::now(),
        )
        .unwrap();
        assert!(
            !cache.contains_key(&path),
            "recent writes are always reread"
        );
    }

    // {test}`requirements.test-coverage.index.unavailable`
    // {test}`requirements.test-coverage.index.selection`
    #[test]
    fn a_corrupt_optional_metadata_cache_does_not_hide_saved_measurements() {
        let (temp, _) = fixture();
        let root = temp.path();
        record(root, "one", 1, "build-a", &[(1, 4)]);
        fs::write(
            root.join("target/habeas/coverage/selection-cache.json"),
            "{",
        )
        .unwrap();
        let saved = load_saved(root, Some(root.join("target")));
        assert!(!saved.uncertain_history);
        assert_eq!(
            union(saved.selected.iter().map(|&index| &saved.records[index])),
            BTreeMap::from([(("code.rs".into(), 1), 4)])
        );
    }

    // {test}`requirements.test-coverage.index.selection`
    #[test]
    fn history_selection_loads_measurements_only_for_latest_attempts() {
        let (temp, _) = fixture();
        let root = temp.path();
        record(root, "one", 1, "build-a", &[(1, 0)]);
        record(root, "one", 2, "build-b", &[(1, 9)]);
        record(root, "one", 3, "build-a", &[(1, 4)]);
        record(root, "two", 4, "build-a", &[(2, 7)]);
        let mut saved = load_saved(root, Some(root.join("target")));
        assert!(!saved.uncertain_history);
        assert_eq!(saved.selected.len(), 2);
        assert_eq!(
            union(saved.selected.iter().map(|&index| &saved.records[index])),
            BTreeMap::from([(("code.rs".into(), 1), 4), (("code.rs".into(), 2), 7)])
        );
        for (index, record) in saved.records.iter().enumerate() {
            assert_eq!(
                record.attempt.processes.is_empty(),
                !saved.selected.contains(&index),
                "excluded attempts should retain metadata without their measurements"
            );
        }
        let historical = &mut saved.records[1];
        assert_eq!(historical.run.configuration, "build-b");
        historical.load_measurements().unwrap();
        assert_eq!(
            union(std::iter::once(&*historical)),
            BTreeMap::from([(("code.rs".into(), 1), 9)])
        );
    }

    // {test}`requirements.test-coverage.check.measurement`
    // {test}`requirements.test-coverage.index.selection`
    #[test]
    fn malformed_historical_measurements_make_selection_uncertain() {
        let (temp, _) = fixture();
        let root = temp.path();
        let old = record(root, "one", 1, "build-a", &[(1, 1)]);
        record(root, "one", 2, "build-a", &[(1, 2)]);
        let path = old.join("attempt.json");
        let original: serde_json::Value = load(&path).unwrap();
        for (pointer, invalid) in [
            (
                "/processes/0/lines/code.rs/1",
                serde_json::json!("not a count"),
            ),
            ("/processes/0/lines", serde_json::json!([])),
            (
                "/processes/0/functions",
                serde_json::json!([{"file": "code.rs"}]),
            ),
            ("/processes/0/functions", serde_json::json!({})),
            ("/processes/0/profiles", serde_json::json!(null)),
            ("/processes", serde_json::json!(null)),
        ] {
            let mut broken = original.clone();
            *broken.pointer_mut(pointer).unwrap() = invalid;
            save(&path, &broken).unwrap();
            let saved = load_saved(root, Some(root.join("target")));
            assert!(saved.uncertain_history, "{pointer}");
            assert_eq!(
                union(saved.selected.iter().map(|&index| &saved.records[index])),
                BTreeMap::from([(("code.rs".into(), 1), 2)])
            );
        }
        save(&path, &original).unwrap();
        assert!(!load_saved(root, Some(root.join("target"))).uncertain_history);
    }

    // {test}`requirements.test-coverage.index.selection`
    // {test}`requirements.test-coverage.check.measurement`
    #[test]
    fn selected_attempt_cannot_be_replaced_between_selection_and_loading() {
        let (temp, _) = fixture();
        let root = temp.path();
        let dir = record(root, "one", 1, "build-a", &[(1, 1)]);
        let mut saved = load_saved(root, Some(root.join("target")));
        let original: Attempt = load(&dir.join("attempt.json")).unwrap();
        for field in ["id", "binary", "test", "started"] {
            let mut changed = serde_json::to_value(&original).unwrap();
            changed[field] = if field == "started" {
                serde_json::json!(2)
            } else {
                serde_json::json!("replacement")
            };
            save(&dir.join("attempt.json"), &changed).unwrap();
            assert!(
                saved.records[0]
                    .load_measurements()
                    .unwrap_err()
                    .to_string()
                    .contains("changed during coverage selection"),
                "{field}"
            );
        }
        save(&dir.join("attempt.json"), &original).unwrap();
        saved.records[0].load_measurements().unwrap();
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
        let saved = load_saved(temp.path(), Some(temp.path().join("target")));
        assert_eq!(saved.selected.len(), 2);
        let binaries = saved
            .selected
            .iter()
            .map(|&i| saved.records[i].attempt.binary.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(binaries, BTreeSet::from(["first", "second"]));

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
        assert!(index.contains("1 attempts from a historical build configuration"));
        let saved = load_saved(temp.path(), Some(temp.path().join("target")));
        let tests = saved
            .selected
            .iter()
            .map(|&i| saved.records[i].attempt.test.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(tests, BTreeSet::from(["one", "two"]));

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
    // {test}`requirements.test-coverage.index.file_map.combined.measurement`
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
            combined
                .chapters()
                .next()
                .unwrap()
                .content
                .contains("2 selected test attempts")
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
                    .contains("gap counts unavailable")
            );
            assert!(
                combined
                    .chapters()
                    .next()
                    .unwrap()
                    .content
                    .contains("missing or unreadable coverage")
            );
            assert!(
                combined
                    .chapters()
                    .next()
                    .unwrap()
                    .content
                    .contains("2 selected test attempts")
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
                .contains("gap counts unavailable")
        );
        assert!(
            stale
                .chapters()
                .next()
                .unwrap()
                .content
                .contains("stale coverage; recollect")
        );
        let saved = load_saved(root, Some(root.join("target")));
        assert!(
            saved
                .records
                .iter()
                .all(|record| record.run.sources["code.rs"].contains("fn one() {}"))
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
        assert!(
            other
                .chapters()
                .next()
                .unwrap()
                .content
                .contains("1 selected test attempts")
        );
    }

    // {test}`requirements.test-coverage.index.summary`
    // {test}`requirements.test-coverage.index.details`
    // {test}`requirements.test-coverage.processes.reports`
    #[test]
    fn gaps_omit_test_data_and_preserve_saved_profiles_and_sources() {
        let (temp, book) = fixture();
        let root = temp.path();
        fs::write(root.join("passing.rs"), "fn passing() {}\n").unwrap();
        let first = record(root, "one", 100, "a", &[(1, 1), (2, 0)]);
        fs::write(first.join("2-2.profraw"), "child profile fixture").unwrap();
        let mut attempt: Attempt = load(&first.join("attempt.json")).unwrap();
        attempt.processes.push(Process {
            functions: Some(vec![]),
            pid: 2,
            profiles: vec!["2-2.profraw".into()],
            lines: BTreeMap::from([("code.rs".into(), BTreeMap::from([(1, 0), (2, 7)]))]),
            diagnostic: None,
        });
        // A passing file must not appear in the gaps table.
        attempt.processes[0]
            .lines
            .insert("passing.rs".into(), BTreeMap::from([(1, 1)]));
        attempt.processes[0]
            .functions
            .as_mut()
            .unwrap()
            .push(super::super::FunctionExecution {
                file: "passing.rs".into(),
                line: 1,
                column: 1,
                count: 1,
            });
        save(&first.join("attempt.json"), &attempt).unwrap();
        let run_path = first.parent().unwrap().parent().unwrap().join("run.json");
        let mut run: Run = load(&run_path).unwrap();
        run.sources
            .insert("passing.rs".into(), "fn passing() {}\n".into());
        save(&run_path, &run).unwrap();
        record(root, "two", 200, "a", &[(1, 1), (2, 0)]);
        let captured_attempt = fs::read(first.join("attempt.json")).unwrap();
        let captured_run = fs::read(&run_path).unwrap();
        let mut combined = book.clone();
        populate(&mut combined, root).unwrap();
        let index = combined.chapters().next().unwrap();
        assert!(index.content.contains("3/3 (100.0%)"));
        assert!(
            index
                .content
                .contains("not establish that all current tests were measured")
        );
        assert!(!map_html(&index.content).contains("data-file=\"passing.rs\""));
        assert!(
            combined
                .chapters()
                .all(|chapter| chapter.name != "File notes: passing.rs")
        );
        for chapter in combined.chapters() {
            for omitted in [
                "Test process coverage",
                "Captured source",
                "habeas-coverage-test",
                "Test runs and provenance",
                "application/json",
                "habeasRenderCoverage",
                "binary / one",
                "binary / two",
                "fn passing() {}",
            ] {
                assert!(
                    !chapter.content.contains(omitted),
                    "report includes {omitted}"
                );
            }
            assert!(!chapter.name.starts_with("binary /"));
            assert!(
                !chapter
                    .path
                    .as_ref()
                    .unwrap()
                    .to_string_lossy()
                    .ends_with("-source.md")
            );
            assert!(chapter.name == "Coverage gaps" || chapter.name.starts_with("File notes:"));
        }
        fs::write(root.join("code.rs"), "rebuilt source\n").unwrap();
        let mut historical = book;
        populate(&mut historical, root).unwrap();
        assert!(
            historical
                .chapters()
                .next()
                .unwrap()
                .content
                .contains("Coverage unavailable")
        );
        assert_eq!(
            fs::read(first.join("attempt.json")).unwrap(),
            captured_attempt
        );
        assert_eq!(fs::read(&run_path).unwrap(), captured_run);
        assert_eq!(
            fs::read_to_string(first.join("1-1.profraw")).unwrap(),
            "retained profile fixture"
        );
        assert_eq!(
            fs::read_to_string(first.join("2-2.profraw")).unwrap(),
            "child profile fixture"
        );
        // Archived reports use the saved source, even though freshness excludes
        // these attempts from the current workspace's gaps report.
        let mut saved = load_saved(root, Some(root.join("target")));
        let archived = saved
            .records
            .iter_mut()
            .find(|record| record.attempt.test == "one")
            .unwrap();
        assert!(!archived.fresh);
        archived.load_measurements().unwrap();
        let captured_lines = |locations: BTreeMap<(String, u32), u64>| {
            locations
                .into_iter()
                .map(|((file, line), count)| {
                    let source_line = archived.run.sources[&file]
                        .lines()
                        .nth(line as usize - 1)
                        .unwrap();
                    ((file, line), (source_line.to_owned(), count))
                })
                .collect::<BTreeMap<_, _>>()
        };
        let individual = |process: &Process| {
            captured_lines(
                process
                    .lines
                    .iter()
                    .flat_map(|(file, lines)| {
                        lines
                            .iter()
                            .map(move |(&line, &count)| ((file.clone(), line), count))
                    })
                    .collect(),
            )
        };
        let parent = individual(&archived.attempt.processes[0]);
        let child = individual(&archived.attempt.processes[1]);
        let combined = captured_lines(union(std::iter::once(&*archived)));
        assert_eq!(parent[&("code.rs".into(), 1)], ("fn one() {}".into(), 1));
        assert_eq!(parent[&("code.rs".into(), 2)], ("fn two() {}".into(), 0));
        assert_eq!(child[&("code.rs".into(), 1)], ("fn one() {}".into(), 0));
        assert_eq!(child[&("code.rs".into(), 2)], ("fn two() {}".into(), 7));
        assert_eq!(combined[&("code.rs".into(), 2)], ("fn two() {}".into(), 7));
        assert_eq!(parent.values().filter(|(_, count)| *count > 0).count(), 2);
        assert_eq!(combined.values().filter(|(_, count)| *count > 0).count(), 3);
        assert!(
            combined
                .values()
                .all(|(source, _)| !source.contains("rebuilt source"))
        );
    }

    // {test}`requirements.test-coverage.index.paths`
    // {test}`requirements.test-coverage.index.unavailable`
    #[test]
    fn unavailable_runs_are_readable_and_generated_paths_cannot_collide() {
        let (temp, mut collision) = fixture();
        collision.push_item(Chapter::new(
            "Authored",
            "Keep me".into(),
            "reports/coverage.md",
            vec![],
        ));
        assert!(
            populate(&mut collision, temp.path())
                .unwrap_err()
                .to_string()
                .contains("coverage index path conflicts")
        );
        let (temp, mut book) = fixture();
        populate(&mut book, temp.path()).unwrap();
        assert!(
            book.chapters()
                .next()
                .unwrap()
                .content
                .contains("Run `habeas coverage`")
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
    }

    // {test}`requirements.test-coverage.index.selection`
    // {test}`requirements.test-coverage.index.unavailable`
    // {test}`requirements.test-coverage.index.freshness`
    // {test}`requirements.test-coverage.storage.outcomes`
    // {test}`requirements.test-coverage.index.file_map.combined.measurement`
    #[test]
    fn unreadable_history_cannot_reintroduce_an_older_success() {
        let (temp, mut book) = fixture();
        record(temp.path(), "one", 1, "a", &[(1, 1)]);
        let newest = record(temp.path(), "one", 2, "a", &[(1, 0)]);
        fs::write(newest.join("attempt.json"), "not JSON").unwrap();
        populate(&mut book, temp.path()).unwrap();
        let index = &book.chapters().next().unwrap().content;
        assert!(index.contains("cannot establish latest-per-test selection"));
        assert!(index.contains("gap counts unavailable"));
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
                .contains("missing or unreadable coverage")
        );
        assert!(
            book.chapters()
                .next()
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
        assert!(index.contains("Coverage history is incomplete"));
        assert!(index.contains("gap counts unavailable"));
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
                .contains("missing or unreadable coverage")
        );
        assert!(
            book.chapters()
                .next()
                .unwrap()
                .content
                .contains("gap counts unavailable")
        );
        assert!(
            book.chapters()
                .next()
                .unwrap()
                .content
                .contains("A retained raw profile is missing")
        );
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

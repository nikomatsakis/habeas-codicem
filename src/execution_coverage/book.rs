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

// {impl}`requirements.test-coverage.index.file_map.directories`
#[derive(Default)]
struct GapDirectory<'a> {
    files: Vec<&'a super::notes::DocumentedFile>,
    directories: BTreeMap<String, GapDirectory<'a>>,
}

impl GapDirectory<'_> {
    // {cite}`habeas:util`
    fn count(&self) -> usize {
        self.files
            .iter()
            .map(|file| file.items.len())
            .sum::<usize>()
            + self.directories.values().map(Self::count).sum::<usize>()
    }

    // {impl}`requirements.test-coverage.index.gaps.details`
    // {impl}`requirements.test-coverage.index.gaps.details.initial`
    // {impl}`requirements.test-coverage.index.gaps.explanations`
    fn render(&self, html: &mut String, folder: &str, index: &Path, next: &mut usize) {
        if !self.files.is_empty() {
            html.push_str("<table class=\"gap-file-table\"><colgroup><col><col><col></colgroup><thead><tr><th>File</th><th>Documented source items</th><th>Notes</th></tr></thead><tbody>\n");
            for file in &self.files {
                let id = format!("habeas-gap-file-{}", *next);
                *next += 1;
                let notes =
                    crate::chapter_href(index, &super::notes::path(index, &file.file), None);
                html.push_str(&format!("<tr class=\"gap-file-row habeas-coverage-file\" data-file=\"{}\"><td><button class=\"gap-file-button\" aria-expanded=\"false\" aria-controls=\"{id}\" onclick=\"const row=document.getElementById(this.getAttribute('aria-controls'));row.hidden=!row.hidden;this.setAttribute('aria-expanded',String(!row.hidden));this.querySelector('.gap-chevron').textContent=row.hidden?'▸':'▾'\"><span class=\"gap-chevron\" aria-hidden=\"true\">▸</span><code>{}</code></button></td><td>{}</td><td><a href=\"{}\">Current source notes</a></td></tr>\n", escaped(&file.file), escaped(&file.file), file.items.len(), escaped(&notes)));
                html.push_str(&format!(
                    "<tr class=\"gap-detail-row\" id=\"{id}\" hidden><td colspan=\"3\">"
                ));
                for (offset, item) in file.items.iter().enumerate() {
                    html.push_str(&format!("<div class=\"gap-explanation\"><p><a href=\"{}#coverage-note-{}\"><code>{}:{} #{}</code></a></p>{}</div>\n", escaped(&notes), offset + 1, escaped(&file.file), item.line, escaped(if item.selector.is_empty() { "File root" } else { &item.selector }), item.html.replace('\n', "&#10;")));
                }
                html.push_str("</td></tr>\n");
            }
            html.push_str("</tbody></table>\n");
        }
        for (name, directory) in &self.directories {
            let path = if folder.is_empty() {
                name.clone()
            } else {
                format!("{folder}/{name}")
            };
            html.push_str(&format!("<details class=\"gap-directory\" data-directory=\"{}\" open><summary><code>{}/</code><span class=\"gap-meta\">{} documented source items</span></summary>\n", escaped(&path), escaped(&path), directory.count()));
            directory.render(html, &path, index, next);
            html.push_str("</details>\n");
        }
    }
}

// {impl}`requirements.test-coverage.index.file_map.combined`
// {impl}`requirements.test-coverage.index.file_map.table`
// {impl}`requirements.test-coverage.index.file_map.directories.initial`
// {impl}`requirements.test-coverage.index.file_map.directories.data`
// {impl}`requirements.test-coverage.index.gaps`
// {impl}`requirements.test-coverage.index.gaps.summary`
// {impl}`requirements.test-coverage.index.gaps.accepted`
// {impl}`requirements.test-coverage.index.file_map.notes`
fn gaps_map(files: &[super::notes::DocumentedFile], index: &Path) -> String {
    let mut tree = GapDirectory::default();
    for file in files.iter().filter(|file| !file.items.is_empty()) {
        let mut directory = &mut tree;
        let parts: Vec<_> = file.file.split('/').collect();
        for part in &parts[..parts.len() - 1] {
            directory = directory.directories.entry((*part).to_owned()).or_default();
        }
        directory.files.push(file);
    }
    let count = tree.count();
    let mut html = format!(
        "<style>{}</style>\n<section class=\"habeas-coverage-map habeas-coverage-gaps\" aria-label=\"Documented coverage gaps\">\n<p class=\"gap-meta\">{count} documented source items carrying coverage annotations</p>\n",
        include_str!("../../assets/coverage-gaps.css")
    );
    tree.render(&mut html, "", index, &mut 0);
    html.push_str("</section>\n\n");
    html
}

// {impl}`requirements.test-coverage.index`
// {impl}`requirements.test-coverage.index.paths`
// {impl}`requirements.test-coverage.index.unavailable`
// {impl}`requirements.test-coverage.index.render_only`
// {impl}`requirements.test-coverage.index.details`
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
}

// {impl}`requirements.test-coverage.index.selection`
// {impl}`requirements.test-coverage.index.freshness`
// {impl}`requirements.test-coverage.check.measurement`
pub(super) fn load_saved(project: &Path, target: Option<PathBuf>) -> SavedCoverage {
    let mut diagnostics = Vec::new();
    let mut uncertain_history = false;
    let mut records = Vec::new();
    let workspace = workspace_root(project);
    if let Ok(root) = &workspace {
        let mut errors = Vec::new();
        let _ = crate::evidence::discovered_files(root, &mut errors);
        diagnostics.extend(errors);
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
    }
}

// {impl}`requirements.test-coverage.index.navigation`
// {impl}`requirements.test-coverage.index.source`
// {impl}`requirements.test-coverage.index.guidance`
// {impl}`requirements.test-coverage.index.details`
// {impl}`requirements.test-coverage.index.file_map.test`
// {impl}`requirements.test-coverage.index.file_map.combined.details`
pub(super) fn populate_in(
    book: &mut Book,
    project: &Path,
    _target: Option<PathBuf>,
    parent: &Path,
) -> io::Result<()> {
    let index_path = parent.with_extension("").join("coverage.md");
    let root = workspace_root(project).unwrap_or_else(|_| project.to_path_buf());
    let mut diagnostics = Vec::new();
    let discovered = crate::evidence::discovered_files(&root, &mut diagnostics);
    // Deleted indexed sources cannot reveal their annotations. Report them
    // here without changing the discovery used by status and author.
    if let Ok(sources) = crate::authoring::access::load_discovery_sources(&root) {
        for file in sources.indexed().into_iter().flatten() {
            if sources.considers(file, false)
                && fs::symlink_metadata(file)
                    .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
            {
                diagnostics.push(format!(
                    "{}: current source is missing",
                    file.strip_prefix(&root).unwrap_or(file).display()
                ));
            }
        }
    }
    let note_files = discovered
        .into_iter()
        .filter_map(|file| {
            file.strip_prefix(&root)
                .ok()
                .map(|path| path.to_string_lossy().into_owned())
        })
        .collect();
    let notes = super::notes::pages(&root, &note_files, &index_path);
    for file in &notes {
        diagnostics.extend(file.diagnostics.iter().cloned());
    }
    diagnostics.sort();
    diagnostics.dedup();
    let mut summary = String::from(
        "# Documented coverage gaps\n\nThese annotations explain deliberate coverage gaps. `habeas status` and `habeas author` check annotations and reviews; `habeas coverage-check` enforces measured coverage. Reviews below show recorded results.\n\n",
    );
    summary.push_str(&gaps_map(&notes, &index_path));
    if notes.iter().all(|file| file.items.is_empty()) && diagnostics.is_empty() {
        summary.push_str("No documented coverage gaps are present.\n\n");
    }
    for diagnostic in diagnostics {
        summary.push_str(&format!(
            "**Source notes unavailable:** {}\n\n",
            escaped(&crate::redaction::redact_text(&diagnostic))
        ));
    }
    let existing: BTreeSet<_> = book
        .chapters()
        .filter_map(|chapter| chapter.path.clone())
        .collect();
    let mut generated = BTreeSet::new();
    reserve_path(&existing, &mut generated, &index_path, "index")?;
    let mut details = Vec::new();
    for file in notes
        .into_iter()
        .filter(|file| !file.items.is_empty() || !file.diagnostics.is_empty())
    {
        let chapter = file.chapter;
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
    let mut coverage = Some(Chapter::new(
        "Documented coverage gaps",
        summary,
        &index_path,
        vec![],
    ));
    let mut found = false;
    book.for_each_chapter_mut(|chapter| {
        if chapter.path.as_deref() == Some(parent) {
            found = true;
            chapter.content.push_str(&format!(
                "\n\n[Documented coverage gaps]({}) — source annotations and recorded reviews.\n",
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

    // {cite}`habeas:util`
    fn saved_coverage(root: &Path) -> SavedCoverage {
        load_saved(root, Some(root.join("target")))
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
            fs::write(
                temp.path().join("code.rs"),
                "// {cite}`habeas:coverage`\nfn one() {}\n",
            )
            .unwrap();
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
                    .contains("[Documented coverage gaps](reports/coverage.html)")
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
                    ["Reports", "Requirements index", "Documented coverage gaps"]
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
                .filter(|chapter| chapter.name == "Documented coverage gaps")
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
                matches!(&parent.sub_items[0], BookItem::Chapter(child) if child.name == "Documented coverage gaps")
            );
            let BookItem::Chapter(authored) = &book.items[1] else {
                unreachable!()
            };
            assert_eq!(authored.content, content);
            assert!(authored.sub_items.is_empty());
        }
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

    // {test}`requirements.test-coverage.storage.provenance`
    // {test}`requirements.test-coverage.index.freshness`
    #[test]
    fn cargo_launch_metadata_does_not_invalidate_but_caller_build_environment_does() {
        let (temp, _) = fixture();
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
        assert!(
            saved_coverage(root)
                .records
                .iter()
                .all(|record| record.fresh)
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
            assert!(
                saved_coverage(root)
                    .records
                    .iter()
                    .all(|record| !record.fresh),
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
        let (temp, _) = fixture();
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
        let saved = load_saved(temp.path(), Some(temp.path().join("target")));
        assert_eq!(saved.selected.len(), 2);
        let binaries = saved
            .selected
            .iter()
            .map(|&i| saved.records[i].attempt.binary.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(binaries, BTreeSet::from(["first", "second"]));

        let (temp, _) = fixture();
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
        let saved = load_saved(temp.path(), Some(temp.path().join("target")));
        let tests = saved
            .selected
            .iter()
            .map(|&i| saved.records[i].attempt.test.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(tests, BTreeSet::from(["one", "two"]));

        // A retry in the same run must also use attempt-start ordering.
        let (temp, _) = fixture();
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
        let saved = saved_coverage(temp.path());
        assert_eq!(
            union(saved.selected.iter().map(|&index| &saved.records[index])),
            BTreeMap::from([(("code.rs".into(), 1), 1)])
        );
    }

    // {test}`requirements.test-coverage.index.selection`
    // {test}`requirements.test-coverage.index.freshness`
    // {test}`requirements.test-coverage.index.summary`
    // {test}`requirements.test-coverage.check.measurement`
    #[test]
    fn filtered_runs_union_latest_attempts_and_invalidate_with_saved_source() {
        let (temp, _) = fixture();
        let root = temp.path();
        record(root, "one", 1, "build-a", &[(1, 1), (2, 0)]);
        record(root, "two", 2, "build-a", &[(1, 0), (2, 1)]);
        let selected = saved_coverage(root);
        assert_eq!(selected.selected.len(), 2);
        let counts = union(
            selected
                .selected
                .iter()
                .map(|&index| &selected.records[index]),
        );
        assert_eq!(
            counts,
            BTreeMap::from([(("code.rs".into(), 1), 1), (("code.rs".into(), 2), 1)])
        );
        let newest = record(root, "one", 3, "build-a", &[]);
        let mut attempt: Attempt = load(&newest.join("attempt.json")).unwrap();
        attempt.diagnostics.push("profile missing".into());
        for outcome in ["failed", "interrupted"] {
            attempt.outcome = outcome.into();
            save(&newest.join("attempt.json"), &attempt).unwrap();
            let selected = saved_coverage(root);
            assert_eq!(selected.selected.len(), 2);
            let report = super::super::check::check_saved(root, &selected).unwrap();
            assert!(!report.passed);
            assert!(report.text.contains("missing or unreadable coverage"));
        }
        fs::write(root.join("code.rs"), "changed source\n").unwrap();
        let selected = saved_coverage(root);
        assert!(selected.records.iter().all(|record| !record.fresh));
        assert!(
            super::super::check::check_saved(root, &selected)
                .unwrap()
                .text
                .contains("stale coverage")
        );
        assert!(
            selected
                .records
                .iter()
                .all(|record| record.run.sources["code.rs"].contains("fn one() {}"))
        );
        fs::write(root.join("code.rs"), "fn one() {}\nfn two() {}\n").unwrap();
        record(root, "one", 4, "build-b", &[(1, 1)]);
        let selected = saved_coverage(root);
        assert_eq!(selected.selected.len(), 1);
        assert!(
            selected
                .selected
                .iter()
                .all(|&index| selected.records[index].run.configuration == "build-b")
        );
    }

    // {test}`requirements.test-coverage.index.summary`
    // {test}`requirements.test-coverage.processes.reports`
    #[test]
    fn saved_reports_preserve_profiles_sources_and_child_union() {
        let (temp, _) = fixture();
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
        let selected = saved_coverage(root);
        let counts = union(
            selected
                .selected
                .iter()
                .map(|&index| &selected.records[index]),
        );
        assert_eq!(counts.len(), 3);
        assert!(counts.values().all(|&count| count > 0));
        fs::write(root.join("code.rs"), "rebuilt source\n").unwrap();
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

    // {test}`requirements.test-coverage.index.freshness`
    // {test}`requirements.test-coverage.index.selection`
    // {test}`requirements.test-coverage.check.measurement`
    #[test]
    fn unreadable_history_cannot_reintroduce_an_older_success() {
        for failure in [
            "invalid metadata",
            "missing source",
            "unsupported schema",
            "missing profile",
        ] {
            let (temp, _) = fixture();
            let root = temp.path();
            record(root, "one", 1, "same", &[(1, 1)]);
            let newest = record(root, "one", 2, "same", &[(1, 0)]);
            match failure {
                "invalid metadata" => fs::write(newest.join("attempt.json"), "not JSON").unwrap(),
                "missing source" => {
                    let run_path = newest.parent().unwrap().parent().unwrap().join("run.json");
                    let mut run: Run = load(&run_path).unwrap();
                    run.sources.clear();
                    save(&run_path, &run).unwrap();
                }
                "unsupported schema" => {
                    let run_path = newest.parent().unwrap().parent().unwrap().join("run.json");
                    let mut run: Run = load(&run_path).unwrap();
                    run.schema = 99;
                    save(&run_path, &run).unwrap();
                }
                _ => fs::remove_file(newest.join("1-1.profraw")).unwrap(),
            }
            let selected = saved_coverage(root);
            assert!(
                !super::super::check::check_saved(root, &selected)
                    .unwrap()
                    .passed,
                "{failure}"
            );
            assert!(
                selected.uncertain_history
                    || !union(
                        selected
                            .selected
                            .iter()
                            .map(|&index| &selected.records[index])
                    )
                    .values()
                    .any(|&count| count > 0),
                "{failure}"
            );
        }
    }

    // {test}`requirements.test-coverage.check.measurement`
    #[test]
    fn saved_run_data_failures_produce_cli_diagnostics() {
        for case in [
            "missing run",
            "corrupt run",
            "missing attempts",
            "run diagnostic",
        ] {
            let (temp, _) = fixture();
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
            let saved = saved_coverage(temp.path());
            let index = saved.diagnostics.join("\n");
            assert!(!index.is_empty(), "{case}");
            if case == "missing run" || case == "corrupt run" {
                assert!(index.contains("unavailable run data"), "{case}");
            } else if case == "missing attempts" {
                assert!(index.contains("attempts"), "{index}");
            } else {
                assert!(index.contains("(interrupted): collection was interrupted"));
            }
        }
    }

    // {test}`requirements.test-coverage.check.measurement`
    #[cfg(unix)]
    #[test]
    fn unreadable_storage_is_a_cli_diagnostic() {
        use std::os::unix::fs::PermissionsExt;
        let (temp, _) = fixture();
        record(temp.path(), "one", 1, "a", &[(1, 1)]);
        let storage = temp.path().join("target/habeas/coverage");
        fs::set_permissions(&storage, fs::Permissions::from_mode(0o000)).unwrap();
        let selected = saved_coverage(temp.path());
        fs::set_permissions(storage, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            selected
                .diagnostics
                .iter()
                .any(|error| error.contains("Coverage storage is unreadable"))
        );
        assert!(
            !super::super::check::check_saved(temp.path(), &selected)
                .unwrap()
                .passed
        );
    }
    // {test}`requirements.test-coverage.index.file_map`
    // {test}`requirements.test-coverage.index.file_map.table`
    // {test}`requirements.test-coverage.index.file_map.combined`
    // {test}`requirements.test-coverage.index.file_map.directories`
    // {test}`requirements.test-coverage.index.file_map.directories.initial`
    // {test}`requirements.test-coverage.index.file_map.directories.data`
    // {test}`requirements.test-coverage.index.gaps.summary`
    // {test}`requirements.test-coverage.index.gaps.details`
    // {test}`requirements.test-coverage.index.gaps.details.initial`
    // {test}`requirements.test-coverage.index.gaps.explanations`
    // {test}`requirements.test-coverage.index.details`
    // {test}`requirements.test-coverage.index.file_map.notes`
    #[test]
    fn documented_items_are_counted_once_with_browsable_directory_and_file_groups() {
        let (temp, mut book) = fixture();
        let root = temp.path();
        for (file, source) in [
            (
                "code.rs",
                "//! :::{cite} habeas:coverage\n//! Root explanation.\n//! :::\n// :::{cite} habeas:coverage\n// First explanation.\n// :::\n// :::{cite} habeas:coverage\n// Second explanation.\n// :::\nfn one() {}\n",
            ),
            (
                "src/direct.rs",
                "/// Function explanation.\n// {cite}`habeas:coverage`\nfn direct() {}\n// {cite}`habeas:coverage`\nfn second() {}\n",
            ),
            (
                "src/deep/nested.rs",
                "// {cite}`habeas:coverage`\nfn unexplained() {}\n",
            ),
            ("src/pass.rs", "fn passing() {}\n"),
            (
                "vendor/foreign.rs",
                "// {cite}`habeas:coverage`\nfn foreign() {}\n",
            ),
            (
                "docs_only/deep/item.rs",
                "// {cite}`habeas:coverage`\nfn nested() {}\n",
            ),
        ] {
            fs::create_dir_all(root.join(file).parent().unwrap()).unwrap();
            fs::write(root.join(file), source).unwrap();
        }
        populate(&mut book, root).unwrap();
        let html = map_html(&book.chapters().next().unwrap().content);
        assert!(!html.contains("\n\n"));
        assert!(html.contains("7 documented source items carrying coverage annotations"));
        for (directory, count) in [
            ("src", 3),
            ("src/deep", 1),
            ("vendor", 1),
            ("docs_only", 1),
            ("docs_only/deep", 1),
        ] {
            let group = directory_group(html, directory);
            assert!(group.starts_with(&format!(
                "<details class=\"gap-directory\" data-directory=\"{directory}\" open>"
            )));
            assert!(
                group
                    .split("</summary>")
                    .next()
                    .unwrap()
                    .contains(&format!("{count} documented source items"))
            );
        }
        assert_directory_contents(html, "src", &["src/deep"], &["src/direct.rs"]);
        assert_directory_contents(html, "src/deep", &[], &["src/deep/nested.rs"]);
        assert_directory_contents(html, "vendor", &[], &["vendor/foreign.rs"]);
        assert_directory_contents(html, "docs_only", &["docs_only/deep"], &[]);
        assert_directory_contents(html, "docs_only/deep", &[], &["docs_only/deep/item.rs"]);
        let ancestor = directory_group(html, "docs_only");
        assert_eq!(
            ancestor
                .split("<details class=\"gap-directory\" data-directory=\"docs_only/deep\"")
                .next()
                .unwrap()
                .matches("data-file=")
                .count(),
            0
        );
        assert!(
            html.find("data-file=\"code.rs\"").unwrap() < html.find("data-directory=").unwrap()
        );
        let root_file = html.find("data-file=\"code.rs\"").unwrap();
        let root_row = html[..root_file].rfind("<tr").unwrap();
        let root_opening = html[root_row..].split('>').next().unwrap();
        assert!(!root_opening.contains(" hidden"), "{root_opening}");
        assert!(!html[..root_row].contains("<details"));
        assert!(!html[..root_row].contains(" hidden"));
        for file in ["code.rs", "src/direct.rs", "src/deep/nested.rs"] {
            let card = file_card(html, file);
            assert!(card.contains("aria-expanded=\"false\""));
            assert!(card.contains(" hidden>"));
            let note_path = super::super::notes::path(Path::new("reports/coverage.md"), file);
            let expected_link =
                crate::chapter_href(Path::new("reports/coverage.md"), &note_path, None);
            assert!(card.contains(&format!("href=\"{expected_link}\"")));
            let expected_count = if file == "src/deep/nested.rs" { 1 } else { 2 };
            assert!(card.contains(&format!("</button></td><td>{expected_count}</td>")));
            let notes = book
                .chapters()
                .find(|chapter| chapter.name == format!("File notes: {file}"))
                .unwrap();
            assert_eq!(notes.path.as_ref(), Some(&note_path));
            assert!(
                notes
                    .content
                    .contains(&format!("File: <code>{file}</code>"))
            );
            for ordinal in 1..=expected_count {
                assert!(
                    card.contains(&format!("href=\"{expected_link}#coverage-note-{ordinal}\""))
                );
                assert!(
                    notes
                        .content
                        .contains(&format!("id=\"coverage-note-{ordinal}\""))
                );
            }
            let explanation = match file {
                "code.rs" => "Root explanation.",
                "src/direct.rs" => "Function explanation.",
                _ => "No tracked justification is present.",
            };
            assert!(card.contains(explanation));
            let locations: &[(&str, usize)] = match file {
                "code.rs" => &[("File root", 1), ("fn[one]", 4)],
                "src/direct.rs" => &[("fn[direct]", 1), ("fn[second]", 4)],
                _ => &[("fn[unexplained]", 1)],
            };
            for (selector, line) in locations {
                assert!(
                    card.contains(&format!("<code>{file}:{line} #{selector}</code>")),
                    "{file}: {card}"
                );
            }
        }
        for explanation in [
            "Root explanation.",
            "First explanation.",
            "Second explanation.",
            "Function explanation.",
            "No tracked justification is present.",
            "Recorded review:",
        ] {
            assert!(html.contains(explanation), "{explanation}");
        }
        assert!(!html.contains("data-file=\"src/pass.rs\""));
        for absent in [
            "CI would",
            "instrumented lines covered",
            "gap-percentage",
            "gap-verdict",
            "data-status=",
        ] {
            assert!(!html.contains(absent));
        }

        assert!(html.contains("vendor/foreign.rs"));
        assert_eq!(
            book.chapters()
                .filter(|page| page.name.starts_with("File notes:"))
                .count(),
            5
        );
    }

    // {test}`requirements.test-coverage.index.file_map`
    // {test}`requirements.test-coverage.index.unavailable`
    // {test}`requirements.test-coverage.index.file_map.notes.render_only`
    // {test}`requirements.index.render_only`
    // {test}`requirements.test-coverage.index.render_only`
    // {test}`requirements.test-coverage.index.gaps`
    // {test}`requirements.test-coverage.index.gaps.accepted`
    // {test}`requirements.test-coverage.index.file_map.test`
    // {test}`requirements.test-coverage.index.file_map.combined.details`
    // {test}`requirements.test-coverage.index.details`
    #[test]
    fn book_ignores_saved_measurements_and_preserves_artifacts() {
        let (temp, book) = fixture();
        let root = temp.path();
        fs::write(
            root.join("code.rs"),
            "/// Documented platform gap.\n// {cite}`habeas:coverage`\nfn other() {}\n",
        )
        .unwrap();
        let mut empty = book.clone();
        populate(&mut empty, root).unwrap();
        let original: Vec<_> = empty
            .chapters()
            .map(|chapter| (chapter.path.clone(), chapter.content.clone()))
            .collect();
        assert!(!root.join("target").exists());
        let attempt = record(
            root,
            "saved-test-name-should-never-appear",
            1,
            "same",
            &[(3, 1)],
        );
        let mut valid = book.clone();
        populate(&mut valid, root).unwrap();
        assert_eq!(
            valid
                .chapters()
                .map(|chapter| (chapter.path.clone(), chapter.content.clone()))
                .collect::<Vec<_>>(),
            original
        );
        assert_eq!(
            valid
                .chapters()
                .map(|chapter| chapter.name.as_str())
                .collect::<Vec<_>>(),
            ["Documented coverage gaps", "File notes: code.rs"]
        );
        for chapter in valid.chapters() {
            assert!(
                !chapter
                    .content
                    .contains("saved-test-name-should-never-appear")
            );
            assert!(!chapter.content.contains("<select"));
            assert!(
                !chapter
                    .path
                    .as_ref()
                    .unwrap()
                    .to_string_lossy()
                    .contains("-source")
            );
        }
        for contents in ["invalid metadata", "{}"] {
            fs::write(attempt.join("attempt.json"), contents).unwrap();
            let mut rendered = book.clone();
            populate(&mut rendered, root).unwrap();
            assert_eq!(
                rendered
                    .chapters()
                    .map(|chapter| (chapter.path.clone(), chapter.content.clone()))
                    .collect::<Vec<_>>(),
                original
            );
            assert_eq!(
                fs::read_to_string(attempt.join("attempt.json")).unwrap(),
                contents
            );
            assert!(attempt.join("1-1.profraw").exists());
            assert!(
                !root
                    .join("target/habeas/coverage/selection-cache.json")
                    .exists()
            );
        }
        let overview = &empty.chapters().next().unwrap().content;
        assert!(overview.starts_with("# Documented coverage gaps"));
        assert!(
            overview.contains("`habeas status` and `habeas author` check annotations and reviews")
        );
        assert!(overview.contains("`habeas coverage-check` enforces measured coverage"));

        for omitted in [
            "CI would",
            "selected test attempts",
            "Line coverage:",
            "Captured source",
            "Test runs and provenance",
            "habeas-coverage-test",
            "0 unresolved gaps",
        ] {
            assert!(!overview.contains(omitted), "{omitted}");
        }
    }

    // {test}`requirements.test-coverage.index.unavailable`
    // {test}`requirements.test-coverage.index.gaps.summary`
    #[test]
    fn empty_documentation_and_source_failures_are_distinct() {
        let (temp, book) = fixture();
        let root = temp.path();
        let mut empty = book.clone();
        populate(&mut empty, root).unwrap();
        assert!(
            empty
                .chapters()
                .next()
                .unwrap()
                .content
                .contains("No documented coverage gaps are present.")
        );
        fs::write(root.join("broken.rs"), "fn broken(").unwrap();
        let mut broken = book.clone();
        populate(&mut broken, root).unwrap();
        let content = &broken.chapters().next().unwrap().content;
        assert!(content.contains("Source notes unavailable"));
        assert!(content.contains("broken.rs"));
        assert!(!content.contains("No documented coverage gaps are present."));
        assert!(
            broken
                .chapters()
                .any(|chapter| chapter.name == "File notes: broken.rs")
        );
        fs::remove_file(root.join("broken.rs")).unwrap();
        git2::Repository::init(root).unwrap();
        fs::write(root.join(".git/index"), "not an index").unwrap();
        let mut failure = book;
        populate(&mut failure, root).unwrap();
        let content = &failure.chapters().next().unwrap().content;
        assert!(content.contains("cannot discover reference sources"));
        assert!(!content.contains("No documented coverage gaps are present."));
    }

    // {test}`requirements.test-coverage.index.file_map.table`
    #[test]
    fn documented_paths_and_explanations_escape_html() {
        let (temp, mut book) = fixture();
        fs::write(
            temp.path().join("a<&>.rs"),
            "/// Explanation with <unsafe> text.\n// {cite}`habeas:coverage`\nfn other() {}\n",
        )
        .unwrap();
        populate(&mut book, temp.path()).unwrap();
        let html = map_html(&book.chapters().next().unwrap().content);
        assert!(html.contains("a&lt;&amp;&gt;.rs"));
        assert!(html.contains("&lt;unsafe&gt;"));
        assert!(!html.contains("<unsafe>"));
    }

    // {test}`requirements.test-coverage.check.measurement`
    // {test}`requirements.test-coverage.index.summary`
    #[test]
    fn saved_measurement_failures_remain_unavailable_to_cli_checks() {
        let (temp, _) = fixture();
        let root = temp.path();
        assert!(
            !super::super::check::check_saved(root, &saved_coverage(root))
                .unwrap()
                .passed
        );
        let dir = record(root, "one", 1, "same", &[(1, 0), (2, 0)]);
        let original: Attempt = load(&dir.join("attempt.json")).unwrap();
        assert_eq!(
            union(saved_coverage(root).records.iter())
                .values()
                .filter(|&&count| count > 0)
                .count(),
            0
        );
        for failure in [
            "no processes",
            "attempt diagnostic",
            "process diagnostic",
            "missing source",
            "empty profile",
            "missing child",
        ] {
            let mut attempt = original.clone();
            match failure {
                "no processes" => attempt.processes.clear(),
                "attempt diagnostic" => attempt.diagnostics.push("incomplete export".into()),
                "process diagnostic" => {
                    attempt.processes[0].diagnostic = Some("incomplete export".into())
                }
                "missing source" => {
                    attempt.processes[0]
                        .lines
                        .insert("uncaptured.rs".into(), BTreeMap::from([(1, 1)]));
                }
                "empty profile" => fs::write(dir.join("1-1.profraw"), "").unwrap(),
                _ => {
                    let mut child = attempt.processes[0].clone();
                    child.profiles = vec!["missing-child.profraw".into()];
                    attempt.processes.push(child);
                }
            }
            save(&dir.join("attempt.json"), &attempt).unwrap();
            let report = super::super::check::check_saved(root, &saved_coverage(root)).unwrap();
            assert!(!report.passed, "{failure}");
            assert!(
                report.text.contains("missing or unreadable coverage"),
                "{failure}: {}",
                report.text
            );
            fs::write(dir.join("1-1.profraw"), "profile fixture").unwrap();
        }
        save(&dir.join("attempt.json"), &original).unwrap();
        let empty = record(root, "one", 2, "same", &[]);
        assert!(
            super::super::check::check_saved(root, &saved_coverage(root))
                .unwrap()
                .text
                .contains("No usable instrumented lines")
        );
        fs::remove_file(empty.join("attempt.json")).unwrap();
    }
    // {test}`requirements.test-coverage.index.file_map.combined`
    // {test}`requirements.test-coverage.index.file_map.notes.justifications`
    #[test]
    fn documentation_includes_non_rust_functions_methods_and_plain_file_roots() {
        let (temp, mut book) = fixture();
        let root = temp.path();
        for (file, source) in [
            (
                "worker.py",
                "# :::{cite} habeas:coverage\n# Python platform explanation.\n# :::\ndef work():\n    pass\n",
            ),
            (
                "worker.ts",
                "// :::{cite} habeas:coverage\n// TypeScript platform explanation.\n// :::\nfunction work() {}\nclass Worker {\n// {cite}`habeas:coverage`\nwork() {}\n}\n",
            ),
            (
                "notes.txt",
                ":::{cite} habeas:coverage\nPlain-file explanation.\n:::\n",
            ),
        ] {
            fs::write(root.join(file), source).unwrap();
        }
        populate(&mut book, root).unwrap();
        let content = &book.chapters().next().unwrap().content;
        for expected in [
            "worker.py",
            "worker.ts",
            "notes.txt",
            "def[work]",
            "function[work]",
            "method[work]",
            "Python platform explanation.",
            "TypeScript platform explanation.",
            "Plain-file explanation.",
            "4 documented source items",
        ] {
            assert!(content.contains(expected), "{expected}: {content}");
        }
    }
    // {test}`requirements.test-coverage.index.paths`
    #[test]
    fn coverage_generation_rejects_authored_index_and_notes_path_collisions() {
        for collision in [
            PathBuf::from("reports/coverage.md"),
            super::super::notes::path(Path::new("reports/coverage.md"), "code.rs"),
        ] {
            let (temp, mut book) = fixture();
            fs::write(
                temp.path().join("code.rs"),
                "// {cite}`habeas:coverage`\nfn work() {}\n",
            )
            .unwrap();
            book.push_item(Chapter::new(
                "Authored",
                "Preserve this chapter.".into(),
                &collision,
                vec![],
            ));
            if let BookItem::Chapter(parent) = &mut book.items[0] {
                parent.path = Some("reports.md".into());
            }
            let before = serde_json::to_value(&book).unwrap();
            let error = populate(&mut book, temp.path()).unwrap_err();
            assert!(
                error.to_string().contains("coverage")
                    && error.to_string().contains("path conflicts")
            );
            assert_eq!(serde_json::to_value(book).unwrap(), before);
        }
    }

    // {test}`requirements.test-coverage.index.gaps.explanations`
    // {test}`requirements.test-coverage.index.gaps.accepted`
    // {test}`requirements.test-coverage.index.file_map.notes.reviews`
    // {test}`requirements.test-coverage.index.render_only`
    // {test}`requirements.test-coverage.index.file_map.notes.render_only`
    #[test]
    fn overview_shows_missing_unreadable_and_recorded_reviews_with_current_justifications() {
        let (temp, book) = fixture();
        let root = temp.path();
        fs::write(
            root.join("code.rs"),
            "/// Original platform explanation.\n// {cite}`habeas:coverage`\nfn other() {}\n",
        )
        .unwrap();
        let review = root.join(crate::citations::review_path("code.rs", "fn[other]"));
        fs::create_dir_all(review.parent().unwrap()).unwrap();
        for (state, value, label) in [
            ("missing", None, "Missing"),
            ("true", Some(serde_json::json!(true)), "Recorded true"),
            ("false", Some(serde_json::json!(false)), "Recorded false"),
            (
                "fixme",
                Some(serde_json::json!({"FIXME": ["platform-only path"]})),
                "Recorded FIXME",
            ),
            ("invalid", None, "Unreadable"),
            ("unreadable", None, "Unreadable"),
        ] {
            if review.is_dir() {
                fs::remove_dir(&review).unwrap();
            }
            if review.is_file() {
                fs::remove_file(&review).unwrap();
            }
            if let Some(determination) = value {
                fs::write(&review, serde_json::json!({"determination": determination, "rationale": "Saved overview rationale.", "inputs": [], "hash": "old hash"}).to_string()).unwrap();
            } else if state == "invalid" {
                fs::write(&review, "not JSON").unwrap();
            } else if state == "unreadable" {
                fs::create_dir(&review).unwrap();
            }
            let mut rendered = book.clone();
            populate(&mut rendered, root).unwrap();
            let html = map_html(&rendered.chapters().next().unwrap().content);
            assert!(
                html.contains(&format!("Recorded review: <strong>{label}</strong>")),
                "{state}: {html}"
            );
            assert!(html.contains("Original platform explanation."));
            assert!(html.contains("1 documented source items"));
            if matches!(state, "true" | "false" | "fixme") {
                assert!(html.contains("Saved overview rationale."));
            }
            if state == "fixme" {
                assert!(html.contains("platform-only path"));
            }
            if state == "invalid" {
                assert!(html.contains("invalid JSON"));
            }
            if state == "unreadable" {
                assert!(html.contains("cannot read evidence"));
            }
            assert!(!html.contains("current and accepted"));
            assert!(!html.contains("input is stale"));
        }
        fs::remove_dir(&review).unwrap();
        fs::write(
            &review,
            "{\"determination\":true,\"rationale\":\"Saved overview rationale.\"}",
        )
        .unwrap();
        let before = fs::read(&review).unwrap();
        for source in [
            "/// Updated platform explanation.\n// {cite}`habeas:coverage`\nfn other() { let new = 1; }\n",
            "// {cite}`habeas:coverage`\nfn other() {}\n",
        ] {
            fs::write(root.join("code.rs"), source).unwrap();
            let mut rendered = book.clone();
            populate(&mut rendered, root).unwrap();
            let html = map_html(&rendered.chapters().next().unwrap().content);
            assert!(html.contains("Recorded true"));
            assert!(html.contains("Saved overview rationale."));
            assert!(!html.contains("Original platform explanation."));
            assert!(html.contains(if source.starts_with("///") {
                "Updated platform explanation."
            } else {
                "No tracked justification is present."
            }));
            assert_eq!(fs::read(&review).unwrap(), before);
        }
        assert!(!root.join("target").exists());
    }

    // {test}`requirements.test-coverage.index.unavailable`
    // {test}`requirements.test-coverage.index.gaps.summary`
    // {test}`requirements.configuration.evidence_paths`
    #[test]
    #[cfg(unix)]
    fn missing_and_unreadable_sources_produce_overview_diagnostics() {
        use std::os::unix::fs::PermissionsExt;
        let (temp, book) = fixture();
        let root = temp.path();
        let repo = git2::Repository::init(root).unwrap();
        let source = root.join("code.rs");
        fs::write(&source, "// {cite}`habeas:coverage`\nfn other() {}\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("code.rs")).unwrap();
        index.write().unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o000)).unwrap();
        let mut rendered = book.clone();
        populate(&mut rendered, root).unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o644)).unwrap();
        let overview = &rendered.chapters().next().unwrap().content;
        assert!(overview.contains("Source notes unavailable"), "{overview}");
        assert!(overview.contains("code.rs"));
        assert!(!overview.contains("No documented coverage gaps are present."));
        fs::remove_file(&source).unwrap();
        let mut rendered = book.clone();
        populate(&mut rendered, root).unwrap();
        let overview = &rendered.chapters().next().unwrap().content;
        assert!(overview.contains("Source notes unavailable"), "{overview}");
        assert!(overview.contains("code.rs: current source is missing"));
        assert!(!overview.contains("No documented coverage gaps are present."));

        // Missing excluded or explicitly disallowed files reveal no source
        // annotations and must remain outside this documentation view.
        fs::write(&source, "fn ordinary() {}\n").unwrap();
        fs::write(
            root.join("habeas.toml"),
            "version=1\n[evidence]\ndisallow=['denied.rs']\n",
        )
        .unwrap();
        for file in [
            "book/ignored.rs",
            "target/ignored.rs",
            "evidence/ignored.rs",
            "denied.rs",
        ] {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "// {cite}`habeas:coverage`\nfn hidden() {}\n").unwrap();
            index.add_path(Path::new(file)).unwrap();
            fs::remove_file(path).unwrap();
        }
        index.write().unwrap();
        let mut rendered = book;
        populate(&mut rendered, root).unwrap();
        let overview = &rendered.chapters().next().unwrap().content;
        assert!(overview.contains("No documented coverage gaps are present."));
        assert!(!overview.contains("Source notes unavailable"), "{overview}");
    }
    // {test}`requirements.test-coverage.index.freshness`
    #[test]
    fn stale_or_unknown_records_publish_no_current_file_totals() {
        let (temp, _) = fixture();
        let root = temp.path();
        fs::create_dir(root.join("md")).unwrap();
        fs::write(root.join("book.toml"), "[book]\nsrc='md'\n").unwrap();
        fs::write(root.join("md/SUMMARY.md"), "# Summary\n").unwrap();
        let attempt = record(root, "both", 1, "same", &[(1, 1), (2, 1)]);
        let run_path = attempt.parent().unwrap().parent().unwrap().join("run.json");
        let fresh = super::super::check::check_saved(root, &saved_coverage(root)).unwrap();
        assert!(fresh.passed, "{}", fresh.text);
        assert!(fresh.text.contains("code.rs: 2/2 (100.0%) — pass"));
        fs::write(
            root.join("code.rs"),
            "fn one() { let changed = 1; }\nfn two() {}\n",
        )
        .unwrap();
        let stale = super::super::check::check_saved(root, &saved_coverage(root)).unwrap();
        assert!(!stale.passed);
        assert!(stale.text.contains("stale coverage"));
        assert!(!stale.text.contains("- code.rs:"));
        assert!(!stale.text.contains("100.0%"));
        fs::write(root.join("code.rs"), "fn one() {}\nfn two() {}\n").unwrap();
        let mut run: Run = load(&run_path).unwrap();
        run.schema = 99;
        save(&run_path, &run).unwrap();
        let unknown = super::super::check::check_saved(root, &saved_coverage(root)).unwrap();
        assert!(!unknown.passed);
        assert!(unknown.text.contains("cannot establish latest-per-test"));
        assert!(!unknown.text.contains("- code.rs:"));
        assert!(!unknown.text.contains("100.0%"));
        assert!(run_path.exists());
        assert!(attempt.join("1-1.profraw").exists());
    }
}

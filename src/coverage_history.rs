//! Recent Rust item coverage: the outline inputs of each existing evidence
//! file's contents on disk and its last N committed versions.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::outline;

/// The nodes that recent versions of the existing evidence files name, by
/// file and selector, whatever hash they recorded.
// {impl}`requirements.references.coverage.history`
#[derive(Default)]
pub(crate) struct RecentCoverage {
    selectors: HashMap<String, HashSet<String>>,
    /// Whether some file's window lacks committed versions the history did
    /// not provide: outside Git, in a shallow clone, or unreadable history.
    pub(crate) incomplete: bool,
}

impl RecentCoverage {
    /// Collects the windows of `evidence_files`, each its contents on disk
    /// and its contents in the last `depth` commits that changed it.
    // {impl}`requirements.references.coverage.history`
    pub(crate) fn load(root: &Path, evidence_files: &[PathBuf], depth: usize) -> Self {
        let mut coverage = Self::default();
        for file in evidence_files {
            if let Ok(text) = fs::read_to_string(file) {
                coverage.add(&text);
            }
        }
        if !coverage.add_history(root, evidence_files, depth) {
            coverage.incomplete = true;
        }
        coverage
    }

    /// Adds the committed versions; false when the history is unavailable
    /// or shallow, so some windows may be short.
    fn add_history(&mut self, root: &Path, evidence_files: &[PathBuf], depth: usize) -> bool {
        let Ok(repository) = git2::Repository::discover(root) else {
            return false;
        };
        let Some(workdir) = repository
            .workdir()
            .and_then(|dir| fs::canonicalize(dir).ok())
        else {
            return false;
        };
        let mut pending: Vec<(PathBuf, usize)> = evidence_files
            .iter()
            .filter_map(|file| {
                let file = fs::canonicalize(file).ok()?;
                Some((file.strip_prefix(&workdir).ok()?.to_owned(), 0))
            })
            .collect();
        if pending.is_empty() || depth == 0 {
            return !repository.is_shallow();
        }
        let Ok(mut walk) = repository.revwalk() else {
            return false;
        };
        if walk.push_head().is_err() {
            // No commits yet: every window is the file on disk alone.
            return true;
        }
        let _ = walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME);
        for oid in walk.flatten() {
            let Ok(commit) = repository.find_commit(oid) else {
                continue;
            };
            let Ok(tree) = commit.tree() else {
                continue;
            };
            let parent = commit.parent(0).ok().and_then(|parent| parent.tree().ok());
            for (path, count) in pending.iter_mut().filter(|(_, count)| *count < depth) {
                let Ok(entry) = tree.get_path(path) else {
                    continue;
                };
                let before = parent
                    .as_ref()
                    .and_then(|parent| parent.get_path(path).ok())
                    .map(|entry| entry.id());
                if before == Some(entry.id()) {
                    continue;
                }
                if let Ok(blob) = repository.find_blob(entry.id())
                    && let Ok(text) = std::str::from_utf8(blob.content())
                {
                    let text = text.to_owned();
                    *count += 1;
                    self.add(&text);
                }
            }
            if pending.iter().all(|(_, count)| *count >= depth) {
                break;
            }
        }
        !repository.is_shallow()
    }

    /// Adds the outline inputs one version of an evidence file names.
    fn add(&mut self, text: &str) {
        let Ok(record) = serde_json::from_str::<serde_json::Value>(text) else {
            return;
        };
        for input in record
            .get("inputs")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            let field = |name: &str| input.get(name).and_then(serde_json::Value::as_str);
            if let (Some(file), Some(item)) = (field("file"), field("item")) {
                self.selectors
                    .entry(file.to_owned())
                    .or_default()
                    .insert(item.to_owned());
            }
        }
    }

    /// Whether node `index` of `file`'s outline, or a node containing it, is
    /// named by some version in a window.
    pub(crate) fn covers(&self, file: &str, outline: &outline::Outline, index: usize) -> bool {
        let Some(items) = self.selectors.get(file) else {
            return false;
        };
        let mut current = Some(index);
        while let Some(index) = current {
            if items.contains(&outline.nodes[index].selector) {
                return true;
            }
            current = outline.nodes[index].parent;
        }
        false
    }
}

impl RecentCoverage {
    /// Whether the node `item` of `file`, or a node containing it, is named
    /// by some version in a window.
    pub(crate) fn covers_selector(&self, file: &str, item: &str) -> bool {
        let Some(items) = self.selectors.get(file) else {
            return false;
        };
        let mut selector = item;
        loop {
            if items.contains(selector) {
                return true;
            }
            match selector.rfind('/') {
                Some(end) => selector = &selector[..end],
                None if !selector.is_empty() => selector = "",
                None => return false,
            }
        }
    }
}

/// The items to warn about: those without recent coverage, outside modules
/// named `tests`, and whether the coverage history was incomplete.
// {impl}`requirements.references.coverage.report`
// {impl}`requirements.references.coverage.history`
pub(crate) fn warnings(
    root: &Path,
    uncovered: &[crate::rust_coverage::UncoveredItem],
) -> std::io::Result<(Vec<crate::rust_coverage::UncoveredItem>, bool)> {
    let candidates: Vec<_> = uncovered
        .iter()
        .filter(|item| !item.in_tests)
        .cloned()
        .collect();
    if candidates.is_empty() {
        return Ok((Vec::new(), false));
    }
    let depth = crate::config::load_coverage_reauthorings(root)? as usize;
    let evidence = root.join("evidence");
    let mut failures = Vec::new();
    let files = if evidence.is_dir() {
        crate::evidence::files_with_extension(root, &evidence, "json", &mut failures)
    } else {
        Vec::new()
    };
    let recent = RecentCoverage::load(root, &files, depth);
    let warned = candidates
        .into_iter()
        .filter(|item| !recent.covers_selector(&item.file, &item.item))
        .collect();
    Ok((warned, recent.incomplete))
}

/// Whether node `index` lies inside a module named `tests`.
// {impl}`requirements.references.coverage.report`
pub(crate) fn in_tests_module(outline: &outline::Outline, index: usize) -> bool {
    let mut current = outline.nodes[index].parent;
    while let Some(index) = current {
        let node = &outline.nodes[index];
        if node.kind == "mod" && node.display_name().as_deref() == Some("tests") {
            return true;
        }
        current = node.parent;
    }
    false
}

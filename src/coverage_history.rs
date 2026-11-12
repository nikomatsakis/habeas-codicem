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

    /// Adds the committed versions; false when some window may be short:
    /// the history is unavailable or unreadable, or a shallow clone ends
    /// before a file's window is full and before the commit that added it.
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
        // Each file's path in the tree and the versions found so far.
        let mut pending: Vec<(PathBuf, usize)> = evidence_files
            .iter()
            .filter_map(|file| {
                let file = fs::canonicalize(file).ok()?;
                Some((file.strip_prefix(&workdir).ok()?.to_owned(), 0))
            })
            .collect();
        if pending.is_empty() || depth == 0 {
            return true;
        }
        let head = match repository.head() {
            Ok(head) => head,
            // No commits yet: every window is the file on disk alone.
            Err(error) if error.code() == git2::ErrorCode::UnbornBranch => return true,
            Err(_) => return false,
        };
        let (Some(head_id), Ok(head_tree)) = (head.target(), head.peel_to_tree()) else {
            return false;
        };
        let mut complete = true;
        // A file absent from HEAD has never been committed on this branch: no
        // earlier commit holds it either, so its window is its contents on
        // disk alone and the walk need not look for it.
        pending.retain(|(path, _)| blob_id(&head_tree, path, &mut complete).is_some());
        if pending.is_empty() {
            return complete;
        }
        // Every evidence file lies under this directory, so a commit whose
        // directory matches each parent's changes none of them.
        let evidence = fs::canonicalize(root.join("evidence"))
            .ok()
            .and_then(|dir| Some(dir.strip_prefix(&workdir).ok()?.to_owned()))
            .filter(|dir| pending.iter().all(|(path, _)| path.starts_with(dir)));
        // A linked worktree shares the main repository's objects and shallow
        // boundary, but libgit2 walks its history as if it were complete, so
        // walk this worktree's HEAD in the main repository instead.
        drop((head_tree, head));
        let repository = if repository.is_worktree() {
            match git2::Repository::open(repository.commondir()) {
                Ok(main) => main,
                Err(_) => return false,
            }
        } else {
            repository
        };
        let Ok(mut walk) = repository.revwalk() else {
            return false;
        };
        if walk.push(head_id).is_err()
            || walk
                .set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)
                .is_err()
        {
            return false;
        }
        // The commits a shallow clone cut off from their parents.
        let boundary = shallow_boundary(&repository);
        for oid in walk {
            let Some((commit, tree)) = oid.ok().and_then(|oid| {
                let commit = repository.find_commit(oid).ok()?;
                let tree = commit.tree().ok()?;
                Some((commit, tree))
            }) else {
                complete = false;
                continue;
            };
            // `None` when the parents are unknown: cut off or unreadable.
            let parents = if boundary.contains(&commit.id()) {
                None
            } else {
                let parents: Result<Vec<_>, _> = (0..commit.parent_count())
                    .map(|index| commit.parent(index).and_then(|parent| parent.tree()))
                    .collect();
                if parents.is_err() {
                    complete = false;
                }
                parents.ok()
            };
            if let (Some(evidence), Some(parents)) = (&evidence, &parents)
                && !parents.is_empty()
                && let Ok(here) = tree.get_path(evidence).map(|entry| entry.id())
                && parents.iter().all(|parent| {
                    parent
                        .get_path(evidence)
                        .is_ok_and(|entry| entry.id() == here)
                })
            {
                continue;
            }
            let mut done = Vec::new();
            for (position, (path, count)) in pending.iter_mut().enumerate() {
                let Some(id) = blob_id(&tree, path, &mut complete) else {
                    continue;
                };
                // A commit changes the file when it differs from every
                // parent; a root commit changes every file it holds.
                let (changed, added) = match &parents {
                    Some(parents) => {
                        let before: Vec<_> = parents
                            .iter()
                            .map(|parent| blob_id(parent, path, &mut complete))
                            .collect();
                        (
                            before.iter().all(|before| *before != Some(id)),
                            before.iter().all(Option::is_none),
                        )
                    }
                    // Its contents here are available, whether or not this
                    // commit changed it.
                    None => (true, false),
                };
                if changed {
                    match repository.find_blob(id) {
                        Ok(blob) => {
                            if let Ok(text) = std::str::from_utf8(blob.content()) {
                                *count += 1;
                                self.add(text);
                            }
                        }
                        Err(_) => complete = false,
                    }
                }
                // No earlier commit holds the file once the one that added it
                // is seen.
                if added || *count >= depth {
                    done.push(position);
                }
            }
            for position in done.into_iter().rev() {
                pending.remove(position);
            }
            if pending.is_empty() {
                break;
            }
        }
        complete && (pending.is_empty() || !repository.is_shallow())
    }

    /// Adds the outline inputs one version of an evidence file names.
    fn add(&mut self, text: &str) {
        let Ok(record) = serde_json::from_str::<serde_json::Value>(text) else {
            return;
        };
        for (file, item, _) in crate::rust_coverage::outline_inputs(&record) {
            self.selectors
                .entry(file.to_owned())
                .or_default()
                .insert(item.to_owned());
        }
    }

    /// Whether some version in a window names the node of `file` whose
    /// selector is the first of `selectors`, or a node containing it, whose
    /// selectors follow.
    pub(crate) fn covers(&self, file: &str, selectors: &[String]) -> bool {
        self.selectors
            .get(file)
            .is_some_and(|items| selectors.iter().any(|selector| items.contains(selector)))
    }
}

/// The blob `path` names in `tree`, or `None` when it is absent; a failure
/// to read the tree is also `None` and makes the history incomplete.
// {cite}`habeas:util`
fn blob_id(tree: &git2::Tree<'_>, path: &Path, complete: &mut bool) -> Option<git2::Oid> {
    match tree.get_path(path) {
        Ok(entry) => Some(entry.id()),
        Err(error) if error.code() == git2::ErrorCode::NotFound => None,
        Err(_) => {
            *complete = false;
            None
        }
    }
}

/// The commits listed in the repository's `shallow` file, whose parents a
/// shallow clone does not have. The file lives in the common directory, which
/// a linked worktree shares with the main one.
// {cite}`habeas:util`
fn shallow_boundary(repository: &git2::Repository) -> HashSet<git2::Oid> {
    fs::read_to_string(repository.commondir().join("shallow"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| git2::Oid::from_str(line.trim()).ok())
        .collect()
}

/// The items to warn about: those without recent coverage, outside modules
/// named `tests`, and whether the coverage history was incomplete. The
/// evidence files are those repository inspection listed; when listing them
/// failed, some file may lack its window, so the history is incomplete.
// {impl}`requirements.references.coverage.report`
// {impl}`requirements.references.coverage.history`
pub(crate) fn warnings(
    root: &Path,
    uncovered: &[crate::rust_coverage::UncoveredItem],
    evidence_files: &[PathBuf],
    evidence_listed: bool,
) -> std::io::Result<(Vec<crate::rust_coverage::UncoveredItem>, bool)> {
    let candidates: Vec<_> = uncovered
        .iter()
        .filter(|item| !item.in_tests)
        .cloned()
        .collect();
    let depth = crate::config::load_coverage_reauthorings(root)? as usize;
    let recent = RecentCoverage::load(root, evidence_files, depth);
    let warned = candidates
        .into_iter()
        .filter(|item| !recent.covers(&item.file, &item.selectors))
        .collect();
    Ok((warned, recent.incomplete || !evidence_listed))
}

/// The selectors of node `index` and of each node containing it, innermost
/// first, ending with the file's root node.
// {impl}`requirements.references.coverage.history`
pub(crate) fn enclosing_selectors(outline: &outline::Outline, index: usize) -> Vec<String> {
    let mut selectors = Vec::new();
    let mut current = Some(index);
    while let Some(index) = current {
        selectors.push(outline.nodes[index].selector.clone());
        current = outline.nodes[index].parent;
    }
    selectors
}

/// Whether the file at `path`, relative to the project, is a module inside a
/// module named `tests`, such as `src/foo/tests.rs` or `src/foo/tests/mod.rs`
/// declared with `mod tests;`. Files directly in a target directory, such as
/// `tests/tests.rs` or `src/main.rs`, are crate roots, not modules.
// {impl}`requirements.references.coverage.report`
pub(crate) fn in_tests_file(path: &Path) -> bool {
    let names: Vec<&str> = path.iter().filter_map(|name| name.to_str()).collect();
    let Some((file, directories)) = names.split_last() else {
        return false;
    };
    let Some(stem) = file.strip_suffix(".rs") else {
        return false;
    };
    let Some(target) = directories
        .iter()
        .position(|name| matches!(*name, "src" | "tests" | "examples" | "benches"))
    else {
        return false;
    };
    let mut modules = &directories[target + 1..];
    // Outside `src`, and in `src/bin`, each file or directory directly in the
    // target directory is its own crate.
    let mut crates = directories[target] != "src";
    if !crates && modules.first() == Some(&"bin") {
        modules = &modules[1..];
        crates = true;
    }
    let root = modules.is_empty() && (crates || matches!(stem, "lib" | "main"));
    if crates {
        // A directory there may be a crate (`<name>/main.rs`) or a module.
        modules = modules.get(1..).unwrap_or_default();
    }
    modules.contains(&"tests") || (!root && stem == "tests")
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{RecentCoverage, enclosing_selectors, in_tests_file};

    // {test}`requirements.references.coverage.history`
    #[test]
    fn coverage_follows_the_outline_parent_chain_not_selector_text() {
        let source = "struct Thing;\n\nimpl Thing {\n    fn x() {}\n}\n\nfn y() {}\n";
        let outline =
            crate::outline::parse_with(&crate::outline::RUST_PROFILE, source).expect("parses");
        let index = |selector: &str| {
            outline
                .nodes
                .iter()
                .position(|node| node.selector == selector)
                .expect(selector)
        };
        let method = enclosing_selectors(&outline, index("impl[Thing]/fn[x]"));
        assert_eq!(method, ["impl[Thing]/fn[x]", "impl[Thing]", ""]);
        let record = |items: &[&str]| {
            let mut recent = RecentCoverage::default();
            let inputs: Vec<_> = items
                .iter()
                .map(|item| serde_json::json!({"file": "src/lib.rs", "item": item}))
                .collect();
            recent.add(&serde_json::json!({ "inputs": inputs }).to_string());
            recent
        };
        assert!(record(&["impl[Thing]"]).covers("src/lib.rs", &method));
        assert!(record(&[""]).covers("src/lib.rs", &method));
        assert!(!record(&["fn[y]"]).covers("src/lib.rs", &method));
        assert!(!record(&["impl[Thing]"]).covers("src/other.rs", &method));
        // A `/` inside brackets does not make a prefix of the selector an
        // enclosing node.
        let chain = ["impl[A/B]/fn[x]".to_owned(), "impl[A/B]".to_owned()];
        assert!(!record(&["impl[A"]).covers("src/lib.rs", &chain));
        assert!(record(&["impl[A/B]"]).covers("src/lib.rs", &chain));
    }

    // {test}`requirements.references.coverage.report`
    // {test}`requirements.references.coverage.test_module`
    #[test]
    fn files_declared_as_tests_modules_are_in_a_tests_module() {
        for path in [
            "src/tests.rs",
            "src/foo/tests.rs",
            "src/foo/tests/mod.rs",
            "src/foo/tests/helpers.rs",
            "crates/a/src/b/tests.rs",
            "src/bin/tool/tests.rs",
            "tests/common/tests.rs",
        ] {
            assert!(in_tests_file(Path::new(path)), "{path}");
        }
        for path in [
            "src/lib.rs",
            "src/main.rs",
            "src/foo.rs",
            "src/foo/mod.rs",
            "src/bin/tests.rs",
            "tests/tests.rs",
            "tests/coverage.rs",
            "tests/common/mod.rs",
            "examples/tests.rs",
            "build.rs",
        ] {
            assert!(!in_tests_file(Path::new(path)), "{path}");
        }
    }
}

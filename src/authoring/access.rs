use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use super::inputs::generated_evidence_path;

pub(crate) struct SourceAccess {
    tracked: Option<BTreeSet<PathBuf>>,
}

impl SourceAccess {
    // {impl}`requirements.authoring.source_tools`
    pub(crate) fn load(root: &Path) -> Result<Self, String> {
        let mut has_git_marker = false;
        for directory in root.ancestors() {
            match fs::symlink_metadata(directory.join(".git")) {
                Ok(_) => {
                    has_git_marker = true;
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("cannot locate Git index: {error}")),
            }
        }
        if !has_git_marker {
            return Ok(Self { tracked: None });
        }
        let discovery = Command::new("git")
            .current_dir(root)
            .env("LC_ALL", "C")
            .args(["rev-parse", "--is-inside-work-tree"])
            .output()
            .map_err(|error| format!("cannot discover Git worktree: {error}"))?;
        if !discovery.status.success() {
            let error = String::from_utf8_lossy(&discovery.stderr);
            // An empty .git directory is not a repository. Let Git distinguish
            // ordinary discovery failure from invalid configuration or access errors.
            if error.starts_with("fatal: not a git repository (") {
                return Ok(Self { tracked: None });
            }
            return Err(format!("cannot discover Git worktree: {}", error.trim()));
        }
        match discovery.stdout.trim_ascii() {
            b"true" => {}
            b"false" => return Ok(Self { tracked: None }),
            _ => return Err("Git returned an invalid worktree discovery result".to_owned()),
        }
        let output = Command::new("git")
            .current_dir(root)
            .args(["ls-files", "--cached", "-z", "--", "."])
            .output()
            .map_err(|error| format!("cannot read Git index: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "cannot read Git index: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let tracked = output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .map(|path| {
                std::str::from_utf8(path)
                    .map(|path| root.join(path))
                    .map_err(|_| "Git index contains a non-UTF-8 source path".to_owned())
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            tracked: Some(tracked),
        })
    }

    // {impl}`requirements.authoring.source_tools`
    // {impl}`requirements.authoring.file_discovery`
    // {impl}`requirements.authoring.text_search`
    pub(crate) fn path(&self, root: &Path, relative: &Path) -> Result<PathBuf, String> {
        let (requested, target) = resolve(root, relative)?;
        if let Some(tracked) = &self.tracked {
            if target.is_dir() {
                if !self.has_source_under(root, &requested) || !self.has_source_under(root, &target)
                {
                    return Err(format!(
                        "{} contains no eligible Git-tracked source files",
                        relative.display()
                    ));
                }
            } else {
                for path in [&requested, &target] {
                    if !tracked.contains(path) {
                        return Err(format!(
                            "{} is not tracked by Git; use git add to stage it before authoring evidence",
                            path.strip_prefix(root).unwrap_or(path).display()
                        ));
                    }
                }
            }
        }
        Ok(target)
    }

    // {impl}`requirements.authoring.source_tools`
    // {impl}`requirements.authoring.mandatory_inputs`
    pub(crate) fn file(&self, root: &Path, relative: &Path) -> Result<PathBuf, String> {
        let target = self.path(root, relative)?;
        if !target.is_file() {
            return Err(format!("{} is not a source file", relative.display()));
        }
        Ok(target)
    }

    // {impl}`requirements.authoring.file_discovery`
    // {impl}`requirements.authoring.source_tools`
    // {impl}`requirements.authoring.text_search`
    fn has_source_under(&self, root: &Path, directory: &Path) -> bool {
        self.tracked.as_ref().is_some_and(|tracked| {
            tracked.iter().any(|path| {
                path != directory
                    && path.starts_with(directory)
                    && path.strip_prefix(root).is_ok_and(|relative| {
                        resolve(root, relative)
                            .is_ok_and(|(_, target)| target.is_file() && tracked.contains(&target))
                    })
            })
        })
    }
}

// {impl}`requirements.authoring.source_tools`
// {impl}`requirements.authoring.file_discovery`
// {impl}`requirements.authoring.text_search`
// {impl}`requirements.ci.input_containment`
fn resolve(root: &Path, relative: &Path) -> Result<(PathBuf, PathBuf), String> {
    if relative.as_os_str().is_empty()
        || !relative
            .components()
            .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
    {
        return Err("path must be relative to the project".to_owned());
    }
    let requested = root.join(
        relative
            .components()
            .filter(|part| *part != Component::CurDir)
            .collect::<PathBuf>(),
    );
    if generated_evidence_path(root, &requested) {
        return Err("generated evidence is output, not assessment source".to_owned());
    }
    let target = fs::canonicalize(&requested)
        .map_err(|error| format!("cannot open {}: {error}", relative.display()))?;
    if !target.starts_with(root) {
        return Err(format!(
            "{} resolves outside the project",
            relative.display()
        ));
    }
    Ok((requested, target))
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    // {test}`requirements.authoring.source_tools`
    pub(crate) fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // {test}`requirements.authoring.source_tools`
    #[test]
    fn nested_project_uses_only_its_part_of_the_git_index() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("project");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("inside.txt"), "inside\n").unwrap();
        fs::write(parent.path().join("outside.txt"), "outside\n").unwrap();
        git(parent.path(), &["init", "--quiet"]);
        git(parent.path(), &["add", "."]);
        let root = fs::canonicalize(root).unwrap();
        let access = SourceAccess::load(&root).unwrap();
        assert!(access.file(&root, Path::new("./inside.txt")).is_ok());
        assert!(access.file(&root, Path::new("../outside.txt")).is_err());
        assert!(access.file(&root, Path::new("outside.txt")).is_err());
    }

    // {test}`requirements.authoring.source_tools`
    #[test]
    fn unreadable_git_index_does_not_fall_back_to_untracked_access() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("source.txt"), "source\n").unwrap();
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["add", "."]);
        fs::write(root.path().join(".git/index"), "invalid index\n").unwrap();
        let error = SourceAccess::load(root.path()).err().unwrap();
        assert!(error.contains("cannot read Git index"), "{error}");
    }

    // {test}`requirements.authoring.source_tools`
    #[test]
    fn empty_git_directory_in_an_ancestor_does_not_make_a_project_a_worktree() {
        let parent = tempfile::tempdir().unwrap();
        fs::create_dir(parent.path().join(".git")).unwrap();
        let root = parent.path().join("project");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("source.txt"), "source\n").unwrap();
        let root = fs::canonicalize(root).unwrap();
        let access = SourceAccess::load(&root).unwrap();
        assert!(access.tracked.is_none());
        assert!(access.file(&root, Path::new("source.txt")).is_ok());
        assert!(access.file(&root, Path::new("../outside.txt")).is_err());
    }

    // {test}`requirements.authoring.source_tools`
    #[test]
    fn invalid_git_configuration_does_not_fall_back_to_untracked_access() {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "--quiet"]);
        fs::write(root.path().join(".git/config"), "[invalid config\n").unwrap();
        let error = SourceAccess::load(root.path()).err().unwrap();
        assert!(error.contains("cannot discover Git worktree"), "{error}");
    }
}

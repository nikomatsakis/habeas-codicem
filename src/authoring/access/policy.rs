use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

#[derive(Default)]
pub struct PathPolicy {
    allow: Vec<PathBuf>,
    disallow: Vec<PathBuf>,
}

impl PathPolicy {
    pub fn new(allow: Vec<PathBuf>, disallow: Vec<PathBuf>) -> Self {
        Self { allow, disallow }
    }

    pub fn allowed(&self, requested: &Path, target: &Path) -> bool {
        self.allow
            .iter()
            .any(|rule| matches_rule(rule, requested, target))
    }

    pub fn check_disallow(&self, requested: &Path, target: &Path) -> io::Result<()> {
        if self.disallow.is_empty() {
            return Ok(());
        }
        // Retain locations traversed before a symlink redirects the request. Resolving
        // each parent also preserves filesystem semantics for subsequent `..`.
        let locations: Vec<_> = requested
            .ancestors()
            .filter_map(|path| requested_location(path).ok())
            .collect();
        if let Some(rule) = self.disallow.iter().find(|rule| {
            matches_rule(rule, requested, target)
                || locations
                    .iter()
                    .any(|path| matches_rule(rule, path, target))
        }) {
            return Err(denied(format!(
                "{} is disallowed by habeas.toml rule {}; update the workspace evidence policy before retrying",
                requested.display(),
                rule.display()
            )));
        }
        Ok(())
    }
}

/// The SDK source provider used by Habeas authoring.
pub struct SourceAccess {
    sources: trust_but_verify::sources::Sources,
}

impl SourceAccess {
    pub fn new(root: &Path) -> Self {
        Self {
            sources: trust_but_verify::sources::Sources::new(root),
        }
    }

    // The SDK resolves parent components lexically before resolving symlinks.
    pub(crate) fn relative(root: &Path, path: &Path) -> io::Result<PathBuf> {
        let path = path.strip_prefix(root).unwrap_or(path);
        let mut relative = PathBuf::new();
        for component in path.components() {
            match component {
                Component::Normal(name) => relative.push(name),
                Component::CurDir => {}
                Component::ParentDir if relative.pop() => {}
                _ => return Err(denied(format!("{} is outside the project", path.display()))),
            }
        }
        Ok(relative)
    }

    pub fn read(
        &self,
        root: &Path,
        path: &Path,
    ) -> io::Result<(PathBuf, trust_but_verify::sources::SourceFile)> {
        use trust_but_verify::sources::SourceError;
        let relative = Self::relative(root, path)?;
        let file =
            futures_executor::block_on(self.sources.read(&relative)).map_err(
                |error| match error {
                    SourceError::Io(error) => io::Error::new(
                        error.kind(),
                        format!("cannot read {}: {error}", path.display()),
                    ),
                    error => denied(format!("cannot read {}: {error}", path.display())),
                },
            )?;
        Ok((root.join(relative), file))
    }

    pub fn file(&self, root: &Path, path: &Path) -> io::Result<PathBuf> {
        self.read(root, path).map(|(path, _)| path)
    }
}

pub(super) fn discover_files(
    root: &Path,
    policy: PathPolicy,
    errors: &mut Vec<String>,
) -> Vec<PathBuf> {
    let mut walk = ignore::WalkBuilder::new(root);
    let project = root.to_owned();
    walk.standard_filters(false)
        .git_ignore(true)
        .require_git(false)
        .filter_entry(move |entry| {
            entry.depth() == 0
                || considers(
                    &project,
                    &policy,
                    entry.path(),
                    entry.file_type().is_some_and(|kind| kind.is_dir()),
                )
        });
    let mut files = Vec::new();
    for entry in walk.build() {
        match entry {
            Ok(entry) => {
                if let Some(error) = entry.error() {
                    errors.push(format!("cannot discover reference sources: {error}"));
                }
                if entry.file_type().is_some_and(|kind| kind.is_file()) {
                    files.push(entry.into_path());
                }
            }
            Err(error) => errors.push(format!("cannot discover reference sources: {error}")),
        }
    }
    files.sort();
    files
}

pub(crate) fn considers(root: &Path, policy: &PathPolicy, path: &Path, directory: bool) -> bool {
    !excluded_from_discovery(root, path, directory) && policy.check_disallow(path, path).is_ok()
}

/// Whether a file, or a directory when `directory` is set, lies in a `.git`
/// or `target` directory at any depth or in the project-root `book` or
/// `evidence` directory.
fn excluded_from_discovery(root: &Path, path: &Path, directory: bool) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return true;
    };
    if relative.file_name().is_some_and(|name| name == ".git") {
        return true;
    }
    let directories = if directory {
        Some(relative)
    } else {
        relative.parent()
    };
    let mut components = directories.into_iter().flat_map(Path::components);
    let first = components.next();
    let is = |component: Option<Component<'_>>, names: &[&str]| matches!(component, Some(Component::Normal(name)) if names.iter().any(|candidate| name == *candidate));
    is(first, &["book", "evidence", ".git", "target"])
        || components.any(|component| is(Some(component), &[".git", "target"]))
}

fn matches_rule(rule: &Path, requested: &Path, target: &Path) -> bool {
    requested_location(rule)
        .is_ok_and(|rule| requested.starts_with(&rule) || target.starts_with(rule))
        || fs::canonicalize(rule)
            .is_ok_and(|rule| requested.starts_with(&rule) || target.starts_with(rule))
}

fn requested_location(path: &Path) -> io::Result<PathBuf> {
    match (path.parent(), path.components().next_back()) {
        (Some(parent), Some(Component::Normal(name))) => Ok(fs::canonicalize(parent)?.join(name)),
        _ => fs::canonicalize(path),
    }
}

// {cite}`habeas:util`
/// A source-access policy denial. A denied mandatory input aborts its assessment; an operating-system permission
/// failure is an ordinary read failure.
#[derive(Debug)]
struct PolicyDenial(String);

impl std::fmt::Display for PolicyDenial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PolicyDenial {}

// {cite}`habeas:util`
fn denied(message: impl Into<String>) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        PolicyDenial(message.into()),
    )
}

fn resolve(root: &Path, relative: &Path) -> io::Result<(PathBuf, PathBuf)> {
    if relative.as_os_str().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path must not be empty",
        ));
    }
    let joined = root.join(relative);
    let target = fs::canonicalize(&joined).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("cannot open {}: {error}", relative.display()),
        )
    })?;
    let requested = requested_location(&joined)?;
    Ok((requested, target))
}

pub(crate) fn tracked_sources(root: &Path) -> io::Result<Option<BTreeSet<PathBuf>>> {
    let mut has_git_marker = false;
    for directory in root.ancestors() {
        match fs::symlink_metadata(directory.join(".git")) {
            Ok(_) => {
                has_git_marker = true;
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(io::Error::other(format!(
                    "cannot locate Git index: {error}"
                )));
            }
        }
    }
    if !has_git_marker {
        return Ok(None);
    }
    let discovery = Command::new("git")
        .current_dir(root)
        .env("LC_ALL", "C")
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()?;
    if !discovery.status.success() {
        let error = String::from_utf8_lossy(&discovery.stderr);
        if error.starts_with("fatal: not a git repository (") {
            return Ok(None);
        }
        return Err(io::Error::other(format!(
            "cannot discover Git worktree: {}",
            error.trim()
        )));
    }
    match discovery.stdout.trim_ascii() {
        b"true" => {}
        b"false" => return Ok(None),
        _ => {
            return Err(io::Error::other(
                "Git returned an invalid worktree discovery result",
            ));
        }
    }
    let output = Command::new("git")
        .current_dir(root)
        .args(["ls-files", "--cached", "-z", "--", "."])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "cannot read Git index: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let tracked = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| {
            std::str::from_utf8(path)
                .map(|path| root.join(path))
                .map_err(|_| io::Error::other("Git index contains a non-UTF-8 source path"))
        })
        .collect::<io::Result<_>>()?;
    Ok(Some(tracked))
}

pub fn generated_roots(root: &Path) -> Vec<PathBuf> {
    let Ok(output) = Command::new("cargo")
        .current_dir(root)
        .args([
            "metadata",
            "--offline",
            "--locked",
            "--no-deps",
            "--format-version=1",
        ])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let Ok(metadata) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
        return Vec::new();
    };
    let Some(directory) = metadata
        .get("build_directory")
        .or_else(|| metadata.get("target_directory"))
        .and_then(serde_json::Value::as_str)
    else {
        return Vec::new();
    };
    let Ok(directory) = fs::canonicalize(directory) else {
        return Vec::new();
    };
    discover_generated_roots(&directory)
}

fn discover_generated_roots(build_root: &Path) -> Vec<PathBuf> {
    let mut roots = BTreeSet::new();
    let mut directories = vec![(build_root.to_owned(), 0)];
    while let Some((directory, depth)) = directories.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_name() == "root-output" {
                let Ok(text) = fs::read_to_string(&path) else {
                    continue;
                };
                let Ok(out) = fs::canonicalize(text.trim()) else {
                    continue;
                };
                let Ok(sibling) = fs::canonicalize(directory.join("out")) else {
                    continue;
                };
                if out == sibling && out.starts_with(build_root) && out.is_dir() {
                    roots.insert(out);
                }
            } else if depth < 4 && entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                // Only inspect Cargo's profile[/triple]/build/<package> layout.
                if depth < 2
                    || directory.file_name().is_some_and(|name| name == "build")
                    || entry.file_name() == "build"
                {
                    directories.push((path, depth + 1));
                }
            }
        }
    }
    roots.into_iter().collect()
}

pub fn resolve_evidence_file(
    root: &Path,
    file: &str,
    policy: impl FnOnce() -> io::Result<PathPolicy>,
) -> io::Result<PathBuf> {
    let root = fs::canonicalize(root)?;
    let (requested, target) = resolve(&root, Path::new(file))?;
    let policy = policy()?;
    policy.check_disallow(&root.join(file), &target)?;
    if (!requested.starts_with(&root) || !target.starts_with(&root))
        && !policy.allowed(&requested, &target)
        && !generated_roots(&root).iter().any(|directory| {
            target.starts_with(directory)
                && (requested.starts_with(&root) || requested.starts_with(directory))
        })
    {
        return Err(denied(format!(
            "{file} resolves outside the repository without an allowed evidence path"
        )));
    }
    if !target.is_file() {
        return Err(io::Error::other(format!("{file} is not a project file")));
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    // {test}`requirements.authoring.generated_inputs`
    #[test]
    fn generated_discovery_accepts_out_directories_but_not_arbitrary_root_output_targets() {
        let root = tempfile::tempdir().unwrap();
        let out = root.path().join("debug/build/package-hash/out");
        fs::create_dir_all(&out).unwrap();
        fs::write(
            out.parent().unwrap().join("root-output"),
            out.to_str().unwrap(),
        )
        .unwrap();
        let cross = root
            .path()
            .join("target-triple/custom/build/other-hash/out");
        fs::create_dir_all(&cross).unwrap();
        fs::write(
            cross.parent().unwrap().join("root-output"),
            cross.to_str().unwrap(),
        )
        .unwrap();
        let unrelated = tempfile::tempdir().unwrap();
        let fake = root.path().join("debug/build/fake-hash");
        fs::create_dir_all(fake.join("out")).unwrap();
        fs::write(fake.join("root-output"), unrelated.path().to_str().unwrap()).unwrap();
        assert_eq!(discover_generated_roots(root.path()), vec![out, cross]);
    }
}

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

// {impl}`requirements.configuration.evidence_paths`
#[derive(Default)]
pub struct PathPolicy {
    allow: Vec<PathBuf>,
    disallow: Vec<PathBuf>,
}

impl PathPolicy {
    // {impl}`requirements.configuration.evidence_paths`
    pub fn new(allow: Vec<PathBuf>, disallow: Vec<PathBuf>) -> Self {
        Self { allow, disallow }
    }

    // {impl}`requirements.configuration.evidence_paths`
    pub fn allowed(&self, requested: &Path, target: &Path) -> bool {
        self.allow
            .iter()
            .any(|rule| matches_rule(rule, requested, target))
    }

    // {impl}`requirements.configuration.evidence_paths`
    // {impl}`requirements.authoring.tool_descriptions.special_cases`
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

// {impl}`requirements.authoring.generated_inputs`
pub struct SourceAccess {
    tracked: Option<BTreeSet<PathBuf>>,
    policy: PathPolicy,
    generated: Vec<PathBuf>,
    /// The book's build directory and the project-root `target` directory.
    build_output: Vec<PathBuf>,
}

impl SourceAccess {
    // {impl}`requirements.configuration.evidence_paths`
    // {impl}`requirements.authoring.generated_inputs`
    pub fn load(root: &Path, policy: impl FnOnce() -> io::Result<PathPolicy>) -> io::Result<Self> {
        Ok(Self {
            tracked: tracked_sources(root)?,
            policy: policy()?,
            generated: generated_roots(root),
            build_output: build_output_dirs(root),
        })
    }

    // {impl}`requirements.authoring.file_discovery`
    // {impl}`requirements.configuration.evidence_paths`
    // {impl}`requirements.authoring.generated_inputs`
    // {impl}`requirements.authoring.read_denial.untracked`
    // {impl}`requirements.authoring.read_denial.build_output`
    // {impl}`requirements.authoring.tool_descriptions.special_cases`
    pub fn path(&self, root: &Path, relative: &Path) -> io::Result<PathBuf> {
        let (requested, target) = resolve(root, relative)?;
        self.policy.check_disallow(&root.join(relative), &target)?;
        if self.policy.allowed(&requested, &target) {
            return Ok(target);
        }
        if generated_evidence_path(root, &requested) {
            return Err(denied(format!(
                "{}: generated evidence is output, not assessment source; an explicit habeas.toml allow rule is required",
                relative.display()
            )));
        }
        if self
            .generated
            .iter()
            .any(|directory| requested.starts_with(directory) && target.starts_with(directory))
        {
            return Ok(target);
        }
        let lexical_request = normalize(&root.join(relative));
        // Build output is not source; a directory holding permitted
        // generated output stays listable.
        if self
            .build_output
            .iter()
            .any(|directory| lexical_request.starts_with(directory))
            && !self
                .extra_roots()
                .any(|extra| extra.starts_with(&lexical_request) || extra.starts_with(&target))
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                OutsideProject(format!(
                    "{} holds build output; the sources it is built from are elsewhere in the project, so read those instead",
                    relative.display()
                )),
            ));
        }
        if !lexical_request.starts_with(root) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                OutsideProject(format!(
                    "{} is outside the project and no habeas.toml rule permits it; only project sources and explicitly allowed paths can be read",
                    relative.display()
                )),
            ));
        }
        if !target.starts_with(root) {
            return Err(denied(format!(
                "{} resolves outside the project; add an explicit habeas.toml allow rule before retrying",
                relative.display()
            )));
        }
        if let Some(tracked) = &self.tracked {
            if target.is_dir() {
                if !self.has_source_under(root, &requested) || !self.has_source_under(root, &target)
                {
                    return Err(denied(format!(
                        "{} contains no tracked sources: its files are untracked, so they cannot be read; the caller can stage them with `git add` or permit them with a workspace habeas.toml allow rule",
                        relative.display()
                    )));
                }
            } else {
                for path in [&lexical_request, &target] {
                    if !tracked.contains(path) {
                        return Err(denied(format!(
                            "{} is untracked: it is not in the Git index, so it cannot be read; the caller can stage it with `git add` or permit it with a workspace habeas.toml allow rule",
                            path.strip_prefix(root).unwrap_or(path).display()
                        )));
                    }
                }
            }
        }
        Ok(target)
    }

    // {impl}`requirements.authoring.mandatory_inputs`
    pub fn file(&self, root: &Path, relative: &Path) -> io::Result<PathBuf> {
        let target = self.path(root, relative)?;
        if !target.is_file() {
            return Err(io::Error::other(format!(
                "{} is not a source file",
                relative.display()
            )));
        }
        Ok(target)
    }

    // {impl}`requirements.authoring.mandatory_inputs`
    // {impl}`requirements.configuration.evidence_paths`
    pub fn omit_generated_evidence(&self, root: &Path, path: &Path) -> bool {
        let joined = root.join(path);
        let requested = requested_location(&joined).unwrap_or_else(|_| joined.clone());
        let target = fs::canonicalize(&joined).unwrap_or_else(|_| joined.clone());
        if !generated_evidence_path(root, &requested) {
            return false;
        }
        self.policy.check_disallow(&joined, &target).is_ok()
            && !self.policy.allowed(&requested, &target)
    }

    // {impl}`requirements.authoring.file_discovery`
    // {impl}`requirements.authoring.generated_inputs`
    // {impl}`requirements.configuration.evidence_paths`
    fn has_source_under(&self, root: &Path, directory: &Path) -> bool {
        self.extra_roots().any(|path| {
            let target = fs::canonicalize(path).unwrap_or_else(|_| path.clone());
            target != directory
                && (path.starts_with(directory) || target.starts_with(directory))
                && self.policy.check_disallow(path, &target).is_ok()
        }) || self.tracked.as_ref().is_some_and(|tracked| {
            tracked.iter().any(|path| {
                path != directory && path.starts_with(directory) && self.file(root, path).is_ok()
            })
        })
    }

    // {impl}`requirements.authoring.generated_inputs`
    // {impl}`requirements.configuration.evidence_paths`
    // {impl}`requirements.authoring.file_discovery`
    pub fn extra_roots(&self) -> impl Iterator<Item = &PathBuf> {
        self.policy.allow.iter().chain(self.generated.iter())
    }
}

/// The project files reference discovery and Rust item coverage consider: the
/// sources [`SourceAccess`] would let authoring read by default, found without
/// authoring configuration or Cargo metadata.
// {impl}`requirements.authoring.deterministic_validation`
pub struct DiscoverySources {
    root: PathBuf,
    tracked: Option<BTreeSet<PathBuf>>,
    policy: PathPolicy,
}

impl DiscoverySources {
    /// Loads the Git index, then asks the consumer for discovery rules.
    // {impl}`requirements.authoring.deterministic_validation`
    pub fn load(root: &Path, policy: impl FnOnce() -> PathPolicy) -> io::Result<Self> {
        Ok(Self {
            root: root.to_owned(),
            tracked: tracked_sources(root)?,
            policy: policy(),
        })
    }

    /// Every considered file, sorted. In a Git worktree these are the indexed
    /// files; outside Git, every file under the project root. Symlinks are not
    /// followed, so no source is counted twice through an alias.
    // {impl}`requirements.ci.errors`
    pub fn files(&self, errors: &mut Vec<String>) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = match &self.tracked {
            Some(tracked) => tracked
                .iter()
                .filter(|path| {
                    // An indexed file deleted from the worktree has no
                    // content to scan; an uninspectable one is kept so that
                    // reading it reports the failure.
                    fs::symlink_metadata(path).map_or_else(
                        |error| error.kind() != io::ErrorKind::NotFound,
                        |metadata| metadata.is_file(),
                    )
                })
                .filter(|path| self.considers(path, false))
                .cloned()
                .collect(),
            None => self.walk(errors),
        };
        files.sort();
        files
    }

    /// The files in the Git index, including staged and deleted ones; `None`
    /// outside Git.
    pub fn indexed(&self) -> Option<&BTreeSet<PathBuf>> {
        self.tracked.as_ref()
    }

    /// Untracked files a Git worktree's ignore rules do not exclude, with the
    /// same exclusions as [`Self::files`]; empty outside Git.
    pub fn untracked(&self) -> io::Result<Vec<PathBuf>> {
        if self.tracked.is_none() {
            return Ok(Vec::new());
        }
        let output = Command::new("git")
            .current_dir(&self.root)
            .args([
                "ls-files",
                "--others",
                "--exclude-standard",
                "-z",
                "--",
                ".",
            ])
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "cannot list untracked files: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let mut files: Vec<PathBuf> = output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .filter_map(|path| std::str::from_utf8(path).ok())
            .map(|path| self.root.join(path))
            .filter(|path| fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file()))
            .filter(|path| self.considers(path, false))
            .collect();
        files.sort();
        Ok(files)
    }

    /// Whether a project file outside the fixed exclusions is permitted by the
    /// workspace rules: not disallowed.
    // {impl}`requirements.configuration.evidence_paths`
    fn considers(&self, path: &Path, directory: bool) -> bool {
        if excluded_from_discovery(&self.root, path, directory) {
            return false;
        }
        let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
        self.policy.check_disallow(path, &target).is_ok()
    }

    /// Lists every file outside Git, skipping excluded and disallowed
    /// directories without descending into them.
    // {impl}`requirements.ci.errors`
    fn walk(&self, errors: &mut Vec<String>) -> Vec<PathBuf> {
        let root = &self.root;
        let label = |path: &Path| {
            path.strip_prefix(root)
                .unwrap_or(path)
                .display()
                .to_string()
        };
        let mut stack = vec![root.clone()];
        let mut files = Vec::new();
        while let Some(directory) = stack.pop() {
            let entries = match fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(error) => {
                    errors.push(format!(
                        "{}: cannot scan references: {error}",
                        label(&directory)
                    ));
                    continue;
                }
            };
            for entry in entries {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        errors.push(format!(
                            "{}: cannot scan directory entry: {error}",
                            label(&directory)
                        ));
                        continue;
                    }
                };
                let path = entry.path();
                let kind = match entry.file_type() {
                    Ok(kind) => kind,
                    Err(error) => {
                        errors.push(format!(
                            "{}: cannot inspect reference source: {error}",
                            label(&path)
                        ));
                        continue;
                    }
                };
                if kind.is_dir() {
                    if self.considers(&path, true) {
                        stack.push(path);
                    }
                } else if kind.is_file() && self.considers(&path, false) {
                    files.push(path);
                }
            }
        }
        files
    }
}

/// Whether a file, or a directory when `directory` is set, lies in a `.git`
/// or `target` directory at any depth or in the project-root `book` or
/// `evidence` directory.
// {impl}`requirements.references.discovery`
fn excluded_from_discovery(root: &Path, path: &Path, directory: bool) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return true;
    };
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

// {impl}`requirements.configuration.evidence_paths`
fn matches_rule(rule: &Path, requested: &Path, target: &Path) -> bool {
    requested_location(rule)
        .is_ok_and(|rule| requested.starts_with(&rule) || target.starts_with(rule))
        || fs::canonicalize(rule)
            .is_ok_and(|rule| requested.starts_with(&rule) || target.starts_with(rule))
}

// {impl}`requirements.configuration.evidence_paths`
fn requested_location(path: &Path) -> io::Result<PathBuf> {
    match (path.parent(), path.components().next_back()) {
        (Some(parent), Some(Component::Normal(name))) => Ok(fs::canonicalize(parent)?.join(name)),
        _ => fs::canonicalize(path),
    }
}

// {impl}`requirements.configuration.evidence_paths`
pub fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

/// A denial of a request that names a path outside the project and is not
/// permitted by workspace or generated-output policy, or of a path holding
/// build output. Mandatory inputs abort on it; a model's dispatched request
/// receives it as a recoverable tool error.
// {cite}`habeas:util`
#[derive(Debug)]
pub struct OutsideProject(String);

impl std::fmt::Display for OutsideProject {
    // {impl}`requirements.authoring.read_denial`
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for OutsideProject {}

/// Turns a denial of a path outside the project into an ordinary read failure,
/// leaving every other error unchanged.
// {impl}`requirements.authoring.file_discovery`
// {impl}`requirements.authoring.outline_tool`
pub fn recoverable_outside_request(error: io::Error) -> io::Error {
    if error
        .get_ref()
        .is_some_and(|inner| inner.is::<OutsideProject>())
    {
        io::Error::other(error.to_string())
    } else {
        error
    }
}

// {cite}`habeas:util`
/// A source-access policy denial. Only these, and [`OutsideProject`] denials
/// of mandatory inputs, abort an assessment; an operating-system permission
/// failure is an ordinary read failure.
#[derive(Debug)]
struct PolicyDenial(String);

impl std::fmt::Display for PolicyDenial {
    // {impl}`requirements.authoring.read_denial`
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PolicyDenial {}

/// Turns a permission error that did not come from source-access policy, such
/// as an operating-system `EACCES`, into an ordinary read failure, leaving
/// policy denials and every other error unchanged.
// {impl}`requirements.authoring.read_denial`
pub fn recoverable_os_denial(error: io::Error) -> io::Error {
    if error.kind() == io::ErrorKind::PermissionDenied
        && !error
            .get_ref()
            .is_some_and(|inner| inner.is::<PolicyDenial>() || inner.is::<OutsideProject>())
    {
        io::Error::other(error.to_string())
    } else {
        error
    }
}

// {cite}`habeas:util`
fn denied(message: impl Into<String>) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        PolicyDenial(message.into()),
    )
}

// {impl}`requirements.authoring.file_discovery`
// {impl}`requirements.configuration.evidence_paths`
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

fn tracked_sources(root: &Path) -> io::Result<Option<BTreeSet<PathBuf>>> {
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

/// The directories holding the project's build output: the book's build
/// directory, `book` unless `book.toml` sets `[build] build-dir`, and the
/// project-root `target` directory.
fn build_output_dirs(root: &Path) -> Vec<PathBuf> {
    let book = fs::read_to_string(root.join("book.toml"))
        .ok()
        .and_then(|text| text.parse::<toml_edit::DocumentMut>().ok())
        .and_then(|book| {
            book.get("build")?
                .get("build-dir")?
                .as_str()
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "book".to_owned());
    vec![normalize(&root.join(book)), root.join("target")]
}

// {impl}`requirements.authoring.generated_inputs`
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

// {impl}`requirements.authoring.generated_inputs`
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

// {impl}`requirements.configuration.evidence_paths`
// {impl}`requirements.authoring.generated_inputs`
// {impl}`requirements.configuration.loading`
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
        && !generated_roots(&root)
            .iter()
            .any(|directory| requested.starts_with(directory) && target.starts_with(directory))
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

// {impl}`requirements.authoring.mandatory_inputs`
// {impl}`requirements.authoring.file_discovery`
pub fn generated_evidence_path(root: &Path, path: &Path) -> bool {
    let path = root.join(path);
    let evidence = root.join("evidence");
    path.starts_with(&evidence)
        || fs::canonicalize(&path)
            .is_ok_and(|path| path.starts_with(fs::canonicalize(&evidence).unwrap_or(evidence)))
}

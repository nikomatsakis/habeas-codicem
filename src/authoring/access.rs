use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use super::inputs::generated_evidence_path;

#[derive(Default)]
pub(crate) struct PathPolicy {
    allow: Vec<PathBuf>,
    disallow: Vec<PathBuf>,
    credential_file: Option<PathBuf>,
}

impl PathPolicy {
    // {impl}`requirements.configuration.evidence_paths`
    // {impl}`requirements.configuration.workspace_location`
    // {impl}`requirements.authoring.credentials`
    pub(crate) fn load(root: &Path) -> io::Result<Self> {
        let Some(config) = crate::config::load_workspace_evidence(root)? else {
            return Ok(Self::default());
        };
        Ok(Self {
            allow: config
                .evidence
                .allow
                .iter()
                .map(|path| config.root.join(path))
                .collect(),
            disallow: config
                .evidence
                .disallow
                .iter()
                .map(|path| config.root.join(path))
                .collect(),
            credential_file: config.contains_credentials.then_some(config.path),
        })
    }

    // {impl}`requirements.configuration.evidence_paths`
    pub(crate) fn allowed(&self, requested: &Path, target: &Path) -> bool {
        self.allow
            .iter()
            .any(|rule| matches_rule(rule, requested, target))
    }

    // {impl}`requirements.configuration.evidence_paths`
    // {impl}`requirements.authoring.read_denial`
    pub(crate) fn check_disallow(&self, requested: &Path, target: &Path) -> io::Result<()> {
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

    // {impl}`requirements.authoring.credentials`
    // {impl}`requirements.authoring.source_tools`
    fn contains_credentials(&self, requested: &Path, target: &Path) -> bool {
        self.credential_file.as_ref().is_some_and(|path| {
            requested == path
                || target == path
                || fs::canonicalize(path).is_ok_and(|path| target == path)
        })
    }
}

pub(crate) struct SourceAccess {
    tracked: Option<BTreeSet<PathBuf>>,
    policy: PathPolicy,
    generated: Vec<PathBuf>,
}

impl SourceAccess {
    // {impl}`requirements.authoring.source_tools`
    // {impl}`requirements.configuration.evidence_paths`
    // {impl}`requirements.authoring.generated_inputs`
    pub(crate) fn load(root: &Path) -> io::Result<Self> {
        Ok(Self {
            tracked: tracked_sources(root)?,
            policy: PathPolicy::load(root)?,
            generated: generated_roots(root),
        })
    }

    // {impl}`requirements.authoring.source_tools`
    // {impl}`requirements.authoring.file_discovery`
    // {impl}`requirements.authoring.text_search`
    // {impl}`requirements.configuration.evidence_paths`
    // {impl}`requirements.authoring.read_denial`
    // {impl}`requirements.authoring.generated_inputs`
    // {impl}`requirements.authoring.credentials`
    pub(crate) fn path(&self, root: &Path, relative: &Path) -> io::Result<PathBuf> {
        let (requested, target) = resolve(root, relative)?;
        self.policy.check_disallow(&root.join(relative), &target)?;
        if self.policy.allowed(&requested, &target) {
            return Ok(target);
        }
        if self.policy.contains_credentials(&requested, &target) {
            return Err(denied(format!(
                "{} contains workspace credentials and is excluded from model inputs",
                relative.display()
            )));
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
        if !lexical_request.starts_with(root) || !target.starts_with(root) {
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
                        "{} contains no eligible sources; use git add to stage sources or update habeas.toml before retrying",
                        relative.display()
                    )));
                }
            } else {
                for path in [&lexical_request, &target] {
                    if !tracked.contains(path) {
                        return Err(denied(format!(
                            "{} is not tracked by Git; use git add to stage it or add an explicit habeas.toml allow rule before retrying",
                            path.strip_prefix(root).unwrap_or(path).display()
                        )));
                    }
                }
            }
        }
        Ok(target)
    }

    // {impl}`requirements.authoring.source_tools`
    // {impl}`requirements.authoring.mandatory_inputs`
    // {impl}`requirements.authoring.read_denial`
    pub(crate) fn file(&self, root: &Path, relative: &Path) -> io::Result<PathBuf> {
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
    // {impl}`requirements.authoring.source_tools`
    // {impl}`requirements.ci.authoring_inputs`
    pub(crate) fn omit_generated_evidence(&self, root: &Path, path: &Path) -> bool {
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
    // {impl}`requirements.authoring.source_tools`
    // {impl}`requirements.authoring.text_search`
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
    // {impl}`requirements.authoring.text_search`
    pub(crate) fn extra_roots(&self) -> impl Iterator<Item = &PathBuf> {
        self.policy.allow.iter().chain(self.generated.iter())
    }
}

// {impl}`requirements.configuration.evidence_paths`
fn matches_rule(rule: &Path, requested: &Path, target: &Path) -> bool {
    requested_location(rule)
        .is_ok_and(|rule| requested.starts_with(&rule) || target.starts_with(rule))
        || fs::canonicalize(rule)
            .is_ok_and(|rule| requested.starts_with(&rule) || target.starts_with(rule))
}

// {impl}`requirements.configuration.evidence_paths`
// {impl}`requirements.ci.input_containment`
fn requested_location(path: &Path) -> io::Result<PathBuf> {
    match (path.parent(), path.components().next_back()) {
        (Some(parent), Some(Component::Normal(name))) => Ok(fs::canonicalize(parent)?.join(name)),
        _ => fs::canonicalize(path),
    }
}

// {impl}`requirements.configuration.evidence_paths`
// {impl}`requirements.ci.input_containment`
pub(crate) fn normalize(path: &Path) -> PathBuf {
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

// {impl}`requirements.authoring.read_denial`
fn denied(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message.into())
}

// {impl}`requirements.authoring.source_tools`
// {impl}`requirements.authoring.file_discovery`
// {impl}`requirements.authoring.text_search`
// {impl}`requirements.ci.input_containment`
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

// {impl}`requirements.authoring.source_tools`
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

// {impl}`requirements.authoring.generated_inputs`
pub(crate) fn generated_roots(root: &Path) -> Vec<PathBuf> {
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
// {impl}`requirements.ci.input_containment`
// {impl}`requirements.authoring.generated_inputs`
pub(crate) fn resolve_evidence_file(root: &Path, file: &str) -> io::Result<PathBuf> {
    let root = fs::canonicalize(root)?;
    let (requested, target) = resolve(&root, Path::new(file))?;
    let policy = PathPolicy::load(&root)?;
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
        assert!(
            error.to_string().contains("cannot read Git index"),
            "{error}"
        );
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
        assert!(
            error.to_string().contains("cannot discover Git worktree"),
            "{error}"
        );
    }

    // {test}`requirements.configuration.evidence_paths`
    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.read_denial`
    // {test}`requirements.ci.input_containment`
    #[test]
    fn explicit_allow_overrides_defaults_but_disallow_wins_for_alias_targets() {
        let root = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("generated/private")).unwrap();
        fs::create_dir(root.path().join("evidence")).unwrap();
        fs::write(root.path().join("generated/ok.txt"), "generated\n").unwrap();
        fs::write(root.path().join("generated/private/secret.txt"), "secret\n").unwrap();
        fs::write(root.path().join("evidence/old.txt"), "old judgment\n").unwrap();
        fs::write(external.path().join("outside.txt"), "outside\n").unwrap();
        git(root.path(), &["init", "--quiet"]);
        let external_toml = toml::Value::String(external.path().display().to_string());
        fs::write(root.path().join("habeas.toml"), format!(
            "version = 1\n[evidence]\nallow = ['generated', 'evidence', {external_toml}]\ndisallow = ['generated/private']\n"
        )).unwrap();
        let root = fs::canonicalize(root.path()).unwrap();
        let access = SourceAccess::load(&root).unwrap();
        for path in [
            root.join("generated/ok.txt"),
            root.join("evidence/old.txt"),
            external.path().join("outside.txt"),
        ] {
            assert!(access.file(&root, &path).is_ok(), "{}", path.display());
            assert!(resolve_evidence_file(&root, path.to_str().unwrap()).is_ok());
        }
        let error = access
            .file(&root, Path::new("generated/private/secret.txt"))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(resolve_evidence_file(&root, "generated/private/secret.txt").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                root.join("generated/private/secret.txt"),
                root.join("generated/alias.txt"),
            )
            .unwrap();
            let error = access
                .file(&root, Path::new("generated/alias.txt"))
                .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        }
    }

    // {test}`requirements.authoring.credentials`
    // {test}`requirements.configuration.evidence_paths`
    // {test}`requirements.authoring.read_denial`
    // {test}`requirements.authoring.source_tools`
    #[test]
    fn workspace_credentials_are_excluded_unless_explicitly_allowed() {
        let root = tempfile::tempdir().unwrap();
        let config = "version = 1\n[authoring.configs.local]\nbackend = 'ollama'\nmodel = 'fixture'\napi_key = 'TEST_KEY'\n";
        fs::write(root.path().join("habeas.toml"), config).unwrap();
        let access = SourceAccess::load(root.path()).unwrap();
        let error = access
            .file(root.path(), Path::new("habeas.toml"))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(!error.to_string().contains("TEST_KEY"));
        fs::write(
            root.path().join("habeas.toml"),
            format!("{config}\n[evidence]\nallow = ['habeas.toml']\n"),
        )
        .unwrap();
        assert!(
            SourceAccess::load(root.path())
                .unwrap()
                .file(root.path(), Path::new("habeas.toml"))
                .is_ok()
        );
    }

    // {test}`requirements.configuration.evidence_paths`
    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.read_denial`
    // {test}`requirements.ci.input_containment`
    #[cfg(unix)]
    #[test]
    fn disallowed_ancestors_cannot_redirect_reads_into_allowed_targets() {
        let root = tempfile::tempdir().unwrap();
        for directory in ["private", "public", "sub"] {
            fs::create_dir(root.path().join(directory)).unwrap();
        }
        fs::write(root.path().join("public/file.txt"), "public source\n").unwrap();
        std::os::unix::fs::symlink("../public", root.path().join("private/link")).unwrap();
        fs::write(
            root.path().join("habeas.toml"),
            "version = 1\n[evidence]\nallow = ['public']\ndisallow = ['private']\n",
        )
        .unwrap();
        let access = SourceAccess::load(root.path()).unwrap();
        assert!(
            access
                .file(root.path(), Path::new("public/file.txt"))
                .is_ok()
        );
        for path in [
            "private/link/file.txt",
            "sub/../private/link/file.txt",
            "private/link",
        ] {
            let error = access.path(root.path(), Path::new(path)).unwrap_err();
            assert_eq!(
                error.kind(),
                io::ErrorKind::PermissionDenied,
                "{path}: {error}"
            );
            assert!(error.to_string().contains(path), "{error}");
        }
    }

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

    // {test}`requirements.configuration.evidence_paths`
    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.read_denial`
    // {test}`requirements.ci.input_containment`
    #[cfg(unix)]
    #[test]
    fn policy_resolves_symlinks_before_parent_components_without_allowing_wrong_subtrees() {
        let root = tempfile::tempdir().unwrap();
        for directory in ["actual/nested", "actual/private", "private"] {
            fs::create_dir_all(root.path().join(directory)).unwrap();
        }
        fs::write(root.path().join("actual/private/file.txt"), "actual\n").unwrap();
        fs::write(root.path().join("private/file.txt"), "wrong subtree\n").unwrap();
        std::os::unix::fs::symlink(root.path().join("actual/nested"), root.path().join("alias"))
            .unwrap();
        git(root.path(), &["init", "--quiet"]);
        fs::write(
            root.path().join("habeas.toml"),
            "version = 1\n[evidence]\nallow = ['alias/../private']\n",
        )
        .unwrap();
        let access = SourceAccess::load(root.path()).unwrap();
        assert!(
            access
                .file(root.path(), Path::new("actual/private/file.txt"))
                .is_ok()
        );
        assert!(
            access
                .file(root.path(), Path::new("alias/../private/file.txt"))
                .is_ok()
        );
        assert_eq!(
            access
                .file(root.path(), Path::new("private/file.txt"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        fs::write(
            root.path().join("habeas.toml"),
            "version = 1\n[evidence]\nallow = ['.']\ndisallow = ['alias/../private']\n",
        )
        .unwrap();
        let access = SourceAccess::load(root.path()).unwrap();
        for denied in ["actual/private/file.txt", "alias/../private/file.txt"] {
            assert_eq!(
                access
                    .file(root.path(), Path::new(denied))
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::PermissionDenied
            );
            assert!(resolve_evidence_file(root.path(), denied).is_err());
        }
        assert!(
            access
                .file(root.path(), Path::new("private/file.txt"))
                .is_ok()
        );
        fs::write(
            root.path().join("habeas.toml"),
            "version = 1\n[evidence]\nallow = ['actual/private']\n",
        )
        .unwrap();
        let access = SourceAccess::load(root.path()).unwrap();
        assert_eq!(
            access
                .file(
                    root.path(),
                    Path::new("actual/private/../../private/file.txt")
                )
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    // {test}`requirements.authoring.generated_inputs`
    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.read_denial`
    #[test]
    fn inherited_out_dir_is_ignored_and_metadata_failure_preserves_ordinary_reads() {
        if let Some(root) = std::env::var_os("HABEAS_TEST_OUT_DIR_PROBE_ROOT") {
            let root = PathBuf::from(root);
            let access = SourceAccess::load(&root).unwrap();
            assert!(access.generated.is_empty());
            assert!(access.file(&root, Path::new("tracked.txt")).is_ok());
            let inherited = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
            let error = access
                .file(&root, &inherited.join("unrelated.txt"))
                .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let unrelated = tempfile::tempdir().unwrap();
        fs::write(root.path().join("tracked.txt"), "ordinary source\n").unwrap();
        fs::write(
            root.path().join("Cargo.toml"),
            "deliberately invalid TOML [\n",
        )
        .unwrap();
        fs::write(
            unrelated.path().join("unrelated.txt"),
            "unrelated generated data\n",
        )
        .unwrap();
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["add", "."]);
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "authoring::access::tests::inherited_out_dir_is_ignored_and_metadata_failure_preserves_ordinary_reads", "--nocapture"])
            .env("HABEAS_TEST_OUT_DIR_PROBE_ROOT", root.path())
            .env("OUT_DIR", unrelated.path())
            .output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

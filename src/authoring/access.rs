//! Habeas configuration adapters for the SDK source-access machinery.
pub(crate) use habeas_sdk::access::*;
use std::io;
use std::path::{Path, PathBuf};

// {impl}`requirements.configuration.evidence_paths`
// {impl}`requirements.configuration.workspace_location`
// {impl}`requirements.configuration.loading`
fn load_policy(root: &Path) -> io::Result<PathPolicy> {
    let Some(config) = crate::config::load_workspace_evidence(root)? else {
        return Ok(PathPolicy::default());
    };
    Ok(PathPolicy::new(
        config
            .evidence
            .allow
            .iter()
            .map(|path| config.root.join(path))
            .collect(),
        config
            .evidence
            .disallow
            .iter()
            .map(|path| config.root.join(path))
            .collect(),
    ))
}

// {impl}`requirements.configuration.evidence_paths`
pub(crate) fn load_source_access(root: &Path) -> io::Result<SourceAccess> {
    SourceAccess::load(root, || load_policy(root))
}

// {impl}`requirements.authoring.deterministic_validation`
pub(crate) fn load_discovery_sources(root: &Path) -> io::Result<DiscoverySources> {
    DiscoverySources::load(root, || load_policy(root).unwrap_or_default())
}

// {impl}`requirements.ci.input_containment`
pub(crate) fn resolve_evidence_file(root: &Path, file: &str) -> io::Result<PathBuf> {
    let root = std::fs::canonicalize(root)?;
    habeas_sdk::access::resolve_evidence_file(&root, file, || load_policy(&root))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    // {cite}`habeas:util`
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
        let access = load_source_access(&root).unwrap();
        assert!(access.file(&root, Path::new("./inside.txt")).is_ok());
        assert!(access.file(&root, Path::new("../outside.txt")).is_err());
        assert!(access.file(&root, Path::new("outside.txt")).is_err());
    }

    // {test}`requirements.references.discovery`
    // {test}`requirements.authoring.credentials`
    #[test]
    fn discovery_considers_indexed_files_except_exclusions_including_credential_configuration() {
        let root = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(root.path()).unwrap();
        for file in [
            "src/lib.rs",
            "src/staged.rs",
            "src/deleted.rs",
            "src/untracked.rs",
            "crates/a/target/out.rs",
            "evidence/x.json",
            "docs/book/page.md",
        ] {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "text\n").unwrap();
        }
        fs::write(
            root.join("habeas.toml"),
            "version = 1\n[authoring.configs.cloud]\nprovider = \"bedrock\"\napi_key = \"secret\"\n",
        )
        .unwrap();
        git(&root, &["init", "--quiet"]);
        git(&root, &["add", "-f", "."]);
        git(&root, &["rm", "--cached", "--quiet", "src/untracked.rs"]);
        fs::remove_file(root.join("src/deleted.rs")).unwrap();
        let sources = load_discovery_sources(&root).unwrap();
        let mut errors = Vec::new();
        let files: Vec<_> = sources
            .files(&mut errors)
            .into_iter()
            .map(|path| path.strip_prefix(&root).unwrap().to_owned())
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            files,
            [
                Path::new("docs/book/page.md"),
                Path::new("habeas.toml"),
                Path::new("src/lib.rs"),
                Path::new("src/staged.rs"),
            ]
        );
        assert_eq!(
            sources.untracked().unwrap(),
            [root.join("src/untracked.rs")]
        );
    }

    // {test}`requirements.authoring.source_tools`
    #[test]
    fn unreadable_git_index_does_not_fall_back_to_untracked_access() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("source.txt"), "source\n").unwrap();
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["add", "."]);
        fs::write(root.path().join(".git/index"), "invalid index\n").unwrap();
        let error = load_source_access(root.path()).err().unwrap();
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
        let access = load_source_access(&root).unwrap();
        assert!(load_discovery_sources(&root).unwrap().indexed().is_none());
        assert!(access.file(&root, Path::new("source.txt")).is_ok());
        assert!(access.file(&root, Path::new("../outside.txt")).is_err());
    }

    // {test}`requirements.authoring.source_tools`
    #[test]
    fn invalid_git_configuration_does_not_fall_back_to_untracked_access() {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "--quiet"]);
        fs::write(root.path().join(".git/config"), "[invalid config\n").unwrap();
        let error = load_source_access(root.path()).err().unwrap();
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
        let access = load_source_access(&root).unwrap();
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

    // {test}`requirements.configuration.evidence_paths`
    // {test}`requirements.authoring.generated_inputs`
    // {test}`requirements.authoring.read_denial.build_output`
    #[test]
    fn explicit_allow_overrides_the_build_output_refusal() {
        let root = tempfile::tempdir().unwrap();
        for directory in ["target/doc", "book"] {
            fs::create_dir_all(root.path().join(directory)).unwrap();
            fs::write(root.path().join(directory).join("page.html"), "built\n").unwrap();
        }
        let root = fs::canonicalize(root.path()).unwrap();
        for file in ["target/doc/page.html", "book/page.html"] {
            let error = load_source_access(&root)
                .unwrap()
                .file(&root, Path::new(file))
                .unwrap_err();
            assert!(error.to_string().contains("holds build output"), "{error}");
        }
        fs::write(
            root.join("habeas.toml"),
            "version = 1\n[evidence]\nallow = ['target/doc', 'book/page.html']\n",
        )
        .unwrap();
        let access = load_source_access(&root).unwrap();
        for file in ["target/doc/page.html", "book/page.html"] {
            assert!(access.file(&root, Path::new(file)).is_ok(), "{file}");
        }
    }

    // {test}`requirements.configuration.evidence_paths`
    // {test}`requirements.authoring.tool_descriptions.special_cases`
    #[test]
    fn evidence_rules_are_literal_paths_not_globs() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/lib.rs"), "pub fn lib() {}\n").unwrap();
        fs::write(root.path().join("notes.txt"), "notes\n").unwrap();
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["add", "src/lib.rs"]);
        fs::write(
            root.path().join("habeas.toml"),
            "version = 1\n[evidence]\nallow = ['*.txt', 'note?.txt']\ndisallow = ['src/*', 'src/[a-z]*.rs']\n",
        )
        .unwrap();
        let root = fs::canonicalize(root.path()).unwrap();
        let access = load_source_access(&root).unwrap();
        // No pattern matches: the tracked file stays readable and the
        // untracked one stays refused by the default rules.
        assert!(access.file(&root, Path::new("src/lib.rs")).is_ok());
        assert!(access.file(&root, Path::new("notes.txt")).is_err());
        #[cfg(unix)]
        {
            // A rule names exactly the path written, metacharacters included.
            fs::write(root.join("*.txt"), "literal\n").unwrap();
            fs::write(root.join("src/*"), "literal\n").unwrap();
            let access = load_source_access(&root).unwrap();
            assert!(access.file(&root, Path::new("*.txt")).is_ok());
            let error = access.file(&root, Path::new("src/*")).unwrap_err();
            assert!(
                error.to_string().contains("disallowed by habeas.toml rule"),
                "{error}"
            );
            assert!(access.file(&root, Path::new("src/lib.rs")).is_ok());
        }
    }

    // {test}`requirements.authoring.credentials`
    // {test}`requirements.configuration.evidence_paths`
    // {test}`requirements.authoring.read_denial`
    // {test}`requirements.authoring.source_tools`
    #[test]
    fn workspace_configuration_with_credentials_is_readable_unless_disallowed() {
        let root = tempfile::tempdir().unwrap();
        let config = "version = 1\n[authoring.configs.local]\nbackend = 'ollama'\nmodel = 'fixture'\napi_key = 'TEST_KEY'\n";
        fs::write(root.path().join("habeas.toml"), config).unwrap();
        let access = load_source_access(root.path()).unwrap();
        assert!(access.file(root.path(), Path::new("habeas.toml")).is_ok());
        fs::write(
            root.path().join("habeas.toml"),
            format!("{config}\n[evidence]\ndisallow = ['habeas.toml']\n"),
        )
        .unwrap();
        let error = load_source_access(root.path())
            .unwrap()
            .file(root.path(), Path::new("habeas.toml"))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(!error.to_string().contains("TEST_KEY"));
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
        let access = load_source_access(root.path()).unwrap();
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
        let access = load_source_access(root.path()).unwrap();
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
        let access = load_source_access(root.path()).unwrap();
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
        let access = load_source_access(root.path()).unwrap();
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
            let access = load_source_access(&root).unwrap();
            assert!(access.extra_roots().next().is_none());
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

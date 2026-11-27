//! Habeas source access and configuration policy.
use std::io;
use std::path::{Path, PathBuf};
mod policy;
pub(crate) use policy::*;

// {impl}`requirements.configuration.evidence_paths`
// {impl}`requirements.configuration.workspace_location`
// {impl}`requirements.configuration.loading`
pub(crate) fn load_policy(root: &Path) -> io::Result<PathPolicy> {
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

// {impl}`requirements.authoring.deterministic_validation`
pub(crate) fn discovery_files(root: &Path, errors: &mut Vec<String>) -> Vec<PathBuf> {
    policy::discover_files(root, load_policy(root).unwrap_or_default(), errors)
}

// {impl}`requirements.ci.input_containment`
pub(crate) fn resolve_evidence_file(root: &Path, file: &str) -> io::Result<PathBuf> {
    let root = std::fs::canonicalize(root)?;
    policy::resolve_evidence_file(&root, file, || load_policy(&root))
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

    // {test}`requirements.configuration.workspace_location`
    // {test}`requirements.configuration.evidence_paths`
    #[test]
    fn evidence_rules_are_relative_to_the_selected_workspace_file() {
        let workspace = tempfile::tempdir().unwrap();
        let base = workspace.path().canonicalize().unwrap();
        let root = base.join("project");
        fs::create_dir(&root).unwrap();
        fs::write(base.join("allowed.txt"), "outside project").unwrap();
        fs::write(root.join("denied.txt"), "inside project").unwrap();
        fs::write(root.join("allowed.txt"), "unrelated local file").unwrap();
        fs::write(
            base.join("habeas.toml"),
            "version = 1\n[evidence]\nallow = ['allowed.txt']\ndisallow = ['project/denied.txt']\n",
        )
        .unwrap();
        assert_eq!(
            resolve_evidence_file(&root, "../allowed.txt").unwrap(),
            base.join("allowed.txt")
        );
        let error = resolve_evidence_file(&root, "denied.txt").unwrap_err();
        assert!(error.to_string().contains("disallowed"), "{error}");
        assert!(resolve_evidence_file(&root, "allowed.txt").is_ok());
    }

    // {test}`requirements.configuration.evidence_paths`
    #[test]
    fn evidence_rules_are_literal_paths_not_globs() {
        let workspace = tempfile::tempdir().unwrap();
        let root = workspace.path().join("project");
        std::fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "source").unwrap();
        fs::write(workspace.path().join("notes.txt"), "outside").unwrap();
        fs::write(
            root.join("habeas.toml"),
            "version = 1\n[evidence]\nallow = ['../*.txt']\ndisallow = ['src/*']\n",
        )
        .unwrap();
        assert!(resolve_evidence_file(&root, "src/lib.rs").is_ok());
        assert!(resolve_evidence_file(&root, "../notes.txt").is_err());
        #[cfg(unix)]
        {
            fs::write(workspace.path().join("*.txt"), "literal").unwrap();
            fs::write(root.join("src/*"), "literal").unwrap();
            assert!(resolve_evidence_file(&root, "../*.txt").is_ok());
            assert!(resolve_evidence_file(&root, "src/*").is_err());
        }
    }

    // {test}`requirements.references.discovery`
    #[test]
    fn discovery_includes_unstaged_and_staged_files_and_skips_generated_files() {
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
        let mut errors = Vec::new();
        let files: Vec<_> = discovery_files(&root, &mut errors)
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
                Path::new("src/untracked.rs"),
            ]
        );
    }
}

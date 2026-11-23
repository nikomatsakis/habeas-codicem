use std::{fs, path::Path, process::Command};
use trust_but_verify::{SourceScope, Sources, StructuralQuery, TextQuery, resource::ResourceId};

fn file(location: &str) -> ResourceId {
    ResourceId {
        scheme: "repo".into(),
        location: location.into(),
    }
}

fn git(root: &Path, args: &[&str]) {
    let result = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

// {test}`requirements.sdk.sources.provider`
// {test}`requirements.sdk.sources.local_provider`
// {test}`requirements.sdk.sources.provider_defaults.composition`
// {test}`requirements.sdk.sources.provider_results`
// {test}`requirements.sdk.graph.context-tools.list_sources.behavior`
// {test}`requirements.sdk.graph.context-tools.search_text.behavior`
#[tokio::test]
async fn gitignore_is_shared_by_all_operations_with_or_without_a_git_repository() {
    for repository in [false, true] {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::create_dir(root.path().join("skip")).unwrap();
        fs::write(root.path().join(".gitignore"), "*.txt\n!keep.txt\nskip/\n").unwrap();
        fs::write(root.path().join("src/.gitignore"), "!nested.txt\n").unwrap();
        fs::write(root.path().join("skip/.gitignore"), "!rescue.txt\n").unwrap();
        for name in [
            "keep.txt",
            "ignored.txt",
            "src/nested.txt",
            "src/ignored.txt",
            "skip/rescue.txt",
            ".visible",
        ] {
            fs::write(root.path().join(name), "needle\n").unwrap();
        }
        if repository {
            git(root.path(), &["init", "--quiet"]);
            git(root.path(), &["add", "--force", "ignored.txt"]);
        }
        let sources = Sources::new(root.path());
        let listed = sources.list(&SourceScope::default()).await.unwrap();
        let paths: Vec<_> = listed.iter().map(|id| id.location.as_str()).collect();
        assert!(paths.contains(&"keep.txt")); // Untracked and not ignored.
        assert!(paths.contains(&"src/nested.txt")); // Nested exception.
        assert!(paths.contains(&".visible")); // Dotfiles are ordinary sources.
        assert!(!paths.contains(&"ignored.txt")); // Ignored even if Git tracks it.
        assert!(!paths.contains(&"src/ignored.txt"));
        assert!(
            !paths
                .iter()
                .any(|path| path.starts_with("skip/") || path.starts_with(".git/"))
        );

        for name in ["ignored.txt", "src/ignored.txt", "skip/rescue.txt"] {
            assert!(sources.read(&file(name)).await.is_err(), "{name}");
            assert!(sources.parse(&file(name)).await.is_err(), "{name}");
            assert!(
                sources
                    .list(&SourceScope {
                        prefix: name.into()
                    })
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
        assert!(
            sources
                .list(&SourceScope {
                    prefix: "skip".into()
                })
                .await
                .unwrap()
                .is_empty()
        );
        assert!(sources.read(&file(".gitignore")).await.is_ok());
        if repository {
            assert!(sources.read(&file(".git/config")).await.is_err());
        }
        let found = sources
            .search_text(&TextQuery {
                scope: SourceScope::default(),
                pattern: "needle".into(),
                context_lines: 0,
                limit: None,
            })
            .await
            .unwrap();
        assert!(found.complete);
        assert_eq!(found.matches.len(), 3);
        let scoped = sources
            .search_text(&TextQuery {
                scope: SourceScope {
                    prefix: "./src/../src".into(),
                },
                pattern: "needle".into(),
                context_lines: 0,
                limit: None,
            })
            .await
            .unwrap();
        assert_eq!(scoped.matches.len(), 1);
        assert_eq!(scoped.matches[0].resource.location, "src/nested.txt");
    }
}

// {test}`requirements.sdk.sources.provider.errors`
// {test}`requirements.sdk.sources.provider_results.parse_snapshot`
// {test}`requirements.sdk.sources.provider_results.search_completeness`
// {test}`requirements.sdk.sources.source_addresses`
#[tokio::test]
async fn empty_sources_errors_and_retained_snapshots_remain_distinct() {
    let root = tempfile::tempdir().unwrap();
    let sources = Sources::new(root.path());
    assert!(
        sources
            .list(&SourceScope::default())
            .await
            .unwrap()
            .is_empty()
    );
    fs::write(root.path().join("empty.txt"), "").unwrap();
    assert!(
        Sources::new(root.path().join("empty.txt"))
            .list(&SourceScope::default())
            .await
            .is_err()
    );
    assert!(
        sources
            .read(&file("empty.txt"))
            .await
            .unwrap()
            .bytes
            .is_empty()
    );
    fs::write(
        root.path().join("note.txt"),
        "needle first\nneedle second\n",
    )
    .unwrap();
    let parsed = sources.parse(&file("note.txt")).await.unwrap();
    fs::write(root.path().join("note.txt"), "changed\n").unwrap();
    assert_eq!(
        parsed.source.text().unwrap(),
        "needle first\nneedle second\n"
    );
    assert_eq!(
        sources
            .read(&file("note.txt"))
            .await
            .unwrap()
            .text()
            .unwrap(),
        "changed\n"
    );
    assert!(sources.read(&file("missing.txt")).await.is_err());
    assert!(sources.read(&file("../outside.txt")).await.is_err());
    assert!(
        sources
            .read(&file(&root.path().join("note.txt").to_string_lossy()))
            .await
            .is_err()
    );
    assert!(
        sources
            .read(&ResourceId {
                scheme: "custom".into(),
                location: "note.txt".into()
            })
            .await
            .is_err()
    );
    assert!(
        sources
            .list(&SourceScope {
                prefix: "missing".into()
            })
            .await
            .is_err()
    );
    assert!(
        Sources::new(root.path().join("missing"))
            .list(&SourceScope::default())
            .await
            .is_err()
    );
    fs::write(root.path().join("note.txt"), "needle\nneedle\n").unwrap();
    let query = TextQuery {
        scope: SourceScope::default(),
        pattern: "needle".into(),
        context_lines: 0,
        limit: Some(1),
    };
    let result = sources.search_text(&query).await.unwrap();
    assert_eq!(result.matches.len(), 1);
    assert!(!result.complete);
    let invalid = TextQuery {
        pattern: "[".into(),
        ..query
    };
    assert!(sources.search_text(&invalid).await.is_err());
}

// {test}`requirements.sdk.sources.provider_defaults`
// {test}`requirements.sdk.sources.local_provider`
// {test}`requirements.sdk.graph.context-tools.search_structure.behavior`
#[tokio::test]
async fn built_in_languages_and_structural_search_use_the_project_scope() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(".gitignore"), "hidden.rs\n").unwrap();
    fs::write(root.path().join("visible.rs"), "fn visible() { call(); }\n").unwrap();
    fs::write(root.path().join("hidden.rs"), "fn hidden() { call(); }\n").unwrap();
    fs::write(root.path().join("note.unknown"), "first\n\nsecond\n").unwrap();
    let sources = Sources::new(root.path());
    let plain = sources.parse(&file("note.unknown")).await.unwrap();
    assert_eq!(plain.outline.profile.name, "plain-text");
    assert_eq!(plain.outline.nodes.len(), 3); // Root and two nonblank lines.
    let rust = sources.parse(&file("visible.rs")).await.unwrap();
    let query = StructuralQuery {
        scope: SourceScope::default(),
        language: "rust".into(),
        rule: serde_json::json!({"rule":{"pattern":"call()"}})
            .as_object()
            .unwrap()
            .clone(),
        limit: None,
    };
    #[cfg(feature = "default-languages")]
    {
        assert_eq!(rust.outline.profile.name, "rust");
        let found = sources.search_structure(&query).await.unwrap();
        assert!(found.complete);
        assert_eq!(found.matches.len(), 1);
        assert_eq!(found.matches[0].node.resource.location, "visible.rs");
    }
    #[cfg(not(feature = "default-languages"))]
    {
        assert_eq!(rust.outline.profile.name, "plain-text");
        assert!(sources.search_structure(&query).await.is_err());
    }
}

// {test}`requirements.sdk.sources.local_provider`
#[cfg(unix)]
#[tokio::test]
async fn symlinks_cannot_reopen_ignored_files_or_leave_the_project() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(root.path().join(".gitignore"), "secret.txt\n").unwrap();
    fs::write(root.path().join("secret.txt"), "ignored").unwrap();
    fs::write(root.path().join("visible.txt"), "available").unwrap();
    fs::write(outside.path().join("file.txt"), "outside").unwrap();
    symlink(
        root.path().join("secret.txt"),
        root.path().join("alias.txt"),
    )
    .unwrap();
    symlink(
        root.path().join("visible.txt"),
        root.path().join("included.txt"),
    )
    .unwrap();
    symlink(outside.path(), root.path().join("external")).unwrap();
    let sources = Sources::new(root.path());
    let listed = sources.list(&SourceScope::default()).await.unwrap();
    assert!(listed.iter().any(|id| id.location == "included.txt"));
    assert!(
        !listed
            .iter()
            .any(|id| id.location == "alias.txt" || id.location.starts_with("external/"))
    );
    assert_eq!(
        sources
            .read(&file("included.txt"))
            .await
            .unwrap()
            .text()
            .unwrap(),
        "available"
    );
    assert!(sources.read(&file("alias.txt")).await.is_err());
    assert!(sources.read(&file("external/file.txt")).await.is_err());
    assert!(
        sources
            .list(&SourceScope {
                prefix: "external".into()
            })
            .await
            .is_err()
    );
}

// {test}`requirements.sdk.sources.provider.errors`
// {test}`requirements.sdk.sources.provider_results`
#[tokio::test]
async fn scoped_inspection_skips_unrelated_directories_and_reports_relevant_ignore_errors() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::create_dir(root.path().join("broken")).unwrap();
    fs::write(root.path().join("src/one.txt"), "available").unwrap();
    fs::write(root.path().join("broken/target.txt"), "target").unwrap();
    fs::write(root.path().join("broken/.gitignore"), "{a,b\n").unwrap();
    let sources = Sources::new(root.path());
    let scope = SourceScope {
        prefix: "src".into(),
    };
    let listed = sources.list(&scope).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].location, "src/one.txt");
    assert!(sources.list(&SourceScope::default()).await.is_err());
    assert!(sources.read(&file("broken/target.txt")).await.is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            root.path().join("broken/target.txt"),
            root.path().join("src/alias.txt"),
        )
        .unwrap();
        assert!(sources.list(&scope).await.is_err());
    }
}

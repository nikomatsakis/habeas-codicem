use std::{fs, path::Path, path::PathBuf, process::Command};
use trust_but_verify::sources::SourceScope;
use trust_but_verify::sources::Sources;
use trust_but_verify::sources::StructuralQuery;
use trust_but_verify::sources::TextQuery;

fn file(location: &str) -> PathBuf {
    location.into()
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

// {test}`requirements.sdk.graph.search.TextQuery`
// {test}`requirements.sdk.graph.search.TextMatch`
// {test}`requirements.sdk.graph.search.TextMatches`
// {test}`requirements.sdk.graph.search.search_text.behavior`
// {test}`requirements.sdk.graph.search.source_scope`
#[tokio::test]
async fn scoped_text_results_keep_line_numbers_context_and_completeness() {
    let root = tempfile::tempdir().unwrap();
    let sources = Sources::new(root.path());
    let scope = SourceScope::under("one.txt");
    assert!(sources.list(&scope).await.is_err());
    fs::write(root.path().join("one.txt"), "hit1\nmiddle\nhit2\n").unwrap();
    fs::write(root.path().join("two.txt"), "hit3\n").unwrap();
    let query = TextQuery::new("^hit[0-9]$".into());
    assert_eq!(query.limit, None);
    assert_eq!(query.context_lines, 0);
    let all = sources.search_text(&query).await.unwrap();
    assert!(all.complete);
    assert_eq!(all.matches.len(), 3);
    let bounded = sources
        .search_text(&TextQuery::new(query.pattern.clone()).limit(2))
        .await
        .unwrap();
    assert_eq!(bounded.matches.len(), 2);
    assert!(!bounded.complete);
    assert!(
        all.matches
            .iter()
            .all(|hit| hit.excerpt.starts_with("hit") && hit.excerpt.len() == 4)
    );
    let scoped = query.scope(scope.clone()).context_lines(1).limit(2);
    let found = sources.search_text(&scoped).await.unwrap();
    assert!(found.complete);
    let entries: Vec<_> = found
        .matches
        .iter()
        .map(|hit| (hit.path.as_path(), hit.line, hit.excerpt.as_str()))
        .collect();
    assert_eq!(
        entries,
        [
            (Path::new("one.txt"), 1, "hit1\nmiddle"),
            (Path::new("one.txt"), 3, "middle\nhit2")
        ]
    );
    let limited = sources.search_text(&scoped.limit(1)).await.unwrap();
    assert_eq!(limited.matches.len(), 1);
    assert!(!limited.complete);
    fs::remove_file(root.path().join("one.txt")).unwrap();
    assert!(sources.list(&scope).await.is_err(), "{scope:?}");
}

// {test}`requirements.sdk.sources.provider`
// {test}`requirements.sdk.sources.provider_defaults`
// {test}`requirements.sdk.sources.local_provider`
// {test}`requirements.sdk.sources.source_addresses`
// {test}`requirements.sdk.sources.provider_defaults.composition`
// {test}`requirements.sdk.sources.provider_results`
// {test}`requirements.sdk.graph.validation-context.list_sources.behavior`
// {test}`requirements.sdk.graph.search.search_text.fn`
// {test}`requirements.sdk.graph.search.search_text.behavior`
// {test}`requirements.sdk.graph.search.source_scope`
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
        let listed = sources.list(&SourceScope::all()).await.unwrap();
        assert_eq!(listed, sources.list(&SourceScope::default()).await.unwrap());
        let paths: Vec<_> = listed.iter().map(|id| id.to_str().unwrap()).collect();
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
                    .list(&SourceScope::under(name))
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
        assert!(
            sources
                .list(&SourceScope::under("skip"))
                .await
                .unwrap()
                .is_empty()
        );
        assert!(sources.read(&file(".gitignore")).await.is_ok());
        if repository {
            assert!(sources.read(&file(".git/config")).await.is_err());
        }
        let found = sources
            .search_text(&TextQuery::new("needle".into()))
            .await
            .unwrap();
        assert!(found.complete);
        assert_eq!(found.matches.len(), 3);
        let regex = sources
            .search_text(&TextQuery::new("^n[e]{2}dle$".into()))
            .await
            .unwrap();
        assert_eq!(regex.matches.len(), 3);
        for prefix in ["ignored.txt", "src/ignored.txt", "skip"] {
            let excluded = sources
                .search_text(&TextQuery::new("needle".into()).scope(SourceScope::under(prefix)))
                .await
                .unwrap();
            assert!(excluded.complete);
            assert!(excluded.matches.is_empty(), "{prefix}");
        }
        if repository {
            assert!(
                sources
                    .search_text(&TextQuery::new(".".into()).scope(SourceScope::under(".git")))
                    .await
                    .unwrap()
                    .matches
                    .is_empty()
            );
        }
        assert!(
            sources
                .search_text(&TextQuery::new(".".into()).scope(SourceScope::under("..")))
                .await
                .is_err()
        );
        fs::create_dir(root.path().join("src-old")).unwrap();
        fs::write(root.path().join("src-old/other.rs"), "needle\n").unwrap();
        let scoped = sources
            .search_text(&TextQuery::new("needle".into()).scope(SourceScope::under("./src/../src")))
            .await
            .unwrap();
        assert_eq!(scoped.matches.len(), 1);
        assert_eq!(scoped.matches[0].path, Path::new("src/nested.txt"));
    }
}

// {test}`requirements.sdk.sources.provider.errors`
// {test}`requirements.sdk.sources.provider_results.parse_snapshot`
// {test}`requirements.sdk.sources.provider_results.search_completeness`
// {test}`requirements.sdk.sources.source_addresses`
// {test}`requirements.sdk.graph.validation-context.outline_source.fn`
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
    assert_eq!(parsed.id().path, Path::new("note.txt"));
    assert_eq!(parsed.id().selector, "");
    fs::write(root.path().join("note.txt"), "changed\n").unwrap();
    assert_eq!(parsed.text(), "needle first\nneedle second\n");
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
    assert!(sources.read(&PathBuf::from("../note.txt")).await.is_err());
    assert!(sources.list(&SourceScope::under("missing")).await.is_err());
    assert!(
        Sources::new(root.path().join("missing"))
            .list(&SourceScope::default())
            .await
            .is_err()
    );
    fs::write(root.path().join("note.txt"), "needle\nneedle\n").unwrap();
    let query = TextQuery::new("needle".into()).limit(1);
    let result = sources.search_text(&query).await.unwrap();
    assert_eq!(result.matches.len(), 1);
    assert!(!result.complete);
    let invalid = TextQuery::new("[".into()).limit(1);
    assert!(sources.search_text(&invalid).await.is_err());
}

// {test}`requirements.sdk.sources.provider_defaults`
// {test}`requirements.sdk.sources.provider`
// {test}`requirements.sdk.sources.provider_defaults.composition`
// {test}`requirements.sdk.sources.local_provider`
// {test}`requirements.sdk.sources.source_addresses`
// {test}`requirements.sdk.graph.search.search_structure.behavior`
// {test}`requirements.sdk.graph.search.StructuralQuery`
// {test}`requirements.sdk.graph.search.StructuralMatch`
// {test}`requirements.sdk.graph.search.StructuralMatches`
// {test}`requirements.sdk.graph.validation-context.outline_source.fn`
#[tokio::test]
async fn built_in_languages_and_structural_search_use_the_project_scope() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(".gitignore"), "hidden.rs\nskip/\n").unwrap();
    fs::create_dir(root.path().join("skip")).unwrap();
    fs::write(
        root.path().join("skip/hidden.rs"),
        "fn skipped() { call(); }\n",
    )
    .unwrap();
    fs::create_dir(root.path().join(".git")).unwrap();
    fs::write(
        root.path().join(".git/metadata.rs"),
        "fn metadata() { call(); }\n",
    )
    .unwrap();
    fs::write(root.path().join("visible.rs"), "fn visible() { call(); }\n").unwrap();
    fs::write(root.path().join("hidden.rs"), "fn hidden() { call(); }\n").unwrap();
    fs::write(root.path().join("note.unknown"), "first\n\nsecond\n").unwrap();
    let sources = Sources::new(root.path());
    let plain = sources.parse(&file("note.unknown")).await.unwrap();
    assert_eq!(plain.fingerprint_scheme(), "plain-text-structural-v1");
    assert_eq!(plain.outline().items.len(), 3); // Root and two nonblank lines.
    let rust = sources.parse(&file("visible.rs")).await.unwrap();
    let query = StructuralQuery::new(
        "rust".into(),
        serde_json::json!({"rule":{"pattern":"call()"}})
            .as_object()
            .unwrap()
            .clone(),
    );
    #[cfg(feature = "default-languages")]
    {
        assert_eq!(query.limit, None);
        assert_eq!(rust.fingerprint_scheme(), "rust-structural-v1");
        let found = sources.search_structure(&query).await.unwrap();
        assert!(found.complete);
        assert_eq!(found.matches.len(), 1);
        assert_eq!(found.matches[0].node.path, Path::new("visible.rs"));
        assert_eq!(found.matches[0].node.selector, "fn[visible]");
        assert_eq!(found.matches[0].excerpt, "call()");
        for (limit, complete) in [(0, false), (1, true)] {
            let bounded =
                StructuralQuery::new(query.language.clone(), query.rule.clone()).limit(limit);
            let result = sources.search_structure(&bounded).await.unwrap();
            assert_eq!((result.matches.len(), result.complete), (limit, complete));
        }
        for prefix in ["hidden.rs", "skip", ".git"] {
            let scoped = sources
                .search_structure(
                    &StructuralQuery::new(query.language.clone(), query.rule.clone())
                        .scope(SourceScope::under(prefix)),
                )
                .await
                .unwrap();
            assert!(scoped.complete);
            assert!(scoped.matches.is_empty(), "{prefix}");
        }
        let visible = sources
            .search_structure(
                &StructuralQuery::new(query.language.clone(), query.rule.clone())
                    .scope(SourceScope::under("visible.rs")),
            )
            .await
            .unwrap();
        assert_eq!(visible.matches[0].node.path, Path::new("visible.rs"));
        fs::write(root.path().join("another.rs"), "fn other() { call(); }\n").unwrap();
        fs::write(root.path().join("script.py"), "def script():\n    call()\n").unwrap();
        let all_rust = sources.search_structure(&query).await.unwrap();
        assert_eq!(all_rust.matches.len(), 2);
        assert!(all_rust.complete);
        let bounded = StructuralQuery::new(query.language.clone(), query.rule.clone()).limit(1);
        let subset = sources.search_structure(&bounded).await.unwrap();
        assert_eq!(subset.matches.len(), 1);
        assert!(!subset.complete);
        let python = StructuralQuery::new("python".into(), query.rule.clone());
        let found = sources.search_structure(&python).await.unwrap();
        assert_eq!(found.matches.len(), 1);
        assert_eq!(found.matches[0].node.path, Path::new("script.py"));
        assert!(
            sources
                .search_structure(
                    &StructuralQuery::new(query.language.clone(), query.rule.clone())
                        .scope(SourceScope::under(".."))
                )
                .await
                .is_err()
        );
    }
    #[cfg(not(feature = "default-languages"))]
    {
        assert_eq!(rust.fingerprint_scheme(), "plain-text-structural-v1");
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
    assert!(listed.iter().any(|id| id == Path::new("included.txt")));
    assert!(
        !listed
            .iter()
            .any(|id| id == Path::new("alias.txt") || id.starts_with("external"))
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
    assert!(sources.list(&SourceScope::under("external")).await.is_err());
}

// {test}`requirements.sdk.sources.provider.errors`
// {test}`requirements.sdk.sources.provider_results`
// {test}`requirements.sdk.sources.source_addresses`
#[tokio::test]
async fn scoped_inspection_skips_unrelated_directories_and_reports_relevant_ignore_errors() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::create_dir(root.path().join("broken")).unwrap();
    fs::write(root.path().join("src/one.txt"), "available").unwrap();
    fs::write(root.path().join("broken/target.txt"), "target").unwrap();
    fs::write(root.path().join("broken/.gitignore"), "{a,b\n").unwrap();
    let sources = Sources::new(root.path());
    let scope = SourceScope::under("src");
    let listed = sources.list(&scope).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0], Path::new("src/one.txt"));
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

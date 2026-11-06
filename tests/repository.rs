//! Checks of this repository's own build, book, and workflow configuration.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use mdbook_preprocessor::PreprocessorContext;
use mdbook_preprocessor::book::{Book, BookItem, Chapter};
use mdbook_preprocessor::config::Config;
use tempfile::TempDir;

// {test}`requirements.cli.mdbook`
// {test}`requirements.layout.project`
// {test}`requirements.layout.repository_links`
// {test}`requirements.integration.ci`
// {test}`requirements.integration.release`
// {test}`requirements.integration.symposium_cli`
fn repository_file(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap()
}

// {test}`requirements.cli.mdbook`
// {test}`requirements.layout.project`
// {test}`requirements.layout.repository_links`
// {test}`requirements.integration.ci`
// {test}`requirements.integration.release`
// {test}`requirements.integration.symposium_cli`
fn repository_toml(path: &str) -> toml::Table {
    repository_file(path).parse().unwrap()
}

/// The lines of one top-level job in a GitHub Actions workflow, from its
/// `  <name>:` line to the next top-level job.
// {test}`requirements.integration.ci`
// {test}`requirements.integration.release`
fn workflow_job(workflow: &str, name: &str) -> String {
    let header = format!("  {name}:");
    // The header may carry a trailing comment, such as a reference.
    let mut lines = workflow
        .lines()
        .skip_while(|line| *line != header && !line.starts_with(&format!("{header} #")));
    let first = lines
        .next()
        .unwrap_or_else(|| panic!("job {name} missing:\n{workflow}"));
    let mut job = format!("{first}\n");
    for line in lines.take_while(|line| line.is_empty() || line.starts_with("   ")) {
        job.push_str(line);
        job.push('\n');
    }
    job
}

// {test}`requirements.cli.mdbook`
#[test]
fn mdbook_preprocessor_renders_definitions_between_stdin_and_stdout() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("md")).unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\nname = \"fixture\"\n").unwrap();
    fs::write(root.join("book.toml"), "[book]\nsrc = \"md\"\n").unwrap();
    let content =
        ":::{req} ready\nReady MUST work.\n:::\n\n:::{req} cites\nSee CITE`rules.ready`.\n:::\n"
            .replace("CITE", "{cite}");
    fs::write(root.join("md/rules.md"), &content).unwrap();
    let mut book = Book::new();
    book.push_item(Chapter::new("Rules", content, "rules.md", Vec::new()));
    let context = PreprocessorContext::new(root.to_path_buf(), Config::default(), "html".into());

    let mut child = Command::new(env!("CARGO_BIN_EXE_habeas"))
        .arg("mdbook")
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&(&context, &book)).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rendered: Book = serde_json::from_slice(&output.stdout).unwrap();
    let BookItem::Chapter(chapter) = &rendered.items[0] else {
        panic!("{rendered:?}");
    };
    assert!(!chapter.content.contains(":::{req}"), "{}", chapter.content);
    assert!(
        chapter.content.contains("https://img.shields.io/badge/"),
        "{}",
        chapter.content
    );
    assert!(
        chapter.content.contains("cite") && chapter.content.contains("rules.ready"),
        "{}",
        chapter.content
    );
    assert_eq!(
        chapter
            .sub_items
            .iter()
            .filter_map(|item| match item {
                BookItem::Chapter(detail) => Some(detail.name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        ["rules.ready", "rules.cites"]
    );
}

// {test}`requirements.cli.mdbook`
// {test}`requirements.layout.project`
#[test]
fn project_book_uses_the_repository_local_preprocessor_alias() {
    let book = repository_toml("book.toml");
    assert_eq!(book["book"]["src"].as_str(), Some("md"));
    let preprocessor = &book["preprocessor"]["habeas"];
    assert_eq!(
        preprocessor["command"].as_str(),
        Some("cargo habeas mdbook")
    );
    assert_eq!(
        preprocessor["renderers"],
        toml::Value::Array(vec!["html".into()])
    );
    let cargo = repository_toml(".cargo/config.toml");
    assert_eq!(
        cargo["alias"]["habeas"].as_str(),
        Some("run --release -p habeas --")
    );

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(root.join("md/SUMMARY.md").is_file());
    assert!(root.join("src/lib.rs").is_file());
    assert!(root.join("src/main.rs").is_file());
}

// {test}`requirements.layout.project`
#[test]
fn crate_targets_are_declared_in_src() {
    let manifest = repository_toml("Cargo.toml");
    assert_eq!(manifest["lib"]["path"].as_str(), Some("src/lib.rs"));
    let bins = manifest["bin"].as_array().unwrap();
    assert_eq!(bins.len(), 1);
    assert_eq!(bins[0]["name"].as_str(), Some("habeas"));
    assert_eq!(bins[0]["path"].as_str(), Some("src/main.rs"));
}

// {test}`requirements.layout.repository_links`
#[test]
fn book_links_to_the_repository_and_main_branch_edit_pages() {
    let book = repository_toml("book.toml");
    let html = &book["output"]["html"];
    assert_eq!(
        html["git-repository-url"].as_str(),
        Some("https://github.com/nikomatsakis/habeas-codicem")
    );
    assert_eq!(
        html["edit-url-template"].as_str(),
        Some("https://github.com/nikomatsakis/habeas-codicem/edit/main/{path}")
    );
}

// {test}`requirements.integration.ci`
#[test]
fn ci_tests_pull_requests_and_main_then_deploys_the_book_from_main() {
    let workflow = repository_file(".github/workflows/ci.yml");
    assert!(
        workflow.contains("\non:\n  push:\n    branches: [main]\n  pull_request:\n\n"),
        "{workflow}"
    );

    let test = workflow_job(&workflow, "test-and-build");
    let steps = [
        "run: node --test .github/scripts/autofix.test.mjs",
        "run: cargo test --locked --workspace",
        "run: mdbook test",
        "run: mdbook build",
        "uses: actions/upload-pages-artifact@v4",
    ];
    let positions: Vec<_> = steps
        .iter()
        .map(|step| test.find(step).unwrap_or_else(|| panic!("{step}:\n{test}")))
        .collect();
    assert!(positions.is_sorted(), "{test}");
    assert!(test.contains("with:\n          path: book\n"), "{test}");

    let hook = workflow_job(&workflow, "symposium-hook");
    assert!(
        hook.contains("run: python3 tests/symposium_hook_smoke.py"),
        "{hook}"
    );

    let deploy = workflow_job(&workflow, "deploy");
    assert!(
        deploy.contains("if: github.event_name == 'push' && github.ref == 'refs/heads/main'"),
        "{deploy}"
    );
    assert!(
        deploy.contains("needs: [test-and-build, symposium-hook]"),
        "{deploy}"
    );
    assert!(deploy.contains("uses: actions/deploy-pages@v4"), "{deploy}");

    let book = repository_toml("book.toml");
    assert_eq!(
        book["output"]["html"]["site-url"].as_str(),
        Some("/habeas-codicem/")
    );
}

// {test}`requirements.integration.release`
#[test]
fn release_workflow_runs_release_plz_with_secret_token_and_trusted_publishing() {
    let workflow = repository_file(".github/workflows/release-plz.yml");
    assert!(
        workflow.contains("\non:\n  push:\n    branches:\n      - main\n\n"),
        "{workflow}"
    );
    assert!(!workflow.contains("pull_request"), "{workflow}");

    let release = workflow_job(&workflow, "release-plz-release");
    assert!(release.contains("id-token: write"), "{release}");
    assert!(release.contains("command: release\n"), "{release}");
    assert!(
        release.contains("GITHUB_TOKEN: ${{ secrets.RELEASE_PLZ_TOKEN }}"),
        "{release}"
    );

    let pr = workflow_job(&workflow, "release-plz-pr");
    assert!(pr.contains("command: release-pr\n"), "{pr}");
    assert!(
        pr.contains("GITHUB_TOKEN: ${{ secrets.RELEASE_PLZ_TOKEN }}"),
        "{pr}"
    );
    assert!(pr.contains("- *checkout\n"), "{pr}");
    for job in [&release, &pr] {
        assert!(
            job.contains("if: ${{ github.repository_owner == 'nikomatsakis' }}"),
            "{job}"
        );
    }
    assert!(
        pr.contains("concurrency:\n      group: release-plz-${{ github.ref }}\n      cancel-in-progress: false\n"),
        "{pr}"
    );
    assert!(
        release.contains("token: ${{ secrets.RELEASE_PLZ_TOKEN }}"),
        "the shared checkout step uses the secret token too: {release}"
    );

    let config = repository_toml("release-plz.toml");
    assert_eq!(config["workspace"]["release_always"].as_bool(), Some(false));
}

// {test}`requirements.integration.symposium_cli`
#[test]
fn symposium_plugin_installs_the_cli_globally_for_its_session_start_hook() {
    let manifest = repository_toml("Cargo.toml");
    let symposium = &manifest["package"]["metadata"]["symposium"];
    let installations = symposium["installations"].as_array().unwrap();
    assert_eq!(installations.len(), 1);
    let installation = &installations[0];
    assert_eq!(installation["source"].as_str(), Some("cargo"));
    assert_eq!(installation["crate"].as_str(), Some("habeas"));
    assert_eq!(installation["executable"].as_str(), Some("habeas"));
    assert_eq!(installation["global"].as_bool(), Some(true));

    let hooks = symposium["hooks"].as_array().unwrap();
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0]["event"].as_str(), Some("SessionStart"));
    assert_eq!(hooks[0]["command"], installation["executable"]);
    assert_eq!(
        hooks[0]["args"],
        toml::Value::Array(vec!["hook".into(), "session-start".into()])
    );
}

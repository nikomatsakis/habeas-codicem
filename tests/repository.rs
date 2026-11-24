//! Checks of this repository's own build, book, and workflow configuration.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use mdbook_preprocessor::PreprocessorContext;
use mdbook_preprocessor::book::{Book, BookItem, Chapter};
use mdbook_preprocessor::config::Config;
use tempfile::TempDir;

// {cite}`habeas:util`
fn repository_file(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap()
}

// {cite}`habeas:util`
fn repository_toml(path: &str) -> toml::Table {
    repository_file(path).parse().unwrap()
}

// {test}`requirements.install.symposium`
// {test}`requirements.install.symposium.skill_hook`
#[test]
fn getting_started_documents_symposium_setup_and_skill_migration() {
    for path in ["README.md", "md/getting_started.md"] {
        let text = repository_file(path);
        for phrase in [
            "cargo install symposium",
            "cargo agents init",
            "cargo add --dev habeas",
            "cargo agents sync",
            "Enable",
            "Ask me later",
            "session",
            "hook",
            "`habeas` skill",
            "habeas setup",
            "updating",
            "local edits",
        ] {
            assert!(text.contains(phrase), "{path} omits {phrase}");
        }
        let paragraphs = text
            .split("\n\n")
            .map(|paragraph| paragraph.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>();
        assert!(
            paragraphs
                .iter()
                .any(|paragraph| paragraph.contains("hook tells")
                    && paragraph.contains("load the `habeas` skill before starting any work")
                    && paragraph.contains("session")),
            "{path}: hook must direct skill loading"
        );
        let advice = if path == "README.md" {
            "The hook also checks the project and advises the agent to run `habeas setup` when configuration is incomplete"
        } else {
            "The hook also checks setup: if configuration is missing or incompatible, it advises the agent to run `habeas setup`"
        };
        assert!(
            text.contains(advice),
            "{path}: missing conditional setup advice"
        );
        assert!(
            paragraphs
                .iter()
                .any(|paragraph| paragraph.contains("cargo agents sync")
                    && paragraph.contains("after updating the Habeas dependency to refresh")),
            "{path}: missing update/sync/refresh guidance"
        );
        assert!(
            paragraphs.iter().any(|paragraph| {
                paragraph.contains(".agents/skills/habeas-*")
                    && paragraph.contains("local edits")
                    && paragraph.contains("different names or elsewhere")
                    && paragraph
                        .find("Save")
                        .zip(paragraph.find("remove those old copies"))
                        .is_some_and(|(save, remove)| save < remove)
                    && paragraph
                        .find("remove those old copies")
                        .zip(paragraph.rfind("sync"))
                        .is_some_and(|(remove, sync)| remove < sync)
            }),
            "{path}: preserve customizations before removing copies and syncing"
        );
    }
}

// {test}`requirements.test-coverage.check.guidance`
// {test}`requirements.test-coverage.check.exceptions.setup`
#[test]
fn shared_agent_guidance_documents_post_suite_checks_and_standard_explanations() {
    let skill = repository_file("assets/skills/habeas/SKILL.md");
    for phrase in [
        "It collects coverage for the full workspace, then checks it",
        "`habeas coverage -- --workspace` followed, only on success, by\n`habeas coverage-check`",
        "static `habeas::validate()` test requires no prior execution coverage",
        "Partly covered\nfunctions need no exception when their file meets its target",
        "standard\n`{cite}` of `habeas:coverage`",
        "rationale in fenced commentary or a Rust\ndoc comment",
        "citation review with `habeas author`",
        "Only current accepted review evidence permits the exception",
        "No\nproject declaration or extra configuration is needed",
    ] {
        assert!(skill.contains(phrase), "agent guidance omits {phrase}");
    }
    let reference = repository_file("md/reference.md");
    for phrase in [
        "whole workspace, then check the saved results",
        "Run the second command only after the first succeeds",
        "it need not run last",
    ] {
        assert!(reference.contains(phrase), "reference omits {phrase}");
    }
    let example = repository_file("assets/skills/habeas/references/ci.md");
    let yaml = example
        .split_once("```yaml\n")
        .unwrap()
        .1
        .split_once("\n```")
        .unwrap()
        .0;
    let example: serde_json::Value = serde_saphyr::from_str(yaml).unwrap();
    let steps = example["steps"].as_array().unwrap();
    assert_eq!(steps[0]["run"], "habeas coverage -- --locked --workspace");
    assert_eq!(steps[1]["run"], "habeas coverage-check");
    assert_eq!(steps[2]["run"], "cargo test --locked --doc --workspace");
    for step in &steps[..2] {
        assert!(step.get("if").is_none(), "use the normal success condition");
        assert!(
            step.get("continue-on-error").is_none(),
            "a failed gate must fail CI"
        );
    }
}

// {test}`requirements.install.symposium`
// {test}`requirements.install.symposium.skill_hook`
#[test]
fn installation_guides_cover_setup_hooks_refresh_and_preserving_local_skills() {
    for path in ["README.md", "md/getting_started.md"] {
        let text = repository_file(path);
        for required in [
            "Cargo project",
            "book.toml",
            "SUMMARY.md",
            "cargo install symposium",
            "cargo agents init",
            "cargo add --dev habeas",
            "cargo agents sync",
            "**Enable**",
            "project tracks requirements with Habeas",
            "load the `habeas` skill",
            "habeas setup",
            "after updating the Habeas dependency",
            ".agents/skills/habeas-*",
            "local edits",
            "different names or elsewhere",
            "remove those old copies",
        ] {
            assert!(text.contains(required), "{path} omits {required}");
        }
        assert!(text.contains("advises the agent"), "{path}: {text}");
        assert!(text.find("Save").unwrap() < text.find("remove those old copies").unwrap());
    }
    let readme = repository_file("README.md");
    assert!(readme.contains("Its session-start hook tells the agent that the project tracks requirements with Habeas and to load the `habeas` skill before starting any work."));
    assert!(readme.contains("The hook also checks the project and advises the agent to run `habeas setup` when configuration is incomplete."));
    let guide = repository_file("md/getting_started.md");
    assert!(guide.contains("When an agent starts a session in this project, Habeas's hook tells it that the project tracks requirements with Habeas and to load the `habeas` skill before starting any work."));
    assert!(guide.contains(
        "if configuration is missing or incompatible, it advises the agent to run `habeas setup`"
    ));
}

/// The lines of one top-level job in a GitHub Actions workflow, from its
/// `  <name>:` line to the next top-level job.
// {cite}`habeas:util`
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
// {test}`requirements.index.chapter.default`
// {test}`requirements.index.detail_pages`
// {test}`requirements.index.navigation`
#[test]
fn mdbook_preprocessor_renders_definitions_between_stdin_and_stdout() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("md")).unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\nname = \"fixture\"\n").unwrap();
    fs::write(root.join("book.toml"), "[book]\nsrc = \"md\"\n").unwrap();
    let content =
        ":::{req} ready\nReady MUST work.\n:::\n\n:::{req} cites\nSee {cite}`rules.ready`.\n:::\n"
            .to_owned();
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
    let badge_rows = chapter
        .content
        .split("<span class=\"habeas-definition-badge\">")
        .skip(1)
        .map(|row| row.split("</span>").next().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(badge_rows.len(), 2, "{}", chapter.content);
    for row in badge_rows {
        assert!(
            row.contains("class=\"habeas-req-badge habeas-badge-kind\""),
            "{row}"
        );
        assert!(
            row.contains("class=\"habeas-req-badge habeas-badge-name\""),
            "{row}"
        );
        assert!(!row.contains("<svg"), "{row}");
        assert_eq!(row.matches("</a>").count(), 2, "{row}");
    }
    assert!(chapter.sub_items.is_empty());
    assert!(
        chapter.content.contains("href=\"#habeas-req-rules.ready\""),
        "{}",
        chapter.content
    );
    assert!(
        chapter
            .content
            .contains("href=\"requirements-index/rules.ready.html\""),
        "{}",
        chapter.content
    );
    assert_eq!(rendered.items.len(), 2);
    let BookItem::Chapter(index) = &rendered.items[1] else {
        panic!("{rendered:?}");
    };
    assert_eq!(index.name, "Requirements index");
    assert_eq!(
        index.path.as_deref(),
        Some(Path::new("requirements-index.md"))
    );
    assert!(index.source_path.is_none());
    assert_eq!(
        index
            .sub_items
            .iter()
            .filter_map(|item| match item {
                BookItem::Chapter(detail) => Some(detail.name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        ["Documented coverage gaps", "rules.ready", "rules.cites"]
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
        toml::Value::Array(vec!["habeas".into()])
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
    let html = &book["output"]["habeas"]["html"];
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
// {test}`requirements.test-coverage.check.guidance`
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
        "cargo build --release --locked -p habeas",
        "cargo habeas coverage -- --locked --workspace",
        "run: cargo habeas coverage-check",
        "run: cargo test --locked --doc --workspace",
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
    let workflow_yaml: serde_json::Value = serde_saphyr::from_str(&workflow).unwrap();
    let gate = workflow_yaml["jobs"]["test-and-build"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|step| step["run"] == "cargo habeas coverage-check")
        .unwrap();
    assert!(
        gate.get("if").is_none(),
        "check only after successful collection"
    );
    assert!(
        gate.get("continue-on-error").is_none(),
        "enforce coverage failures"
    );

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
        book["output"]["habeas"]["html"]["site-url"].as_str(),
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

// {test}`requirements.integration.symposium`
// {test}`requirements.test-coverage.check.guidance`
#[test]
fn published_package_contains_the_bundled_skills_and_ci_example() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("cargo")
        .args(["package", "--list", "--allow-dirty", "--offline"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let packaged = String::from_utf8(output.stdout).unwrap();
    let manifest = repository_toml("Cargo.toml");
    let source = manifest["package"]["metadata"]["symposium"]["skills"][0]["source"]["path"]
        .as_str()
        .unwrap();
    let mut skills = 0;
    for entry in fs::read_dir(root.join(source)).unwrap() {
        let entry = entry.unwrap();
        if entry.path().join("SKILL.md").is_file() {
            let path = format!("{source}/{}/SKILL.md", entry.file_name().to_str().unwrap());
            assert!(
                packaged.lines().any(|file| file == path),
                "package omits {path}"
            );
            skills += 1;
        }
    }
    assert!(skills > 0, "the plugin's source directory contains skills");
    assert!(
        packaged
            .lines()
            .any(|file| file == format!("{source}/habeas/references/ci.md")),
        "the CI reference must ship with the skill that links to it"
    );
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

// {test}`requirements.references.outline_comments`
#[test]
fn workflow_job_marks_belong_to_the_job_that_follows() {
    for path in [
        ".github/workflows/ci.yml",
        ".github/workflows/release-plz.yml",
    ] {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
        let source = std::fs::read_to_string(&file).unwrap();
        let outline = trust_but_verify::outline_engine::parse(&file, &source).unwrap();
        for (index, line) in source.lines().enumerate() {
            let Some(job) = source.lines().nth(index + 1) else {
                continue;
            };
            if line.trim_start().starts_with("# {")
                && line.starts_with("  #")
                && job.starts_with("  ")
                && job.trim_end().ends_with(':')
            {
                let owners = outline.comment_owners_at_line(index + 1);
                let name = job.trim().trim_end_matches(':');
                assert_eq!(owners.len(), 1, "{path}:{}", index + 1);
                assert_eq!(
                    outline.nodes[owners[0]].selector,
                    format!("pair[jobs]/pair[{name}]"),
                    "{path}:{}",
                    index + 1
                );
            }
        }
    }
}

// {test}`requirements.layout.project`
// {test}`requirements.layout.project.workspace`
#[test]
fn workspace_contains_frontend_and_sdk_but_not_the_vendor_fork() {
    let output = std::process::Command::new("cargo")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "metadata",
            "--offline",
            "--locked",
            "--no-deps",
            "--format-version=1",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let members = metadata["workspace_members"].as_array().unwrap();
    let mut names: Vec<_> = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|package| members.contains(&package["id"]))
        .map(|package| package["name"].as_str().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, ["habeas", "trust-but-verify"]);
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(root.join("crates/trust-but-verify/src/lib.rs").is_file());
    let manifest = repository_toml("Cargo.toml");
    assert_eq!(
        manifest["dependencies"]["trust-but-verify"]["path"].as_str(),
        Some("crates/trust-but-verify")
    );
}

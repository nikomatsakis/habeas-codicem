//! Rust item coverage warnings from the recent history of evidence files.

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::json;
use tempfile::TempDir;

/// A Git project whose book defines `rules.check` and `rules.render`, and
/// whose `src/lib.rs` holds three functions with no references.
// {cite}`habeas:util`
fn project() -> TempDir {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for directory in ["md", "src", "evidence"] {
        fs::create_dir_all(root.join(directory)).unwrap();
    }
    fs::write(root.join("Cargo.toml"), "[package]\nname = 'fixture'\n").unwrap();
    fs::write(root.join("book.toml"), "[book]\nsrc = 'md'\n").unwrap();
    fs::write(
        root.join("md/rules.md"),
        ":::{req} check\nChecking MUST work.\n:::\n\n:::{req} render\nRendering MUST work.\n:::\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "fn parse() {}\n\nfn render() {}\n\nfn helper() {}\n\nmod tests {\n    fn fixture() {}\n}\n",
    )
    .unwrap();
    git(root, &["init", "--quiet"]);
    git(root, &["config", "user.email", "test@example.com"]);
    git(root, &["config", "user.name", "Test"]);
    temp
}

// {cite}`habeas:util`
fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {output:?}");
}

/// Writes `name`'s evidence reading `items` of `src/lib.rs`.
// {cite}`habeas:util`
fn record(root: &Path, name: &str, items: &[&str]) {
    let inputs: Vec<_> = items
        .iter()
        .map(|item| habeas::outline_input(root, "src/lib.rs", item).unwrap())
        .collect();
    fs::write(
        root.join(format!("evidence/{name}.json")),
        json!({"requirement": "x", "determination": true, "rationale": "x", "inputs": inputs})
            .to_string(),
    )
    .unwrap();
}

// {cite}`habeas:util`
fn commit(root: &Path, message: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "--quiet", "--allow-empty", "-m", message]);
}

/// The names of the functions warned about.
// {cite}`habeas:util`
fn warned(root: &Path) -> Vec<String> {
    habeas::coverage_warning_lines(root)
        .unwrap()
        .into_iter()
        .filter_map(|line| {
            let name = line.split(": function ").nth(1)?;
            Some(name.split(' ').next()?.to_owned())
        })
        .collect()
}

// {test}`requirements.references.coverage.reauthored`
// {test}`requirements.references.coverage.new_function`
// {test}`requirements.references.coverage.test_module`
// {test}`requirements.references.coverage.history`
// {test}`requirements.references.coverage.report`
#[test]
fn coverage_lasts_for_three_reauthorings_of_the_record_that_read_an_item() {
    let temp = project();
    let root = temp.path();
    record(root, "rules.check", &["fn[parse]"]);
    record(root, "rules.render", &["fn[render]"]);
    commit(root, "first");
    // `helper` was never read, and `fixture` is in `mod tests`.
    assert_eq!(warned(root), ["helper"]);
    for round in 1..=2 {
        // Each re-authoring changes the file, so each commit counts.
        fs::write(
            root.join("evidence/rules.check.json"),
            json!({"requirement": "x", "determination": true, "rationale": format!("round {round}"), "inputs": []})
                .to_string(),
        )
        .unwrap();
        commit(root, &format!("round {round}"));
        assert_eq!(warned(root), ["helper"], "round {round}");
    }
    fs::write(
        root.join("evidence/rules.check.json"),
        json!({"requirement": "x", "determination": true, "rationale": "round 3", "inputs": []})
            .to_string(),
    )
    .unwrap();
    commit(root, "round 3");
    assert_eq!(warned(root), ["parse", "helper"]);
    // A warning does not fail validation.
    let errors = habeas::validate_repository(root).unwrap().errors;
    assert!(
        !errors.iter().any(|error| error.contains("parse")),
        "{errors:#?}"
    );
}

// {test}`requirements.references.coverage.deleted_requirement`
// {test}`requirements.references.coverage.history`
#[test]
fn deleting_the_only_record_that_read_an_item_warns_at_once() {
    let temp = project();
    let root = temp.path();
    record(root, "rules.check", &["fn[parse]", "fn[helper]"]);
    record(root, "rules.render", &["fn[render]"]);
    commit(root, "first");
    assert!(warned(root).is_empty());
    fs::remove_file(root.join("evidence/rules.render.json")).unwrap();
    assert_eq!(warned(root), ["render"]);
}

// {test}`requirements.references.coverage.uncommitted`
// {test}`requirements.references.coverage.history`
#[test]
fn uncommitted_records_count_as_they_are_on_disk() {
    let temp = project();
    let root = temp.path();
    record(root, "rules.check", &["fn[parse]"]);
    commit(root, "first");
    assert_eq!(warned(root), ["render", "helper"]);
    // Re-authored but not committed: the committed version still covers it.
    record(root, "rules.check", &[]);
    assert_eq!(warned(root), ["render", "helper"]);
    // A new record on disk covers what it reads before any commit.
    record(root, "rules.render", &["fn[render]"]);
    assert_eq!(warned(root), ["helper"]);
}

// {test}`requirements.references.coverage.shallow_clone`
// {test}`requirements.references.coverage.history`
#[test]
fn a_shallow_clone_uses_the_history_it_has_and_says_so() {
    let temp = project();
    let root = temp.path();
    record(root, "rules.check", &["fn[parse]"]);
    commit(root, "first");
    record(root, "rules.check", &[]);
    commit(root, "second");
    let clone = tempfile::tempdir().unwrap();
    let url = format!("file://{}", root.display());
    let target = clone.path().join("shallow");
    let output = Command::new("git")
        .args(["clone", "--quiet", "--depth", "1", &url])
        .arg(&target)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    // Only the last commit is available, and it no longer reads `parse`.
    let lines = habeas::coverage_warning_lines(&target).unwrap();
    assert!(
        lines
            .iter()
            .any(|line| line.contains("coverage history is incomplete")),
        "{lines:#?}"
    );
    assert_eq!(warned(&target), ["parse", "render", "helper"]);
    // The full history still covers it.
    assert_eq!(warned(root), ["render", "helper"]);
}

// {test}`requirements.references.coverage.history`
// {test}`requirements.configuration.loading`
#[test]
fn the_window_follows_the_workspace_setting() {
    let temp = project();
    let root = temp.path();
    fs::write(
        root.join("habeas.toml"),
        "version = 1\n\n[coverage]\nreauthorings = 1\n",
    )
    .unwrap();
    record(
        root,
        "rules.check",
        &["fn[parse]", "fn[render]", "fn[helper]"],
    );
    commit(root, "first");
    assert!(warned(root).is_empty());
    record(root, "rules.check", &["fn[render]", "fn[helper]"]);
    commit(root, "second");
    assert_eq!(warned(root), ["parse"]);
    fs::write(
        root.join("habeas.toml"),
        "version = 1\n\n[coverage]\nreauthorings = 0\n",
    )
    .unwrap();
    let error = habeas::coverage_warning_lines(root)
        .unwrap_err()
        .to_string();
    assert!(error.contains("coverage.reauthorings"), "{error}");
}

// {test}`requirements.references.coverage.shallow_clone`
// {test}`requirements.references.coverage.history`
// {test}`requirements.references.coverage.report`
#[test]
fn status_shows_coverage_warnings_and_says_when_history_is_incomplete() {
    let temp = project();
    let root = temp.path();
    record(root, "rules.check", &["fn[parse]"]);
    commit(root, "first");
    let status = habeas::status_markdown(root, false).unwrap();
    assert!(status.contains("## Coverage warnings"), "{status}");
    assert!(
        status.contains("function render has no recent coverage"),
        "{status}"
    );
    assert!(
        !status.contains("function parse has no recent coverage"),
        "{status}"
    );
    assert!(!status.contains("history is incomplete"), "{status}");
    record(root, "rules.check", &[]);
    commit(root, "second");
    let clone = tempfile::tempdir().unwrap();
    let target = clone.path().join("shallow");
    let output = Command::new("git")
        .args([
            "clone",
            "--quiet",
            "--depth",
            "1",
            &format!("file://{}", root.display()),
        ])
        .arg(&target)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let status = habeas::status_markdown(&target, false).unwrap();
    assert!(
        status.contains("Coverage history is incomplete"),
        "{status}"
    );
    assert!(
        status.contains("function parse has no recent coverage"),
        "{status}"
    );
}

// {test}`requirements.references.coverage.history`
#[test]
fn outside_git_only_the_files_on_disk_count_and_history_is_incomplete() {
    let temp = project();
    let root = temp.path();
    fs::remove_dir_all(root.join(".git")).unwrap();
    record(root, "rules.check", &["fn[parse]"]);
    let lines = habeas::coverage_warning_lines(root).unwrap();
    assert!(
        lines
            .iter()
            .any(|line| line.contains("coverage history is incomplete")),
        "{lines:#?}"
    );
    assert_eq!(warned(root), ["render", "helper"]);
}

// {test}`requirements.references.coverage.tests`
// {test}`requirements.references.coverage.test_module`
// {test}`requirements.references.procedure`
#[test]
fn a_test_without_a_test_reference_fails_validation_even_in_mod_tests() {
    let temp = project();
    let root = temp.path();
    fs::write(
        root.join("src/lib.rs"),
        "fn parse() {}\n\nmod tests {\n    fn fixture() {}\n\n    #[test]\n    fn checks() {}\n}\n",
    )
    .unwrap();
    commit(root, "first");
    let errors = habeas::validate_repository(root).unwrap().errors;
    assert!(
        errors.iter().any(|error| error
            == "src/lib.rs:7: test function checks has no {test} reference to a requirement or example"),
        "{errors:#?}"
    );
    // The helper in `mod tests` is neither an error nor a warning.
    assert!(
        !errors.iter().any(|error| error.contains("fixture")),
        "{errors:#?}"
    );
    assert_eq!(warned(root), ["parse"]);
}

// {test}`requirements.references.coverage.report`
#[test]
fn habeas_author_prints_coverage_warnings_on_stderr() {
    let temp = project();
    let root = temp.path();
    // No definitions need assessment, so authoring needs no model.
    fs::write(root.join("md/rules.md"), "# Rules\n").unwrap();
    commit(root, "first");
    let output = Command::new(env!("CARGO_BIN_EXE_habeas"))
        .args(["author", "--all"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Coverage warnings"), "{stderr}");
    assert!(
        stderr.contains("function helper has no recent coverage"),
        "{stderr}"
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("has no recent coverage"));
}

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
    habeas::coverage_warnings(root)
        .unwrap()
        .items
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
    assert!(habeas::coverage_warnings(&target).unwrap().incomplete);
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
    let error = habeas::coverage_warnings(root).unwrap_err().to_string();
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
        status.contains("src/lib.rs:3: function render has no recent coverage"),
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
    assert!(habeas::coverage_warnings(root).unwrap().incomplete);
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
        stderr.contains("src/lib.rs:5: function helper has no recent coverage"),
        "{stderr}"
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("has no recent coverage"));
}

// {test}`requirements.references.coverage.history`
// {test}`requirements.references.coverage.shallow_clone`
#[test]
fn incomplete_history_is_noted_even_with_nothing_to_warn_about() {
    let temp = project();
    let root = temp.path();
    record(
        root,
        "rules.check",
        &["fn[parse]", "fn[render]", "fn[helper]"],
    );
    commit(root, "first");
    commit(root, "second");
    let clone = tempfile::tempdir().unwrap();
    let target = clone.path().join("shallow");
    let output = Command::new("git")
        .args(["clone", "--quiet", "--depth", "1"])
        .arg(format!("file://{}", root.display()))
        .arg(&target)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(warned(&target).is_empty());
    assert_eq!(
        habeas::coverage_warnings(&target).unwrap(),
        habeas::CoverageWarnings {
            items: Vec::new(),
            incomplete: true
        }
    );
    let status = habeas::status_markdown(&target, false).unwrap();
    assert!(
        status.contains("Coverage history is incomplete"),
        "{status}"
    );
    assert!(warned(root).is_empty());
    assert_eq!(
        habeas::coverage_warnings(root).unwrap(),
        habeas::CoverageWarnings::default()
    );
}

// {test}`requirements.references.coverage.report`
// {test}`requirements.references.coverage.history`
#[test]
fn validation_does_not_read_git_history() {
    let temp = project();
    let root = temp.path();
    record(root, "rules.check", &["fn[parse]"]);
    commit(root, "first");
    let before = habeas::validate_repository(root).unwrap().errors;
    // Without the object database, no committed version can be read.
    for entry in fs::read_dir(root.join(".git/objects")).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().unwrap().len() == 2 {
            fs::remove_dir_all(path).unwrap();
        }
    }
    assert_eq!(habeas::validate_repository(root).unwrap().errors, before);
    assert!(habeas::coverage_warnings(root).unwrap().incomplete);
}

/// The output of `git` in `root`.
// {cite}`habeas:util`
fn git_output(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

/// A shallow clone of `root` holding its last `depth` commits.
// {cite}`habeas:util`
fn shallow_clone(root: &Path, depth: usize) -> (TempDir, std::path::PathBuf) {
    let clone = tempfile::tempdir().unwrap();
    let target = clone.path().join("shallow");
    let output = Command::new("git")
        .args(["clone", "--quiet", "--depth", &depth.to_string()])
        .arg(format!("file://{}", root.display()))
        .arg(&target)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    (clone, target)
}

// {test}`requirements.references.coverage.history`
#[test]
fn a_merge_counts_only_when_it_differs_from_every_parent() {
    let temp = project();
    let root = temp.path();
    fs::write(
        root.join("habeas.toml"),
        "version = 1\n\n[coverage]\nreauthorings = 2\n",
    )
    .unwrap();
    record(
        root,
        "rules.check",
        &["fn[parse]", "fn[render]", "fn[helper]"],
    );
    commit(root, "first");
    git(root, &["switch", "--quiet", "-c", "side"]);
    record(root, "rules.check", &["fn[render]", "fn[helper]"]);
    commit(root, "side");
    git(root, &["switch", "--quiet", "-"]);
    git(
        root,
        &["merge", "--quiet", "--no-ff", "-m", "merge", "side"],
    );
    // The merge takes the side branch's version unchanged, so the window of
    // two holds that version and the first, which still reads `parse`.
    assert!(warned(root).is_empty(), "{:?}", warned(root));
    assert!(!habeas::coverage_warnings(root).unwrap().incomplete);
}

// {test}`requirements.references.coverage.history`
#[test]
fn an_unreadable_historical_version_makes_history_incomplete() {
    let temp = project();
    let root = temp.path();
    record(root, "rules.check", &["fn[parse]"]);
    commit(root, "first");
    record(root, "rules.check", &[]);
    commit(root, "second");
    assert_eq!(warned(root), ["render", "helper"]);
    assert!(!habeas::coverage_warnings(root).unwrap().incomplete);
    let blob = git_output(root, &["rev-parse", "HEAD~1:evidence/rules.check.json"]);
    fs::remove_file(root.join(format!(".git/objects/{}/{}", &blob[..2], &blob[2..]))).unwrap();
    // The version that read `parse` cannot be read, and that is reported.
    let warnings = habeas::coverage_warnings(root).unwrap();
    assert!(warnings.incomplete, "{warnings:?}");
    assert_eq!(warned(root), ["parse", "render", "helper"]);
}

// {test}`requirements.references.coverage.history`
// {test}`requirements.references.coverage.shallow_clone`
#[test]
fn a_shallow_clone_holding_every_window_is_complete() {
    let temp = project();
    let root = temp.path();
    commit(root, "first");
    record(
        root,
        "rules.check",
        &["fn[parse]", "fn[render]", "fn[helper]"],
    );
    commit(root, "add the record");
    commit(root, "unrelated");
    // The clone reaches the commit before the record was added, so the
    // record's whole history is there.
    let (_clone, target) = shallow_clone(root, 3);
    assert_eq!(
        habeas::coverage_warnings(&target).unwrap(),
        habeas::CoverageWarnings::default()
    );
    let status = habeas::status_markdown(&target, false).unwrap();
    assert!(!status.contains("history is incomplete"), "{status}");
    // A clone that stops at the commit adding it cannot tell.
    let (_clone, target) = shallow_clone(root, 2);
    assert!(habeas::coverage_warnings(&target).unwrap().incomplete);
}

// {test}`requirements.references.coverage.report`
// {test}`requirements.references.coverage.test_module`
#[test]
fn items_in_out_of_line_tests_modules_are_not_warned_about() {
    let temp = project();
    let root = temp.path();
    fs::create_dir_all(root.join("src/foo")).unwrap();
    fs::create_dir_all(root.join("src/bar/tests")).unwrap();
    fs::write(root.join("src/foo.rs"), "mod tests;\n\nfn in_foo() {}\n").unwrap();
    fs::write(root.join("src/foo/tests.rs"), "fn foo_fixture() {}\n").unwrap();
    fs::write(root.join("src/bar.rs"), "mod tests;\n").unwrap();
    fs::write(root.join("src/bar/tests/mod.rs"), "fn bar_fixture() {}\n").unwrap();
    commit(root, "first");
    let mut names = warned(root);
    names.sort();
    assert_eq!(names, ["helper", "in_foo", "parse", "render"]);
}

// {test}`requirements.configuration.workspace_structure`
// {test}`requirements.configuration.loading`
// {test}`requirements.references.coverage.history`
#[test]
fn an_invalid_coverage_table_is_a_validation_finding_but_authoring_proceeds() {
    let temp = project();
    let root = temp.path();
    fs::write(root.join("md/rules.md"), "# Rules\n").unwrap();
    commit(root, "first");
    for (table, setting) in [
        ("[coverage]\nreauthorings = 0\n", "coverage.reauthorings"),
        ("[coverage]\nreauthorings = 'x'\n", "coverage.reauthorings"),
        ("[coverage]\nwindow = 2\n", "coverage.window"),
    ] {
        fs::write(root.join("habeas.toml"), format!("version = 1\n\n{table}")).unwrap();
        let errors = habeas::validate_repository(root).unwrap().errors;
        assert!(
            errors
                .iter()
                .any(|error| error.contains("habeas.toml") && error.contains(setting)),
            "{table}: {errors:#?}"
        );
        // Authoring does not use `[coverage]`.
        let output = Command::new(env!("CARGO_BIN_EXE_habeas"))
            .args(["author", "--all"])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{table}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("cannot compute coverage warnings"),
            "{stderr}"
        );
    }
}

// {test}`requirements.references.coverage.examples`
#[test]
fn an_untracked_example_is_not_a_target() {
    let temp = project();
    let root = temp.path();
    fs::create_dir_all(root.join("examples")).unwrap();
    fs::write(
        root.join("md/rules.md"),
        ":::{req} check\nChecking MUST work.\n:::\n\n:::{example} demo\nA demo. {cite}`rules.check`\n:::\n",
    )
    .unwrap();
    fs::write(root.join("examples/tracked.rs"), "fn main() {}\n").unwrap();
    commit(root, "first");
    // Untracked, it is not discovered, so its citation is not found either.
    fs::write(
        root.join("examples/untracked.rs"),
        "// {cite}`rules.demo`\nfn main() {}\n",
    )
    .unwrap();
    let errors = habeas::validate_repository(root).unwrap().errors;
    let examples: Vec<_> = errors
        .iter()
        .filter(|error| error.contains("Cargo example"))
        .collect();
    assert_eq!(
        examples,
        ["examples/tracked.rs: Cargo example does not {cite} a named example of the book"]
    );
}

// {test}`requirements.references.coverage.history`
// {test}`requirements.references.coverage.shallow_clone`
#[test]
fn a_linked_worktree_of_a_shallow_clone_knows_where_history_ends() {
    let temp = project();
    let root = temp.path();
    fs::write(
        root.join("habeas.toml"),
        "version = 1\n\n[coverage]\nreauthorings = 1\n",
    )
    .unwrap();
    record(root, "rules.check", &["fn[parse]"]);
    commit(root, "first");
    record(root, "rules.check", &["fn[render]"]);
    commit(root, "second");
    // The one-commit clone holds the record's whole window of one version;
    // so does a linked worktree of it, whose `shallow` file is in the common
    // directory rather than its own.
    let (clone, target) = shallow_clone(root, 1);
    assert!(!habeas::coverage_warnings(&target).unwrap().incomplete);
    let worktree = clone.path().join("linked");
    git(
        &target,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            worktree.to_str().unwrap(),
        ],
    );
    let warnings = habeas::coverage_warnings(&worktree).unwrap();
    assert!(!warnings.incomplete, "{warnings:?}");
    assert_eq!(warned(&worktree), ["parse", "helper"]);
}

// {test}`requirements.references.coverage.history`
// {test}`requirements.references.coverage.uncommitted`
#[test]
fn an_uncommitted_record_does_not_make_a_full_shallow_history_incomplete() {
    let temp = project();
    let root = temp.path();
    fs::write(
        root.join("habeas.toml"),
        "version = 1\n\n[coverage]\nreauthorings = 1\n",
    )
    .unwrap();
    record(root, "rules.check", &["fn[parse]"]);
    commit(root, "first");
    record(root, "rules.check", &["fn[render]"]);
    commit(root, "second");
    // The one-commit clone holds the committed record's whole window.
    let (_clone, target) = shallow_clone(root, 1);
    assert!(!habeas::coverage_warnings(&target).unwrap().incomplete);
    // A new record that was never committed has no history to miss.
    record(&target, "rules.render", &["fn[helper]"]);
    let warnings = habeas::coverage_warnings(&target).unwrap();
    assert!(!warnings.incomplete, "{warnings:?}");
    assert_eq!(warned(&target), ["parse"]);
}

// {test}`requirements.references.coverage.history`
#[cfg(unix)]
#[test]
fn an_unlistable_evidence_directory_makes_history_incomplete() {
    use std::os::unix::fs::PermissionsExt;
    let temp = project();
    let root = temp.path();
    record(
        root,
        "rules.check",
        &["fn[parse]", "fn[render]", "fn[helper]"],
    );
    commit(root, "first");
    assert!(!habeas::coverage_warnings(root).unwrap().incomplete);
    let hidden = root.join("evidence/hidden");
    fs::create_dir_all(&hidden).unwrap();
    fs::set_permissions(&hidden, fs::Permissions::from_mode(0o000)).unwrap();
    // Permissions do not stop a privileged user, so there is nothing to test.
    let listable = fs::read_dir(&hidden).is_ok();
    let warnings = (!listable).then(|| habeas::coverage_warnings(root));
    fs::set_permissions(&hidden, fs::Permissions::from_mode(0o755)).unwrap();
    if let Some(warnings) = warnings {
        let warnings = warnings.unwrap();
        assert!(warnings.incomplete, "{warnings:?}");
    }
}

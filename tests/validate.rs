//! `habeas::validate()` reads the process's current directory, so each case
//! runs it in a child copy of this test binary started in a fixture project.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

const CHILD: &str = "HABEAS_VALIDATE_CHILD";

/// Runs `habeas::validate()` when started as the child process; otherwise
/// does nothing, so the ordinary test run ignores it.
// {test}`requirements.integration.validate`
#[test]
fn validate_child() {
    if std::env::var_os(CHILD).is_some() {
        habeas::validate();
    }
}

// {cite}`habeas:util`
/// Runs [`validate_child`] in `cwd`, with Habeas state directed to `state`.
fn validate_in(cwd: &Path, state: &Path) -> Output {
    Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "validate_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .current_dir(cwd)
        .env(CHILD, "1")
        .env("XDG_STATE_HOME", state)
        .env("XDG_CONFIG_HOME", state)
        .env("HOME", state)
        .output()
        .unwrap()
}

// {test}`requirements.integration.validate`
#[test]
fn validate_fails_the_test_with_every_error_and_authoring_instructions() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("project");
    let state = temp.path().join("state");
    fs::create_dir_all(root.join("md")).unwrap();
    fs::create_dir_all(root.join("src/nested")).unwrap();
    fs::create_dir_all(root.join("evidence")).unwrap();
    fs::create_dir_all(&state).unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\nname = \"fixture\"\n").unwrap();
    fs::write(root.join("book.toml"), "[book]\nsrc = \"md\"\n").unwrap();
    fs::write(
        root.join("md/rules.md"),
        ":::{req} ready\nReady.\n:::\n\n:::{req} broken\nBroken.\n:::\n",
    )
    .unwrap();
    fs::write(root.join("evidence/rules.broken.json"), "{broken").unwrap();

    // Validation starts from a nested directory and finds the project.
    let output = validate_in(&root.join("src/nested"), &state);
    assert!(!output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let message = format!("{stdout}{stderr}");
    assert!(message.contains("Habeas validation failed"), "{message}");
    assert!(message.contains("rules.ready"), "{message}");
    assert!(message.contains("evidence/rules.broken.json"), "{message}");
    assert!(
        message.contains("Run `habeas author --all`")
            && message.contains("address any reported gaps and rerun the tests"),
        "{message}"
    );
    // Validation never authors: no evidence or usage history is written.
    assert_eq!(fs::read_dir(root.join("evidence")).unwrap().count(), 1);
    assert_eq!(fs::read_dir(&state).unwrap().count(), 0);

    // Repeating validation gives the same result.
    let again = validate_in(&root, &state);
    assert_eq!(
        format!(
            "{}{}",
            String::from_utf8_lossy(&again.stdout),
            String::from_utf8_lossy(&again.stderr)
        )
        .lines()
        .filter(|line| line.starts_with("- "))
        .collect::<Vec<_>>(),
        message
            .lines()
            .filter(|line| line.starts_with("- "))
            .collect::<Vec<_>>()
    );

    // Valid evidence passes.
    fs::write(root.join("md/rules.md"), ":::{req} ready\nReady.\n:::\n").unwrap();
    fs::remove_file(root.join("evidence/rules.broken.json")).unwrap();
    fs::write(
        root.join("evidence/rules.ready.json"),
        serde_json::json!({
            "requirement": "Ready.",
            "determination": true,
            "rationale": "Reviewed.",
            "references": habeas::reference_set_digest_for(&root, "rules.ready").unwrap(),
            "inputs": []
        })
        .to_string(),
    )
    .unwrap();
    let output = validate_in(&root, &state);
    assert!(output.status.success(), "{output:?}");
}

// {test}`requirements.integration.validate`
#[test]
fn validate_fails_outside_a_cargo_and_mdbook_project() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
    let output = validate_in(temp.path(), temp.path());
    assert!(!output.status.success(), "{output:?}");
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        message.contains("cannot find Cargo and mdBook project root"),
        "{message}"
    );
}

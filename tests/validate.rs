//! `habeas::validate()` reads the process's current directory, so each case
//! runs it in a child copy of this test binary started in a fixture project.

mod common;

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
        .env("HABEAS_RELEASE_VALIDATOR", env!("CARGO_BIN_EXE_habeas"))
        .env(
            "HABEAS_VALIDATOR_SOURCE",
            concat!(env!("CARGO_MANIFEST_DIR"), "@", env!("CARGO_PKG_VERSION")),
        )
        .env("XDG_STATE_HOME", state)
        .env("XDG_CONFIG_HOME", state)
        .env("HOME", state)
        .output()
        .unwrap()
}

// {test}`requirements.integration.validate`
// {test}`requirements.integration.validate.optimized`
// {test}`requirements.test-coverage.check.guidance`
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

    common::coverage_setup(&root);

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
    assert!(
        !root.join("target/habeas/coverage").exists(),
        "static validation needs no execution data"
    );
}

// {test}`requirements.integration.validate.installed`
// {test}`requirements.integration.validate.optimized`
#[cfg(unix)]
#[test]
fn standalone_test_builds_the_dependency_validator_for_the_consumer() {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\nname='consumer'\n").unwrap();
    fs::write(root.join("book.toml"), "[book]\n").unwrap();
    let cargo = root.join("cargo");
    fs::write(&cargo, "#!/bin/sh\npwd > cwd\nprintf '%s\\n' \"$@\" > args\necho 'consumer evidence is stale' >&2\nexit 1\n").unwrap();
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "validate_child", "--nocapture"])
        .current_dir(&root)
        .env(CHILD, "1")
        .env("CARGO", cargo)
        .env("CARGO_TARGET_DIR", root.join("build cache"))
        .env("HABEAS_RELEASE_VALIDATOR", root.join("wrong-version"))
        .env("HABEAS_VALIDATOR_SOURCE", "different dependency")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("consumer evidence is stale"));
    assert_eq!(
        fs::read_to_string(root.join("cwd")).unwrap().trim(),
        root.to_str().unwrap()
    );
    let args = fs::read_to_string(root.join("args")).unwrap();
    let args: Vec<_> = args.lines().collect();
    assert!(args.contains(&"--release"));
    assert!(args.windows(2).any(|pair| pair
        == [
            "--manifest-path",
            concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")
        ]));
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--target-dir", root.join("build cache").to_str().unwrap()])
    );
    assert!(args.ends_with(&["--", "validate"]));
}

// {test}`requirements.integration.validate.optimized`
#[cfg(unix)]
#[test]
fn matching_collector_validates_the_consumer_without_cargo() {
    use std::os::unix::fs::PermissionsExt;
    let temp = TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let validator = root.join("release validator");
    fs::write(&validator, "#!/bin/sh\npwd > cwd\nprintf '%s\\n' \"$@\" > args\nif test -f stale; then echo 'consumer evidence is stale' >&2; exit 1; fi\n").unwrap();
    fs::set_permissions(&validator, fs::Permissions::from_mode(0o755)).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap());
    child
        .args(["--exact", "validate_child", "--nocapture"])
        .current_dir(&root)
        .env(CHILD, "1")
        .env("HABEAS_RELEASE_VALIDATOR", validator)
        .env(
            "HABEAS_VALIDATOR_SOURCE",
            concat!(env!("CARGO_MANIFEST_DIR"), "@", env!("CARGO_PKG_VERSION")),
        )
        .env("CARGO", root.join("must not invoke cargo"));
    assert!(child.output().unwrap().status.success());
    assert_eq!(fs::read_to_string(root.join("args")).unwrap(), "validate\n");
    assert_eq!(
        fs::read_to_string(root.join("cwd")).unwrap().trim(),
        root.to_str().unwrap()
    );
    fs::write(root.join("stale"), "").unwrap();
    let failure = child.output().unwrap();
    assert!(!failure.status.success());
    assert!(String::from_utf8_lossy(&failure.stderr).contains("consumer evidence is stale"));
}

// {test}`requirements.integration.validate.installed`
#[cfg(unix)]
#[test]
fn path_validator_requires_crates_io_and_an_exact_successful_version_probe() {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::write(root.join("Cargo.toml"), "[package]\nname='consumer'\n").unwrap();
    fs::write(root.join("book.toml"), "[book]\n").unwrap();
    let cargo = root.join("cargo");
    fs::write(
        &cargo,
        r#"#!/bin/sh
if test "$1" = pkgid; then
    test "$2" = --frozen && test "$3" = "habeas@$DEP_VERSION" || exit 2
    printf '%s\n' "$PACKAGE_ID"
    exit "$PKGID_STATUS"
fi
test "$1" = run || exit 2
echo build >> calls
"#,
    )
    .unwrap();
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
    let cli = root.join("habeas");
    fs::write(
        &cli,
        r#"#!/bin/sh
echo "$1" >> calls
case "$1" in
    --version) echo "$REPORTED_VERSION"; exit "$PROBE_STATUS" ;;
    validate) echo 'cached CLI validation failed' >&2; exit 1 ;;
    *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();

    let version = env!("CARGO_PKG_VERSION");
    let matching = format!("habeas {version}");
    let registry =
        format!("registry+https://github.com/rust-lang/crates.io-index#habeas@{version}");
    let mut child = Command::new(std::env::current_exe().unwrap());
    child
        .args(["--exact", "validate_child", "--nocapture"])
        .current_dir(root)
        .env(CHILD, "1")
        .env("CARGO", cargo)
        .env("DEP_VERSION", version)
        .env("PATH", root)
        .env("CARGO_TARGET_DIR", root.join("target"))
        .env_remove("HABEAS_RELEASE_VALIDATOR");
    let calls = root.join("calls");
    for (source, reported, pkgid_status, probe_status, expected) in [
        (
            registry.as_str(),
            matching.as_str(),
            "0",
            "0",
            "--version\nvalidate\n",
        ),
        (&registry, "habeas 0.0.0", "0", "0", "--version\nbuild\n"),
        (&registry, &matching, "0", "1", "--version\nbuild\n"),
        (&registry, &matching, "1", "0", "build\n"),
        (
            "path+file:///checkout#habeas",
            &matching,
            "0",
            "0",
            "build\n",
        ),
        (
            "git+https://example.org/habeas#habeas",
            &matching,
            "0",
            "0",
            "build\n",
        ),
    ] {
        fs::write(&calls, "").unwrap();
        let output = child
            .env("PACKAGE_ID", source)
            .env("REPORTED_VERSION", reported)
            .env("PKGID_STATUS", pkgid_status)
            .env("PROBE_STATUS", probe_status)
            .output()
            .unwrap();
        assert_eq!(fs::read_to_string(&calls).unwrap(), expected);
        if expected.ends_with("validate\n") {
            assert!(!output.status.success());
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("cached CLI validation failed")
            );
        } else {
            assert!(output.status.success(), "{output:?}");
        }
    }

    fs::remove_file(cli).unwrap();
    fs::write(&calls, "").unwrap();
    let missing = child.env("PACKAGE_ID", &registry).output().unwrap();
    assert!(missing.status.success(), "{missing:?}");
    assert_eq!(fs::read_to_string(calls).unwrap(), "build\n");
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

//! Annotation checks retain tests and examples without indexing ordinary items.
mod common;

use std::{fs, process::Command};
use tempfile::TempDir;

fn project() -> TempDir {
    let temp = TempDir::new().unwrap();
    for directory in ["md", "src", "evidence"] {
        fs::create_dir_all(temp.path().join(directory)).unwrap();
    }
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname = 'fixture'\n",
    )
    .unwrap();
    fs::write(temp.path().join("book.toml"), "[book]\nsrc = 'md'\n").unwrap();
    fs::write(
        temp.path().join("md/rules.md"),
        ":::{req} ready\nReady holds.\n:::\n",
    )
    .unwrap();
    common::coverage_setup(temp.path());
    temp
}

// {test}`requirements.references.rust_item_coverage`
// {test}`requirements.cli.status`
// {test}`requirements.references.coverage.tests`
// {test}`requirements.references.procedure`
#[test]
fn ordinary_functions_types_and_test_helpers_require_no_book_connection() {
    let temp = project();
    fs::write(
        temp.path().join("src/lib.rs"),
        concat!(
            "struct Thing; enum Choice { One } union Bits { value: u32 }\n",
            "trait Interface { fn call(&self); } impl Thing { fn helper(&self) {} }\n",
            "fn ordinary() {} mod tests { fn fixture() {} }\n",
            "// {test}`rules.ready`\n#[test]\nfn test_ready() {}\n",
        ),
    )
    .unwrap();
    let errors = habeas::validate_repository(temp.path()).unwrap().errors;
    assert!(
        !errors
            .iter()
            .any(|error| error.contains("coverage") || error.contains("no {test}")),
        "{errors:?}"
    );
    for all in [false, true] {
        let status = habeas::status_markdown(temp.path(), all).unwrap();
        assert!(!status.contains("Coverage warnings"), "{status}");
        assert!(!status.contains("has no recent coverage"), "{status}");
        assert!(!status.contains("nodes have coverage warnings"), "{status}");
    }
}

// {test}`requirements.references.coverage.tests`
// {test}`requirements.references.rust_item_coverage.parsing`
#[test]
fn unmarked_tests_and_parse_failures_remain_validation_errors() {
    let temp = project();
    fs::write(
        temp.path().join("src/lib.rs"),
        "mod tests { #[test] fn bare() {} fn helper() {} }\n",
    )
    .unwrap();
    fs::write(temp.path().join("src/broken.rs"), "fn unfinished( {\n").unwrap();
    let errors = habeas::validate_repository(temp.path()).unwrap().errors;
    assert!(
        errors
            .iter()
            .any(|error| error.contains("test function bare has no {test}")),
        "{errors:?}"
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("src/broken.rs:1: cannot parse Rust source")),
        "{errors:?}"
    );
    assert!(
        !errors.iter().any(|error| error.contains("function helper")),
        "{errors:?}"
    );
}

// {test}`requirements.cli.status`
// {test}`requirements.cli.clap`
#[test]
fn obsolete_status_coverage_flag_is_rejected_with_usage() {
    let temp = project();
    let output = Command::new(env!("CARGO_BIN_EXE_habeas"))
        .current_dir(temp.path())
        .args(["status", "--coverage"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("unexpected argument '--coverage'"),
        "{stderr}"
    );
    assert!(stderr.contains("Usage:"), "{stderr}");
}

// {test}`requirements.configuration.workspace_structure`
// {test}`requirements.configuration.loading`
#[test]
fn obsolete_coverage_history_table_is_rejected_by_all_workspace_loaders() {
    let temp = project();
    fs::write(
        temp.path().join("habeas.toml"),
        "version = 1\n[coverage]\nreauthorings = 3\n",
    )
    .unwrap();
    let path = temp.path().join("habeas.toml");
    for result in [
        habeas::config::load_workspace_config(temp.path()).map(|_| ()),
        habeas::config::load_workspace_evidence(temp.path()).map(|_| ()),
        habeas::config::WorkspaceConfig::parse(&fs::read_to_string(&path).unwrap(), &path)
            .map(|_| ()),
    ] {
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("coverage: unknown workspace configuration field"),
            "{error}"
        );
    }
}

// {test}`requirements.references.rust_item_coverage`
// {test}`requirements.references.coverage.examples`
#[test]
fn untracked_examples_do_not_become_cargo_targets() {
    let temp = project();
    fs::create_dir_all(temp.path().join("examples")).unwrap();
    fs::write(temp.path().join("examples/loose.rs"), "fn main() {}\n").unwrap();
    // Outside Git all files are discoverable, so this is an actual example target.
    let errors = habeas::validate_repository(temp.path()).unwrap().errors;
    assert!(
        errors
            .iter()
            .any(|error| error.contains("examples/loose.rs: Cargo example does not {cite}")),
        "{errors:?}"
    );
    for args in [
        vec!["init", "--quiet"],
        vec!["add", "Cargo.toml", "book.toml", "md", "src", ".config"],
    ] {
        let output = Command::new("git")
            .current_dir(temp.path())
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
    let errors = habeas::validate_repository(temp.path()).unwrap().errors;
    assert!(
        !errors
            .iter()
            .any(|error| error.contains("examples/loose.rs")),
        "{errors:?}"
    );
}

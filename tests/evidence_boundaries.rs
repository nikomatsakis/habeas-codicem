use std::fs;
use std::path::Path;

use habeas::validate_repository;
use serde_json::{Value, json};
use tempfile::TempDir;

// {test}`requirements.ci.inputs`
// {test}`requirements.ci.input_containment`
// {test}`requirements.ci.orphans`
fn fixture() -> TempDir {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    for directory in ["md", "src", "evidence"] {
        fs::create_dir_all(root.join(directory)).unwrap();
    }
    fs::write(root.join("Cargo.toml"), "[package]\nname = 'fixture'\n").unwrap();
    fs::write(root.join("book.toml"), "[book]\nsrc = 'md'\n").unwrap();
    fs::write(
        root.join("md/rules.md"),
        ":::{req} ready\nRead the data.\n:::\n",
    )
    .unwrap();
    fs::write(root.join("data.txt"), "first\nsecond\n").unwrap();
    write_inputs(&root, vec![]);
    temp
}

// {test}`requirements.ci.schema`
fn write_inputs(root: &Path, inputs: Vec<Value>) {
    fs::write(
        root.join("evidence/rules.ready.json"),
        json!({
            "requirement": "Read the data.",
            "determination": true,
            "rationale": "The fixture records the data being checked.",
            "inputs": inputs,
        })
        .to_string(),
    )
    .unwrap();
}

// {test}`requirements.ci.inputs`
// {test}`requirements.ci.input_containment`
#[test]
fn file_ranges_report_order_eof_length_and_missing_file_errors() {
    let temp = fixture();
    let root = temp.path().join("project");
    write_inputs(
        &root,
        vec![json!({"file":"data.txt", "line_start":2, "line_end":2, "text":["second"]})],
    );
    assert!(validate_repository(&root).unwrap().errors.is_empty());

    for (input, expected) in [
        (
            json!({"file":"data.txt", "line_start":2, "line_end":1, "text":["second"]}),
            "line_end must be at least line_start",
        ),
        (
            json!({"file":"data.txt", "line_start":1, "line_end":3, "text":["first", "second", "third"]}),
            "exceeds the file's 2 lines",
        ),
        (
            json!({"file":"data.txt", "line_start":1, "line_end":2, "text":["first"]}),
            "text has 1 lines, but data.txt:1-2 has 2",
        ),
        (
            json!({"file":"missing.txt", "line_start":1, "line_end":1, "text":["first"]}),
            "cannot open missing.txt",
        ),
    ] {
        write_inputs(&root, vec![input]);
        let errors = validate_repository(&root).unwrap().errors.join("\n");
        assert!(errors.contains(expected), "missing {expected:?}: {errors}");
    }
}

// {test}`requirements.ci.input_containment`
// {test}`requirements.ci.inputs`
// {test}`requirements.ci.rust_item_inputs`
#[cfg(unix)]
#[test]
fn symlinks_may_resolve_inside_but_not_to_a_sibling_with_the_same_prefix() {
    use std::os::unix::fs::symlink;

    let temp = fixture();
    let root = temp.path().join("project");
    symlink(root.join("data.txt"), root.join("inside.txt")).unwrap();
    write_inputs(
        &root,
        vec![
            json!({"file":"inside.txt", "line_start":1, "line_end":2, "text":["first", "second"]}),
        ],
    );
    assert!(validate_repository(&root).unwrap().errors.is_empty());

    let sibling = temp.path().join("project-sibling");
    fs::create_dir(&sibling).unwrap();
    fs::write(sibling.join("outside.rs"), "fn outside() {}\n").unwrap();
    symlink(sibling.join("outside.rs"), root.join("escape.rs")).unwrap();
    for input in [
        json!({"file":"escape.rs", "line_start":1, "line_end":1, "text":["fn outside() {}"]}),
        json!({"rust_file":"escape.rs", "rust_item":"fn[outside]", "text":["fn outside() {}"]}),
    ] {
        write_inputs(&root, vec![input]);
        let errors = validate_repository(&root).unwrap().errors.join("\n");
        assert!(
            errors.contains("escape.rs resolves outside the repository"),
            "{errors}"
        );
    }
}

// {test}`requirements.ci.orphans`
#[test]
fn nested_evidence_and_non_source_rust_sidecars_are_reported() {
    let temp = fixture();
    let root = temp.path().join("project");
    assert!(validate_repository(&root).unwrap().errors.is_empty());
    let record = fs::read(root.join("evidence/rules.ready.json")).unwrap();
    for directory in ["evidence/nested", "evidence/rust"] {
        fs::create_dir_all(root.join(directory)).unwrap();
    }
    fs::write(root.join("evidence/nested/rules.ready.json"), &record).unwrap();
    fs::write(root.join("evidence/rust/other.json"), &record).unwrap();
    let errors = validate_repository(&root).unwrap().errors.join("\n");
    assert!(
        errors.contains("evidence/nested/rules.ready.json: no matching requirement or example"),
        "{errors}"
    );
    assert!(
        errors.contains("evidence/rust/other.json: orphaned Rust attestation file"),
        "{errors}"
    );
}

// {test}`requirements.ci.orphans`
// {test}`requirements.references.rust_function_attestations`
#[test]
fn deleted_function_entries_are_reported_in_an_existing_sidecar() {
    let temp = fixture();
    let root = temp.path().join("project");
    let source = "// IMPL`rules.ready`\npub fn present() {}\n".replace("IMPL", "{impl}");
    fs::write(root.join("src/lib.rs"), &source).unwrap();
    let function_input = json!({
        "rust_file": "src/lib.rs", "rust_item": "fn[present]",
        "text": source.lines().collect::<Vec<_>>(),
    });
    write_inputs(&root, vec![function_input.clone()]);
    let entry = json!({
        "rust_item":"fn[present]",
        "citations":[{"role":"impl", "name":"rules.ready"}],
        "determination":true,
        "rationale":"The fixture's only function cites its only requirement.",
        "inputs":[function_input, {
            "requirements":"rules.ready", "matches":["rules.ready"],
            "text":[":::{req} ready", "Read the data.", ":::"],
        }],
    });
    let path = root.join("evidence/rust/src/lib.rs.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut sidecar = json!({"rust_file":"src/lib.rs", "functions":[entry.clone()]});
    fs::write(&path, sidecar.to_string()).unwrap();
    assert!(validate_repository(&root).unwrap().errors.is_empty());

    let mut removed = entry;
    removed["rust_item"] = json!("fn[removed]");
    sidecar["functions"].as_array_mut().unwrap().push(removed);
    fs::write(&path, sidecar.to_string()).unwrap();
    let errors = validate_repository(&root).unwrap().errors.join("\n");
    assert!(
        errors.contains("orphaned Rust function attestation") && errors.contains("fn[removed]"),
        "{errors}"
    );
}

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
fn nested_evidence_files_are_reported() {
    let temp = fixture();
    let root = temp.path().join("project");
    assert!(validate_repository(&root).unwrap().errors.is_empty());
    let record = fs::read(root.join("evidence/rules.ready.json")).unwrap();
    for directory in ["evidence/nested", "evidence/rust/src"] {
        fs::create_dir_all(root.join(directory)).unwrap();
    }
    fs::write(root.join("evidence/nested/rules.ready.json"), &record).unwrap();
    fs::write(root.join("evidence/rust/src/lib.rs.json"), &record).unwrap();
    let errors = validate_repository(&root).unwrap().errors.join("\n");
    for label in [
        "evidence/nested/rules.ready.json",
        "evidence/rust/src/lib.rs.json",
    ] {
        assert!(
            errors.contains(&format!("{label}: no matching requirement or example")),
            "{errors}"
        );
    }
}

// {test}`requirements.configuration.evidence_paths`
// {test}`requirements.ci.input_containment`
// {test}`requirements.ci.inputs`
// {test}`requirements.ci.rust_item_inputs`
#[test]
fn configured_external_inputs_validate_and_remain_subject_to_freshness_and_disallow() {
    let temp = fixture();
    let root = temp.path().join("project");
    let external = temp.path().join("generated");
    fs::create_dir(&external).unwrap();
    let source = external.join("generated.rs");
    fs::write(&source, "pub fn generated() {}\n").unwrap();
    let external_toml = toml::Value::String(external.display().to_string());
    let config = format!("version = 1\n[evidence]\nallow = [{external_toml}]\n");
    for input in [
        json!({"file":source, "line_start":1, "line_end":1, "text":["pub fn generated() {}"]}),
        json!({"rust_file":source, "rust_item":"fn[generated]", "text":["pub fn generated() {}"]}),
    ] {
        write_inputs(&root, vec![input]);
        fs::write(root.join("habeas.toml"), "version = 1\n").unwrap();
        assert!(
            validate_repository(&root)
                .unwrap()
                .errors
                .join("\n")
                .contains("outside the repository")
        );
        fs::write(root.join("habeas.toml"), &config).unwrap();
        let report = validate_repository(&root).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        fs::write(&source, "pub fn generated() { /* changed */ }\n").unwrap();
        assert!(
            validate_repository(&root)
                .unwrap()
                .errors
                .join("\n")
                .contains("input text changed")
        );
        fs::write(&source, "pub fn generated() {}\n").unwrap();
        fs::write(
            root.join("habeas.toml"),
            format!("{config}disallow = [{external_toml}]\n"),
        )
        .unwrap();
        assert!(
            validate_repository(&root)
                .unwrap()
                .errors
                .join("\n")
                .contains("disallowed")
        );
    }
}

// {test}`requirements.configuration.evidence_paths`
// {test}`requirements.ci.input_containment`
// {test}`requirements.ci.inputs`
// {test}`requirements.ci.rust_item_inputs`
#[cfg(unix)]
#[test]
fn evidence_rejects_disallowed_ancestors_even_when_symlinks_target_allowed_sources() {
    let temp = fixture();
    let root = temp.path().join("project");
    for directory in ["private", "public", "sub"] {
        fs::create_dir(root.join(directory)).unwrap();
    }
    fs::write(root.join("public/source.rs"), "pub struct Source;\n").unwrap();
    std::os::unix::fs::symlink("../public", root.join("private/link")).unwrap();
    fs::write(
        root.join("habeas.toml"),
        "version = 1\n[evidence]\nallow = ['public']\ndisallow = ['private']\n",
    )
    .unwrap();
    for file in [
        "public/source.rs",
        "private/link/source.rs",
        "sub/../private/link/source.rs",
    ] {
        for input in [
            json!({"file":file, "line_start":1, "line_end":1, "text":["pub struct Source;"]}),
            json!({"rust_file":file, "rust_item":"struct[Source]", "text":["pub struct Source;"]}),
        ] {
            write_inputs(&root, vec![input]);
            let errors = validate_repository(&root).unwrap().errors.join("\n");
            if file == "public/source.rs" {
                assert!(errors.is_empty(), "{errors}");
            } else {
                assert!(errors.contains("disallowed"), "{file}: {errors}");
                assert!(errors.contains(file), "{errors}");
            }
        }
    }
}

// {test}`requirements.configuration.workspace_location`
// {test}`requirements.configuration.evidence_paths`
// {test}`requirements.configuration.workspace_structure`
// {test}`requirements.configuration.loading`
// {test}`requirements.authoring.deterministic_validation`
#[test]
fn validation_checks_workspace_common_config_without_loading_unused_authoring_settings() {
    let temp = fixture();
    let root = temp.path().join("project");
    for text in [
        "version = 2\n",
        "version = 1\nunknown = true\n",
        "version = [\n",
    ] {
        fs::write(root.join("habeas.toml"), text).unwrap();
        let errors = validate_repository(&root).unwrap().errors.join("\n");
        assert!(errors.contains("habeas.toml"), "{text:?}: {errors}");
    }
    fs::write(
        root.join("habeas.toml"),
        "version = 1\n[authoring]\nconfigs = 'invalid unused authoring settings'\n",
    )
    .unwrap();
    let report = validate_repository(&root).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
}

// {test}`requirements.authoring.generated_inputs`
// {test}`requirements.configuration.evidence_paths`
// {test}`requirements.ci.input_containment`
// {test}`requirements.ci.inputs`
// {test}`requirements.ci.rust_item_inputs`
// {test}`requirements.integration.report_api`
#[test]
fn discovered_external_cargo_outputs_validate_freshness_and_obey_disallow() {
    let temp = fixture();
    let root = temp.path().join("project");
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = 'fixture'\nversion = '0.1.0'\n",
    )
    .unwrap();
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = 'fixture'\nversion = '0.1.0'\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "").unwrap();
    fs::create_dir(root.join(".cargo")).unwrap();
    let build = temp.path().join("external-build");
    let target = temp.path().join("external-target");
    fs::write(
        root.join(".cargo/config.toml"),
        format!(
            "[build]\nbuild-dir = {}\ntarget-dir = {}\n",
            toml::Value::String(build.display().to_string()),
            toml::Value::String(target.display().to_string()),
        ),
    )
    .unwrap();
    let out = build.join("debug/build/fixture-hash/out");
    fs::create_dir_all(&out).unwrap();
    fs::write(
        out.parent().unwrap().join("root-output"),
        out.to_str().unwrap(),
    )
    .unwrap();
    let generated = out.join("generated.rs");
    fs::write(&generated, "pub fn generated() {}\n").unwrap();
    for input in [
        json!({"file":generated, "line_start":1, "line_end":1, "text":["pub fn generated() {}"]}),
        json!({"rust_file":generated, "rust_item":"fn[generated]", "text":["pub fn generated() {}"]}),
    ] {
        write_inputs(&root, vec![input]);
        fs::write(root.join("habeas.toml"), "version = 1\n").unwrap();
        let report = validate_repository(&root).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        fs::write(&generated, "pub fn generated() { /* changed */ }\n").unwrap();
        let errors = validate_repository(&root).unwrap().errors.join("\n");
        assert!(errors.contains("input text changed"), "{errors}");
        fs::write(&generated, "pub fn generated() {}\n").unwrap();
        fs::write(
            root.join("habeas.toml"),
            "version = 1\n[evidence]\ndisallow = ['../external-build']\n",
        )
        .unwrap();
        let errors = validate_repository(&root).unwrap().errors.join("\n");
        assert!(errors.contains("disallowed"), "{errors}");
    }
    assert!(!target.exists(), "metadata must not run a build");
}

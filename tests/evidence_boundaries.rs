use std::fs;
use std::path::Path;

use habeas::validate_repository;
use serde_json::{Value, json};
use tempfile::TempDir;

// {test}`requirements.ci.outline_inputs`
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
// {test}`requirements.ci.reference_set`
fn write_inputs(root: &Path, inputs: Vec<Value>) {
    fs::write(
        root.join("evidence/rules.ready.json"),
        json!({
            "requirement": "Read the data.",
            "determination": true,
            "rationale": "The fixture records the data being checked.",
            // An unreadable source makes the reference set unknowable; such
            // fixtures are invalid regardless.
            "references": habeas::reference_set_digest_for(root, "rules.ready")
                .unwrap_or_else(|_| "0".repeat(64)),
            "inputs": inputs,
        })
        .to_string(),
    )
    .unwrap();
}

/// The recorded outline input for `item` in `file`.
// {test}`requirements.ci.outline_inputs`
fn outline(root: &Path, file: &str, item: &str) -> Value {
    habeas::outline_input(root, file, item).unwrap()
}

// {test}`requirements.ci.outline_inputs`
// {test}`requirements.ci.input_containment`
// {test}`requirements.ci.diagnostics`
// {test}`requirements.ci.schema`
// {test}`requirements.ci.fixme_stale_input`
// {test}`requirements.references.selector_raw_lines`
// {test}`requirements.references.structural_hash`
#[test]
fn outline_inputs_survive_moves_and_unhashed_comments_but_not_edits() {
    let temp = fixture();
    let root = temp.path().join("project");
    let errors = |inputs: Vec<Value>| {
        write_inputs(&root, inputs);
        validate_repository(&root).unwrap().errors.join("\n")
    };
    let rust = "// CITE`rules.ready`\nfn alpha() { work(1); }\n\n/// Docs.\n// CITE`rules.ready`\nfn beta() {}\n"
        .replace("CITE", "{cite}");
    fs::write(root.join("src/lib.rs"), &rust).unwrap();
    let inputs = vec![
        outline(&root, "src/lib.rs", "fn[alpha]"),
        outline(&root, "src/lib.rs", "fn[beta]"),
        habeas::outline_inputs_for_lines(&root, "data.txt", 2, 2)
            .unwrap()
            .remove(0),
    ];
    assert_eq!(errors(inputs.clone()), "");

    // Moving nodes and lines, and editing unhashed comments, keeps inputs valid.
    fs::write(
        root.join("src/lib.rs"),
        format!("// CITE`rules.ready` moved below a new line\n\n{rust}")
            .replacen("CITE", "{cite}", 1)
            .replace("fn alpha() { work(1); }", "fn alpha() {\n    work(1);\n}"),
    )
    .unwrap();
    fs::write(root.join("data.txt"), "zero\nfirst\nsecond\n").unwrap();
    assert_eq!(errors(inputs.clone()), "");

    // Editing a node's code or its doc comment makes the input stale, naming the
    // file, selector, and current line range.
    fs::write(
        root.join("src/lib.rs"),
        rust.replace("work(1)", "work(2)")
            .replace("/// Docs.", "/// Changed docs."),
    )
    .unwrap();
    let found = errors(inputs.clone());
    for expected in [
        "inputs[0]: outline input changed: src/lib.rs fn[alpha] (now lines 1-2) no longer has the recorded hash",
        "inputs[1]: outline input changed: src/lib.rs fn[beta] (now lines 4-6) no longer has the recorded hash",
    ] {
        assert!(found.contains(expected), "missing {expected:?}: {found}");
    }
    assert!(!found.contains("inputs[2]"), "{found}");
    assert!(!found.contains("work(2)"), "no diff: {found}");

    // An edited plain-text line is a different node.
    fs::write(root.join("data.txt"), "first\nchanged\n").unwrap();
    let found = errors(inputs[2..].to_vec());
    assert!(
        found.contains("data.txt: no outline node matches \"line@"),
        "{found}"
    );

    fs::write(
        root.join("src/lib.rs"),
        "// CITE`rules.ready`\n#[cfg(a)]\nfn dup() {}\n// CITE`rules.ready`\n#[cfg(b)]\nfn dup() {}\n"
            .replace("CITE", "{cite}"),
    )
    .unwrap();
    let hash = "0".repeat(64);
    for (input, expected) in [
        (
            json!({"file":"src/lib.rs", "item":"fn[missing]", "hash":hash}),
            "src/lib.rs: no outline node matches \"fn[missing]\"",
        ),
        (
            json!({"file":"src/lib.rs", "item":"fn[dup]", "hash":hash}),
            "src/lib.rs: outline selector \"fn[dup]\" is ambiguous",
        ),
        (
            json!({"file":"src/lib.rs", "item":"fn[unclosed", "hash":hash}),
            "src/lib.rs: invalid outline selector \"fn[unclosed\"",
        ),
        (
            json!({"file":"src/lib.rs", "item":"fn[dup]#2", "hash":"ABC"}),
            "hash must be a lowercase hexadecimal SHA-256 digest",
        ),
        (
            json!({"file":"missing.txt", "item":"", "hash":hash}),
            "cannot open missing.txt",
        ),
        (
            json!({"file":"data.txt", "line_start":1, "line_end":1, "text":["first"]}),
            "unexpected field \"line_start\"",
        ),
        (
            json!({"rust_file":"src/lib.rs", "rust_item":"fn[dup]#1", "text":["fn dup() {}"]}),
            "unexpected field \"rust_file\"",
        ),
        (
            json!({"file":"src/lib.rs", "item":"fn[dup]#1", "hash":hash, "text":["fn dup() {}"]}),
            "unexpected field \"text\"",
        ),
    ] {
        let found = errors(vec![input]);
        assert!(found.contains(expected), "missing {expected:?}: {found}");
    }
    fs::write(root.join("src/broken.rs"), "fn {").unwrap();
    let found = errors(vec![
        json!({"file":"src/broken.rs", "item":"", "hash":hash}),
    ]);
    assert!(
        found.contains("src/broken.rs: cannot parse rust source"),
        "{found}"
    );
}

// {test}`requirements.ci.input_containment`
// {test}`requirements.ci.outline_inputs`
#[cfg(unix)]
#[test]
fn symlinks_may_resolve_inside_but_not_to_a_sibling_with_the_same_prefix() {
    use std::os::unix::fs::symlink;

    let temp = fixture();
    let root = temp.path().join("project");
    symlink(root.join("data.txt"), root.join("inside.txt")).unwrap();
    write_inputs(&root, vec![outline(&root, "inside.txt", "")]);
    assert!(validate_repository(&root).unwrap().errors.is_empty());

    let sibling = temp.path().join("project-sibling");
    fs::create_dir(&sibling).unwrap();
    fs::write(sibling.join("outside.rs"), "fn outside() {}\n").unwrap();
    symlink(sibling.join("outside.rs"), root.join("escape.rs")).unwrap();
    {
        let input = json!({"file":"escape.rs", "item":"fn[outside]", "hash":"0".repeat(64)});
        write_inputs(&root, vec![input]);
        let errors = validate_repository(&root).unwrap().errors.join("\n");
        assert!(
            errors.contains("escape.rs resolves outside the repository"),
            "{errors}"
        );
    }
}

// {test}`requirements.ci.input_containment`
#[test]
fn definition_and_requirements_inputs_obey_containment_for_declaring_files() {
    let temp = fixture();
    let root = temp.path().join("project");
    fs::create_dir(root.join("md/private")).unwrap();
    fs::write(
        root.join("md/private/terms.md"),
        ":::{req} secret\nSecret.\n:::\n\n:::{definition} term\n:phrase: term\n\nA term.\n:::\n",
    )
    .unwrap();
    let term = json!([
        ":::{definition} term",
        ":phrase: term",
        "",
        "A term.",
        ":::"
    ]);
    let inputs = vec![
        json!({"definition": "private.terms.term", "text": term}),
        json!({
            "requirements": "private.terms.*",
            "matches": ["private.terms.secret"],
            "text": [":::{req} secret", "Secret.", ":::"],
        }),
    ];
    write_inputs(&root, inputs.clone());
    let own_errors = |root: &Path| {
        validate_repository(root)
            .unwrap()
            .errors
            .into_iter()
            .filter(|error| error.starts_with("evidence/rules.ready.json"))
            .collect::<Vec<_>>()
    };
    assert!(own_errors(&root).is_empty(), "{:?}", own_errors(&root));

    fs::write(
        root.join("habeas.toml"),
        "version = 1\n[evidence]\ndisallow = ['md/private']\n",
    )
    .unwrap();
    let errors = own_errors(&root);
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors[0].contains("inputs[0]") && errors[0].contains("disallowed"));
    assert!(errors[1].contains("inputs[1]") && errors[1].contains("disallowed"));
    assert!(
        errors
            .iter()
            .all(|error| error.contains("md/private/terms.md"))
    );
}

// {test}`requirements.ci.input_containment`
#[cfg(unix)]
#[test]
fn unreadable_and_missing_inputs_are_reported() {
    use std::os::unix::fs::PermissionsExt;

    let temp = fixture();
    let root = temp.path().join("project");
    fs::write(root.join("locked.txt"), "secret\n").unwrap();
    let input = outline(&root, "locked.txt", "");
    fs::set_permissions(root.join("locked.txt"), fs::Permissions::from_mode(0o000)).unwrap();
    let missing = json!({"file": "gone.txt", "item": "", "hash": "0".repeat(64)});
    write_inputs(&root, vec![input, missing]);
    let errors = validate_repository(&root).unwrap().errors.join("\n");
    // A privileged user can read the file regardless of its mode.
    let unreadable = fs::read(root.join("locked.txt")).is_err();
    fs::set_permissions(root.join("locked.txt"), fs::Permissions::from_mode(0o644)).unwrap();
    assert!(
        errors.contains("inputs[1]: cannot open gone.txt"),
        "{errors}"
    );
    if unreadable {
        assert!(
            errors.contains("inputs[0]: cannot read locked.txt"),
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

// {test}`requirements.ci.orphans`
#[test]
fn evidence_named_for_terms_or_nothing_is_reported_at_the_top_level() {
    let temp = fixture();
    let root = temp.path().join("project");
    fs::write(
        root.join("md/rules.md"),
        ":::{req} ready\nRead the data.\n:::\n\n:::{definition} datum\n:phrase: datum\n\nA datum.\n:::\n",
    )
    .unwrap();
    assert!(validate_repository(&root).unwrap().errors.is_empty());
    let record = fs::read(root.join("evidence/rules.ready.json")).unwrap();
    for name in ["rules.datum", "rules.ghost"] {
        fs::write(root.join(format!("evidence/{name}.json")), &record).unwrap();
    }
    let errors = validate_repository(&root).unwrap().errors;
    let orphans: Vec<_> = errors
        .iter()
        .filter(|error| error.ends_with(": no matching requirement or example"))
        .collect();
    assert_eq!(
        orphans,
        [
            "evidence/rules.datum.json: no matching requirement or example",
            "evidence/rules.ghost.json: no matching requirement or example",
        ],
        "{errors:?}"
    );
    // Other files under evidence/ are not evidence JSON files.
    fs::remove_file(root.join("evidence/rules.datum.json")).unwrap();
    fs::remove_file(root.join("evidence/rules.ghost.json")).unwrap();
    fs::write(root.join("evidence/notes.txt"), "not evidence").unwrap();
    assert!(validate_repository(&root).unwrap().errors.is_empty());
}

// {test}`requirements.ci.orphans`
#[cfg(unix)]
#[test]
fn symlinked_evidence_json_files_are_checked_like_other_files() {
    let temp = fixture();
    let root = temp.path().join("project");
    let outside = temp.path().join("outside.json");
    fs::copy(root.join("evidence/rules.ready.json"), &outside).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("evidence/rules.ghost.json")).unwrap();
    fs::create_dir(root.join("evidence/nested")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("evidence/nested/rules.ready.json")).unwrap();
    let errors = validate_repository(&root).unwrap().errors.join("\n");
    for label in [
        "evidence/rules.ghost.json",
        "evidence/nested/rules.ready.json",
    ] {
        assert!(
            errors.contains(&format!("{label}: no matching requirement or example")),
            "{errors}"
        );
    }
    // A symlink with the expected name is accepted as that definition's file.
    fs::remove_file(root.join("evidence/rules.ghost.json")).unwrap();
    fs::remove_dir_all(root.join("evidence/nested")).unwrap();
    fs::remove_file(root.join("evidence/rules.ready.json")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("evidence/rules.ready.json")).unwrap();
    let report = validate_repository(&root).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
}

// {test}`requirements.configuration.evidence_paths`
// {test}`requirements.ci.input_containment`
// {test}`requirements.ci.outline_inputs`
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
    fs::write(root.join("habeas.toml"), &config).unwrap();
    {
        let input = outline(&root, source.to_str().unwrap(), "fn[generated]");
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
        fs::write(&source, "pub fn generated() { changed(); }\n").unwrap();
        assert!(
            validate_repository(&root)
                .unwrap()
                .errors
                .join("\n")
                .contains("outline input changed")
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
// {test}`requirements.ci.outline_inputs`
#[cfg(unix)]
#[test]
fn evidence_rejects_disallowed_ancestors_even_when_symlinks_target_allowed_sources() {
    let temp = fixture();
    let root = temp.path().join("project");
    for directory in ["private", "public", "sub"] {
        fs::create_dir(root.join(directory)).unwrap();
    }
    // Every struct needs a reference under rust_item_coverage.
    let cite = ["// {", "cite}`rules.ready`"].concat();
    fs::write(
        root.join("public/source.rs"),
        format!("{cite}\npub struct Source;\n"),
    )
    .unwrap();
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
        {
            let mut input = outline(&root, "public/source.rs", "struct[Source]");
            input["file"] = json!(file);
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
// {test}`requirements.ci.outline_inputs`
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
    {
        fs::write(root.join("habeas.toml"), "version = 1\n").unwrap();
        let input = outline(&root, generated.to_str().unwrap(), "fn[generated]");
        write_inputs(&root, vec![input]);
        let report = validate_repository(&root).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        fs::write(&generated, "pub fn generated() { changed(); }\n").unwrap();
        let errors = validate_repository(&root).unwrap().errors.join("\n");
        assert!(errors.contains("outline input changed"), "{errors}");
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

// {test}`requirements.integration.report_api`
#[test]
fn inaccessible_project_root_is_an_io_error_not_a_report() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("missing");
    let error = validate_repository(&missing).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{error}");

    // Ordinary invalid evidence in an accessible root is reported instead.
    let project = fixture();
    let root = project.path().join("project");
    fs::write(root.join("evidence/unknown.thing.json"), "{broken").unwrap();
    let report = validate_repository(&root).unwrap();
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.contains("unknown.thing.json")),
        "{:#?}",
        report.errors
    );
}

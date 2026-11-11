//! `habeas refactor` end to end: evidence stays valid across moves whose
//! content is unchanged, and only those.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};
use tempfile::TempDir;

/// A project with the `rules` chapter and `src/config.rs` implementing and
/// testing `rules.load`.
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
        concat!(
            "# Rules\n\n",
            ":::{req} load\nConfiguration MUST load.\n:::\n\n",
            ":::{req} check\n:impl: data.txt\nThe data MUST be checked.\n:::\n",
        ),
    )
    .unwrap();
    fs::write(root.join("data.txt"), "checked data\n").unwrap();
    fs::write(root.join("src/config.rs"), config_source("")).unwrap();
    temp
}

/// `src/config.rs`, with `extra` at the end of `load`'s body.
fn config_source(extra: &str) -> String {
    format!(
        concat!(
            "// {{impl}}`rules.load`\n",
            "fn load() {{ read();{} }}\n\n",
            "// {{impl}}`rules.check`\n",
            "fn check() {{}}\n\n",
            "// {{test}}`rules.load`\n",
            "// {{test}}`rules.check`\n",
            "fn verify() {{ load(); check(); }}\n",
        ),
        extra
    )
}

/// `load` alone, as it reads after moving to its own file.
const LOAD_ALONE: &str = "// {impl}`rules.load`\nfn load() { read(); }\n";

/// Writes current evidence for `name` reading `inputs`.
// {cite}`habeas:util`
fn write_evidence(root: &Path, name: &str, rationale: &str, inputs: Vec<Value>) {
    let body = match name {
        "rules.load" | "core.load" => "Configuration MUST load.",
        _ => "The data MUST be checked.",
    };
    fs::write(
        root.join(format!("evidence/{name}.json")),
        serde_json::to_string_pretty(&json!({
            "requirement": body,
            "determination": true,
            "rationale": rationale,
            "references": habeas::reference_set_digest_for(root, name).unwrap(),
            "inputs": inputs,
        }))
        .unwrap(),
    )
    .unwrap();
}

/// A requirements input selecting `selection` from the current book.
fn requirements_input(selection: &str, matches: &[&str], text: &[&str]) -> Value {
    json!({"requirements": selection, "matches": matches, "text": text})
}

const LOAD_TEXT: [&str; 3] = [":::{req} load", "Configuration MUST load.", ":::"];
// Raw declaration text omits anchor options, which the reference set covers.
const CHECK_TEXT: [&str; 3] = [":::{req} check", "The data MUST be checked.", ":::"];

/// Writes current evidence for both requirements.
fn author(root: &Path) {
    let outline = |file: &str, item: &str| habeas::outline_input(root, file, item).unwrap();
    write_evidence(
        root,
        "rules.load",
        "`src/config.rs#fn[load]` loads it, as `rules.load` asks; `verify` tests it in `src/config.rs`.",
        vec![
            requirements_input("rules.load", &["rules.load"], &LOAD_TEXT),
            outline("src/config.rs", "fn[load]"),
            outline("src/config.rs", "fn[verify]"),
        ],
    );
    let mut both = CHECK_TEXT.to_vec();
    both.push("");
    both.extend(LOAD_TEXT);
    write_evidence(
        root,
        "rules.check",
        "`data.txt` is checked by `src/config.rs#fn[check]`.",
        vec![
            requirements_input("rules.*", &["rules.check", "rules.load"], &both),
            outline("data.txt", ""),
            outline("src/config.rs", "fn[check]"),
            outline("src/config.rs", "fn[verify]"),
        ],
    );
}

/// The validation errors.
fn errors(root: &Path) -> Vec<String> {
    habeas::validate_repository(root).unwrap().errors
}

/// Runs `habeas refactor` in `root`.
fn refactor(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_habeas"))
        .arg("refactor")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap()
}

/// Every file under `root`, by relative path.
fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_owned()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path.strip_prefix(root).unwrap().display().to_string();
                files.insert(relative, fs::read(&path).unwrap());
            }
        }
    }
    files
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Moves `load` unchanged to `src/config/load.rs`.
fn move_load(root: &Path, extra: &str) {
    fs::create_dir_all(root.join("src/config")).unwrap();
    fs::write(
        root.join("src/config/load.rs"),
        LOAD_ALONE.replace("read();", &format!("read();{extra}")),
    )
    .unwrap();
    let rest = config_source("").replace(LOAD_ALONE, "");
    fs::write(root.join("src/config.rs"), rest).unwrap();
}

// {test}`requirements.refactor.moved_function`
// {test}`requirements.refactor.command`
// {test}`requirements.refactor.outline_inputs`
// {test}`requirements.refactor.reference_digest`
// {test}`requirements.refactor.rationale.addresses`
// {test}`requirements.refactor.rationale.untouched`
// {test}`requirements.refactor.record_fields`
// {test}`requirements.refactor.equivalence`
// {test}`requirements.refactor.whole_record`
#[test]
fn a_moved_function_keeps_its_evidence_current() {
    let temp = project();
    let root = temp.path();
    author(root);
    assert_eq!(errors(root), Vec::<String>::new());
    move_load(root, "");
    let stale = errors(root);
    assert!(
        stale.iter().any(|error| error.contains("fn[load]")),
        "{stale:#?}"
    );

    let output = refactor(
        root,
        &[
            "--map",
            "src/config.rs#fn[load]=src/config/load.rs#fn[load]",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let report = stdout(&output);
    assert!(
        report.contains("Changed 1 records:\n- evidence/rules.load.json\n"),
        "{report}"
    );
    // The other mention of the old file is listed for review.
    assert!(
        report.contains("review the rationale's `src/config.rs`"),
        "{report}"
    );
    // `rules.check` reads no moved node, so it is unaffected.
    assert!(!report.contains("rules.check"), "{report}");
    assert_eq!(errors(root), Vec::<String>::new());
    let record: Value =
        serde_json::from_str(&fs::read_to_string(root.join("evidence/rules.load.json")).unwrap())
            .unwrap();
    assert_eq!(record["inputs"][1]["file"], "src/config/load.rs");
    assert_eq!(
        record["rationale"],
        "`src/config/load.rs#fn[load]` loads it, as `rules.load` asks; `verify` tests it in `src/config.rs`."
    );
    // Running it again changes nothing.
    let again = refactor(
        root,
        &[
            "--map",
            "src/config.rs#fn[load]=src/config/load.rs#fn[load]",
        ],
    );
    assert!(
        stdout(&again).contains("Changed 0 records:"),
        "{}",
        stdout(&again)
    );
}

// {test}`requirements.refactor.edited_while_moving`
// {test}`requirements.refactor.outline_inputs`
// {test}`requirements.refactor.whole_record`
#[test]
fn a_function_edited_while_moving_leaves_its_records_stale() {
    let temp = project();
    let root = temp.path();
    author(root);
    move_load(root, " parse();");
    let before = fs::read(root.join("evidence/rules.load.json")).unwrap();
    let output = refactor(
        root,
        &[
            "--map",
            "src/config.rs#fn[load]=src/config/load.rs#fn[load]",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let report = stdout(&output);
    assert!(
        report.contains(
            "- evidence/rules.load.json: inputs[1]: src/config/load.rs#fn[load] no longer has the recorded hash"
        ),
        "{report}"
    );
    assert_eq!(
        fs::read(root.join("evidence/rules.load.json")).unwrap(),
        before
    );
    assert!(!errors(root).is_empty());
}

// {test}`requirements.refactor.moved_requirement`
// {test}`requirements.refactor.reference_rewrite`
// {test}`requirements.refactor.record_files`
// {test}`requirements.refactor.definition_inputs`
// {test}`requirements.refactor.subject_text`
// {test}`requirements.refactor.rationale.names`
#[test]
fn a_moved_requirement_renames_its_references_and_evidence() {
    let temp = project();
    let root = temp.path();
    author(root);
    // `load` moves to the `core` chapter; references still name the old name.
    fs::write(
        root.join("md/rules.md"),
        "# Rules\n\n:::{req} check\n:impl: data.txt\nThe data MUST be checked.\n:::\n",
    )
    .unwrap();
    fs::write(
        root.join("md/core.md"),
        "# Core\n\n:::{req} load\nConfiguration MUST load.\n:::\n",
    )
    .unwrap();
    let output = refactor(root, &["--map", "rules.load=core.load"]);
    assert!(output.status.success(), "{output:?}");
    let report = stdout(&output);
    assert!(report.contains("- src/config.rs (2)"), "{report}");
    assert!(
        report.contains("- evidence/rules.load.json -> evidence/core.load.json"),
        "{report}"
    );
    // The glob no longer matches what it recorded, so that record is stale.
    assert!(
        report.contains(
            "- evidence/rules.check.json: inputs[0]: rules.* no longer selects what it recorded"
        ),
        "{report}"
    );
    assert!(!root.join("evidence/rules.load.json").exists());
    let source = fs::read_to_string(root.join("src/config.rs")).unwrap();
    assert!(source.contains("// {impl}`core.load`") && source.contains("// {test}`core.load`"));
    let record: Value =
        serde_json::from_str(&fs::read_to_string(root.join("evidence/core.load.json")).unwrap())
            .unwrap();
    assert_eq!(
        record["inputs"][0],
        requirements_input("core.load", &["core.load"], &LOAD_TEXT)
    );
    assert!(
        record["rationale"]
            .as_str()
            .unwrap()
            .contains("as `core.load` asks")
    );
    let remaining = errors(root);
    assert!(
        remaining
            .iter()
            .all(|error| error.starts_with("evidence/rules.check.json")),
        "{remaining:#?}"
    );
}

// {test}`requirements.refactor.reference_rewrite`
// {test}`requirements.refactor.reference_digest`
// {test}`requirements.refactor.rationale.paths`
// {test}`requirements.refactor.equivalence`
#[test]
fn a_moved_file_rewrites_anchors_and_paths() {
    let temp = project();
    let root = temp.path();
    author(root);
    fs::rename(root.join("data.txt"), root.join("info.txt")).unwrap();
    let output = refactor(root, &["--map", "data.txt=info.txt"]);
    assert!(output.status.success(), "{output:?}");
    let book = fs::read_to_string(root.join("md/rules.md")).unwrap();
    assert!(book.contains(":impl: info.txt\n"), "{book}");
    let record: Value =
        serde_json::from_str(&fs::read_to_string(root.join("evidence/rules.check.json")).unwrap())
            .unwrap();
    assert!(
        record["rationale"]
            .as_str()
            .unwrap()
            .starts_with("`info.txt` is checked")
    );
    assert_eq!(errors(root), Vec::<String>::new());
}

// {test}`requirements.refactor.dry_run`
#[test]
fn a_dry_run_reports_the_same_changes_and_writes_nothing() {
    let temp = project();
    let root = temp.path();
    author(root);
    move_load(root, "");
    let before = snapshot(root);
    let args = [
        "--map",
        "src/config.rs#fn[load]=src/config/load.rs#fn[load]",
    ];
    let dry = refactor(root, &["--dry-run", args[0], args[1]]);
    assert!(dry.status.success(), "{dry:?}");
    assert_eq!(snapshot(root), before);
    let report = stdout(&refactor(root, &args));
    assert_eq!(
        stdout(&dry)
            .replace("Would change", "Changed")
            .replace("Would ", ""),
        report.replace("Rewrote", "rewrite")
    );
}

// {test}`requirements.refactor.invalid_mappings`
// {test}`requirements.refactor.mappings`
#[test]
fn invalid_mappings_fail_with_a_usage_error_and_change_nothing() {
    let temp = project();
    let root = temp.path();
    author(root);
    fs::create_dir_all(root.join("rules.load")).unwrap();
    let before = snapshot(root);
    for (args, message) in [
        (vec!["--map", "rules.load"], "expected <old>=<new>"),
        (vec!["--map", "=core.load"], "expected <old>=<new>"),
        (vec!["--map", "rules.check=rules.check"], "different"),
        (vec!["--map", "rules.load=core.load"], "ambiguous"),
        (
            vec!["--map", "rules.check=core.check"],
            "no definition named core.check",
        ),
        (
            vec!["--map", "src/config.rs=src/gone.rs"],
            "src/gone.rs does not exist",
        ),
        (
            vec!["--map", "src/config.rs#fn[load]=src/config.rs#fn[gone]"],
            "does not exist",
        ),
        (
            vec!["--map", "src=md", "--map", "src/config.rs=data.txt"],
            "overlap",
        ),
        (
            vec!["--map", "nothing.here=data.txt"],
            "names no recorded definition, file, or node",
        ),
    ] {
        let output = refactor(root, &args);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(message), "{args:?}: {stderr}");
        assert!(stderr.contains("Usage:"), "{stderr}");
        assert_eq!(snapshot(root), before, "{args:?}");
    }
    assert_eq!(refactor(root, &[]).status.code(), Some(2));
}

// {test}`requirements.refactor.record_files`
#[test]
fn a_rename_onto_an_existing_record_is_refused() {
    let temp = project();
    let root = temp.path();
    author(root);
    fs::write(
        root.join("md/rules.md"),
        "# Rules\n\n:::{req} check\n:impl: data.txt\nThe data MUST be checked.\n:::\n",
    )
    .unwrap();
    fs::write(
        root.join("md/core.md"),
        "# Core\n\n:::{req} load\nConfiguration MUST load.\n:::\n",
    )
    .unwrap();
    fs::write(root.join("evidence/core.load.json"), "{}\n").unwrap();
    let output = refactor(root, &["--map", "rules.load=core.load"]);
    assert!(output.status.success(), "{output:?}");
    assert!(
        stdout(&output)
            .contains("- evidence/rules.load.json: evidence/core.load.json would be overwritten"),
        "{}",
        stdout(&output)
    );
    assert_eq!(
        fs::read_to_string(root.join("evidence/core.load.json")).unwrap(),
        "{}\n"
    );
    assert!(root.join("evidence/rules.load.json").exists());
}

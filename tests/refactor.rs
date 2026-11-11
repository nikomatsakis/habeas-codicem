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

// {cite}`habeas:util`
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
const LOAD_ALONE: &str = "// \u{7b}impl}`rules.load`\nfn load() { read(); }\n";

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

// {cite}`habeas:util`
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

// {cite}`habeas:util`
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

// {cite}`habeas:util`
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
// {test}`requirements.refactor.mappings`
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
    let unaffected = fs::read(root.join("evidence/rules.check.json")).unwrap();

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
    // `rules.check` reads no moved node, so it is unaffected and unchanged.
    assert!(!report.contains("rules.check"), "{report}");
    assert_eq!(
        fs::read(root.join("evidence/rules.check.json")).unwrap(),
        unaffected
    );
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
// {test}`requirements.refactor.command`
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
// {test}`requirements.refactor.command`
// {test}`requirements.refactor.mappings`
#[test]
fn a_moved_requirement_renames_its_references_and_evidence() {
    let temp = project();
    let root = temp.path();
    author(root);
    fs::write(
        root.join("md/notes.md"),
        "Loading follows \u{7b}cite}`rules.load`.\n",
    )
    .unwrap();
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
    let before = snapshot(root);
    let output = refactor(root, &["--map", "rules.load=core.load"]);
    assert!(output.status.success(), "{output:?}");
    let report = stdout(&output);
    assert!(report.contains("- src/config.rs (2)"), "{report}");
    assert!(report.contains("- md/notes.md (1)"), "{report}");
    assert_eq!(
        fs::read_to_string(root.join("md/notes.md")).unwrap(),
        "Loading follows \u{7b}cite}`core.load`.\n"
    );
    // Only the rewritten references and the moved evidence changed.
    let after = snapshot(root);
    let differing = before
        .keys()
        .chain(after.keys())
        .filter(|file| before.get(*file) != after.get(*file))
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        differing.into_iter().collect::<Vec<_>>(),
        [
            "evidence/core.load.json",
            "evidence/rules.load.json",
            "md/notes.md",
            "src/config.rs",
        ]
    );
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
    assert!(
        source.contains("// \u{7b}impl}`core.load`")
            && source.contains("// \u{7b}test}`core.load`")
    );
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
// {test}`requirements.refactor.mappings`
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
            vec![
                "--map",
                "rules.check=rules.load",
                "--map",
                "rules=rules.load",
            ],
            "mappings of rules.check and rules overlap",
        ),
        (
            vec![
                "--map",
                "src/config.rs#fn[load]=src/config.rs#fn[check]",
                "--map",
                "src/config.rs#=src/config.rs#fn[verify]",
            ],
            "overlap",
        ),
        (
            vec!["--map", "#fn[load]=src/config.rs#fn[load]"],
            "a node needs a file",
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

// {test}`requirements.refactor.outline_inputs`
// {test}`requirements.refactor.reference_rewrite`
// {test}`requirements.refactor.equivalence`
#[test]
fn a_rename_rewrites_the_hash_of_a_node_whose_doc_comment_names_it() {
    let temp = project();
    let root = temp.path();
    // Doc comments are hashed, so the rewritten reference changes the hash.
    fs::write(
        root.join("src/config.rs"),
        config_source("").replace("// \u{7b}impl}`rules.load`", "/// \u{7b}impl}`rules.load`"),
    )
    .unwrap();
    author(root);
    assert_eq!(errors(root), Vec::<String>::new());
    let hash = |root: &Path| {
        habeas::outline_input(root, "src/config.rs", "fn[load]").unwrap()["hash"].clone()
    };
    let before = hash(root);
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
    let source = fs::read_to_string(root.join("src/config.rs")).unwrap();
    assert!(source.contains("/// \u{7b}impl}`core.load`"), "{source}");
    let after = hash(root);
    assert_ne!(before, after);
    let record: Value =
        serde_json::from_str(&fs::read_to_string(root.join("evidence/core.load.json")).unwrap())
            .unwrap();
    assert_eq!(record["inputs"][1]["hash"], after);
    // Only the glob record, whose match set changed, remains stale.
    let remaining = errors(root);
    assert!(
        remaining
            .iter()
            .all(|error| error.starts_with("evidence/rules.check.json")),
        "{remaining:#?}"
    );
}

// {test}`requirements.refactor.record_files`
// {test}`requirements.refactor.record_fields`
// {test}`requirements.refactor.equivalence`
#[test]
fn a_citation_review_follows_its_item_and_the_name_it_cites() {
    let temp = project();
    let root = temp.path();
    let cited = format!(
        "{}\n// {{cite}}`rules.load`\nfn helper() {{}}\n",
        config_source("")
    );
    fs::write(root.join("src/config.rs"), &cited).unwrap();
    author(root);
    let review = |root: &Path, file: &str, name: &str, text: &[&str]| {
        json!({
            "file": file,
            "item": "fn[helper]",
            "citations": [{"role": "cite", "name": name, "commentary": ""}],
            "determination": true,
            "rationale": format!("`{file}#fn[helper]` relies on `{name}`."),
            "inputs": [
                habeas::outline_input(root, file, "fn[helper]").unwrap(),
                requirements_input(name, &[name], text),
            ],
        })
    };
    fs::create_dir_all(root.join("evidence/citations/src/config.rs")).unwrap();
    fs::write(
        root.join("evidence/citations/src/config.rs/fn[helper].json"),
        serde_json::to_string_pretty(&review(root, "src/config.rs", "rules.load", &LOAD_TEXT))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(errors(root), Vec::<String>::new());
    // The helper moves to its own file, and `load` to the `core` chapter.
    fs::write(root.join("src/config.rs"), config_source("")).unwrap();
    fs::write(
        root.join("src/helper.rs"),
        "// \u{7b}cite}`rules.load`\nfn helper() {}\n",
    )
    .unwrap();
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
    let output = refactor(
        root,
        &[
            "--map",
            "src/config.rs#fn[helper]=src/helper.rs#fn[helper]",
            "--map",
            "rules.load=core.load",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    assert!(
        stdout(&output).contains(
            "- evidence/citations/src/config.rs/fn[helper].json -> evidence/citations/src/helper.rs/fn[helper].json"
        ),
        "{}",
        stdout(&output)
    );
    assert!(!root.join("evidence/citations/src/config.rs").exists());
    let moved: Value = serde_json::from_str(
        &fs::read_to_string(root.join("evidence/citations/src/helper.rs/fn[helper].json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        moved,
        review(root, "src/helper.rs", "core.load", &LOAD_TEXT)
    );
    let remaining = errors(root);
    assert!(
        remaining
            .iter()
            .all(|error| error.starts_with("evidence/rules.check.json")),
        "{remaining:#?}"
    );
}

// {cite}`habeas:util`
/// Moves `load` from the `rules` chapter to `core`, with `body`.
fn move_load_requirement(root: &Path, body: &str) {
    fs::write(
        root.join("md/rules.md"),
        "# Rules\n\n:::{req} check\n:impl: data.txt\nThe data MUST be checked.\n:::\n",
    )
    .unwrap();
    fs::write(
        root.join("md/core.md"),
        format!("# Core\n\n:::{{req}} load\n{body}\n:::\n"),
    )
    .unwrap();
}

// {test}`requirements.refactor.record_files`
#[test]
fn a_move_onto_a_record_whose_own_move_is_refused_is_refused_too() {
    let temp = project();
    let root = temp.path();
    // `core.load` exists, with evidence of its own.
    fs::write(
        root.join("md/core.md"),
        "# Core\n\n:::{req} load\nConfiguration MUST load.\n:::\n",
    )
    .unwrap();
    author(root);
    write_evidence(
        root,
        "core.load",
        "Fine.",
        vec![requirements_input("core.load", &["core.load"], &LOAD_TEXT)],
    );
    // `rules.load` becomes `core.load`, and `core.load` becomes `next.load`,
    // whose evidence path is already taken.
    fs::write(
        root.join("md/rules.md"),
        "# Rules\n\n:::{req} check\n:impl: data.txt\nThe data MUST be checked.\n:::\n",
    )
    .unwrap();
    fs::write(
        root.join("md/next.md"),
        "# Next\n\n:::{req} load\nConfiguration MUST load.\n:::\n",
    )
    .unwrap();
    fs::write(root.join("evidence/next.load.json"), "{}\n").unwrap();
    let before = snapshot(root);
    let output = refactor(
        root,
        &[
            "--map",
            "rules.load=core.load",
            "--map",
            "core.load=next.load",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let report = stdout(&output);
    assert!(
        report.contains("- evidence/core.load.json: evidence/next.load.json would be overwritten"),
        "{report}"
    );
    assert!(
        report.contains("- evidence/rules.load.json: evidence/core.load.json would be overwritten"),
        "{report}"
    );
    let after = snapshot(root);
    for file in [
        "evidence/core.load.json",
        "evidence/next.load.json",
        "evidence/rules.load.json",
    ] {
        assert_eq!(after.get(file), before.get(file), "{file}");
    }
}

// {test}`requirements.refactor.subject_text`
// {test}`requirements.refactor.whole_record`
#[test]
fn a_requirement_edited_while_moving_keeps_its_old_record() {
    let temp = project();
    let root = temp.path();
    author(root);
    move_load_requirement(root, "Configuration MUST load quickly.");
    let before = fs::read(root.join("evidence/rules.load.json")).unwrap();
    let output = refactor(root, &["--map", "rules.load=core.load"]);
    assert!(output.status.success(), "{output:?}");
    let report = stdout(&output);
    assert!(
        report.contains("- evidence/rules.load.json: requirement text of core.load changed"),
        "{report}"
    );
    assert_eq!(
        fs::read(root.join("evidence/rules.load.json")).unwrap(),
        before
    );
    assert!(!root.join("evidence/core.load.json").exists());
}

// {test}`requirements.refactor.reference_digest`
// {test}`requirements.refactor.whole_record`
#[test]
fn a_reference_set_changed_beyond_the_mapping_blocks_every_rewrite() {
    let temp = project();
    let root = temp.path();
    author(root);
    move_load_requirement(root, "Configuration MUST load.");
    // A new `{impl}` is not part of the move.
    fs::write(
        root.join("src/config.rs"),
        config_source("").replace(
            "// \u{7b}impl}`rules.check`\n",
            "// \u{7b}impl}`rules.check`\n// \u{7b}impl}`rules.load`\n",
        ),
    )
    .unwrap();
    let before = fs::read(root.join("evidence/rules.load.json")).unwrap();
    let output = refactor(root, &["--map", "rules.load=core.load"]);
    assert!(output.status.success(), "{output:?}");
    let report = stdout(&output);
    // Its text and requirements input would be rewritten, but the reference
    // set check fails, so nothing is.
    assert!(
        report.contains("- evidence/rules.load.json: the reference set of core.load changed"),
        "{report}"
    );
    assert!(!report.contains("text of core.load changed"), "{report}");
    assert_eq!(
        fs::read(root.join("evidence/rules.load.json")).unwrap(),
        before
    );
}

// {cite}`habeas:util`
/// Adds `usage.use`, which uses the term `terms.term`, with current
/// evidence reading both.
fn add_term(root: &Path) {
    fs::write(
        root.join("md/terms.md"),
        "# Terms\n\n:::{definition} term\n:phrase: term\n\nA word.\n:::\n",
    )
    .unwrap();
    fs::write(
        root.join("md/usage.md"),
        "# Usage\n\n:::{req} use\nThe \u{7b}term}`terms.term` MUST be used.\n:::\n",
    )
    .unwrap();
    fs::write(
        root.join("evidence/usage.use.json"),
        serde_json::to_string_pretty(&json!({
            "requirement": "The \u{7b}term}`terms.term` MUST be used.",
            "determination": true,
            "rationale": "Nothing implements `usage.use`, which uses `terms.term`.",
            "references": habeas::reference_set_digest_for(root, "usage.use").unwrap(),
            "inputs": [
                requirements_input(
                    "usage.use",
                    &["usage.use"],
                    &[":::{req} use", "The \u{7b}term}`terms.term` MUST be used.", ":::"],
                ),
                {"definition": "terms.term", "text": [":::{definition} term", ":phrase: term", "", "A word.", ":::"]},
            ],
        }))
        .unwrap(),
    )
    .unwrap();
}

// {cite}`habeas:util`
/// The validation errors of `usage.use`.
fn usage_errors(root: &Path) -> Vec<String> {
    errors(root)
        .into_iter()
        .filter(|error| error.starts_with("evidence/usage.use.json"))
        .collect()
}

// {test}`requirements.refactor.definition_inputs`
// {test}`requirements.refactor.subject_text`
// {test}`requirements.refactor.reference_rewrite`
// {test}`requirements.refactor.rationale.names`
// {test}`requirements.refactor.record_fields`
#[test]
fn a_moved_term_rewrites_definition_inputs_and_the_texts_using_it() {
    let temp = project();
    let root = temp.path();
    add_term(root);
    assert_eq!(usage_errors(root), Vec::<String>::new());
    fs::remove_file(root.join("md/terms.md")).unwrap();
    fs::write(
        root.join("md/glossary.md"),
        "# Glossary\n\n:::{definition} term\n:phrase: term\n\nA word.\n:::\n",
    )
    .unwrap();
    let output = refactor(root, &["--map", "terms.term=glossary.term"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read_to_string(root.join("md/usage.md")).unwrap(),
        "# Usage\n\n:::{req} use\nThe \u{7b}term}`glossary.term` MUST be used.\n:::\n"
    );
    let record: Value =
        serde_json::from_str(&fs::read_to_string(root.join("evidence/usage.use.json")).unwrap())
            .unwrap();
    assert_eq!(
        record["requirement"],
        "The \u{7b}term}`glossary.term` MUST be used."
    );
    assert_eq!(record["inputs"][1]["definition"], "glossary.term");
    assert_eq!(
        record["rationale"],
        "Nothing implements `usage.use`, which uses `glossary.term`."
    );
    assert_eq!(usage_errors(root), Vec::<String>::new());
}

// {test}`requirements.refactor.definition_inputs`
// {test}`requirements.refactor.whole_record`
#[test]
fn a_term_edited_while_moving_leaves_its_definition_input_stale() {
    let temp = project();
    let root = temp.path();
    add_term(root);
    fs::remove_file(root.join("md/terms.md")).unwrap();
    fs::write(
        root.join("md/glossary.md"),
        "# Glossary\n\n:::{definition} term\n:phrase: term\n\nA longer word.\n:::\n",
    )
    .unwrap();
    let before = fs::read(root.join("evidence/usage.use.json")).unwrap();
    let report = stdout(&refactor(root, &["--map", "terms.term=glossary.term"]));
    assert!(
        report.contains(
            "- evidence/usage.use.json: inputs[1]: glossary.term no longer selects what it recorded"
        ),
        "{report}"
    );
    assert_eq!(
        fs::read(root.join("evidence/usage.use.json")).unwrap(),
        before
    );
}

// {test}`requirements.refactor.outline_inputs`
#[test]
fn an_input_whose_node_is_missing_after_the_move_is_not_rewritten() {
    let temp = project();
    let root = temp.path();
    author(root);
    // The file moves, but `verify` does not come with it.
    fs::remove_file(root.join("src/config.rs")).unwrap();
    fs::write(
        root.join("src/settings.rs"),
        config_source("").replace("fn verify() { load(); check(); }\n", ""),
    )
    .unwrap();
    let before = fs::read(root.join("evidence/rules.load.json")).unwrap();
    let report = stdout(&refactor(root, &["--map", "src/config.rs=src/settings.rs"]));
    assert!(
        report.contains("inputs[2]: src/settings.rs: no outline node matches \"fn[verify]\""),
        "{report}"
    );
    assert_eq!(
        fs::read(root.join("evidence/rules.load.json")).unwrap(),
        before
    );
}

// {test}`requirements.refactor.equivalence`
#[test]
fn a_record_failing_before_the_move_fails_the_same_way_after_it() {
    let temp = project();
    let root = temp.path();
    author(root);
    let path = root.join("evidence/rules.load.json");
    let mut record: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    record["determination"] = json!(false);
    fs::write(&path, serde_json::to_string_pretty(&record).unwrap()).unwrap();
    let failing = errors(root);
    assert!(!failing.is_empty());
    move_load(root, "");
    let output = refactor(
        root,
        &[
            "--map",
            "src/config.rs#fn[load]=src/config/load.rs#fn[load]",
        ],
    );
    assert!(
        stdout(&output).contains("- evidence/rules.load.json\n"),
        "{}",
        stdout(&output)
    );
    // The same failure, quoting the rationale as rewritten.
    let expected = failing
        .iter()
        .map(|error| error.replace("`src/config.rs#fn[load]`", "`src/config/load.rs#fn[load]`"))
        .collect::<Vec<_>>();
    assert_eq!(errors(root), expected);
}

// {test}`requirements.refactor.record_fields`
// {test}`requirements.refactor.definition_inputs`
// {test}`requirements.refactor.equivalence`
// {test}`requirements.refactor.mappings`
#[test]
fn a_moved_chapter_rewrites_glob_selections_and_their_matches() {
    let temp = project();
    let root = temp.path();
    author(root);
    let chapter = fs::read_to_string(root.join("md/rules.md")).unwrap();
    fs::remove_file(root.join("md/rules.md")).unwrap();
    fs::write(
        root.join("md/core.md"),
        chapter.replace("# Rules", "# Core"),
    )
    .unwrap();
    let output = refactor(root, &["--map", "rules=core"]);
    assert!(output.status.success(), "{output:?}");
    let record: Value =
        serde_json::from_str(&fs::read_to_string(root.join("evidence/core.check.json")).unwrap())
            .unwrap();
    assert_eq!(record["inputs"][0]["requirements"], "core.*");
    assert_eq!(
        record["inputs"][0]["matches"],
        json!(["core.check", "core.load"])
    );
    assert_eq!(errors(root), Vec::<String>::new());
}

// {test}`requirements.refactor.record_fields`
#[test]
fn a_citation_reviews_commentary_follows_a_renamed_requirement() {
    let temp = project();
    let root = temp.path();
    let source = |name: &str| {
        format!(
            "{}\n// :::{{cite}} rules.check\n// Shares {{term}}`{name}` wording.\n// :::\nfn helper() {{}}\n",
            config_source("")
        )
    };
    fs::write(
        root.join("md/terms.md"),
        "# Terms\n\n:::{definition} word\n:phrase: word\n\nA word.\n:::\n",
    )
    .unwrap();
    fs::write(root.join("src/config.rs"), source("terms.word")).unwrap();
    author(root);
    let review = |name: &str| {
        json!({
            "file": "src/config.rs",
            "item": "fn[helper]",
            "citations": [{"role": "cite", "name": "rules.check", "commentary": format!("Shares {{term}}`{name}` wording.")}],
            "determination": true,
            "rationale": "It relies on `rules.check`.",
            "inputs": [
                habeas::outline_input(root, "src/config.rs", "fn[helper]").unwrap(),
                requirements_input("rules.check", &["rules.check"], &CHECK_TEXT),
                {"definition": name, "text": [":::{definition} word", ":phrase: word", "", "A word.", ":::"]},
            ],
        })
    };
    let path = root.join("evidence/citations/src/config.rs/fn[helper].json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        serde_json::to_string_pretty(&review("terms.word")).unwrap(),
    )
    .unwrap();
    let citation_errors = |root: &Path| {
        errors(root)
            .into_iter()
            .filter(|error| error.starts_with("evidence/citations/"))
            .collect::<Vec<_>>()
    };
    assert_eq!(citation_errors(root), Vec::<String>::new());
    fs::remove_file(root.join("md/terms.md")).unwrap();
    fs::write(
        root.join("md/glossary.md"),
        "# Glossary\n\n:::{definition} word\n:phrase: word\n\nA word.\n:::\n",
    )
    .unwrap();
    let output = refactor(root, &["--map", "terms.word=glossary.word"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read_to_string(root.join("src/config.rs")).unwrap(),
        source("glossary.word")
    );
    let record: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(record["citations"], review("glossary.word")["citations"]);
    assert_eq!(record["inputs"][2]["definition"], "glossary.word");
    assert_eq!(citation_errors(root), Vec::<String>::new());
}

// {test}`requirements.refactor.reference_digest`
// {test}`requirements.refactor.whole_record`
// {test}`requirements.refactor.command`
#[test]
fn a_changed_reference_set_is_reported_on_a_record_reading_nothing_moved() {
    let temp = project();
    let root = temp.path();
    author(root);
    // `rules.check` records only its glob input, so a move touches only its
    // reference set.
    let path = root.join("evidence/rules.check.json");
    let mut record: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    record["inputs"] = json!([record["inputs"][0]]);
    fs::write(&path, serde_json::to_string_pretty(&record).unwrap()).unwrap();
    let before = fs::read(&path).unwrap();
    // `check` moves, and an extra `{impl}` is added that is not part of it.
    fs::write(
        root.join("src/config.rs"),
        config_source("")
            .replace("// \u{7b}impl}`rules.check`\nfn check() {}\n\n", "")
            .replace(
                "// \u{7b}impl}`rules.load`\n",
                "// \u{7b}impl}`rules.load`\n// \u{7b}impl}`rules.check`\n",
            ),
    )
    .unwrap();
    fs::write(
        root.join("src/check.rs"),
        "// \u{7b}impl}`rules.check`\nfn check() {}\n",
    )
    .unwrap();
    let report = stdout(&refactor(
        root,
        &["--map", "src/config.rs#fn[check]=src/check.rs#fn[check]"],
    ));
    assert!(
        report.contains("- evidence/rules.check.json: the reference set of rules.check changed"),
        "{report}"
    );
    assert_eq!(fs::read(&path).unwrap(), before);
}

// {test}`requirements.refactor.subject_text`
// {test}`requirements.refactor.record_files`
#[test]
fn a_moved_example_rewrites_its_example_field() {
    let temp = project();
    let root = temp.path();
    author(root);
    // The role's brace is an argument, so this source holds no reference.
    let body = |name: &str| format!("Loading works. {}cite}}`{name}`", '{');
    fs::write(
        root.join("md/cases.md"),
        format!(
            "# Cases\n\n:::{{example}} loading\n{}\n:::\n",
            body("rules.load")
        ),
    )
    .unwrap();
    fs::write(
        root.join("evidence/cases.loading.json"),
        serde_json::to_string_pretty(&json!({
            "example": body("rules.load"),
            "determination": false,
            "rationale": "No test covers `cases.loading`.",
            "references": habeas::reference_set_digest_for(root, "cases.loading").unwrap(),
            "inputs": [
                requirements_input("cases.loading", &["cases.loading"], &[":::{example} loading", &body("rules.load"), ":::"]),
                requirements_input("rules.load", &["rules.load"], &LOAD_TEXT),
            ],
        }))
        .unwrap(),
    )
    .unwrap();
    let case_errors = |root: &Path| {
        errors(root)
            .into_iter()
            .filter(|error| error.starts_with("evidence/cases."))
            .collect::<Vec<_>>()
    };
    let before = case_errors(root);
    // The example moves to the `examples` chapter.
    fs::remove_file(root.join("md/cases.md")).unwrap();
    fs::write(
        root.join("md/examples.md"),
        format!(
            "# Examples\n\n:::{{example}} loading\n{}\n:::\n",
            body("rules.load")
        ),
    )
    .unwrap();
    let output = refactor(root, &["--map", "cases.loading=examples.loading"]);
    assert!(output.status.success(), "{output:?}");
    let record: Value = serde_json::from_str(
        &fs::read_to_string(root.join("evidence/examples.loading.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(record["example"], body("rules.load"));
    assert!(record.get("requirement").is_none());
    // It fails validation exactly as before, under its new name.
    let after = errors(root)
        .into_iter()
        .filter(|error| error.starts_with("evidence/examples."))
        .map(|error| error.replace("examples.loading", "cases.loading"))
        .collect::<Vec<_>>();
    let before = before
        .into_iter()
        .map(|error| {
            error
                .replacen("evidence/cases.", "evidence/examples.", 1)
                .replace("examples.loading", "cases.loading")
        })
        .collect::<Vec<_>>();
    assert_eq!(after, before);
}

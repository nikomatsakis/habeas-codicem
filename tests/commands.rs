use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::json;
use tempfile::TempDir;

// {test}`requirements.cli.status`
// {test}`requirements.cli.info`
fn fixture() -> TempDir {
    let temp = TempDir::new().unwrap();
    fs::create_dir_all(temp.path().join("md")).unwrap();
    fs::create_dir_all(temp.path().join("evidence")).unwrap();
    fs::create_dir_all(temp.path().join("src/nested")).unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname = \"fixture\"\n",
    )
    .unwrap();
    fs::write(temp.path().join("book.toml"), "[book]\nsrc = \"md\"\n").unwrap();
    fs::write(temp.path().join("md/SUMMARY.md"), "# Summary\n").unwrap();
    let requirements = [
        ("ready", "Ready MUST work."),
        ("xready", "Xready MUST work."),
        ("pending", "Pending MUST work."),
        ("stale", "Stale MUST work."),
        ("stale_input", "Stale input MUST work."),
        ("absent", "Absent MUST work."),
        ("malformed", "Malformed MUST work."),
        ("uncovered", "Uncovered MUST work."),
    ];
    let mut source = String::from("# Requirements\n\n");
    for (name, text) in requirements {
        source.push_str(&format!(":::{{req}} {name}\n{text}\n:::\n\n"));
    }
    source.push_str(
        &":::{example} case\nGiven ready, it works. CITE`requirements.ready`\n:::\n"
            .replace("CITE", "{cite}"),
    );
    let case_line = source
        .lines()
        .position(|line| line.contains("Given ready, it works."))
        .unwrap()
        + 1;
    let case_text = source.lines().nth(case_line - 1).unwrap().to_owned();
    fs::write(temp.path().join("md/requirements.md"), source).unwrap();
    fs::write(
        temp.path().join("src/main.rs"),
        ["fn main() {}\n// {", "impl}`requirements.uncovered`\n"].concat(),
    )
    .unwrap();
    for (name, text, determination) in [
        ("ready", "Ready MUST work.", true),
        ("xready", "Xready MUST work.", true),
        ("pending", "Pending MUST work.", false),
        ("stale", "Old requirement.", true),
        ("stale_input", "Stale input MUST work.", true),
        ("uncovered", "Uncovered MUST work.", true),
    ] {
        let inputs = if name == "ready" {
            vec![
                json!({
                    "file": "src/main.rs",
                    "line_start": 1,
                    "line_end": 1,
                    "text": ["fn main() {}"]
                }),
                json!({
                    "file": "md/requirements.md",
                    "line_start": case_line,
                    "line_end": case_line,
                    "text": [case_text]
                }),
            ]
        } else if name == "stale_input" {
            vec![json!({
                "file": "src/main.rs",
                "line_start": 1,
                "line_end": 1,
                "text": ["old main"]
            })]
        } else {
            vec![]
        };
        fs::write(
            temp.path()
                .join(format!("evidence/requirements.{name}.json")),
            json!({
                "requirement": text,
                "determination": determination,
                "rationale": format!("**{name}** was inspected."),
                "inputs": inputs
            })
            .to_string(),
        )
        .unwrap();
    }
    fs::write(
        temp.path().join("evidence/requirements.malformed.json"),
        "{broken",
    )
    .unwrap();
    temp
}

// {test}`requirements.cli.status`
// {test}`requirements.cli.info`
fn habeas(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_habeas"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

// {test}`requirements.cli.status`
#[test]
fn status_lists_nonvalid_requirements_and_all_option_in_name_order() {
    let temp = fixture();
    let nested = temp.path().join("src/nested");
    let output = habeas(&nested, &["status"]);
    assert!(output.status.success(), "{:?}", output);
    let text = String::from_utf8(output.stdout).unwrap();
    for (name, state) in [
        ("absent", "Missing"),
        ("malformed", "Invalid"),
        ("pending", "Determination false"),
        ("stale", "Invalid"),
        ("stale_input", "Invalid"),
        ("uncovered", "Invalid"),
    ] {
        assert!(
            text.lines().any(|line| {
                line.starts_with(&format!("| `requirements.{name}`"))
                    && line.contains(&format!("| {state}"))
            }),
            "{text}"
        );
    }
    assert!(!text.contains("| `requirements.ready`"), "{text}");
    assert!(!text.contains("| `requirements.xready`"), "{text}");
    assert!(text.contains("missing for requirement"), "{text}");
    assert!(text.contains("determination is false"), "{text}");
    assert!(text.contains("requirement text changed"), "{text}");
    assert!(text.contains("input text changed"), "{text}");
    assert!(text.contains("invalid JSON"), "{text}");
    assert!(text.contains("no input covers"), "{text}");

    let output = habeas(&nested, &["status", "--all"]);
    assert!(output.status.success(), "{:?}", output);
    let all = String::from_utf8(output.stdout).unwrap();
    assert!(all.contains("| `requirements.case` (example)"), "{all}");
    let names = [
        "absent",
        "malformed",
        "pending",
        "ready",
        "stale",
        "stale_input",
        "uncovered",
        "xready",
    ];
    let positions = names
        .map(|name| all.find(&format!("| `requirements.{name}`")).unwrap())
        .to_vec();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]), "{all}");
    assert!(
        all.lines()
            .any(|line| { line.starts_with("| `requirements.ready`") && line.contains("| Valid") }),
        "{all}"
    );
}

// {test}`requirements.cli.info`
// {test}`requirements.cli.definition_glob_search`
#[test]
fn info_renders_exact_and_glob_matches_as_markdown_without_an_index() {
    let temp = fixture();
    let output = habeas(temp.path(), &["info", "requirements.ready"]);
    assert!(output.status.success(), "{:?}", output);
    let exact = String::from_utf8(output.stdout).unwrap();
    assert!(exact.starts_with("# `requirements.ready`\n"), "{exact}");
    assert!(!exact.contains("# `requirements.xready`"), "{exact}");
    assert!(
        exact.contains("## Requirement\n\n> Ready MUST work."),
        "{exact}"
    );
    assert!(
        exact.contains("## Rationale\n\n**ready** was inspected."),
        "{exact}"
    );
    assert!(exact.contains("## Inputs"), "{exact}");
    assert!(exact.contains("src/main.rs:1–1"), "{exact}");
    assert!(
        exact.contains("```rust,noplayground\nfn main() {}"),
        "{exact}"
    );
    assert!(
        exact.contains("img.shields.io/badge/evidence-Valid-237346"),
        "{exact}"
    );

    let output = habeas(temp.path(), &["info", "requirements.*"]);
    assert!(output.status.success(), "{:?}", output);
    let matches = String::from_utf8(output.stdout).unwrap();
    let pending = matches.find("# `requirements.pending`").unwrap();
    let ready = matches.find("# `requirements.ready`").unwrap();
    assert!(pending < ready, "{matches}");
    assert!(matches.contains("# `requirements.xready`"), "{matches}");
    assert!(
        matches.contains("# `requirements.case`\n\nDefinition:"),
        "{matches}"
    );
    assert!(
        matches.contains("## Example\n\n> Given ready, it works."),
        "{matches}"
    );
    assert!(matches.contains("\n\n---\n\n"), "{matches}");

    let missing = habeas(temp.path(), &["info", "requirements.absent"]);
    assert!(missing.status.success(), "{:?}", missing);
    let missing = String::from_utf8(missing.stdout).unwrap();
    assert!(missing.contains("## Validation findings"), "{missing}");
    assert!(missing.contains("missing for requirement"), "{missing}");
    assert!(missing.contains("No rationale recorded."), "{missing}");
    assert!(missing.contains("No inputs recorded."), "{missing}");

    let malformed = habeas(temp.path(), &["info", "requirements.malformed"]);
    assert!(malformed.status.success(), "{:?}", malformed);
    let malformed = String::from_utf8(malformed.stdout).unwrap();
    assert!(
        malformed.contains("## Unreadable evidence record"),
        "{malformed}"
    );
    assert!(malformed.contains("```json\n{broken\n```"), "{malformed}");
}

// {test}`requirements.cli.status`
// {test}`requirements.cli.info`
// {test}`requirements.references.definition_globs`
#[test]
fn commands_report_invalid_arguments_and_unmatched_selectors() {
    let temp = fixture();
    for args in [
        ["status", "--wrong"].as_slice(),
        ["status", "--all", "extra", "more"].as_slice(),
        ["info"].as_slice(),
        ["info", "ready", "extra"].as_slice(),
    ] {
        let output = habeas(temp.path(), args);
        assert!(!output.status.success(), "{args:?}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("Usage: habeas"),
            "{args:?}: {output:?}"
        );
    }
    let invalid = habeas(temp.path(), &["info", "["]);
    assert!(!invalid.status.success());
    assert!(
        String::from_utf8_lossy(&invalid.stderr).contains("invalid definition glob"),
        "{invalid:?}"
    );
    let unmatched = habeas(temp.path(), &["info", "nothing.*"]);
    assert!(!unmatched.status.success());
    assert!(
        String::from_utf8_lossy(&unmatched.stderr).contains("no definitions match"),
        "{unmatched:?}"
    );
    let status = habeas(temp.path(), &["status", "requirements.*ready", "--all"]);
    assert!(status.status.success(), "{status:?}");
    let status = String::from_utf8(status.stdout).unwrap();
    assert!(
        status.contains("`requirements.ready` (requirement)"),
        "{status}"
    );
    assert!(
        status.contains("`requirements.xready` (requirement)"),
        "{status}"
    );
    assert!(!status.contains("| `requirements.pending`"), "{status}");
    let no_status = habeas(temp.path(), &["status", "nothing.*"]);
    assert!(!no_status.status.success());
    assert!(String::from_utf8_lossy(&no_status.stderr).contains("no definitions match"));
}

// {test}`requirements.cli.status_table_alignment`
#[test]
fn status_tables_align_each_column_to_its_widest_cell() {
    let temp = fixture();
    for args in [["status"].as_slice(), ["status", "--all"].as_slice()] {
        let output = habeas(temp.path(), args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        let text = String::from_utf8(output.stdout).unwrap();
        let table = text
            .lines()
            .skip_while(|line| !line.starts_with("| "))
            .take_while(|line| line.starts_with("| "))
            .collect::<Vec<_>>();
        assert!(table.len() > 2, "{text}");
        let pipes = table[0]
            .match_indices('|')
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        assert_eq!(pipes.len(), 3, "{text}");
        for line in &table {
            let actual = line
                .match_indices('|')
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            assert_eq!(actual, pipes, "{text}");
            for cell in line.split('|').skip(1).take(2) {
                assert!(cell.starts_with(' ') && cell.ends_with(' '), "{text}");
                assert!(!cell[1..].starts_with(' '), "{text}");
            }
        }
        for column in 0..2 {
            let content_width = table
                .iter()
                .map(|line| line.split('|').nth(column + 1).unwrap().trim().len())
                .max()
                .unwrap();
            assert_eq!(
                pipes[column + 1] - pipes[column] - 3,
                content_width,
                "{text}"
            );
        }
        assert_eq!(table[1].split('|').nth(1).unwrap().trim(), "---");
        assert_eq!(table[1].split('|').nth(2).unwrap().trim(), "---");
    }
}

// {test}`requirements.cli.add_input`
// {test}`requirements.cli.add_input_file_range`
// {test}`requirements.cli.add_input_rust_item`
// {test}`requirements.cli.add_input_requirements`
// {test}`requirements.references.selector_same_line`
// {test}`requirements.references.selector_raw_lines`
#[test]
fn add_input_modes_capture_raw_sources_and_refresh_idempotently() {
    let temp = TempDir::new().unwrap();
    fs::create_dir_all(temp.path().join("md")).unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname = \"fixture\"\n",
    )
    .unwrap();
    fs::write(temp.path().join("book.toml"), "[book]\nsrc = \"md\"\n").unwrap();
    fs::write(
        temp.path().join("md/rules.md"),
        ":::{req} ready\nReady.\n:::\n\n:::{example} case\nGiven ready, it works. CITE`rules.ready`\n:::\n".replace("CITE", "{cite}"),
    )
    .unwrap();
    fs::write(
        temp.path().join("src/main.rs"),
        "/// user docs\n#[allow(dead_code)]\n// IMPL`rules.ready`\nfn main() { fn nested() {} }\n"
            .replace("IMPL", "{impl}"),
    )
    .unwrap();
    let output = habeas(
        temp.path(),
        &[
            "add-input",
            "--for-requirement",
            "rules.ready",
            "--file-range",
            "src/main.rs:1-2",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let output = habeas(
        temp.path(),
        &[
            "add-input",
            "--for-requirement",
            "rules.ready",
            "--rust-item",
            "src/main.rs:4",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("fn[main]"), "{stdout}");
    assert!(stdout.contains("fn[main]/fn[nested]"), "{stdout}");
    let requirement_path = temp.path().join("evidence/rules.ready.json");
    let first = fs::read(&requirement_path).unwrap();
    let output = habeas(
        temp.path(),
        &[
            "add-input",
            "--for-requirement",
            "rules.ready",
            "--rust-item",
            "src/main.rs:4",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("unchanged"));
    assert_eq!(fs::read(&requirement_path).unwrap(), first);
    let requirement: serde_json::Value = serde_json::from_slice(&first).unwrap();
    assert_eq!(requirement["inputs"].as_array().unwrap().len(), 3);
    assert_eq!(
        requirement["inputs"][1]["text"],
        json!([
            "/// user docs",
            "#[allow(dead_code)]",
            "// IMPL`rules.ready`".replace("IMPL", "{impl}"),
            "fn main() { fn nested() {} }"
        ])
    );
    let output = habeas(
        temp.path(),
        &[
            "add-input",
            "--for-example",
            "rules.case",
            "--requirements",
            "rules.*",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let example: serde_json::Value =
        serde_json::from_slice(&fs::read(temp.path().join("evidence/rules.case.json")).unwrap())
            .unwrap();
    assert_eq!(
        example["example"],
        "Given ready, it works. CITE`rules.ready`".replace("CITE", "{cite}")
    );
    assert_eq!(
        example["inputs"][0]["matches"],
        json!(["rules.case", "rules.ready"])
    );
    let output = habeas(
        temp.path(),
        &[
            "add-input",
            "--for-requirement",
            "rules.ready",
            "--requirements",
            "rules.*",
        ],
    );
    assert!(output.status.success(), "{output:?}");

    let output = habeas(
        temp.path(),
        &[
            "add-input",
            "--for-rust-item",
            "src/main.rs",
            "fn[main]",
            "--requirements",
            "rules.*",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("rules.case, rules.ready"), "{stdout}");
    assert!(stdout.contains("remains pending"), "{stdout}");
    let output = habeas(
        temp.path(),
        &[
            "add-input",
            "--for-rust-item",
            "src/main.rs",
            "fn[main]",
            "--rust-item",
            "src/main.rs:4",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let attestation: serde_json::Value = serde_json::from_slice(
        &fs::read(temp.path().join("evidence/rust/src/main.rs.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(attestation["functions"][0]["determination"], false);
    assert_eq!(
        attestation["functions"][0]["citations"],
        json!([{"role": "impl", "name": "rules.ready"}])
    );
    assert_eq!(
        attestation["functions"][0]["inputs"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        attestation["functions"][0]["inputs"][0]["matches"],
        json!(["rules.case", "rules.ready"])
    );
    let detail = habeas(temp.path(), &["info", "rules.ready"]);
    assert!(detail.status.success(), "{detail:?}");
    let detail = String::from_utf8(detail.stdout).unwrap();
    assert!(
        detail.contains("Rust item src/main.rs fn[main]"),
        "{detail}"
    );
    assert!(detail.contains("src/main.rs:1–4"), "{detail}");
    assert!(detail.contains("requirements glob rules.*"), "{detail}");
    assert!(detail.contains("md/rules.md:5–7"), "{detail}");

    let invalid = habeas(
        temp.path(),
        &[
            "add-input",
            "--for-requirement",
            "rules.ready",
            "--requirements",
            "missing.*",
        ],
    );
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("matches no requirement or example"));
}

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
    fs::write(temp.path().join("md/requirements.md"), source).unwrap();
    fs::write(
        temp.path().join("src/main.rs"),
        // The comment lies between two functions, so it belongs to the next
        // one and `main` stays uncited.
        [
            "fn main() {}\n// {",
            "impl}`requirements.uncovered`\nfn after() {}\n",
        ]
        .concat(),
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
                habeas::outline_input(temp.path(), "src/main.rs", "fn[main]").unwrap(),
                habeas::outline_inputs_for_lines(
                    temp.path(),
                    "md/requirements.md",
                    case_line,
                    case_line,
                )
                .unwrap()
                .remove(0),
            ]
        } else if name == "stale_input" {
            vec![json!({
                "file": "src/main.rs",
                "item": "fn[main]",
                "hash": "0".repeat(64)
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
// {test}`requirements.cli.fixme_summary`
// {test}`requirements.cli.info`
fn habeas(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_habeas"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

// {test}`requirements.cli.fixme_summary`
// {test}`requirements.ci.outcomes`
// {test}`requirements.ci.determination`
// {test}`requirements.ci.schema`
// {test}`requirements.integration.report_api`
fn fixme_fixture() -> TempDir {
    let temp = TempDir::new().unwrap();
    fs::create_dir(temp.path().join("md")).unwrap();
    fs::create_dir(temp.path().join("evidence")).unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname = 'fixture'\n",
    )
    .unwrap();
    fs::write(temp.path().join("book.toml"), "[book]\nsrc = 'md'\n").unwrap();
    let example = "A concrete case. CITE`rules.alpha`.".replace("CITE", "{cite}");
    let declarations = [
        (
            "req",
            "alpha",
            "Alpha MUST work.",
            vec![
                "FIXME(#00123)",
                "Repeated #123 and rust-lang/rust#000456.",
                "https://github.com/rust-lang/rust/issues/456",
                "Unlinked extra detail",
            ],
        ),
        (
            "req",
            "beta",
            "Beta MUST work.",
            vec!["#123", "#45", "Rust-Lang/rust#456", "An unrelated note"],
        ),
        ("example", "case", &example, vec!["FIXME(#45)"]),
        (
            "req",
            "unlinked",
            "Unlinked MUST work.",
            vec![
                "No ticket for <script>alert(1)</script>",
                "#000 is not an issue",
                "#12abc and ##123 are not complete issue references",
            ],
        ),
    ];
    let source: String = declarations
        .iter()
        .map(|(kind, name, text, _)| format!(":::{{{kind}}} {name}\n{text}\n:::\n\n"))
        .collect();
    fs::write(temp.path().join("md/rules.md"), &source).unwrap();
    let input = habeas::outline_input(temp.path(), "md/rules.md", "").unwrap();
    for (kind, name, text, references) in declarations {
        let subject_key = if kind == "example" {
            "example"
        } else {
            "requirement"
        };
        fs::write(
            temp.path().join(format!("evidence/rules.{name}.json")),
            json!({
                subject_key: text,
                "determination": {"FIXME": references},
                "rationale": "The fixture records acknowledged implementation gaps.",
                "inputs": [input]
            })
            .to_string(),
        )
        .unwrap();
    }
    let report = habeas::validate_repository(temp.path()).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    temp
}

// {test}`requirements.configuration.workspace_location`
// {test}`requirements.configuration.evidence_paths`
// {test}`requirements.configuration.workspace_structure`
// {test}`requirements.configuration.loading`
// {test}`requirements.authoring.deterministic_validation`
// {test}`requirements.cli.status`
#[test]
fn status_loads_nearest_workspace_file_without_validating_authoring() {
    let temp = fixture();
    let nested = temp.path().join("src/nested");
    fs::write(temp.path().join("habeas.toml"), "version = 99\n").unwrap();
    fs::write(
        nested.join("habeas.toml"),
        "version = 1\n[authoring]\nconfigs = 'invalid but unused'\n",
    )
    .unwrap();
    let output = habeas(&nested, &["status"]);
    assert!(output.status.success(), "{output:?}");

    let output = habeas(temp.path(), &["status"]);
    assert!(!output.status.success(), "{output:?}");
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("habeas.toml") && error.contains("version"),
        "{error}"
    );

    fs::write(temp.path().join("habeas.toml"), "version = 1\n").unwrap();
    fs::write(nested.join("habeas.toml"), "version = [\n").unwrap();
    let output = habeas(&nested, &["status"]);
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains(&nested.join("habeas.toml").display().to_string()),
        "{output:?}"
    );
}

// {test}`requirements.configuration.evidence_paths`
// {test}`requirements.ci.input_containment`
// {test}`requirements.cli.status`
// {test}`requirements.authoring.deterministic_validation`
#[test]
fn status_applies_workspace_disallow_to_existing_evidence() {
    let temp = fixture();
    fs::write(
        temp.path().join("habeas.toml"),
        "version = 1\n[evidence]\nallow = ['src']\ndisallow = ['src/main.rs']\n",
    )
    .unwrap();
    let output = habeas(temp.path(), &["status", "requirements.ready"]);
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("Invalid") && text.contains("disallowed"),
        "{text}"
    );
    assert!(text.contains("src/main.rs"), "{text}");
}

// {test}`requirements.cli.status`
// {test}`requirements.cli.fixme_summary`
#[test]
fn status_groups_fixme_judgments_by_distinct_normalized_issue() {
    let temp = fixme_fixture();
    let output = habeas(temp.path(), &["status"]);
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    let summaries: Vec<_> = text
        .lines()
        .filter(|line| line.contains("affects"))
        .collect();
    assert_eq!(summaries.len(), 4, "{text}");
    for (line, reference, count) in [
        (summaries[0], "#123", "affects 2 judgments"),
        (summaries[1], "#45", "affects 2 judgments"),
        (summaries[2], "Rust-Lang/rust#456", "affects 1 judgment"),
        (summaries[3], "rust-lang/rust#456", "affects 1 judgment"),
    ] {
        assert!(line.contains(reference) && line.contains(count), "{text}");
    }
    assert!(
        text.lines()
            .any(|line| line.contains("1 judgment") && line.contains("without an issue")),
        "{text}"
    );
    assert!(!text.contains("#00123"), "{text}");
    assert!(!text.contains("#000456"), "{text}");
    assert!(
        text.find("without an issue").unwrap() > text.find(summaries[3]).unwrap(),
        "{text}"
    );
    for name in ["alpha", "beta", "case", "unlinked"] {
        assert!(!text.contains(&format!("rules.{name}")), "{text}");
    }

    let output = habeas(temp.path(), &["status", "--all"]);
    assert!(output.status.success(), "{output:?}");
    let all = String::from_utf8(output.stdout).unwrap();
    for name in ["alpha", "beta", "case", "unlinked"] {
        assert!(
            all.lines().any(
                |line| line.starts_with(&format!("| `rules.{name}`")) && line.contains("FIXME")
            ),
            "{all}"
        );
    }
    for summary in summaries {
        assert!(all.contains(summary), "{all}");
    }
}

// {test}`requirements.cli.status`
// {test}`requirements.cli.fixme_summary`
// {test}`requirements.references.definition_globs`
#[test]
fn verbose_status_lists_fixme_judgments_and_globs_scope_issue_groups() {
    let temp = fixme_fixture();
    let output = habeas(temp.path(), &["status", "--verbose"]);
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    for name in ["alpha", "beta", "case", "unlinked"] {
        assert!(text.contains(&format!("rules.{name}")), "{text}");
    }
    for detail in [
        "requirement",
        "example",
        "FIXME(#00123)",
        "Repeated #123 and rust-lang/rust#000456.",
        "https://github.com/rust-lang/rust/issues/456",
        "Unlinked extra detail",
        "An unrelated note",
        "&lt;script&gt;alert(1)&lt;/script&gt;",
    ] {
        assert!(text.contains(detail), "{text}");
    }
    assert!(!text.contains("<script>"), "{text}");

    let output = habeas(temp.path(), &["status", "rules.case", "--verbose"]);
    assert!(output.status.success(), "{output:?}");
    let filtered = String::from_utf8(output.stdout).unwrap();
    assert!(
        filtered.contains("#45") && filtered.contains("affects 1 judgment"),
        "{filtered}"
    );
    assert!(filtered.contains("rules.case"), "{filtered}");
    for excluded in [
        "rules.alpha",
        "rules.beta",
        "rules.unlinked",
        "#123",
        "rust-lang/rust",
    ] {
        assert!(!filtered.contains(excluded), "{filtered}");
    }

    let output = habeas(temp.path(), &["status", "--verbose", "rules.unlinked"]);
    assert!(output.status.success(), "{output:?}");
    let unlinked = String::from_utf8(output.stdout).unwrap();
    assert!(unlinked.contains("rules.unlinked"), "{unlinked}");
    assert!(
        unlinked
            .lines()
            .any(|line| line.contains("1 judgment") && line.contains("without an issue")),
        "{unlinked}"
    );
    assert!(!unlinked.contains("affects"), "{unlinked}");
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
    assert!(text.contains("outline input changed"), "{text}");
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

// {test}`requirements.cli.status`
// {test}`requirements.references.rust_item_coverage`
// {test}`requirements.references.definition_globs`
#[test]
fn status_preserves_repository_wide_rust_findings_in_every_form() {
    let temp = fixture();
    let nested = temp.path().join("src/nested");
    for flags in [
        [].as_slice(),
        ["--all"].as_slice(),
        ["--verbose"].as_slice(),
        ["--all", "--verbose"].as_slice(),
    ] {
        for selection in [None, Some("requirements.ready")] {
            let mut args = vec!["status"];
            args.extend_from_slice(flags);
            args.extend(selection);
            let output = habeas(&nested, &args);
            assert!(output.status.success(), "{args:?}: {output:?}");
            let text = String::from_utf8(output.stdout).unwrap();
            assert!(
                text.contains("## Other validation findings"),
                "{args:?}: {text}"
            );
            assert!(
                text.contains(
                    "src/main.rs:1: function main has no requirement or example citation"
                ),
                "{args:?}: {text}"
            );
            assert!(!text.contains("attestation"), "{args:?}: {text}");
        }
    }
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
    for command in ["info", "status"] {
        let invalid = habeas(temp.path(), &[command, "["]);
        assert!(!invalid.status.success());
        assert!(
            String::from_utf8_lossy(&invalid.stderr).contains("invalid definition glob"),
            "{command}: {invalid:?}"
        );
    }
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

    let outside_project = TempDir::new().unwrap();
    let status = habeas(outside_project.path(), &["status"]);
    assert!(!status.status.success(), "{status:?}");
    assert!(
        String::from_utf8_lossy(&status.stderr)
            .contains("cannot find a project with Cargo.toml and book.toml"),
        "{status:?}"
    );
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
// {test}`requirements.ci.input_containment`
// {test}`requirements.cli.add_input_item`
// {test}`requirements.configuration.evidence_paths`
// {test}`requirements.ci.schema`
#[test]
fn add_input_external_sources_respect_allow_and_disallow() {
    let temp = fixture();
    let external = TempDir::new().unwrap();
    let source = external.path().join("source.rs");
    fs::write(&source, "pub fn external() {}\n").unwrap();
    let source = fs::canonicalize(source).unwrap();
    let allowed = toml::Value::String(external.path().display().to_string());
    let config = format!("version = 1\n[evidence]\nallow = [{allowed}]\n");
    fs::write(temp.path().join("habeas.toml"), &config).unwrap();
    let evidence = temp.path().join("evidence/requirements.absent.json");
    let location = format!("{}:1", source.display());
    let output = habeas(
        temp.path(),
        &[
            "add-input",
            "--for-requirement",
            "requirements.absent",
            "--item",
            &location,
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let contents = fs::read(&evidence).unwrap();
    let record: serde_json::Value = serde_json::from_slice(&contents).unwrap();
    assert_eq!(
        record["inputs"],
        json!([
            habeas::outline_input(temp.path(), source.to_str().unwrap(), "fn[external]").unwrap()
        ])
    );
    assert_eq!(record["inputs"][0]["file"], json!(source));

    fs::write(
        temp.path().join("habeas.toml"),
        format!("{config}disallow = [{allowed}]\n"),
    )
    .unwrap();
    {
        let output = habeas(
            temp.path(),
            &[
                "add-input",
                "--for-requirement",
                "requirements.absent",
                "--item",
                &location,
            ],
        );
        assert!(!output.status.success(), "{output:?}");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("disallowed"), "{error}");
        assert!(!error.contains("panicked"), "{error}");
        assert_eq!(fs::read(&evidence).unwrap(), contents);
    }
}

// {test}`requirements.cli.status`
// {test}`requirements.cli.info`
// {test}`requirements.cli.definition_glob_search`
#[test]
fn status_and_info_accept_a_definition_selection() {
    let temp = fixture();
    for (command, usage) in [("status", "[SELECTION]"), ("info", "<SELECTION>")] {
        let help = habeas(temp.path(), &[command, "--help"]);
        assert!(help.status.success(), "{help:?}");
        let help = String::from_utf8(help.stdout).unwrap();
        assert!(help.contains(usage), "{help}");
        assert!(
            help.contains("A full requirement or example name, or a definition glob"),
            "{help}"
        );
    }
    let status = habeas(temp.path(), &["status", "requirements.c*", "--all"]);
    assert!(status.status.success(), "{status:?}");
    let status = String::from_utf8(status.stdout).unwrap();
    assert!(
        status.contains("| `requirements.case` (example)"),
        "{status}"
    );
    assert!(!status.contains("| `requirements.ready`"), "{status}");
    let info = habeas(temp.path(), &["info", "requirements.case"]);
    assert!(info.status.success(), "{info:?}");
    let info = String::from_utf8(info.stdout).unwrap();
    assert!(info.starts_with("# `requirements.case`\n"), "{info}");
    assert!(!info.contains("# `requirements.ready`"), "{info}");
}

// {test}`requirements.cli.add_input`
#[test]
fn add_input_rejects_function_targets_without_creating_evidence() {
    let temp = fixture();
    fs::write(
        temp.path().join("src/lib.rs"),
        "// IMPL`requirements.ready`\npub fn run() {}\n".replace("IMPL", "{impl}"),
    )
    .unwrap();
    let before = fs::read_dir(temp.path().join("evidence"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<std::collections::BTreeSet<_>>();
    let output = habeas(
        temp.path(),
        &[
            "add-input",
            "--for-rust-item",
            "src/lib.rs",
            "fn[run]",
            "--requirements",
            "requirements.ready",
        ],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("--for-rust-item"), "{error}");
    assert!(!error.contains("panicked"), "{error}");
    let after = fs::read_dir(temp.path().join("evidence"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(before, after);
    assert!(!temp.path().join("evidence/rust").exists());

    let help = habeas(temp.path(), &["add-input", "--help"]);
    assert!(help.status.success(), "{help:?}");
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(!help.contains("--for-rust-item"), "{help}");
    assert!(help.contains("--for-requirement"), "{help}");
    assert!(help.contains("--for-example"), "{help}");
    assert!(help.contains("--requirements <selection>"), "{help}");
}

// {test}`requirements.cli.add_input`
// {test}`requirements.cli.info`
// {test}`requirements.cli.add_input_item`
// {test}`requirements.cli.add_input_requirements`
// {test}`requirements.references.selector_same_line`
// {test}`requirements.references.selector_raw_lines`
// {test}`requirements.references.structural_hash`
#[test]
fn add_input_modes_capture_outline_nodes_and_refresh_idempotently() {
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
    let main =
        "/// user docs\n#[allow(dead_code)]\n// IMPL`rules.ready`\nfn main() { fn nested() {} }\n"
            .replace("IMPL", "{impl}");
    fs::write(temp.path().join("src/main.rs"), &main).unwrap();
    fs::write(temp.path().join("notes.txt"), "alpha\n\nbeta\n").unwrap();
    let add = |location: &str| {
        habeas(
            temp.path(),
            &[
                "add-input",
                "--for-requirement",
                "rules.ready",
                "--item",
                location,
            ],
        )
    };
    let output = add("src/main.rs:4");
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("fn[main]"), "{stdout}");
    assert!(stdout.contains("fn[main]/fn[nested]"), "{stdout}");
    // A plain-text file's lines are outline nodes too.
    let output = add("notes.txt:3");
    assert!(output.status.success(), "{output:?}");
    let requirement_path = temp.path().join("evidence/rules.ready.json");
    let first = fs::read(&requirement_path).unwrap();
    let output = add("src/main.rs:4");
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("unchanged"));
    assert_eq!(fs::read(&requirement_path).unwrap(), first);
    let requirement: serde_json::Value = serde_json::from_slice(&first).unwrap();
    let outline = |file: &str, item: &str| habeas::outline_input(temp.path(), file, item).unwrap();
    let beta = habeas::outline_inputs_for_lines(temp.path(), "notes.txt", 3, 3)
        .unwrap()
        .remove(0);
    assert!(beta["item"].as_str().unwrap().starts_with("line@"));
    assert_eq!(
        requirement["inputs"],
        json!([
            outline("src/main.rs", "fn[main]"),
            outline("src/main.rs", "fn[main]/fn[nested]"),
            beta,
        ])
    );
    for location in [
        "src/main.rs:3",
        "notes.txt:2",
        "src/main.rs:x",
        "missing.rs:1",
    ] {
        let output = add(location);
        assert!(!output.status.success(), "{location}: {output:?}");
        assert_eq!(fs::read(&requirement_path).unwrap(), first);
    }
    assert!(
        String::from_utf8_lossy(&add("src/main.rs:3").stderr)
            .contains("src/main.rs:3: no outline node is declared on this line")
    );
    // The citation comment is not hashed; the doc comment is.
    let recorded = requirement["inputs"][0]["hash"].clone();
    fs::write(
        temp.path().join("src/main.rs"),
        main.replace("`rules.ready`", "`rules.ready` (edited)"),
    )
    .unwrap();
    assert_eq!(outline("src/main.rs", "fn[main]")["hash"], recorded);
    fs::write(
        temp.path().join("src/main.rs"),
        main.replace("user docs", "changed docs"),
    )
    .unwrap();
    assert_ne!(outline("src/main.rs", "fn[main]")["hash"], recorded);
    fs::write(temp.path().join("src/main.rs"), &main).unwrap();
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
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("rules.case, rules.ready"), "{stdout}");
    let requirement: serde_json::Value =
        serde_json::from_slice(&fs::read(&requirement_path).unwrap()).unwrap();
    assert_eq!(requirement["determination"], false);
    assert!(
        requirement["rationale"]
            .as_str()
            .unwrap()
            .contains("Review pending"),
        "{requirement}"
    );
    assert!(!temp.path().join("evidence/rust").exists());
    let detail = habeas(temp.path(), &["info", "rules.ready"]);
    assert!(detail.status.success(), "{detail:?}");
    let detail = String::from_utf8(detail.stdout).unwrap();
    assert!(detail.contains("outline src/main.rs fn[main]"), "{detail}");
    assert!(detail.contains("- Kind: <code>fn</code>"), "{detail}");
    assert!(detail.contains("src/main.rs:1–4"), "{detail}");
    assert!(detail.contains("fn main() { fn nested() {} }"), "{detail}");
    assert!(
        detail.contains("requirements selection rules.*"),
        "{detail}"
    );
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

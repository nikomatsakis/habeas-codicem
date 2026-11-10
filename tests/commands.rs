use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::json;
use tempfile::TempDir;

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
        // No outline node follows the comment, so it belongs to the root and
        // `main` stays uncited.
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
                "references": habeas::reference_set_digest_for(
                    temp.path(),
                    &format!("requirements.{name}")
                )
                .unwrap(),
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

fn habeas(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_habeas"))
        .current_dir(dir)
        // No test contacts GitHub: nothing listens on this port, so issues
        // are unreachable and count as open.
        .env("HABEAS_GITHUB_API_URL", "http://127.0.0.1:9")
        .args(args)
        .output()
        .unwrap()
}

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
                "example/app#123",
                "rust-lang/rust#456",
                "Unlinked extra detail",
            ],
        ),
        (
            "req",
            "beta",
            "Beta MUST work.",
            vec![
                "example/app#123",
                "example/app#45",
                "Rust-Lang/rust#456",
                "An unrelated note",
            ],
        ),
        ("example", "case", &example, vec!["example/app#45"]),
        (
            "req",
            "unlinked",
            "Unlinked MUST work.",
            vec![
                "No ticket for <script>alert(1)</script>",
                "#123 is not a qualified issue",
                "https://github.com/rust-lang/rust/issues/456",
            ],
        ),
    ];
    let source: String = declarations
        .iter()
        .map(|(kind, name, text, _)| format!(":::{{{kind}}} {name}\n{text}\n:::\n\n"))
        .collect();
    fs::write(temp.path().join("md/rules.md"), &source).unwrap();
    // Each judgment's strings come from the `{fixme}` references to it: one
    // naming its issues, and one per gap without an issue.
    let mut gaps = String::new();
    for (_, name, _, entries) in &declarations {
        let (issues, others): (Vec<&str>, Vec<&str>) = entries
            .iter()
            .partition(|entry| entry.contains('/') && !entry.contains(' ') && !entry.contains(':'));
        if !issues.is_empty() {
            gaps.push_str(&format!(":::{{fixme}} rules.{name}\n"));
            for issue in issues {
                gaps.push_str(&format!(":github: {issue}\n"));
            }
            gaps.push_str("Tracked gaps.\n:::\n\n");
        }
        for other in others {
            gaps.push_str(&format!(":::{{fixme}} rules.{name}\n{other}\n:::\n\n"));
        }
    }
    fs::write(temp.path().join("gaps.txt"), gaps).unwrap();
    let input = habeas::outline_input(temp.path(), "md/rules.md", "").unwrap();
    let gaps_input = habeas::outline_input(temp.path(), "gaps.txt", "").unwrap();
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
                "references": habeas::reference_set_digest_for(
                    temp.path(),
                    &format!("rules.{name}")
                )
                .unwrap(),
                "inputs": [input, gaps_input]
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
// {test}`requirements.ci.errors`
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

    // An invalid workspace file is one validation finding: `status` and
    // `info` still run and report the checks that do not depend on it.
    for args in [["status"].as_slice(), ["info", "requirements.*"].as_slice()] {
        let output = habeas(temp.path(), args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(
            text.contains("habeas.toml: version: expected version = 1"),
            "{args:?}: {text}"
        );
        assert!(
            text.contains("evidence/requirements.malformed.json: invalid JSON"),
            "{args:?}: {text}"
        );
    }

    fs::write(temp.path().join("habeas.toml"), "version = 1\n").unwrap();
    fs::write(nested.join("habeas.toml"), "version = [\n").unwrap();
    let output = habeas(&nested, &["status"]);
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains(&nested.join("habeas.toml").display().to_string())
            && text.contains("evidence/requirements.malformed.json: invalid JSON"),
        "{text}"
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
fn status_groups_fixme_judgments_by_distinct_exact_issue() {
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
        (summaries[0], "example/app#123", "affects 2 judgments"),
        (summaries[1], "example/app#45", "affects 2 judgments"),
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
    // Strings that are not exactly `owner/repo#N` name no issue.
    assert!(!text.contains("`#123`"), "{text}");
    assert!(!text.contains("github.com"), "{text}");
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
        "#123 is not a qualified issue",
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
        filtered.contains("example/app#45") && filtered.contains("affects 1 judgment"),
        "{filtered}"
    );
    assert!(filtered.contains("rules.case"), "{filtered}");
    for excluded in [
        "rules.alpha",
        "rules.beta",
        "rules.unlinked",
        "example/app#123",
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
    // `main` is read by an evidence record; this item is read by none.
    fs::write(temp.path().join("src/extra.rs"), "fn unread() {}\n").unwrap();
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
                    "src/extra.rs:1: function unread is uncovered: it has no requirement or example reference, and no evidence record or review read it"
                ),
                "{args:?}: {text}"
            );
            assert!(
                !text.contains("function main is uncovered"),
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
// {test}`requirements.cli.shared_status_rendering`
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
    // The labels are the evidence-state labels the mdBook index also shows.
    let output = habeas(temp.path(), &["status", "--all"]);
    let text = String::from_utf8(output.stdout).unwrap();
    for (name, state) in [
        ("ready", "Valid"),
        ("absent", "Missing"),
        ("pending", "Determination false"),
        ("malformed", "Invalid"),
    ] {
        let row = text
            .lines()
            .find(|line| line.starts_with(&format!("| `requirements.{name}` ")))
            .unwrap_or_else(|| panic!("missing {name} in:\n{text}"));
        assert_eq!(row.split('|').nth(2).unwrap().trim(), state, "{text}");
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

// {test}`requirements.cli.definition_glob_search`
#[test]
fn definition_glob_lists_a_requirement_and_an_example_in_info_and_status() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("md")).unwrap();
    fs::create_dir_all(root.join("evidence")).unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\nname = \"fixture\"\n").unwrap();
    fs::write(root.join("book.toml"), "[book]\nsrc = \"md\"\n").unwrap();
    fs::write(
        root.join("md/foo.md"),
        ":::{req} ready\nReady MUST work.\n:::\n\n:::{example} case\nGiven ready, it works.\n:::\n",
    )
    .unwrap();
    fs::write(
        root.join("md/bar.md"),
        ":::{req} other\nOther MUST work.\n:::\n",
    )
    .unwrap();
    fs::write(
        root.join("evidence/foo.ready.json"),
        json!({
            "requirement": "Ready MUST work.",
            "determination": false,
            "rationale": "Not yet.",
            "references": habeas::reference_set_digest_for(root, "foo.ready").unwrap(),
            "inputs": []
        })
        .to_string(),
    )
    .unwrap();

    let info = habeas(root, &["info", "foo.*"]);
    assert!(info.status.success(), "{info:?}");
    let info = String::from_utf8(info.stdout).unwrap();
    assert!(info.starts_with("# `foo.case`\n"), "{info}");
    assert!(
        info.contains("## Example\n\n> Given ready, it works."),
        "{info}"
    );
    assert!(info.contains("# `foo.ready`\n"), "{info}");
    assert!(
        info.contains("## Requirement\n\n> Ready MUST work."),
        "{info}"
    );
    assert!(!info.contains("bar.other"), "{info}");

    let status = habeas(root, &["status", "foo.*", "--all"]);
    assert!(status.status.success(), "{status:?}");
    let status = String::from_utf8(status.stdout).unwrap();
    let row = |name: &str| {
        status
            .lines()
            .find(|line| line.starts_with(&format!("| `{name}`")))
            .unwrap_or_else(|| panic!("{name} missing: {status}"))
            .to_owned()
    };
    let case = row("foo.case");
    assert!(
        case.contains("(example)") && case.contains("| Missing"),
        "{case}"
    );
    let ready = row("foo.ready");
    assert!(
        ready.contains("(requirement)") && ready.contains("| Determination false"),
        "{ready}"
    );
    assert!(!status.contains("`bar.other`"), "{status}");
}

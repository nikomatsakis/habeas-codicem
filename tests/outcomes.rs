use std::fs;
use std::path::Path;

use habeas::judgment::Judgment;
use mdbook_preprocessor::book::{Book, BookItem, Chapter};
use serde_json::{Value, json};
use tempfile::TempDir;

/// A role written so this file's own source does not contain it.
fn role(kind: &str) -> String {
    format!("{{{kind}}}")
}

/// A fenced `{fixme}` in Rust comments: an opening fence naming `name`,
/// option lines for `issues`, the `body`, and a closing fence.
fn fixme(name: &str, issues: &[&str], body: &str) -> String {
    let mut text = format!("// :::{} {name}\n", role("fixme"));
    for issue in issues {
        text.push_str(&format!("// :github: {issue}\n"));
    }
    text.push_str(&format!("// {body}\n// :::\n"));
    text
}

/// The example `requirements.timeout`, judged FIXME and acknowledged by a
/// `{fixme}` in `observed.rs` naming two issues.
// {test}`requirements.examples.evidence`
fn fixture() -> TempDir {
    let temp = TempDir::new().unwrap();
    fs::create_dir(temp.path().join("md")).unwrap();
    fs::create_dir(temp.path().join("evidence")).unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname = 'fixture'\n",
    )
    .unwrap();
    fs::write(temp.path().join("book.toml"), "[book]\nsrc = 'md'\n").unwrap();
    let example = "Timeout handling retries. {cite}`requirements.ready`".to_owned();
    let example_lines = vec![
        ":::{example} timeout".to_owned(),
        example.clone(),
        ":::".to_owned(),
    ];
    fs::write(
        temp.path().join("md/requirements.md"),
        format!(
            ":::{{req}} ready\nThe behavior MUST work.\n:::\n\n{}\n",
            example_lines.join("\n")
        ),
    )
    .unwrap();
    let source = fixme(
        "requirements.timeout",
        &["example/io#125", "rust-lang/rust#123"],
        "Retry-after-timeout and partial-write recovery are untested.",
    );
    fs::write(
        temp.path().join("observed.rs"),
        format!("{source}const RETRIES: u32 = 1;\n"),
    )
    .unwrap();
    let source_input = habeas::outline_input(temp.path(), "observed.rs", "").unwrap();
    write_record(
        temp.path(),
        "ready",
        json!({
            "requirement": "The behavior MUST work.",
            "determination": true,
            "rationale": "The requirement is supported by the fixture.",
            "inputs": [{
                "requirements": "requirements.timeout",
                "matches": ["requirements.timeout"],
                "text": example_lines
            }]
        }),
    );
    write_record(
        temp.path(),
        "timeout",
        json!({
            "example": example,
            "determination": { "FIXME": ["example/io#125", "rust-lang/rust#123"] },
            "rationale": "The recorded comments acknowledge the missing timeout tests.",
            "inputs": [source_input]
        }),
    );
    temp
}

fn write_record(root: &Path, name: &str, mut value: Value) {
    if value.get("references").is_none() {
        value["references"] =
            json!(habeas::reference_set_digest_for(root, &format!("requirements.{name}")).unwrap());
    }
    fs::write(
        root.join(format!("evidence/requirements.{name}.json")),
        serde_json::to_string_pretty(&value).unwrap(),
    )
    .unwrap();
}

fn replace_judgment(root: &Path, name: &str, judgment: Value) {
    let path = root.join(format!("evidence/requirements.{name}.json"));
    let mut record: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    record["determination"] = judgment;
    write_record(root, name, record);
}

// {test}`requirements.ci.outcomes`
// {test}`requirements.ci.schema`
#[test]
fn judgments_round_trip_and_reject_invalid_fixme_shapes() {
    for value in [
        json!(true),
        json!(false),
        json!({ "FIXME": ["#125", "rust-lang/rust#123"] }),
        json!({ "FIXME": ["cancellation races are not tested on Windows"] }),
    ] {
        assert_eq!(Judgment::from_value(&value).unwrap().to_value(), value);
    }
    for invalid in [
        Value::Null,
        json!("FIXME"),
        json!(["#125"]),
        json!({}),
        json!({ "FIXME": [] }),
        json!({ "FIXME": [""] }),
        json!({ "FIXME": ["   "] }),
        json!({ "FIXME": ["#125", 125] }),
        json!({ "FIXME": "#125" }),
        json!({ "FIXME": ["#125"], "extra": true }),
        json!({ "fixme": ["#125"] }),
    ] {
        assert!(Judgment::from_value(&invalid).is_err(), "{invalid}");
    }
}

// {test}`requirements.ci.determination`
// {test}`requirements.examples.evidence`
// {test}`requirements.ci.fixme_example_test_gaps`
#[test]
fn fixme_examples_pass_without_test_references_but_true_examples_do_not() {
    let temp = fixture();
    let report = habeas::validate_repository(temp.path()).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    replace_judgment(temp.path(), "timeout", json!(true));
    let report = habeas::validate_repository(temp.path()).unwrap();
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.contains("no {test} reference"))
    );
    replace_judgment(temp.path(), "timeout", json!(false));
    let report = habeas::validate_repository(temp.path()).unwrap();
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.contains("determination is false"))
    );
}

/// Writes `source` to `reviewed.rs` and records `judgment` for
/// `requirements.ready` with that file among its inputs, returning the
/// validation errors.
fn judge_ready(root: &Path, source: &str, judgment: Value) -> Vec<String> {
    fs::write(root.join("reviewed.rs"), source).unwrap();
    let path = root.join("evidence/requirements.ready.json");
    let mut record: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    record["determination"] = judgment;
    record["rationale"] = json!("The fixture's gaps and their locations.");
    // The new source changes the reference set; record its current digest.
    record.as_object_mut().unwrap().remove("references");
    let inputs = record["inputs"].as_array_mut().unwrap();
    inputs.retain(|input| input.get("file").and_then(Value::as_str) != Some("reviewed.rs"));
    inputs.push(habeas::outline_input(root, "reviewed.rs", "").unwrap());
    write_record(root, "ready", record);
    habeas::validate_repository(root).unwrap().errors
}

// {test}`requirements.ci.determination`
// {test}`requirements.ci.outcomes`
// {test}`requirements.ci.fixme_implementation_gap`
#[test]
fn an_acknowledged_implementation_gap_records_its_issue() {
    let temp = fixture();
    let source = fixme(
        "requirements.ready",
        &["example/queue#125"],
        "Capacity-zero insertion does not return Full yet.",
    );
    let errors = judge_ready(
        temp.path(),
        &source,
        json!({ "FIXME": ["example/queue#125"] }),
    );
    assert!(errors.is_empty(), "{errors:?}");
    // The strings must be the issues as written after `:github:`.
    for judgment in [json!(["#125"]), json!(["FIXME(example/queue#125)"])] {
        let errors = judge_ready(temp.path(), &source, json!({ "FIXME": judgment }));
        assert!(
            errors
                .iter()
                .any(|error| error.contains("is neither an issue named by a {fixme}")),
            "{errors:?}"
        );
    }
    // A plain FIXME comment acknowledges nothing.
    let errors = judge_ready(
        temp.path(),
        "// FIXME(example/queue#125): capacity-zero insertion does not return Full yet.\n",
        json!({ "FIXME": ["example/queue#125"] }),
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("\"example/queue#125\" is neither an issue")),
        "{errors:?}"
    );
}

// {test}`requirements.ci.determination`
// {test}`requirements.ci.outcomes`
// {test}`requirements.ci.fixme_without_issue`
#[test]
fn an_acknowledged_gap_without_an_issue_records_the_body_words() {
    let temp = fixture();
    let source = fixme(
        "requirements.ready",
        &[],
        "Cancellation races are not\n//   tested on Windows.",
    );
    let errors = judge_ready(
        temp.path(),
        &source,
        json!({ "FIXME": ["Cancellation races are not tested on Windows."] }),
    );
    assert!(errors.is_empty(), "{errors:?}");
    // An invented issue number, or the words not joined by single spaces,
    // matches no `{fixme}`.
    for entry in [
        "example/tasks#1",
        "Cancellation races are not  tested on Windows.",
    ] {
        let errors = judge_ready(temp.path(), &source, json!({ "FIXME": [entry] }));
        assert!(
            errors
                .iter()
                .any(|error| error.contains("is neither an issue")),
            "{entry}: {errors:?}"
        );
    }
}

// {test}`requirements.ci.outcomes`
// {test}`requirements.ci.fixme_example_test_gaps`
#[test]
fn fixme_strings_list_each_issue_of_an_acknowledging_fixme_once() {
    let temp = fixture();
    let path = "evidence/requirements.timeout.json";
    for (judgment, expected) in [
        (
            json!(["rust-lang/rust#123"]),
            "FIXME entries omit \"example/io#125\" named by the acknowledging {fixme} to requirements.timeout at observed.rs:1",
        ),
        (
            json!(["example/io#125", "rust-lang/rust#123", "example/io#125"]),
            "FIXME entry \"example/io#125\" is listed more than once",
        ),
    ] {
        replace_judgment(temp.path(), "timeout", json!({ "FIXME": judgment }));
        let errors = habeas::validate_repository(temp.path()).unwrap().errors;
        assert!(
            errors
                .iter()
                .any(|error| error == &format!("{path}: {expected}")),
            "{expected}: {errors:#?}"
        );
    }
}

// {test}`requirements.ci.outcomes`
// {test}`requirements.ci.determination`
// {test}`requirements.ci.fixme_with_unacknowledged_gap`
#[test]
fn an_unacknowledged_gap_beside_an_acknowledged_one_is_false() {
    let temp = fixture();
    let source = fixme(
        "requirements.ready",
        &["example/queue#125"],
        "Capacity-zero insertion does not return Full yet.",
    );
    let errors = judge_ready(temp.path(), &source, json!(false));
    assert!(
        errors.iter().any(|error| error.contains(
            "evidence/requirements.ready.json: determination is false: The fixture's gaps and their locations."
        )),
        "{errors:?}"
    );
}

// {test}`requirements.ci.schema`
// {test}`requirements.ci.outcomes`
// {test}`requirements.index.state`
// {test}`requirements.cli.fixme_summary`
// {test}`requirements.ci.determination`
#[test]
fn malformed_fixme_is_invalid_and_reports_the_required_shape() {
    let temp = fixture();
    replace_judgment(temp.path(), "timeout", json!({ "FIXME": [] }));
    let report = habeas::validate_repository(temp.path()).unwrap();
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.contains("nonempty array"))
    );
    let status = habeas::status_markdown(temp.path(), false).unwrap();
    let row = status
        .lines()
        .find(|line| line.starts_with("| `requirements.timeout`"))
        .unwrap();
    assert!(row.contains("Invalid"), "{row}");
    assert!(status.contains("nonempty array"), "{status}");
    assert!(!status.contains("affects"), "{status}");
}

// {test}`requirements.ci.determination`
#[test]
fn fixme_does_not_excuse_missing_or_malformed_evidence() {
    let temp = fixture();
    let errors = |root: &Path| habeas::validate_repository(root).unwrap().errors;
    assert!(errors(temp.path()).is_empty(), "{:?}", errors(temp.path()));
    let path = temp.path().join("evidence/requirements.timeout.json");
    let original: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();

    // A malformed record judged FIXME: an unknown field and a wrong example
    // text are each still reported.
    let mut malformed = original.clone();
    malformed["extra"] = json!(1);
    malformed["example"] = json!("Timeout handling does not retry.");
    fs::write(&path, serde_json::to_string_pretty(&malformed).unwrap()).unwrap();
    let found = errors(temp.path());
    for expected in ["unexpected field \"extra\"", "example text changed"] {
        assert!(
            found.iter().any(
                |error| error.starts_with("evidence/requirements.timeout.json")
                    && error.contains(expected)
            ),
            "missing {expected:?}: {found:#?}"
        );
    }

    // A definition whose gap a {fixme} acknowledges still needs evidence.
    fs::remove_file(&path).unwrap();
    let found = errors(temp.path());
    assert!(
        found
            .iter()
            .any(|error| error
                .starts_with("evidence/requirements.timeout.json: missing for example")),
        "{found:#?}"
    );
}

// {test}`requirements.ci.outline_inputs`
// {test}`requirements.ci.determination`
// {test}`requirements.references.evidence_coverage`
// {test}`requirements.ci.fixme_stale_input`
// {test}`requirements.index.state`
// {test}`requirements.cli.fixme_summary`
#[test]
fn fixme_does_not_hide_stale_inputs_or_uncovered_references() {
    let temp = fixture();
    let observed = temp.path().join("observed.rs");
    let source = fs::read_to_string(&observed).unwrap();
    fs::write(&observed, source.replace("= 1;", "= 2;")).unwrap();
    let report = habeas::validate_repository(temp.path()).unwrap();
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.contains("outline input changed: observed.rs <root>")),
        "{:?}",
        report.errors
    );
    let status = habeas::status_markdown(temp.path(), false).unwrap();
    let row = status
        .lines()
        .find(|line| line.starts_with("| `requirements.timeout`"))
        .unwrap();
    assert!(row.contains("Invalid"), "{row}");
    assert!(status.contains("example/io#125"), "{status}");
    assert!(status.contains("outline input changed"), "{status}");
    fs::write(
        temp.path().join("other.txt"),
        "{impl}`requirements.timeout`",
    )
    .unwrap();
    let report = habeas::validate_repository(temp.path()).unwrap();
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.contains("no input covers") && error.contains("other.txt:1"))
    );
}

// {test}`requirements.cli.status`
// {test}`requirements.cli.fixme_summary`
// {test}`requirements.cli.info`
// {test}`requirements.index.state`
// {test}`requirements.index.detail_format`
// {test}`requirements.examples.index`
#[test]
fn valid_fixme_remains_visible_in_status_info_and_book() {
    let temp = fixture();
    for all in [false, true] {
        let status = habeas::status_markdown(temp.path(), all).unwrap();
        assert_eq!(status.contains("requirements.timeout"), all, "{status}");
        assert!(status.contains("FIXME"));
        assert!(status.contains("example/io#125"));
        assert!(status.contains("rust-lang/rust#123"));
        assert!(status.contains("affects 1 judgment"), "{status}");
        assert!(!status.contains("## Validation findings"));
    }
    let verbose = habeas::status_markdown_with_options(temp.path(), false, None, true).unwrap();
    assert!(verbose.contains("requirements.timeout"), "{verbose}");
    assert!(verbose.contains("example"), "{verbose}");
    let info = habeas::info_markdown(temp.path(), "requirements.timeout").unwrap();
    assert!(info.contains("## FIXME references"));
    assert!(info.contains("rust-lang/rust#123"));

    let mut book = Book::new();
    book.push_item(Chapter::new(
        "Requirements",
        fs::read_to_string(temp.path().join("md/requirements.md")).unwrap(),
        "requirements.md",
        Vec::new(),
    ));
    book.push_item(Chapter::new(
        "Index",
        ":::{requirements-index}\n:::\n".to_owned(),
        "requirements-index.md",
        Vec::new(),
    ));
    habeas::preprocess_book(&mut book, temp.path()).unwrap();
    let index = book
        .chapters()
        .find(|chapter| chapter.name == "Index")
        .unwrap();
    assert!(index.content.contains("FIXME"));
    // Detail pages are nested under the defining chapter.
    let detail = book
        .chapters()
        .next()
        .unwrap()
        .sub_items
        .iter()
        .find_map(|item| match item {
            BookItem::Chapter(chapter) if chapter.name == "requirements.timeout" => Some(chapter),
            _ => None,
        })
        .unwrap();
    assert!(detail.content.contains("evidence-FIXME"));
    assert!(detail.content.contains("## FIXME references"));
    assert!(detail.content.contains("rust-lang/rust#123"));
}

// {test}`requirements.ci.reference_set`
// {test}`requirements.references.evidence_coverage`
#[test]
fn editing_or_adding_a_fixme_makes_evidence_stale_or_uncovered() {
    let temp = fixture();
    let observed = temp.path().join("observed.rs");
    let source = fs::read_to_string(&observed).unwrap();
    // Comments do not change the input's hash, but the `{fixme}` body is
    // part of the reference set.
    fs::write(&observed, source.replace("are untested", "remain untested")).unwrap();
    let errors = habeas::validate_repository(temp.path()).unwrap().errors;
    assert!(
        errors.iter().any(
            |error| error.contains("evidence/requirements.timeout.json: reference set changed")
        ),
        "{errors:?}"
    );
    assert!(
        !errors
            .iter()
            .any(|error| error.contains("outline input changed")),
        "{errors:?}"
    );
    fs::write(&observed, &source).unwrap();
    fs::write(
        temp.path().join("other.rs"),
        fixme("requirements.timeout", &[], "Another gap."),
    )
    .unwrap();
    let errors = habeas::validate_repository(temp.path()).unwrap().errors;
    assert!(
        errors.iter().any(|error| error
            == "evidence/requirements.timeout.json: no input covers {fixme}`requirements.timeout` at other.rs:1"),
        "{errors:?}"
    );
}

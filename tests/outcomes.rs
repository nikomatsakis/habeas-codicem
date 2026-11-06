use std::fs;
use std::path::Path;

use habeas::judgment::Judgment;
use mdbook_preprocessor::book::{Book, BookItem, Chapter};
use serde_json::{Value, json};
use tempfile::TempDir;

// {test}`requirements.ci.outcomes`
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
    let example = "Timeout handling retries. CITE`requirements.ready`".replace("CITE", "{cite}");
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
    let source = "// FIXME(#125): timeout retries are untested; partial-write tests are missing (rust-lang/rust#123).";
    fs::write(temp.path().join("observed.txt"), format!("{source}\n")).unwrap();
    let source_input = habeas::outline_input(temp.path(), "observed.txt", "").unwrap();
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
            "determination": { "FIXME": ["#125", "rust-lang/rust#123"] },
            "rationale": "The recorded comments acknowledge the missing timeout tests.",
            "inputs": [source_input]
        }),
    );
    temp
}

// {test}`requirements.ci.schema`
fn write_record(root: &Path, name: &str, value: Value) {
    fs::write(
        root.join(format!("evidence/requirements.{name}.json")),
        serde_json::to_string_pretty(&value).unwrap(),
    )
    .unwrap();
}

// {test}`requirements.ci.outcomes`
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
    assert!(
        habeas::validate_repository(temp.path())
            .unwrap()
            .errors
            .is_empty()
    );
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

// {test}`requirements.ci.determination`
// {test}`requirements.ci.fixme_implementation_gap`
// {test}`requirements.ci.fixme_without_issue`
#[test]
fn fixme_requirements_accept_issue_references_and_other_content() {
    let temp = fixture();
    for (comment, judgment) in [
        (
            "// FIXME(#125): capacity-zero insertion does not return Full yet.",
            json!({ "FIXME": ["#125"] }),
        ),
        (
            "// FIXME: cancellation races are not tested on Windows.",
            json!({ "FIXME": ["cancellation races are not tested on Windows"] }),
        ),
    ] {
        fs::write(temp.path().join("reviewed.txt"), format!("{comment}\n")).unwrap();
        let path = temp.path().join("evidence/requirements.ready.json");
        let mut record: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        record["determination"] = judgment;
        record["inputs"]
            .as_array_mut()
            .unwrap()
            .retain(|input| input.get("file").and_then(Value::as_str) != Some("reviewed.txt"));
        record["inputs"]
            .as_array_mut()
            .unwrap()
            .push(habeas::outline_input(temp.path(), "reviewed.txt", "").unwrap());
        write_record(temp.path(), "ready", record);
        let report = habeas::validate_repository(temp.path()).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
    }
}

// {test}`requirements.ci.schema`
// {test}`requirements.ci.outcomes`
// {test}`requirements.index.state`
// {test}`requirements.cli.fixme_summary`
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

// {test}`requirements.ci.outline_inputs`
// {test}`requirements.ci.determination`
// {test}`requirements.references.evidence_coverage`
// {test}`requirements.ci.fixme_stale_input`
// {test}`requirements.index.state`
// {test}`requirements.cli.fixme_summary`
#[test]
fn fixme_does_not_hide_stale_inputs_or_uncovered_references() {
    let temp = fixture();
    fs::write(temp.path().join("observed.txt"), "changed source\n").unwrap();
    let report = habeas::validate_repository(temp.path()).unwrap();
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.contains("outline input changed: observed.txt <root>"))
    );
    let status = habeas::status_markdown(temp.path(), false).unwrap();
    let row = status
        .lines()
        .find(|line| line.starts_with("| `requirements.timeout`"))
        .unwrap();
    assert!(row.contains("Invalid"), "{row}");
    assert!(status.contains("#125"), "{status}");
    assert!(status.contains("outline input changed"), "{status}");
    fs::write(
        temp.path().join("other.txt"),
        "IMPL`requirements.timeout`".replace("IMPL", "{impl}"),
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
        assert!(status.contains("#125"));
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

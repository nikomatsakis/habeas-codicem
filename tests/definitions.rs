use std::fs;
use std::path::Path;

use habeas::{InputMode, InputTarget, add_input, validate_repository};
use mdbook_preprocessor::book::{Book, Chapter};
use serde_json::{Value, json};
use tempfile::TempDir;

// {test}`requirements.definitions.dependencies`
// {test}`requirements.definitions.declaration`
fn fixture() -> TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("md")).unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::create_dir_all(temp.path().join("evidence")).unwrap();
    fs::write(temp.path().join("book.toml"), "[book]\nsrc = 'md'\n").unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname = 'fixture'\n",
    )
    .unwrap();
    fs::write(temp.path().join("md/SUMMARY.md"), "# Summary\n").unwrap();
    fs::write(
        temp.path().join("md/rules.md"),
        ":::{req} ready\nUse TERM`glossary.a`.\n:::\n".replace("TERM", "{term}"),
    )
    .unwrap();
    fs::write(temp.path().join("md/glossary.md"), ":::{definition} a\n:phrase: alpha\n\nSee TERM`glossary.b`.\n:::\n\n:::{definition} b\n:phrase: beta\n\nSee TERM`glossary.a`.\n:::\n".replace("TERM", "{term}")).unwrap();
    temp
}

// {test}`requirements.definitions.capture`
fn capture(root: &Path) {
    add_input(
        root,
        InputTarget::Requirement("rules.ready".into()),
        InputMode::Requirements("rules.*".into()),
    )
    .unwrap();
}

// {test}`requirements.definitions.capture`
fn approve(path: &Path) {
    let mut record: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    record["determination"] = json!(true);
    record["rationale"] = json!("Reviewed the requirement and its complete definition context.");
    fs::write(path, serde_json::to_string_pretty(&record).unwrap()).unwrap();
}

// {test}`requirements.definitions.transitive_cycle`
// {test}`requirements.definitions.reassessment`
// {test}`requirements.definitions.dependencies`
// {test}`requirements.definitions.inputs`
// {test}`requirements.definitions.capture`
#[test]
fn cyclic_dependencies_are_captured_once_and_changes_require_review() {
    let temp = fixture();
    let root = temp.path();
    capture(root);
    let path = root.join("evidence/rules.ready.json");
    approve(&path);
    let record: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(record["inputs"].as_array().unwrap().len(), 3);
    assert!(validate_repository(root).unwrap().errors.is_empty());
    let before = fs::read(&path).unwrap();
    capture(root);
    assert_eq!(before, fs::read(&path).unwrap());

    let glossary = root.join("md/glossary.md");
    let original = fs::read_to_string(&glossary).unwrap();
    fs::write(&glossary, format!("\n\n{original}")).unwrap();
    assert!(validate_repository(root).unwrap().errors.is_empty());
    for changed in [
        original.replace(":phrase: beta", ":phrase: new beta"),
        original
            .replace("See TERM", "Read TERM")
            .replace("See {term}", "Read {term}"),
    ] {
        fs::write(&glossary, changed).unwrap();
        let report = validate_repository(root).unwrap();
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("input text changed") && e.contains("glossary.")),
            "{:?}",
            report.errors
        );
        assert_eq!(before, fs::read(&path).unwrap());
    }
    capture(root);
    let refreshed: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(refreshed["determination"], false);
    assert_eq!(refreshed["inputs"].as_array().unwrap().len(), 3);
    approve(&path);
    assert!(validate_repository(root).unwrap().errors.is_empty());

    let mut missing = refreshed.clone();
    missing["inputs"]
        .as_array_mut()
        .unwrap()
        .retain(|i| i.get("definition") != Some(&json!("glossary.b")));
    fs::write(&path, missing.to_string()).unwrap();
    assert!(
        validate_repository(root)
            .unwrap()
            .errors
            .iter()
            .any(|e| e.contains("glossary.b, found 0"))
    );
    let mut duplicate = refreshed;
    let input = duplicate["inputs"][1].clone();
    duplicate["inputs"].as_array_mut().unwrap().push(input);
    fs::write(&path, duplicate.to_string()).unwrap();
    assert!(
        validate_repository(root)
            .unwrap()
            .errors
            .iter()
            .any(|e| e.contains("found 2"))
    );
    capture(root);
    let repaired: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(repaired["inputs"].as_array().unwrap().len(), 3);
}

// {test}`requirements.definitions.selection`
// {test}`requirements.definitions.glob_dependency`
#[test]
fn globs_exclude_terms_but_capture_referenced_terms_outside_the_glob() {
    let temp = fixture();
    // `rules.used` matches the glob and is referenced; `rules.unrelated`
    // matches the glob and shares its prefix but is not referenced.
    fs::write(
        temp.path().join("md/rules.md"),
        ":::{req} ready\nUse TERM`glossary.a` and TERM`rules.used`.\n:::\n\n:::{definition} used\n:phrase: used\n\nReferenced.\n:::\n\n:::{definition} unrelated\n:phrase: unrelated\n\nNot referenced.\n:::\n".replace("TERM", "{term}"),
    )
    .unwrap();
    capture(temp.path());
    let record: Value = serde_json::from_str(
        &fs::read_to_string(temp.path().join("evidence/rules.ready.json")).unwrap(),
    )
    .unwrap();
    let inputs = record["inputs"].as_array().unwrap();
    assert_eq!(inputs[0]["requirements"], "rules.*");
    assert_eq!(inputs[0]["matches"], json!(["rules.ready"]));
    assert!(
        inputs[0]["text"]
            .as_array()
            .unwrap()
            .iter()
            .all(|line| !line.as_str().unwrap().contains("{definition}")),
        "definitions are not part of the selected text: {inputs:?}"
    );
    let definitions = inputs[1..]
        .iter()
        .map(|input| input["definition"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        definitions,
        ["glossary.a", "glossary.b", "rules.used"],
        "exactly the referenced definitions, captured separately"
    );
    assert!(
        validate_repository(temp.path())
            .unwrap()
            .errors
            .iter()
            .all(|error| !error.contains("rules.unrelated")),
    );
}

// {test}`requirements.definitions.declaration`
// {test}`requirements.definitions.references`
#[test]
fn invalid_declarations_and_reference_targets_are_reported() {
    for body in [
        "Body only.",
        ":phrase: \n\nBody.",
        ":phrase: one\n:phrase: two\n\nBody.",
        ":phrase: only",
    ] {
        let temp = fixture();
        fs::write(
            temp.path().join("md/bad.md"),
            format!(":::{{definition}} bad\n{body}\n:::\n"),
        )
        .unwrap();
        assert!(
            validate_repository(temp.path())
                .unwrap()
                .errors
                .iter()
                .any(|e| e.contains("definition bad.bad:"))
        );
    }
    let temp = fixture();
    fs::write(
        temp.path().join("md/bad.md"),
        ":::{definition} open\n:phrase: open\n\nNo closing fence.\n",
    )
    .unwrap();
    assert!(
        validate_repository(temp.path())
            .unwrap()
            .errors
            .iter()
            .any(|e| e.contains("missing a closing colon fence"))
    );
    fs::write(temp.path().join("md/bad.md"), ":::{req} x\nRequirement.\n:::\n:::{definition} x\n:phrase: x\n\nDefinition.\n:::\nTERM`rules.ready` TERM`missing.name`".replace("TERM", "{term}")).unwrap();
    let errors = validate_repository(temp.path()).unwrap().errors;
    assert!(
        errors
            .iter()
            .any(|e| e.contains("duplicate definition bad.x"))
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("term rules.ready has no matching named definition"))
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("term missing.name has no matching named definition"))
    );
}

// {test}`requirements.definitions.phrase_rendering`
// {test}`requirements.definitions.references`
// {test}`requirements.definitions.declaration`
#[test]
fn phrases_are_plain_links_and_markdown_literals_are_preserved() {
    let mut book = Book::new();
    book.push_item(Chapter::new(
        "Glossary",
        ":::{definition} a\n:phrase: *alpha* & <beta>\n\nThe body.\n:::\n".into(),
        "glossary.md",
        Vec::new(),
    ));
    let source = "Use TERM`glossary.a`.\n\n`TERM`glossary.a``\n\n```text\nTERM`glossary.a`\n```\n\n\\TERM`glossary.a`".replace("TERM", "{term}");
    book.push_item(Chapter::new("Guide", source, "guide/deep.md", Vec::new()));
    habeas::preprocess_book(&mut book, Path::new(".")).unwrap();
    let chapters: Vec<_> = book.chapters().collect();
    assert!(
        chapters[0]
            .content
            .contains("id=\"habeas-definition-glossary.a\"")
    );
    assert!(!chapters[0].content.contains(":phrase:"));
    assert!(!chapters[1].content.contains("shields.io"));
    assert_eq!(
        chapters[1]
            .content
            .matches("href=\"../glossary.html#habeas-definition-glossary.a\"")
            .count(),
        1
    );
    assert!(
        chapters[1]
            .content
            .contains("&#42;alpha&#42; &amp; &lt;beta&gt;")
    );
}

// {test}`requirements.definitions.transitive_cycle`
// {test}`requirements.definitions.dependencies`
// {test}`requirements.definitions.references`
#[test]
fn outline_inputs_capture_terms_without_counting_them_as_citations() {
    let temp = fixture();
    let root = temp.path();
    fs::write(
        root.join("src/lib.rs"),
        "// IMPL`rules.ready`\n// TERM`glossary.a`\npub fn run() {}\n"
            .replace("IMPL", "{impl}")
            .replace("TERM", "{term}"),
    )
    .unwrap();
    add_input(
        root,
        InputTarget::Requirement("rules.ready".into()),
        InputMode::ItemsAtLine("src/lib.rs:3".into()),
    )
    .unwrap();
    let path = root.join("evidence/rules.ready.json");
    let record: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let inputs = record["inputs"].as_array().unwrap();
    assert!(
        inputs.iter().any(|input| input["item"] == "fn[run]"),
        "{inputs:?}"
    );
    for term in ["glossary.a", "glossary.b"] {
        assert!(
            inputs.iter().any(|input| input["definition"] == term),
            "{inputs:?}"
        );
    }
    capture(root);
    add_input(
        root,
        InputTarget::Requirement("rules.ready".into()),
        InputMode::ItemsAtLine("src/lib.rs:3".into()),
    )
    .unwrap();
    approve(&root.join("evidence/rules.ready.json"));
    assert!(validate_repository(root).unwrap().errors.is_empty());
    let glossary = root.join("md/glossary.md");
    fs::write(
        &glossary,
        fs::read_to_string(&glossary)
            .unwrap()
            .replace(":phrase: beta", ":phrase: changed"),
    )
    .unwrap();
    let errors = validate_repository(root).unwrap().errors;
    assert!(
        errors
            .iter()
            .any(|e| e.contains("evidence/rules.ready.json")
                && e.contains("input text changed")
                && e.contains("glossary.b")),
        "{errors:?}"
    );
    fs::write(
        root.join("src/lib.rs"),
        "// TERM`glossary.a`\npub fn run() {}\n".replace("TERM", "{term}"),
    )
    .unwrap();
    assert!(
        validate_repository(root)
            .unwrap()
            .errors
            .iter()
            .any(|e| e.contains("run") && e.contains("no requirement or example citation"))
    );
}

// {test}`requirements.definitions.inputs`
// {test}`requirements.definitions.references`
// {test}`requirements.definitions.dependencies`
#[test]
fn definition_schema_deletion_and_literal_references_are_checked() {
    let temp = fixture();
    let root = temp.path();
    capture(root);
    let path = root.join("evidence/rules.ready.json");
    approve(&path);
    let original: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let mut invalid = original.clone();
    invalid["inputs"][1]["unexpected"] = json!(true);
    fs::write(&path, invalid.to_string()).unwrap();
    assert!(
        validate_repository(root)
            .unwrap()
            .errors
            .iter()
            .any(|e| e.contains("unexpected field"))
    );
    fs::write(&path, original.to_string()).unwrap();
    fs::remove_file(root.join("md/glossary.md")).unwrap();
    assert!(
        validate_repository(root)
            .unwrap()
            .errors
            .iter()
            .any(|e| e.contains("glossary.a") && e.contains("no matching named definition"))
    );

    let temp = fixture();
    let root = temp.path();
    let body =
        "Literal `TERM`missing.name``.\n\n```text\nTERM`missing.name`\n```\n\n\\TERM`missing.name`"
            .replace("TERM", "{term}");
    fs::write(
        root.join("md/rules.md"),
        format!(":::{{req}} ready\n{body}\n:::\n"),
    )
    .unwrap();
    capture(root);
    approve(&root.join("evidence/rules.ready.json"));
    let report = validate_repository(root).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let record: Value =
        serde_json::from_str(&fs::read_to_string(root.join("evidence/rules.ready.json")).unwrap())
            .unwrap();
    assert_eq!(record["inputs"].as_array().unwrap().len(), 1);
}

// {test}`requirements.definitions.dependencies`
// {test}`requirements.definitions.capture`
#[test]
fn example_subject_and_definition_display_use_the_same_dependencies() {
    let temp = fixture();
    let root = temp.path();
    let path = root.join("md/rules.md");
    let text = fs::read_to_string(&path).unwrap()
        + &"\n:::{example} case\nCITE`rules.ready` with TERM`glossary.a`.\n:::\n"
            .replace("CITE", "{cite}")
            .replace("TERM", "{term}");
    fs::write(path, text).unwrap();
    add_input(
        root,
        InputTarget::Example("rules.case".into()),
        InputMode::Requirements("rules.ready".into()),
    )
    .unwrap();
    let record: Value =
        serde_json::from_str(&fs::read_to_string(root.join("evidence/rules.case.json")).unwrap())
            .unwrap();
    assert_eq!(record["inputs"].as_array().unwrap().len(), 3);
    let display = habeas::info_markdown(root, "rules.case").unwrap();
    assert!(display.contains("definition glossary.a"));
    assert!(display.contains("md/glossary.md"));
    assert!(display.contains(":phrase: alpha"));
}

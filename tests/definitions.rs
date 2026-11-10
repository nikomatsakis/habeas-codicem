use std::fs;
use std::path::{Path, PathBuf};

use habeas::{outline_input, outline_inputs_for_lines, validate_repository};
use mdbook_preprocessor::book::{Book, Chapter};
use serde_json::{Value, json};
use tempfile::TempDir;

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

/// The raw fenced declaration of a fixture definition, as evidence records
/// it: `rules.ready` is the `ready` block in `md/rules.md`.
// {test}`requirements.ci.requirements_inputs`
fn declaration(root: &Path, name: &str) -> Vec<String> {
    let (file, local) = name.split_once('.').unwrap();
    let source = fs::read_to_string(root.join(format!("md/{file}.md"))).unwrap();
    let lines = source.lines().collect::<Vec<_>>();
    let start = lines
        .iter()
        .position(|line| line.starts_with(":::{") && line.ends_with(&format!("}} {local}")))
        .unwrap_or_else(|| panic!("no declaration of {name}"));
    let end = start
        + lines[start..]
            .iter()
            .position(|line| *line == ":::")
            .unwrap();
    lines[start..=end]
        .iter()
        .map(|line| (*line).to_owned())
        .collect()
}

fn definition(root: &Path, name: &str) -> Value {
    json!({ "definition": name, "text": declaration(root, name) })
}

/// A requirements input selecting `selection` and recording `matches`.
// {test}`requirements.ci.requirements_inputs`
fn requirements(root: &Path, selection: &str, matches: &[&str]) -> Value {
    let text = matches
        .iter()
        .map(|name| declaration(root, name))
        .collect::<Vec<_>>()
        .join(&String::new());
    json!({ "requirements": selection, "matches": matches, "text": text })
}

/// Writes a pending evidence record for the requirement or example `name`
/// containing exactly `inputs`, and returns its path.
fn record(root: &Path, name: &str, inputs: Vec<Value>) -> PathBuf {
    let lines = declaration(root, name);
    let kind = if lines[0].starts_with(":::{example}") {
        "example"
    } else {
        "requirement"
    };
    let body = lines[1..lines.len() - 1].join("\n");
    let path = root.join(format!("evidence/{name}.json"));
    let record = json!({
        kind: body.trim(),
        "determination": false,
        "rationale": "Review pending.",
        "references": habeas::reference_set_digest_for(root, name).unwrap(),
        "inputs": inputs,
    });
    fs::write(&path, serde_json::to_string_pretty(&record).unwrap()).unwrap();
    path
}

/// Records the fixture requirement with its selection and the complete
/// definition cycle it depends on.
fn capture(root: &Path) {
    record(
        root,
        "rules.ready",
        vec![
            requirements(root, "rules.*", &["rules.ready"]),
            definition(root, "glossary.a"),
            definition(root, "glossary.b"),
        ],
    );
}

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
#[test]
fn cyclic_dependencies_are_required_once_and_changes_require_review() {
    let temp = fixture();
    let root = temp.path();
    capture(root);
    let path = root.join("evidence/rules.ready.json");
    approve(&path);
    let report = validate_repository(root).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let before = fs::read(&path).unwrap();

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
    // Reassessment records the current snapshots.
    capture(root);
    let refreshed: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
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
    let root = temp.path();
    let selection = requirements(root, "rules.*", &["rules.ready"]);
    let path = record(
        root,
        "rules.ready",
        vec![
            selection.clone(),
            definition(root, "glossary.a"),
            definition(root, "glossary.b"),
            definition(root, "rules.used"),
        ],
    );
    approve(&path);
    // The glob selects only the requirement, and exactly the referenced
    // definitions are required as separate inputs.
    let errors = validate_repository(root).unwrap().errors;
    assert!(errors.is_empty(), "{errors:?}");

    // The referenced definition is required although the glob does not
    // select it; the unreferenced one is never required.
    record(
        root,
        "rules.ready",
        vec![
            selection,
            definition(root, "glossary.a"),
            definition(root, "glossary.b"),
        ],
    );
    approve(&path);
    let errors = validate_repository(root).unwrap().errors;
    assert!(
        errors
            .iter()
            .any(|error| error.contains("rules.used, found 0")),
        "{errors:?}"
    );
    assert!(
        errors
            .iter()
            .all(|error| !error.contains("rules.unrelated")),
        "{errors:?}"
    );

    // A selection whose recorded matches include named definitions is stale.
    let definitions = requirements(root, "rules.*", &["rules.ready", "rules.used"]);
    record(
        root,
        "rules.ready",
        vec![
            definitions,
            definition(root, "glossary.a"),
            definition(root, "glossary.b"),
            definition(root, "rules.used"),
        ],
    );
    approve(&path);
    let errors = validate_repository(root).unwrap().errors;
    assert!(
        errors
            .iter()
            .any(|error| error.starts_with("evidence/rules.ready.json")),
        "{errors:?}"
    );
}

// {test}`requirements.definitions.selection`
// {test}`requirements.references.definition_globs`
#[test]
fn selections_match_requirements_and_examples_but_never_named_definitions() {
    let temp = fixture();
    let root = temp.path();
    fs::write(
        root.join("md/rules.md"),
        ":::{req} ready\nUse TERM`glossary.a`.\n:::\n\n\
         :::{example} case\nA ready case. CITE`rules.ready`\n:::\n\n\
         :::{definition} used\n:phrase: used\n\nNot referenced by the selection.\n:::\n"
            .replace("TERM", "{term}")
            .replace("CITE", "{cite}"),
    )
    .unwrap();
    // The glob selects the requirement and the example, not the definition
    // whose name it also matches, and that definition is not a dependency.
    let selected = vec![
        requirements(root, "rules.*", &["rules.case", "rules.ready"]),
        definition(root, "glossary.a"),
        definition(root, "glossary.b"),
    ];
    let path = record(root, "rules.ready", selected.clone());
    approve(&path);
    let errors = validate_repository(root).unwrap().errors;
    assert!(
        errors
            .iter()
            .all(|error| !error.starts_with("evidence/rules.ready.json")),
        "{errors:?}"
    );

    // A named definition's exact name selects nothing.
    let exact = requirements(root, "rules.used", &["rules.used"]);
    record(
        root,
        "rules.ready",
        vec![
            exact,
            definition(root, "glossary.a"),
            definition(root, "glossary.b"),
        ],
    );
    let errors = validate_repository(root).unwrap().errors;
    assert!(
        errors
            .iter()
            .any(|error| error.contains("matches no requirement or example")),
        "{errors:?}"
    );
    record(root, "rules.ready", selected);

    // The CLI selection paths apply the same rule.
    for args in [["status", "rules.*"], ["info", "rules.*"]] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_habeas"))
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("rules.ready"), "{args:?}: {stdout}");
        assert!(stdout.contains("rules.case"), "{args:?}: {stdout}");
        assert!(!stdout.contains("rules.used"), "{args:?}: {stdout}");
    }
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_habeas"))
        .current_dir(root)
        .args(["info", "rules.used"])
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
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

// {test}`requirements.definitions.declaration`
#[test]
fn definitions_share_name_and_fence_rules_and_need_no_evidence() {
    let temp = fixture();
    let root = temp.path();
    // A longer opening fence holds a shorter colon line in its body.
    fs::write(
        root.join("md/long.md"),
        "::::{definition} fenced\n:phrase: fenced\n\nBody with\n:::\na shorter colon line.\n::::\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("md/bad dir")).unwrap();
    fs::write(
        root.join("md/bad dir/terms.md"),
        ":::{definition} a\n:phrase: a\n\nBody.\n:::\n",
    )
    .unwrap();
    let errors = validate_repository(root).unwrap().errors;
    assert!(
        errors
            .iter()
            .any(|e| e.contains("invalid definition name \"bad dir.terms.a\"")),
        "{errors:#?}"
    );
    // The long fence closes only at four colons, and definitions need no
    // evidence records, unlike the requirement.
    assert!(
        !errors.iter().any(|e| e.contains("long.fenced")),
        "{errors:#?}"
    );
    assert!(
        !errors.iter().any(|e| e.contains("glossary.")),
        "{errors:#?}"
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("evidence/rules.ready.json: missing")),
        "{errors:#?}"
    );
}

// {test}`requirements.ci.definition_errors`
#[test]
fn duplicates_involving_named_definitions_name_the_first_declaration() {
    for (first, second, kind) in [
        (
            ":::{req} x\nRequirement.\n:::\n",
            ":::{definition} x\n:phrase: x\n\nDefinition.\n:::\n",
            "definition",
        ),
        (
            ":::{definition} x\n:phrase: x\n\nDefinition.\n:::\n",
            ":::{example} x\nExample.\n:::\n",
            "example",
        ),
    ] {
        let temp = fixture();
        fs::write(temp.path().join("md/bad.md"), format!("{first}\n{second}")).unwrap();
        let errors = validate_repository(temp.path()).unwrap().errors;
        let line = first.lines().count() + 2;
        let expected =
            format!("md/bad.md:{line}: duplicate {kind} bad.x; first declared at md/bad.md:1");
        assert!(
            errors.iter().any(|error| error.contains(&expected)),
            "{expected}\n{errors:#?}"
        );
    }
}

// {test}`requirements.definitions.references`
#[test]
fn references_to_a_definition_of_another_kind_are_reported() {
    let temp = fixture();
    fs::write(
        temp.path().join("md/cases.md"),
        ":::{example} case\nA ready case. CITE`rules.ready`\n:::\n".replace("CITE", "{cite}"),
    )
    .unwrap();
    let source = ["impl", "cite", "test"]
        .iter()
        .map(|role| format!("// {{{role}}}`glossary.a`\n"))
        .chain(["// TERM`cases.case`\n".replace("TERM", "{term}")])
        .collect::<String>();
    fs::write(
        temp.path().join("src/lib.rs"),
        format!("{source}fn uses() {{}}\n"),
    )
    .unwrap();
    let errors = validate_repository(temp.path()).unwrap().errors;
    for (line, role) in [(1, "impl"), (2, "cite"), (3, "test")] {
        let expected = format!(
            "src/lib.rs:{line}: {{{role}}}`glossary.a` has no matching requirement or example"
        );
        assert!(
            errors.iter().any(|error| error.contains(&expected)),
            "{expected}: {errors:#?}"
        );
    }
    assert!(
        errors.iter().any(|error| error
            .contains("src/lib.rs:4: term cases.case has no matching named definition")),
        "{errors:#?}"
    );
    // A reference to a term does not cover the Rust item.
    assert!(
        errors.iter().any(|error| error
            .contains("src/lib.rs:5: function uses is uncovered: it has no requirement or example reference, and no evidence record or review read it")),
        "{errors:#?}"
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

// {test}`requirements.definitions.phrase_rendering`
#[test]
fn a_term_reference_renders_only_its_phrase_as_a_plain_link() {
    let mut book = Book::new();
    book.push_item(Chapter::new(
        "Glossary",
        ":::{definition} first_parent_history\n:phrase: first-parent history\n\nHistory along first parents.\n:::\n".into(),
        "glossary.md",
        Vec::new(),
    ));
    book.push_item(Chapter::new(
        "Guide",
        "Follow the TERM`glossary.first_parent_history`.\n".replace("TERM", "{term}"),
        "guide.md",
        Vec::new(),
    ));
    habeas::preprocess_book(&mut book, Path::new(".")).unwrap();
    let chapters: Vec<_> = book.chapters().collect();
    let guide = &chapters[1].content;
    // The whole reference becomes exactly one ordinary hyperlink: no class,
    // badge image, or other markup, and the phrase is the only link text.
    assert_eq!(
        guide.trim_end(),
        "Follow the <a href=\"glossary.html#habeas-definition-glossary.first_parent_history\">first-parent history</a>."
    );
    assert!(
        chapters[0]
            .content
            .contains("id=\"habeas-definition-glossary.first_parent_history\"")
    );
}

// {test}`requirements.definitions.inputs`
#[test]
fn changed_definition_snapshots_show_a_compact_diff_and_text_is_required() {
    let temp = fixture();
    let root = temp.path();
    capture(root);
    let path = root.join("evidence/rules.ready.json");
    approve(&path);
    assert!(validate_repository(root).unwrap().errors.is_empty());
    let glossary = root.join("md/glossary.md");
    let original = fs::read_to_string(&glossary).unwrap();
    fs::write(
        &glossary,
        original.replace(
            concat!("See {", "term}`glossary.b`."),
            concat!("See {", "term}`glossary.b` again."),
        ),
    )
    .unwrap();
    let errors = validate_repository(root).unwrap().errors;
    let changed = errors
        .iter()
        .find(|error| error.contains("input text changed") && error.contains("glossary.a"))
        .unwrap_or_else(|| panic!("{errors:?}"));
    // Only the changed body line is shown, as a removal and an addition.
    let diff: Vec<_> = changed.lines().skip(1).collect();
    assert_eq!(
        diff,
        [
            concat!("- See {", "term}`glossary.b`."),
            concat!("+ See {", "term}`glossary.b` again.")
        ],
        "{changed}"
    );
    fs::write(&glossary, &original).unwrap();

    // A definition input must carry a nonempty text array.
    let record: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    for text in [None, Some(json!([])), Some(json!("not lines"))] {
        let mut broken = record.clone();
        let input = broken["inputs"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|input| input["definition"] == "glossary.a")
            .unwrap();
        match &text {
            None => {
                input.as_object_mut().unwrap().remove("text");
            }
            Some(text) => input["text"] = text.clone(),
        }
        fs::write(&path, broken.to_string()).unwrap();
        let errors = validate_repository(root).unwrap().errors;
        assert!(
            errors
                .iter()
                .any(|error| error.starts_with("evidence/rules.ready.json")
                    && error.contains("text")),
            "{text:?}: {errors:?}"
        );
    }
}

// {test}`requirements.definitions.dependencies`
#[test]
fn markdown_and_plain_text_inputs_contribute_their_term_references() {
    let temp = fixture();
    let root = temp.path();
    fs::write(
        root.join("md/more.md"),
        ":::{definition} prose\n:phrase: prose\n\nProse.\n:::\n\n\
         :::{definition} line\n:phrase: line\n\nA line.\n:::\n\n\
         :::{definition} literal\n:phrase: literal\n\nLiteral.\n:::\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("docs/notes.md"),
        "# Notes\n\nThis uses TERM`more.prose` in prose.\n\n```text\nTERM`more.literal`\n```\n"
            .replace("TERM", "{term}"),
    )
    .unwrap();
    fs::write(
        root.join("notes.txt"),
        "plain TERM`more.line` text\n".replace("TERM", "{term}"),
    )
    .unwrap();
    // The heading's section covers both the paragraph and the code block.
    let section = outline_input(root, "docs/notes.md", "section[Notes]").unwrap();
    let line = outline_inputs_for_lines(root, "notes.txt", 1, 1).unwrap();
    assert_eq!(line.len(), 1, "{line:?}");
    let sources = [section, line[0].clone()];
    // The fixture's requirement text needs the glossary cycle; the Markdown
    // paragraph and the text line add their terms, and the code block does not.
    let terms = |names: &[&str]| {
        sources
            .iter()
            .cloned()
            .chain(names.iter().map(|name| definition(root, name)))
            .collect::<Vec<_>>()
    };
    let path = record(
        root,
        "rules.ready",
        terms(&["glossary.a", "glossary.b", "more.line", "more.prose"]),
    );
    approve(&path);
    let errors = validate_repository(root).unwrap().errors;
    assert!(errors.is_empty(), "{errors:?}");
    for (names, missing) in [
        (&["glossary.a", "glossary.b", "more.line"][..], "more.prose"),
        (&["glossary.a", "glossary.b", "more.prose"][..], "more.line"),
    ] {
        record(root, "rules.ready", terms(names));
        approve(&path);
        let errors = validate_repository(root).unwrap().errors;
        assert!(
            errors
                .iter()
                .any(|error| error.contains(&format!("{missing}, found 0"))),
            "{missing}: {errors:?}"
        );
    }
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
    // The function's term reference needs the same glossary cycle as the
    // requirement text, captured once.
    let path = record(
        root,
        "rules.ready",
        vec![
            requirements(root, "rules.*", &["rules.ready"]),
            outline_input(root, "src/lib.rs", "fn[run]").unwrap(),
            definition(root, "glossary.a"),
            definition(root, "glossary.b"),
        ],
    );
    approve(&path);
    let errors = validate_repository(root).unwrap().errors;
    assert!(errors.is_empty(), "{errors:?}");
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
    // Removing a citation comment leaves the node's hash unchanged, so the
    // record reading it would still cover it.
    fs::remove_file(&path).unwrap();
    assert!(
        validate_repository(root)
            .unwrap()
            .errors
            .iter()
            .any(|e| e.contains("run") && e.contains("is uncovered"))
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
    // Literal and escaped references require no definition inputs.
    let path = record(
        root,
        "rules.ready",
        vec![requirements(root, "rules.*", &["rules.ready"])],
    );
    approve(&path);
    let report = validate_repository(root).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
}

// {test}`requirements.definitions.dependencies`
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
    // The example's own term reference and its selected requirement need the
    // same glossary cycle, recorded once.
    record(
        root,
        "rules.case",
        vec![
            requirements(root, "rules.ready", &["rules.ready"]),
            definition(root, "glossary.a"),
            definition(root, "glossary.b"),
        ],
    );
    let errors = validate_repository(root).unwrap().errors;
    assert!(
        errors
            .iter()
            .all(|error| !error.contains("definition input")),
        "{errors:?}"
    );
    let display = habeas::info_markdown(root, "rules.case").unwrap();
    assert!(display.contains("definition glossary.a"));
    assert!(display.contains("md/glossary.md"));
    assert!(display.contains(":phrase: alpha"));
}

//! Fenced references, built-in definitions, and citation reviews, checked
//! through repository validation and `habeas status`.

use std::fs;
use std::path::Path;

use mdbook_preprocessor::book::{Book, Chapter};
use serde_json::{Value, json};
use tempfile::TempDir;

/// A role written so this file's own source does not contain it.
// {cite}`habeas:util`
fn role(kind: &str) -> String {
    format!("{{{kind}}}")
}

/// A project with one requirement, `rules.ready`, and empty sources.
// {cite}`habeas:util`
fn project() -> TempDir {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("md")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("evidence")).unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\nname = \"fixture\"\n").unwrap();
    fs::write(root.join("book.toml"), "[book]\nsrc = \"md\"\n").unwrap();
    fs::write(root.join("md/rules.md"), ":::{req} ready\nReady.\n:::\n").unwrap();
    temp
}

// {cite}`habeas:util`
fn errors(root: &Path) -> Vec<String> {
    habeas::validate_repository(root).unwrap().errors
}

// {cite}`habeas:util`
fn has(errors: &[String], needle: &str) -> bool {
    errors.iter().any(|error| error.contains(needle))
}

// {test}`requirements.ci.errors`
#[test]
fn unclosed_fences_still_report_unknown_names() {
    let temp = project();
    let root = temp.path();
    let cr = role("cite");
    fs::write(
        root.join("src/lib.rs"),
        format!(
            "// :::{cr} rules.missing\n// never closed\n\n// :::{cr} habeas:other\n// never closed\n"
        ),
    )
    .unwrap();
    let errors = errors(root);
    for expected in [
        "src/lib.rs:1: fenced {cite} reference to rules.missing has no closing fence".to_owned(),
        format!("src/lib.rs:1: {cr}`rules.missing` has no matching requirement or example"),
        "src/lib.rs:4: fenced {cite} reference to habeas:other has no closing fence".to_owned(),
        format!("src/lib.rs:4: {cr}`habeas:other` has no matching built-in definition"),
    ] {
        assert!(has(&errors, &expected), "missing {expected:?}: {errors:#?}");
    }
}

// {test}`requirements.references.roles`
// {test}`requirements.references.builtin_definitions`
// {test}`requirements.references.rust_item_coverage`
// {test}`requirements.references.procedure`
#[test]
fn fenced_references_in_comments_and_built_in_rules_are_validated() {
    let temp = project();
    let root = temp.path();
    let cr = role("cite");
    let source = format!(
        "// :::{cr} habeas:util\n// Glue that formats output.\n// :::\nfn glue() {{}}\n\n\
         // {cr}`habeas:none`\nfn inline() {{}}\n\n\
         // :::{cr} habeas:util\n// {cr}`rules.ready`\n// :::\nfn empty() {{}}\n\n\
         // :::{im} habeas:util\n// Wrong role.\n// :::\nfn wrong_role() {{}}\n\n\
         // :::{cr} habeas:other\n// Unknown.\n// :::\nfn unknown() {{}}\n\n\
         // :::{cr} rules.ready\n// never closed\n\n// :::\nfn unclosed() {{}}\n\n\
         // :::{cr} habeas:none\n// Formats output.\n// :::\nfn fenced_none() {{}}\n",
        im = role("impl"),
    );
    fs::write(root.join("src/lib.rs"), source).unwrap();
    let errors = errors(root);
    for expected in [
        // `habeas:none` was removed: it is an unknown name in either form.
        "src/lib.rs:6: {cite}`habeas:none` has no matching built-in definition",
        "src/lib.rs:30: {cite}`habeas:none` has no matching built-in definition",
        "src/lib.rs:14: built-in habeas:util may only be referenced with {cite}",
        "src/lib.rs:19: {cite}`habeas:other` has no matching built-in definition",
        "src/lib.rs:24: fenced {cite} reference to rules.ready has no closing fence",
        "src/lib.rs:7: function inline is uncovered: it has no requirement or example reference, and no evidence record or review read it",
        "src/lib.rs:17: function wrong_role is uncovered: it has no requirement or example reference, and no evidence record or review read it",
        "src/lib.rs:22: function unknown is uncovered: it has no requirement or example reference, and no evidence record or review read it",
        "src/lib.rs:28: function unclosed is uncovered: it has no requirement or example reference, and no evidence record or review read it",
        "src/lib.rs:33: function fenced_none is uncovered: it has no requirement or example reference, and no evidence record or review read it",
    ] {
        assert!(has(&errors, expected), "missing {expected:?}: {errors:#?}");
    }
    // A fenced citation of habeas:util covers its function, as does an
    // inline reference in a fence body.
    assert!(!has(&errors, "function glue"), "{errors:#?}");
    assert!(!has(&errors, "function empty"), "{errors:#?}");
    // Only valid citations other than habeas:util need reviews.
    assert!(has(
        &errors,
        "missing citation review for src/lib.rs fn[empty]"
    ));
    for item in ["glue", "inline", "fenced_none"] {
        assert!(
            !has(
                &errors,
                &format!("missing citation review for src/lib.rs fn[{item}]")
            ),
            "{errors:#?}"
        );
    }
    // Built-ins need no evidence.
    assert!(!has(&errors, "evidence/habeas:"), "{errors:#?}");
}

// {test}`requirements.references.util`
// {test}`requirements.references.citation_review`
// {test}`requirements.references.rust_item_coverage`
#[test]
fn habeas_util_citations_cover_their_item_without_a_citation_review() {
    let temp = project();
    let root = temp.path();
    let cr = role("cite");
    let source = format!(
        "// {cr}`habeas:util`\nfn inline() {{}}\n\n\
         // :::{cr} habeas:util\n// Formats output for its caller.\n// :::\nfn fenced() {{}}\n\n\
         // :::{cr} habeas:util\n// :::\nfn bare() {{}}\n\n\
         // {cr}`habeas:util`\n// {cr}`rules.ready`\nfn mixed() {{}}\n\n\
         // {im}`habeas:util`\nfn implemented() {{}}\n\n\
         // {te}`habeas:util`\nfn tested() {{}}\n",
        im = role("impl"),
        te = role("test"),
    );
    fs::write(root.join("src/lib.rs"), source).unwrap();
    let errors = errors(root);
    // Inline and fenced citations, with or without commentary, cover their
    // item and need no citation review.
    for item in ["inline", "fenced", "bare"] {
        assert!(!has(&errors, &format!("function {item} ")), "{errors:#?}");
        assert!(
            !has(
                &errors,
                &format!("missing citation review for src/lib.rs fn[{item}]")
            ),
            "{errors:#?}"
        );
    }
    // Another citation on the same item still needs its review.
    assert!(
        has(&errors, "missing citation review for src/lib.rs fn[mixed]"),
        "{errors:#?}"
    );
    // Only {cite} may reference it.
    for line in [17, 20] {
        let expected = format!(
            "src/lib.rs:{line}: built-in habeas:util may only be referenced with {}",
            role("cite")
        );
        assert!(has(&errors, &expected), "missing {expected:?}: {errors:#?}");
    }
    assert!(
        has(&errors, "function implemented is uncovered"),
        "{errors:#?}"
    );
    assert!(has(&errors, "function tested is uncovered"), "{errors:#?}");
    assert!(!has(&errors, "evidence/habeas:util"), "{errors:#?}");
}

// {test}`requirements.references.builtin_definitions`
#[test]
fn built_ins_have_no_evidence_index_entry_or_status_and_are_validated_in_the_book() {
    let temp = project();
    let root = temp.path();
    let cr = role("cite");
    let chapter = format!(
        "# Rules\n\n:::{{req}} ready\nReady.\n:::\n\nSee {cr}`habeas:none`.\n\n:::{im} habeas:util\nWrong role.\n:::\n\n:::{cr} habeas:util\nServes its callers.\n:::\n\n# Index\n\n:::{{requirements-index}}\n:::\n",
        im = role("impl"),
    );
    fs::write(root.join("md/rules.md"), &chapter).unwrap();
    // An evidence file for a built-in is not permitted: it is an orphan.
    fs::write(
        root.join("evidence/habeas:util.json"),
        json!({"requirement": "x", "determination": true, "rationale": "x", "inputs": []})
            .to_string(),
    )
    .unwrap();
    let errors = errors(root);
    for expected in [
        // `habeas:none` was removed, so it is an unknown built-in.
        "md/rules.md:7: {cite}`habeas:none` has no matching built-in definition",
        "md/rules.md:9: built-in habeas:util may only be referenced with {cite}",
    ] {
        assert!(has(&errors, expected), "missing {expected:?}: {errors:#?}");
    }
    assert!(
        has(&errors, "evidence/habeas:util.json"),
        "an evidence file for a built-in must be reported: {errors:#?}"
    );
    // The fenced citation in the book is valid, and no built-in
    // needs evidence.
    assert!(!has(&errors, "md/rules.md:13"), "{errors:#?}");
    assert!(!has(&errors, "missing evidence for habeas:"), "{errors:#?}");

    // Built-ins are not definitions in status or the requirements index.
    let status = habeas::status_markdown(root, true).unwrap();
    let table = &status[..status.find("## ").unwrap()];
    assert!(table.contains("`rules.ready` (requirement)"), "{status}");
    assert!(!table.contains("habeas:"), "{status}");
    assert!(!status.contains("### `habeas:"), "{status}");
    let mut book = Book::new();
    book.push_item(Chapter::new("Rules", chapter, "rules.md", Vec::new()));
    habeas::preprocess_book(&mut book, root).unwrap();
    let rendered = book
        .chapters()
        .map(|chapter| (chapter.name.clone(), chapter.content.clone()))
        .collect::<Vec<_>>();
    assert!(
        !rendered.iter().any(|(name, _)| name.contains("habeas:")),
        "no detail page for a built-in: {rendered:#?}"
    );
    let (_, content) = &rendered[0];
    let index = &content[content.find("# Index").unwrap()..];
    assert!(index.contains("rules.ready"), "{index}");
    assert!(!index.contains("habeas:util"), "{index}");
}

// {test}`requirements.references.builtin_definitions`
#[test]
fn project_definitions_cannot_use_reserved_built_in_names() {
    let temp = project();
    let root = temp.path();
    fs::write(
        root.join("md/rules.md"),
        ":::{req} ready\nReady.\n:::\n\n:::{req} habeas:none\nShadow.\n:::\n",
    )
    .unwrap();
    let errors = errors(root);
    assert!(
        has(
            &errors,
            "md/rules.md:5: invalid requirement name \"rules.habeas:none\": names beginning with habeas: are reserved"
        ),
        "{errors:#?}"
    );
    let status = habeas::status_markdown(root, true).unwrap();
    assert!(!status.contains("`habeas:none` (requirement)"), "{status}");
}

/// The digest after writing `source` to `src/lib.rs`.
// {cite}`habeas:util`
fn digest(root: &Path, source: &str) -> String {
    fs::write(root.join("src/lib.rs"), source).unwrap();
    habeas::reference_set_digest_for(root, "rules.ready").unwrap()
}

// {test}`requirements.ci.reference_set`
#[test]
fn fenced_commentary_outside_the_book_contributes_to_the_reference_set() {
    let temp = project();
    let root = temp.path();
    let fenced = |body: &str| {
        format!(
            "// :::{} rules.ready\n{body}// :::\nfn run() {{}}\n",
            role("impl")
        )
    };
    let base = digest(root, &fenced("// Handles the empty case.\n"));
    // Reflowing the commentary keeps the digest; changing its words does not.
    assert_eq!(
        digest(root, &fenced("// Handles the\n//   empty case.\n")),
        base
    );
    assert_ne!(digest(root, &fenced("// Handles the full case.\n")), base);
    assert_ne!(
        digest(
            root,
            &format!("// {}`rules.ready`\nfn run() {{}}\n", role("impl"))
        ),
        base
    );

    // In the book, commentary is part of the declaration text instead.
    let book = |body: &str| {
        fs::write(
            root.join("md/rules.md"),
            format!(
                "# Rules\n\n::::{{req}} ready\nReady.\n::::\n\n::::{{req}} other\nOther.\n\n:::{} rules.ready\n{body}\n:::\n::::\n",
                role("impl")
            ),
        )
        .unwrap();
        habeas::reference_set_digest_for(root, "rules.ready").unwrap()
    };
    assert_eq!(book("Refines the check."), book("Refines another check."));
}

// {test}`requirements.references.roles`
// {test}`requirements.references.discovery`
#[test]
fn fixme_references_are_fenced_described_and_named_only_outside_the_book() {
    let temp = project();
    let root = temp.path();
    let fx = role("fixme");
    fs::write(
        root.join("src/gaps.rs"),
        format!(
            "// {fx}`rules.ready`\n\n\
             // :::{fx} rules.ready\n// :github: example/rules#1\n// :gitlab: example/rules#2\n// :github: example/rules\n// The gap.\n// :::\n\n\
             // :::{fx} rules.ready\n// :github: example/rules#1\n// :::\n\n\
             // :::{fx}\n// Only words.\n// :::\n\n\
             // :::{fx}\n// :github: example/rules#4\n// Tracked here.\n// :::\n\n\
             // :::{fx} rules.missing\n// Gap.\n// :::\n"
        ),
    )
    .unwrap();
    fs::write(
        root.join("md/rules.md"),
        format!(
            "::::{{req}} ready\nReady.\n\n:::{fx} rules.ready\nIn a declaration.\n:::\n\n:::{fx}\n:github: example/rules#3\n:::\n::::\n\n:::{fx} rules.ready\nIn the book.\n:::\n"
        ),
    )
    .unwrap();
    let errors = errors(root);
    for expected in [
        "src/gaps.rs:1: inline {fixme} reference to rules.ready must use the fenced form",
        "src/gaps.rs:3: unsupported {fixme} option :gitlab:",
        "src/gaps.rs:3: :github: value \"example/rules\" is not of the form <owner>/<repo>#<number>",
        "src/gaps.rs:10: {fixme} to rules.ready has an empty body",
        "src/gaps.rs:14: an unnamed {fixme} must name at least one issue",
        "src/gaps.rs:23: {fixme}`rules.missing` has no matching requirement or example",
        "md/rules.md:4: a named {fixme} to rules.ready must not appear in a requirement or example declaration",
        "md/rules.md:13: a named {fixme} to rules.ready must appear in a source outside the book",
    ] {
        assert!(has(&errors, expected), "missing {expected:?}: {errors:#?}");
    }
    // An unnamed `{fixme}` with an issue is valid anywhere, including a
    // requirement body, and is not an edge needing coverage.
    for line in ["src/gaps.rs:18", "md/rules.md:8"] {
        assert!(!has(&errors, line), "{line}: {errors:#?}");
    }
    // A misplaced named `{fixme}` is reported once, not also as uncovered.
    assert!(!has(&errors, "` at md/"), "{errors:#?}");
}

// {test}`requirements.references.procedure`
// {test}`requirements.references.rust_item_coverage`
// {test}`requirements.references.roles`
#[test]
fn a_fixme_does_not_count_as_an_items_reference() {
    let temp = project();
    let root = temp.path();
    let fx = role("fixme");
    fs::write(
        root.join("src/lib.rs"),
        format!(
            "// :::{fx} rules.ready\n// The gap.\n// :::\nfn gap() {{}}\n\n\
             // :::{fx}\n// :github: example/rules#1\n// :::\nfn tracked() {{}}\n\n\
             // {}`rules.ready`\n// :::{fx} rules.ready\n// Another gap.\n// :::\nfn implemented() {{}}\n",
            role("impl")
        ),
    )
    .unwrap();
    let errors = errors(root);
    for expected in [
        "src/lib.rs:4: function gap is uncovered: it has no requirement or example reference, and no evidence record or review read it",
        "src/lib.rs:9: function tracked is uncovered: it has no requirement or example reference, and no evidence record or review read it",
    ] {
        assert!(has(&errors, expected), "missing {expected:?}: {errors:#?}");
    }
    assert!(!has(&errors, "function implemented"), "{errors:#?}");
}

// {test}`requirements.ci.reference_set`
#[test]
fn fixme_edges_and_their_bodies_are_part_of_the_reference_set() {
    let temp = project();
    let root = temp.path();
    let fx = role("fixme");
    let fixme = |body: &str| format!("// :::{fx} rules.ready\n{body}// :::\nfn run() {{}}\n");
    let none = digest(root, "fn run() {}\n");
    let base = digest(
        root,
        &fixme("// :github: example/rules#1\n// The empty case fails.\n"),
    );
    assert_ne!(base, none, "adding a {{fixme}} changes the reference set");
    // Reflowing keeps the digest; editing an issue or the description does not.
    assert_eq!(
        digest(
            root,
            &fixme("// :github: example/rules#1\n// The empty\n//   case fails.\n")
        ),
        base
    );
    assert_ne!(
        digest(
            root,
            &fixme("// :github: example/rules#2\n// The empty case fails.\n")
        ),
        base
    );
    assert_ne!(
        digest(
            root,
            &fixme("// :github: example/rules#1\n// The full case fails.\n")
        ),
        base
    );
    // A different role on the same node is a different edge.
    assert_ne!(
        digest(
            root,
            &format!(
                "// :::{} rules.ready\n// :github: example/rules#1\n// The empty case fails.\n// :::\nfn run() {{}}\n",
                role("impl")
            )
        ),
        base
    );
    // An inline `{fixme}` is invalid and is not an edge either.
    assert_eq!(
        digest(root, &format!("// {fx}`rules.ready`\nfn run() {{}}\n")),
        none
    );
    // An unnamed `{fixme}` is not an edge.
    assert_eq!(
        digest(
            root,
            &format!("// :::{fx}\n// :github: example/rules#1\n// :::\nfn run() {{}}\n")
        ),
        none
    );
}

/// A recorded citation review for `fn[helper]` in `src/lib.rs`.
// {cite}`habeas:util`
fn review(root: &Path, citations: Value, determination: Value) -> Value {
    json!({
        "file": "src/lib.rs",
        "item": "fn[helper]",
        "citations": citations,
        "determination": determination,
        "rationale": "The citations are sensible.",
        "inputs": [
            habeas::outline_input(root, "src/lib.rs", "fn[helper]").unwrap(),
            {"requirements": "rules.ready", "matches": ["rules.ready"],
             "text": [":::{req} ready", "Ready.", ":::"]}
        ],
    })
}

// {test}`requirements.ci.errors`
// {test}`requirements.references.citation_review`
#[test]
fn malformed_citation_entries_do_not_hide_later_independent_errors() {
    let temp = project();
    let root = temp.path();
    fs::write(
        root.join("src/lib.rs"),
        format!("// {}`rules.ready`\nfn helper() {{}}\n", role("cite")),
    )
    .unwrap();
    let mut record = review(
        root,
        json!([null, {"role": "unknown", "name": 42, "commentary": false}]),
        json!(true),
    );
    record["rationale"] = json!("");
    let path = root.join("evidence/citations/src/lib.rs/fn[helper].json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, record.to_string()).unwrap();
    let errors = errors(root);
    for expected in [
        "citations[0]: expected a JSON object",
        "citations[1]: unknown role \"unknown\"",
        "citations[1]: name must be a string",
        "citations[1]: commentary must be a string",
        "rationale must not be empty",
    ] {
        assert!(has(&errors, expected), "{expected}: {errors:#?}");
    }
    assert!(!has(&errors, "citations changed"), "{errors:#?}");
}

// {test}`requirements.references.citation_review`
// {test}`requirements.ci.outcomes`
// {test}`requirements.ci.orphans`
#[test]
fn citation_reviews_are_required_current_and_not_orphaned() {
    let temp = project();
    let root = temp.path();
    let cr = role("cite");
    let helper = |body: &str| {
        format!(
            "// {cr}`rules.ready`\n// :::{cr} habeas:util\n// {body}\n// :::\nfn helper() {{\n    work();\n}}\n"
        )
    };
    fs::write(root.join("src/lib.rs"), helper("Formats output.")).unwrap();
    fs::write(root.join("notes.txt"), format!("See {cr}`rules.ready`.\n")).unwrap();
    // Citations in the book are reviewed with their declaration instead.
    fs::write(
        root.join("md/guide.md"),
        format!("# Guide\n\nSee {cr}`rules.ready`.\n"),
    )
    .unwrap();
    let path = "evidence/citations/src/lib.rs/fn[helper].json";
    let errors_for = |prefix: &str| {
        errors(root)
            .into_iter()
            .filter(|error| error.starts_with(prefix))
            .collect::<Vec<_>>()
    };
    let missing = errors_for("evidence/citations/");
    assert!(
        has(
            &missing,
            "evidence/citations/src/lib.rs/fn[helper].json: missing citation review for src/lib.rs fn[helper] at src/lib.rs:1"
        ),
        "{missing:#?}"
    );
    assert!(
        has(
            &missing,
            "evidence/citations/notes.txt/_root.json: missing citation review for notes.txt <root>"
        ),
        "{missing:#?}"
    );
    assert!(!has(&missing, "md/guide.md"), "{missing:#?}");

    let citations = json!([
        {"role": "cite", "name": "rules.ready", "commentary": ""},
        {"role": "cite", "name": "habeas:util", "commentary": "Formats output."}
    ]);
    fs::create_dir_all(root.join("evidence/citations/src/lib.rs")).unwrap();
    let write = |record: Value| fs::write(root.join(path), record.to_string()).unwrap();
    write(review(root, citations.clone(), json!(true)));
    assert_eq!(errors_for(path), Vec::<String>::new());

    // A citation review cannot accept a `{fixme}`, so FIXME is reported; a
    // false review is reported with its rationale.
    write(review(
        root,
        citations.clone(),
        json!({"FIXME": ["example/rules#4"]}),
    ));
    assert!(
        has(
            &errors_for(path),
            "determination is FIXME, but a citation review cannot accept a"
        ),
        "{:#?}",
        errors_for(path)
    );
    write(review(root, citations.clone(), json!(false)));
    assert!(
        has(
            &errors_for(path),
            "determination is false: The citations are sensible."
        ),
        "{:#?}",
        errors_for(path)
    );

    // Changed commentary or citations make the review stale; reflowing does not.
    write(review(root, citations.clone(), json!(true)));
    fs::write(root.join("src/lib.rs"), helper("Formats  output.")).unwrap();
    assert_eq!(errors_for(path), Vec::<String>::new());
    fs::write(root.join("src/lib.rs"), helper("Formats all output.")).unwrap();
    assert!(
        has(&errors_for(path), "citations changed"),
        "{:#?}",
        errors_for(path)
    );
    fs::write(root.join("src/lib.rs"), helper("Formats output.")).unwrap();

    // So do an edited item and an edited cited declaration.
    fs::write(
        root.join("src/lib.rs"),
        helper("Formats output.").replace("work()", "rest()"),
    )
    .unwrap();
    assert!(
        has(&errors_for(path), "outline input changed"),
        "{:#?}",
        errors_for(path)
    );
    fs::write(root.join("src/lib.rs"), helper("Formats output.")).unwrap();
    fs::write(
        root.join("md/rules.md"),
        ":::{req} ready\nReady now.\n:::\n",
    )
    .unwrap();
    assert!(
        has(&errors_for(path), "input text changed"),
        "{:#?}",
        errors_for(path)
    );
    fs::write(root.join("md/rules.md"), ":::{req} ready\nReady.\n:::\n").unwrap();

    // The item and each cited declaration must be inputs.
    let mut record = review(root, citations.clone(), json!(true));
    record["inputs"] = json!([]);
    write(record);
    let found = errors_for(path);
    assert!(
        has(&found, "inputs must select the reviewed item"),
        "{found:#?}"
    );
    assert!(
        has(
            &found,
            "inputs must include the cited declaration rules.ready"
        ),
        "{found:#?}"
    );
    let mut record = review(root, citations.clone(), json!(true));
    record["extra"] = json!(1);
    write(record);
    assert!(has(&errors_for(path), "unexpected field \"extra\""));

    // Removing the citations orphans the review.
    write(review(root, citations, json!(true)));
    fs::write(
        root.join("src/lib.rs"),
        format!("// {}`rules.ready`\nfn helper() {{}}\n", role("impl")),
    )
    .unwrap();
    assert!(
        has(&errors_for(path), "orphaned citation review"),
        "{:#?}",
        errors_for(path)
    );
    // The review is not mistaken for misplaced definition evidence.
    assert!(!has(&errors(root), "no matching requirement or example"));
}

// {test}`requirements.references.citation_review`
#[test]
fn review_paths_encode_selectors() {
    let temp = project();
    let root = temp.path();
    let cr = role("cite");
    fs::write(
        root.join("src/lib.rs"),
        format!("struct Thing;\nimpl Thing {{\n    // {cr}`rules.ready`\n    fn run() {{}}\n}}\n"),
    )
    .unwrap();
    let slashed = errors(root);
    assert!(
        has(
            &slashed,
            "evidence/citations/src/lib.rs/impl[Thing]%2Ffn[run].json: missing citation review for src/lib.rs impl[Thing]/fn[run]"
        ),
        "{slashed:#?}"
    );

    // A `%` already in the selector is encoded before `/`, so it is not
    // mistaken for an encoded `/`.
    fs::write(
        root.join("src/lib.rs"),
        format!(
            "impl [u8; 4] {{
    // {cr}`rules.ready`
    fn edge() {{}}
}}
"
        ),
    )
    .unwrap();
    let encoded = errors(root);
    assert!(
        has(
            &encoded,
            "evidence/citations/src/lib.rs/impl[%255Bu8;%25204%255D]%2Ffn[edge].json: missing citation review for src/lib.rs impl[%5Bu8;%204%5D]/fn[edge]"
        ),
        "{encoded:#?}"
    );
}

/// Runs `git` in `root`, failing the test if it fails.
// {cite}`habeas:util`
fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// {test}`requirements.references.discovery`
// {test}`requirements.references.rust_item_coverage`
// {test}`requirements.cli.status`
#[test]
fn untracked_sources_are_not_validated_until_staged_and_status_lists_them() {
    let temp = project();
    let root = temp.path();
    let source = format!(
        "// {}`rules.missing`\nfn referenced() {{}}\n\nfn uncited() {{}}\n",
        role("impl")
    );
    fs::write(root.join("src/extra.rs"), &source).unwrap();
    // An untracked file without references needs no reminder.
    fs::write(root.join("notes.txt"), "No references here.\n").unwrap();
    git(root, &["init", "--quiet"]);
    git(root, &["add", "md", "book.toml", "Cargo.toml"]);

    let untracked = errors(root);
    assert!(!has(&untracked, "src/extra.rs"), "{untracked:#?}");
    let status = habeas::status_markdown(root, false).unwrap();
    let section = &status[status
        .find("## Untracked files with references")
        .unwrap_or_else(|| panic!("{status}"))..];
    assert!(section.contains("git add"), "{status}");
    assert!(section.contains("- `src/extra.rs`"), "{status}");
    assert!(!section.contains("notes.txt"), "{status}");

    // Staging the new file brings its references and items back.
    git(root, &["add", "src/extra.rs"]);
    let errors = errors(root);
    assert!(has(&errors, "src/extra.rs:1"), "{errors:#?}");
    assert!(has(&errors, "rules.missing"), "{errors:#?}");
    assert!(
        has(
            &errors,
            "src/extra.rs:4: function uncited is uncovered: it has no requirement or example reference, and no evidence record or review read it"
        ),
        "{errors:#?}"
    );
    let status = habeas::status_markdown(root, false).unwrap();
    assert!(
        !status.contains("Untracked files with references"),
        "{status}"
    );
}

// {test}`requirements.references.discovery`
// {test}`requirements.references.rust_item_coverage`
// {test}`requirements.configuration.evidence_paths`
#[test]
fn disallowed_paths_are_skipped_and_projects_outside_git_scan_every_file() {
    let temp = project();
    let root = temp.path();
    let source = format!("// {}`rules.missing`\nfn uncited() {{}}\n", role("impl"));
    for file in [
        "private/hidden.rs",
        "src/secret.rs",
        "notes/other.rs",
        "vendor/dep/lib.rs",
        "crates/sub/target/out.rs",
        "book/out.rs",
    ] {
        let path = root.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &source).unwrap();
    }
    fs::write(
        root.join("habeas.toml"),
        "version = 1\n\n[evidence]\ndisallow = [\"private\", \"src/secret.rs\"]\n",
    )
    .unwrap();
    let errors = errors(root);
    for skipped in [
        "private/hidden.rs",
        "src/secret.rs",
        "crates/sub/target/out.rs",
        "book/out.rs",
    ] {
        assert!(!has(&errors, skipped), "{skipped}: {errors:#?}");
    }
    // Outside Git every other project file is considered, untracked or not;
    // vendored Rust sources still carry references but are not covered.
    assert!(has(&errors, "notes/other.rs:1"), "{errors:#?}");
    assert!(
        has(&errors, "notes/other.rs:2: function uncited"),
        "{errors:#?}"
    );
    assert!(has(&errors, "vendor/dep/lib.rs:1"), "{errors:#?}");
    assert!(
        !has(&errors, "vendor/dep/lib.rs:2: function uncited"),
        "{errors:#?}"
    );
    let status = habeas::status_markdown(root, false).unwrap();
    assert!(
        !status.contains("Untracked files with references"),
        "{status}"
    );
}

// {test}`requirements.references.rust_item_coverage`
// {test}`requirements.references.procedure`
#[test]
fn items_an_assessment_read_with_their_current_hash_are_covered() {
    let temp = project();
    let root = temp.path();
    let source = "fn read_directly() {}\n\nstruct Thing;\n\nimpl Thing {\n    fn method(&self) {}\n}\n\nfn changed() {}\n\nfn unread() {}\n";
    fs::write(root.join("src/lib.rs"), source).unwrap();
    let input = |item: &str| habeas::outline_input(root, "src/lib.rs", item).unwrap();
    let mut stale = input("fn[changed]");
    stale["hash"] = json!("0".repeat(64));
    // An ordinary record reads one item; a citation review reads the impl
    // block containing a method; a site review records a stale hash.
    fs::write(
        root.join("evidence/rules.ready.json"),
        json!({"inputs": [input("fn[read_directly]")]}).to_string(),
    )
    .unwrap();
    fs::create_dir_all(root.join("evidence/citations/src/lib.rs")).unwrap();
    fs::write(
        root.join("evidence/citations/src/lib.rs/struct[Thing].json"),
        json!({"inputs": [input("impl[Thing]"), input("struct[Thing]")]}).to_string(),
    )
    .unwrap();
    fs::create_dir_all(root.join("evidence/sites/rules.ready/src/lib.rs")).unwrap();
    fs::write(
        root.join("evidence/sites/rules.ready/src/lib.rs/fn[changed].json"),
        json!({"inputs": [stale]}).to_string(),
    )
    .unwrap();
    let uncovered = |errors: &[String]| {
        errors
            .iter()
            .filter(|error| error.contains(" is uncovered: "))
            .map(|error| error.split(':').take(3).collect::<Vec<_>>().join(":"))
            .collect::<Vec<_>>()
    };
    let found = errors(root);
    assert_eq!(
        uncovered(&found),
        [
            "src/lib.rs:9: function changed is uncovered",
            "src/lib.rs:11: function unread is uncovered"
        ],
        "{found:#?}"
    );
    // Editing a read item leaves its recorded hash stale, so it is no
    // longer covered; so does editing a method inside a read impl block.
    fs::write(
        root.join("src/lib.rs"),
        source
            .replace("fn read_directly() {}", "fn read_directly() { 1; }")
            .replace("fn method(&self) {}", "fn method(&self) { 2; }"),
    )
    .unwrap();
    let found = errors(root);
    assert_eq!(
        uncovered(&found),
        [
            "src/lib.rs:1: function read_directly is uncovered",
            "src/lib.rs:6: function method is uncovered",
            "src/lib.rs:9: function changed is uncovered",
            "src/lib.rs:11: function unread is uncovered"
        ],
        "{found:#?}"
    );
}

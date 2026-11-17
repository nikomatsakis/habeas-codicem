//! Current source explanations, kept distinct from retained execution reports.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mdbook_preprocessor::book::Chapter;

use crate::citations::{CitationItem, citation_items, validate_review};
use crate::evidence::Declaration;
use crate::inputs::{OutlineCache, ParsedSource};

// {cite}`habeas:util`
fn escaped(text: &str) -> String {
    crate::escape_html(text).replace('"', "&quot;")
}

pub(super) fn path(index: &Path, file: &str) -> PathBuf {
    index
        .parent()
        .unwrap_or(Path::new(""))
        .join("coverage-reports/files")
        .join(format!("{}.md", super::digest(file.as_bytes())))
}

// {impl}`requirements.test-coverage.index.file_map.notes`
// {impl}`requirements.test-coverage.index.file_map.notes.justifications`
// {impl}`requirements.test-coverage.index.file_map.notes.current`
// {impl}`requirements.test-coverage.index.file_map.notes.availability`
// {impl}`requirements.test-coverage.index.file_map.notes.reviews`
// {impl}`requirements.test-coverage.index.file_map.notes.render_only`
pub(super) fn pages(root: &Path, files: &BTreeSet<String>, index: &Path) -> Vec<Chapter> {
    let outlines = OutlineCache::default();
    let mut diagnostics = Vec::new();
    let references = crate::evidence::collect_references(root, &mut diagnostics);
    let definitions = crate::evidence::load_definitions(root).unwrap_or_else(|error| {
        diagnostics.push(error.to_string());
        BTreeMap::new()
    });
    let items = citation_items(root, &definitions, &references, &outlines);
    files
        .iter()
        .map(|file| {
            let path = path(index, file);
            let mut content = format!(
                "# Coverage explanations\n\n[Back to coverage index]({})\n\nFile: <code>{}</code>.\n\n**Current workspace source notes**, read at book-render time. These explanations and review states describe the current source, including when reached from a historical test report; they do not describe the captured source of that historical run.\n\n",
                crate::chapter_href(&path, index, None), escaped(file),
            );
            match outlines.parse(root, file).as_ref() {
                Err(error) => diagnostic(&mut content, error),
                Ok(parsed) => {
                    for error in &diagnostics {
                            diagnostic(&mut content, error);
                        }
                        let mut count = 0;
                        for item in items.values().filter(|item| item.file == *file) {
                            let Ok(node) = parsed.resolve(file, &item.item) else {
                                diagnostic(&mut content, "Cannot resolve the cited source item.");
                                continue;
                            };
                            if !(item.item.is_empty() || parsed.outline.nodes[node].kind == "fn")
                                || !item.citations.iter().any(|cite| cite.name == "habeas:coverage")
                            {
                                continue;
                            }
                            count += 1;
                            entry(&mut content, root, parsed, node, item, &definitions, &outlines);
                        }
                    if count == 0 && diagnostics.is_empty() {
                            content.push_str("No coverage explanations are present on this file's root or explicitly declared functions and methods.\n");
                        }
                },
            }
            let mut chapter = Chapter::new(&format!("File notes: {file}"), content, path, vec![]);
            chapter.source_path = None;
            chapter
        })
        .collect()
}

fn diagnostic(content: &mut String, error: &str) {
    content.push_str(&format!(
        "**Coverage notes unavailable:** {}\n\n",
        escaped(error),
    ));
}

fn doc_comments(parsed: &ParsedSource, owner: usize) -> String {
    parsed
        .outline
        .comments
        .iter()
        .filter(|comment| comment.owner == owner)
        .filter_map(|comment| {
            let text = &parsed.source[comment.byte_start..comment.byte_end];
            if let Some(text) = text
                .strip_prefix("///")
                .or_else(|| text.strip_prefix("//!"))
            {
                Some(text.trim_start().to_owned())
            } else {
                text.strip_prefix("/**")
                    .or_else(|| text.strip_prefix("/*!"))
                    .map(|text| {
                        text.trim_end_matches("*/")
                            .lines()
                            .map(|line| line.trim().trim_start_matches('*').trim_start())
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn entry(
    content: &mut String,
    root: &Path,
    parsed: &ParsedSource,
    node: usize,
    item: &CitationItem,
    definitions: &BTreeMap<String, Declaration>,
    outlines: &OutlineCache,
) {
    let label = if item.item.is_empty() {
        "File root".to_owned()
    } else {
        item.item.clone()
    };
    let scope = if item.item.is_empty() {
        "File-threshold exception proposal (80% instrumented-line target)"
    } else {
        "Function exception proposal (zero execution or not measured)"
    };
    let source = &parsed.outline.nodes[node];
    content.push_str(&format!(
        "## <code>{}</code>\n\n{scope}.\n\nSource: <code>{}:{}–{}</code>. First source citation at line {}.\n\n",
        escaped(&label),
        escaped(&item.file),
        source.line_start,
        source.line_end,
        item.line,
    ));
    let commentary = item
        .citations
        .iter()
        .filter(|cite| cite.name == "habeas:coverage")
        .map(|cite| cite.commentary.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let docs = doc_comments(parsed, node);
    for (heading, text) in [
        ("Citation commentary", &commentary),
        ("Rust doc comments", &docs),
    ] {
        if !text.trim().is_empty() {
            let text = crate::redaction::redact_text(text);
            content.push_str(&format!(
                "### {heading}\n\n<pre style=\"white-space:pre-wrap;overflow-wrap:anywhere\">{}</pre>\n\n",
                escaped(&text),
            ));
        }
    }
    if commentary.is_empty() && docs.trim().is_empty() {
        content.push_str("**No tracked justification is present.**\n\n");
    }
    let review = item.review_path();
    let mut findings = Vec::new();
    validate_review(root, &review, item, definitions, outlines, &mut findings);
    if findings.is_empty() {
        content.push_str("Review: **current and accepted**.\n\n");
    } else {
        let state = if root
            .join(&review)
            .symlink_metadata()
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        {
            "missing"
        } else {
            "not accepted"
        };
        content.push_str(&format!("Review: **{state}**.\n\n"));
        for finding in findings {
            content.push_str(&format!("- {}\n", escaped(&finding)));
        }
        content.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn fixture(source: &str) -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("md")).unwrap();
        fs::write(temp.path().join("book.toml"), "[book]\nsrc='md'\n").unwrap();
        fs::write(temp.path().join("md/SUMMARY.md"), "# Summary\n").unwrap();
        fs::write(temp.path().join("code.rs"), source).unwrap();
        temp
    }

    fn render(root: &Path) -> String {
        let chapters = pages(
            root,
            &BTreeSet::from(["code.rs".into()]),
            Path::new("reports/index.md"),
        );
        assert_eq!(chapters.len(), 1);
        assert_eq!(
            chapters[0].path.as_ref().unwrap(),
            &path(Path::new("reports/index.md"), "code.rs")
        );
        chapters.into_iter().next().unwrap().content
    }

    // Synthetic fixture reviews only; production evidence is model-authored.
    fn write_reviews(root: &Path, determination: bool) -> Vec<PathBuf> {
        let outlines = OutlineCache::default();
        let mut errors = Vec::new();
        let references = crate::evidence::collect_references(root, &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
        citation_items(root, &BTreeMap::new(), &references, &outlines)
            .into_values()
            .map(|item| {
                let parsed = outlines.parse(root, &item.file);
                let parsed = parsed.as_ref().as_ref().unwrap();
                let node = parsed.resolve(&item.file, &item.item).unwrap();
                let path = root.join(item.review_path());
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(
                    &path,
                    serde_json::to_vec(&json!({
                        "file": item.file, "item": item.item, "citations": item.citations_value(),
                        "determination": determination,
                        "rationale": "Synthetic review outcome for this test fixture.",
                        "inputs": [{"file": item.file, "item": item.item,
                            "hash": parsed.outline.nodes[node].structural_hash}]
                    }))
                    .unwrap(),
                )
                .unwrap();
                path
            })
            .collect()
    }

    // {test}`requirements.test-coverage.index.file_map.notes`
    // {test}`requirements.test-coverage.index.file_map.notes.justifications`
    #[test]
    fn notes_show_root_function_and_method_proposals_with_owned_commentary_and_docs() {
        let temp = fixture(
            "//! Root documentation.\n\
             //! :::{cite} habeas:coverage\n\
             //! File threshold justification.\n\
             //! :::\n\n\
             /// Function documentation.\n\
             // :::{cite} habeas:coverage\n\
             // Function explanation with <unsafe> text.\n\
             // :::\n\
             fn ordinary() {}\n\n\
             struct Worker;\n\
             impl Worker {\n\
               /** Method documentation. */\n\
               // :::{cite} habeas:coverage\n\
               // Method explanation.\n\
               // :::\n\
               fn work(&self) {}\n\
             }\n\n\
             // {cite}`habeas:coverage`\n\
             fn unexplained() {}\n\n\
             // :::{cite} habeas:coverage\n\
             // Nested module must not become a file exception.\n\
             // :::\n\
             mod nested {\n\
               /// Child documentation must stay with the child.\n\
               fn child() {}\n\
             }\n",
        );
        let content = render(temp.path());
        for text in [
            "File: <code>code.rs</code>",
            "File root",
            "File-threshold exception proposal",
            "fn[ordinary]",
            "impl[Worker]/fn[work]",
            "Function exception proposal",
            "Root documentation.",
            "File threshold justification.",
            "Function documentation.",
            "Function explanation with &lt;unsafe&gt; text.",
            "Method documentation.",
            "Method explanation.",
            "fn[unexplained]",
            "No tracked justification is present.",
            "Source: <code>code.rs:",
            "First source citation at line",
        ] {
            assert!(content.contains(text), "missing {text}: {content}");
        }
        assert_eq!(
            content.matches("File-threshold exception proposal").count(),
            1
        );
        assert_eq!(content.matches("Function exception proposal").count(), 3);
        assert!(!content.contains("Nested module must"));
        assert!(!content.contains("Child documentation"));
        assert!(!content.contains("<unsafe>"));
        assert_eq!(content.matches("Review: **missing**").count(), 4);
        assert_eq!(
            content
                .matches("No tracked justification is present.")
                .count(),
            1
        );
        assert!(content.contains("cannot read citation review"));
        let unexplained = content
            .split("## <code>fn[unexplained]")
            .nth(1)
            .unwrap()
            .split("## <code>")
            .next()
            .unwrap();
        assert!(!unexplained.contains("Rust doc comments"));
        assert!(unexplained.contains("No tracked justification is present."));
        for (label, first, last, citation) in [
            ("File root", 1, 30, 2),
            ("fn[ordinary]", 6, 10, 7),
            ("impl[Worker]/fn[work]", 14, 18, 15),
            ("fn[unexplained]", 21, 22, 21),
        ] {
            let heading = format!("## <code>{label}</code>");
            let entry = content
                .split(&heading)
                .nth(1)
                .unwrap()
                .split("## <code>")
                .next()
                .unwrap();
            assert!(
                entry.contains(&format!("Source: <code>code.rs:{first}–{last}</code>")),
                "{label}: {entry}"
            );
            assert!(
                entry.contains(&format!("First source citation at line {citation}.")),
                "{label}: {entry}"
            );
        }
    }

    // {test}`requirements.test-coverage.index.file_map.notes.justifications`
    #[test]
    fn justification_can_come_from_docs_or_fenced_commentary_but_not_ordinary_comments() {
        for (source, text, heading, justified) in [
            (
                "/// Documentation alone explains the platform gap.\n// {cite}`habeas:coverage`\nfn work() {}\n",
                "Documentation alone",
                "Rust doc comments",
                true,
            ),
            (
                "// :::{cite} habeas:coverage\n// Fenced commentary alone explains the platform gap.\n// :::\nfn work() {}\n",
                "Fenced commentary alone",
                "Citation commentary",
                true,
            ),
            (
                "// Ordinary comments are not tracked justification.\n// {cite}`habeas:coverage`\nfn work() {}\n",
                "Ordinary comments",
                "",
                false,
            ),
        ] {
            let temp = fixture(source);
            let content = render(temp.path());
            let entry = content.split("## <code>fn[work]").nth(1).unwrap();
            assert_eq!(
                entry.contains("No tracked justification is present."),
                !justified,
                "{entry}"
            );
            assert_eq!(entry.contains(text), justified, "{entry}");
            if justified {
                assert!(entry.contains(&format!("### {heading}")));
            }
        }
    }

    // {test}`requirements.test-coverage.index.file_map.notes.availability`
    #[test]
    fn absent_citations_are_distinct_from_missing_unreadable_and_unparseable_source() {
        let temp = fixture("fn ordinary() {}\n");
        let empty = render(temp.path());
        assert!(empty.contains("No coverage explanations are present"));
        assert!(!empty.contains("Coverage notes unavailable"));
        for source in ["fn invalid( {", "fn invalid() { let = ; }"] {
            fs::write(temp.path().join("code.rs"), source).unwrap();
            let invalid = render(temp.path());
            assert!(invalid.contains("Coverage notes unavailable"));
            assert!(invalid.contains("syntax error"), "{invalid}");
            assert!(!invalid.contains("No coverage explanations are present"));
        }
        fs::remove_file(temp.path().join("code.rs")).unwrap();
        let missing = render(temp.path());
        assert!(missing.contains("Coverage notes unavailable"));
        assert!(!missing.contains("No coverage explanations are present"));
        fs::create_dir(temp.path().join("code.rs")).unwrap();
        let unreadable = render(temp.path());
        assert!(unreadable.contains("Coverage notes unavailable"));
        assert!(!unreadable.contains("No coverage explanations are present"));
        let chapters = pages(
            temp.path(),
            &BTreeSet::from(["code.rs".into(), "missing.rs".into()]),
            Path::new("coverage.md"),
        );
        assert_eq!(chapters.len(), 2); // one failure does not abort other chapters
        assert!(
            chapters
                .iter()
                .all(|chapter| chapter.content.contains("Coverage notes unavailable"))
        );
    }

    // {test}`requirements.test-coverage.index.file_map.notes.availability`
    // {test}`requirements.test-coverage.index.file_map.notes.justifications`
    #[test]
    fn citations_on_modules_and_citation_text_in_literals_do_not_create_explanations() {
        let temp = fixture(
            "// :::{cite} habeas:coverage\n// Module only.\n// :::\n\
             mod nested { fn inside() {} }\n\
             fn ordinary() { let text = \"{cite}`habeas:coverage`\"; }\n",
        );
        let content = render(temp.path());
        assert!(content.contains("No coverage explanations are present"));
        assert!(!content.contains("exception proposal"));
        assert!(!content.contains("Module only."));
    }

    // {test}`requirements.test-coverage.index.file_map.notes.reviews`
    #[test]
    fn notes_distinguish_accepted_missing_rejected_stale_invalid_and_unreadable_reviews() {
        let source = "// :::{cite} habeas:coverage\n// Only supported on another platform.\n// :::\nfn other() { let value = 1; }\n";
        for state in [
            "accepted",
            "missing",
            "rejected",
            "stale",
            "invalid",
            "unreadable",
        ] {
            let temp = fixture(source);
            let reviews = write_reviews(temp.path(), state != "rejected");
            assert_eq!(reviews.len(), 1);
            let review = &reviews[0];
            match state {
                "missing" => fs::remove_file(review).unwrap(),
                "stale" => fs::write(
                    temp.path().join("code.rs"),
                    source.replace("value = 1", "value = 2"),
                )
                .unwrap(),
                "invalid" => fs::write(review, "not JSON").unwrap(),
                "unreadable" => {
                    fs::remove_file(review).unwrap();
                    fs::create_dir(review).unwrap();
                }
                _ => {}
            }
            let content = render(temp.path());
            let expected = match state {
                "accepted" => "Review: **current and accepted**",
                "missing" => "Review: **missing**",
                _ => "Review: **not accepted**",
            };
            assert!(content.contains(expected), "{state}: {content}");
            let finding = match state {
                "rejected" => "determination is false",
                "stale" => "hash",
                "invalid" => "invalid JSON",
                "unreadable" | "missing" => "cannot read citation review",
                _ => "current and accepted",
            };
            assert!(content.contains(finding), "{state}: {content}");
            if state != "accepted" {
                assert!(
                    !content.contains("current and accepted"),
                    "{state}: {content}"
                );
            }
        }
    }

    // {test}`requirements.test-coverage.index.file_map.notes.current`
    // {test}`requirements.test-coverage.index.file_map.notes.render_only`
    #[test]
    fn each_render_reads_current_notes_and_validates_saved_reviews_without_writing() {
        let source = "// :::{cite} habeas:coverage\n// Original platform explanation.\n// :::\nfn other() {}\n";
        let temp = fixture(source);
        let reviews = write_reviews(temp.path(), true);
        let saved_review = fs::read(&reviews[0]).unwrap();
        assert!(render(temp.path()).contains("current and accepted"));
        fs::write(
            temp.path().join("code.rs"),
            source.replace("Original platform", "Updated platform"),
        )
        .unwrap();
        let content = render(temp.path());
        assert!(content.contains("Updated platform explanation."));
        assert!(
            !content
                .split("Review:")
                .next()
                .unwrap()
                .contains("Original platform explanation.")
        );
        assert!(content.contains("Current workspace source notes"));
        assert!(content.contains("Review: **not accepted**"));
        assert!(content.contains("citations changed"));
        assert_eq!(fs::read(&reviews[0]).unwrap(), saved_review);
        assert!(!temp.path().join("target").exists());
    }
}

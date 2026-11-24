use std::sync::Arc;
use trust_but_verify::domain::NodeContents;
use trust_but_verify::outline::Outline;
use trust_but_verify::outline::OutlineContent;
use trust_but_verify::outline::OutlineError;
use trust_but_verify::outline::OutlineId;
use trust_but_verify::outline::OutlineItem;
use trust_but_verify::outline::OutlineLang;
use trust_but_verify::outline::Span;
use trust_but_verify::outline::languages;

// {test}`requirements.sdk.outlines.Span`
#[test]
fn span_indexes_str_at_utf8_byte_boundaries() {
    let source = "aé🦀";
    assert_eq!(&source[Span::new(1, 3)], "é");
    assert_eq!(&source[Span::new(7, 7)], "");
    for span in [
        Span::new(2, 3),
        Span::new(0, 2),
        Span::new(2, 2),
        Span::new(0, 8),
        Span::new(3, 1),
    ] {
        assert!(std::panic::catch_unwind(|| &source[span]).is_err());
    }
}

// {test}`requirements.sdk.outlines.address_identity`
#[test]
fn addresses_reconstruct_without_reading_the_file() {
    let id = OutlineId::new("not-present.rs", "mod[auth]/fn[check]".into()).unwrap();
    let root = OutlineId::root("not-present.rs");
    assert!(root.selector.is_empty());
    assert_eq!(root.clone().selector(id.selector.clone()).unwrap(), id);
    assert!(root.selector("fn[".into()).is_err());
    let encoded = serde_json::to_string(&id).unwrap();
    assert_eq!(serde_json::from_str::<OutlineId>(&encoded).unwrap(), id);
    assert_eq!(id.path, std::path::Path::new("not-present.rs"));
    assert_eq!(id.selector, "mod[auth]/fn[check]");
    assert!(OutlineId::new("not-present.rs", "fn[".into()).is_err());
    assert_ne!(
        id,
        OutlineId::new("different.rs", "mod[auth]/fn[check]".into()).unwrap()
    );
    assert_ne!(
        id,
        OutlineId::new("not-present.rs", "mod[auth]".into()).unwrap()
    );
}

// {test}`requirements.sdk.outlines.OutlineError`
#[test]
fn outline_errors_preserve_diagnostic_and_line() {
    fn assert_eq_impl<T: Eq>() {}
    assert_eq_impl::<OutlineError>();
    let error = OutlineError::new(7, "bad selector".into());
    assert_eq!(error.line, 7);
    assert_eq!(error.message, "bad selector");
    assert_eq!(error, error.clone());
    assert_ne!(error, OutlineError::new(8, error.message.clone()));
    assert_ne!(error, OutlineError::new(7, "different diagnostic".into()));
    let reported: &dyn std::error::Error = &error;
    assert_eq!(reported.to_string(), "line 7: bad selector");
}

// {test}`requirements.sdk.outlines.parser.supplied_source`
// {test}`requirements.sdk.outlines.defaults`
// {test}`requirements.sdk.outlines.items`
// {test}`requirements.sdk.outlines.selection`
// {test}`requirements.sdk.sources.plain-text.items`
// {test}`requirements.sdk.sources.plain-text.fingerprints`
// {test}`requirements.sdk.sources.plain-text.PlainText`
#[test]
fn unknown_extensions_keep_nonblank_lines_and_content_selectors() {
    let first =
        OutlineContent::from_source("missing.unclaimed", "alpha\n\nbeta\nalpha\n".into()).unwrap();
    assert_eq!(first.id().selector, "");
    assert_eq!(first.outline().items.len(), 4);
    assert_eq!(first.outline().items[0].span, Span::new(0, 18));
    let alpha = &first.outline().items[1];
    assert_eq!(alpha.kind, "line");
    assert_eq!(alpha.selector, first.outline().items[3].selector);
    assert_ne!(alpha.selector, first.outline().items[2].selector);
    assert_eq!(
        first.outline().items[1..]
            .iter()
            .map(|item| &first.text()[item.span])
            .collect::<Vec<_>>(),
        ["alpha", "beta", "alpha"]
    );
    for item in &first.outline().items[1..] {
        assert_eq!(item.kind, "line");
        let prefix = item.selector.strip_prefix("line@").unwrap();
        assert_eq!(prefix.len(), 12);
        assert!(prefix.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(prefix, &item.structural_hash[..12]);
    }
    assert_eq!(first.select(&alpha.selector).unwrap().text(), "alpha");
    let reflow =
        OutlineContent::from_source("missing.unclaimed", "\n alpha \n beta\nalpha\n".into())
            .unwrap();
    assert_eq!(first.fingerprint(), reflow.fingerprint());
    assert_eq!(alpha.selector, reflow.outline().items[1].selector);
    let reordered =
        OutlineContent::from_source("missing.unclaimed", "beta\nalpha\nalpha\n".into()).unwrap();
    assert_ne!(first.fingerprint(), reordered.fingerprint());
    let joined =
        OutlineContent::from_source("missing.unclaimed", "alpha beta\nalpha\n".into()).unwrap();
    assert_ne!(first.fingerprint(), joined.fingerprint());
    let empty = OutlineContent::from_source("missing", Arc::from("")).unwrap();
    assert_eq!(empty.outline().items.len(), 1);
    assert_eq!(empty.text(), "");
    assert_eq!(empty.outline().items[0].span, Span::new(0, 0));
}

// {test}`requirements.sdk.outlines.parser`
// {test}`requirements.sdk.outlines.independent`
// {test}`requirements.sdk.outlines.items`
// {test}`requirements.sdk.outlines.selection`
// {test}`requirements.sdk.outlines.snapshot`
// {test}`requirements.sdk.outlines.node_contents`
// {test}`requirements.sdk.graph.NodeContents`
#[test]
fn selected_contents_outlive_handles_and_later_file_edits() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.unknown");
    std::fs::write(&path, "first\nsecond\n").unwrap();
    let file = OutlineContent::from_path(&path).unwrap();
    let id = OutlineId::new(path.clone(), file.outline().items[2].selector.clone()).unwrap();
    let selected = OutlineContent::from_id(&id).unwrap();
    let selected_again = file.select(&id.selector).unwrap();
    assert_eq!(selected.id(), &id);
    assert_eq!(selected_again.id(), &id);
    assert_eq!(selected.text(), "second");
    assert_eq!(selected.format(), "second");
    assert_eq!(selected.structural_hash(), selected.fingerprint().value);
    assert_eq!(selected.fingerprint_scheme(), selected.fingerprint().scheme);
    assert_eq!(selected.outline().items.len(), 3);
    let original = selected.fingerprint();
    drop(file);
    std::fs::write(&path, "changed\n").unwrap();
    assert_eq!(selected.fingerprint(), original);
    assert_eq!(selected_again.text(), "second");
    assert_eq!(selected.format(), "second");
    assert!(OutlineContent::from_id(&id).is_err());
    let retained_root = selected.select("").unwrap();
    assert_eq!(retained_root.id(), &OutlineId::root(path));
    assert_eq!(retained_root.text(), "first\nsecond\n");
    assert!(format!("{selected:?}").contains("OutlineContent"));
}

// {test}`requirements.sdk.outlines.errors`
// {test}`requirements.sdk.outlines.selection`
#[test]
fn missing_file_and_invalid_selections_are_errors() {
    let directory = tempfile::tempdir().unwrap();
    assert!(OutlineContent::from_path(directory.path().join("missing")).is_err());
    let file = OutlineContent::from_source("empty", "".into()).unwrap();
    assert!(file.select("fn[").is_err());
    assert!(file.select("fn[absent]").is_err());
    assert!(OutlineContent::from_path(directory.path()).is_err());
}

#[derive(Clone)]
struct CustomLanguage {
    configuration: Arc<String>,
    span: Span,
}

impl OutlineLang for CustomLanguage {
    fn name(&self) -> &str {
        "custom"
    }

    fn extensions(&self) -> &[&str] {
        &["custom"]
    }

    fn outline(&self, source: &str) -> Result<Outline, OutlineError> {
        if source == "fail" {
            return Err(OutlineError::new(1, "custom parse failure".into()));
        }
        Ok(Outline::new(vec![
            OutlineItem::new(
                String::new(),
                String::new(),
                Span::new(0, source.len()),
                source.to_owned(),
            ),
            OutlineItem::new(
                "part[selected]".into(),
                "part".into(),
                self.span,
                "selected-digest".into(),
            )
            .with_name_span(self.span),
        ]))
    }

    fn fingerprint_scheme(&self) -> &str {
        &self.configuration
    }
}

// {test}`requirements.sdk.outlines.language_provider`
// {test}`requirements.sdk.outlines.additional_languages`
// {test}`requirements.sdk.outlines.language_provider.fingerprint_scheme`
// {test}`requirements.sdk.outlines.selection`
// {test}`requirements.sdk.outlines.snapshot`
#[test]
fn explicit_languages_share_configuration_and_retain_their_scheme() {
    let scheme = Arc::new("custom-v2".to_owned());
    let language = CustomLanguage {
        configuration: scheme.clone(),
        span: Span::new(2, 7),
    };
    let cloned = language.clone();
    assert!(Arc::ptr_eq(&language.configuration, &cloned.configuration));
    assert_eq!(language.name(), "custom");
    assert_eq!(language.extensions(), &["custom"]);
    let supplied =
        OutlineContent::from_source_with(&language, "misleading.rs", "a hello z".into()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("misleading.rs");
    std::fs::write(&path, "a hello z").unwrap();
    let loaded = OutlineContent::from_path_with(&cloned, &path).unwrap();
    for root in [&loaded, &supplied] {
        assert_eq!(root.id().selector, "");
        assert_eq!(root.text(), "a hello z");
        assert_eq!(root.outline().items[0].span, Span::new(0, 9));
        assert_eq!(root.fingerprint().value, "a hello z");
        assert_eq!(root.fingerprint().scheme, "custom-v2");
    }
    drop(language);
    drop(cloned);
    drop(scheme);
    assert_eq!(supplied.fingerprint_scheme(), "custom-v2");
    let selected = supplied.select("part[selected]").unwrap();
    assert_eq!(selected.text(), "hello");
    assert_eq!(selected.structural_hash(), "selected-digest");
    assert_eq!(
        loaded.select("part[selected]").unwrap().fingerprint(),
        selected.fingerprint()
    );
    assert_ne!(supplied.fingerprint(), selected.fingerprint());
    let newer_language = CustomLanguage {
        configuration: Arc::new("custom-v3".into()),
        span: Span::new(2, 7),
    };
    let newer =
        OutlineContent::from_source_with(&newer_language, "misleading.rs", "a hello z".into())
            .unwrap()
            .select("part[selected]")
            .unwrap();
    assert_eq!(newer.structural_hash(), selected.structural_hash());
    assert_ne!(newer.fingerprint(), selected.fingerprint());
}

// {test}`requirements.sdk.outlines.errors`
#[test]
fn explicit_language_errors_and_invalid_spans_are_not_plain_text_successes() {
    let language = CustomLanguage {
        configuration: Arc::new("custom-v1".into()),
        span: Span::new(0, 1),
    };
    assert!(
        OutlineContent::from_source_with(&language, "file.txt", "fail".into())
            .unwrap_err()
            .message
            .contains("custom parse failure")
    );
    let invalid = CustomLanguage {
        span: Span::new(0, 99),
        ..language
    };
    assert!(OutlineContent::from_source_with(&invalid, "file.txt", "short".into()).is_err());
    let middle_of_utf8 = CustomLanguage {
        span: Span::new(1, 2),
        ..invalid
    };
    assert!(OutlineContent::from_source_with(&middle_of_utf8, "file.txt", "é".into()).is_err());
}

// {test}`requirements.sdk.outlines.errors`
// {test}`requirements.sdk.outlines.selection`
#[test]
fn ambiguous_items_are_errors_except_for_identical_plain_text_lines() {
    #[derive(Clone)]
    struct Duplicates {
        kind: &'static str,
        different_hashes: bool,
    }
    impl OutlineLang for Duplicates {
        fn name(&self) -> &str {
            "duplicates"
        }
        fn extensions(&self) -> &[&str] {
            &[]
        }
        fn fingerprint_scheme(&self) -> &str {
            "duplicates-v1"
        }
        fn outline(&self, source: &str) -> Result<Outline, OutlineError> {
            let mut items = vec![OutlineItem::new(
                String::new(),
                String::new(),
                Span::new(0, source.len()),
                "root".into(),
            )];
            for (index, span) in [Span::new(0, 4), Span::new(5, 9)].into_iter().enumerate() {
                items.push(
                    OutlineItem::new(
                        format!("{}[same]", self.kind),
                        self.kind.into(),
                        span,
                        if self.different_hashes && index == 1 {
                            "different".into()
                        } else {
                            "same".into()
                        },
                    )
                    .with_name_span(span),
                );
            }
            Ok(Outline::new(items))
        }
    }
    for language in [
        Duplicates {
            kind: "part",
            different_hashes: false,
        },
        Duplicates {
            kind: "line",
            different_hashes: true,
        },
    ] {
        assert_eq!(language.name(), "duplicates");
        assert!(language.extensions().is_empty());
        let contents =
            OutlineContent::from_source_with(&language, "file", "same\nsame".into()).unwrap();
        let error = contents
            .select(&format!("{}[same]", language.kind))
            .unwrap_err();
        assert!(error.message.contains("ambiguous"), "{error}");
    }
    let identical = OutlineContent::from_source("plain", "same\nsame".into()).unwrap();
    assert_eq!(
        identical.outline().items[1].selector,
        identical.outline().items[2].selector
    );
    assert_eq!(
        identical
            .select(&identical.outline().items[1].selector)
            .unwrap()
            .text(),
        "same"
    );
}

// {test}`requirements.sdk.outlines.parser`
// {test}`requirements.sdk.outlines.independent`
#[test]
fn file_constructors_infer_the_language_and_return_addressed_contents() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("AUTH.RS");
    let source = "fn check_token() { allow(); }\n";
    std::fs::write(&path, source).unwrap();
    let root = OutlineContent::from_path(&path).unwrap();
    let supplied = OutlineContent::from_source(&path, source.into()).unwrap();
    assert_eq!(root.id().selector, "");
    assert_eq!(root.text(), source);
    assert_eq!(root.fingerprint(), supplied.fingerprint());
    assert_eq!(root.fingerprint_scheme(), "rust-structural-v1");
    assert_eq!(
        root.outline()
            .items
            .iter()
            .map(|item| item.selector.as_str())
            .collect::<Vec<_>>(),
        ["", "fn[check_token]"]
    );
    let id = OutlineId::new(&path, "fn[check_token]".into()).unwrap();
    let selected = OutlineContent::from_id(&id).unwrap();
    assert_eq!(selected.id(), &id);
    assert_eq!(selected.text(), "fn check_token() { allow(); }");
    assert_eq!(
        selected.fingerprint(),
        root.select(&id.selector).unwrap().fingerprint()
    );
    assert_eq!(
        selected.fingerprint(),
        supplied.select(&id.selector).unwrap().fingerprint()
    );
}

// {test}`requirements.sdk.outlines.language_provider.structural_search`
#[test]
fn outline_only_languages_report_unsupported_structural_search() {
    let language = CustomLanguage {
        configuration: Arc::new("custom-v1".into()),
        span: Span::new(0, 1),
    };
    let rule = serde_json::json!({"rule": {"regex": "."}});
    let rule = rule.as_object().unwrap();
    assert!(
        language
            .search_structure("contents", rule)
            .unwrap_err()
            .message
            .contains("unsupported")
    );
    assert!(
        languages::PlainText
            .search_structure("contents", rule)
            .unwrap_err()
            .message
            .contains("plain-text")
    );
}

// {test}`requirements.sdk.outlines.defaults`
// {test}`requirements.sdk.outlines.independent`
// {test}`requirements.sdk.outlines.parser.supplied_source`
// {test}`requirements.sdk.outlines.items`
// {test}`requirements.sdk.sources.rust.language`
// {test}`requirements.sdk.sources.python.language`
// {test}`requirements.sdk.sources.typescript.language`
// {test}`requirements.sdk.sources.markdown.language`
// {test}`requirements.sdk.sources.css.language`
// {test}`requirements.sdk.sources.toml.language`
// {test}`requirements.sdk.sources.yaml.language`
// {test}`requirements.sdk.sources.rust.fingerprints`
// {test}`requirements.sdk.sources.python.fingerprints`
// {test}`requirements.sdk.sources.typescript.fingerprints`
// {test}`requirements.sdk.sources.markdown.fingerprints`
// {test}`requirements.sdk.sources.css.fingerprints`
// {test}`requirements.sdk.sources.toml.fingerprints`
// {test}`requirements.sdk.sources.yaml.fingerprints`
// {test}`requirements.sdk.outlines.language_provider.structural_search`
// {test}`requirements.sdk.outlines.language_provider`
// {test}`requirements.sdk.sources.rust.Rust`
// {test}`requirements.sdk.sources.python.Python`
// {test}`requirements.sdk.sources.typescript.TypeScript`
// {test}`requirements.sdk.sources.typescript.Tsx`
// {test}`requirements.sdk.sources.toml.Toml`
// {test}`requirements.sdk.sources.yaml.Yaml`
// {test}`requirements.sdk.sources.plain-text.PlainText`
// {test}`requirements.sdk.sources.css.Css`
// {test}`requirements.sdk.sources.markdown.Markdown`
#[test]
fn bundled_languages_claim_extensions_and_supply_the_same_public_outline() {
    fn check<L: OutlineLang + Default + Copy>(
        language: L,
        filename: &str,
        source: &str,
        expected_selector: &str,
        expected_extensions: &[&str],
        expected_scheme: &str,
    ) {
        assert!(!language.name().is_empty());
        assert_eq!(language.extensions(), expected_extensions);
        assert_eq!(language.fingerprint_scheme(), expected_scheme);
        assert_eq!(
            std::mem::size_of::<L>(),
            0,
            "bundled language clones have no owned allocation"
        );
        let cloned = Clone::clone(&language);
        assert_eq!(cloned.extensions(), language.extensions());
        assert_eq!(L::default().fingerprint_scheme(), expected_scheme);
        let inferred = OutlineContent::from_source(filename, source.into()).unwrap();
        let explicit =
            OutlineContent::from_source_with(&language, filename, source.into()).unwrap();
        assert_eq!(inferred.fingerprint(), explicit.fingerprint());
        assert_eq!(inferred.fingerprint_scheme(), language.fingerprint_scheme());
        assert_eq!(inferred.outline().items[0].selector, "");
        assert_eq!(inferred.outline().items[0].span, Span::new(0, source.len()));
        assert!(inferred.select(expected_selector).is_ok());
        for extension in expected_extensions {
            let filename = format!("not-present.{}", extension.to_uppercase());
            let selected = OutlineContent::from_source(filename, source.into()).unwrap();
            assert_eq!(selected.fingerprint(), inferred.fingerprint());
            assert!(selected.select(expected_selector).is_ok());
        }
        for item in &inferred.outline().items {
            assert!(source.get(item.span.start..item.span.end).is_some());
        }
        let rule = serde_json::json!({"rule": {"regex": "."}});
        let ranges = language
            .search_structure(source, rule.as_object().unwrap())
            .unwrap();
        assert!(!ranges.is_empty());
        assert!(
            ranges
                .iter()
                .all(|range| source.get(range.start..range.end).is_some())
        );
    }
    check(
        languages::Rust,
        "a.RS",
        "fn check() {}",
        "fn[check]",
        &["rs"],
        "rust-structural-v1",
    );
    check(
        languages::Python,
        "a.PYI",
        "def check():\n    pass\n",
        "def[check]",
        &["py", "pyi"],
        "python-structural-v1",
    );
    check(
        languages::TypeScript,
        "a.MTS",
        "function check() {}",
        "function[check]",
        &["ts", "mts", "cts"],
        "typescript-structural-v1",
    );
    check(
        languages::Tsx,
        "a.TSX",
        "function check() { return <div />; }",
        "function[check]",
        &["tsx"],
        "tsx-structural-v1",
    );
    check(
        languages::Markdown,
        "a.MARKDOWN",
        "# Check\n",
        "section[Check]",
        &["md", "markdown"],
        "markdown-structural-v1",
    );
    check(
        languages::Css,
        "a.CSS",
        ".check { color: red; }",
        "rule[.check]",
        &["css"],
        "css-structural-v1",
    );
    check(
        languages::Toml,
        "a.TOML",
        "[check]\nvalue = 1\n",
        "table[check]",
        &["toml"],
        "toml-structural-v1",
    );
    check(
        languages::Yaml,
        "a.YAML",
        "check: true\n",
        "pair[check]",
        &["yml", "yaml"],
        "yaml-structural-v1",
    );
    let plain = languages::PlainText;
    assert_eq!(plain.name(), "plain-text");
    assert!(plain.extensions().is_empty());
}

// {test}`requirements.sdk.outlines.items`
// {test}`requirements.sdk.outlines.selection`
// {test}`requirements.sdk.sources.plain-text.items`
// {test}`requirements.sdk.outlines.Span`
#[test]
fn unicode_spans_use_bytes_and_whitespace_only_lines_create_no_items() {
    let source = "é\n \t\n🦀\n";
    let contents = OutlineContent::from_source("unicode.unknown", source.into()).unwrap();
    assert_eq!(
        contents
            .outline()
            .items
            .iter()
            .map(|item| item.span)
            .collect::<Vec<_>>(),
        [
            Span::new(0, source.len()),
            Span::new(0, 2),
            Span::new(6, 10)
        ]
    );
    assert_eq!(contents.outline().items[0].selector, "");
    for (item, text) in contents.outline().items[1..].iter().zip(["é", "🦀"]) {
        assert_eq!(item.kind, "line");
        assert_eq!(&source[item.span], text);
        assert_eq!(contents.select(&item.selector).unwrap().text(), text);
    }
}

// {test}`requirements.sdk.outlines.items`
// {test}`requirements.sdk.outlines.selection`
// {test}`requirements.sdk.sources.markdown.sections`
// {test}`requirements.sdk.sources.markdown.Markdown`
#[test]
fn markdown_heading_levels_resolve_encoded_duplicates_and_anonymous_items() {
    let source = "préface\n\n# Main / [x] # @ % end\n### Deep\nnested\n\n## Same\nfirst\n\n## Same\nsecond\n\n# End\nLevel two\n---------\n:::{note}\ninside\n:::\n\n## Peer\nlast\n";
    let contents = OutlineContent::from_source("headings.md", source.into()).unwrap();
    let sections = contents
        .outline()
        .items
        .iter()
        .filter(|item| item.kind == "section")
        .map(|item| item.selector.as_str())
        .collect::<Vec<_>>();
    let main = "section[Main%20%2F%20%5Bx%5D%20%23%20%40%20%25%20end]";
    assert!(sections[0].starts_with("section@"));
    assert_eq!(
        &sections[1..],
        [
            main,
            &format!("{main}/section[Deep]"),
            &format!("{main}/section[Same]#1"),
            &format!("{main}/section[Same]#2"),
            "section[End]",
            "section[End]/section[Level%20two]",
            "section[End]/section[Peer]",
        ]
    );
    assert_eq!(
        contents.select(sections[0]).unwrap().text().trim_end(),
        "préface"
    );
    assert!(
        contents
            .select(&format!("{main}/section[Same]#1"))
            .unwrap()
            .text()
            .contains("first")
    );
    let second = contents.select(&format!("{main}/section[Same]#2")).unwrap();
    assert!(second.text().contains("second"));
    assert!(!second.text().contains("first"));
    assert!(std::ptr::eq(second.outline(), contents.outline()));
    assert!(second.select(main).unwrap().text().starts_with("# Main"));
    let block = contents
        .outline()
        .items
        .iter()
        .find(|item| item.kind == "block")
        .unwrap();
    assert!(
        block
            .selector
            .starts_with("section[End]/section[Level%20two]/block@")
    );
    let selected = contents.select(&block.selector).unwrap();
    assert_eq!(selected.text().trim_end(), ":::{note}\ninside\n:::");
    assert_eq!(selected.text(), &source[block.span]);
    assert_eq!(contents.outline().items[0].span, Span::new(0, source.len()));
    assert!(
        contents
            .outline()
            .items
            .iter()
            .all(|item| source.get(item.span.start..item.span.end).is_some())
    );
}

// {test}`requirements.sdk.sources.markdown.fingerprints`
#[test]
fn directive_prose_hashes_words_across_reflow_without_joining_them() {
    fn directive(source: &str) -> OutlineContent {
        let contents = OutlineContent::from_source("note.md", source.into()).unwrap();
        let selector = contents
            .outline()
            .items
            .iter()
            .find(|item| item.kind == "block")
            .unwrap()
            .selector
            .clone();
        contents.select(&selector).unwrap()
    }
    let original = directive("# H\n:::{note}\ntwo words\n:::\n");
    let reflowed = directive("# H\n:::{note}\n two\n words \n:::\n");
    let joined = directive("# H\n:::{note}\ntwowords\n:::\n");
    assert_eq!(original.fingerprint(), reflowed.fingerprint());
    assert_ne!(original.fingerprint(), joined.fingerprint());
}

// {test}`requirements.sdk.sources.css.items`
#[test]
fn css_media_and_supports_preludes_collapse_whitespace() {
    let source = "@media  \t(max-width:\n  600px) { a { color: red; } }\n@supports  (display:  grid) { b { display: grid; } }\n";
    let contents = OutlineContent::from_source("preludes.css", source.into()).unwrap();
    assert_eq!(
        contents
            .outline()
            .items
            .iter()
            .map(|item| (item.kind.as_str(), item.selector.as_str()))
            .collect::<Vec<_>>(),
        [
            ("", ""),
            ("media", "media[(max-width:%20600px)]"),
            ("rule", "media[(max-width:%20600px)]/rule[a]"),
            ("supports", "supports[(display:%20grid)]"),
            ("rule", "supports[(display:%20grid)]/rule[b]"),
        ]
    );
}

// {test}`requirements.sdk.sources.css.items`
// {test}`requirements.sdk.outlines.selection`
// {test}`requirements.sdk.sources.css.Css`
#[test]
fn css_duplicate_names_are_numbered_after_normalization_within_each_parent() {
    let source = "a  b { color: red; }\na b { color: blue; }\n@media  print { a { color: red; } }\n@media print { a { color: blue; } }\n@keyframes spin { from { opacity: 0; } }\n@keyframes spin { to { opacity: 1; } }\n";
    let contents = OutlineContent::from_source("duplicates.css", source.into()).unwrap();
    assert_eq!(
        contents
            .outline()
            .items
            .iter()
            .map(|item| item.selector.as_str())
            .collect::<Vec<_>>(),
        [
            "",
            "rule[a%20b]#1",
            "rule[a%20b]#2",
            "media[print]#1",
            "media[print]#1/rule[a]",
            "media[print]#2",
            "media[print]#2/rule[a]",
            "keyframes[spin]#1",
            "keyframes[spin]#2",
        ]
    );
    assert!(
        contents
            .select("rule[a%20b]#1")
            .unwrap()
            .text()
            .contains("red")
    );
    assert!(
        contents
            .select("rule[a%20b]#2")
            .unwrap()
            .text()
            .contains("blue")
    );
    assert!(contents.select("rule[a%20b]").is_err());
    assert!(
        contents
            .select("media[print]#2/rule[a]")
            .unwrap()
            .text()
            .contains("blue")
    );
}

// {test}`requirements.sdk.sources.markdown.sections`
// {test}`requirements.sdk.outlines.items`
#[test]
fn multiline_setext_sections_keep_original_source_spans_and_skip_code() {
    let source = "# Parent\nTítle\ntwo\n---\nbody\n\n```text\nliteral\n===\n```\n\n# Last\n";
    let contents = OutlineContent::from_source("setext.md", source.into()).unwrap();
    let sections = contents
        .outline()
        .items
        .iter()
        .filter(|item| item.kind == "section")
        .map(|item| item.selector.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        sections,
        [
            "section[Parent]",
            "section[Parent]/section[Títle%20two]",
            "section[Last]",
        ]
    );
    let selected = contents
        .select("section[Parent]/section[Títle%20two]")
        .unwrap();
    assert!(selected.text().starts_with("Títle\ntwo\n---\nbody"));
    assert!(selected.text().contains("```text\nliteral\n===\n```"));
    assert!(!selected.text().contains("# Last"));
    let item = contents
        .outline()
        .items
        .iter()
        .find(|item| item.selector == selected.id().selector)
        .unwrap();
    assert_eq!(item.span.start, source.find("Títle").unwrap());
    assert_eq!(&source[item.span], selected.text());
}

// {test}`requirements.sdk.sources.markdown.sections`
// {test}`requirements.sdk.outlines.items`
// {test}`requirements.sdk.outlines.Span`
#[test]
fn setext_sections_inside_directives_lists_and_quotes_retain_container_spans() {
    let cases = [
        (
            "# Root\n\n:::{note}\nRésumé\ntwo\n---\nbody\n:::\n",
            "---",
            true,
        ),
        ("# Root\n\n> Résumé\n> two\n> ---\n> body\n", "> ---", false),
        ("# Root\n\n- Résumé\n  two\n  ---\n  body\n", "  ---", false),
    ];
    for (source, underline, in_block) in cases {
        let contents = OutlineContent::from_source("containers.md", source.into()).unwrap();
        let item = contents
            .outline()
            .items
            .iter()
            .find(|item| item.selector.ends_with("/section[Résumé%20two]"))
            .unwrap_or_else(|| panic!("missing setext section in {source:?}"));
        assert!(item.selector.starts_with("section[Root]/"));
        assert_eq!(item.selector.contains("/block@"), in_block);
        let first = source.find("Résumé").unwrap();
        let end = source.find("two").unwrap() + "two".len();
        assert_eq!(item.span.start, first);
        assert_eq!(item.name_span, Some(Span::new(first, end)));
        let selected = contents.select(&item.selector).unwrap();
        assert!(selected.text().contains(underline), "{source:?}");
        assert!(selected.text().contains("body"), "{source:?}");
        assert_eq!(selected.text(), &source[item.span]);
    }
}

// {test}`requirements.sdk.outlines.language_provider.structural_search`
#[test]
fn bundled_structural_search_returns_matches_and_rejects_bad_rules_and_source() {
    let source = "fn first() {}\nfn second() {}\n";
    let rule = serde_json::json!({"rule": {"kind": "function_item"}});
    let ranges = languages::Rust
        .search_structure(source, rule.as_object().unwrap())
        .unwrap();
    assert_eq!(
        ranges
            .iter()
            .map(|range| &source[*range])
            .collect::<Vec<_>>(),
        ["fn first() {}", "fn second() {}"]
    );
    let invalid = serde_json::json!({"unexpected": true});
    assert!(
        languages::Rust
            .search_structure(source, invalid.as_object().unwrap())
            .is_err()
    );
    assert!(
        languages::Rust
            .search_structure("fn {", rule.as_object().unwrap())
            .is_err()
    );
}

// {test}`requirements.sdk.outlines.items`
// {test}`requirements.sdk.outlines.selection`
// {test}`requirements.sdk.outlines.errors`
// {test}`requirements.sdk.sources.rust.items`
// {test}`requirements.sdk.sources.rust.fingerprints`
// {test}`requirements.sdk.sources.rust.Rust`
// {test}`requirements.sdk.outlines.Outline`
#[test]
fn rust_items_attach_documentation_and_preserve_structural_hashes() {
    let qualified = OutlineContent::from_source(
        "qualified.rs",
        "impl core::fmt::Display for Thing {}".into(),
    )
    .unwrap();
    assert_eq!(
        qualified.outline().items[1].selector,
        "impl[Thing/core::fmt::Display]"
    );
    let source = "mod auth {\n    /// Check tokens.\n    #[inline]\n    fn check() { allow(); }\n    struct Token;\n}\nfn last() {}\n";
    let contents = OutlineContent::from_source("auth.rs", source.into()).unwrap();
    let selectors = contents
        .outline()
        .items
        .iter()
        .map(|item| item.selector.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        selectors,
        [
            "",
            "mod[auth]",
            "mod[auth]/fn[check]",
            "mod[auth]/struct[Token]",
            "fn[last]"
        ]
    );
    assert_eq!(contents.outline().items[0].span, Span::new(0, source.len()));
    assert_eq!(
        contents
            .outline()
            .items
            .iter()
            .map(|item| item.kind.as_str())
            .collect::<Vec<_>>(),
        ["", "mod", "fn", "struct", "fn"]
    );
    for item in &contents.outline().items {
        let selected = contents.select(&item.selector).unwrap();
        assert_eq!(selected.text(), &source[item.span]);
        assert_eq!(selected.structural_hash(), item.structural_hash);
    }
    let function = contents.select("mod[auth]/fn[check]").unwrap();
    assert!(function.text().starts_with("/// Check tokens."));
    assert!(function.text().contains("#[inline]"));
    assert!(std::ptr::eq(function.outline(), contents.outline()));
    let span = contents.outline().items[2].span;
    assert_eq!(&source[span.clone()], function.text());
    assert_eq!(span.start, source.find("/// Check tokens.").unwrap());
    assert!(source[..span.end].ends_with("fn check() { allow(); }"));
    let formatted = OutlineContent::from_source(
        "auth.rs",
        "mod auth { /// Check\n/// tokens.\n#[inline]\nfn check(){ allow(); } }".into(),
    )
    .unwrap()
    .select("mod[auth]/fn[check]")
    .unwrap();
    assert_eq!(function.fingerprint(), formatted.fingerprint());
    let changed = OutlineContent::from_source("auth.rs", source.replace("allow", "deny").into())
        .unwrap()
        .select("mod[auth]/fn[check]")
        .unwrap();
    assert_ne!(function.fingerprint(), changed.fingerprint());
    assert!(function.outline().diagnostic.is_none());
}

// {test}`requirements.sdk.outlines.errors`
// {test}`requirements.sdk.outlines.Outline`
#[test]
fn recovered_outlines_keep_the_diagnostic_and_intact_items() {
    use trust_but_verify::outline::AttachmentKind;
    let source = "// intact comment\nfn intact() {}\nfn broken( {\n";
    let root = OutlineContent::from_source("broken.rs", source.into()).unwrap();
    let intact = root.select("fn[intact]").unwrap();
    let error = intact.outline().diagnostic.as_ref().unwrap();
    assert_eq!(error.line, 3);
    assert!(error.message.contains("syntax error"));
    let comments: Vec<_> = root
        .outline()
        .items
        .iter()
        .flat_map(|item| &item.attachments)
        .filter(|attachment| attachment.kind == AttachmentKind::Comment)
        .map(|attachment| &source[attachment.span])
        .collect();
    assert_eq!(comments, ["// intact comment"]);
    assert_eq!(
        languages::Rust.outline(source).unwrap().diagnostic.as_ref(),
        Some(error)
    );
}

// {test}`requirements.sdk.graph.structural-fingerprint`
// {test}`requirements.sdk.graph.NodeContents`
#[test]
fn structural_fingerprint_example_tracks_the_selected_function() {
    let original = OutlineContent::from_source("auth.rs", "fn check_token() { allow(); }".into())
        .unwrap()
        .select("fn[check_token]")
        .unwrap();
    let reformatted =
        OutlineContent::from_source("auth.rs", "fn check_token()\n{\n    allow();\n}\n".into())
            .unwrap()
            .select("fn[check_token]")
            .unwrap();
    let changed = OutlineContent::from_source("auth.rs", "fn check_token() { deny(); }".into())
        .unwrap()
        .select("fn[check_token]")
        .unwrap();
    assert_eq!(original.fingerprint(), reformatted.fingerprint());
    assert_ne!(original.fingerprint(), changed.fingerprint());
    assert!(original.format().contains("fn check_token()"));
}

// {test}`requirements.sdk.sources.rust.fingerprints`
// {test}`requirements.sdk.sources.python.fingerprints`
// {test}`requirements.sdk.sources.typescript.fingerprints`
// {test}`requirements.sdk.sources.css.fingerprints`
// {test}`requirements.sdk.sources.markdown.fingerprints`
// {test}`requirements.sdk.sources.toml.fingerprints`
// {test}`requirements.sdk.sources.yaml.fingerprints`
// {test}`requirements.sdk.outlines.attachments.fingerprint`
#[test]
fn bundled_fingerprints_exclude_comments_and_preserve_word_boundaries() {
    fn fingerprint(path: &str, text: &str, selector: &str) -> trust_but_verify::graph::Fingerprint {
        OutlineContent::from_source(path, text.into())
            .unwrap()
            .select(selector)
            .unwrap()
            .fingerprint()
    }
    let cases = [
        (
            "a.rs",
            "fn[f]",
            "/// two words\nfn f() { /* note */ allow(); }\n",
            "/// two   words\nfn f() { /* edited */ allow(); }\n",
            "/// twowords\nfn f() { allow(); }\n",
        ),
        (
            "a.py",
            "def[f]",
            "def f():\n    # note\n    return \"two words\"\n",
            "def f():\n    # edited\n    return \"two   words\"\n",
            "def f():\n    return \"twowords\"\n",
        ),
        (
            "a.ts",
            "function[f]",
            "/** docs */\nfunction f() { return \"two words\"; }",
            "/** other docs */\nfunction f() { return \"two   words\"; }",
            "function f() { return \"twowords\"; }",
        ),
        (
            "a.tsx",
            "function[f]",
            "/** docs */\nfunction f() { return <div title=\"two words\"/>; }",
            "/** other docs */\nfunction f() { return <div title=\"two   words\"/>; }",
            "function f() { return <div title=\"twowords\"/>; }",
        ),
        (
            "a.css",
            "rule[a]",
            "a { /* note */ content: \"two words\"; }",
            "a { /* edited */ content: \"two   words\"; }",
            "a { content: \"twowords\"; }",
        ),
        (
            "a.md",
            "section[H]",
            "# H\n\n<!-- note -->\n\ntwo words\n",
            "# H\n\n<!-- edited -->\n\ntwo\nwords\n",
            "# H\n\ntwowords\n",
        ),
        (
            "a.toml",
            "table[t]",
            "[t]\n# note\na = \"two words\"\n",
            "[t]\n# edited\na = \"two   words\"\n",
            "[t]\na = \"twowords\"\n",
        ),
        (
            "a.yml",
            "pair[a]",
            "a:\n  # note\n  b: \"two words\"\n",
            "a:\n  # edited\n  b: \"two   words\"\n",
            "a:\n  b: \"twowords\"\n",
        ),
    ];
    for (path, selector, original, comments_and_space, joined_words) in cases {
        let saved = fingerprint(path, original, selector);
        assert_eq!(saved.value.len(), 64, "{path}");
        assert!(
            saved.value.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "{path}"
        );
        assert_eq!(
            saved,
            fingerprint(path, comments_and_space, selector),
            "{path}"
        );
        assert_ne!(saved, fingerprint(path, joined_words, selector), "{path}");
    }
    let docstring = "def f():\n    \"\"\"two words\"\"\"\n    return 1\n";
    assert_eq!(
        fingerprint("a.py", docstring, "def[f]"),
        fingerprint(
            "a.py",
            &docstring.replace("two words", "two   words"),
            "def[f]"
        )
    );
    assert_ne!(
        fingerprint("a.py", docstring, "def[f]"),
        fingerprint(
            "a.py",
            &docstring.replace("two words", "changed words"),
            "def[f]"
        )
    );
    let inline = "# H\n\ntext <!-- one --> prose\n";
    assert_ne!(
        fingerprint("a.md", inline, "section[H]"),
        fingerprint("a.md", &inline.replace("one", "two"), "section[H]")
    );
}

// {test}`requirements.sdk.sources.typescript.items`
// {test}`requirements.sdk.sources.typescript.TypeScript`
// {test}`requirements.sdk.sources.typescript.Tsx`
#[test]
fn typescript_and_tsx_share_signatures_abstract_classes_and_module_kinds() {
    let source = "function* generated() { yield 1; }\ndeclare function signed(x: number): void;\nabstract class Abstract { abstract run(): void; }\ndeclare module \"remote\" { export interface Thing {} }\nnamespace Outer { export class Inner { call() {} } }\n";
    let mut observed = Vec::new();
    for path in ["api.ts", "api.tsx"] {
        let root = OutlineContent::from_source(path, source.into()).unwrap();
        let selectors = root
            .outline()
            .items
            .iter()
            .map(|item| item.selector.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            selectors,
            [
                "",
                "function[generated]",
                "function[signed]",
                "class[Abstract]",
                "class[Abstract]/method[run]",
                "namespace[\"remote\"]",
                "namespace[\"remote\"]/interface[Thing]",
                "namespace[Outer]",
                "namespace[Outer]/class[Inner]",
                "namespace[Outer]/class[Inner]/method[call]",
            ],
            "{path}"
        );
        observed.push(
            root.outline()
                .items
                .iter()
                .map(|item| (item.selector.clone(), item.kind.clone()))
                .collect::<Vec<_>>(),
        );
    }
    assert_eq!(observed[0], observed[1]);
}

// {test}`requirements.sdk.sources.toml.items`
// {test}`requirements.sdk.sources.toml.Toml`
#[test]
fn toml_preserves_written_dotted_literal_and_quoted_header_names() {
    let source = "a.b = 1\n'lit key' = 2\nkey_7-2 = 3\n[\"space #/@[]\"]\nvalue = 4\n[[items.parts]]\n\"nested/key\" = { 'deep key' = 5 }\n";
    let root = OutlineContent::from_source("config.toml", source.into()).unwrap();
    let selectors = root
        .outline()
        .items
        .iter()
        .map(|item| item.selector.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        selectors,
        [
            "",
            "pair[a.b]",
            "pair['lit%20key']",
            "pair[key_7-2]",
            "table[\"space%20%23%2F%40%5B%5D\"]",
            "table[\"space%20%23%2F%40%5B%5D\"]/pair[value]",
            "table[items.parts]",
            "table[items.parts]/pair[\"nested%2Fkey\"]",
            "table[items.parts]/pair[\"nested%2Fkey\"]/pair['deep%20key']",
        ]
    );
}

// {test}`requirements.sdk.sources.yaml.items`
// {test}`requirements.sdk.sources.yaml.Yaml`
#[test]
fn yaml_selection_ends_before_trailing_comment_tokens() {
    let root = OutlineContent::from_source("config.yaml", "key: value # trailing comment\n".into())
        .unwrap();
    let selected = root.select("pair[key]").unwrap();
    assert_eq!(selected.text(), "key: value");
    assert_eq!(root.outline().items[1].span, Span::new(0, 10));
    let nested = "a:\n  b: 1\n# before next\nd: 2\n";
    let root = OutlineContent::from_source("nested.yml", nested.into()).unwrap();
    assert_eq!(root.select("pair[a]").unwrap().text(), "a:\n  b: 1");
    assert_eq!(
        root.select("pair[a]/pair[b]").unwrap().text().trim_start(),
        "b: 1"
    );
}

// {test}`requirements.sdk.sources.python.items`
// {test}`requirements.sdk.sources.python.Python`
#[test]
fn python_selected_source_includes_its_decorators() {
    let source = "@cache\ndef checked():\n    pass\n";
    let root = OutlineContent::from_source("decorated.py", source.into()).unwrap();
    let selected = root.select("def[checked]").unwrap();
    assert!(selected.text().starts_with("@cache\ndef checked():"));
    assert_eq!(root.outline().items[1].span.start, 0);
    let classes = "@first\nclass Checked: pass\n@second\nclass Checked: pass\n";
    let root = OutlineContent::from_source("decorated.py", classes.into()).unwrap();
    for (selector, decorator) in [
        ("class[Checked]#1", "@first"),
        ("class[Checked]#2", "@second"),
    ] {
        let selected = root.select(selector).unwrap();
        assert!(selected.text().starts_with(decorator));
    }
}

// {test}`requirements.sdk.sources.markdown.sections`
#[test]
fn markdown_sections_include_setext_headings_and_anonymous_preamble() {
    let source = "intro\n\nHeading   words\n===\n\nparagraph\n\n## Nested\nleaf\n";
    let root = OutlineContent::from_source("guide.md", source.into()).unwrap();
    let sections = root
        .outline()
        .items
        .iter()
        .filter(|item| item.kind == "section")
        .map(|item| item.selector.as_str())
        .collect::<Vec<_>>();
    assert_eq!(sections.len(), 3);
    assert!(sections[0].starts_with("section@"));
    assert_eq!(sections[1], "section[Heading%20words]");
    assert_eq!(sections[2], "section[Heading%20words]/section[Nested]");
    assert!(
        root.select(sections[1])
            .unwrap()
            .text()
            .starts_with("Heading   words")
    );
    assert_eq!(
        root.select(sections[2]).unwrap().text().trim_end(),
        "## Nested\nleaf"
    );
}

// {test}`requirements.sdk.sources.plain-text.fingerprints`
// {test}`requirements.sdk.sources.plain-text.PlainText`
#[test]
fn plain_line_fingerprints_use_sha256_words_without_losing_boundaries() {
    fn line(text: &str) -> trust_but_verify::graph::Fingerprint {
        let root = OutlineContent::from_source("note.txt", text.into()).unwrap();
        root.select(&root.outline().items[1].selector)
            .unwrap()
            .fingerprint()
    }
    assert_eq!(
        line("hello").value,
        "8b8d04e8a7ca68bfe4afe50ada62c244b03c0dcd31aafb2b885c81095f1fdd5c"
    );
    assert_eq!(line("two words"), line(" two \t words "));
    assert_ne!(line("two words"), line("twowords"));
    assert_eq!(line("two words").scheme, "plain-text-structural-v1");
}

// {test}`requirements.sdk.sources.redaction`
// {test}`requirements.sdk.outlines.node_contents`
// {test}`requirements.sdk.outlines.selection`
#[test]
fn formatted_contents_redact_credentials_while_retaining_raw_text_and_spans() {
    let token = format!("gh{}_{}", "p", "16C7e42F292c6912E7710c838347Ae178B4a");
    let different = format!("gh{}_{}", "p", "C69C118783E8886B5119E0fe942B153973Ca");
    let source = format!("credential {token}\nvisible text\n");
    let contents = OutlineContent::from_source("note.unknown", source.clone().into()).unwrap();
    assert_eq!(contents.text(), source);
    assert!(contents.format().contains("[REDACTED:GitHub Token]"));
    assert!(!contents.format().contains(&token));
    assert_eq!(contents.format().lines().count(), source.lines().count());
    let changed =
        OutlineContent::from_source("note.unknown", source.replace(&token, &different).into())
            .unwrap();
    assert_eq!(contents.fingerprint(), changed.fingerprint());
    let selected = contents
        .select(&contents.outline().items[1].selector)
        .unwrap();
    assert!(selected.text().contains(&token));
    assert!(!selected.format().contains(&token));
}

// {test}`requirements.sdk.sources.python.items`
#[test]
fn python_nested_definitions_use_the_nearest_outline_ancestor() {
    let source = "class Outer:\n    class Inner:\n        @decorate\n        def method(self):\n            if True:\n                def local():\n                    pass\ndef factory():\n    class Product:\n        def run(self):\n            pass\nasync def async_outer():\n    async def async_inner():\n        pass\n";
    let contents = OutlineContent::from_source("nested.py", source.into()).unwrap();
    assert_eq!(
        contents
            .outline()
            .items
            .iter()
            .map(|item| item.selector.as_str())
            .collect::<Vec<_>>(),
        [
            "",
            "class[Outer]",
            "class[Outer]/class[Inner]",
            "class[Outer]/class[Inner]/def[method]",
            "class[Outer]/class[Inner]/def[method]/def[local]",
            "def[factory]",
            "def[factory]/class[Product]",
            "def[factory]/class[Product]/def[run]",
            "def[async_outer]",
            "def[async_outer]/def[async_inner]"
        ]
    );
    let method = contents
        .select("class[Outer]/class[Inner]/def[method]")
        .unwrap();
    assert!(method.text().trim_start().starts_with("@decorate"));
    assert!(
        contents
            .select("def[async_outer]/def[async_inner]")
            .unwrap()
            .text()
            .trim_start()
            .starts_with("async def async_inner")
    );
}

// {test}`requirements.sdk.outlines.attachments`
// {test}`requirements.sdk.outlines.attachments.span`
// {test}`requirements.sdk.outlines.attachments.order`
// {test}`requirements.sdk.outlines.attachments.comments`
// {test}`requirements.sdk.sources.rust.attachments`
#[test]
fn rust_attachments_keep_the_owner_kind_and_original_bytes() {
    use trust_but_verify::outline::AttachmentKind::{Annotation, Comment};
    let source = "//! file docs\nmod m {\n    /// café\n    /// café\n    #[inline]\n    fn f() {} // trailing\n}\n";
    let snapshot = OutlineContent::from_source("example.rs", source.into()).unwrap();
    let selected = snapshot.select("mod[m]/fn[f]").unwrap();
    let items = &selected.outline().items;
    let records = |index: usize| {
        items[index]
            .attachments
            .iter()
            .map(|a| (a.kind.clone(), source[a.span].trim_end()))
            .collect::<Vec<_>>()
    };
    assert_eq!(records(0), [(Comment, "//! file docs")]);
    assert!(records(1).is_empty());
    assert_eq!(
        records(2),
        [
            (Comment, "/// café"),
            (Comment, "/// café"),
            (Annotation, "#[inline]"),
            (Comment, "// trailing")
        ]
    );
    assert!(items[2].attachments[0].span.end <= items[2].attachments[1].span.start);
}

// {test}`requirements.sdk.outlines.attachments.comments`
// {test}`requirements.sdk.outlines.attachments.span`
// {test}`requirements.sdk.outlines.AttachmentKind`
// {test}`requirements.sdk.sources.rust.attachments`
// {test}`requirements.sdk.outlines.attachments.empty`
// {test}`requirements.sdk.sources.python.attachments`
// {test}`requirements.sdk.sources.markdown.attachments`
// {test}`requirements.sdk.sources.typescript.attachments`
// {test}`requirements.sdk.sources.css.attachments`
// {test}`requirements.sdk.sources.yaml.attachments`
// {test}`requirements.sdk.sources.toml.attachments`
// {test}`requirements.sdk.sources.plain-text.attachments`
#[test]
fn bundled_languages_classify_only_their_attachment_syntax() {
    use trust_but_verify::outline::AttachmentKind::{Annotation, Comment};
    for (path, source, expected) in [
        (
            "test.py",
            "# doc\n@a\n@b(1)\ndef f():\n    \"\"\"# literal\"\"\"\n",
            vec![
                (Comment, "# doc"),
                (Annotation, "@a"),
                (Annotation, "@b(1)"),
            ],
        ),
        (
            "blocks.rs",
            "/*! inner */\n/** outer */\n/* ordinary */\nfn f() {}\n",
            vec![
                (Comment, "/*! inner */"),
                (Comment, "/** outer */"),
                (Comment, "/* ordinary */"),
            ],
        ),
        (
            "class.py",
            "@pkg.decorate(\n  1\n)\nclass C:\n    pass # trailing\n",
            vec![
                (Annotation, "@pkg.decorate(\n  1\n)"),
                (Comment, "# trailing"),
            ],
        ),
        (
            "test.ts",
            "/** doc */\nconst s = '// string'; // trailing\n@dec class C {}\n",
            vec![(Comment, "/** doc */"), (Comment, "// trailing")],
        ),
        (
            "test.tsx",
            "/* doc */\nconst s = <p>// text</p>; // trailing\n@dec class C {}\n",
            vec![(Comment, "/* doc */"), (Comment, "// trailing")],
        ),
        (
            "test.css",
            "/* block */\na { color: red; }\n// line\n",
            vec![(Comment, "/* block */"), (Comment, "// line")],
        ),
        (
            "test.yaml",
            "a: |\n  # literal\n# standalone\nb: '# literal' # real\nc: \"# literal\"\n",
            vec![(Comment, "# standalone"), (Comment, "# real")],
        ),
        (
            "test.toml",
            "# first\n# second\na = '# literal' # real\n\"# key\" = \"\"\"# literal\"\"\"\n",
            vec![
                (Comment, "# first"),
                (Comment, "# second"),
                (Comment, "# real"),
            ],
        ),
        (
            "test.md",
            "<!-- block -->\n\n<!-- multi\nline -->\n\n<div>html</div>\n\nA <!-- inline --> word.\n\n```html\n<!-- code -->\n```\n\n<!-- unfinished\n",
            vec![
                (Comment, "<!-- block -->"),
                (Comment, "<!-- multi\nline -->"),
            ],
        ),
        ("test.txt", "# just text\n", vec![]),
    ] {
        let snapshot = OutlineContent::from_source(path, source.into()).unwrap();
        let mut found: Vec<_> = snapshot
            .outline()
            .items
            .iter()
            .flat_map(|i| &i.attachments)
            .collect();
        found.sort_by_key(|a| a.span.start);
        assert_eq!(
            found
                .iter()
                .map(|a| (a.kind.clone(), source[a.span].trim_end()))
                .collect::<Vec<_>>(),
            expected,
            "{path}"
        );
        if path == "class.py" {
            let class = snapshot
                .outline()
                .items
                .iter()
                .find(|item| item.selector == "class[C]")
                .unwrap();
            assert_eq!(
                source[class.attachments[0].span].trim_end(),
                "@pkg.decorate(\n  1\n)"
            );
        }
        if path == "test.ts" || path == "test.tsx" {
            assert!(snapshot.text().contains("@dec class C"));
        }
        if path == "test.py" {
            assert_eq!(
                snapshot
                    .outline()
                    .items
                    .iter()
                    .find(|item| item.selector == "def[f]")
                    .unwrap()
                    .attachments
                    .len(),
                3
            );
        }
    }
}

// {test}`requirements.sdk.outlines.OutlineAttachment`
// {test}`requirements.sdk.outlines.AttachmentKind`
// {test}`requirements.sdk.outlines.attachments.span`
#[test]
fn custom_attachment_spans_are_checked_against_the_original_snapshot() {
    use trust_but_verify::outline::{AttachmentKind, OutlineAttachment};
    #[derive(Clone)]
    struct Custom {
        span: Span,
    }
    impl OutlineLang for Custom {
        fn name(&self) -> &str {
            "custom"
        }
        fn extensions(&self) -> &[&str] {
            &["custom"]
        }
        fn fingerprint_scheme(&self) -> &str {
            "custom-v1"
        }
        fn outline(&self, source: &str) -> Result<Outline, OutlineError> {
            let marker = OutlineAttachment::new(AttachmentKind::Other("marker".into()), self.span);
            let item = OutlineItem::new(
                String::new(),
                String::new(),
                Span::new(0, source.len()),
                "root".into(),
            )
            .with_attachments(vec![marker]);
            Ok(Outline::new(vec![item]))
        }
    }
    let load = |span| OutlineContent::from_source_with(&Custom { span }, "one.custom", "é".into());
    let snapshot = load(Span::new(0, 2)).unwrap();
    let attachment = &snapshot.outline().items[0].attachments[0];
    assert_eq!(attachment.kind, AttachmentKind::Other("marker".into()));
    assert_eq!(&snapshot.text()[attachment.span], "é");
    assert!(load(Span::new(1, 2)).is_err());
    assert!(load(Span::new(0, 3)).is_err());
}

// {test}`requirements.sdk.outlines.Outline`
// {test}`requirements.sdk.outlines.OutlineItem`
// {test}`requirements.sdk.outlines.name_span`
// {test}`requirements.sdk.public_api`
#[test]
fn outline_constructors_preserve_fields_and_replace_attachments() {
    use trust_but_verify::outline::{AttachmentKind, OutlineAttachment};
    let item = OutlineItem::new(
        "fn[f]".into(),
        "fn".into(),
        Span::new(2, 12),
        "digest".into(),
    );
    assert_eq!(item.name_span, None);
    assert!(item.attachments.is_empty());
    let item = item
        .with_name_span(Span::new(7, 8))
        .with_attachments(vec![OutlineAttachment::new(
            AttachmentKind::Comment,
            Span::new(2, 4),
        )])
        .with_attachments(vec![OutlineAttachment::new(
            AttachmentKind::Annotation,
            Span::new(4, 6),
        )]);
    let outline = Outline::new(vec![item.clone()]);
    assert!(outline.diagnostic.is_none());
    assert!(!format!("{outline:?}").is_empty());
    let cloned = outline.clone();
    let [item] = cloned.items.as_slice() else {
        panic!("{cloned:?}")
    };
    assert_eq!(
        (
            &*item.selector,
            &*item.kind,
            item.span,
            &*item.structural_hash
        ),
        ("fn[f]", "fn", Span::new(2, 12), "digest")
    );
    assert_eq!(item.name_span, Some(Span::new(7, 8)));
    let [attachment] = item.attachments.as_slice() else {
        panic!("{item:?}")
    };
    assert_eq!(attachment.kind, AttachmentKind::Annotation);
    assert_eq!(attachment.span, Span::new(4, 6));
}

// {test}`requirements.sdk.outlines.name_span`
// {test}`requirements.sdk.sources.rust.items`
#[test]
fn rust_name_spans_highlight_spelling_in_the_original_source() {
    let source = "use x;\nimpl<T> Trait for Wrapper<T> {\n #[test]\n fn café() {}\n}\nimpl Wrapper<u8> {}\nunsafe extern {}\nextern \"C\" {}\n";
    let contents = OutlineContent::from_source("names.rs", source.into()).unwrap();
    let named: Vec<_> = contents
        .outline()
        .items
        .iter()
        .map(|item| (item.kind.as_str(), item.name_span.map(|span| &source[span])))
        .collect();
    assert_eq!(
        named,
        [
            ("", None),
            ("use", None),
            ("impl", Some("Trait for Wrapper<T>")),
            ("fn", Some("café")),
            ("impl", Some("Wrapper<u8>")),
            ("extern", Some("extern")),
            ("extern", Some("\"C\""))
        ]
    );
    assert_eq!(
        contents.outline().items[2].selector,
        "impl[Wrapper<T>/Trait]"
    );
    assert_eq!(contents.outline().items[5].selector, "extern[Rust]");
}

// {test}`requirements.sdk.outlines.name_span`
#[test]
fn language_name_spans_retain_original_bytes_when_selectors_are_normalized() {
    for (path, source, expected) in [
        ("a.py", "@cache\nclass Café: pass\n", vec![Some("Café")]),
        (
            "a.ts",
            "function f() {}\nfunction f() {}",
            vec![Some("f"), Some("f")],
        ),
        ("a.tsx", "function F() { return <i/>; }", vec![Some("F")]),
        (
            "a.css",
            "@media  screen   and (color) { .a,  .b {} }",
            vec![Some("screen   and (color)"), Some(".a,  .b")],
        ),
        (
            "a.toml",
            "[a.b]\n'c/d' = 1",
            vec![Some("a.b"), Some("'c/d'")],
        ),
        ("a.yml", "key: [1]\n", vec![Some("key")]),
        (
            "a.md",
            ":::{note}\nCafé   title\n===\n:::\n",
            vec![None, None, Some("Café   title")],
        ),
        ("a.txt", "plain\n", vec![None]),
    ] {
        let root = OutlineContent::from_source(path, source.into()).unwrap();
        let items = &root.outline().items;
        assert_eq!(items[0].name_span, None);
        assert_eq!(
            items[1..]
                .iter()
                .map(|item| item.name_span.map(|span| &source[span]))
                .collect::<Vec<_>>(),
            expected,
            "{path}"
        );
        if path == "a.ts" {
            assert_ne!(items[1].name_span, items[2].name_span);
        }
    }
}

use trust_but_verify::outline::OutlineData;

// {test}`requirements.sdk.graph.search.TextMatch`
// {test}`requirements.sdk.graph.search.SearchResults`
// {test}`requirements.sdk.graph.search.StructuralMatch`
#[test]
fn callers_can_construct_complete_and_partial_search_results() {
    use trust_but_verify::sources::{SearchResults, StructuralMatch, TextMatch};

    let path = std::path::PathBuf::from("src/lib.rs");
    let found = TextMatch::new(path.clone(), 7, "fn example() {}".into());
    let partial = SearchResults::new(vec![found], false);
    let [found] = partial.matches.as_slice() else {
        panic!("expected one text match");
    };
    assert!(!partial.complete);
    assert_eq!(found.path, path);
    assert_eq!(found.line, 7);
    assert_eq!(found.excerpt, "fn example() {}");

    let address = OutlineData::new(path, "fn[example]".into()).unwrap();
    let found = StructuralMatch::new(address.clone(), "example".into());
    let complete = SearchResults::new(vec![found], true);
    assert!(complete.complete);
    assert_eq!(complete.matches.len(), 1);
    assert_eq!(complete.matches[0].node, address);
    assert_eq!(complete.matches[0].excerpt, "example");
}

// {test}`requirements.sdk.sources.source_addresses`
// {test}`requirements.references.outline_selectors`
#[test]
fn outline_addresses_validate_selectors_without_reading_paths() {
    let path = std::path::PathBuf::from("opaque/../é?key");
    for selector in ["", "fn[kept]", "mod[a%2Fb]/fn[kept]"] {
        let address = OutlineData::new(path.clone(), selector.into()).unwrap();
        assert_eq!(address.path, path);
        assert_eq!(address.selector, selector);
    }
    for selector in ["fn[missing", "fn[bad%ZZ]", "/fn[kept]"] {
        assert!(
            OutlineData::new(path.clone(), selector.into()).is_err(),
            "{selector}"
        );
    }
}

// {test}`requirements.sdk.graph.search.StructuralRule`
#[test]
fn compile_once_and_search_supplied_source() {
    use trust_but_verify::sources::{SourceError, StructuralRule};
    let core = serde_json::json!({"rule": {"kind": "call_expression"}});
    let core = core.as_object().unwrap();
    let rule = StructuralRule::compile("rust", core).unwrap();
    assert!(StructuralRule::languages().contains(&"rust"));
    assert!(rule.extensions().contains(&"rs"));
    for (text, expected) in [
        ("fn é() { f(g()); h(); }", &["f(g())", "g()", "h()"][..]),
        ("fn next() { g(); }", &["g()"][..]),
        ("fn chain() { f()(); }", &["f()", "f()()"][..]),
    ] {
        let matches = rule
            .matches(text)
            .into_iter()
            .map(|span| &text[span])
            .collect::<Vec<_>>();
        assert_eq!(matches, expected);
    }
    assert!(rule.matches("").is_empty());
    assert!(matches!(
        StructuralRule::compile("plain-text", core),
        Err(SourceError::Invalid(_))
    ));
    assert!(matches!(
        StructuralRule::compile("rust", &Default::default()),
        Err(SourceError::Invalid(_))
    ));
}

// {test}`requirements.sdk.graph.search.StructuralRule`
#[test]
fn nodes_covering_the_same_source_produce_one_span() {
    use trust_but_verify::outline::Span;
    use trust_but_verify::sources::StructuralRule;
    let text = "fn f() {}";
    // With no trailing newline, the file and its only function occupy identical bytes.
    for kinds in [
        serde_json::json!([{"kind":"source_file"}]),
        serde_json::json!([{"kind":"function_item"}]),
        serde_json::json!([{"kind":"source_file"}, {"kind":"function_item"}]),
    ] {
        let core = serde_json::json!({"rule": {"any": kinds}});
        let rule = StructuralRule::compile("rust", core.as_object().unwrap()).unwrap();
        assert_eq!(rule.matches(text), [Span::new(0, text.len())]);
    }
}

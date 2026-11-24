use trust_but_verify::outline::OutlineId;

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

    let address = OutlineId::new(path, "fn[example]".into()).unwrap();
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
        let address = OutlineId::new(path.clone(), selector.into()).unwrap();
        assert_eq!(address.path, path);
        assert_eq!(address.selector, selector);
    }
    for selector in ["fn[missing", "fn[bad%ZZ]", "/fn[kept]"] {
        assert!(
            OutlineId::new(path.clone(), selector.into()).is_err(),
            "{selector}"
        );
    }
}

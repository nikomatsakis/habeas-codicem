use habeas_sdk::{
    resource::{OutlineId, ResourceId},
    search::RegexSearch,
};

// {test}`requirements.sdk.sources.source_addresses`
// {test}`requirements.references.outline_selectors`
#[test]
fn outline_addresses_validate_selectors_without_reinterpreting_resources() {
    let resource = ResourceId {
        scheme: "generated".into(),
        location: "opaque/../é?key".into(),
    };
    for selector in ["", "fn[kept]", "mod[a%2Fb]/fn[kept]"] {
        let address = OutlineId::new(resource.clone(), selector.into()).unwrap();
        assert_eq!(address.resource, resource);
        assert_eq!(address.selector, selector);
    }
    for selector in ["fn[missing", "fn[bad%ZZ]", "/fn[kept]"] {
        assert!(
            OutlineId::new(resource.clone(), selector.into()).is_err(),
            "{selector}"
        );
    }
}

// {test}`requirements.authoring.source_tools`
#[test]
fn path_and_byte_search_agree_on_case_sensitive_one_based_matching_lines() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("sample.txt");
    let text = "Hit\nhit hit\nabsent\né hit\n";
    std::fs::write(&path, text).unwrap();
    let search = RegexSearch::compile("hit").unwrap();
    assert_eq!(search.search_path(&path).unwrap(), [2, 4]);
    assert_eq!(search.search_bytes(text.as_bytes()).unwrap(), [2, 4]);
    std::fs::write(&path, b"\0hit\nhit\n").unwrap();
    assert!(search.search_path(&path).unwrap().is_empty());
    assert!(search.search_bytes(b"\0hit\nhit\n").unwrap().is_empty());
    assert!(RegexSearch::compile("[").is_err());
    std::fs::remove_file(&path).unwrap();
    let error = search.search_path(&path).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    assert!(error.to_string().contains(path.to_str().unwrap()));
}

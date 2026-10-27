// {impl}`requirements.references.definition_globs`
pub(crate) fn validate(pattern: &str) -> Result<(), String> {
    if pattern.is_empty() {
        return Err("definition glob must not be empty".to_owned());
    }
    if let Some(character) = pattern.chars().find(|character| {
        !character.is_ascii_alphanumeric() && !matches!(character, '.' | '_' | '-' | '*')
    }) {
        return Err(format!(
            "invalid definition glob {pattern:?}: unsupported character {character:?}"
        ));
    }
    Ok(())
}

// {impl}`requirements.references.definition_globs`
pub(crate) fn matches(pattern: &str, name: &str) -> bool {
    let bytes = name.as_bytes();
    let mut previous = vec![false; bytes.len() + 1];
    previous[0] = true;
    for token in pattern.bytes() {
        let mut next = vec![false; bytes.len() + 1];
        if token == b'*' {
            next[0] = previous[0];
            for index in 1..=bytes.len() {
                next[index] = previous[index] || next[index - 1];
            }
        } else {
            for index in 1..=bytes.len() {
                next[index] = previous[index - 1] && bytes[index - 1] == token;
            }
        }
        previous = next;
    }
    previous[bytes.len()]
}

#[cfg(test)]
mod tests {
    use super::{matches, validate};

    // {test}`requirements.references.definition_globs`
    // {test}`requirements.references.definition_glob_cases`
    #[test]
    fn wildcard_crosses_dot_segments_and_matches_the_whole_name() {
        assert!(validate("foo.*.bar").is_ok());
        assert!(matches("foo.*.bar", "foo.x.bar"));
        assert!(matches("foo.*.bar", "foo.x.y.bar"));
        assert!(!matches("foo.*.bar", "prefix.foo.x.bar"));
        assert!(!matches("foo.bar.*", "foo.bar"));
        assert!(matches("foo.bar.*", "foo.bar.x.y"));
        assert!(validate("").is_err());
        assert!(validate("foo[bar]").is_err());
    }
}

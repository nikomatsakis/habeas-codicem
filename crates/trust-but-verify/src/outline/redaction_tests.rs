use super::{LegacyOutline as Outline, Redaction, parse_redacted, profile_for_path};
use crate::redaction::tests::{github_token, private_key};
use std::path::Path;

fn outline(path: &str, source: &str, redaction: &Redaction) -> Outline {
    parse_redacted(profile_for_path(Path::new(path)), source, redaction)
        .unwrap_or_else(|error| panic!("{path}: {error}"))
}

/// Every outline property except hashes: kinds, names, selectors of named
/// nodes, structure, and line and byte ranges.
fn structure(outline: &Outline) -> Vec<String> {
    outline
        .nodes
        .iter()
        .map(|node| {
            format!(
                "{} {:?} {:?} {:?} {}-{} {}-{}",
                node.kind,
                node.name,
                node.parent,
                node.children,
                node.line_start,
                node.line_end,
                node.byte_start,
                node.byte_end
            )
        })
        .collect()
}

// {test}`requirements.authoring.redaction`
// {test}`requirements.references.structural_hash`
// {test}`requirements.sdk.sources.redaction`
#[test]
fn hashes_do_not_depend_on_credential_values() {
    let hashes = |path: &str, source: &str| {
        let redaction = Redaction::of_text(source);
        outline(path, source, &redaction)
            .nodes
            .iter()
            .map(|node| (node.selector.clone(), node.structural_hash.clone()))
            .collect::<Vec<_>>()
    };
    let token = github_token();
    let other = format!("gh{}_{}", "p", "aB3dE5fG7hJ9kL1mN3pQ5rS7tV9wX1yZ3aB5");
    let rust = |token: &str| {
        format!(
            "/// Uses `{token}` by default.\nfn token() -> &'static str {{\n    \"{token}\"\n}}\n"
        )
    };
    assert_eq!(
        hashes("src/lib.rs", &rust(&token)),
        hashes("src/lib.rs", &rust(&other))
    );
    let markdown = |token: &str| format!("# Settings\nUse `{token}`.\n");
    assert_eq!(
        hashes("guide.md", &markdown(&token)),
        hashes("guide.md", &markdown(&other))
    );
    let text = |token: &str| format!("token {token}\nother line\n");
    assert_eq!(
        hashes("notes.txt", &text(&token)),
        hashes("notes.txt", &text(&other))
    );
    // Changing code outside a redacted region still changes the hash.
    assert_ne!(
        hashes("src/lib.rs", &rust(&token)),
        hashes("src/lib.rs", &rust(&token).replace("default", "design"))
    );
}

// {test}`requirements.authoring.redaction`
// {test}`requirements.references.outlines`
// {test}`requirements.sdk.sources.redaction`
#[test]
fn redaction_never_changes_an_outline() {
    let token = github_token();
    let cases = [
        // A YAML plain scalar: its redacted text would parse as a flow sequence.
        (
            "config.yaml",
            format!("service:\n  token: {token}\n  retries: 3\n"),
        ),
        (
            "src/lib.rs",
            format!(
                "const KEY: &str = r\"{}\";\nfn after() {{}}\n",
                private_key()
            ),
        ),
        (
            "habeas.toml",
            format!("[authoring.configs.local]\napi_key = '{token}'\n"),
        ),
        (
            "notes.md",
            format!("# Notes\n\nUse `{token}`.\n\n## Next\n\nText.\n"),
        ),
        ("notes.txt", format!("{}\nafter\n", private_key())),
    ];
    for (path, source) in cases {
        let redaction = Redaction::of_text(&source);
        assert!(!redaction.is_empty(), "{path}");
        let original = outline(path, &source, &Redaction::default());
        let redacted = outline(path, &source, &redaction);
        assert_eq!(structure(&original), structure(&redacted), "{path}");
        assert_eq!(
            redaction.apply(&source).lines().count(),
            source.lines().count(),
            "{path}"
        );
    }
    // Parsing the redacted YAML text itself would change the outline.
    let yaml = format!("service:\n  token: {token}\n  retries: 3\n");
    let redacted = Redaction::of_text(&yaml).apply(&yaml).into_owned();
    assert_eq!(
        redacted,
        "service:\n  token: [REDACTED:GitHub Token]\n  retries: 3\n"
    );
    assert_ne!(
        structure(&outline("config.yaml", &yaml, &Redaction::default())),
        structure(&outline("config.yaml", &redacted, &Redaction::default()))
    );
}

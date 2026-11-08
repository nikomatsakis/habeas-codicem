use std::path::Path;

use super::{Redaction, credential_fields, redact_text};
use crate::outline::{Outline, parse_redacted, profile_for_path};

// Credential-shaped values are assembled at run time so this source holds
// no literal match of a `secretshape` rule.

/// A GitHub classic token shape.
// {test}`requirements.authoring.redaction`
pub(crate) fn github_token() -> String {
    format!("gh{}_{}", "p", "16C7e42F292c6912E7710c838347Ae178B4a")
}

/// A PEM private key block whose body lines the entropy heuristic flags.
// {test}`requirements.authoring.redaction`
pub(crate) fn private_key() -> String {
    [
        format!("-----BEGIN {}-----", "RSA PRIVATE KEY"),
        "MIIEowIBAAKCAQEAu1SU1LfVLPHCozMxH2Mo4lgOEePzNm0tRgeLezV6ffAt0gun".to_owned(),
        "VTLw7onLRnrq0/IzW7yWR7QkrmBL7jTKEn5u+qKhbwKfBstIs+bMY2Zkp18gnTxK".to_owned(),
        "LxoS2tFczGkPLPgizskuemMghRniWaoLcyehkd3qqGElvW/VDL5AaWTg0nLVkjRo".to_owned(),
        format!("-----END {}-----", "RSA PRIVATE KEY"),
    ]
    .join("\n")
}

// {test}`requirements.authoring.redaction`
fn outline(path: &str, source: &str, redaction: &Redaction) -> Outline {
    parse_redacted(profile_for_path(Path::new(path)), source, redaction)
        .unwrap_or_else(|error| panic!("{path}: {error}"))
}

/// Every outline property except hashes: kinds, names, selectors of named
/// nodes, structure, and line and byte ranges.
// {test}`requirements.authoring.redaction`
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
// {test}`requirements.authoring.credentials`
#[test]
fn configuration_credential_fields_are_redacted_by_name_whatever_their_values() {
    let source = "version = 1\n\
        [authoring.configs.cloud]\n\
        backend = 'bedrock'\n\
        model = 'm'\n\
        [authoring.configs.cloud.credentials]\n\
        access_key_id = 'plainaccesskey'\n\
        secret_access_key = \"no prefix secret\"\n\
        session_token = '''\nsession\ntoken'''\n\
        [authoring.configs.local]\n\
        backend = 'ollama'\n\
        model = 'm'\n\
        api_key = 'ollama key'\n";
    let redacted = Redaction::of_file(Path::new("sub/habeas.toml"), source)
        .apply(source)
        .into_owned();
    assert_eq!(
        redacted,
        "version = 1\n\
        [authoring.configs.cloud]\n\
        backend = 'bedrock'\n\
        model = 'm'\n\
        [authoring.configs.cloud.credentials]\n\
        access_key_id = '[REDACTED:access_key_id]'\n\
        secret_access_key = \"[REDACTED:secret_access_key]\"\n\
        session_token = '''[REDACTED:session_token]\n\n'''\n\
        [authoring.configs.local]\n\
        backend = 'ollama'\n\
        model = 'm'\n\
        api_key = '[REDACTED:api_key]'\n"
    );
    assert_eq!(redacted.lines().count(), source.lines().count());
    let fields = credential_fields(source).unwrap();
    assert_eq!(
        fields
            .iter()
            .map(|field| field.setting.as_str())
            .collect::<Vec<_>>(),
        [
            "authoring.configs.cloud.credentials.access_key_id",
            "authoring.configs.cloud.credentials.secret_access_key",
            "authoring.configs.cloud.credentials.session_token",
            "authoring.configs.local.api_key",
        ]
    );
    // Inline tables are found by name too.
    let inline = "version = 1\n[authoring.configs.c]\ncredentials = { access_key_id = 'a1', secret_access_key = 's1' }\n";
    assert_eq!(
        Redaction::of_file(Path::new("habeas.toml"), inline).apply(inline),
        "version = 1\n[authoring.configs.c]\ncredentials = { access_key_id = '[REDACTED:access_key_id]', secret_access_key = '[REDACTED:secret_access_key]' }\n"
    );
    // The same text in a file that is not a configuration file has no
    // credential-shaped value, so it is unchanged.
    assert_eq!(
        Redaction::of_file(Path::new("notes.toml"), source).apply(source),
        source
    );
}

// {test}`requirements.authoring.redaction`
#[test]
fn private_keys_are_redacted_from_header_through_body_keeping_lines() {
    let key = private_key();
    let plain = format!("before\n{key}\nafter\n");
    let redacted = redact_text(&plain).into_owned();
    assert_eq!(
        redacted,
        format!(
            "before\n[REDACTED:Private Key Block]\n\n\n\n-----END {}-----\nafter\n",
            "RSA PRIVATE KEY"
        )
    );
    assert_eq!(redacted.lines().count(), plain.lines().count());

    let rust =
        format!("const KEY: &str = r\"{key}\";\n\nfn after() -> usize {{\n    KEY.len()\n}}\n");
    let redacted = Redaction::of_file(Path::new("src/key.rs"), &rust)
        .apply(&rust)
        .into_owned();
    assert!(!redacted.contains("MIIEow"), "{redacted}");
    assert!(!redacted.contains("VTLw7o"), "{redacted}");
    assert!(!redacted.contains("LxoS2t"), "{redacted}");
    assert!(
        redacted.starts_with("const KEY: &str = r\"[REDACTED:Private Key Block]\n\n\n\n-----END"),
        "{redacted}"
    );
    assert!(redacted.ends_with("\";\n\nfn after() -> usize {\n    KEY.len()\n}\n"));
    assert_eq!(redacted.lines().count(), rust.lines().count());
}

// {test}`requirements.authoring.redaction`
#[test]
fn tokens_in_code_are_redacted_without_adjacent_code() {
    let token = github_token();
    let source = format!(
        "fn client() -> Client {{\n    let token = \"{token}\";\n    Client::builder().token(token).retries(3).build()\n}}\n"
    );
    assert_eq!(
        redact_text(&source),
        "fn client() -> Client {\n    let token = \"[REDACTED:GitHub Token]\";\n    Client::builder().token(token).retries(3).build()\n}\n"
    );
}

// {test}`requirements.authoring.redaction`
#[test]
fn heuristic_findings_alone_are_not_redacted() {
    let source = "const DIGEST: &str = \"9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08\";\n\
        let value = Builder::new(Config42).with_parts(PartsV2::Default).finish_all(true)?;\n\
        let id = \"Xk7pQm2Rv9Ls4Tz8Wn3Yb6Hd\";\n";
    assert!(Redaction::of_text(source).is_empty());
    assert_eq!(redact_text(source), source);
}

// {test}`requirements.authoring.redaction`
// {test}`requirements.references.structural_hash`
#[test]
fn hashes_do_not_depend_on_credential_values() {
    let config = |key: &str| {
        format!(
            "version = 1\n[authoring.configs.cloud]\nbackend = 'bedrock'\nmodel = 'm'\n[authoring.configs.cloud.credentials]\naccess_key_id = '{key}'\nsecret_access_key = 'secret'\n"
        )
    };
    let hashes = |path: &str, source: &str| {
        let redaction = Redaction::of_file(Path::new(path), source);
        outline(path, source, &redaction)
            .nodes
            .iter()
            .map(|node| (node.selector.clone(), node.structural_hash.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        hashes("habeas.toml", &config("first")),
        hashes("habeas.toml", &config("second-value"))
    );
    assert_ne!(
        hashes("notes.toml", &config("first")),
        hashes("notes.toml", &config("second-value"))
    );

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
#[test]
fn redaction_never_changes_an_outline() {
    let token = github_token();
    let cases = [
        // A YAML plain scalar: its redacted text would parse as a flow sequence.
        ("config.yaml", format!("service:\n  token: {token}\n  retries: 3\n")),
        ("src/lib.rs", format!("const KEY: &str = r\"{}\";\nfn after() {{}}\n", private_key())),
        (
            "habeas.toml",
            "version = 1\n[authoring.configs.local]\nbackend = 'ollama'\nmodel = 'm'\napi_key = '''\nkey\n'''\n".to_owned(),
        ),
        ("notes.md", format!("# Notes\n\nUse `{token}`.\n\n## Next\n\nText.\n")),
        ("notes.txt", format!("{}\nafter\n", private_key())),
    ];
    for (path, source) in cases {
        let redaction = Redaction::of_file(Path::new(path), &source);
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

// {test}`requirements.authoring.redaction`
#[test]
fn slices_concatenate_to_the_redacted_whole() {
    let source = format!("a {}\nb\n", private_key());
    let redaction = Redaction::of_text(&source);
    let whole = redaction.apply(&source).into_owned();
    for split in (0..=source.len()).filter(|index| source.is_char_boundary(*index)) {
        let joined = format!(
            "{}{}",
            redaction.slice(&source, 0..split),
            redaction.slice(&source, split..source.len())
        );
        assert_eq!(joined, whole, "split at {split}");
    }
}

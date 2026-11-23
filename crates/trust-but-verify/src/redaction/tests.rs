use std::path::Path;

use super::{Redaction, credential_fields, is_configuration_file_for, redact_text};
use crate::outline::{Outline, parse_redacted, profile_for_path};

// Credential-shaped values are assembled at run time so this source holds
// no literal match of a `secretshape` rule.

/// A GitHub classic token shape.
pub(crate) fn github_token() -> String {
    format!("gh{}_{}", "p", "16C7e42F292c6912E7710c838347Ae178B4a")
}

/// A PEM private key block whose body lines the entropy heuristic flags.
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

// {test}`requirements.authoring.redaction`
#[test]
fn regions_extend_both_ways_keep_crlf_and_scan_past_a_mebibyte() {
    let token = github_token();
    let source = format!("let pair = \"Xk7pQm2Rv9Ls4Tz8Wn3Yb6Hd {token}\";\n");
    assert_eq!(
        redact_text(&source),
        "let pair = [REDACTED:GitHub Token]\";\n"
    );
    let crlf = format!("a\r\n{}\r\nb\r\n", private_key().replace('\n', "\r\n"));
    let redacted = redact_text(&crlf).into_owned();
    assert!(
        redacted.starts_with("a\r\n[REDACTED:Private Key Block]\r\n\r\n\r\n\r\n-----END"),
        "{redacted:?}"
    );
    assert_eq!(
        redacted.matches("\r\n").count(),
        crlf.matches("\r\n").count()
    );
    // Extension runs forward too, and repeats through each adjacent region.
    let chained =
        format!("let pair = \"{token} Xk7pQm2Rv9Ls4Tz8Wn3Yb6Hd Gq4Vx8Nc2Jw6Pf9Ry3Kt7Mb5\";\n");
    assert_eq!(
        redact_text(&chained),
        "let pair = \"[REDACTED:GitHub Token]\n"
    );
    let entropy = "Xk7pQm2Rv9Ls4Tz8Wn3Yb6Hd";
    for separator in [",", ";"] {
        for source in [
            format!("{entropy}{separator}{token}"),
            format!("{token}{separator}{entropy}"),
        ] {
            let redacted = redact_text(&source);
            assert!(redacted.contains(entropy), "{source}: {redacted}");
            assert!(redacted.contains(separator), "{source}: {redacted}");
            assert!(!redacted.contains(&token), "{source}: {redacted}");
        }
    }
    let spaced = format!("{entropy}\t\r\n{token}\n\t{entropy}");
    let redacted = redact_text(&spaced);
    assert!(!redacted.contains(entropy), "{redacted}");
    assert!(!redacted.contains(&token), "{redacted}");
    assert_eq!(redacted.matches('\n').count(), spaced.matches('\n').count());
    assert!(redacted.contains("\r\n"), "{redacted:?}");
    let large = format!("{}\n{token}\n", "x ".repeat(1 << 20));
    assert!(redact_text(&large).ends_with("\n[REDACTED:GitHub Token]\n"));
}

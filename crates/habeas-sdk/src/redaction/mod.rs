//! Credential redaction for source text that models receive or Habeas displays.
//!
//! Redaction locates byte regions of the original source and replaces each
//! with `[REDACTED:<kind>]` followed by the region's line breaks, so line
//! numbers never change. Outlines are built from the original source; their
//! hashes use [`Redaction::slice`] for each text piece.

use std::borrow::Cow;
use std::fs;
use std::ops::Range;
use std::path::Path;
use std::sync::OnceLock;

use secretshape::Scanner;
use toml_edit::{Item, Value};

/// The rule name `secretshape` gives findings of its entropy heuristic.
const HEURISTIC: &str = "high-entropy-token";

/// One replaced byte range of a source and the kind its marker names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Region {
    pub(crate) span: Range<usize>,
    pub(crate) kind: String,
}

/// The redacted regions of one source text: sorted, disjoint, and nonempty.
#[derive(Clone, Debug, Default)]
pub(crate) struct Redaction {
    regions: Vec<Region>,
}

/// An explicit credential value in a configuration file: its full setting
/// name, such as `authoring.configs.cloud.credentials.access_key_id`, and the
/// byte range of the value's contents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CredentialField {
    pub(crate) setting: String,
    pub(crate) field: String,
    pub(crate) span: Range<usize>,
}

impl Redaction {
    /// Redaction by `secretshape` rules alone, for text that is not a Habeas
    /// configuration file.
    pub(crate) fn of_text(source: &str) -> Self {
        Self::merge(secret_regions(source))
    }

    /// Redaction of a file: `secretshape` rules everywhere, plus the explicit
    /// credential fields when the file is a workspace `habeas.toml` or the
    /// user configuration file.
    pub(crate) fn of_file(path: &Path, source: &str) -> Self {
        let mut regions = secret_regions(source);
        if is_configuration_file(path) {
            regions.extend(configuration_regions(source));
        }
        Self::merge(regions)
    }

    /// Sorts regions and joins overlapping ones under the first one's kind.
    fn merge(mut regions: Vec<Region>) -> Self {
        regions.retain(|region| !region.span.is_empty());
        regions.sort_by_key(|region| (region.span.start, region.span.end));
        let mut merged: Vec<Region> = Vec::with_capacity(regions.len());
        for region in regions {
            match merged.last_mut() {
                Some(last) if region.span.start < last.span.end => {
                    last.span.end = last.span.end.max(region.span.end);
                }
                _ => merged.push(region),
            }
        }
        Self { regions: merged }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    /// The redacted form of `source[range]`. A region contributes its marker
    /// to the slice containing the region's first byte and its line breaks
    /// to every slice it overlaps, so slices of adjacent ranges concatenate
    /// to the redacted form of their union.
    pub(crate) fn slice<'a>(&self, source: &'a str, range: Range<usize>) -> Cow<'a, str> {
        let first = self
            .regions
            .partition_point(|region| region.span.end <= range.start);
        let mut output: Option<String> = None;
        let mut cursor = range.start;
        for region in self.regions[first..]
            .iter()
            .take_while(|region| region.span.start < range.end)
        {
            let output = output.get_or_insert_with(String::new);
            let start = region.span.start.max(range.start);
            let end = region.span.end.min(range.end);
            output.push_str(&source[cursor..start]);
            if region.span.start >= range.start {
                output.push_str("[REDACTED:");
                output.push_str(&region.kind);
                output.push(']');
            }
            push_line_breaks(output, &source[start..end]);
            cursor = end;
        }
        match output {
            None => Cow::Borrowed(&source[range]),
            Some(mut output) => {
                output.push_str(&source[cursor..range.end]);
                Cow::Owned(output)
            }
        }
    }

    /// The redacted form of the whole source.
    pub(crate) fn apply<'a>(&self, source: &'a str) -> Cow<'a, str> {
        self.slice(source, 0..source.len())
    }
}

/// Redacts text that is not a configuration file, such as a declaration or
/// a citation's commentary.
pub(crate) fn redact_text(text: &str) -> Cow<'_, str> {
    let redaction = Redaction::of_text(text);
    if redaction.is_empty() {
        return Cow::Borrowed(text);
    }
    Cow::Owned(redaction.apply(text).into_owned())
}

/// Redacts each line of a line list as one text, keeping the line count.
pub(crate) fn redact_lines(lines: &[String]) -> Vec<String> {
    let text = lines.join("\n");
    match redact_text(&text) {
        Cow::Borrowed(_) => lines.to_vec(),
        Cow::Owned(text) => text.split('\n').map(str::to_owned).collect(),
    }
}

/// Pushes the line breaks in `text`, keeping `\r\n` pairs.
fn push_line_breaks(output: &mut String, text: &str) {
    let bytes = text.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            if index > 0 && bytes[index - 1] == b'\r' {
                output.push('\r');
            }
            output.push('\n');
        }
    }
}

/// Rule-only and heuristic scanners over the whole input; `secretshape`
/// otherwise stops scanning after its default input cap.
fn scanners() -> &'static (Scanner, Scanner) {
    static SCANNERS: OnceLock<(Scanner, Scanner)> = OnceLock::new();
    SCANNERS.get_or_init(|| {
        (
            Scanner::new().max_input_bytes(usize::MAX),
            Scanner::new()
                .include_heuristics(true)
                .max_input_bytes(usize::MAX),
        )
    })
}

/// Regions of `secretshape` rule matches, each extended through adjacent
/// heuristic findings separated from it only by whitespace. A heuristic
/// finding never starts a region.
fn secret_regions(source: &str) -> Vec<Region> {
    let (rules, heuristics) = scanners();
    let findings = rules.scan(source);
    if findings.is_empty() {
        return Vec::new();
    }
    let flagged: Vec<Range<usize>> = heuristics
        .scan(source)
        .into_iter()
        .filter(|finding| finding.rule == HEURISTIC)
        .map(|finding| finding.span)
        .collect();
    let whitespace = |range: Range<usize>| source[range].chars().all(char::is_whitespace);
    let mut regions = Redaction::merge(
        findings
            .into_iter()
            .map(|finding| Region {
                span: finding.span,
                kind: finding.rule.into_owned(),
            })
            .collect(),
    )
    .regions;
    for region in &mut regions {
        // Flagged tokens are disjoint and sorted, so one pass in each
        // direction reaches the fixpoint.
        let mut next = flagged.partition_point(|span| span.end <= region.span.start);
        while let Some(span) = flagged.get(next) {
            if span.start < region.span.end || whitespace(region.span.end..span.start) {
                region.span.end = region.span.end.max(span.end);
                next += 1;
            } else {
                break;
            }
        }
        let mut previous = flagged.partition_point(|span| span.start < region.span.start);
        while previous > 0 {
            let span = &flagged[previous - 1];
            if span.end > region.span.start || whitespace(span.end..region.span.start) {
                region.span.start = span.start;
                previous -= 1;
            } else {
                break;
            }
        }
    }
    regions
}

/// Whether a file is a workspace `habeas.toml` or the user configuration
/// file, whose explicit credential fields are redacted by name.
fn is_configuration_file(path: &Path) -> bool {
    is_configuration_file_for(path, crate::config::user_config_path().ok().as_deref())
}

/// [`is_configuration_file`] with the user configuration path given: a file
/// named `habeas.toml`, the user file itself, or an alias resolving to it.
fn is_configuration_file_for(path: &Path, user: Option<&Path>) -> bool {
    if path.file_name().is_some_and(|name| name == "habeas.toml") {
        return true;
    }
    let Some(user) = user else {
        return false;
    };
    path == user
        || fs::canonicalize(user)
            .is_ok_and(|user| fs::canonicalize(path).is_ok_and(|path| path == user))
}

/// Regions of the explicit credential fields of a configuration file. A file
/// that does not parse as TOML cannot be searched by field, so all of it is
/// redacted with the kind `configuration`.
fn configuration_regions(source: &str) -> Vec<Region> {
    match credential_fields(source) {
        Some(fields) => fields
            .into_iter()
            .map(|field| Region {
                span: field.span,
                kind: field.field,
            })
            .collect(),
        None => vec![Region {
            span: 0..source.len(),
            kind: "configuration".to_owned(),
        }],
    }
}

/// The explicit credential fields of a Habeas configuration document: every
/// value under an authoring configuration's `credentials`, and its `api_key`.
/// Returns `None` when the document is not valid TOML.
pub(crate) fn credential_fields(source: &str) -> Option<Vec<CredentialField>> {
    let document = toml_edit::Document::parse(source).ok()?;
    let mut fields = Vec::new();
    let Some(configs) = document
        .get("authoring")
        .and_then(|authoring| authoring.get("configs"))
        .and_then(Item::as_table_like)
    else {
        return Some(fields);
    };
    for (name, config) in configs.iter() {
        let Some(config) = config.as_table_like() else {
            continue;
        };
        for key in ["credentials", "api_key"] {
            if let Some(item) = config.get(key) {
                let setting = format!("authoring.configs.{name}.{key}");
                item_fields(source, item, key, &setting, &mut fields);
            }
        }
    }
    fields.sort_by_key(|field| field.span.start);
    Some(fields)
}

/// Collects the leaf values under a credential item, each named by its key.
fn item_fields(
    source: &str,
    item: &Item,
    field: &str,
    setting: &str,
    fields: &mut Vec<CredentialField>,
) {
    match item {
        Item::Value(value) => value_fields(source, value, field, setting, fields),
        Item::Table(table) => {
            for (key, item) in table.iter() {
                item_fields(source, item, key, &format!("{setting}.{key}"), fields);
            }
        }
        Item::ArrayOfTables(tables) => {
            for table in tables.iter() {
                for (key, item) in table.iter() {
                    item_fields(source, item, key, &format!("{setting}.{key}"), fields);
                }
            }
        }
        Item::None => {}
    }
}

/// Collects the leaf values of a credential value. A string is redacted
/// inside its quotes so the replacement stays readable TOML.
fn value_fields(
    source: &str,
    value: &Value,
    field: &str,
    setting: &str,
    fields: &mut Vec<CredentialField>,
) {
    match value {
        Value::InlineTable(table) => {
            for (key, value) in table.iter() {
                value_fields(source, value, key, &format!("{setting}.{key}"), fields);
            }
        }
        Value::Array(values) => {
            for value in values.iter() {
                value_fields(source, value, field, setting, fields);
            }
        }
        _ => {
            let Some(mut span) = value.span() else {
                return;
            };
            if let Value::String(_) = value {
                let raw = &source[span.clone()];
                let quote = ["\"\"\"", "'''", "\"", "'"]
                    .into_iter()
                    .find(|quote| raw.len() >= 2 * quote.len() && raw.starts_with(quote))
                    .map_or(0, str::len);
                span = span.start + quote..span.end - quote;
            }
            fields.push(CredentialField {
                setting: setting.to_owned(),
                field: field.to_owned(),
                span,
            });
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;

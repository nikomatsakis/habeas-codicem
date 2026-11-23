use std::{fs, ops::Range, path::Path};

use toml_edit::{Item, Value};
use trust_but_verify::__habeas::redaction::{Redaction as SourceRedaction, Region};
pub(crate) use trust_but_verify::__habeas::redaction::{redact_lines, redact_text};

/// HC's configuration recognition layered on the SDK's fixed secret rules.
#[derive(Clone, Debug, Default)]
pub(crate) struct Redaction(SourceRedaction);

impl Redaction {
    /// Use generic source rules for a non-configuration observation.
    #[cfg(test)]
    pub(crate) fn of_text(source: &str) -> Self {
        Self(SourceRedaction::of_text(source))
    }

    /// Apply HC configuration fields as well as the generic SDK rules.
    pub(crate) fn of_file(path: &Path, source: &str) -> Self {
        Self::of_file_with_config(path, source, None)
    }

    // {impl}`requirements.authoring.redaction`
    // {impl}`requirements.authoring.credentials`
    pub(crate) fn of_file_with_config(path: &Path, source: &str, user: Option<&Path>) -> Self {
        let generic = SourceRedaction::of_text(source);
        Self(if is_configuration_file_for(path, user) {
            generic.with_regions(configuration_regions(source))
        } else {
            generic
        })
    }
}

impl std::ops::Deref for Redaction {
    type Target = SourceRedaction;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod configuration_tests;

#[cfg(test)]
pub(crate) mod tests {
    /// A GitHub classic token shape used by frontend integration tests.
    // {cite}`habeas:util`
    pub(crate) fn github_token() -> String {
        format!("gh{}_{}", "p", "16C7e42F292c6912E7710c838347Ae178B4a")
    }
}

/// An explicit credential value in a configuration file: its full setting
/// name, such as `authoring.configs.cloud.credentials.access_key_id`, and the
/// byte range of the value's contents.
#[derive(Clone, Debug, PartialEq, Eq)]
// {cite}`habeas:util`
pub struct CredentialField {
    pub setting: String,
    pub field: String,
    pub span: Range<usize>,
}

/// Whether this is a Habeas configuration file or an alias of the supplied user file.
// {cite}`habeas:util`
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
// {cite}`habeas:util`
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
// {cite}`habeas:util`
pub fn credential_fields(source: &str) -> Option<Vec<CredentialField>> {
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
// {cite}`habeas:util`
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
// {cite}`habeas:util`
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

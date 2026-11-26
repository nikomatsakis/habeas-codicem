use std::borrow::Cow;

use toml_edit::{Item, Value};
pub(crate) use trust_but_verify::sources::redact_text;

/// Redacts each line of a line list as one text, keeping the line count.
// {cite}`habeas:util`
pub(crate) fn redact_lines(lines: &[String]) -> Vec<String> {
    let text = lines.join("\n");
    match redact_text(&text) {
        Cow::Borrowed(_) => lines.to_vec(),
        Cow::Owned(text) => text.split('\n').map(str::to_owned).collect(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    /// A GitHub classic token shape used by frontend integration tests.
    // {cite}`habeas:util`
    pub(crate) fn github_token() -> String {
        format!("gh{}_{}", "p", "16C7e42F292c6912E7710c838347Ae178B4a")
    }

    #[test]
    fn credential_fields_reports_nested_settings_in_source_order() {
        let source = r#"
[authoring.configs]
unused = false
[authoring.configs.local]
api_key = "first"
credentials = { profiles = [{ token = "second" }, { key = "third" }] }
[[authoring.configs.cloud.credentials]]
token = "fourth"
"#;
        assert_eq!(
            super::credential_fields(source).unwrap(),
            [
                "authoring.configs.local.api_key",
                "authoring.configs.local.credentials.profiles.token",
                "authoring.configs.local.credentials.profiles.key",
                "authoring.configs.cloud.credentials.token",
            ]
        );
    }

    #[test]
    fn credential_fields_distinguishes_absent_settings_from_invalid_toml() {
        assert_eq!(super::credential_fields("version = 1"), Some(Vec::new()));
        assert_eq!(super::credential_fields("[authoring"), None);
    }
}

/// The explicit credential fields of a Habeas configuration document: every
/// value under an authoring configuration's `credentials`, and its `api_key`.
/// Returns `None` when the document is not valid TOML.
// {cite}`habeas:util`
pub(crate) fn credential_fields(source: &str) -> Option<Vec<String>> {
    let document = toml_edit::Document::parse(source).ok()?;
    let mut fields = Vec::new();
    let Some(configs) = document
        .get("authoring")
        .and_then(|authoring| authoring.get("configs"))
        .and_then(Item::as_table_like)
    else {
        return Some(Vec::new());
    };
    for (name, config) in configs.iter() {
        let Some(config) = config.as_table_like() else {
            continue;
        };
        for key in ["credentials", "api_key"] {
            if let Some(item) = config.get(key) {
                let setting = format!("authoring.configs.{name}.{key}");
                item_fields(item, &setting, &mut fields);
            }
        }
    }
    fields.sort_by_key(|(start, _)| *start);
    Some(fields.into_iter().map(|(_, setting)| setting).collect())
}

/// Collects the leaf values under a credential item, each named by its key.
// {cite}`habeas:util`
fn item_fields(item: &Item, setting: &str, fields: &mut Vec<(usize, String)>) {
    match item {
        Item::Value(value) => value_fields(value, setting, fields),
        Item::Table(table) => {
            for (key, item) in table.iter() {
                item_fields(item, &format!("{setting}.{key}"), fields);
            }
        }
        Item::ArrayOfTables(tables) => {
            for table in tables.iter() {
                for (key, item) in table.iter() {
                    item_fields(item, &format!("{setting}.{key}"), fields);
                }
            }
        }
        Item::None => {}
    }
}

/// Collect the names of explicit credential settings in source order.
// {cite}`habeas:util`
fn value_fields(value: &Value, setting: &str, fields: &mut Vec<(usize, String)>) {
    match value {
        Value::InlineTable(table) => {
            for (key, value) in table.iter() {
                value_fields(value, &format!("{setting}.{key}"), fields);
            }
        }
        Value::Array(values) => {
            for value in values.iter() {
                value_fields(value, setting, fields);
            }
        }
        _ => {
            if let Some(span) = value.span() {
                fields.push((span.start, setting.to_owned()));
            }
        }
    }
}

use serde_json::{Value, json};

/// A requirement or example assessment, independent of source freshness.
// {cite}`requirements.ci.outcomes`
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Judgment {
    True,
    False,
    Fixme(Vec<String>),
}

impl Judgment {
    // {impl}`requirements.ci.outcomes`
    // {impl}`requirements.ci.schema`
    pub fn from_value(value: &Value) -> Result<Self, String> {
        match value {
            Value::Bool(true) => Ok(Self::True),
            Value::Bool(false) => Ok(Self::False),
            Value::Object(object) if object.len() == 1 && object.contains_key("FIXME") => {
                let entries = object["FIXME"]
                    .as_array()
                    .filter(|entries| !entries.is_empty())
                    .ok_or("FIXME must be a nonempty array of nonempty strings")?;
                let entries = entries
                    .iter()
                    .map(|entry| {
                        entry
                            .as_str()
                            .filter(|entry| !entry.trim().is_empty())
                            .map(str::to_owned)
                            .ok_or("FIXME must be a nonempty array of nonempty strings")
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Self::Fixme(entries))
            }
            _ => Err(
                "expected true, false, or an object containing only FIXME with a nonempty array of nonempty strings"
                    .to_owned(),
            ),
        }
    }

    // {impl}`requirements.ci.outcomes`
    // {impl}`requirements.authoring.persistence`
    pub fn to_value(&self) -> Value {
        match self {
            Self::True => Value::Bool(true),
            Self::False => Value::Bool(false),
            Self::Fixme(entries) => json!({ "FIXME": entries }),
        }
    }

    // {impl}`requirements.authoring.usage_storage`
    pub fn kind(&self) -> &'static str {
        match self {
            Self::True => "true",
            Self::False => "false",
            Self::Fixme(_) => "FIXME",
        }
    }

    // {impl}`requirements.index.detail_format`
    // {impl}`requirements.cli.status`
    // {impl}`requirements.cli.fixme_summary`
    pub fn fixme_entries(&self) -> &[String] {
        match self {
            Self::Fixme(entries) => entries,
            Self::True | Self::False => &[],
        }
    }
}

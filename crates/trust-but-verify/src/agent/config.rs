//! Configuration of the SDK's supported model backends.

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, ffi::OsString, fmt, io};

/// A supported backend and its model, connection, and inference settings.
// {impl}`requirements.sdk.graph.agent-harness.agent_config`
// {impl}`requirements.sdk.graph.agent-harness.agent_config.serialization`
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "backend", rename_all = "lowercase")]
#[non_exhaustive]
pub enum AgentConfig {
    Bedrock(BedrockConfig),
    #[serde(rename = "openai")]
    OpenAi(OpenAiConfig),
    Ollama(OllamaConfig),
}

/// A credential whose Debug representation hides its value.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretString(String);

impl SecretString {
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl From<String> for SecretString {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for SecretString {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<redacted>")
    }
}

/// Explicit AWS credentials. Omitted credentials use AWS's standard resolution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BedrockCredentials {
    pub access_key_id: SecretString,
    pub secret_access_key: SecretString,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_token: Option<SecretString>,
}

/// Settings for an Amazon Bedrock model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BedrockConfig {
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aws_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials: Option<BedrockCredentials>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, OptionValue>,
}

/// Settings for an OpenAI Responses model.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenAiConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<SecretString>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, OptionValue>,
}

/// Settings for an Ollama model and server.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OllamaConfig {
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<SecretString>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, OptionValue>,
}

/// A backend inference setting, preserving nested maps and arrays.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OptionValue {
    String(String),
    Boolean(bool),
    Integer(i64),
    Float(f64),
    Array(Vec<OptionValue>),
    Map(BTreeMap<String, OptionValue>),
}

impl AgentConfig {
    pub(crate) fn options_json(&self) -> io::Result<serde_json::Value> {
        let values = match self {
            Self::Bedrock(config) => &config.options,
            Self::OpenAi(config) => &config.options,
            Self::Ollama(config) => &config.options,
        };
        for value in values.values() {
            value.validate()?;
        }
        let mut result = serde_json::to_value(values).map_err(io::Error::other)?;
        if matches!(self, Self::OpenAi(_)) {
            let options = result.as_object_mut().expect("options map");
            options
                .entry("max_output_tokens")
                .or_insert_with(|| serde_json::json!(16_384));
            let reasoning = options
                .entry("reasoning")
                .or_insert_with(|| serde_json::json!({}));
            if let Some(reasoning) = reasoning.as_object_mut() {
                reasoning
                    .entry("effort")
                    .or_insert_with(|| serde_json::json!("low"));
            }
        }
        Ok(result)
    }
}

impl OptionValue {
    fn validate(&self) -> io::Result<()> {
        match self {
            Self::Float(value) if !value.is_finite() => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "inference option floats must be finite",
            )),
            Self::Array(values) => values.iter().try_for_each(Self::validate),
            Self::Map(values) => values.values().try_for_each(Self::validate),
            _ => Ok(()),
        }
    }
}

fn credential(
    explicit: &Option<SecretString>,
    name: &str,
    lookup: impl Fn(&str) -> Option<OsString>,
) -> io::Result<String> {
    if let Some(value) = explicit {
        return Ok(value.expose_secret().to_owned());
    }
    lookup(name)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("authoring api_key_env {name:?} must name a set, nonempty UTF-8 environment variable"),
            )
        })
}

impl OpenAiConfig {
    pub(crate) fn model(&self) -> &str {
        self.model.as_deref().unwrap_or("gpt-6.1-sol")
    }

    pub(crate) fn endpoint(&self) -> &str {
        self.endpoint
            .as_deref()
            .unwrap_or("https://api.openai.com/v1")
    }

    pub(crate) fn api_key(&self) -> io::Result<String> {
        credential(
            &self.api_key,
            self.api_key_env.as_deref().unwrap_or("OPENAI_API_KEY"),
            |name| std::env::var_os(name),
        )
    }
}

impl OllamaConfig {
    pub(crate) fn endpoint(&self) -> &str {
        self.endpoint.as_deref().unwrap_or("http://localhost:11434")
    }

    pub(crate) fn api_key(&self) -> io::Result<Option<String>> {
        if let Some(key) = &self.api_key {
            return Ok(Some(key.expose_secret().to_owned()));
        }
        self.api_key_env
            .as_deref()
            .map(|name| credential(&self.api_key, name, |name| std::env::var_os(name)))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // {test}`requirements.sdk.graph.agent-harness.agent_config`
    // {test}`requirements.sdk.graph.agent-harness.agent_config.serialization`
    #[test]
    fn all_backends_preserve_nested_settings_and_credentials() {
        for value in [
            json!({"backend":"bedrock", "model":"example", "aws_profile":"dev",
                "region":"us-east-1", "credentials":{"access_key_id":"id",
                "secret_access_key":"secret", "session_token":"token"},
                "options":{"maxTokens":1024, "nested":{"enabled":true,"array":["x",2,0.5]}}}),
            json!({"backend":"openai", "model":"example", "endpoint":"http://localhost",
                "api_key":"key", "options":{"reasoning":{"effort":"high"}}}),
            json!({"backend":"ollama", "model":"example", "api_key_env":"TOKEN",
                "options":{"temperature":0.5}}),
        ] {
            let config: AgentConfig = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(&config).unwrap(), value);
            let copy: AgentConfig =
                serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
            assert_eq!(config, copy);
        }
    }

    // {test}`requirements.sdk.graph.agent-harness.agent_config.serialization`
    #[test]
    fn configuration_debug_hides_credentials() {
        let secret: SecretString = "an-explicit-secret".into();
        assert_eq!(secret.expose_secret(), "an-explicit-secret");
        assert_eq!(
            SecretString::from(String::from("owned-secret")).expose_secret(),
            "owned-secret"
        );
        assert_eq!(format!("{secret:?}"), "<redacted>");
        assert_eq!(serde_json::to_value(secret).unwrap(), "an-explicit-secret");
        assert!(
            serde_json::from_value::<AgentConfig>(
                json!({"backend":"openai","unknown_setting":true})
            )
            .is_err()
        );
    }

    // {test}`requirements.sdk.graph.agent-harness.agent_config`
    #[test]
    fn option_defaults_and_credential_resolution_are_deterministic() {
        let config = OpenAiConfig::default();
        assert_eq!(config.model(), "gpt-6.1-sol");
        assert_eq!(config.endpoint(), "https://api.openai.com/v1");
        let options = AgentConfig::OpenAi(config).options_json().unwrap();
        assert_eq!(options["max_output_tokens"], 16_384);
        assert_eq!(options["reasoning"]["effort"], "low");
        assert!(credential(&None, "MISSING", |_| None).is_err());
        assert!(credential(&None, "EMPTY", |_| Some("".into())).is_err());
        assert_eq!(
            credential(&None, "PRESENT", |_| Some("environment-value".into())).unwrap(),
            "environment-value"
        );
        assert_eq!(
            credential(&Some("explicit".into()), "MISSING", |_| None).unwrap(),
            "explicit"
        );
        assert!(
            AgentConfig::Ollama(OllamaConfig {
                model: "example".into(),
                endpoint: None,
                api_key_env: None,
                api_key: None,
                options: BTreeMap::from([(
                    "nested".into(),
                    OptionValue::Array(vec![OptionValue::Float(f64::NAN)])
                )]),
            })
            .options_json()
            .is_err()
        );
    }
}

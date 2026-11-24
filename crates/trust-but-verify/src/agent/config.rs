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

// {impl}`requirements.sdk.graph.agent-harness.agent_config.settings`
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

// {impl}`requirements.sdk.graph.agent-harness.agent_config.settings`
/// Explicit AWS credentials. Omitted credentials use AWS's standard resolution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct BedrockCredentials {
    pub access_key_id: SecretString,
    pub secret_access_key: SecretString,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_token: Option<SecretString>,
}

impl BedrockCredentials {
    pub fn new(access_key_id: SecretString, secret_access_key: SecretString) -> Self {
        Self {
            access_key_id,
            secret_access_key,
            session_token: None,
        }
    }

    pub fn session_token(mut self, session_token: SecretString) -> Self {
        self.session_token = Some(session_token);
        self
    }
}

// {impl}`requirements.sdk.graph.agent-harness.agent_config.settings`
/// Settings for an Amazon Bedrock model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
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

impl BedrockConfig {
    pub fn new(model: String) -> Self {
        Self {
            model,
            aws_profile: None,
            region: None,
            credentials: None,
            options: BTreeMap::new(),
        }
    }

    pub fn aws_profile(mut self, aws_profile: String) -> Self {
        self.aws_profile = Some(aws_profile);
        self
    }

    pub fn region(mut self, region: String) -> Self {
        self.region = Some(region);
        self
    }

    pub fn credentials(mut self, credentials: BedrockCredentials) -> Self {
        self.credentials = Some(credentials);
        self
    }

    pub fn option(mut self, key: String, value: OptionValue) -> Self {
        self.options.insert(key, value);
        self
    }
}

// {impl}`requirements.sdk.graph.agent-harness.agent_config.settings`
/// Settings for an OpenAI Responses model.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
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

impl OpenAiConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn model(mut self, model: String) -> Self {
        self.model = Some(model);
        self
    }

    pub fn endpoint(mut self, endpoint: String) -> Self {
        self.endpoint = Some(endpoint);
        self
    }

    pub fn api_key_env(mut self, api_key_env: String) -> Self {
        self.api_key_env = Some(api_key_env);
        self
    }

    pub fn api_key(mut self, api_key: SecretString) -> Self {
        self.api_key = Some(api_key);
        self
    }

    pub fn option(mut self, key: String, value: OptionValue) -> Self {
        self.options.insert(key, value);
        self
    }
}

// {impl}`requirements.sdk.graph.agent-harness.agent_config.settings`
/// Settings for an Ollama model and server.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
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

impl OllamaConfig {
    pub fn new(model: String) -> Self {
        Self {
            model,
            endpoint: None,
            api_key_env: None,
            api_key: None,
            options: BTreeMap::new(),
        }
    }

    pub fn endpoint(mut self, endpoint: String) -> Self {
        self.endpoint = Some(endpoint);
        self
    }

    pub fn api_key_env(mut self, api_key_env: String) -> Self {
        self.api_key_env = Some(api_key_env);
        self
    }

    pub fn api_key(mut self, api_key: SecretString) -> Self {
        self.api_key = Some(api_key);
        self
    }

    pub fn option(mut self, key: String, value: OptionValue) -> Self {
        self.options.insert(key, value);
        self
    }
}

// {impl}`requirements.sdk.graph.agent-harness.agent_config.settings`
/// A backend inference setting, preserving nested maps and arrays.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
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
    pub(crate) fn resolved_model(&self) -> &str {
        self.model.as_deref().unwrap_or("gpt-6.1-sol")
    }

    pub(crate) fn resolved_endpoint(&self) -> &str {
        self.endpoint
            .as_deref()
            .unwrap_or("https://api.openai.com/v1")
    }

    pub(crate) fn resolved_api_key(&self) -> io::Result<String> {
        credential(
            &self.api_key,
            self.api_key_env.as_deref().unwrap_or("OPENAI_API_KEY"),
            |name| std::env::var_os(name),
        )
    }
}

impl OllamaConfig {
    pub(crate) fn resolved_endpoint(&self) -> &str {
        self.endpoint.as_deref().unwrap_or("http://localhost:11434")
    }

    pub(crate) fn resolved_api_key(&self) -> io::Result<Option<String>> {
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

    // {test}`requirements.sdk.graph.agent-harness.agent_config.settings`
    // {test}`requirements.sdk.graph.agent-harness.BedrockConfig`
    // {test}`requirements.sdk.graph.agent-harness.BedrockCredentials`
    // {test}`requirements.sdk.graph.agent-harness.OpenAiConfig`
    // {test}`requirements.sdk.graph.agent-harness.OllamaConfig`
    // {test}`requirements.sdk.public_api`
    #[test]
    fn backend_builders_set_only_the_requested_fields() {
        let bedrock = BedrockConfig::new("bedrock-model".into());
        let openai = OpenAiConfig::new();
        let ollama = OllamaConfig::new("ollama-model".into());
        assert_eq!(
            serde_json::to_value(&bedrock).unwrap(),
            json!({"model": "bedrock-model"})
        );
        assert_eq!(serde_json::to_value(&openai).unwrap(), json!({}));
        assert_eq!(
            serde_json::to_value(&ollama).unwrap(),
            json!({"model": "ollama-model"})
        );

        let permanent = BedrockCredentials::new("id".into(), "secret".into());
        let encoded = json!({"access_key_id":"id", "secret_access_key":"secret"});
        assert_eq!(serde_json::to_value(&permanent).unwrap(), encoded);
        assert_eq!(
            serde_json::from_value::<BedrockCredentials>(encoded).unwrap(),
            permanent
        );
        let credentials = permanent.clone().session_token("token".into());
        assert_ne!(credentials, permanent);
        let bedrock = bedrock
            .aws_profile("dev".into())
            .region("region".into())
            .credentials(credentials)
            .option("a".into(), OptionValue::Integer(1))
            .option("b".into(), OptionValue::Boolean(true))
            .option("a".into(), OptionValue::Integer(2));
        assert_eq!(
            serde_json::to_value(bedrock).unwrap(),
            json!({
                "model": "bedrock-model", "aws_profile": "dev", "region": "region",
                "credentials": {"access_key_id": "id", "secret_access_key": "secret", "session_token": "token"},
                "options": {"a": 2, "b": true}
            })
        );
        let openai = openai
            .model("model".into())
            .endpoint("endpoint".into())
            .api_key_env("KEY".into())
            .api_key("key".into())
            .option("a".into(), OptionValue::Integer(2));
        let ollama = ollama
            .endpoint("endpoint".into())
            .api_key_env("KEY".into())
            .api_key("key".into())
            .option("a".into(), OptionValue::Integer(2));
        for (config, model) in [
            (serde_json::to_value(openai), "model"),
            (serde_json::to_value(ollama), "ollama-model"),
        ] {
            assert_eq!(
                config.unwrap(),
                json!({
                    "model": model, "endpoint": "endpoint", "api_key_env": "KEY", "api_key": "key", "options": {"a": 2}
                })
            );
        }
    }

    // {test}`requirements.sdk.graph.agent-harness.agent_config`
    // {test}`requirements.sdk.graph.agent-harness.agent_config.serialization`
    // {test}`requirements.sdk.graph.agent-harness.BedrockConfig`
    // {test}`requirements.sdk.graph.agent-harness.BedrockCredentials`
    // {test}`requirements.sdk.graph.agent-harness.OpenAiConfig`
    // {test}`requirements.sdk.graph.agent-harness.OllamaConfig`
    // {test}`requirements.sdk.graph.agent-harness.OptionValue`
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
    // {test}`requirements.sdk.graph.agent-harness.SecretString`
    // {test}`requirements.sdk.graph.agent-harness.agent_config.settings`
    #[test]
    fn configuration_debug_hides_credentials() {
        let secret: SecretString = "an-explicit-secret".into();
        assert_eq!(secret.expose_secret(), "an-explicit-secret");
        assert_eq!(
            SecretString::from(String::from("owned-secret")).expose_secret(),
            "owned-secret"
        );
        assert_eq!(format!("{secret:?}"), "<redacted>");
        assert_eq!(
            serde_json::from_value::<SecretString>(json!("an-explicit-secret")).unwrap(),
            secret.clone()
        );
        assert_ne!(secret, SecretString::from("another value"));
        assert_eq!(serde_json::to_value(secret).unwrap(), "an-explicit-secret");
        assert!(
            serde_json::from_value::<AgentConfig>(
                json!({"backend":"openai","unknown_setting":true})
            )
            .is_err()
        );
    }

    // {test}`requirements.sdk.graph.agent-harness.agent_config`
    // {test}`requirements.sdk.graph.agent-harness.OptionValue`
    // {test}`requirements.sdk.graph.agent-harness.OpenAiConfig`
    // {test}`requirements.sdk.graph.agent-harness.OllamaConfig`
    // {test}`requirements.sdk.graph.agent-harness.agent_config.settings`
    #[test]
    fn option_defaults_and_credential_resolution_are_deterministic() {
        let ollama = OllamaConfig::new("example".into());
        assert_eq!(ollama.resolved_endpoint(), "http://localhost:11434");
        assert_eq!(ollama.resolved_api_key().unwrap(), None);
        let config = OpenAiConfig::default();
        assert_eq!(config.resolved_model(), "gpt-6.1-sol");
        assert_eq!(config.resolved_endpoint(), "https://api.openai.com/v1");
        let options = AgentConfig::OpenAi(config).options_json().unwrap();
        assert_eq!(options["max_output_tokens"], 16_384);
        assert_eq!(options["reasoning"]["effort"], "low");
        assert!(credential(&None, "MISSING", |_| None).is_err());
        assert!(credential(&None, "EMPTY", |_| Some("".into())).is_err());
        assert!(
            OptionValue::Map(BTreeMap::from([(
                "nested".into(),
                OptionValue::Float(f64::INFINITY)
            )]))
            .validate()
            .is_err()
        );
        assert_eq!(
            serde_json::from_value::<OptionValue>(json!(1)).unwrap(),
            OptionValue::Integer(1)
        );
        assert_eq!(
            serde_json::from_value::<OptionValue>(json!(1.0)).unwrap(),
            OptionValue::Float(1.0)
        );
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

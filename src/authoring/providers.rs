use std::io;

use rig_core::completion::{CompletionRequest, CompletionResponse};
use rig_core::{DynModel, operation::Completion};
use serde_json::{Map, Value};

use crate::config::BackendConfig;

pub(super) trait CompletionProvider {
    // {impl}`requirements.authoring.provider`
    async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse>;
}

pub(super) struct RigProvider {
    model: DynModel<Completion>,
    options: serde_json::Value,
    bedrock: bool,
}

impl RigProvider {
    // {impl}`requirements.authoring.provider`
    // {impl}`requirements.authoring.rig_configuration_mapping`
    // {impl}`requirements.authoring.configuration_precedence`
    // {impl}`requirements.authoring.usage_requests`
    pub(super) async fn connect(config: &BackendConfig) -> io::Result<Self> {
        let model = match config {
            BackendConfig::Bedrock(config) => {
                // Keep each provider attempt visible to the usage ledger. An SDK
                // retry would otherwise be hidden inside one recorded request.
                let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
                    .retry_config(aws_config::retry::RetryConfig::disabled());
                if let Some(profile) = &config.aws_profile {
                    loader = loader.profile_name(profile);
                }
                if let Some(region) = &config.region {
                    loader = loader.region(aws_config::Region::new(region.clone()));
                }
                let sdk = aws_sdk_bedrockruntime::Client::new(&loader.load().await);
                rig_bedrock::client::BedrockRuntime::from(sdk)
                    .completion(&config.model)
                    .erase()
            }
            BackendConfig::Ollama(config) => {
                let mut provider = rig_core::providers::ollama::OllamaConfig::new()
                    .with_base_url(config.endpoint());
                if let Some(key) = config.api_key()? {
                    provider = provider.with_api_key(key);
                }
                provider.client().native_completion(&config.model).erase()
            }
        };
        Ok(Self {
            model,
            options: config.options_json()?,
            bedrock: matches!(config, BackendConfig::Bedrock(_)),
        })
    }
}

impl CompletionProvider for RigProvider {
    // {impl}`requirements.authoring.provider`
    // {impl}`requirements.authoring.configuration_storage`
    // {impl}`requirements.authoring.rig_configuration_mapping`
    async fn complete(&self, mut request: CompletionRequest) -> io::Result<CompletionResponse> {
        let mut options = self
            .options
            .as_object()
            .expect("configuration options are a map")
            .clone();
        if self.bedrock {
            apply_bedrock_options(&mut request, &mut options)?;
        }
        request.additional_params = (!options.is_empty()).then_some(Value::Object(options));
        self.model.call(request).await.map_err(io::Error::other)
    }
}

// {impl}`requirements.authoring.configuration_storage`
// {impl}`requirements.authoring.rig_configuration_mapping`
fn apply_bedrock_options(
    request: &mut CompletionRequest,
    options: &mut Map<String, Value>,
) -> io::Result<()> {
    if options.contains_key("inferenceConfig") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Bedrock options.inferenceConfig is not a nested wrapper; put temperature, maxTokens, topP, and stopSequences directly in the options table",
        ));
    }
    if let Some(value) = options.remove("temperature") {
        request.temperature = Some(
            value
                .as_f64()
                .ok_or_else(|| option_type("temperature", "a number"))?,
        );
    }
    if let Some(value) = options.remove("maxTokens") {
        let tokens = value
            .as_u64()
            .filter(|value| i32::try_from(*value).is_ok())
            .ok_or_else(|| {
                option_type(
                    "maxTokens",
                    "a nonnegative integer representable by Converse's signed 32-bit field",
                )
            })?;
        request.max_tokens = Some(tokens);
    }
    if let Some(value) = options.remove("topP") {
        request.options.top_p = Some(
            value
                .as_f64()
                .ok_or_else(|| option_type("topP", "a number"))?,
        );
    }
    if let Some(value) = options.remove("stopSequences") {
        request.options.stop = serde_json::from_value(value)
            .map_err(|_| option_type("stopSequences", "an array of strings"))?;
    }
    Ok(())
}

// {impl}`requirements.authoring.configuration_storage`
fn option_type(name: &str, expected: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("Bedrock options.{name} must be {expected}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // {test}`requirements.authoring.configuration_storage`
    // {test}`requirements.authoring.rig_configuration_mapping`
    #[test]
    fn bedrock_native_option_types_are_checked_without_supplying_defaults() {
        let mut request = CompletionRequest::new("prompt");
        let mut options = Map::from_iter([("top_k".into(), json!(12))]);
        apply_bedrock_options(&mut request, &mut options).unwrap();
        assert!(request.temperature.is_none());
        assert!(request.max_tokens.is_none());
        assert!(request.options.is_default());
        assert_eq!(options["top_k"], 12);
        for value in [
            json!({"temperature":"hot"}),
            json!({"maxTokens":-1}),
            json!({"maxTokens":2147483648_u64}),
            json!({"topP":true}),
            json!({"stopSequences":"stop"}),
            json!({"inferenceConfig":{}}),
        ] {
            let mut options = value.as_object().unwrap().clone();
            assert!(
                apply_bedrock_options(&mut CompletionRequest::new("prompt"), &mut options).is_err()
            );
        }
    }
}

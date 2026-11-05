use std::io;

use rig_core::completion::{CompletionRequest, CompletionResponse};
use rig_core::message::{AssistantContent, ToolCall, ToolFunction, ToolName};
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
        let mut response = self.model.call(request).await.map_err(io::Error::other)?;
        if self.bedrock {
            normalize_bedrock_client_calls(&mut response)?;
        }
        Ok(response)
    }
}

// {impl}`requirements.authoring.provider`
// {impl}`requirements.authoring.rig_configuration_mapping`
fn normalize_bedrock_client_calls(response: &mut CompletionResponse) -> io::Result<()> {
    // Rig 0.44 treats every typed Converse toolUse block as a hosted tool.
    // Newer Claude responses explicitly identify client calls as "tool_use".
    for block in &mut response.choice {
        let AssistantContent::Opaque(opaque) = block else {
            continue;
        };
        let Some(call) = opaque.item.get("toolUse") else {
            continue;
        };
        if call.get("type").and_then(Value::as_str) != Some("tool_use") {
            continue;
        }
        let id = call
            .get("toolUseId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| io::Error::other("Bedrock client tool call has no toolUseId"))?;
        let name = call
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| io::Error::other("Bedrock client tool call has no name"))?;
        let name = ToolName::new(name).map_err(io::Error::other)?;
        let input = call
            .get("input")
            .filter(|input| input.is_object())
            .ok_or_else(|| io::Error::other("Bedrock client tool call input must be an object"))?;
        let recovered = ToolCall::from_wire(id, ToolFunction::new(name, input.clone()));
        // Keeping the native item preserves the original block for replay,
        // including its type, without disturbing signed reasoning or origin.
        *block = AssistantContent::ToolCall(recovered).with_native(opaque.item.clone());
    }
    Ok(())
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
    use rig_core::completion::{FinishReason, Usage};
    use rig_core::message::{Opaque, Origin, Reasoning, Text};
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

    // {test}`requirements.authoring.provider`
    // {test}`requirements.authoring.rig_configuration_mapping`
    #[test]
    fn typed_client_calls_preserve_order_and_native_history() {
        let typed = json!({"toolUse":{
            "type":"tool_use", "toolUseId":"client-1", "name":"habeas",
            "input":{"file":"src/lib.rs","line_start":1}
        }});
        let reasoning = AssistantContent::Reasoning(Reasoning::new("Inspect the source."))
            .with_native(json!({"reasoningContent":{"reasoningText":{
                "text":"Inspect the source.","signature":"original-signature"
            }}}));
        let content = vec![
            reasoning,
            AssistantContent::Opaque(Opaque {
                item: typed.clone(),
                replay: true,
            }),
            AssistantContent::Text(Text::new("Then submit the judgment.")),
            AssistantContent::Opaque(Opaque {
                item: json!({"toolUse":{"type":"server_tool_use","toolUseId":"server-1","name":"server","input":{}}}),
                replay: true,
            }),
            AssistantContent::Opaque(Opaque {
                item: json!({"toolUse":{"type":"future_tool_use","toolUseId":"future-1","name":"future","input":{}}}),
                replay: true,
            }),
            AssistantContent::ToolCall(ToolCall::from_wire(
                "client-2",
                ToolFunction::new(ToolName::new("list_files").unwrap(), json!({})),
            )),
        ];
        let origin = Origin::new(
            "bedrock.converse",
            "aws_bedrock",
            "us.anthropic.claude-sonnet-5-5",
        );
        let raw = json!({"original":"provider response"});
        let mut response = CompletionResponse::new(
            content.clone(),
            Usage::default(),
            origin.clone(),
            raw.clone(),
        )
        .with_finish_reason(FinishReason::ToolCalls);
        response.provider_request_id = Some("request-1".into());

        normalize_bedrock_client_calls(&mut response).unwrap();
        assert_eq!(response.choice.len(), content.len());
        for index in [0, 2, 3, 4, 5] {
            assert_eq!(response.choice[index], content[index]);
        }
        let calls = response.tool_calls().collect::<Vec<_>>();
        assert_eq!(calls.len(), 2);
        assert_eq!(
            calls[0].id,
            rig_core::message::CallId::from_wire("client-1")
        );
        assert_eq!(calls[0].function.name.as_str(), "habeas");
        assert_eq!(
            calls[0].function.arguments,
            typed["toolUse"]["input"].as_object().unwrap().clone()
        );
        assert_eq!(response.choice[1].native_item(), Some(&typed));
        assert_eq!(
            calls[1].id,
            rig_core::message::CallId::from_wire("client-2")
        );
        assert_eq!(response.raw, raw);
        assert_eq!(response.origin, origin);
        assert_eq!(response.provider_request_id.as_deref(), Some("request-1"));
        assert_eq!(response.finish_reason(), Some(FinishReason::ToolCalls));
        let normalized = response.clone();
        normalize_bedrock_client_calls(&mut response).unwrap();
        assert_eq!(response, normalized);
    }

    // {test}`requirements.authoring.provider`
    // {test}`requirements.authoring.rig_configuration_mapping`
    #[test]
    fn malformed_typed_client_calls_are_reported() {
        for call in [
            json!({"type":"tool_use","name":"habeas","input":{}}),
            json!({"type":"tool_use","toolUseId":"id","input":{}}),
            json!({"type":"tool_use","toolUseId":"id","name":"habeas","input":[]}),
        ] {
            let mut response = CompletionResponse::new(
                vec![AssistantContent::Opaque(Opaque {
                    item: json!({"toolUse":call}),
                    replay: true,
                })],
                Usage::default(),
                Origin::new("bedrock.converse", "aws_bedrock", "model"),
                Value::Null,
            );
            assert!(normalize_bedrock_client_calls(&mut response).is_err());
        }
    }
}

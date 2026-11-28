//! Configure the SDK's supplied Bedrock, OpenAI, and Ollama agents.

use rig_core::{
    completion::{CompletionRequest, CompletionResponse, FinishReason, ToolDefinition},
    message::{Message, ToolCall, ToolResult, ToolResultContent, turn_failure},
};
use serde_json::Value;
use std::{future::Future, io, time::Duration};

#[path = "agent/pages.rs"]
mod pages;
use pages::Pages;
#[path = "agent/backoff.rs"]
#[doc(hidden)]
pub mod backoff;
pub(crate) use crate::agent as config;
#[path = "agent/configured.rs"]
mod configured;
#[path = "agent/project_sources.rs"]
pub(crate) mod project_sources;
#[path = "agent/providers.rs"]
#[doc(hidden)]
pub mod providers;
#[path = "agent/tools.rs"]
mod tools;
#[cfg(test)]
use crate::agent::{BedrockConfig, OllamaConfig, OptionValue};
#[doc(hidden)]
use backoff::{ProviderBackoff, Retry};
#[cfg(test)]
use backoff::{RETRY_LIMIT, failure_codes};
pub(crate) use configured::ConfiguredAgent;
#[doc(hidden)]
pub(crate) use tools::{ToolAgent, ToolDelivery, ToolFactory, ToolSession};

#[doc(hidden)]
pub trait CompletionProvider: Send + Sync {
    // {cite}`habeas:util`
    fn complete(
        &self,
        request: CompletionRequest,
    ) -> impl Future<Output = io::Result<CompletionResponse>> + Send;
    // {cite}`habeas:util`
    fn output_limit(&self) -> Option<u64> {
        None
    }
}

#[cfg(test)]
#[path = "agent/tests.rs"]
mod tests;

// {cite}`habeas:util`
#[doc(hidden)]
pub enum Submission<T> {
    Accepted(T),
    Retry(String),
}

/// Keeps domain failures from answer vetting distinct from provider and protocol failures.
#[doc(hidden)]
#[derive(Debug)]
pub enum AgentError<E> {
    Conversation(io::Error),
    Provider(io::Error),
    Domain(E),
}

impl<E> From<io::Error> for AgentError<E> {
    fn from(error: io::Error) -> Self {
        Self::Conversation(error)
    }
}

/// One call's state. A factory can create independent sessions for concurrent
/// calls while their delivered reads contribute to the same inspection trace.
#[doc(hidden)]
pub trait Conversation: Send {
    type Output: Send;

    // {cite}`habeas:util`
    fn tools(&self, submission: Option<ToolDefinition>) -> io::Result<Vec<ToolDefinition>>;
    // {cite}`habeas:util`
    fn submit(&mut self, name: &str, arguments: &Value) -> io::Result<Submission<Self::Output>>;
    // {cite}`habeas:util`
    /// Return the tool message and the number of new inputs it carries.
    fn dispatch(
        &mut self,
        name: &str,
        arguments: &Value,
    ) -> impl Future<Output = io::Result<(String, usize)>> + Send;
    // {cite}`habeas:util`
    fn delivered(&mut self) -> io::Result<()> {
        Ok(())
    }
    // {cite}`habeas:util`
    fn cut_off(&mut self, response: &CompletionResponse, limit: u64) -> Vec<Message>;
}

// Publish an interrupted attempt too. A dropped provider future supplies no token measurements.
struct ProviderAttempt<'a, 'd, D: crate::domain::Domain> {
    context: &'a crate::request::ValidationContext<'d, D>,
    usage: crate::graph::UsageStats,
}

impl<D: crate::domain::Domain> Drop for ProviderAttempt<'_, '_, D> {
    fn drop(&mut self) {
        self.context.record_usage(&self.usage);
    }
}

// {cite}`habeas:util`
fn tool_error(call: &ToolCall, text: impl Into<String>) -> ToolResult {
    ToolResult {
        is_error: true,
        ..call.result(vec![ToolResultContent::text(text)])
    }
}

// {impl}`requirements.sdk.graph.agent-harness.submission_count`
#[doc(hidden)]
pub async fn converse<P: CompletionProvider, C: Conversation, D: crate::domain::Domain, T>(
    context: &crate::request::ValidationContext<'_, D>,
    provider: &P,
    session: &mut C,
    mut request: CompletionRequest,
    opening_inputs: Vec<crate::request::PreparedInput<crate::domain::NodeId<D>>>,
    result_schema: Value,
    mut finish: impl FnMut(C::Output) -> Result<Submission<T>, AgentError<D::Error>> + Send,
) -> Result<T, AgentError<D::Error>>
where
    D::NodeData: serde::Serialize + serde::de::DeserializeOwned + schemars::JsonSchema,
    D::EdgeLabel: serde::de::DeserializeOwned + schemars::JsonSchema + PartialEq,
    D::Error: std::fmt::Display,
{
    let submit = "submit_result";
    request.tools = session
        .tools(Some(tools::submission_tool(result_schema)))?
        .into_iter()
        .map(Into::into)
        .collect();
    // Only names from the configured tool list belong in usage records.
    // Record entry before dispatch so cancellation retains the invocation.
    let begin_tool = |name: &str| {
        context.update_usage(|usage| usage.tools.entry(name.to_owned()).or_default().calls += 1);
    };
    let record_result = |name: &str, text: &str| {
        context.update_usage(|usage| {
            if let Some(tool) = usage.tools.get_mut(name) {
                tool.result_bytes += text.len() as u64;
            }
        });
    };
    let mut backoff = ProviderBackoff::default();
    let mut pending_inputs = opening_inputs.len() as u64;
    let mut opening_inputs = opening_inputs.into_iter();
    let mut retrying = false;
    let mut continued = false;
    let mut reminded = false;
    loop {
        let mut attempt = ProviderAttempt {
            context,
            usage: crate::graph::UsageStats::attempt(retrying, false, None),
        };
        let response = match tokio::time::timeout(
            Duration::from_secs(600),
            provider.complete(request.clone()),
        )
        .await
        {
            Ok(response) => response,
            Err(_) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "provider request had no response within ten minutes",
            )),
        };
        attempt.usage = crate::graph::UsageStats::attempt(
            retrying,
            response.is_err(),
            response.as_ref().ok().map(|r| &r.usage),
        );
        if response.is_ok() {
            attempt.usage.delivered_inputs = std::mem::take(&mut pending_inputs);
        }
        drop(attempt);
        let response = match response {
            Ok(response) => response,
            Err(error) => match backoff.retry(error, tokio::time::Instant::now().into_std()) {
                Retry::Wait(delay) => {
                    retrying = true;
                    tokio::time::sleep(delay).await;
                    continue;
                }
                Retry::Stop(error) => return Err(AgentError::Provider(error)),
            },
        };
        retrying = false;
        backoff = ProviderBackoff::default();
        for input in opening_inputs.by_ref() {
            context.record_read(input.node, input.fingerprint);
        }
        session.delivered()?;
        if matches!(response.finish_reason(), Some(FinishReason::Length)) {
            if response.raw.get("stopReason").and_then(Value::as_str)
                == Some("model_context_window_exceeded")
            {
                return Err(io::Error::other(
                    "the model's response was incomplete: the conversation exceeded the model's context window",
                ).into());
            }
            if continued {
                return Err(io::Error::other(
                    "the model's response was incomplete: it reached the output limit again after one continuation",
                ).into());
            }
            let limit = provider.output_limit().ok_or_else(|| io::Error::other(
                "the model's response was incomplete: it reached an output limit the configuration does not set"))?;
            continued = true;
            request.chat_history.extend(
                session
                    .cut_off(&response, limit)
                    .into_iter()
                    .map(std::sync::Arc::new),
            );
            continue;
        }
        if let Some(error) = turn_failure(
            &response.choice,
            Some(&response.stop()),
            response.finish_reason().as_ref(),
        ) {
            return Err(io::Error::other(error).into());
        }
        let calls: Vec<_> = response.tool_calls().cloned().collect();
        let submissions: Vec<_> = calls
            .iter()
            .filter(|call| call.function.name.as_str() == submit)
            .collect();
        if calls.is_empty() || submissions.len() > 1 {
            if reminded {
                return Err(io::Error::other(format!(
                    "model must invoke {submit} exactly once, even after a reminder; {} submissions received",
                    submissions.len()
                )).into());
            }
            reminded = true;
            request
                .chat_history
                .extend(response.message().map(std::sync::Arc::new));
            if !calls.is_empty() {
                let results = calls
                    .iter()
                    .map(|call| tool_error(call, format!("not run: invoke {submit} exactly once")))
                    .collect();
                request
                    .chat_history
                    .push(Message::tool_results(results).into());
            }
            let reminder =
                format!("Reminder: you MUST invoke {submit} exactly once to submit your result.");
            request.chat_history.push(Message::user(reminder).into());
            continue;
        }
        if !submissions.is_empty() {
            begin_tool(submit);
            let arguments = Value::Object(submissions[0].function.arguments.clone());
            let vetted = match tools::submitted_value(&arguments) {
                Ok(value) => match session.submit(submit, &value)? {
                    Submission::Accepted(value) => finish(value),
                    Submission::Retry(message) => Ok(Submission::Retry(message)),
                },
                Err(error) => Ok(Submission::Retry(error.to_string())),
            };
            match vetted {
                Ok(Submission::Accepted(value)) => {
                    return Ok(value);
                }
                Err(error) => {
                    return Err(error);
                }
                Ok(Submission::Retry(message)) => {
                    record_result(submit, &message);
                    request.chat_history.push(
                        response
                            .message()
                            .ok_or_else(|| io::Error::other("model returned an empty response"))?
                            .into(),
                    );
                    let results = calls
                        .iter()
                        .map(|call| {
                            let text = if call.function.name.as_str() == submit {
                                message.clone()
                            } else {
                                "not run: the response also made a submission".to_owned()
                            };
                            tool_error(call, text)
                        })
                        .collect();
                    request
                        .chat_history
                        .push(Message::tool_results(results).into());
                    continue;
                }
            }
        }
        request.chat_history.push(
            response
                .message()
                .ok_or_else(|| io::Error::other("model returned an empty response"))?
                .into(),
        );
        let mut results = Vec::new();
        for call in calls {
            let name = call.function.name.as_str();
            let arguments = Value::Object(call.function.arguments.clone());
            let known = request.tools.iter().any(|tool| tool.name == name);
            if known {
                begin_tool(name);
            }
            let delivery = session.dispatch(name, &arguments).await;
            let result = match delivery {
                Ok((text, inputs)) => {
                    if inputs > 0 {
                        pending_inputs += inputs as u64;
                        context.record_usage(&crate::graph::UsageStats {
                            prepared_inputs: inputs as u64,
                            ..Default::default()
                        });
                    }
                    if known {
                        record_result(name, &text);
                    }
                    call.result(vec![ToolResultContent::text(text)])
                }
                Err(error) => {
                    let text = error.to_string();
                    if known {
                        record_result(name, &text);
                    }
                    tool_error(&call, text)
                }
            };
            results.push(result);
        }
        request
            .chat_history
            .push(Message::tool_results(results).into());
    }
}

/// Execute a criterion using an application-provided conversation adapter.
#[doc(hidden)]
#[cfg(test)]
pub async fn verify_with_agent<D: crate::domain::Domain, V: crate::graph::Criterion<D>>(
    validator: &mut crate::graph::Validator<'_, D>,
    agent: &dyn crate::request::Agent<D>,
    criterion: V,
) -> Result<crate::graph::Fingerprinted<D, V::Ok>, crate::graph::ValidationError<D::Error>> {
    validator.execute(criterion, Some(agent)).await
}

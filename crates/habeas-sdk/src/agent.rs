//! The reusable tool conversation loop. Applications supply review policy,
//! source-tool adapters, and telemetry; providers only transport requests.

use rig_core::{
    completion::{CompletionRequest, CompletionResponse, FinishReason, ToolDefinition},
    message::{Message, ToolResultContent, UserContent, turn_failure},
};
use serde_json::Value;
use std::{
    future::Future,
    io,
    time::{Duration, Instant},
};

mod backoff;
mod sources;
mod tools;
pub use backoff::{ProviderBackoff, RETRY_LIMIT, Retry, failure_codes, retry_after};
pub use sources::SourceTools;
pub use tools::{NoTools, ToolAgent, ToolDelivery, ToolFactory, ToolSession};

pub trait CompletionProvider: Send + Sync {
    fn complete(
        &self,
        request: CompletionRequest,
    ) -> impl Future<Output = io::Result<CompletionResponse>> + Send;
    fn output_limit(&self) -> Option<u64> {
        None
    }
}

pub enum Submission<T> {
    Accepted(T),
    Retry(String),
}

/// One call's state. A factory can create independent sessions for concurrent
/// calls while their delivered reads contribute to the same inspection trace.
pub trait Conversation: Send {
    type Output: Send;

    fn tools(&self) -> io::Result<Vec<ToolDefinition>>;
    fn submit(&mut self, name: &str, arguments: &Value) -> io::Result<Submission<Self::Output>>;
    fn dispatch(
        &mut self,
        name: &str,
        arguments: &Value,
    ) -> impl Future<Output = io::Result<String>> + Send;
    fn delivered(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn begin_request(&mut self) -> io::Result<usize> {
        Ok(0)
    }
    fn end_request(
        &mut self,
        _index: usize,
        _elapsed: Duration,
        _response: &io::Result<CompletionResponse>,
    ) -> io::Result<()> {
        Ok(())
    }
    fn retried(&mut self, _index: usize) -> io::Result<()> {
        Ok(())
    }
    fn skipped_tool(&mut self, _name: &str) {}
    fn tool(&mut self, _name: &str, _failed: bool) {}
    fn save(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn cut_off(&mut self, response: &CompletionResponse, limit: u64) -> Vec<Message>;
}

// {impl}`requirements.sdk.graph.agent-calls.judge.submission`
// {impl}`requirements.authoring.submission_count`
// {impl}`requirements.authoring.provider_backoff`
// {impl}`requirements.authoring.output_limit`
pub async fn converse<P: CompletionProvider, C: Conversation>(
    provider: &P,
    session: &mut C,
    mut request: CompletionRequest,
    submit: &str,
) -> io::Result<C::Output> {
    request.tools = session.tools()?;
    let mut backoff = ProviderBackoff::default();
    let mut continued = false;
    let mut reminded = false;
    loop {
        let index = session.begin_request()?;
        let started = Instant::now();
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
        session.end_request(index, started.elapsed(), &response)?;
        let response = match response {
            Ok(response) => response,
            Err(error) => match backoff.retry(error, tokio::time::Instant::now().into_std()) {
                Retry::Wait(delay) => {
                    session.retried(index)?;
                    tokio::time::sleep(delay).await;
                    continue;
                }
                Retry::Stop(error) => return Err(error),
            },
        };
        backoff = ProviderBackoff::default();
        session.delivered()?;
        if matches!(response.finish_reason(), Some(FinishReason::Length)) {
            if response.raw.get("stopReason").and_then(Value::as_str)
                == Some("model_context_window_exceeded")
            {
                return Err(io::Error::other(
                    "the model's response was incomplete: the conversation exceeded the model's context window",
                ));
            }
            if continued {
                return Err(io::Error::other(
                    "the model's response was incomplete: it reached the output limit again after one continuation",
                ));
            }
            let limit = provider.output_limit().ok_or_else(|| io::Error::other(
                "the model's response was incomplete: it reached an output limit the configuration does not set"))?;
            continued = true;
            request
                .chat_history
                .extend(session.cut_off(&response, limit));
            session.save()?;
            continue;
        }
        if let Some(error) = turn_failure(
            &response.choice,
            Some(&response.stop()),
            response.finish_reason().as_ref(),
        ) {
            return Err(io::Error::other(error));
        }
        let calls: Vec<_> = response.tool_calls().cloned().collect();
        let submissions: Vec<_> = calls
            .iter()
            .filter(|call| call.function.name.as_str() == submit)
            .collect();
        if !submissions.is_empty() {
            for call in &calls {
                if call.function.name.as_str() != submit {
                    session.skipped_tool(call.function.name.as_str());
                }
            }
            if submissions.len() != 1 {
                for _ in &submissions {
                    session.tool(submit, true);
                }
                return Err(io::Error::other(format!(
                    "{submit} must be invoked exactly once; multiple submissions received"
                )));
            }
            let arguments = Value::Object(submissions[0].function.arguments.clone());
            match session.submit(submit, &arguments) {
                Ok(Submission::Accepted(value)) => {
                    session.tool(submit, false);
                    return Ok(value);
                }
                Err(error) => {
                    session.tool(submit, true);
                    return Err(error);
                }
                Ok(Submission::Retry(message)) => {
                    session.tool(submit, true);
                    request.chat_history.push(
                        response
                            .message()
                            .ok_or_else(|| io::Error::other("model returned an empty response"))?,
                    );
                    let results = calls
                        .iter()
                        .map(|call| {
                            let text = if call.function.name.as_str() == submit {
                                message.clone()
                            } else {
                                "not run: the response also made a submission".to_owned()
                            };
                            call.error_result(vec![ToolResultContent::text(text)])
                        })
                        .collect();
                    session.save()?;
                    request.chat_history.push(Message::tool_results(results));
                    continue;
                }
            }
        }
        if calls.is_empty() {
            if reminded {
                return Err(io::Error::other(format!(
                    "model finished without invoking {submit}, even after a reminder"
                )));
            }
            reminded = true;
            request.chat_history.extend(response.message());
            request.chat_history.push(Message::User {
                content: vec![UserContent::text(format!(
                    "Reminder: you MUST submit your result with the {submit} tool."
                ))],
            });
            session.save()?;
            continue;
        }
        request.chat_history.push(
            response
                .message()
                .ok_or_else(|| io::Error::other("model returned an empty response"))?,
        );
        let mut results = Vec::new();
        for call in calls {
            let name = call.function.name.as_str();
            let result = match session
                .dispatch(name, &Value::Object(call.function.arguments.clone()))
                .await
            {
                Ok(text) => {
                    session.tool(name, false);
                    call.result(vec![ToolResultContent::text(text)])
                }
                Err(error) => {
                    session.tool(name, true);
                    call.error_result(vec![ToolResultContent::text(error.to_string())])
                }
            };
            results.push(result);
        }
        session.save()?;
        request.chat_history.push(Message::tool_results(results));
    }
}

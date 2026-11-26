//! Configure the SDK's supplied Bedrock, OpenAI, and Ollama agents.

#[cfg(test)]
use crate::domain::NodeId;
use rig_core::{
    completion::{CompletionRequest, CompletionResponse, FinishReason, ToolDefinition},
    message::{Message, ToolResultContent, turn_failure},
};
use serde_json::Value;
use std::{
    future::Future,
    io,
    time::{Duration, Instant},
};

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
#[path = "agent/sources.rs"]
#[cfg(test)]
mod sources;
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

/// Built-in project source tools with an application graph-observation mapping.
///
/// File access and outline languages use the SDK's defaults. The mapping
/// receives the exact snapshot delivered to the agent.
#[doc(hidden)]
#[cfg(test)]
pub struct SourceTools<F> {
    inner: sources::SourceTools<crate::source_engine::LocalSources, F>,
}

#[cfg(test)]
impl<F> SourceTools<F> {
    // {impl}`requirements.sdk.sources.provider`
    /// Inspect this project directory and map selected items into the graph.
    pub fn new(root: impl Into<std::path::PathBuf>, observation: F) -> Self {
        Self {
            inner: sources::SourceTools {
                analysis: crate::sources::Sources::new(root).analysis,
                observation,
            },
        }
    }
}

#[cfg(test)]
impl<D, F> ToolFactory<D> for SourceTools<F>
where
    D: crate::domain::Domain,
    F: Fn(
            &crate::resource::OutlineId,
            &crate::source_engine::ParsedSource,
        ) -> Result<Option<crate::graph::Dependency<NodeId<D>, D::EdgeLabel>>, String>
        + Send
        + Sync,
{
    fn session<'a>(&'a self) -> Box<dyn ToolSession<D> + 'a>
    where
        NodeId<D>: 'a,
        D::EdgeLabel: 'a,
    {
        self.inner.session()
    }
}

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

/// One call's state. A factory can create independent sessions for concurrent
/// calls while their delivered reads contribute to the same inspection trace.
#[doc(hidden)]
pub trait Conversation: Send {
    type Output: Send;

    // {cite}`habeas:util`
    fn tools(&self) -> io::Result<Vec<ToolDefinition>>;
    // {cite}`habeas:util`
    fn submit(&mut self, name: &str, arguments: &Value) -> io::Result<Submission<Self::Output>>;
    // {cite}`habeas:util`
    fn dispatch(
        &mut self,
        name: &str,
        arguments: &Value,
    ) -> impl Future<Output = io::Result<String>> + Send;
    // {cite}`habeas:util`
    fn delivered(&mut self) -> io::Result<()> {
        Ok(())
    }
    // {cite}`habeas:util`
    fn begin_request(&mut self) -> io::Result<usize> {
        Ok(0)
    }
    // {cite}`habeas:util`
    fn end_request(
        &mut self,
        _index: usize,
        _elapsed: Duration,
        _response: &io::Result<CompletionResponse>,
    ) -> io::Result<()> {
        Ok(())
    }
    // {cite}`habeas:util`
    fn retried(&mut self, _index: usize) -> io::Result<()> {
        Ok(())
    }
    // {cite}`habeas:util`
    fn skipped_tool(&mut self, _name: &str) {}
    // {cite}`habeas:util`
    fn tool(&mut self, _name: &str, _failed: bool) {}
    // {cite}`habeas:util`
    fn save(&mut self) -> io::Result<()> {
        Ok(())
    }
    // {cite}`habeas:util`
    fn cut_off(&mut self, response: &CompletionResponse, limit: u64) -> Vec<Message>;
}

// Publish an interrupted attempt too. A dropped provider future supplies no token measurements.
// {impl}`requirements.sdk.validation-cache.usage`
struct ProviderAttempt<'a, 'd, D: crate::domain::Domain> {
    context: &'a crate::request::ValidationContext<'d, D>,
    usage: crate::graph::UsageStats,
}

impl<D: crate::domain::Domain> Drop for ProviderAttempt<'_, '_, D> {
    // {impl}`requirements.sdk.validation-cache.usage`
    // {impl}`requirements.sdk.graph.running-criteria.Validator.usage`
    fn drop(&mut self) {
        if let Some(local) = &self.context.usage {
            local
                .lock()
                .expect("usage lock poisoned")
                .accumulate(&self.usage);
        }
        self.context
            .graph()
            .cache()
            .usage
            .lock()
            .expect("usage lock poisoned")
            .accumulate(&self.usage);
    }
}

// {impl}`requirements.sdk.graph.agent-calls.judge.submission`
// {impl}`requirements.sdk.graph.agent-harness.submission.acceptance`
// {impl}`requirements.sdk.graph.agent-harness.submission.reminder`
// {impl}`requirements.authoring.submission_count`
// {impl}`requirements.authoring.provider_backoff`
// {impl}`requirements.authoring.output_limit`
#[doc(hidden)]
pub async fn converse<P: CompletionProvider, C: Conversation, D: crate::domain::Domain>(
    context: &crate::request::ValidationContext<'_, D>,
    provider: &P,
    session: &mut C,
    mut request: CompletionRequest,
    submit: &str,
) -> io::Result<C::Output> {
    request.tools = session.tools()?;
    let mut backoff = ProviderBackoff::default();
    let mut retrying = false;
    let mut continued = false;
    let mut reminded = false;
    loop {
        let index = session.begin_request()?;
        let started = Instant::now();
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
        drop(attempt);
        session.end_request(index, started.elapsed(), &response)?;
        let response = match response {
            Ok(response) => response,
            Err(error) => match backoff.retry(error, tokio::time::Instant::now().into_std()) {
                Retry::Wait(delay) => {
                    retrying = true;
                    session.retried(index)?;
                    tokio::time::sleep(delay).await;
                    continue;
                }
                Retry::Stop(error) => return Err(error),
            },
        };
        retrying = false;
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
        // Habeas's authoring bridge retains its separate submission protocol.
        if calls.is_empty() || (submit == "submit_result" && submissions.len() > 1) {
            if reminded {
                return Err(io::Error::other(format!(
                    "model must invoke {submit} exactly once, even after a reminder; {} submissions received",
                    submissions.len()
                )));
            }
            reminded = true;
            request.chat_history.extend(response.message());
            if !calls.is_empty() {
                let results = calls
                    .iter()
                    .map(|call| {
                        session.tool(call.function.name.as_str(), true);
                        call.error_result(vec![ToolResultContent::text(format!(
                            "not run: invoke {submit} exactly once"
                        ))])
                    })
                    .collect();
                request.chat_history.push(Message::tool_results(results));
            }
            let reminder = if submit == "submit_result" {
                format!("Reminder: you MUST invoke {submit} exactly once to submit your result.")
            } else {
                format!("Reminder: you MUST submit your result with the {submit} tool.")
            };
            request.chat_history.push(Message::user(reminder));
            session.save()?;
            continue;
        }
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

/// Execute a criterion using an application-provided conversation adapter.
#[doc(hidden)]
#[cfg(any(test, feature = "habeas-integration"))]
pub async fn verify_with_agent<D: crate::domain::Domain, V: crate::graph::Criterion<D>>(
    validator: &mut crate::graph::Validator<'_, D>,
    agent: &dyn crate::request::Agent<D>,
    criterion: V,
) -> Result<crate::graph::Fingerprinted<D, V::Ok>, crate::graph::ValidationError<D::Error>> {
    validator.execute(criterion, Some(agent)).await
}

use super::{AgentConfig, CompletionProvider, ToolAgent, ToolFactory, providers::RigProvider};
use crate::domain::Domain;
use crate::procedure::Agent;
use crate::procedure::AgentRequest;
use crate::request::ValidationContext;
use futures::future::BoxFuture;
use serde_json::Value;
use std::{io, sync::Arc};
use tokio::sync::OnceCell;

/// Backend state is initialized on the first awaited request.
pub(crate) struct ConfiguredAgent {
    config: AgentConfig,
    provider: OnceCell<Arc<RigProvider>>,
}

impl ConfiguredAgent {
    pub(crate) fn new(config: AgentConfig) -> Self {
        Self {
            config,
            provider: OnceCell::new(),
        }
    }
}

impl<D> Agent<D> for ConfiguredAgent
where
    D: Domain,
    D::EdgeLabel: serde::de::DeserializeOwned + schemars::JsonSchema + PartialEq,
    D::Error: std::fmt::Display,
{
    // {impl}`requirements.sdk.graph.agent-harness.with_agent`
    // {impl}`requirements.sdk.graph.agent-harness.tool_agent`
    // {impl}`requirements.sdk.graph.agent-harness.tool_agent.provider_failure`
    fn run<'a>(
        &'a self,
        request: AgentRequest<D::NodeId>,
        inspection: &'a ValidationContext<'_, D>,
    ) -> BoxFuture<'a, Result<Value, io::Error>> {
        Box::pin(async move {
            let provider = self
                .provider
                .get_or_try_init(|| async {
                    RigProvider::connect(&self.config).await.map(Arc::new)
                })
                .await?;
            let agent = ToolAgent::new(provider.clone());
            if let Some(project) = inspection.graph().project_sources() {
                agent.with_tools(project).run(request, inspection).await
            } else {
                agent.run(request, inspection).await
            }
        })
    }
}

impl<P: CompletionProvider + ?Sized> CompletionProvider for Arc<P> {
    async fn complete(
        &self,
        request: rig_core::completion::CompletionRequest,
    ) -> io::Result<rig_core::completion::CompletionResponse> {
        (**self).complete(request).await
    }

    fn output_limit(&self) -> Option<u64> {
        (**self).output_limit()
    }
}

impl<D: Domain, T: ToolFactory<D> + ?Sized> ToolFactory<D> for Arc<T> {
    fn session<'a>(&'a self) -> Box<dyn super::ToolSession<D> + 'a>
    where
        D::NodeId: 'a,
        D::EdgeLabel: 'a,
    {
        (**self).session()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::OpenAiConfig;
    use crate::domain::NodeContents;
    use crate::graph as validation;
    use crate::graph::Criterion;
    use crate::graph::CriterionPath;
    use crate::graph::CriterionVersion;
    use crate::graph::Fingerprint;
    use crate::graph::NodePath;
    use crate::graph::Record;
    use crate::request::ValidationContext;

    #[derive(Clone, Default)]
    struct TestDomain(
        Arc<std::sync::Mutex<Option<Record<u32, String>>>>,
        Arc<std::sync::Mutex<Vec<crate::outline::OutlineId>>>,
        crate::domain::ValidationCache,
    );
    struct Content;

    impl NodeContents for Content {
        fn fingerprint(&self) -> Fingerprint {
            Fingerprint {
                scheme: "test-v1".into(),
                value: "same".into(),
            }
        }

        fn format(&self) -> String {
            "a source snapshot".into()
        }
    }

    impl Domain for TestDomain {
        fn source_node(
            &self,
            item: &crate::outline::OutlineId,
        ) -> Result<u32, crate::sources::SourceError> {
            self.1.lock().unwrap().push(item.clone());
            match item.path.to_str() {
                Some("visible/selected.txt") => Ok(7),
                Some("alpha.txt") => Ok(8),
                Some("beta.txt") => Ok(9),
                Some("unused.txt") => Ok(10),
                _ => Ok(11),
            }
        }

        fn cache(&self) -> &crate::domain::ValidationCache {
            &self.2
        }
        type Error = String;
        type NodeId = u32;
        type NodeContent = Content;
        type EdgeLabel = String;

        fn node_path(&self, node: &u32) -> NodePath {
            NodePath::new(vec![node.to_string()])
        }

        fn node_content(&self, _: &u32) -> Result<Content, String> {
            Ok(Content)
        }

        fn outgoing_edges(&self, _: &u32, _: String) -> Result<Vec<u32>, String> {
            Ok(Vec::new())
        }

        async fn load_evidence(
            &self,
            _: &u32,
            _: &CriterionPath,
        ) -> Result<Option<Record<u32, String>>, String> {
            Ok(self.0.lock().unwrap().clone())
        }

        async fn store_evidence(
            &self,
            _: &u32,
            _: &CriterionPath,
            record: &Record<u32, String>,
        ) -> Result<(), String> {
            *self.0.lock().unwrap() = Some(record.clone());
            Ok(())
        }
    }

    struct Question(bool);

    impl Criterion<TestDomain> for Question {
        type Ok = bool;

        fn source_node_id(&self) -> u32 {
            1
        }

        fn criterion_path(&self) -> CriterionPath {
            CriterionPath::new(vec!["answer".into()])
        }

        fn version(&self) -> CriterionVersion {
            CriterionVersion::new("v1")
        }

        async fn execute(
            self,
            cx: &mut ValidationContext<'_, TestDomain>,
        ) -> Result<bool, crate::graph::ValidationError<String>> {
            if self.0 {
                cx.ask::<bool>().contents_of_current_node().await
            } else {
                Ok(!cx
                    .successors(&String::from("related"))
                    .map_err(crate::graph::ValidationError::Graph)?
                    .is_empty())
            }
        }
    }

    // {test}`requirements.sdk.graph.agent-harness.with_agent`
    // {test}`requirements.sdk.graph.agent-harness.tool_agent.provider_failure`
    // {test}`requirements.sdk.graph.criteria.deterministic_procedures`
    // {test}`requirements.sdk.validation-cache.with_agent`
    #[tokio::test]
    async fn configured_agents_initialize_only_when_asked_and_report_setup_errors() {
        // An explicitly empty credential fails before any request is sent.
        let config = AgentConfig::OpenAi(OpenAiConfig {
            api_key_env: Some("TBV_TEST_NONEXISTENT_CREDENTIAL_59a680".into()),
            ..OpenAiConfig::default()
        });
        let domain = TestDomain(
            Default::default(),
            Default::default(),
            crate::domain::ValidationCache::with_agent(config),
        );
        let agent = domain.cache().agent.as_ref().unwrap();
        assert!(agent.provider.get().is_none());
        let deterministic = validation::refresh_evidence(&domain, Question(false))
            .await
            .unwrap();
        assert!(!deterministic.output());
        assert!(agent.provider.get().is_none());
        assert_eq!(deterministic.dependencies().len(), 1);
        let error = match validation::verify(&domain, Question(true)).await {
            Ok(_) => panic!("missing credentials must fail the awaited ask"),
            Err(error) => error,
        };
        assert!(matches!(error, crate::graph::ValidationError::Agent(_)));
        assert!(
            error
                .to_string()
                .contains("TBV_TEST_NONEXISTENT_CREDENTIAL_59a680")
        );
    }

    // {test}`requirements.sdk.graph.agent-harness.setup`
    // {test}`requirements.sdk.graph.agent-harness.with_agent`
    // {test}`requirements.sdk.validation-cache.cache`
    #[tokio::test]
    // {test}`requirements.sdk.validation-cache.with_agent`
    async fn bedrock_configuration_attaches_to_a_cache_without_being_persisted() {
        let config = AgentConfig::Bedrock(
            super::super::BedrockConfig::new("us.anthropic.claude-sonnet-5-5".into())
                .aws_profile("development".into())
                .region("us-east-1".into()),
        );
        let mut domain = TestDomain::default();
        domain.2 = crate::domain::ValidationCache::with_agent(config.clone());
        assert!(domain.cache().outlines.lock().unwrap().is_empty());
        assert!(domain.cache().sources.is_none());
        let peer = domain.clone();
        assert!(Arc::ptr_eq(
            domain.cache().agent.as_ref().unwrap(),
            peer.cache().agent.as_ref().unwrap()
        ));
        assert_eq!(peer.cache().agent.as_ref().unwrap().config, config);
        let verified = validation::refresh_evidence(&domain, Question(false))
            .await
            .unwrap();
        assert!(!verified.output());
        let saved = serde_json::to_value(domain.0.lock().unwrap().as_ref().unwrap()).unwrap();
        assert!(saved.get("backend").is_none());
        assert!(!saved.to_string().contains("development"));
        assert!(!saved.to_string().contains("claude-sonnet"));
    }

    // {test}`requirements.sdk.graph.agent-harness.with_agent`
    // {test}`requirements.sdk.graph.agent-harness.tool_agent`
    // {test}`requirements.sdk.graph.agent-harness.tool_agent.isolation`
    // {test}`requirements.sdk.graph.agent-harness.submission`
    // {test}`requirements.sdk.graph.agent-harness.submission.mcp`
    // {test}`requirements.sdk.graph.agent-harness.submission.acceptance`
    // {test}`requirements.sdk.graph.agent-harness.fixed_tools`
    // {test}`requirements.sdk.graph.agent-harness.OpenAiConfig`
    // {test}`requirements.sdk.graph.validation-context.node_reference`
    // {test}`requirements.sdk.validation-cache.with_sources`
    // {test}`requirements.sdk.graph.search.source_identity`
    // {test}`requirements.sdk.sources.provider.attachment`
    #[tokio::test]
    async fn supplied_openai_backend_runs_independent_calls_against_a_local_transport() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            time::{Duration, Instant},
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(15);
            for call in 0..4 {
                let (mut stream, _) = loop {
                    match listener.accept() {
                        Ok(stream) => break stream,
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "missing local backend request");
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("cannot accept local request: {error}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut bytes = Vec::new();
                let header_end = loop {
                    let mut chunk = [0; 4096];
                    let read = stream.read(&mut chunk).unwrap();
                    assert!(read > 0, "request ended before its headers");
                    bytes.extend_from_slice(&chunk[..read]);
                    if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                        break end + 4;
                    }
                };
                let headers = std::str::from_utf8(&bytes[..header_end]).unwrap();
                assert!(headers.starts_with("POST /responses "));
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .unwrap();
                while bytes.len() < header_end + length {
                    let mut chunk = [0; 4096];
                    let read = stream.read(&mut chunk).unwrap();
                    assert!(read > 0, "request ended before its body");
                    bytes.extend_from_slice(&chunk[..read]);
                }
                requests.push(
                    serde_json::from_slice::<Value>(&bytes[header_end..header_end + length])
                        .unwrap(),
                );
                let body = serde_json::json!({
                    "id":format!("response_{call}"), "object":"response",
                    "created_at":1, "model":"local-test", "status":"completed",
                    "output":[{"type":"function_call", "id":format!("fc_{call}"),
                        "call_id":format!("call_{call}"),
                        "name":if call % 2 == 0 { "read_node" } else { "submit_result" },
                        "arguments":if call % 2 == 0 {
                            "{\"node\":\"n0\"}"
                        } else { "{\"result\":true}" },
                        "status":"completed"}],
                    "usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15}
                })
                .to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            requests
        });
        let mut domain = TestDomain::default();
        domain.2 = crate::domain::ValidationCache::with_agent(AgentConfig::OpenAi(OpenAiConfig {
            model: Some("local-test".into()),
            endpoint: Some(endpoint),
            api_key: Some("local-transport-key".into()),
            ..OpenAiConfig::default()
        }));
        for _ in 0..2 {
            let answer = validation::verify(&domain, Question(true)).await.unwrap();
            assert!(*answer.output());
            assert_eq!(answer.dependencies().len(), 1);
            validation::store_evidence(&domain, &answer).await.unwrap();
            let saved = serde_json::to_string(domain.0.lock().unwrap().as_ref().unwrap()).unwrap();
            for private in ["local-transport-key", "local-test", "backend", "api_key"] {
                assert!(!saved.contains(private), "{saved}");
            }
        }
        assert!(domain.1.lock().unwrap().is_empty());
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 4);
        for (index, request) in requests.into_iter().enumerate() {
            assert_eq!(request["model"], "local-test");
            assert_eq!(request["max_output_tokens"], 16_384);
            assert_eq!(
                request["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|tool| tool["name"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                [
                    "read_node",
                    "outgoing_edges",
                    "get_next_page",
                    "submit_result"
                ]
            );
            let submit = request["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["name"] == "submit_result")
                .unwrap();
            assert_eq!(
                submit["parameters"]["required"],
                serde_json::json!(["result"])
            );
            assert_eq!(
                submit["parameters"]["properties"]["result"]["type"],
                "boolean"
            );
            let input = serde_json::to_string(&request["input"]).unwrap();
            assert!(input.contains("Finish by calling submit_result"), "{input}");
            if index % 2 == 1 {
                assert!(input.contains("function_call_output"), "{input}");
                assert!(input.contains("a source snapshot"), "{input}");
            } else {
                assert!(!input.contains("function_call_output"), "{input}");
            }
        }
    }

    // {test}`requirements.sdk.graph.agent-harness.tool_agent`
    // {test}`requirements.sdk.graph.agent-harness.with_agent`
    // {test}`requirements.sdk.graph.agent-harness.submission`
    // {test}`requirements.sdk.graph.agent-harness.submission.mcp`
    // {test}`requirements.sdk.graph.agent-harness.OllamaConfig`
    #[tokio::test]
    async fn supplied_ollama_backend_delivers_tools_and_results_over_native_chat() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            time::{Duration, Instant},
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(15);
            for turn in 0..2 {
                let (mut stream, _) = loop {
                    match listener.accept() {
                        Ok(stream) => break stream,
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "missing native Ollama request");
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("cannot accept Ollama request: {error}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut bytes = Vec::new();
                let header_end = loop {
                    let mut chunk = [0; 4096];
                    let read = stream.read(&mut chunk).unwrap();
                    assert!(read > 0);
                    bytes.extend_from_slice(&chunk[..read]);
                    if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                        break end + 4;
                    }
                };
                let headers = std::str::from_utf8(&bytes[..header_end]).unwrap();
                assert!(headers.starts_with("POST /api/chat "));
                assert!(
                    headers
                        .to_ascii_lowercase()
                        .contains("authorization: bearer local-ollama-key")
                );
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .unwrap();
                while bytes.len() < header_end + length {
                    let mut chunk = [0; 4096];
                    let read = stream.read(&mut chunk).unwrap();
                    assert!(read > 0);
                    bytes.extend_from_slice(&chunk[..read]);
                }
                requests.push(
                    serde_json::from_slice::<Value>(&bytes[header_end..header_end + length])
                        .unwrap(),
                );
                let (name, arguments) = if turn == 0 {
                    ("read_node", serde_json::json!({"node":"n0"}))
                } else {
                    ("submit_result", serde_json::json!({"result":true}))
                };
                let body = serde_json::json!({
                    "model":"local-ollama", "created_at":"2026-11-23T00:00:00Z",
                    "done":true, "done_reason":"stop",
                    "message":{"role":"assistant","content":"",
                        "tool_calls":[{"id":format!("call_{turn}"),
                            "function":{"name":name,"arguments":arguments}}]},
                    "prompt_eval_count":10, "eval_count":5
                })
                .to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            requests
        });
        let config = AgentConfig::Ollama(super::super::OllamaConfig {
            model: "local-ollama".into(),
            endpoint: Some(endpoint),
            api_key_env: None,
            api_key: Some("local-ollama-key".into()),
            options: std::collections::BTreeMap::from([(
                "num_predict".into(),
                super::super::OptionValue::Integer(256),
            )]),
        });
        let domain = TestDomain(
            Default::default(),
            Default::default(),
            crate::domain::ValidationCache::with_agent(config),
        );
        let answer = validation::verify(&domain, Question(true)).await.unwrap();
        assert!(*answer.output());
        assert_eq!(answer.dependencies().len(), 1);
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        for (turn, request) in requests.iter().enumerate() {
            assert_eq!(request["model"], "local-ollama");
            assert_eq!(request["stream"], false);
            assert_eq!(request["options"]["num_predict"], 256);
            let submit = request["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["function"]["name"] == "submit_result")
                .unwrap();
            assert_eq!(
                submit["function"]["parameters"]["required"],
                serde_json::json!(["result"])
            );
            let messages = serde_json::to_string(&request["messages"]).unwrap();
            assert!(messages.contains("Finish by calling submit_result"));
            if turn == 1 {
                let delivered = request["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|message| message["role"] == "tool")
                    .unwrap();
                assert!(
                    delivered["content"]
                        .as_str()
                        .unwrap()
                        .contains("a source snapshot")
                );
            }
        }
    }

    // {test}`requirements.sdk.graph.search.finish_search.behavior`
    // {test}`requirements.sdk.graph.search.finish_search.inputs`
    // {test}`requirements.sdk.graph.agent-harness.submission.acceptance`
    // {test}`requirements.sdk.graph.search.search_text.inputs`
    // {test}`requirements.sdk.graph.validation-context.node_reference`
    // {test}`requirements.sdk.validation-cache.with_sources`
    // {test}`requirements.sdk.graph.search.source_identity`
    // {test}`requirements.sdk.sources.provider.attachment`
    // {test}`requirements.sdk.sources.local_provider`
    // {test}`requirements.sdk.sources.source_nodes`
    // {test}`requirements.sdk.sources.provider_observations`
    // {test}`requirements.sdk.graph.agent-harness.fixed_tools`
    // {test}`requirements.sdk.graph.validation-context.read_source.behavior`
    // {test}`requirements.sdk.graph.validation-context.list_sources.behavior`
    // {test}`requirements.sdk.graph.validation-context.list_sources.mcp`
    // {test}`requirements.sdk.sources.SourceProject`
    // {test}`requirements.sdk.graph.running-criteria.evaluate_criterion`
    #[tokio::test]
    async fn configured_agent_uses_domain_source_tools_in_independent_executions() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            time::{Duration, Instant},
        };
        let selected = tempfile::tempdir().unwrap();
        for file in ["alpha.txt", "beta.txt", "unused.txt"] {
            std::fs::write(selected.path().join(file), format!("excerpt {file}")).unwrap();
        }
        std::fs::create_dir(selected.path().join("visible")).unwrap();
        std::fs::create_dir(selected.path().join("visible-old")).unwrap();
        std::fs::write(
            selected.path().join("visible-old/outside.txt"),
            "outside scope",
        )
        .unwrap();
        std::fs::write(
            selected.path().join("visible/selected.txt"),
            "selected disk contents",
        )
        .unwrap();
        std::fs::write(
            selected.path().join("visible/ignored.txt"),
            "ignored contents",
        )
        .unwrap();
        std::fs::write(selected.path().join(".gitignore"), "visible/ignored.txt\n").unwrap();
        let files = crate::sources::Sources::new(selected.path())
            .list(&Default::default())
            .await
            .unwrap();
        let selected_index = files
            .iter()
            .position(|p| p == std::path::Path::new("visible/selected.txt"))
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(15);
            let mut page = Value::Null;
            fn last_output(request: &Value) -> &str {
                request["input"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .rev()
                    .find(|item| item["type"] == "function_call_output")
                    .unwrap()["output"]
                    .as_str()
                    .unwrap()
            }
            for turn in 0..32 {
                let (mut stream, _) = loop {
                    match listener.accept() {
                        Ok(stream) => break stream,
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "missing attached-tools request");
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("cannot accept attached-tools request: {error}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut bytes = Vec::new();
                let header_end = loop {
                    let mut chunk = [0; 4096];
                    let read = stream.read(&mut chunk).unwrap();
                    assert!(read > 0);
                    bytes.extend_from_slice(&chunk[..read]);
                    if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                        break end + 4;
                    }
                };
                let headers = std::str::from_utf8(&bytes[..header_end]).unwrap();
                assert!(headers.starts_with("POST /responses "));
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .unwrap();
                while bytes.len() < header_end + length {
                    let mut chunk = [0; 4096];
                    let read = stream.read(&mut chunk).unwrap();
                    assert!(read > 0);
                    bytes.extend_from_slice(&chunk[..read]);
                }
                requests.push(
                    serde_json::from_slice::<Value>(&bytes[header_end..header_end + length])
                        .unwrap(),
                );
                let (name, arguments) = match turn % 16 {
                    0 => ("list_sources", serde_json::json!({})),
                    1 => ("read_node", serde_json::json!({"node":"unknown"})),
                    2 => {
                        let listing: Value =
                            serde_json::from_str(last_output(&requests[turn - 1])).unwrap();
                        (
                            "read_node",
                            serde_json::json!({"node":listing["files"][selected_index]}),
                        )
                    }
                    3 | 4 => (
                        "search_text",
                        serde_json::json!({"pattern":"no matching line"}),
                    ),
                    6 => {
                        let ids = [turn - 2, turn - 1].map(|i| {
                            let args: Value =
                                serde_json::from_str(last_output(&requests[i])).unwrap();
                            args["search_id"].as_str().unwrap().to_owned()
                        });
                        assert_ne!(ids[0], ids[1]);
                        let retry = last_output(&requests[turn]);
                        assert!(retry.contains("Call finish_search for every open search_id"));
                        assert!(ids.iter().all(|id| retry.contains(id)));
                        ("prose", Value::Null)
                    }
                    step @ 7..=8 => {
                        if step == 7 {
                            assert!(
                                requests[turn]["input"].to_string().contains(
                                    "Reminder: you MUST invoke submit_result exactly once"
                                )
                            );
                        }
                        let empty: Value =
                            serde_json::from_str(last_output(&requests[turn - 3])).unwrap();
                        assert_eq!(empty.get("instructions").is_some(), step == 7);
                        (
                            "finish_search",
                            serde_json::json!({"search_id":empty["search_id"],"used":[]}),
                        )
                    }
                    9 => ("search_text", serde_json::json!({"pattern":"^excerpt "})),
                    step @ 10..=13 => {
                        if step == 10 {
                            page = serde_json::from_str(last_output(&requests[turn])).unwrap();
                            assert_eq!(page["matches"].as_array().unwrap().len(), 3);
                        }
                        if step == 12 {
                            assert!(last_output(&requests[turn]).contains("unknown search tag"));
                        }
                        let tag = |file: &str| {
                            page["matches"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .find(|hit| hit["excerpt"] == format!("excerpt {file}"))
                                .unwrap()["tag"]
                                .clone()
                        };
                        let used = match step {
                            11 => serde_json::json!([tag("unused.txt"), "invalid"]),
                            12 | 13 => serde_json::json!([tag("beta.txt")]),
                            _ => serde_json::json!([tag("alpha.txt")]),
                        };
                        (
                            "finish_search",
                            serde_json::json!({"search_id":page["search_id"], "used":used}),
                        )
                    }
                    14 => {
                        let hit = page["matches"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|hit| hit["excerpt"] == "excerpt unused.txt")
                            .unwrap();
                        ("read_node", serde_json::json!({"node": hit["node"]}))
                    }
                    _ => ("submit_result", serde_json::json!({"result":true})),
                };
                let reply = if name == "prose" {
                    serde_json::json!({"type":"message","id":format!("msg_{turn}"),"role":"assistant","status":"completed","content":[{"type":"output_text","text":"Tentative result","annotations":[]}]})
                } else {
                    serde_json::json!({"type":"function_call","id":format!("fc_{turn}"),
                        "call_id":format!("call_{turn}"),"name":name,
                        "arguments":arguments.to_string(),"status":"completed"})
                };
                let body = serde_json::json!({
                    "id":format!("response_{turn}"),"object":"response",
                    "created_at":1,"model":"local-source-test","status":"completed",
                    "output":[reply],
                    "usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15}
                })
                .to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            requests
        });
        let config = AgentConfig::OpenAi(OpenAiConfig {
            model: Some("local-source-test".into()),
            endpoint: Some(endpoint),
            api_key: Some("local-source-key".into()),
            ..Default::default()
        });
        let domain = TestDomain(
            Default::default(),
            Default::default(),
            crate::domain::ValidationCache::with_agent(config)
                .with_sources(crate::sources::Sources::new(selected.path())),
        );
        for _ in 0..2 {
            domain.1.lock().unwrap().clear();
            let answer = validation::verify(&domain, Question(true)).await.unwrap();
            assert!(*answer.output());
            assert_eq!(
                answer.dependencies(),
                &[
                    crate::graph::Dependency::Node(1, Content.fingerprint()),
                    crate::graph::Dependency::Node(7, Content.fingerprint()),
                    crate::graph::Dependency::Node(
                        8,
                        crate::outline::OutlineContent::from_source(
                            domain.cache(),
                            "alpha.txt",
                            "excerpt alpha.txt".into()
                        )
                        .unwrap()
                        .at_offset(0)
                        .fingerprint()
                    ),
                    crate::graph::Dependency::Node(
                        9,
                        crate::outline::OutlineContent::from_source(
                            domain.cache(),
                            "beta.txt",
                            "excerpt beta.txt".into()
                        )
                        .unwrap()
                        .at_offset(0)
                        .fingerprint()
                    ),
                    crate::graph::Dependency::Node(10, Content.fingerprint()),
                ]
            );
            assert_eq!(
                domain
                    .1
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|id| id.path.starts_with("visible"))
                    .cloned()
                    .collect::<Vec<_>>(),
                [crate::outline::OutlineId::new("visible/selected.txt", String::new()).unwrap()]
            );
        }
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 32);
        for first in [0, 16] {
            let tools = requests[first]["tools"].as_array().unwrap();
            assert_eq!(
                tools
                    .iter()
                    .map(|tool| tool["name"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                [
                    "read_node",
                    "outgoing_edges",
                    "get_next_page",
                    "submit_result",
                    "list_sources",
                    "outline_source",
                    "search_text",
                    "search_structure",
                    "finish_search"
                ]
            );
            let output = |request: &Value, name: &str| -> String {
                request["input"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|item| item["type"] == "function_call_output")
                    .last()
                    .unwrap_or_else(|| panic!("missing {name} tool delivery"))["output"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            };
            let listing: Value =
                serde_json::from_str(&output(&requests[first + 1], "list_sources")).unwrap();
            assert!(
                listing["files"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|n| n.as_str().unwrap().starts_with('n'))
            );
            let rejected = output(&requests[first + 2], "read_node");
            assert!(rejected.contains("unknown node"));
            let mapped: Value =
                serde_json::from_str(&output(&requests[first + 3], "read_node")).unwrap();
            assert_eq!(mapped["text"], "a source snapshot");
            assert!(!mapped.to_string().contains("selected disk contents"));
        }
    }
}

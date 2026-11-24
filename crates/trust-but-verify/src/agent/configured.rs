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
pub(crate) struct ConfiguredAgent<D: Domain> {
    config: AgentConfig,
    provider: OnceCell<Arc<RigProvider>>,
    tools: Arc<dyn ToolFactory<D>>,
}

impl<D: Domain> ConfiguredAgent<D> {
    #[cfg(test)]
    pub(crate) fn new(config: AgentConfig) -> Self {
        Self::with_tools(config, Arc::new(super::NoTools))
    }

    pub(crate) fn with_tools(config: AgentConfig, tools: Arc<dyn ToolFactory<D>>) -> Self {
        Self {
            config,
            provider: OnceCell::new(),
            tools,
        }
    }
}

impl<D> Agent<D> for ConfiguredAgent<D>
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
            ToolAgent::new(provider.clone())
                .with_tools(self.tools.clone())
                .run(request, inspection)
                .await
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
    use crate::graph::Criterion;
    use crate::graph::CriterionPath;
    use crate::graph::CriterionVersion;
    use crate::graph::Fingerprint;
    use crate::graph::NodePath;
    use crate::graph::Record;
    use crate::graph::validator::Validator;
    use crate::request::ValidationContext;

    #[derive(Clone, Default)]
    struct TestDomain(
        Arc<std::sync::Mutex<Option<Record<u32, String>>>>,
        Arc<std::sync::Mutex<Vec<crate::outline::OutlineId>>>,
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

    impl crate::domain::SourceNodes for TestDomain {
        fn source_node(&self, item: &crate::outline::OutlineId) -> Result<u32, String> {
            assert_eq!(item.path, std::path::Path::new("visible/selected.txt"));
            assert!(item.selector.is_empty());
            self.1.lock().unwrap().push(item.clone());
            Ok(7)
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
    #[tokio::test]
    async fn configured_agents_initialize_only_when_asked_and_report_setup_errors() {
        // An explicitly empty credential fails before any request is sent.
        let config = AgentConfig::OpenAi(OpenAiConfig {
            api_key_env: Some("TBV_TEST_NONEXISTENT_CREDENTIAL_59a680".into()),
            ..OpenAiConfig::default()
        });
        let agent = ConfiguredAgent::<TestDomain>::new(config.clone());
        assert!(agent.provider.get().is_none());
        let validator = Validator::new(TestDomain::default()).with_agent(config);
        let deterministic = validator.refresh_evidence(Question(false)).await.unwrap();
        assert!(!deterministic.output());
        assert_eq!(deterministic.dependencies().len(), 1);
        let error = match validator.verify(Question(true)).await {
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
    #[tokio::test]
    async fn bedrock_configuration_attaches_to_a_validator_without_being_persisted() {
        let config = AgentConfig::Bedrock(super::super::BedrockConfig {
            model: "us.anthropic.claude-sonnet-5-5".into(),
            aws_profile: Some("development".into()),
            region: Some("us-east-1".into()),
            credentials: None,
            options: Default::default(),
        });
        let domain = TestDomain::default();
        let validator = Validator::new(domain.clone()).with_agent(config);
        let verified = validator.refresh_evidence(Question(false)).await.unwrap();
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
                            "{\"path\":{\"segments\":[\"1\"]}}"
                        } else { "{\"result\":true}" },
                        "status":"completed"}],
                    "usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15}
                })
                .to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            requests
        });
        let domain = TestDomain::default();
        let validator =
            Validator::new(domain.clone()).with_agent(AgentConfig::OpenAi(OpenAiConfig {
                model: Some("local-test".into()),
                endpoint: Some(endpoint),
                api_key: Some("local-transport-key".into()),
                ..OpenAiConfig::default()
            }));
        for _ in 0..2 {
            let answer = validator.verify(Question(true)).await.unwrap();
            assert!(*answer.output());
            assert_eq!(answer.dependencies().len(), 1);
            validator.store_evidence(&answer).await.unwrap();
            let saved = serde_json::to_string(domain.0.lock().unwrap().as_ref().unwrap()).unwrap();
            for private in ["local-transport-key", "local-test", "backend", "api_key"] {
                assert!(!saved.contains(private), "{saved}");
            }
        }
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
                    ("read_node", serde_json::json!({"path":{"segments":["1"]}}))
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
        let validator = Validator::new(TestDomain::default()).with_agent(config);
        let answer = validator.verify(Question(true)).await.unwrap();
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
    // {test}`requirements.sdk.sources.provider.attachment`
    // {test}`requirements.sdk.sources.local_provider`
    // {test}`requirements.sdk.sources.source_nodes`
    // {test}`requirements.sdk.sources.provider_observations`
    // {test}`requirements.sdk.graph.agent-harness.fixed_tools`
    // {test}`requirements.sdk.graph.validation-context.read_source.behavior`
    // {test}`requirements.sdk.graph.validation-context.list_sources.behavior`
    #[tokio::test]
    async fn supplied_agent_uses_attached_project_tools_in_both_configuration_orders() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            time::{Duration, Instant},
        };
        let selected = tempfile::tempdir().unwrap();
        std::fs::create_dir(selected.path().join("visible")).unwrap();
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
        let wrong = tempfile::tempdir().unwrap();
        std::fs::create_dir(wrong.path().join("visible")).unwrap();
        std::fs::write(
            wrong.path().join("visible/wrong-project.txt"),
            "wrong project",
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(15);
            for turn in 0..14 {
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
                let (name, arguments) = match turn % 7 {
                    0 => ("list_sources", serde_json::json!({"prefix":"visible"})),
                    1 => (
                        "read_source",
                        serde_json::json!({"path":"visible/ignored.txt"}),
                    ),
                    2 => (
                        "read_source",
                        serde_json::json!({"path":"visible/selected.txt"}),
                    ),
                    3 => (
                        "search_text",
                        serde_json::json!({"pattern":"no matching line"}),
                    ),
                    5 => {
                        let reply = &requests[turn - 1]["input"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .rev()
                            .find(|item| item["type"] == "function_call_output")
                            .unwrap()["output"];
                        let page: Value = serde_json::from_str(reply.as_str().unwrap()).unwrap();
                        assert!(page["instructions"].is_string());
                        let retry = requests[turn]["input"].to_string();
                        assert!(retry.contains("Call finish_search for every open search_id"));
                        (
                            "finish_search",
                            serde_json::json!({"search_id":page["search_id"],"used":[]}),
                        )
                    }
                    _ => ("submit_result", serde_json::json!({"result":true})),
                };
                let body = serde_json::json!({
                    "id":format!("response_{turn}"),"object":"response",
                    "created_at":1,"model":"local-source-test","status":"completed",
                    "output":[{"type":"function_call","id":format!("fc_{turn}"),
                        "call_id":format!("call_{turn}"),"name":name,
                        "arguments":arguments.to_string(),"status":"completed"}],
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
        for agent_first in [true, false] {
            let domain = TestDomain::default();
            let base = Validator::new(domain.clone())
                .with_sources(crate::sources::Sources::new(wrong.path()));
            let validator = if agent_first {
                base.with_agent(config.clone())
                    .with_sources(crate::sources::Sources::new(selected.path()))
            } else {
                base.with_sources(crate::sources::Sources::new(selected.path()))
                    .with_agent(config.clone())
            };
            let answer = validator.verify(Question(true)).await.unwrap();
            assert!(*answer.output());
            assert_eq!(
                answer.dependencies(),
                &[
                    crate::graph::Dependency::Node(1, Content.fingerprint()),
                    crate::graph::Dependency::Node(7, Content.fingerprint()),
                ]
            );
            assert_eq!(
                *domain.1.lock().unwrap(),
                [crate::outline::OutlineId {
                    path: "visible/selected.txt".into(),
                    selector: String::new(),
                }]
            );
        }
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 14);
        for first in [0, 7] {
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
                    "read_source",
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
            assert_eq!(
                listing["files"],
                serde_json::json!(["visible/selected.txt"])
            );
            let ignored = output(&requests[first + 2], "read_source");
            assert!(ignored.contains("excluded"), "{ignored}");
            let mapped: Value =
                serde_json::from_str(&output(&requests[first + 3], "read_source")).unwrap();
            assert_eq!(mapped["node"]["path"], "visible/selected.txt");
            assert_eq!(mapped["text"], "a source snapshot");
            assert!(!mapped.to_string().contains("selected disk contents"));
        }
    }
}

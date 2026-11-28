use super::{CompletionProvider, ToolAgent, ToolFactory, providers::RigProvider};
use crate::domain::Domain;
use crate::domain::NodeId;
use crate::procedure::Agent;
use crate::procedure::AgentRequest;
use crate::request::ValidationContext;
use futures::future::BoxFuture;
#[cfg(test)]
use serde_json::Value;
use std::{io, sync::Arc};
use tokio::sync::OnceCell;

/// Backend state is initialized on the first awaited request.
#[derive(Default)]
pub(crate) struct ConfiguredAgent {
    provider: OnceCell<Arc<RigProvider>>,
}

impl<D> Agent<D> for ConfiguredAgent
where
    D: Domain,
    D::NodeData: serde::Serialize + serde::de::DeserializeOwned + schemars::JsonSchema,
    D::EdgeLabel: serde::de::DeserializeOwned + schemars::JsonSchema + PartialEq,
    D::Error: std::fmt::Display,
{
    fn run<'a>(
        &'a self,
        request: AgentRequest<'a, NodeId<D>, D::Error>,
        inspection: &'a ValidationContext<'_, D>,
    ) -> BoxFuture<'a, Result<(), super::AgentError<D::Error>>> {
        Box::pin(async move {
            let configured = inspection
                .graph()
                .cache()
                .agent
                .as_deref()
                .expect("agent is installed in the domain cache");
            let provider = self
                .provider
                .get_or_try_init(|| async {
                    RigProvider::connect(&configured.backend)
                        .await
                        .map(Arc::new)
                        .map_err(super::AgentError::Provider)
                })
                .await?;
            let tools = &configured.tools;
            let agent = ToolAgent::new(provider.clone());
            if let Some(project) = inspection.graph().project_sources() {
                agent
                    .with_tools(project)
                    .run_with_domain_tools(request, inspection, tools)
                    .await
            } else {
                agent
                    .run_with_domain_tools(request, inspection, tools)
                    .await
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
        NodeId<D>: 'a,
        D::EdgeLabel: 'a,
    {
        (**self).session()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{AgentConfig, OpenAiConfig};
    use crate::domain::NodeContents;
    use crate::graph as validation;
    use crate::graph::Criterion;
    use crate::graph::CriterionPath;
    use crate::graph::CriterionVersion;
    use crate::graph::Fingerprint;
    use crate::graph::NodePath;
    use crate::graph::Record;
    use crate::request::ValidationContext;

    #[derive(
        Clone,
        Debug,
        PartialEq,
        Eq,
        serde::Serialize,
        serde::Deserialize,
        schemars::JsonSchema,
        Hash,
    )]
    struct TestNode {
        id: u32,
    }

    #[derive(Clone, Default)]
    struct TestDomain(
        Arc<std::sync::Mutex<Option<Record<TestNode, String>>>>,
        Arc<std::sync::Mutex<Vec<crate::outline::OutlineData>>>,
        crate::domain::ValidationCache<Self>,
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
            item: &crate::outline::OutlineData,
        ) -> Result<TestNode, crate::sources::SourceError> {
            self.1.lock().unwrap().push(item.clone());
            match item.path.to_str() {
                Some("visible/selected.txt") => Ok(TestNode { id: 7 }),
                Some("alpha.txt") => Ok(TestNode { id: 8 }),
                Some("beta.txt") => Ok(TestNode { id: 9 }),
                Some("unused.txt") => Ok(TestNode { id: 10 }),
                _ => Ok(TestNode { id: 11 }),
            }
        }

        fn cache(&self) -> &crate::domain::ValidationCache<Self> {
            &self.2
        }
        type Error = String;
        type NodeData = TestNode;
        type NodeContent = Content;
        type EdgeLabel = String;

        fn node_path(&self, node: &NodeId<Self>) -> NodePath {
            let node = &*self.cache().node_data(*node).unwrap();
            NodePath::new(vec![node.id.to_string()])
        }

        fn node_content(&self, _: &NodeId<Self>) -> Result<Content, String> {
            Ok(Content)
        }

        fn outgoing_edges(&self, _: &NodeId<Self>, _: String) -> Result<Vec<NodeId<Self>>, String> {
            Ok(Vec::new())
        }

        async fn load_evidence(
            &self,
            _: &NodeId<Self>,
            _: &CriterionPath,
        ) -> Result<Option<Record<TestNode, String>>, String> {
            Ok(self.0.lock().unwrap().clone())
        }

        async fn store_evidence(
            &self,
            _: &NodeId<Self>,
            _: &CriterionPath,
            record: &Record<TestNode, String>,
        ) -> Result<(), String> {
            *self.0.lock().unwrap() = Some(record.clone());
            Ok(())
        }
    }

    struct Question(bool, NodeId<TestDomain>);

    impl Criterion<TestDomain> for Question {
        type Ok = bool;

        fn source_node_id(&self) -> NodeId<TestDomain> {
            self.1
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

    // {test}`requirements.sdk.graph.agent-calls.subject_message`
    #[tokio::test]
    async fn a_late_task_message_precedes_the_first_source_in_the_provider_request() {
        let (backend, server) =
            local_backend(vec![("submit_result", serde_json::json!({"result":true}))]);
        let domain = TestDomain(
            Default::default(),
            Default::default(),
            crate::domain::ValidationCache::with_agent(backend),
        );
        let current = domain.cache().intern_node(TestNode { id: 1 });
        let cx = ValidationContext::new(&domain, current);
        cx.ask::<bool>()
            .message("old task")
            .contents_of_current_node()
            .message("new task")
            .await
            .unwrap();
        let requests = server.join().unwrap();
        let content = &requests[0]["input"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "user")
            .unwrap()["content"];
        let user = content
            .as_str()
            .or_else(|| content[0]["text"].as_str())
            .unwrap();
        assert!(user.starts_with("new task"), "{user}");
        assert!(user.contains("a source snapshot"));
        assert!(!user.contains("old task"));
    }

    // {test}`requirements.sdk.graph.validation-context.shared_context`
    #[tokio::test]
    // {test}`requirements.sdk.graph.validation-context.ValidationContext.ask`
    async fn verify_returns_direct_and_tool_reads_in_one_trace() {
        struct ReadThenAsk(NodeId<TestDomain>);
        impl Criterion<TestDomain> for ReadThenAsk {
            type Ok = bool;
            fn source_node_id(&self) -> NodeId<TestDomain> {
                self.0
            }
            fn criterion_path(&self) -> CriterionPath {
                CriterionPath::new(vec!["mixed-reads".into()])
            }
            fn version(&self) -> CriterionVersion {
                CriterionVersion::new("v1")
            }
            async fn execute(
                self,
                cx: &mut ValidationContext<'_, TestDomain>,
            ) -> Result<bool, validation::ValidationError<String>> {
                cx.read(&cx.graph().cache().intern_node(TestNode { id: 42 }))
                    .map_err(validation::ValidationError::Graph)?;
                cx.ask::<bool>().await
            }
        }
        let (backend, server) = local_backend(vec![
            ("read_node", serde_json::json!({"node": 0})),
            ("submit_result", serde_json::json!({"result": true})),
        ]);
        let domain = TestDomain(
            Default::default(),
            Default::default(),
            crate::domain::ValidationCache::with_agent(backend),
        );
        domain.cache().intern_node(TestNode { id: 7 });
        let answer = validation::Validator::new(&domain)
            .verify(ReadThenAsk(domain.cache().intern_node(TestNode { id: 1 })))
            .await
            .unwrap();
        assert_eq!(
            answer.dependencies(),
            &[
                validation::Dependency::Node(
                    domain.cache().intern_node(TestNode { id: 42 }),
                    Content.fingerprint()
                ),
                validation::Dependency::Node(
                    domain.cache().intern_node(TestNode { id: 7 }),
                    Content.fingerprint()
                ),
            ]
        );
        assert_eq!(server.join().unwrap().len(), 2);
    }

    // {test}`requirements.sdk.graph.running-criteria.validate_node.errors`
    // {test}`requirements.sdk.graph.agent-harness.with_agent`
    // {test}`requirements.sdk.graph.agent-harness.tool_agent.provider_failure`
    // {test}`requirements.sdk.graph.criteria.deterministic_procedures`
    // {test}`requirements.sdk.validation-cache.with_agent`
    #[tokio::test]
    // {test}`requirements.sdk.graph.running-criteria.ValidationError`
    // {test}`requirements.sdk.graph.agent-harness.agent_config`
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
        let mut validator = validation::Validator::new(&domain);
        let deterministic = validator
            .refresh_evidence(Question(
                false,
                domain.cache().intern_node(TestNode { id: 1 }),
            ))
            .await
            .unwrap();
        assert!(!deterministic.output());
        assert_eq!(deterministic.dependencies().len(), 1);
        let error = match validator
            .verify(Question(
                true,
                domain.cache().intern_node(TestNode { id: 1 }),
            ))
            .await
        {
            Ok(_) => panic!("missing credentials must fail the awaited ask"),
            Err(error) => error,
        };
        assert!(matches!(error, crate::graph::ValidationError::Provider(_)));
        assert!(
            error
                .to_string()
                .contains("TBV_TEST_NONEXISTENT_CREDENTIAL_59a680")
        );
    }

    // {test}`requirements.sdk.graph.agent-harness.AgentSettings`
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
        assert_eq!(domain.cache().agent.as_ref().unwrap().backend, config);
        assert!(domain.cache().outlines.lock().unwrap().is_empty());
        assert!(domain.cache().sources.is_none());
        let peer = domain.clone();
        assert!(Arc::ptr_eq(
            domain.cache().agent.as_ref().unwrap(),
            peer.cache().agent.as_ref().unwrap()
        ));
        let verified = validation::Validator::new(&domain)
            .refresh_evidence(Question(
                false,
                domain.cache().intern_node(TestNode { id: 1 }),
            ))
            .await
            .unwrap();
        assert!(!verified.output());
        let saved = serde_json::to_value(domain.0.lock().unwrap().as_ref().unwrap()).unwrap();
        assert!(saved.get("backend").is_none());
        assert!(!saved.to_string().contains("development"));
        assert!(!saved.to_string().contains("claude-sonnet"));
    }

    // {test}`requirements.sdk.validation-cache.new`
    #[test]
    fn new_cache_starts_without_files_or_agent() {
        let cache = crate::domain::ValidationCache::<TestDomain>::new();
        assert!(cache.agent.is_none());
        assert!(cache.sources.is_none());
        assert!(cache.outlines.lock().unwrap().is_empty());
    }

    #[derive(
        Clone,
        Debug,
        PartialEq,
        Eq,
        serde::Serialize,
        serde::Deserialize,
        schemars::JsonSchema,
        Hash,
    )]
    enum LookupNode {
        Requirement(String),
        Selection(String),
    }

    struct LookupContent(String);
    impl NodeContents for LookupContent {
        fn fingerprint(&self) -> Fingerprint {
            Fingerprint::new("test-text".into(), self.0.clone())
        }
        fn format(&self) -> String {
            self.0.clone()
        }
    }

    #[derive(serde::Deserialize, schemars::JsonSchema)]
    struct Pattern {
        pattern: String,
    }

    type LookupRecords =
        std::collections::HashMap<(NodePath, CriterionPath), Record<LookupNode, String>>;

    #[derive(Clone)]
    struct LookupDomain<'a> {
        records: Arc<std::sync::Mutex<LookupRecords>>,
        lookup_calls: Arc<std::sync::Mutex<Vec<String>>>,
        catalog: &'a [&'a str],
        cache: crate::domain::ValidationCache<Self>,
    }

    impl Domain for LookupDomain<'_> {
        type Error = String;
        type NodeData = LookupNode;
        type EdgeLabel = String;
        type NodeContent = LookupContent;
        fn cache(&self) -> &crate::domain::ValidationCache<Self> {
            &self.cache
        }
        fn node_path(&self, node: &NodeId<Self>) -> NodePath {
            let node = &*self.cache().node_data(*node).unwrap();
            NodePath::new(vec![serde_json::to_string(node).unwrap()])
        }
        fn node_content(&self, node: &NodeId<Self>) -> Result<LookupContent, String> {
            let node = &*self.cache().node_data(*node).unwrap();
            let text = match node {
                LookupNode::Selection(pattern) => {
                    serde_json::to_string(&self.matches(pattern)).unwrap()
                }
                LookupNode::Requirement(name) if name == "requirements.sdk.outlines" => {
                    "An outline identifies the selectable items in a source file.".into()
                }
                LookupNode::Requirement(name) => name.clone(),
            };
            Ok(LookupContent(text))
        }
        fn outgoing_edges(&self, _: &NodeId<Self>, _: String) -> Result<Vec<NodeId<Self>>, String> {
            Ok(vec![])
        }
        async fn load_evidence(
            &self,
            node: &NodeId<Self>,
            criterion: &CriterionPath,
        ) -> Result<Option<Record<LookupNode, String>>, String> {
            let key = (self.node_path(node), criterion.clone());
            Ok(self.records.lock().unwrap().get(&key).cloned())
        }
        async fn store_evidence(
            &self,
            node: &NodeId<Self>,
            criterion: &CriterionPath,
            record: &Record<LookupNode, String>,
        ) -> Result<(), String> {
            let key = (self.node_path(node), criterion.clone());
            self.records.lock().unwrap().insert(key, record.clone());
            Ok(())
        }
    }

    impl LookupDomain<'_> {
        fn matches(&self, pattern: &str) -> Vec<LookupNode> {
            self.catalog
                .iter()
                .filter(|name| match pattern.strip_suffix('*') {
                    Some(prefix) => name.starts_with(prefix),
                    None => **name == pattern,
                })
                .map(|name| LookupNode::Requirement(name.to_string()))
                .collect()
        }
    }

    struct Lookup(&'static str);
    impl<'d> crate::agent::Tool<LookupDomain<'d>, Pattern, Vec<NodeId<LookupDomain<'d>>>> for Lookup {
        fn name(&self) -> &str {
            self.0
        }
        fn description(&self) -> &str {
            "Find requirement node IDs matching a name glob."
        }
        async fn execute(
            &self,
            cx: &ValidationContext<'_, LookupDomain<'d>>,
            args: Pattern,
        ) -> Vec<NodeId<LookupDomain<'d>>> {
            cx.graph()
                .lookup_calls
                .lock()
                .unwrap()
                .push(args.pattern.clone());
            cx.graph()
                .matches(&args.pattern)
                .into_iter()
                .map(|data| cx.graph().cache().intern_node(data))
                .collect()
        }
    }
    #[derive(serde::Deserialize, schemars::JsonSchema)]
    struct InspectArgs {
        node: NodeId<LookupDomain<'static>>,
    }
    struct Inspect;
    impl crate::agent::Tool<LookupDomain<'static>, InspectArgs, String> for Inspect {
        fn name(&self) -> &str {
            "inspect"
        }
        fn description(&self) -> &str {
            "Read an entry."
        }
        async fn execute(
            &self,
            cx: &ValidationContext<'_, LookupDomain<'static>>,
            InspectArgs { node }: InspectArgs,
        ) -> String {
            let entry = cx.read(&node).unwrap();
            cx.outgoing_edges(&node, "related".into()).unwrap();
            entry.format()
        }
    }

    // {test}`requirements.sdk.validation-cache.usage`
    // {test}`requirements.sdk.validation-cache.with_agent`
    // {test}`requirements.sdk.graph.agent-harness.AgentWithTools`
    // {test}`requirements.sdk.graph.agent-harness.AgentSettings`
    // {test}`requirements.sdk.graph.agent-harness.Tool`
    // {test}`requirements.sdk.graph.agent-harness.with_tool`
    // {test}`requirements.sdk.graph.agent-harness.domain_tool_dispatch`
    // {test}`requirements.sdk.graph.agent-harness.domain_tool_inputs`
    // {test}`requirements.sdk.graph.agent-harness.lookup_requirements`
    #[tokio::test]
    // {test}`requirements.sdk.graph.running-criteria.UsageStats`
    // {test}`requirements.sdk.graph.running-criteria.ToolUsage.by_name`
    // {test}`requirements.sdk.graph.running-criteria.ToolUsage.result_bytes`
    async fn registered_tools_use_the_current_context_and_ordinary_node_ids() {
        use serde_json::json;
        let (backend, server) = local_backend(vec![
            (
                "lookup_requirements",
                json!({"pattern":"requirements.sdk.*"}),
            ),
            ("lookup_requirements", json!({"pattern":false})),
            ("inspect", json!({"node":2})),
            ("submit_result", json!({"result":true})),
            ("submit_result", json!({"result":false})),
        ]);
        let catalog: &[&str] = &["requirements.sdk.cache", "requirements.sdk.outlines"];
        let outline = LookupNode::Requirement(catalog[1].into());
        let domain = LookupDomain {
            records: Default::default(),
            lookup_calls: Default::default(),
            catalog,
            cache: crate::domain::ValidationCache::with_agent(
                backend
                    .clone()
                    .with_tool(Arc::new(Lookup("lookup_requirements")))
                    .with_tool(Arc::new(Inspect)),
            ),
        };
        let peer = domain.clone();
        let cx = ValidationContext::new(
            &peer,
            peer.cache()
                .intern_node(LookupNode::Requirement("subject".into())),
        );
        assert!(domain.cache().usage().tools.is_empty());
        assert!(cx.ask::<bool>().await.unwrap());
        let stats = domain.cache().usage();
        assert_eq!(
            stats.tools.keys().map(String::as_str).collect::<Vec<_>>(),
            ["inspect", "lookup_requirements", "submit_result"]
        );
        assert_eq!((stats.prepared_inputs, stats.delivered_inputs), (0, 0));
        assert!(matches!(
            &cx.dependencies()[..],
            [validation::Dependency::Node(node, hash), validation::Dependency::OutgoingEdges(edges, label, _)] if node == &domain.cache().intern_node(outline.clone()) && hash == &domain.node_content(&domain.cache().intern_node(outline.clone())).unwrap().fingerprint() && edges == &domain.cache().intern_node(outline.clone()) && label == "related"
        ));
        assert_eq!(*domain.lookup_calls.lock().unwrap(), ["requirements.sdk.*"]);
        let other = LookupDomain {
            records: Default::default(),
            lookup_calls: Default::default(),
            catalog,
            cache: crate::domain::ValidationCache::with_agent(
                backend.with_tool(Arc::new(Lookup("other"))),
            ),
        };
        assert!(
            !ValidationContext::new(
                &other,
                other
                    .cache()
                    .intern_node(LookupNode::Requirement("subject".into()))
            )
            .ask::<bool>()
            .await
            .unwrap()
        );
        let requests = server.join().unwrap();
        for request in &requests[..4] {
            let names: Vec<_> = request["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|tool| tool["name"].as_str().unwrap())
                .collect();
            assert_eq!(
                names,
                [
                    "read_node",
                    "outgoing_edges",
                    "get_next_page",
                    "submit_result",
                    "lookup_requirements",
                    "inspect"
                ]
            );
        }
        let tools = requests[0]["tools"].as_array().unwrap();
        let lookup = tools
            .iter()
            .find(|tool| tool["name"] == "lookup_requirements")
            .unwrap();
        assert_eq!(
            lookup["parameters"]["properties"]["pattern"]["type"],
            "string"
        );
        assert_eq!(
            lookup["description"],
            "Find requirement node IDs matching a name glob."
        );
        let output = |turn: usize| {
            requests[turn]["input"]
                .as_array()
                .unwrap()
                .iter()
                .rev()
                .find(|msg| msg["type"] == "function_call_output")
                .unwrap()["output"]
                .as_str()
                .unwrap()
        };
        assert_eq!(
            serde_json::from_str::<Value>(output(1)).unwrap(),
            json!([1, 2])
        );
        assert_eq!(stats.tools["lookup_requirements"].calls, 2);
        assert_eq!(
            stats.tools["lookup_requirements"].result_bytes,
            (output(1).len() + output(2).len()) as u64
        );
        assert_eq!(stats.tools["inspect"].result_bytes, output(3).len() as u64);
        assert!(output(2).contains("invalid type"));
        assert_eq!(
            serde_json::from_str::<String>(output(3)).unwrap(),
            "An outline identifies the selectable items in a source file."
        );
        let other_tools = requests[4]["tools"].as_array().unwrap();
        assert!(other_tools.iter().any(|tool| tool["name"] == "other"));
        assert!(
            !other_tools
                .iter()
                .any(|tool| tool["name"] == "inspect" || tool["name"] == "lookup_requirements")
        );
    }

    // {test}`requirements.sdk.validation-cache.ValidationCache`
    #[test]
    fn cloning_a_configured_cache_shares_its_agent_sources_and_observations() {
        let cache = crate::domain::ValidationCache::<TestDomain>::with_agent(AgentConfig::Bedrock(
            super::super::BedrockConfig::new("fixture".into()),
        ))
        .with_sources(crate::sources::Sources::new("."));
        let peer = cache.clone();
        assert!(Arc::ptr_eq(
            cache.agent.as_ref().unwrap(),
            peer.agent.as_ref().unwrap()
        ));
        assert!(Arc::ptr_eq(
            cache.sources.as_ref().unwrap(),
            peer.sources.as_ref().unwrap()
        ));
        assert!(Arc::ptr_eq(&cache.source_addresses, &peer.source_addresses));
        assert!(Arc::ptr_eq(&cache.outlines, &peer.outlines));
        assert!(Arc::ptr_eq(&cache.usage, &peer.usage));
    }

    // {test}`requirements.sdk.validation-cache.set_agent`
    #[tokio::test]
    async fn installing_an_agent_after_discovery_preserves_the_cache() {
        let mut domain = TestDomain::default();
        let discovery = domain.clone();
        let node = discovery.cache().intern_node(TestNode { id: 9 });
        let (first, first_server) = local_backend(vec![(
            "submit_result",
            serde_json::json!({"result": false}),
        )]);
        let (second, second_server) =
            local_backend(vec![("submit_result", serde_json::json!({"result": true}))]);
        domain.2.set_agent(first);
        let earlier = domain.clone();
        domain.2.set_agent(second);
        let later = domain.clone();
        assert!(discovery.2.agent.is_none());
        assert!(Arc::ptr_eq(
            later.2.agent.as_ref().unwrap(),
            domain.2.agent.as_ref().unwrap()
        ));
        assert_eq!(later.cache().node_data(node).unwrap().id, 9);
        assert!(Arc::ptr_eq(&later.2.outlines, &discovery.2.outlines));
        for (handle, expected) in [(&earlier, false), (&later, true)] {
            let result = validation::Validator::new(handle)
                .verify(Question(true, node))
                .await
                .unwrap();
            assert_eq!(*result.output(), expected);
        }
        assert_eq!(discovery.cache().usage().attempts, 2);
        assert_eq!(first_server.join().unwrap().len(), 1);
        assert_eq!(second_server.join().unwrap().len(), 1);
    }

    struct ReadSelection;
    impl crate::agent::Tool<LookupDomain<'_>, Pattern, String> for ReadSelection {
        fn name(&self) -> &str {
            "read_selection"
        }
        fn description(&self) -> &str {
            "Read the selected set of requirements."
        }
        async fn execute(
            &self,
            cx: &ValidationContext<'_, LookupDomain<'_>>,
            args: Pattern,
        ) -> String {
            cx.read(
                &cx.graph()
                    .cache()
                    .intern_node(LookupNode::Selection(args.pattern)),
            )
            .unwrap()
            .format()
        }
    }

    // {test}`requirements.sdk.graph.agent-harness.lookup_requirements`
    // {test}`requirements.sdk.graph.agent-harness.domain_tool_inputs`
    #[tokio::test]
    async fn lookup_read_and_empty_selection_have_distinct_evidence() {
        use serde_json::json;
        let pattern = json!({"pattern":"requirements.sdk.*"});
        let outline = LookupNode::Requirement("requirements.sdk.outlines".into());
        let submit = ("submit_result", json!({"result":true}));
        let (backend, server) = local_backend(vec![
            ("lookup_requirements", pattern.clone()),
            ("read_node", json!({"node":0})),
            submit.clone(),
            ("lookup_requirements", pattern.clone()),
            submit.clone(),
            ("read_selection", pattern),
            submit,
        ]);
        let domain = LookupDomain {
            records: Default::default(),
            lookup_calls: Default::default(),
            catalog: &["requirements.sdk.cache", "requirements.sdk.outlines"],
            cache: crate::domain::ValidationCache::with_agent(
                backend
                    .with_tool(Arc::new(Lookup("lookup_requirements")))
                    .with_tool(Arc::new(ReadSelection)),
            ),
        };
        let cx = ValidationContext::new(&domain, domain.cache().intern_node(outline.clone()));
        assert!(cx.ask::<bool>().await.unwrap());
        assert!(
            matches!(&cx.dependencies()[..], [validation::Dependency::Node(node, hash)] if node == &domain.cache().intern_node(outline.clone()) && hash == &domain.node_content(node).unwrap().fingerprint())
        );
        let empty = LookupDomain {
            records: Default::default(),
            lookup_calls: Default::default(),
            catalog: &[],
            cache: domain.cache.clone(),
        };
        let discovery = ValidationContext::new(&empty, empty.cache().intern_node(outline.clone()));
        assert!(discovery.ask::<bool>().await.unwrap());
        assert!(discovery.dependencies().is_empty());
        let tracked = ValidationContext::new(&empty, empty.cache().intern_node(outline.clone()));
        assert!(tracked.ask::<bool>().await.unwrap());
        let version = CriterionVersion::new("v1");
        let record = tracked.finish(version.clone(), true).unwrap();
        assert!(record.is_current(&empty, &version).unwrap());
        let criterion = CriterionPath::new(vec!["lookup".into()]);
        empty
            .store_evidence(tracked.node_id(), &criterion, &record)
            .await
            .unwrap();
        let first = LookupDomain {
            records: empty.records.clone(),
            lookup_calls: Default::default(),
            catalog: &["requirements.sdk.cache"],
            cache: Default::default(),
        };
        let other = first
            .cache()
            .intern_node(LookupNode::Requirement("requirements.sdk.cache".into()));
        assert!(
            first
                .load_evidence(&other, &criterion)
                .await
                .unwrap()
                .is_none()
        );
        let subject = first.cache().intern_node(outline);
        assert_ne!(subject, *tracked.node_id());
        let loaded = first
            .load_evidence(&subject, &criterion)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.dependencies, record.dependencies);
        assert!(!loaded.is_current(&first, &version).unwrap());
        let requests = server.join().unwrap();
        let output = |turn: usize| {
            requests[turn]["input"]
                .as_array()
                .unwrap()
                .iter()
                .rev()
                .find(|m| m["type"] == "function_call_output")
                .unwrap()["output"]
                .as_str()
                .unwrap()
        };
        assert_eq!(
            serde_json::from_str::<Value>(output(2)).unwrap(),
            json!({"text":"An outline identifies the selectable items in a source file."})
        );
        assert_eq!(output(4), "[]");
    }

    // {test}`requirements.sdk.graph.validation-context.node_reference`
    // {test}`requirements.sdk.graph.validation-context.outgoing_edges.mcp`
    // {test}`requirements.sdk.graph.validation-context.outline_source.mcp`
    #[tokio::test]
    async fn edge_and_outline_tools_accept_source_addresses_and_domain_data() {
        use serde_json::json;
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("alpha.txt"), "").unwrap();
        let (backend, server) = local_backend(vec![
            ("outline_source", json!({"path":"alpha.txt", "location":""})),
            ("outline_source", json!({"data":{"id":8}})),
            (
                "outgoing_edges",
                json!({"path":"alpha.txt", "location":"", "label":"related"}),
            ),
            (
                "outgoing_edges",
                json!({"data":{"id":8}, "label":"related"}),
            ),
            ("submit_result", json!({"result":true})),
        ]);
        let domain = TestDomain(
            Default::default(),
            Default::default(),
            crate::domain::ValidationCache::with_agent(backend)
                .with_sources(crate::sources::Sources::new(root.path())),
        );
        let source = domain.cache().intern_node(TestNode { id: 8 });
        let cx = ValidationContext::new(&domain, source);
        assert!(cx.ask::<bool>().await.unwrap());
        let requests = server.join().unwrap();
        for index in 1..=4 {
            let msg = requests[index]["input"]
                .as_array()
                .unwrap()
                .iter()
                .rev()
                .find(|msg| msg["type"] == "function_call_output")
                .unwrap();
            let response: Value = serde_json::from_str(msg["output"].as_str().unwrap()).unwrap();
            if index <= 2 {
                assert_eq!(
                    response,
                    json!({"node":source,"items":[{"node":source,"file":"alpha.txt","selector":"","kind":""}]})
                );
            } else {
                assert_eq!(response, json!({"targets":[]}));
            }
        }
        assert!(
            matches!(cx.dependencies().as_slice(), [validation::Dependency::OutgoingEdges(node, label, _)] if *node == source && label == "related")
        );
        for name in ["read_node", "outgoing_edges", "outline_source"] {
            let tools = requests[0]["tools"].as_array().unwrap();
            let schema = &tools.iter().find(|tool| tool["name"] == name).unwrap()["parameters"];
            let def = schema["properties"]["data"]["$ref"]
                .as_str()
                .unwrap()
                .strip_prefix('#')
                .unwrap();
            assert_eq!(
                schema.pointer(def).unwrap()["properties"]["id"]["type"],
                "integer"
            );
            assert_eq!(schema["properties"]["node"]["type"], "integer");
        }
    }

    // {test}`requirements.sdk.graph.agent-harness.fixed_tools`
    // {test}`requirements.sdk.graph.validation-context.read_node.mcp`
    #[tokio::test]
    async fn read_node_advertises_and_reads_every_domain_id_variant() {
        use serde_json::json;
        let requirement = LookupNode::Requirement("requirements.sdk.outlines".into());
        let selection = LookupNode::Selection("requirements.sdk.*".into());
        let (backend, server) = local_backend(vec![
            ("read_node", json!({"node":0})),
            ("read_node", json!({"node":1})),
            ("submit_result", json!({"result":true})),
        ]);
        let domain = LookupDomain {
            records: Default::default(),
            lookup_calls: Default::default(),
            catalog: &["requirements.sdk.outlines"],
            cache: crate::domain::ValidationCache::with_agent(backend.with_tool(Arc::new(Inspect))),
        };
        let cx = ValidationContext::new(&domain, domain.cache().intern_node(requirement.clone()));
        domain.cache().intern_node(selection.clone());
        assert!(cx.ask::<bool>().await.unwrap());
        assert_eq!(
            cx.dependencies(),
            [requirement.clone(), selection.clone()].map(|node| {
                let hash = domain
                    .node_content(&domain.cache().intern_node(node.clone()))
                    .unwrap()
                    .fingerprint();
                validation::Dependency::Node(domain.cache().intern_node(node), hash)
            })
        );
        let requests = server.join().unwrap();
        let params = &requests[0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "read_node")
            .unwrap()["parameters"];
        assert_eq!(params["properties"]["node"]["type"], "integer");
        for (index, node) in [requirement, selection].into_iter().enumerate() {
            let output = requests[index + 1]["input"]
                .as_array()
                .unwrap()
                .iter()
                .rev()
                .find(|msg| msg["type"] == "function_call_output")
                .unwrap()["output"]
                .as_str()
                .unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(output).unwrap(),
                json!({"text":domain.node_content(&domain.cache().intern_node(node.clone())).unwrap().format()})
            );
        }
    }

    // {test}`requirements.sdk.graph.agent-harness.with_tool`
    #[tokio::test]
    async fn duplicate_tool_names_fail_the_ask_before_any_completion() {
        let (backend, server) = local_backend(vec![]);
        for name in ["read_node", "lookup", "search_text"] {
            let domain = LookupDomain {
                records: Default::default(),
                lookup_calls: Default::default(),
                catalog: &[],
                cache: crate::domain::ValidationCache::with_agent(
                    backend
                        .clone()
                        .with_tool(Arc::new(Lookup("lookup")))
                        .with_tool(Arc::new(Lookup(name))),
                ),
            };
            let error = ValidationContext::new(
                &domain,
                domain
                    .cache()
                    .intern_node(LookupNode::Requirement("subject".into())),
            )
            .ask::<bool>()
            .await
            .unwrap_err();
            assert!(
                matches!(error, validation::ValidationError::Agent(message) if message.contains("duplicate agent tool name") && message.contains(name))
            );
        }
        assert!(server.join().unwrap().is_empty());
    }

    // {test}`requirements.sdk.graph.running-criteria.Validator.usage`
    #[tokio::test]
    async fn refreshing_missing_evidence_accounts_for_accepted_and_corrected_answers() {
        let (backend, server) = local_backend(vec![
            ("submit_result", serde_json::json!({"result":true})),
            (
                "submit_result",
                serde_json::json!({"result":"invalid boolean"}),
            ),
            ("submit_result", serde_json::json!({"result":false})),
        ]);
        let domain = TestDomain(
            Default::default(),
            Default::default(),
            crate::domain::ValidationCache::with_agent(backend),
        );
        let node = domain.cache().intern_node(TestNode { id: 1 });
        let mut validator = validation::Validator::new(&domain);
        validator
            .refresh_evidence(Question(true, node))
            .await
            .unwrap();
        let usage = validator.usage();
        assert_eq!(
            (
                usage.attempts,
                usage.prepared_inputs,
                usage.delivered_inputs,
                usage.input_tokens.reported
            ),
            (1, 1, 1, 10)
        );
        domain.0.lock().unwrap().take();
        validator
            .refresh_evidence(Question(true, node))
            .await
            .unwrap();
        let usage = validator.usage();
        assert_eq!(
            (
                usage.attempts,
                usage.prepared_inputs,
                usage.delivered_inputs,
                usage.input_tokens.reported
            ),
            (3, 2, 2, 30)
        );
        assert_eq!(server.join().unwrap().len(), 3);
    }

    fn local_backend(
        calls: Vec<(&'static str, Value)>,
    ) -> (AgentConfig, std::thread::JoinHandle<Vec<Value>>) {
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
            for (call, (name, args)) in calls.into_iter().enumerate() {
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
                        "name":name, "arguments":args.to_string(),
                        "status":"completed"}],
                    "usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15}
                })
                .to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            requests
        });
        let config = AgentConfig::OpenAi(OpenAiConfig {
            model: Some("local-test".into()),
            endpoint: Some(endpoint),
            api_key: Some("local-transport-key".into()),
            ..OpenAiConfig::default()
        });
        (config, server)
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
        let calls = vec![
            ("read_node", serde_json::json!({"node":0})),
            ("submit_result", serde_json::json!({"result":true})),
        ];
        let calls = [calls.clone(), calls].concat();
        let (config, server) = local_backend(calls);
        let mut domain = TestDomain::default();
        domain.2 = crate::domain::ValidationCache::with_agent(config);
        assert_eq!(
            domain
                .node_path(&domain.cache().intern_node(TestNode { id: 1 }))
                .segments(),
            ["1"]
        );
        for _ in 0..2 {
            let answer = validation::Validator::new(&domain)
                .verify(Question(
                    true,
                    domain.cache().intern_node(TestNode { id: 1 }),
                ))
                .await
                .unwrap();
            assert!(*answer.output());
            assert_eq!(answer.dependencies().len(), 1);
            validation::Validator::new(&domain)
                .store_evidence(&answer)
                .await
                .unwrap();
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
            let prompt = &request["input"]
                .as_array()
                .unwrap()
                .iter()
                .find(|message| message["role"] == "user")
                .unwrap()["content"];
            let text = if let Some(text) = prompt.as_str() {
                text
            } else {
                prompt[0]["text"].as_str().unwrap()
            };
            assert!(text.contains("Current node: 0"));
            assert!(text.contains("Node: 0"));
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
                    ("read_node", serde_json::json!({"node":0}))
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
        let answer = validation::Validator::new(&domain)
            .verify(Question(
                true,
                domain.cache().intern_node(TestNode { id: 1 }),
            ))
            .await
            .unwrap();
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

    // {test}`requirements.sdk.graph.validation-context.read_source.inputs`
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
    // {test}`requirements.sdk.graph.validation-context.shared_context`
    // {test}`requirements.sdk.graph.running-criteria.UsageStats`
    // {test}`requirements.sdk.graph.agent-harness.submission_count`
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
                        ("read_node", listing["files"][selected_index].clone())
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
                            12 => serde_json::json!([tag("beta.txt"), tag("beta.txt")]),
                            13 => serde_json::json!([tag("beta.txt")]),
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
            let mut validator = validation::Validator::new(&domain);
            let answer = validator
                .verify(Question(
                    true,
                    domain.cache().intern_node(TestNode { id: 1 }),
                ))
                .await
                .unwrap();
            assert!(*answer.output());
            // One opening node, two node reads and three search excerpts. Listings,
            // rejected calls and repeated search finalization supply no inputs.
            let stats = validator.usage();
            assert_eq!((stats.prepared_inputs, stats.delivered_inputs), (6, 6));
            assert_eq!(
                answer.dependencies(),
                &[
                    crate::graph::Dependency::Node(
                        domain.cache().intern_node(TestNode { id: 1 }),
                        Content.fingerprint()
                    ),
                    crate::graph::Dependency::Node(
                        domain.cache().intern_node(TestNode { id: 7 }),
                        Content.fingerprint()
                    ),
                    crate::graph::Dependency::Node(
                        domain.cache().intern_node(TestNode { id: 8 }),
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
                        domain.cache().intern_node(TestNode { id: 9 }),
                        crate::outline::OutlineContent::from_source(
                            domain.cache(),
                            "beta.txt",
                            "excerpt beta.txt".into()
                        )
                        .unwrap()
                        .at_offset(0)
                        .fingerprint()
                    ),
                    crate::graph::Dependency::Node(
                        domain.cache().intern_node(TestNode { id: 10 }),
                        Content.fingerprint()
                    ),
                ]
            );
            assert_eq!(
                domain
                    .1
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|id| id.path.starts_with("visible") && id.selector.is_empty())
                    .cloned()
                    .collect::<Vec<_>>(),
                [
                    crate::outline::OutlineData::new("visible/selected.txt", String::new())
                        .unwrap()
                ]
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
            for name in ["read_node", "outline_source", "outgoing_edges"] {
                let tool = tools.iter().find(|tool| tool["name"] == name).unwrap();
                let params = &tool["parameters"];
                assert_eq!(params["properties"]["node"]["type"], "integer");
            }
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
                    .all(|entry| entry["node"].is_u64() && entry["data"]["id"].is_u64())
            );
            let rejected = output(&requests[first + 2], "read_node");
            assert!(rejected.contains("invalid node ID"));
            let mapped: Value =
                serde_json::from_str(&output(&requests[first + 3], "read_node")).unwrap();
            assert_eq!(mapped["text"], "a source snapshot");
            assert!(!mapped.to_string().contains("selected disk contents"));
        }
    }
}

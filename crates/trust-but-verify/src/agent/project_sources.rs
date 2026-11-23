//! Fixed project tools; every delivered source read has a domain observation.
use super::{ToolDelivery, ToolFactory, ToolSession};
use crate::domain::NodeContents;
use crate::domain::SourceNodes;
use crate::graph::Dependency;
use crate::outline::OutlineId;
use crate::sources::SourceScope;
use crate::sources::Sources;
use crate::sources::StructuralQuery;
use crate::sources::TextQuery;
use futures::future::BoxFuture;
use rig_core::completion::ToolDefinition;
use serde_json::{Value, json};
use std::{collections::BTreeMap, io};

pub(crate) struct ProjectSources<D: SourceNodes> {
    pub sources: Sources,
    pub domain: D,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Domain;
    use crate::graph::Criterion;
    use crate::graph::CriterionPath;
    use crate::graph::Fingerprint;
    use crate::graph::NodePath;
    use crate::graph::Record;
    use crate::graph::ValidationError;
    use crate::graph::validator::Validator;
    use crate::request::ValidationContext;
    use std::sync::{Arc, Mutex};

    struct State {
        text: String,
        reads: usize,
        read_nodes: Vec<String>,
        record: Option<Record<String>>,
        fail_read: bool,
    }

    #[derive(Clone)]
    struct Project(Arc<Mutex<State>>);

    struct Content(String);
    impl NodeContents for Content {
        fn fingerprint(&self) -> Fingerprint {
            crate::procedure::context_fingerprint(&self.0)
        }
        fn format(&self) -> String {
            self.0.clone()
        }
    }
    impl Domain for Project {
        type Error = io::Error;
        type NodeId = String;
        type NodeContent = Content;
        type EdgeLabel = ();
        // :::{cite} habeas:coverage
        // Source-tool tests publish OutlineId addresses and retain mapped NodeIds
        // directly in dependencies. This required Domain method is unused in
        // those flows; stable-path and filesystem-storage tests use other domains.
        // :::
        fn node_path(&self, id: &String) -> NodePath {
            NodePath::new(vec![id.clone()])
        }
        fn node_content(&self, node: &String) -> io::Result<Content> {
            let mut state = self.0.lock().unwrap();
            state.reads += 1;
            state.read_nodes.push(node.clone());
            if state.fail_read {
                return Err(io::Error::other("prepared source read failed"));
            }
            Ok(Content(state.text.clone()))
        }
        // :::{cite} habeas:coverage
        // This source-only fixture has no graph edges; graph-selection regressions use other fixtures.
        // :::
        fn outgoing_edges(&self, _: &String, _: ()) -> io::Result<Vec<String>> {
            Ok(Vec::new())
        }
        async fn load_evidence(
            &self,
            _: &String,
            _: &CriterionPath,
        ) -> io::Result<Option<Record<String>>> {
            Ok(self.0.lock().unwrap().record.clone())
        }
        async fn store_evidence(
            &self,
            _: &String,
            _: &CriterionPath,
            record: &Record<String>,
        ) -> io::Result<()> {
            self.0.lock().unwrap().record = Some(record.clone());
            Ok(())
        }
    }
    impl SourceNodes for Project {
        fn source_node(&self, item: &OutlineId) -> io::Result<String> {
            if item.path != std::path::Path::new("source.txt") || !item.selector.is_empty() {
                return Err(io::Error::other("unsupported source item"));
            }
            Ok("source-node".into())
        }
    }
    struct ReadSource;
    impl Criterion<Project> for ReadSource {
        type Ok = String;
        fn source_node_id(&self) -> String {
            "source-node".into()
        }
        fn criterion_path(&self) -> CriterionPath {
            CriterionPath::new(vec!["read".into()])
        }
        fn revision(&self) -> String {
            "v1".into()
        }
        async fn execute(
            self,
            cx: &mut ValidationContext<'_, Project>,
        ) -> Result<String, ValidationError<io::Error>> {
            cx.inspection()
                .read_source(&OutlineId {
                    path: "source.txt".into(),
                    selector: String::new(),
                })
                .map(|contents| contents.format())
                .map_err(ValidationError::Graph)
        }
    }

    fn project(text: &str) -> Project {
        Project(Arc::new(Mutex::new(State {
            text: text.into(),
            reads: 0,
            read_nodes: Vec::new(),
            record: None,
            fail_read: false,
        })))
    }

    // {test}`requirements.sdk.sources.provider_observations`
    // {test}`requirements.sdk.sources.source_nodes`
    // {test}`requirements.sdk.graph.context-tools.read_source.behavior`
    // {test}`requirements.sdk.graph.context-tools.read_source.fn`
    // {test}`requirements.sdk.graph.context-tools.read_source.inputs`
    // {test}`requirements.sdk.graph.context-tools.list_sources.inputs`
    // {test}`requirements.sdk.graph.context-tools.list_sources.behavior`
    // {test}`requirements.sdk.graph.context-tools.outline_source.inputs`
    // {test}`requirements.sdk.graph.context-tools.outline_source.behavior`
    // {test}`requirements.sdk.graph.context-tools.search_text.inputs`
    // {test}`requirements.sdk.graph.context-tools.search_structure.inputs`
    // {test}`requirements.sdk.graph.context-tools.pages.inputs`
    // {test}`requirements.sdk.graph.context-tools.pages.fn`
    // {test}`requirements.sdk.graph.context-tools.pages.behavior`
    // {test}`requirements.sdk.graph.agent-harness.fixed_tools`
    // {test}`requirements.sdk.graph.context-tools.list_sources.fn`
    // {test}`requirements.sdk.graph.context-tools.outline_source.fn`
    #[tokio::test]
    async fn fixed_tools_deliver_domain_contents_with_mandatory_snapshot_dependencies() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("source.txt"), "disk text\n").unwrap();
        std::fs::write(root.path().join(".gitignore"), "ignored.txt\n").unwrap();
        std::fs::write(root.path().join("ignored.txt"), "secret").unwrap();
        #[cfg(feature = "default-languages")]
        std::fs::write(
            root.path().join("code.rs"),
            "fn chosen() { call(); }\nimpl Thing { fn nested() {} }\n",
        )
        .unwrap();
        let domain = project("prepared domain contents");
        let tools = ProjectSources {
            sources: Sources::new(root.path()),
            domain: domain.clone(),
        };
        let mut session = tools.session();
        assert_eq!(
            session
                .definitions()
                .unwrap()
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            [
                "list_sources",
                "outline_source",
                "read_source",
                "search_text",
                "search_structure",
                "get_source_page"
            ]
        );
        let listing = session
            .dispatch("list_sources", &json!({"prefix":"source.txt"}))
            .await
            .unwrap();
        assert!(listing.dependencies.is_empty());
        assert_eq!(
            serde_json::from_str::<Value>(&listing.text).unwrap(),
            json!({"files":["source.txt"]})
        );
        let outlined = session
            .dispatch("outline_source", &json!({"path":"source.txt"}))
            .await
            .unwrap();
        assert!(outlined.dependencies.is_empty());
        let result: Value = serde_json::from_str(&outlined.text).unwrap();
        assert_eq!(result["path"], "source.txt");
        assert_eq!(result["items"][0], json!({"selector":"","kind":""}));
        assert_eq!(result["items"].as_array().unwrap().len(), 2);
        assert_eq!(result["items"][1]["kind"], "line");
        assert!(
            result["items"][1]["selector"]
                .as_str()
                .unwrap()
                .starts_with("line@")
        );
        let search = session
            .dispatch(
                "search_text",
                &json!({"prefix":"source.txt","pattern":"disk"}),
            )
            .await
            .unwrap();
        assert!(search.dependencies.is_empty());
        assert_eq!(
            serde_json::from_str::<Value>(&search.text).unwrap(),
            json!({"complete":true,"matches":[{"path":"source.txt","line":1,"excerpt":"disk text"}]})
        );
        #[cfg(feature = "default-languages")]
        {
            let outlined = session
                .dispatch("outline_source", &json!({"path":"code.rs"}))
                .await
                .unwrap();
            assert!(outlined.dependencies.is_empty());
            assert_eq!(
                serde_json::from_str::<Value>(&outlined.text).unwrap(),
                json!({"path":"code.rs","items":[
                    {"selector":"","kind":""},
                    {"selector":"fn[chosen]","kind":"fn"},
                    {"selector":"impl[Thing]","kind":"impl"},
                    {"selector":"impl[Thing]/fn[nested]","kind":"fn"}
                ]})
            );
            let search = session
                .dispatch(
                    "search_structure",
                    &json!({"prefix":"code.rs","language":"rust","rule":{"rule":{"pattern":"call()"}}}),
                )
                .await
                .unwrap();
            assert!(search.dependencies.is_empty());
            assert_eq!(
                serde_json::from_str::<Value>(&search.text).unwrap(),
                json!({"complete":true,"matches":[{
                    "node":{"path":"code.rs","selector":"fn[chosen]"},
                    "excerpt":"call()"
                }]})
            );
        }
        assert_eq!(domain.0.lock().unwrap().reads, 0);
        let read = session
            .dispatch("read_source", &json!({"path":"source.txt"}))
            .await
            .unwrap();
        assert!(read.text.contains("prepared domain contents"));
        assert!(!read.text.contains("disk text"));
        assert_eq!(
            read.dependencies,
            vec![Dependency::Node(
                "source-node".into(),
                Content("prepared domain contents".into()).fingerprint()
            )]
        );
        assert_eq!(domain.0.lock().unwrap().read_nodes, ["source-node"]);
        for args in [
            json!({"path":"ignored.txt"}),
            json!({"path":"../source.txt"}),
            json!({"path":"source.txt","selector":"line[unknown]"}),
            json!({"path":"source.txt","selector":"fn[unclosed"}),
        ] {
            assert!(session.dispatch("read_source", &args).await.is_err());
        }
        assert_eq!(domain.0.lock().unwrap().reads, 1);
        domain.0.lock().unwrap().text = "é".repeat(20_000);
        let initial = session
            .dispatch("read_source", &json!({"path":"source.txt"}))
            .await
            .unwrap();
        let envelope: Value = serde_json::from_str(&initial.text).unwrap();
        let pages = envelope["pages"].as_array().unwrap();
        let expected = initial.dependencies;
        let mut peer = tools.session();
        let peer_initial = peer
            .dispatch("read_source", &json!({"path":"source.txt"}))
            .await
            .unwrap();
        let peer_envelope: Value = serde_json::from_str(&peer_initial.text).unwrap();
        let peer_page = &peer_envelope["pages"][0];
        assert!(
            session
                .dispatch("get_source_page", &json!({"page":peer_page}))
                .await
                .is_err()
        );
        assert!(
            peer.dispatch("get_source_page", &json!({"page":pages[0]}))
                .await
                .is_err()
        );
        domain.0.lock().unwrap().text = "replacement".into();
        let mut retained = String::new();
        for page in pages {
            let result = session
                .dispatch("get_source_page", &json!({"page":page}))
                .await
                .unwrap();
            assert_eq!(result.dependencies, expected);
            retained.push_str(&result.text);
        }
        assert!(retained.contains(&"é".repeat(20_000)));
        assert!(!retained.contains("replacement"));
        assert_eq!(domain.0.lock().unwrap().reads, 3);
        assert!(
            session
                .dispatch("get_source_page", &json!({"page":999999}))
                .await
                .is_err()
        );
        assert!(
            tools
                .session()
                .dispatch("get_source_page", &json!({"page":0}))
                .await
                .is_err()
        );
    }

    // {test}`requirements.sdk.graph.context-tools.read_source.behavior`
    // {test}`requirements.sdk.graph.context-tools.read_source.inputs`
    #[tokio::test]
    async fn omitted_and_explicit_empty_selectors_deliver_the_same_whole_file() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("source.txt"), "disk text").unwrap();
        let domain = project("whole file from domain");
        let tools = ProjectSources {
            sources: Sources::new(root.path()),
            domain: domain.clone(),
        };
        let mut session = tools.session();
        for args in [
            json!({"path":"source.txt"}),
            json!({"path":"source.txt","selector":""}),
        ] {
            let delivered = session.dispatch("read_source", &args).await.unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&delivered.text).unwrap(),
                json!({
                    "node":{"path":"source.txt","selector":""},
                    "text":"whole file from domain"
                }),
                "{args}"
            );
            assert_eq!(
                delivered.dependencies,
                [Dependency::Node(
                    "source-node".into(),
                    Content("whole file from domain".into()).fingerprint()
                )],
                "{args}"
            );
        }
        assert_eq!(
            domain.0.lock().unwrap().read_nodes,
            ["source-node", "source-node"]
        );
    }

    // {test}`requirements.sdk.sources.provider.attachment`
    // {test}`requirements.sdk.sources.provider_observations`
    // {test}`requirements.sdk.graph.context-tools.read_source.fn`
    // {test}`requirements.sdk.graph.context-tools.read_source.behavior`
    // {test}`requirements.sdk.graph.context-tools.read_source.inputs`
    // {test}`requirements.sdk.graph.evidence.refresh_evidence`
    #[tokio::test]
    async fn source_capable_validator_tracks_deterministic_reads_and_reuses_saved_results() {
        let root = tempfile::tempdir().unwrap();
        let domain = project("first");
        let config = crate::agent::AgentConfig::OpenAi(crate::agent::OpenAiConfig {
            api_key_env: Some("TBV_NEVER_REQUIRED_BY_DETERMINISTIC_SOURCE_READS".into()),
            ..Default::default()
        });
        let validator = Validator::new(domain.clone())
            .with_agent(config.clone())
            .with_sources(Sources::new(root.path()));
        let first = validator.refresh_evidence(ReadSource).await.unwrap();
        assert_eq!(first.output(), "first");
        assert_eq!(
            first.dependencies(),
            &[Dependency::Node(
                "source-node".into(),
                Content("first".into()).fingerprint()
            )]
        );
        let saved = validator.refresh_evidence(ReadSource).await.unwrap();
        assert_eq!(saved.output(), "first");
        domain.0.lock().unwrap().text = "second".into();
        let changed = validator.refresh_evidence(ReadSource).await.unwrap();
        assert_eq!(changed.output(), "second");
        let reverse_order = Validator::new(domain.clone())
            .with_sources(Sources::new(root.path()))
            .with_agent(config);
        assert_eq!(
            reverse_order.verify(ReadSource).await.unwrap().output(),
            "second"
        );
    }

    // {test}`requirements.sdk.graph.context-tools.read_source.fn`
    // {test}`requirements.sdk.graph.context-tools.read_source.behavior`
    // {test}`requirements.sdk.graph.context-tools.read_source.inputs`
    // {test}`requirements.sdk.sources.source_nodes`
    // {test}`requirements.sdk.sources.provider_observations`
    #[test]
    fn direct_source_reads_preserve_mapping_errors_and_record_only_successful_returns() {
        let domain = project("prepared contents");
        let inspection = crate::graph::InspectionContext::new(&domain);
        let supported = OutlineId::new("source.txt", String::new()).unwrap();
        for address in [
            OutlineId::new("unknown.txt", String::new()).unwrap(),
            OutlineId::new("source.txt", "line[unknown]".into()).unwrap(),
        ] {
            let error = inspection.read_source(&address).err().unwrap();
            assert!(error.to_string().contains("unsupported source item"));
            assert!(inspection.dependencies().is_empty());
        }
        assert_eq!(domain.0.lock().unwrap().reads, 0);
        assert!(domain.0.lock().unwrap().read_nodes.is_empty());
        domain.0.lock().unwrap().fail_read = true;
        let error = inspection.read_source(&supported).err().unwrap();
        assert!(error.to_string().contains("prepared source read failed"));
        assert!(inspection.dependencies().is_empty());
        domain.0.lock().unwrap().fail_read = false;
        let returned = inspection.read_source(&supported).unwrap();
        assert_eq!(returned.format(), "prepared contents");
        assert_eq!(
            inspection.dependencies(),
            [Dependency::Node(
                "source-node".into(),
                returned.fingerprint()
            )]
        );
        assert_eq!(domain.0.lock().unwrap().reads, 2);
        assert_eq!(
            domain.0.lock().unwrap().read_nodes,
            ["source-node", "source-node"]
        );
        let record = inspection.finish("v1".into(), true).unwrap();
        assert!(crate::graph::record_is_current(&domain, "v1", &record).unwrap());
        domain.0.lock().unwrap().text = "changed contents".into();
        assert!(!crate::graph::record_is_current(&domain, "v1", &record).unwrap());
    }

    // {test}`requirements.sdk.graph.context-tools.read_source.behavior`
    // {test}`requirements.sdk.graph.context-tools.read_source.inputs`
    // {test}`requirements.sdk.sources.redaction`
    #[tokio::test]
    async fn fixed_source_reads_redact_delivery_and_propagate_preparation_errors() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("source.txt"), "disk text").unwrap();
        let token = format!("gh{}_{}", "p", "16C7e42F292c6912E7710c838347Ae178B4a");
        let raw = format!("visible text\ncredential {token}\nlast line\n");
        let domain = project(&raw);
        let tools = ProjectSources {
            sources: Sources::new(root.path()),
            domain: domain.clone(),
        };
        let mut session = tools.session();
        domain.0.lock().unwrap().fail_read = true;
        let error = session
            .dispatch("read_source", &json!({"path":"source.txt"}))
            .await
            .err()
            .unwrap();
        assert!(error.to_string().contains("prepared source read failed"));
        domain.0.lock().unwrap().fail_read = false;
        let returned = session
            .dispatch("read_source", &json!({"path":"source.txt"}))
            .await
            .unwrap();
        let body: Value = serde_json::from_str(&returned.text).unwrap();
        assert!(!returned.text.contains(&token));
        assert!(
            body["text"]
                .as_str()
                .unwrap()
                .contains("[REDACTED:GitHub Token]")
        );
        assert_eq!(
            body["text"].as_str().unwrap().lines().count(),
            raw.lines().count()
        );
        assert_eq!(
            returned.dependencies,
            [Dependency::Node(
                "source-node".into(),
                Content(raw).fingerprint()
            )]
        );
        assert_eq!(domain.0.lock().unwrap().reads, 2);
    }

    // {test}`requirements.sdk.graph.context-tools.outline_source.behavior`
    // {test}`requirements.sdk.graph.context-tools.outline_source.inputs`
    // {test}`requirements.sdk.sources.local_provider`
    #[tokio::test]
    async fn outline_tool_rejects_excluded_and_unreadable_sources_without_domain_reads() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("source.txt"), "first\nsecond\n").unwrap();
        std::fs::write(root.path().join(".gitignore"), "ignored.txt\n").unwrap();
        std::fs::write(root.path().join("ignored.txt"), "hidden").unwrap();
        std::fs::create_dir(root.path().join(".git")).unwrap();
        std::fs::write(root.path().join(".git/config"), "metadata").unwrap();
        let domain = project("domain text");
        let tools = ProjectSources {
            sources: Sources::new(root.path()),
            domain: domain.clone(),
        };
        let mut session = tools.session();
        let allowed = session
            .dispatch("outline_source", &json!({"path":"source.txt"}))
            .await
            .unwrap();
        assert!(allowed.dependencies.is_empty());
        let outline: Value = serde_json::from_str(&allowed.text).unwrap();
        assert_eq!(outline["path"], "source.txt");
        assert_eq!(outline["items"][0]["selector"], "");
        assert_eq!(outline["items"].as_array().unwrap().len(), 3);
        for args in [
            json!({"path":"ignored.txt"}),
            json!({"path":"../source.txt"}),
            json!({"path":".git/config"}),
            json!({"path":"missing.txt"}),
            json!({"path":4}),
        ] {
            assert!(
                session.dispatch("outline_source", &args).await.is_err(),
                "{args}"
            );
        }
        #[cfg(feature = "default-languages")]
        {
            std::fs::write(root.path().join("invalid.rs"), "fn {").unwrap();
            assert!(
                session
                    .dispatch("outline_source", &json!({"path":"invalid.rs"}))
                    .await
                    .is_err()
            );
        }
        assert_eq!(domain.0.lock().unwrap().reads, 0);
    }
}

impl<D: SourceNodes> ToolFactory<D> for ProjectSources<D>
where
    D::Error: std::fmt::Display,
{
    fn session<'a>(&'a self) -> Box<dyn ToolSession<D> + 'a>
    where
        D::NodeId: 'a,
        D::EdgeLabel: 'a,
    {
        Box::new(Session {
            tools: self,
            pages: BTreeMap::new(),
        })
    }
}

struct Session<'a, D: SourceNodes> {
    tools: &'a ProjectSources<D>,
    pages: BTreeMap<usize, ToolDelivery<D::NodeId, D::EdgeLabel>>,
}

impl<D: SourceNodes> ToolSession<D> for Session<'_, D>
where
    D::Error: std::fmt::Display,
{
    // {impl}`requirements.sdk.graph.context-tools.list_sources.fn`
    // {impl}`requirements.sdk.graph.context-tools.outline_source.fn`
    fn definitions(&self) -> io::Result<Vec<ToolDefinition>> {
        [
            json!({"name":"list_sources","description":"List available project files.",
                "parameters":{"type":"object","properties":{"prefix":{"type":"string"}}}}),
            json!({"name":"outline_source","description":"Inspect selectable items in a project file.",
                "parameters":{"type":"object","required":["path"],"properties":{"path":{"type":"string"}}}}),
            json!({"name":"read_source","description":"Read a selected source node and record its exact observation.",
                "parameters":{"type":"object","required":["path"],"properties":{"path":{"type":"string"},"selector":{"type":"string"}}}}),
            json!({"name":"search_text","description":"Find regular-expression matches in available files. Read selected nodes separately.",
                "parameters":{"type":"object","required":["pattern"],"properties":{"pattern":{"type":"string"},"prefix":{"type":"string"},"context_lines":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":0}}}}),
            json!({"name":"search_structure","description":"Find structural matches in available files. Read selected nodes separately.",
                "parameters":{"type":"object","required":["language","rule"],"properties":{"language":{"type":"string"},"rule":{"type":"object"},"prefix":{"type":"string"},"limit":{"type":"integer","minimum":0}}}}),
            json!({"name":"get_source_page","description":"Read a continuation from this request's retained source result.",
                "parameters":{"type":"object","required":["page"],"properties":{"page":{"type":"integer","minimum":0}}}}),
        ]
        .into_iter()
        .map(|definition| serde_json::from_value(definition).map_err(io::Error::other))
        .collect()
    }

    // {impl}`requirements.sdk.graph.context-tools.list_sources.behavior`
    // {impl}`requirements.sdk.graph.context-tools.list_sources.inputs`
    // {impl}`requirements.sdk.graph.context-tools.outline_source.behavior`
    // {impl}`requirements.sdk.graph.context-tools.outline_source.inputs`
    // {impl}`requirements.sdk.sources.provider_observations`
    // {impl}`requirements.sdk.graph.context-tools.read_source.behavior`
    // {impl}`requirements.sdk.graph.context-tools.read_source.inputs`
    // {impl}`requirements.sdk.graph.context-tools.pages.inputs`
    fn dispatch<'a>(
        &'a mut self,
        name: &'a str,
        args: &'a Value,
    ) -> BoxFuture<'a, io::Result<ToolDelivery<D::NodeId, D::EdgeLabel>>> {
        Box::pin(async move {
            let string = |key: &str| -> io::Result<&str> {
                args[key]
                    .as_str()
                    .ok_or_else(|| io::Error::other(format!("{key} must be a string")))
            };
            let optional_number = |key: &str| -> io::Result<Option<usize>> {
                args.get(key)
                    .map(|value| {
                        value
                            .as_u64()
                            .and_then(|n| usize::try_from(n).ok())
                            .ok_or_else(|| {
                                io::Error::other(format!("{key} must be a nonnegative integer"))
                            })
                    })
                    .transpose()
            };
            let scope = SourceScope {
                prefix: args
                    .get("prefix")
                    .map(|_| string("prefix"))
                    .transpose()?
                    .unwrap_or("")
                    .into(),
            };
            let limit = optional_number("limit")?;
            let mut dependencies = Vec::new();
            let result = match name {
                "list_sources" => {
                    json!({"files":self.tools.sources.list(&scope).await.map_err(io::Error::other)?})
                }
                "outline_source" => {
                    let contents = self
                        .tools
                        .sources
                        .parse(string("path")?)
                        .await
                        .map_err(io::Error::other)?;
                    json!({"path":contents.id().path,"items":contents.outline().items.iter().map(|item|
                        json!({"selector":item.selector,"kind":item.kind})).collect::<Vec<_>>()})
                }
                "read_source" => {
                    let path = string("path")?;
                    self.tools
                        .sources
                        .check_path(std::path::Path::new(path))
                        .map_err(io::Error::other)?;
                    let item = OutlineId::new(
                        path,
                        args.get("selector")
                            .map(|_| string("selector"))
                            .transpose()?
                            .unwrap_or("")
                            .into(),
                    )
                    .map_err(io::Error::other)?;
                    let node = self
                        .tools
                        .domain
                        .source_node(&item)
                        .map_err(|error| io::Error::other(error.to_string()))?;
                    let contents = self
                        .tools
                        .domain
                        .node_content(&node)
                        .map_err(|error| io::Error::other(error.to_string()))?;
                    dependencies.push(Dependency::Node(node, contents.fingerprint()));
                    json!({"node":item,"text":contents.format()})
                }
                "search_text" => {
                    let matches = self
                        .tools
                        .sources
                        .search_text(&TextQuery {
                            scope,
                            pattern: string("pattern")?.into(),
                            context_lines: optional_number("context_lines")?.unwrap_or(0),
                            limit,
                        })
                        .await
                        .map_err(io::Error::other)?;
                    json!({"complete":matches.complete,"matches":matches.matches.into_iter().map(|found|
                        json!({"path":found.path,"line":found.line,"excerpt":found.excerpt})).collect::<Vec<_>>()})
                }
                "search_structure" => {
                    let matches = self
                        .tools
                        .sources
                        .search_structure(&StructuralQuery {
                            scope,
                            language: string("language")?.into(),
                            rule: args["rule"]
                                .as_object()
                                .ok_or_else(|| io::Error::other("rule must be an object"))?
                                .clone(),
                            limit,
                        })
                        .await
                        .map_err(io::Error::other)?;
                    json!({"complete":matches.complete,"matches":matches.matches.into_iter().map(|found|
                        json!({"node":found.node,"excerpt":found.excerpt})).collect::<Vec<_>>()})
                }
                "get_source_page" => {
                    let page = optional_number("page")?
                        .ok_or_else(|| io::Error::other("page is required"))?;
                    let retained = self
                        .pages
                        .get(&page)
                        .ok_or_else(|| io::Error::other("unknown source page"))?;
                    return Ok(ToolDelivery {
                        text: retained.text.clone(),
                        dependencies: retained.dependencies.clone(),
                    });
                }
                _ => return Err(io::Error::other(format!("unknown source tool {name:?}"))),
            };
            let serialized = serde_json::to_string(&result).map_err(io::Error::other)?;
            let text = crate::redaction::redact_text(&serialized).into_owned();
            let chars: Vec<_> = text.chars().collect();
            if chars.len() <= 8192 {
                return Ok(ToolDelivery { text, dependencies });
            }
            let mut pages = Vec::new();
            for chunk in chars.chunks(8192) {
                let page = super::next_page_id();
                pages.push(page);
                self.pages.insert(
                    page,
                    ToolDelivery {
                        text: chunk.iter().collect(),
                        dependencies: dependencies.clone(),
                    },
                );
            }
            Ok(ToolDelivery {
                text: json!({"text":self.pages[&pages[0]].text,"pages":pages} ).to_string(),
                dependencies,
            })
        })
    }
}

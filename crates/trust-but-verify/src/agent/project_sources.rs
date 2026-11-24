//! Fixed project tools; every delivered source read has a domain observation.
use super::{Pages, ToolDelivery, ToolFactory, ToolSession};
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
use std::io;

pub(crate) struct ProjectSources<D: SourceNodes> {
    pub sources: Sources,
    pub domain: D,
}

impl<D: SourceNodes> ProjectSources<D>
where
    D::Error: std::fmt::Display,
{
    // Text context can span several outline items. Capture the file root from
    // the bytes searched so every line in the delivered excerpt is covered.
    fn text_input(
        &self,
        path: &std::path::Path,
        file: &crate::sources::SourceFile,
    ) -> Result<Dependency<D::NodeId, D::EdgeLabel>, String> {
        let contents = crate::outline::OutlineContent::from_source(
            path,
            file.text().map_err(|e| e.to_string())?.into(),
        )
        .map_err(|e| e.to_string())?;
        let node = self
            .domain
            .source_node(contents.id())
            .map_err(|e| e.to_string())?;
        Ok(Dependency::Node(node, contents.fingerprint()))
    }
}

#[cfg(test)]
mod tests {
    use super::super::tools::SessionTools;
    use super::*;
    use crate::domain::Domain;
    use crate::graph::Criterion;
    use crate::graph::CriterionPath;
    use crate::graph::CriterionVersion;
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
            if item.path == std::path::Path::new("code.rs") {
                return Ok(format!("code.rs#{}", item.selector));
            }
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
        fn version(&self) -> CriterionVersion {
            CriterionVersion::new("v1")
        }
        async fn execute(
            self,
            cx: &mut ValidationContext<'_, Project>,
        ) -> Result<String, ValidationError<io::Error>> {
            cx.read_source(&OutlineId {
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

    // {test}`requirements.sdk.graph.search.first_search.instructions`
    // {test}`requirements.sdk.graph.search.first_search.instructions.content`
    // {test}`requirements.sdk.graph.search.finish_search.behavior`
    // {test}`requirements.sdk.graph.search.finish_search.inputs`
    #[tokio::test]
    async fn search_obligations_follow_delivery_and_keep_the_original_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("source.txt");
        let original = "hit me here\n".repeat(300);
        std::fs::write(&path, &original).unwrap();
        let expected = crate::outline::OutlineContent::from_source("source.txt", original.into())
            .unwrap()
            .fingerprint();
        let tools = ProjectSources {
            sources: Sources::new(root.path()),
            domain: project("unrelated domain text"),
        };
        let mut session = SessionTools::new(tools.session());
        let mut peer = SessionTools::new(tools.session());
        assert!(
            session
                .dispatch("search_text", &json!({"pattern":"("}))
                .await
                .is_err()
        );
        let call = |text: &str| json!({"pattern":text});
        let empty = session
            .dispatch("search_text", &call("absent"))
            .await
            .unwrap();
        let empty: Value = serde_json::from_str(&empty.text).unwrap();
        let instructions = empty["instructions"].as_str().unwrap();
        assert!(instructions.contains("finish_search") && instructions.contains("used:[]"));
        let finish = |page: &Value, used: Value| json!({"search_id":page["search_id"],"used":used});
        let closed = session
            .dispatch("finish_search", &finish(&empty, json!([])))
            .await
            .unwrap();
        assert!(closed.dependencies.is_empty());
        assert_eq!(
            serde_json::from_str::<Value>(&closed.text).unwrap(),
            json!({"search_id":empty["search_id"],"finished":true})
        );
        let result = session.dispatch("search_text", &call("hit")).await.unwrap();
        let first: Value = serde_json::from_str(&result.text).unwrap();
        assert!(first.get("instructions").is_none());
        assert!(result.dependencies.is_empty());
        let next = json!({"uuid":first["next"]});
        assert!(
            session
                .dispatch(
                    "finish_search",
                    &json!({"search_id":first["next"],"used":[]})
                )
                .await
                .is_err()
        );
        assert!(
            peer.dispatch("finish_search", &finish(&first, json!([])))
                .await
                .is_err()
        );
        assert!(peer.dispatch("get_next_page", &next).await.is_err());
        std::fs::write(&path, "changed after search").unwrap();
        let delivery = session.dispatch("get_next_page", &next).await.unwrap();
        let second: Value = serde_json::from_str(&delivery.text).unwrap();
        assert!(second.get("instructions").is_none());
        assert_ne!(first["search_id"], second["search_id"]);
        assert!(
            session
                .dispatch("finish_search", &finish(&first, json!(["1", "unknown"])))
                .await
                .is_err()
        );
        assert_eq!(session.outstanding_searches().len(), 2);
        let selected = finish(&first, json!([first["matches"][0]["tag"]]));
        for _ in 0..2 {
            let closed = session.dispatch("finish_search", &selected).await.unwrap();
            assert_eq!(
                closed.dependencies,
                [Dependency::Node("source-node".into(), expected.clone())]
            );
        }
        session
            .dispatch("finish_search", &finish(&second, json!([])))
            .await
            .unwrap();
        assert_eq!(
            session.dispatch("get_next_page", &next).await.unwrap().text,
            delivery.text
        );
        assert!(session.outstanding_searches().is_empty());
        assert_eq!(tools.domain.0.lock().unwrap().reads, 0);
    }

    // {test}`requirements.sdk.graph.validation-context.list_sources.mcp`
    // {test}`requirements.sdk.graph.validation-context.pages.behavior`
    // {test}`requirements.sdk.graph.validation-context.list_sources.inputs`
    #[tokio::test]
    async fn listing_continuations_return_complete_paths_from_the_original_listing() {
        let root = tempfile::tempdir().unwrap();
        let expected: Vec<_> = (0..200)
            .map(|i| format!("{i:03}-{}.rs", "a".repeat(100)))
            .collect();
        for name in &expected {
            std::fs::write(root.path().join(name), "").unwrap();
        }
        let tools = ProjectSources {
            sources: Sources::new(root.path()),
            domain: project(""),
        };
        let mut session = SessionTools::new(tools.session());
        let first = session.dispatch("list_sources", &json!({})).await.unwrap();
        assert!(first.dependencies.is_empty());
        let mut page: Value = serde_json::from_str(&first.text).unwrap();
        assert!(page["next"].is_string());
        std::fs::write(root.path().join("added.rs"), "").unwrap();
        let mut files = Vec::new();
        loop {
            files.extend(serde_json::from_value::<Vec<String>>(page["files"].take()).unwrap());
            let Some(uuid) = page.get("next") else { break };
            let delivery = session
                .dispatch("get_next_page", &json!({"uuid":uuid}))
                .await
                .unwrap();
            assert!(delivery.dependencies.is_empty());
            page = serde_json::from_str(&delivery.text).unwrap();
        }
        assert_eq!(files, expected);
    }

    // {test}`requirements.sdk.sources.provider_observations`
    // {test}`requirements.sdk.sources.source_nodes`
    // {test}`requirements.sdk.graph.validation-context.read_source.behavior`
    // {test}`requirements.sdk.graph.validation-context.read_source.mcp`
    // {test}`requirements.sdk.graph.validation-context.read_source.inputs`
    // {test}`requirements.sdk.graph.validation-context.list_sources.inputs`
    // {test}`requirements.sdk.graph.validation-context.list_sources.behavior`
    // {test}`requirements.sdk.graph.validation-context.outline_source.inputs`
    // {test}`requirements.sdk.graph.validation-context.outline_source.behavior`
    // {test}`requirements.sdk.graph.search.search_text.inputs`
    // {test}`requirements.sdk.graph.search.search_text.fn`
    // {test}`requirements.sdk.graph.search.search_structure.inputs`
    // {test}`requirements.sdk.graph.validation-context.pages.inputs`
    // {test}`requirements.sdk.graph.validation-context.pages.mcp`
    // {test}`requirements.sdk.graph.validation-context.pages.behavior`
    // {test}`requirements.sdk.graph.agent-harness.fixed_tools`
    // {test}`requirements.sdk.graph.validation-context.list_sources.mcp`
    // {test}`requirements.sdk.graph.validation-context.outline_source.mcp`
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
        let mut session = SessionTools::new(tools.session());
        assert_eq!(
            session
                .extra
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
                "finish_search"
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
            serde_json::from_str::<Value>(&search.text).unwrap()["matches"],
            json!([{"path":"source.txt","line":1,"excerpt":"disk text","tag":"1"}])
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
                serde_json::from_str::<Value>(&search.text).unwrap()["matches"],
                json!([{
                    "node":{"path":"code.rs","selector":"fn[chosen]"},
                    "excerpt":"call()","tag":"1"
                }])
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
        let uuid = &envelope["next"];
        assert!(uuid::Uuid::parse_str(uuid.as_str().unwrap()).is_ok());
        let expected = initial.dependencies;
        let mut peer = SessionTools::new(tools.session());
        let peer_initial = peer
            .dispatch("read_source", &json!({"path":"source.txt"}))
            .await
            .unwrap();
        let peer_envelope: Value = serde_json::from_str(&peer_initial.text).unwrap();
        let peer_page = &peer_envelope["next"];
        assert!(
            session
                .dispatch("get_next_page", &json!({"uuid":peer_page}))
                .await
                .is_err()
        );
        assert!(
            peer.dispatch("get_next_page", &json!({"uuid":uuid}))
                .await
                .is_err()
        );
        domain.0.lock().unwrap().text = "replacement".into();
        let mut page = envelope;
        let mut retained = page["text"].as_str().unwrap().to_owned();
        while let Some(next) = page.get("next") {
            let args = json!({"uuid":next});
            let result = session.dispatch("get_next_page", &args).await.unwrap();
            assert_eq!(
                session.dispatch("get_next_page", &args).await.unwrap().text,
                result.text
            );
            assert_eq!(result.dependencies, expected);
            let following: Value = serde_json::from_str(&result.text).unwrap();
            assert_eq!(following["node"], page["node"]);
            retained.push_str(following["text"].as_str().unwrap());
            page = following;
        }
        assert_eq!(retained, "é".repeat(20_000));
        assert_eq!(domain.0.lock().unwrap().reads, 3);
        assert!(
            session
                .dispatch("get_next_page", &json!({"uuid":"invalid"}))
                .await
                .is_err()
        );
        assert!(
            SessionTools::new(tools.session())
                .dispatch(
                    "get_next_page",
                    &json!({"uuid":uuid::Uuid::nil().to_string()})
                )
                .await
                .is_err()
        );
    }

    // {test}`requirements.sdk.graph.validation-context.read_source.behavior`
    // {test}`requirements.sdk.graph.validation-context.read_source.inputs`
    #[tokio::test]
    async fn omitted_and_explicit_empty_selectors_deliver_the_same_whole_file() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("source.txt"), "disk text").unwrap();
        let domain = project("whole file from domain");
        let tools = ProjectSources {
            sources: Sources::new(root.path()),
            domain: domain.clone(),
        };
        let mut session = SessionTools::new(tools.session());
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
    // {test}`requirements.sdk.graph.validation-context.read_source.fn`
    // {test}`requirements.sdk.graph.validation-context.read_source.behavior`
    // {test}`requirements.sdk.graph.validation-context.read_source.inputs`
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

    // {test}`requirements.sdk.graph.validation-context.read_source.fn`
    // {test}`requirements.sdk.graph.validation-context.read_source.behavior`
    // {test}`requirements.sdk.graph.validation-context.read_source.inputs`
    // {test}`requirements.sdk.sources.source_nodes`
    // {test}`requirements.sdk.sources.provider_observations`
    #[test]
    fn direct_source_reads_preserve_mapping_errors_and_record_only_successful_returns() {
        let domain = project("prepared contents");
        let inspection = ValidationContext::new(&domain, ReadSource.source_node_id());
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
        let record = inspection
            .finish(CriterionVersion::new("v1"), true)
            .unwrap();
        assert!(
            record
                .is_current(&domain, &CriterionVersion::new("v1"))
                .unwrap()
        );
        domain.0.lock().unwrap().text = "changed contents".into();
        assert!(
            !record
                .is_current(&domain, &CriterionVersion::new("v1"))
                .unwrap()
        );
    }

    // {test}`requirements.sdk.graph.validation-context.read_source.behavior`
    // {test}`requirements.sdk.graph.validation-context.read_source.inputs`
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
        let mut session = SessionTools::new(tools.session());
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

    // {test}`requirements.sdk.sources.redaction`
    #[tokio::test]
    async fn search_excerpts_redact_credentials_before_serialization_and_paging() {
        let root = tempfile::tempdir().unwrap();
        let token = format!("gh{}_{}", "p", "16C7e42F292c6912E7710c838347Ae178B4a");
        let key_body = [
            "MIIEowIBAAKCAQEAu1SU1LfVLPHCozMxH2Mo4lgOEePzNm0tRgeLezV6ffAt0gun",
            "VTLw7onLRnrq0/IzW7yWR7QkrmBL7jTKEn5u+qKhbwKfBstIs+bMY2Zkp18gnTxK",
            "LxoS2tFczGkPLPgizskuemMghRniWaoLcyehkd3qqGElvW/VDL5AaWTg0nLVkjRo",
        ];
        let key = format!(
            "-----BEGIN {}-----\n{}\n-----END {}-----",
            "RSA PRIVATE KEY",
            key_body.join("\n"),
            "RSA PRIVATE KEY"
        );
        let padding = "é".repeat(9_000);
        let raw = format!(
            "fn chosen() {{\n    let token = \"{token}\";\n    let key = r#\"{key}\"#;\n    let padding = \"{padding}\";\n}}"
        );
        let expected = format!(
            "fn chosen() {{\n    let token = \"[REDACTED:GitHub Token]\";\n    let key = r#\"[REDACTED:Private Key Block]\n\n\n\n-----END RSA PRIVATE KEY-----\"#;\n    let padding = \"{padding}\";\n}}"
        );
        std::fs::write(root.path().join("code.rs"), &raw).unwrap();
        let domain = project("domain contents are not read for search");
        let tools = ProjectSources {
            sources: Sources::new(root.path()),
            domain: domain.clone(),
        };
        let mut session = SessionTools::new(tools.session());
        let queries = [(
            "search_text",
            json!({"prefix":"code.rs","pattern":"fn chosen","context_lines":20}),
        )];
        #[cfg(feature = "default-languages")]
        let queries = [
            queries[0].clone(),
            (
                "search_structure",
                json!({"prefix":"code.rs","language":"rust","rule":{"rule":{"kind":"function_item"}}}),
            ),
        ];
        for (tool, args) in queries {
            let first = session.dispatch(tool, &args).await.unwrap();
            assert!(first.dependencies.is_empty());
            let result: Value = serde_json::from_str(&first.text).unwrap();
            // One complete match stays together even if its excerpt exceeds a page.
            assert!(result.get("next").is_none());
            assert_eq!(result["complete"], true);
            assert_eq!(result["matches"].as_array().unwrap().len(), 1);
            let excerpt = result["matches"][0]["excerpt"].as_str().unwrap();
            assert!(
                excerpt == expected,
                "{tool} must redact complete credentials and preserve surrounding text"
            );
            assert_eq!(excerpt.lines().count(), raw.lines().count());
            assert!(!first.text.contains(&token));
            assert_eq!(result.get("instructions").is_some(), tool == "search_text");
            let selected = session
                .dispatch(
                    "finish_search",
                    &json!({"search_id":result["search_id"], "used":[result["matches"][0]["tag"]]}),
                )
                .await
                .unwrap();
            let selector = if tool == "search_text" {
                ""
            } else {
                "fn[chosen]"
            };
            let snapshot =
                crate::outline::OutlineContent::from_source("code.rs", raw.as_str().into())
                    .unwrap()
                    .select(selector)
                    .unwrap();
            assert_eq!(
                selected.dependencies,
                [Dependency::Node(
                    format!("code.rs#{selector}"),
                    snapshot.fingerprint()
                )]
            );
            for line in key_body {
                assert!(!first.text.contains(line));
            }
        }
        assert_eq!(
            std::fs::read_to_string(root.path().join("code.rs")).unwrap(),
            raw
        );
        assert_eq!(domain.0.lock().unwrap().reads, 0);
    }

    // {test}`requirements.sdk.graph.validation-context.outline_source.behavior`
    // {test}`requirements.sdk.graph.validation-context.outline_source.inputs`
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
        let mut session = SessionTools::new(tools.session());
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
        Box::new(Session { tools: self })
    }
}

struct Session<'a, D: SourceNodes> {
    tools: &'a ProjectSources<D>,
}

impl<D: SourceNodes> ToolSession<D> for Session<'_, D>
where
    D::Error: std::fmt::Display,
{
    // {impl}`requirements.sdk.graph.validation-context.list_sources.mcp`
    // {impl}`requirements.sdk.graph.validation-context.list_sources.mcp.description`
    // {impl}`requirements.sdk.graph.validation-context.outline_source.mcp`
    // {impl}`requirements.sdk.graph.validation-context.outline_source.mcp.description`
    // {impl}`requirements.sdk.graph.validation-context.read_source.mcp`
    // {impl}`requirements.sdk.graph.validation-context.read_source.mcp.description`
    // {impl}`requirements.sdk.graph.search.search_text.mcp`
    // {impl}`requirements.sdk.graph.search.search_text.mcp.description`
    // {impl}`requirements.sdk.graph.search.search_structure.mcp`
    // {impl}`requirements.sdk.graph.search.search_structure.mcp.description`
    // {impl}`requirements.sdk.graph.search.finish_search.mcp`
    // {impl}`requirements.sdk.graph.search.finish_search.mcp.description`
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
        json!({
            "name":"finish_search",
            "description":"Close a delivered search and record dependencies for snippets you used to justify your conclusion without an additional source read. search_id is the result's UUID and used is an array of its short entry tags, e.g. {\"search_id\":\"11111111-1111-4111-8111-111111111111\",\"used\":[\"1\"]}. Entries whose source you also read may be omitted; including them is harmless. Use used:[] when no entries qualify, including an empty search. Every delivered search must be finished before submission. The result is {search_id, finished:true}; no additional source is delivered.",
            "parameters":{"type":"object","properties":{
                "search_id":{"type":"string","format":"uuid"},
                "used":{"type":"array","items":{"type":"string"}}
            },"required":["search_id","used"],"additionalProperties":false}
        }),
        ]
        .into_iter()
        .map(|definition| serde_json::from_value(definition).map_err(io::Error::other))
        .collect()
    }

    // {impl}`requirements.sdk.graph.search.search_text.behavior`
    // {impl}`requirements.sdk.graph.search.search_structure.behavior`
    // {impl}`requirements.sdk.graph.search.search_text.inputs`
    // {impl}`requirements.sdk.graph.search.search_structure.inputs`
    // {impl}`requirements.sdk.graph.validation-context.list_sources.behavior`
    // {impl}`requirements.sdk.graph.validation-context.list_sources.inputs`
    // {impl}`requirements.sdk.graph.validation-context.outline_source.behavior`
    // {impl}`requirements.sdk.graph.validation-context.outline_source.inputs`
    // {impl}`requirements.sdk.sources.provider_observations`
    // {impl}`requirements.sdk.graph.validation-context.read_source.behavior`
    // {impl}`requirements.sdk.graph.validation-context.read_source.inputs`
    // {impl}`requirements.sdk.graph.validation-context.pages.inputs`
    // {impl}`requirements.sdk.sources.redaction`
    fn dispatch<'a>(
        &'a mut self,
        name: &'a str,
        args: &'a Value,
        pages: &'a mut Pages<D::NodeId, D::EdgeLabel>,
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
            let scope = SourceScope::under(
                args.get("prefix")
                    .map(|_| string("prefix"))
                    .transpose()?
                    .unwrap_or(""),
            );
            let limit = optional_number("limit")?;
            let mut dependencies = Vec::new();
            let (metadata, field, contents) = match name {
                "list_sources" => (
                    json!({}),
                    "files",
                    serde_json::to_value(
                        self.tools
                            .sources
                            .list(&scope)
                            .await
                            .map_err(io::Error::other)?,
                    )
                    .map_err(io::Error::other)?,
                ),
                "outline_source" => {
                    let contents = self
                        .tools
                        .sources
                        .parse(string("path")?)
                        .await
                        .map_err(io::Error::other)?;
                    (
                        json!({"path":contents.id().path}),
                        "items",
                        json!(
                            contents
                                .outline()
                                .items
                                .iter()
                                .map(|item| json!({"selector":item.selector,"kind":item.kind}))
                                .collect::<Vec<_>>()
                        ),
                    )
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
                    let text = crate::redaction::redact_text(&contents.format()).into_owned();
                    (json!({"node":item}), "text", Value::String(text))
                }
                "finish_search" => return pages.finish_search(args),
                "search_text" => {
                    let query = TextQuery {
                        scope,
                        pattern: string("pattern")?.to_owned(),
                        context_lines: optional_number("context_lines")?.unwrap_or(0),
                        limit,
                    };
                    let matches = crate::source_engine::defaults::search_text_with(
                        &self.tools.sources.analysis.provider,
                        &query,
                        |path, file| self.tools.text_input(path, file),
                        |found, input| {
                            let excerpt = crate::redaction::redact_text(&found.excerpt);
                            let entry =
                                json!({"path":found.path,"line":found.line,"excerpt":excerpt});
                            (entry, input.clone())
                        },
                    )
                    .await
                    .map_err(io::Error::other)?;
                    return Ok(pages.search(matches));
                }
                "search_structure" => {
                    let query = StructuralQuery {
                        scope,
                        language: string("language")?.into(),
                        rule: args["rule"]
                            .as_object()
                            .ok_or_else(|| io::Error::other("rule must be an object"))?
                            .clone(),
                        limit,
                    };
                    let analysis = &self.tools.sources.analysis;
                    let matches = crate::source_engine::defaults::search_structure_with(
                        &analysis.provider,
                        &query,
                        &analysis.languages,
                        |found, parsed, node| {
                            let scheme = format!("{}-structural-v1", parsed.outline.profile.name);
                            let fingerprint = crate::graph::Fingerprint::new(
                                scheme,
                                node.structural_hash.clone(),
                            );
                            let input = self
                                .tools
                                .domain
                                .source_node(&found.node)
                                .map(|id| Dependency::Node(id, fingerprint))
                                .map_err(|error| error.to_string());
                            let excerpt = crate::redaction::redact_text(&found.excerpt);
                            let entry = json!({"node":found.node,"excerpt":excerpt});
                            (entry, input)
                        },
                    )
                    .await
                    .map_err(io::Error::other)?;
                    return Ok(pages.search(matches));
                }
                _ => return Err(io::Error::other(format!("unknown source tool {name:?}"))),
            };
            Ok(pages.prepare(metadata, field, contents, dependencies))
        })
    }
}

//! Fixed project tools; every delivered source read has a domain observation.
use super::{Pages, ToolDelivery, ToolFactory, ToolSession};
use crate::domain::NodeContents;
use crate::domain::NodeId;

use crate::graph::Dependency;
use crate::outline::OutlineData;
use crate::sources::SourceScope;

use crate::sources::StructuralQuery;
use crate::sources::TextQuery;
use futures::future::BoxFuture;
use rig_core::completion::ToolDefinition;
use serde_json::{Value, json};
use std::io;

use crate::sources::SourceProject as ProjectSources;

const LIST_SOURCES: &str = "list_sources";
const OUTLINE_SOURCE: &str = "outline_source";
const SEARCH_TEXT: &str = "search_text";
const SEARCH_STRUCTURE: &str = "search_structure";
const FINISH_SEARCH: &str = "finish_search";

pub(super) const TOOL_NAMES: &[&str] = &[
    LIST_SOURCES,
    OUTLINE_SOURCE,
    SEARCH_TEXT,
    SEARCH_STRUCTURE,
    FINISH_SEARCH,
];

// {impl}`requirements.sdk.graph.search.windows`
// {impl}`requirements.sdk.graph.search.omissions`
fn excerpt(
    item: &crate::outline::OutlineContent,
    hits: &[crate::outline::Span],
    window: usize,
) -> (String, Vec<crate::outline::Span>) {
    let info = item.item();
    let bounds = info.span;
    let source = item.text();
    let mut spans: Vec<_> = [info.header_span, info.trailer_span]
        .into_iter()
        .flatten()
        .collect();
    for hit in hits {
        let start = hit
            .start
            .saturating_sub(window)
            .clamp(bounds.start, bounds.end)
            - bounds.start;
        let end = hit
            .end
            .saturating_add(window)
            .clamp(bounds.start, bounds.end)
            - bounds.start;
        spans.push(crate::outline::Span::new(
            bounds.start + source.ceil_char_boundary(start),
            bounds.start + source.floor_char_boundary(end),
        ));
    }
    spans.sort_unstable_by_key(|s| s.start);
    let mut cursor = bounds.start;
    let mut result = String::new();
    for span in &spans {
        if span.start > cursor {
            result.push_str("...");
        }
        let start = cursor.max(span.start);
        if start < span.end {
            result.push_str(
                &item
                    .source_span_redacted(crate::outline::Span::new(start, span.end))
                    .expect("validated outline and excerpt spans"),
            );
        }
        cursor = cursor.max(span.end);
    }
    if cursor < bounds.end {
        result.push_str("...");
    }
    (result, spans)
}

// {impl}`requirements.sdk.graph.search.finish_search.inputs`
fn excerpt_dependencies<D: crate::domain::Domain>(
    project: &ProjectSources<D>,
    hit: &crate::sources::SearchHit<NodeId<D>>,
    spans: &[crate::outline::Span],
) -> io::Result<Vec<Dependency<NodeId<D>, D::EdgeLabel>>> {
    let mut inputs = vec![Dependency::Node(hit.node, hit.item.fingerprint())];
    for item in hit.item.excerpt_items(spans) {
        let node = project.intern_source(item.id())?;
        inputs.push(Dependency::Node(node, item.fingerprint()));
    }
    Ok(inputs)
}

#[cfg(test)]
mod tests {
    use super::super::tools::SessionTools;
    use super::*;
    use crate::domain::Domain;
    use crate::graph as validation;
    use crate::graph::Criterion;
    use crate::graph::CriterionPath;
    use crate::graph::CriterionVersion;
    use crate::graph::Fingerprint;
    use crate::graph::NodePath;
    use crate::graph::Record;
    use crate::graph::ValidationError;
    use crate::request::ValidationContext;
    use crate::sources::Sources;
    use std::sync::{Arc, Mutex};

    struct State {
        text: String,
        reads: usize,
        read_nodes: Vec<String>,
        record: Option<Record<String>>,
        fail_read: bool,
        outline_root: Option<std::path::PathBuf>,
    }

    #[derive(Clone)]
    struct Project(Arc<Mutex<State>>, crate::domain::ValidationCache<Self>);

    enum Content {
        Text(String),
        Outline(crate::outline::OutlineContent),
    }
    impl NodeContents for Content {
        fn fingerprint(&self) -> Fingerprint {
            match self {
                Self::Text(text) => crate::procedure::context_fingerprint(text),
                Self::Outline(item) => item.fingerprint(),
            }
        }
        fn format(&self) -> String {
            match self {
                Self::Text(text) => text.clone(),
                Self::Outline(item) => item.format(),
            }
        }
    }
    impl Domain for Project {
        fn source_node(&self, item: &OutlineData) -> Result<String, crate::sources::SourceError> {
            let path = item.path.to_str().unwrap();
            if path == "unknown.txt" || item.selector == "line[unknown]" {
                return Err(crate::sources::SourceError::Invalid(
                    "unsupported source item".into(),
                ));
            }
            if path == "source.txt" && item.selector.is_empty() {
                return Ok("source-node".into());
            }
            Ok(format!("{path}#{}", item.selector))
        }

        fn source_address(&self, data: &String) -> Option<OutlineData> {
            let (path, selector) = data.split_once('#')?;
            OutlineData::new(path, selector.into()).ok()
        }

        fn cache(&self) -> &crate::domain::ValidationCache<Self> {
            &self.1
        }
        type Error = io::Error;
        type NodeData = String;
        type NodeContent = Content;
        type EdgeLabel = ();
        // :::{cite} habeas:coverage
        // Source-tool tests publish OutlineData addresses and retain mapped NodeIds
        // directly in dependencies. This required Domain method is unused in
        // those flows; stable-path and filesystem-storage tests use other domains.
        // :::
        fn node_path(&self, id: &NodeId<Self>) -> NodePath {
            let id = &*self.cache().node_data(*id).unwrap();
            NodePath::new(vec![id.clone()])
        }
        fn node_content(&self, node: &NodeId<Self>) -> io::Result<Content> {
            let node = &*self.cache().node_data(*node).unwrap();
            let mut state = self.0.lock().unwrap();
            state.reads += 1;
            state.read_nodes.push(node.clone());
            if state.fail_read {
                return Err(io::Error::other("prepared source read failed"));
            }
            if let Some(root) = &state.outline_root {
                let address = self.source_address(node).unwrap();
                let file = crate::outline::OutlineContent::from_path(
                    self.cache(),
                    root.join(address.path),
                )
                .map_err(io::Error::other)?;
                return Ok(Content::Outline(
                    file.select(&address.selector).map_err(io::Error::other)?,
                ));
            }
            Ok(Content::Text(state.text.clone()))
        }
        // :::{cite} habeas:coverage
        // This source-only fixture has no graph edges; graph-selection regressions use other fixtures.
        // :::
        fn outgoing_edges(&self, _: &NodeId<Self>, _: ()) -> io::Result<Vec<NodeId<Self>>> {
            Ok(Vec::new())
        }
        async fn load_evidence(
            &self,
            _: &NodeId<Self>,
            _: &CriterionPath,
        ) -> io::Result<Option<Record<String>>> {
            Ok(self.0.lock().unwrap().record.clone())
        }
        async fn store_evidence(
            &self,
            _: &NodeId<Self>,
            _: &CriterionPath,
            record: &Record<String>,
        ) -> io::Result<()> {
            self.0.lock().unwrap().record = Some(record.clone());
            Ok(())
        }
    }
    struct ReadSource(NodeId<Project>);
    impl Criterion<Project> for ReadSource {
        type Ok = String;
        fn source_node_id(&self) -> NodeId<Project> {
            self.0
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
            cx.read_source(&OutlineData {
                path: "source.txt".into(),
                selector: String::new(),
            })
            .map(|contents| contents.format())
            .map_err(ValidationError::Graph)
        }
    }

    fn project(text: &str) -> Project {
        Project(
            Arc::new(Mutex::new(State {
                text: text.into(),
                reads: 0,
                read_nodes: Vec::new(),
                record: None,
                fail_read: false,
                outline_root: None,
            })),
            Default::default(),
        )
    }

    // {test}`requirements.sdk.graph.validation-context.read_node.mcp`
    // {test}`requirements.sdk.sources.source_nodes`
    // {test}`requirements.sdk.graph.validation-context.file_node_arguments`
    // {test}`requirements.sdk.graph.validation-context.node_reference.consistency`
    // {test}`requirements.sdk.graph.validation-context.node_reference`
    // {test}`requirements.sdk.graph.validation-context.read_node.source_children`
    #[tokio::test]
    // {test}`requirements.sdk.graph.validation-context.reading_an_outline_parent`
    async fn agent_node_addresses_round_trip_and_reject_inconsistent_identities_before_reads() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("parser.rs"),
            "impl Parser { fn parse(&self) -> bool { true } }",
        )
        .unwrap();
        let domain = project("");
        domain.0.lock().unwrap().outline_root = Some(root.path().to_owned());
        let tools = ProjectSources::new(domain, Sources::new(root.path()));
        let file = crate::outline::OutlineContent::from_path(
            tools.domain.cache(),
            root.path().join("parser.rs"),
        )
        .unwrap();
        let expected = |selector: &str| {
            Dependency::Node(
                tools
                    .domain
                    .cache()
                    .intern_node(format!("parser.rs#{selector}")),
                file.select(selector).unwrap().fingerprint(),
            )
        };
        let mut session = SessionTools::new(tools.session());
        let cx = ValidationContext::new(
            tools.domain.as_ref(),
            tools.domain.cache().intern_node("criterion".into()),
        );
        let parent = session
            .read_node(&cx, &json!({"data":"parser.rs#impl[Parser]"}))
            .unwrap();
        assert_eq!(parent.dependencies, [expected("impl[Parser]")]);
        let parent: Value = serde_json::from_str(&parent.text).unwrap();
        let method = tools
            .domain
            .cache()
            .intern_node("parser.rs#impl[Parser]/fn[parse]".into());
        assert_eq!(
            parent,
            json!({
                "file":"parser.rs", "selector":"impl[Parser]", "text":"impl Parser { … }",
                "children":[{"node":method,"file":"parser.rs","selector":"impl[Parser]/fn[parse]",
                             "header":"fn parse(&self) -> bool {"}]
            })
        );
        let by_path = session
            .read_node(&cx, &json!({"path":"parser.rs","location":"impl[Parser]"}))
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&by_path.text).unwrap(),
            parent
        );
        let child = &parent["children"][0];
        let read = session
            .read_node(
                &cx,
                &json!({"path":"parser.rs", "location":child["selector"]}),
            )
            .unwrap();
        let by_id = session.read_node(&cx, child).unwrap();
        assert_eq!(read.dependencies, [expected("impl[Parser]/fn[parse]")]);
        assert_eq!(read.dependencies, by_id.dependencies);
        let method_page: Value = serde_json::from_str(&by_id.text).unwrap();
        assert_eq!(method_page["children"], json!([]));
        assert_eq!(method_page["text"], "fn parse(&self) -> bool { true }");
        let listed = session.dispatch("list_sources", &json!({})).await.unwrap();
        assert!(listed.dependencies.is_empty());
        let entry = &serde_json::from_str::<Value>(&listed.text).unwrap()["files"][0];
        let from_entry = session.read_node(&cx, entry).unwrap();
        let root_page: Value = serde_json::from_str(&from_entry.text).unwrap();
        assert_eq!(
            root_page["children"],
            json!([{"node":tools.domain.cache().intern_node("parser.rs#impl[Parser]".into()),"file":"parser.rs","selector":"impl[Parser]","header":"impl Parser {"}])
        );
        for address in [
            json!({"node":entry["node"]}),
            json!({"data":entry["data"]}),
            json!({"path":"parser.rs"}),
        ] {
            assert_eq!(
                session.read_node(&cx, &address).unwrap().dependencies,
                from_entry.dependencies
            );
        }
        let reads = tools.domain.0.lock().unwrap().reads;
        for invalid in [
            json!({}),
            json!({"node":entry["node"],"data":"parser.rs#impl[Parser]"}),
            json!({"node":10000,"data":entry["data"]}),
            json!({"node":-1}),
            json!({"data":false}),
            json!({"node":entry["node"],"data":false}),
            json!({"path":"parser.rs","data":entry["data"]}),
            json!({"path":"parser.rs","node":entry["node"]}),
            json!({"location":"impl[Parser]"}),
            json!({"path":"parser.rs","location":"fn[absent]"}),
            json!({"path":"absent.rs"}),
        ] {
            assert!(session.read_node(&cx, &invalid).is_err(), "{invalid}");
        }
        assert_eq!(tools.domain.0.lock().unwrap().reads, reads);
    }

    // {test}`requirements.sdk.graph.validation-context.read_node.source_children`
    // {test}`requirements.sdk.sources.redaction`
    #[test]
    fn child_headers_use_redacted_source_when_the_language_supplies_a_header() {
        let root = tempfile::tempdir().unwrap();
        let token = format!("gh{}_{}", "p", "16C7e42F292c6912E7710c838347Ae178B4a");
        std::fs::write(
            root.path().join("items.rs"),
            format!("fn run(/* {token} */) {{}}\nstruct Marker;"),
        )
        .unwrap();
        let domain = project("");
        domain.0.lock().unwrap().outline_root = Some(root.path().to_owned());
        let tools = ProjectSources::new(domain, Sources::new(root.path()));
        let mut session = SessionTools::new(tools.session());
        let graph = tools.domain.as_ref();
        let cx = ValidationContext::new(graph, graph.cache().intern_node("criterion".into()));
        let read = session.read_node(&cx, &json!({"path":"items.rs"})).unwrap();
        let page: Value = serde_json::from_str(&read.text).unwrap();
        let children = page["children"].as_array().unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(
            children[0]["header"],
            "fn run(/* [REDACTED:GitHub Token] */) {"
        );
        assert_eq!(children[1]["selector"], "struct[Marker]");
        assert!(children[1].get("header").is_none());
        assert!(!read.text.contains(&token));
    }

    // {test}`requirements.sdk.graph.search.search_structure.mcp`
    // {test}`requirements.sdk.graph.search.search_text.mcp`
    // {test}`requirements.sdk.graph.search.finish_search.inputs`
    #[tokio::test]
    // {test}`requirements.sdk.graph.search.agent_search`
    async fn scoped_search_accounts_for_visible_descendant_text() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("src")).unwrap();
        for path in ["src/code.rs", "other.rs"] {
            std::fs::write(
                root.path().join(path),
                "impl Parser { fn parse() { hit(); } }",
            )
            .unwrap();
        }
        let tools = ProjectSources::new(project(""), Sources::new(root.path()));
        let mut session = SessionTools::new(tools.session());
        let fixture = root.path().join("src/code.rs");
        let original =
            crate::outline::OutlineContent::from_path(tools.domain.cache(), &fixture).unwrap();
        let dependency = |selector: &str| {
            Dependency::Node(
                tools
                    .domain
                    .cache()
                    .intern_node(format!("src/code.rs#{selector}")),
                original.select(selector).unwrap().fingerprint(),
            )
        };
        for (tool, args, selector, expected) in [
            (
                "search_structure",
                json!({"language":"rust","rule":{"rule":{"pattern":"hit()"}},"path":"src"}),
                "impl[Parser]/fn[parse]",
                1,
            ),
            (
                "search_text",
                json!({"pattern":"impl Parser","path":"src/code.rs"}),
                "impl[Parser]",
                2,
            ),
            (
                "search_text",
                json!({"pattern":"impl Parser","path":"src","window_bytes":0}),
                "impl[Parser]",
                1,
            ),
        ] {
            let delivery = session.dispatch(tool, &args).await.unwrap();
            let page: Value = serde_json::from_str(&delivery.text).unwrap();
            assert_eq!(page["matches"].as_array().unwrap().len(), 1, "{page}");
            assert_eq!(page["matches"][0]["file"], "src/code.rs");
            assert_eq!(page["matches"][0]["selector"], selector);
            std::fs::write(&fixture, "impl Parser { fn parse() { changed(); } }").unwrap();
            let tracked = session
                .dispatch(
                    "finish_search",
                    &json!({"search_id":page["search_id"], "used":["1"]}),
                )
                .await
                .unwrap();
            let mut inputs = vec![dependency(selector)];
            if expected == 2 {
                inputs.push(dependency("impl[Parser]/fn[parse]"));
            }
            assert_eq!(tracked.dependencies, inputs, "{page}");
        }
    }

    // {test}`requirements.sdk.graph.search.agent_search_accounting`
    // {test}`requirements.sdk.graph.search.first_search.instructions`
    // {test}`requirements.sdk.graph.search.first_search.instructions.content`
    // {test}`requirements.sdk.graph.search.search_structure.inputs`
    // {test}`requirements.sdk.graph.search.finish_search.behavior`
    // {test}`requirements.sdk.graph.search.finish_search.mcp`
    // {test}`requirements.sdk.graph.search.finish_search.inputs`
    // {test}`requirements.sdk.graph.search.search_text.inputs`
    // {test}`requirements.sdk.graph.search.search_text.behavior`
    // {test}`requirements.sdk.graph.validation-context.pages.mcp`
    // {test}`requirements.sdk.graph.validation-context.pages.behavior`
    // {test}`requirements.sdk.graph.search.search_text.mcp`
    #[tokio::test]
    // {test}`requirements.sdk.graph.search.agent_search`
    async fn search_obligations_follow_delivery_and_keep_the_original_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("source.txt");
        let original = (0..300)
            .map(|n| format!("hit me here {n}\n"))
            .collect::<String>();
        std::fs::write(&path, &original).unwrap();
        let expected = crate::outline::OutlineContent::from_source(
            project("").cache(),
            "source.txt",
            original.into(),
        )
        .unwrap()
        .at_offset(0);
        let expected_id = project("").source_node(expected.id()).unwrap();
        let expected = expected.fingerprint();
        let tools = ProjectSources {
            sources: Sources::new(root.path()).into(),
            domain: project("unrelated domain text").into(),
        };
        let mut session = SessionTools::new(tools.session());
        let finish = session
            .extra
            .definitions()
            .unwrap()
            .into_iter()
            .find(|tool| tool.name == "finish_search")
            .unwrap();
        assert_eq!(finish.parameters["required"], json!(["search_id", "used"]));
        assert_eq!(
            finish.parameters["properties"]["search_id"],
            json!({"type":"string","format":"uuid"})
        );
        assert_eq!(
            finish.parameters["properties"]["used"],
            json!({"type":"array","items":{"type":"string"}})
        );
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
        for phrase in [
            "Before submitting, close every delivered search page",
            "finish_search",
            "tags for excerpts you relied on without a separate source read",
            "used:[] if none qualify, including an empty result",
            "without reading more source",
        ] {
            assert!(instructions.contains(phrase), "{instructions}");
        }
        let mut fresh = SessionTools::new(tools.session());
        let hits = fresh.dispatch("search_text", &call("hit")).await.unwrap();
        let hits: Value = serde_json::from_str(&hits.text).unwrap();
        assert!(!hits["matches"].as_array().unwrap().is_empty());
        assert_eq!(hits["complete"], true);
        let capped = fresh
            .dispatch("search_text", &json!({"pattern":"hit","limit":1}))
            .await
            .unwrap();
        let capped: Value = serde_json::from_str(&capped.text).unwrap();
        assert_eq!(capped["matches"].as_array().unwrap().len(), 1);
        assert_eq!(capped["complete"], false);
        fresh
            .dispatch(
                "finish_search",
                &json!({"search_id":capped["search_id"],"used":[]}),
            )
            .await
            .unwrap();
        assert_eq!(
            fresh.outstanding_searches(),
            [hits["search_id"].as_str().unwrap()]
        );
        assert_eq!(hits["instructions"], instructions);
        let rest = fresh
            .dispatch("get_next_page", &json!({"uuid":hits["next"]}))
            .await
            .unwrap();
        let rest: Value = serde_json::from_str(&rest.text).unwrap();
        assert!(rest.get("instructions").is_none());
        let first = peer
            .dispatch(
                "search_structure",
                &json!({"language":"rust","rule":{"rule":{"pattern":"absent()"}}}),
            )
            .await
            .unwrap();
        let page: Value = serde_json::from_str(&first.text).unwrap();
        assert_eq!(page["matches"], json!([]));
        assert_eq!(page["instructions"], instructions);
        assert_eq!(
            peer.outstanding_searches(),
            [page["search_id"].as_str().unwrap()]
        );
        let closed = peer
            .dispatch(
                "finish_search",
                &json!({"search_id": page["search_id"], "used": []}),
            )
            .await
            .unwrap();
        assert!(closed.dependencies.is_empty());
        let later = peer.dispatch("search_text", &call("absent")).await.unwrap();
        let page: Value = serde_json::from_str(&later.text).unwrap();
        assert!(page.get("instructions").is_none());
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
        let matches = first["matches"].as_array().unwrap();
        let tags: std::collections::BTreeSet<_> =
            matches.iter().map(|m| m["tag"].as_str().unwrap()).collect();
        assert!(matches.len() > 1);
        assert_eq!(tags.len(), matches.len());
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
                [Dependency::Node(
                    tools.domain.cache().intern_node(expected_id.clone()),
                    expected.clone()
                )]
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

    // {test}`requirements.sdk.graph.validation-context.pages.behavior`
    // {test}`requirements.sdk.graph.search.search_text.mcp`
    #[tokio::test]
    // {test}`requirements.sdk.graph.search.group_hits`
    async fn search_pages_return_every_original_match_in_order() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("source.txt");
        let lines: Vec<_> = (1..=350).map(|i| format!("hit {i} hit")).collect();
        std::fs::write(&path, lines.join("\n")).unwrap();
        let tools = ProjectSources {
            sources: Sources::new(root.path()).into(),
            domain: project("").into(),
        };
        let mut session = SessionTools::new(tools.session());
        let search = session
            .extra
            .definitions()
            .unwrap()
            .into_iter()
            .find(|tool| tool.name == "search_text")
            .unwrap();
        assert_eq!(
            search.parameters,
            json!({"type":"object","required":["pattern"],"properties":{
                "pattern":{"type":"string"}, "path":{"type":"string"},
                "window_bytes":{"type":"integer","minimum":0,"default":128}, "limit":{"type":"integer","minimum":0}
            }})
        );
        for invalid in [
            json!({}),
            json!({"pattern":"hit","limit":"1"}),
            json!({"pattern":"hit","window_bytes":-1}),
        ] {
            assert!(session.dispatch("search_text", &invalid).await.is_err());
        }

        let hits = tools
            .search_text(&TextQuery::new("hit".into()))
            .await
            .unwrap();
        let mut nodes: Vec<_> = hits
            .matches
            .into_iter()
            .map(|hit| (hit.node, hit.item.id().selector.clone()))
            .collect();
        nodes.dedup();
        let mut delivery = session
            .dispatch("search_text", &json!({"pattern":"hit"}))
            .await
            .unwrap();
        std::fs::write(&path, "changed").unwrap();
        let mut count = 0;
        let mut pages = 0;
        loop {
            let page: Value = serde_json::from_str(&delivery.text).unwrap();
            assert_eq!(page["complete"], true);
            assert!(uuid::Uuid::parse_str(page["search_id"].as_str().unwrap()).is_ok());
            assert_eq!(page.get("instructions").is_some(), pages == 0);
            let matches = page["matches"].as_array().unwrap();
            assert!(!matches.is_empty());
            for (index, entry) in matches.iter().enumerate() {
                assert_eq!(
                    entry,
                    &json!({"node":nodes[count].0, "file":"source.txt", "selector":nodes[count].1, "excerpt":lines[count], "tag":(index+1).to_string()})
                );
                count += 1;
            }
            pages += 1;
            let Some(next) = page.get("next") else { break };
            delivery = session
                .dispatch("get_next_page", &json!({"uuid":next}))
                .await
                .unwrap();
        }
        assert!(pages > 1);
        assert_eq!(count, lines.len());
    }

    // {test}`requirements.sdk.graph.search.search_structure.inputs`
    // {test}`requirements.sdk.graph.search.finish_search.inputs`
    // {test}`requirements.sdk.graph.search.agent_search`
    // {test}`requirements.sdk.graph.search.occurrence`
    #[tokio::test]
    // {test}`requirements.sdk.sources.SourceProject`
    // {test}`requirements.sdk.graph.search.search_structure.behavior`
    // {test}`requirements.sdk.graph.search.agent_search_accounting`
    // {test}`requirements.sdk.graph.search.enclosing_item`
    // {test}`requirements.sdk.graph.agent-harness.fixed_tools`
    // {test}`requirements.sdk.graph.search.StructuralRule`
    // {test}`requirements.sdk.graph.search.search_structure.mcp`
    async fn structural_tags_select_only_the_original_enclosing_item() {
        let root = tempfile::tempdir().unwrap();
        let code = root.path().join("code.rs");
        std::fs::write(&code, "fn one(){call();} fn two(){call();}").unwrap();
        let domain = project("");
        let tools = ProjectSources {
            sources: Sources::new(root.path()).into(),
            domain: domain.clone().into(),
        };
        let mut session = SessionTools::new(tools.session());
        let args = json!({"language":"rust","rule":{"rule":{"pattern":"call()"}}});
        let query = StructuralQuery::new("rust".into(), args["rule"].as_object().unwrap().clone());
        let hits = tools.search_structure(&query).await.unwrap();
        let rule =
            crate::sources::StructuralRule::compile(query.language.as_str(), &query.rule).unwrap();
        assert_eq!(
            rule.matches(&std::fs::read_to_string(&code).unwrap()),
            hits.matches.iter().map(|hit| hit.span).collect::<Vec<_>>()
        );
        assert_eq!(
            hits.matches
                .iter()
                .map(|h| (h.node, h.span.start..h.span.end, h.text.as_str()))
                .collect::<Vec<_>>(),
            [
                (
                    domain.cache().intern_node("code.rs#fn[one]".into()),
                    9..15,
                    "call()"
                ),
                (
                    domain.cache().intern_node("code.rs#fn[two]".into()),
                    27..33,
                    "call()"
                )
            ]
        );
        assert!(session.outstanding_searches().is_empty());
        std::fs::write(&code, "fn replaced() {}").unwrap();
        let mut limited = args.clone();
        limited["limit"] = json!(1);
        let limited = session
            .dispatch("search_structure", &limited)
            .await
            .unwrap();
        let limited: Value = serde_json::from_str(&limited.text).unwrap();
        assert_eq!(limited["complete"], false);
        assert_eq!(limited["matches"].as_array().unwrap().len(), 1);
        assert!(limited.get("next").is_none());
        let definition = session
            .extra
            .definitions()
            .unwrap()
            .into_iter()
            .find(|tool| tool.name == "search_structure")
            .unwrap();
        assert_eq!(
            definition.parameters,
            json!({"type":"object","required":["language","rule"],"properties":{"language":{"type":"string"},"rule":{"type":"object"},"path":{"type":"string"},"limit":{"type":"integer","minimum":0}}})
        );
        for invalid in [
            json!({"language":false,"rule":{}}),
            json!({"language":"rust","rule":false}),
            json!({"language":"rust","rule":{},"path":3}),
            json!({"language":"rust","rule":{},"limit":-1}),
        ] {
            assert!(
                session
                    .dispatch("search_structure", &invalid)
                    .await
                    .is_err()
            );
        }

        let search = session.dispatch("search_structure", &args).await.unwrap();
        assert!(search.dependencies.is_empty());
        let page: Value = serde_json::from_str(&search.text).unwrap();
        let matches = page["matches"].as_array().unwrap();
        assert_eq!(page["complete"], hits.complete);
        assert_eq!(matches.len(), 2);
        for (hit, entry) in hits.matches.iter().zip(matches) {
            assert_eq!(entry["excerpt"], hit.text);
        }
        let context = ValidationContext::new(&domain, domain.cache().intern_node(String::new()));
        let read = session.read_node(&context, &matches[1]).unwrap();
        assert_eq!(
            read.dependencies,
            [Dependency::Node(
                hits.matches[1].node.clone(),
                Content::Text(String::new()).fingerprint()
            )]
        );
        let tag = &matches
            .iter()
            .find(|m| m["node"] == json!(hits.matches[1].node))
            .unwrap()["tag"];
        let expected = crate::outline::OutlineContent::from_path(domain.cache(), &code)
            .unwrap()
            .select("fn[two]")
            .unwrap()
            .fingerprint();
        std::fs::write(&code, "fn two(){changed();}").unwrap();
        let finish = |used: Value| json!({"search_id":page["search_id"],"used":used});
        assert!(
            session
                .dispatch("finish_search", &finish(json!([])))
                .await
                .unwrap()
                .dependencies
                .is_empty()
        );
        for _ in 0..2 {
            let selected = session
                .dispatch("finish_search", &finish(json!([tag])))
                .await
                .unwrap();
            assert_eq!(
                selected.dependencies,
                [Dependency::Node(
                    domain.cache().intern_node("code.rs#fn[two]".into()),
                    expected.clone()
                )]
            );
        }
        assert_eq!(domain.0.lock().unwrap().reads, 1);
    }

    // {test}`requirements.sdk.graph.validation-context.outline_source.mcp`
    // {test}`requirements.sdk.graph.validation-context.outline_source.behavior`
    // {test}`requirements.sdk.graph.validation-context.pages.mcp`
    // {test}`requirements.sdk.graph.validation-context.pages.behavior`
    #[tokio::test]
    // {test}`requirements.sdk.graph.agent-harness.fixed_tools`
    // {test}`requirements.sdk.graph.validation-context.read_node.source_children`
    // {test}`requirements.sdk.graph.validation-context.read_node.mcp`
    async fn outline_continuations_keep_the_file_and_complete_ordered_items() {
        let root = tempfile::tempdir().unwrap();
        let names: Vec<_> = (0..400).map(|i| format!("item_{i}")).collect();
        let code: String = names
            .iter()
            .map(|name| format!("fn {name}() {{}}\n"))
            .collect();
        std::fs::write(root.path().join("code.rs"), code).unwrap();
        let tools = ProjectSources {
            sources: Sources::new(root.path()).into(),
            domain: project("fn item_4() {}").into(),
        };
        let mut session = SessionTools::new(tools.session());
        let mut delivery = session
            .dispatch("outline_source", &json!({"path":"code.rs"}))
            .await
            .unwrap();
        std::fs::write(root.path().join("code.rs"), "fn changed() {}").unwrap();
        let again = session
            .dispatch(
                "outline_source",
                &json!({"node":tools.domain.cache().intern_node("code.rs#".into())}),
            )
            .await
            .unwrap();
        let first: Value = serde_json::from_str(&delivery.text).unwrap();
        let again: Value = serde_json::from_str(&again.text).unwrap();
        assert_eq!(first["items"], again["items"]);
        for invalid in ["invalid", "00000000-0000-4000-8000-000000000000"] {
            assert!(
                session
                    .dispatch("get_next_page", &json!({"uuid":invalid}))
                    .await
                    .is_err()
            );
        }
        let mut items = Vec::new();
        let mut pages = 0;
        loop {
            let page: Value = serde_json::from_str(&delivery.text).unwrap();
            assert!(delivery.dependencies.is_empty());
            assert_eq!(
                page["node"],
                json!(tools.domain.cache().intern_node("code.rs#".into()))
            );
            let chunk = page["items"].as_array().unwrap();
            assert!(!chunk.is_empty());
            items.extend(chunk.iter().cloned());
            pages += 1;
            let Some(next) = page.get("next") else { break };
            delivery = session
                .dispatch("get_next_page", &json!({"uuid":next}))
                .await
                .unwrap();
        }
        assert!(pages > 1);
        assert_eq!(items.len(), names.len() + 1);
        for (i, item) in items.iter().enumerate() {
            assert_eq!(
                item["node"],
                json!(tools.domain.cache().intern_node(if i == 0 {
                    "code.rs#".into()
                } else {
                    format!("code.rs#fn[{}]", names[i - 1])
                }))
            );
            assert_eq!(item["file"], "code.rs");
            assert_eq!(
                item["selector"],
                if i == 0 {
                    String::new()
                } else {
                    format!("fn[{}]", names[i - 1])
                }
            );
            assert_eq!(item["kind"], if i == 0 { "" } else { "fn" });
        }
        let context = ValidationContext::new(
            tools.domain.as_ref(),
            tools.domain.as_ref().cache().intern_node(String::new()),
        );
        let root_text = "//! Project documentation.\n".repeat(800);
        tools.domain.0.lock().unwrap().text = root_text.clone();
        let mut root_read = session
            .read_node(&context, &json!({"path":"code.rs"}))
            .unwrap();
        tools.domain.0.lock().unwrap().text = "fn item_4() {}".into();
        let mut received = String::new();
        let mut children = Vec::new();
        loop {
            let page: Value = serde_json::from_str(&root_read.text).unwrap();
            assert_eq!(page["file"], "code.rs");
            assert_eq!(page["selector"], "");
            received.push_str(page["text"].as_str().unwrap());
            assert_eq!(root_read.dependencies.len(), 1);
            children.extend(
                page["children"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|child| child["node"].clone()),
            );
            let Some(next) = page.get("next") else { break };
            root_read = session
                .dispatch("get_next_page", &json!({"uuid":next}))
                .await
                .unwrap();
        }
        assert_eq!(received, root_text);
        assert_eq!(
            children,
            items[1..]
                .iter()
                .map(|item| item["node"].clone())
                .collect::<Vec<_>>()
        );
        let read = session.read_node(&context, &items[5]).unwrap();
        let text: Value = serde_json::from_str(&read.text).unwrap();
        assert_eq!(text["text"], "fn item_4() {}");
        assert_eq!(
            tools.domain.0.lock().unwrap().read_nodes,
            ["code.rs#", "code.rs#fn[item_4]"]
        );
    }

    // {test}`requirements.sdk.graph.validation-context.list_sources.mcp`
    // {test}`requirements.sdk.graph.validation-context.pages.behavior`
    // {test}`requirements.sdk.graph.validation-context.list_sources.inputs`
    // {test}`requirements.sdk.graph.validation-context.list_sources.behavior`
    // {test}`requirements.sdk.graph.validation-context.pages.mcp`
    #[tokio::test]
    async fn listing_continuations_return_complete_paths_from_the_original_listing() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("empty_directory")).unwrap();
        let expected: Vec<_> = (0..2000).map(|i| format!("item{i:04}.rs")).collect();
        for name in &expected {
            std::fs::write(root.path().join(name), "").unwrap();
        }
        let tools = ProjectSources {
            sources: Sources::new(root.path()).into(),
            domain: project("").into(),
        };
        let mut session = SessionTools::new(tools.session());
        let first = session.dispatch("list_sources", &json!({})).await.unwrap();
        assert!(first.dependencies.is_empty());
        let mut page: Value = serde_json::from_str(&first.text).unwrap();
        assert!(page["next"].is_string());
        std::fs::write(root.path().join("added.rs"), "").unwrap();
        let mut files = Vec::new();
        loop {
            for entry in page["files"].as_array().unwrap() {
                let node: NodeId<Project> = serde_json::from_value(entry["node"].clone()).unwrap();
                assert_eq!(
                    entry["data"],
                    **tools.domain.cache().node_data(node).as_ref().unwrap()
                );
                files.push(node);
            }
            let Some(uuid) = page.get("next") else { break };
            let delivery = session
                .dispatch("get_next_page", &json!({"uuid":uuid}))
                .await
                .unwrap();
            assert!(delivery.dependencies.is_empty());
            page = serde_json::from_str(&delivery.text).unwrap();
        }
        assert_eq!(
            files,
            expected
                .iter()
                .map(|file| tools.domain.cache().intern_node(format!("{file}#")))
                .collect::<Vec<_>>()
        );
    }

    // {test}`requirements.sdk.sources.SourceProject`
    // {test}`requirements.sdk.graph.search.search_text.behavior`
    // {test}`requirements.sdk.graph.search.same_line`
    // {test}`requirements.sdk.graph.search.agent_search`
    // {test}`requirements.sdk.graph.search.domain_node`
    // {test}`requirements.sdk.graph.search.grouped_constants`
    // {test}`requirements.sdk.graph.search.selecting_an_excerpt`
    // {test}`requirements.sdk.graph.search.occurrence`
    // {test}`requirements.sdk.graph.search.group_hits`
    // {test}`requirements.sdk.graph.search.omissions`
    // {test}`requirements.sdk.graph.search.windows`
    #[tokio::test]
    // {test}`requirements.sdk.graph.search.agent_search_accounting`
    async fn search_groups_exact_occurrences_and_records_the_selected_declaration() {
        let root = tempfile::tempdir().unwrap();
        let code = "const A: &str = \"éhit\"; const B: &str = \"hit hit\";";
        std::fs::write(root.path().join("code.rs"), code).unwrap();
        let project = ProjectSources::new(project(""), Sources::new(root.path()));
        let results = project
            .search_text(&TextQuery::new("hit".into()))
            .await
            .unwrap();
        assert_eq!(
            results
                .matches
                .iter()
                .map(|h| (h.node, h.span.start..h.span.end, h.text.as_str()))
                .collect::<Vec<_>>(),
            [
                (
                    project
                        .domain
                        .cache()
                        .intern_node("code.rs#const[A]".into()),
                    19..22,
                    "hit"
                ),
                (
                    project
                        .domain
                        .cache()
                        .intern_node("code.rs#const[B]".into()),
                    42..45,
                    "hit"
                ),
                (
                    project
                        .domain
                        .cache()
                        .intern_node("code.rs#const[B]".into()),
                    46..49,
                    "hit"
                )
            ]
        );
        let mut session = SessionTools::new(project.session());
        let result = session
            .dispatch("search_text", &json!({"pattern":"hit"}))
            .await
            .unwrap();
        assert!(result.dependencies.is_empty());
        let page: Value = serde_json::from_str(&result.text).unwrap();
        assert_eq!(page["complete"], true);
        assert_eq!(
            page["matches"],
            json!([
                {"tag":"1", "node":results.matches[0].node, "file":"code.rs", "selector":"const[A]", "excerpt":"const A: &str = \"éhit\";"},
                {"tag":"2", "node":results.matches[1].node, "file":"code.rs", "selector":"const[B]", "excerpt":"const B: &str = \"hit hit\";"}
            ])
        );
        let context = ValidationContext::new(
            project.domain.as_ref(),
            project.domain.as_ref().cache().intern_node(String::new()),
        );
        let read = session.read_node(&context, &page["matches"][1]).unwrap();
        assert_eq!(
            read.dependencies,
            [Dependency::Node(
                results.matches[1].node.clone(),
                Content::Text(String::new()).fingerprint()
            )]
        );
        let unused = session
            .dispatch(
                "finish_search",
                &json!({"search_id":page["search_id"], "used":[]}),
            )
            .await
            .unwrap();
        assert!(unused.dependencies.is_empty());
        let finished = session
            .dispatch(
                "finish_search",
                &json!({"search_id":page["search_id"], "used":["2"]}),
            )
            .await
            .unwrap();
        assert_eq!(
            finished.dependencies,
            [Dependency::Node(
                project
                    .domain
                    .cache()
                    .intern_node("code.rs#const[B]".into()),
                results.matches[1].item.fingerprint()
            )]
        );
    }

    // {test}`requirements.sdk.graph.search.enclosing_item`
    // {test}`requirements.sdk.outlines.attachments.item_span`
    #[tokio::test]
    async fn search_in_leading_and_trailing_comments_selects_the_function() {
        let root = tempfile::tempdir().unwrap();
        let source = "// note before\npub fn check() {} // note after\nfn next() {}\n";
        std::fs::write(root.path().join("example.rs"), source).unwrap();
        let project = ProjectSources::new(project(source), Sources::new(root.path()));
        let hits = project
            .search_text(&TextQuery::new("note".into()))
            .await
            .unwrap();
        let expected = project
            .domain
            .cache()
            .intern_node("example.rs#fn[check]".into());
        assert_eq!(hits.matches.len(), 2);
        assert!(hits.matches.iter().all(|hit| hit.node == expected));

        let select = |source: &str| {
            crate::outline::OutlineContent::from_source(
                project.domain.cache(),
                "example.rs",
                source.into(),
            )
            .unwrap()
            .select("fn[check]")
            .unwrap()
        };
        let selected = select(source);
        assert_eq!(
            selected.text().trim_end(),
            "// note before\npub fn check() {} // note after"
        );
        assert_eq!(
            &source[selected.item().header_span.unwrap()],
            "pub fn check() {"
        );
        assert_eq!(
            selected.structural_hash(),
            select(&source.replace("note", "remark")).structural_hash()
        );
    }

    // {test}`requirements.sdk.graph.search.windows`
    // {test}`requirements.sdk.graph.search.omissions`
    // {test}`requirements.sdk.graph.search.enclosing_item`
    // {test}`requirements.sdk.graph.search.file_root`
    // {test}`requirements.sdk.graph.search.utf8_window`
    #[tokio::test]
    // {test}`requirements.sdk.graph.search.agent_search`
    async fn root_hits_clip_context_to_utf8_boundaries() {
        let root = tempfile::tempdir().unwrap();
        for (source, pattern, small) in
            [("needle", "needle", "needle"), ("éhit猫", "hit", "éhit...")]
        {
            std::fs::write(root.path().join("fragment.rs"), source).unwrap();
            let project = ProjectSources::new(project(source), Sources::new(root.path()));
            let hits = project
                .search_text(&TextQuery::new(pattern.into()))
                .await
                .unwrap();
            assert_eq!(
                hits.matches[0].node,
                project.domain.cache().intern_node("fragment.rs#".into())
            );
            let cx = ValidationContext::new(
                project.domain.as_ref(),
                project.domain.as_ref().cache().intern_node(String::new()),
            );
            let mut session = SessionTools::new(project.session());
            for (args, expected) in [
                (json!({"pattern":pattern, "window_bytes":2}), small),
                (json!({"pattern":pattern}), source),
            ] {
                let result = session.dispatch("search_text", &args).await.unwrap();
                let page: Value = serde_json::from_str(&result.text).unwrap();
                assert_eq!(
                    page["matches"],
                    json!([{"tag":"1", "node":hits.matches[0].node, "file":"fragment.rs", "selector":"", "excerpt":expected}])
                );
                let read = session.read_node(&cx, &page["matches"][0]).unwrap();
                let payload: Value = serde_json::from_str(&read.text).unwrap();
                assert_eq!(payload["text"], source);
                assert_eq!(
                    project.domain.0.lock().unwrap().read_nodes.last().unwrap(),
                    "fragment.rs#"
                );
            }
        }
    }

    // {test}`requirements.sdk.graph.search.windows`
    // {test}`requirements.sdk.graph.search.omissions`
    #[tokio::test]
    async fn default_windows_retain_128_bytes_on_each_side() {
        let root = tempfile::tempdir().unwrap();
        let code = format!("{}猫hit猫{}", "a".repeat(200), "z".repeat(200));
        std::fs::write(root.path().join("fragment.rs"), code).unwrap();
        let project = ProjectSources::new(project(""), Sources::new(root.path()));
        let mut session = SessionTools::new(project.session());
        for (args, expected) in [
            (
                json!({"pattern":"hit"}),
                format!("...{}猫hit猫{}...", "a".repeat(125), "z".repeat(125)),
            ),
            (
                json!({"pattern":"hit","window_bytes":1}),
                "...hit...".to_owned(),
            ),
        ] {
            let result = session.dispatch("search_text", &args).await.unwrap();
            let page: Value = serde_json::from_str(&result.text).unwrap();
            assert_eq!(page["matches"][0]["excerpt"], expected);
        }
    }

    // {test}`requirements.sdk.graph.search.windows`
    #[test]
    // {test}`requirements.sdk.graph.search.omissions`
    fn touching_windows_keep_the_contiguous_source() {
        use crate::outline::{OutlineContent, Span};
        let item =
            OutlineContent::from_source(project("").cache(), "code.rs", "  hit    hit  ".into())
                .unwrap();
        assert_eq!(
            excerpt(&item, &[Span::new(2, 5), Span::new(9, 12)], 2).0,
            item.text()
        );
    }

    // {test}`requirements.sdk.graph.search.enclosing_item`
    #[tokio::test]
    async fn matches_crossing_item_boundaries_belong_to_the_item_at_the_start() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("pair.rs"), "fn a() {}   fn b() {}").unwrap();
        let project = ProjectSources::new(project(""), Sources::new(root.path()));
        for (pattern, expected) in [
            ("   fn b", "pair.rs#"),
            (r"\}   fn b", "pair.rs#fn[a]"),
            ("fn b", "pair.rs#fn[b]"),
        ] {
            let hits = project
                .search_text(&TextQuery::new(pattern.into()))
                .await
                .unwrap();
            assert_eq!(hits.matches.len(), 1);
            assert_eq!(
                hits.matches[0].node,
                project.domain.cache().intern_node(expected.into())
            );
        }
    }

    // {test}`requirements.sdk.graph.search.domain_node`
    #[tokio::test]
    async fn an_unmapped_hit_fails_the_search() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("unknown.txt"), "hit").unwrap();
        let project = ProjectSources::new(project(""), Sources::new(root.path()));
        let error = project
            .search_text(&TextQuery::new("hit".into()))
            .await
            .err()
            .unwrap();
        assert!(error.to_string().contains("unsupported source item"));
    }

    // {test}`requirements.sdk.graph.search.declaration_spans`
    #[test]
    fn rust_declarations_supply_independent_file_spans() {
        let code = "// lead\nstruct T; impl T { fn f() {} } const C: u8 = 1; static S: u8 = 2;";
        let outline = crate::outline::OutlineContent::from_source(
            project("").cache(),
            "input.rs",
            code.into(),
        )
        .unwrap();
        for (kind, header, trailer) in [
            ("impl", Some("impl T {"), Some("}")),
            ("fn", Some("fn f() {"), Some("}")),
            ("const", Some("const C: u8 ="), None),
            ("static", Some("static S: u8 ="), None),
            ("struct", None, None),
        ] {
            let item = outline
                .outline()
                .items
                .iter()
                .find(|i| i.kind == kind)
                .unwrap();
            assert_eq!(item.header_span.map(|s| &code[s]), header);
            assert_eq!(item.trailer_span.map(|s| &code[s]), trailer);
            if let Some(span) = item.trailer_span {
                assert_eq!(span.end, item.span.end);
                assert_eq!(
                    span.start,
                    if kind == "impl" {
                        code.rfind('}').unwrap()
                    } else {
                        code.find('}').unwrap()
                    }
                );
            }
        }
    }

    // {test}`requirements.sdk.graph.search.windows`
    // {test}`requirements.sdk.graph.search.separated_hits`
    // {test}`requirements.sdk.graph.search.declaration_spans`
    // {test}`requirements.sdk.graph.search.enclosing_item`
    // {test}`requirements.sdk.graph.search.omissions`
    #[tokio::test]
    // {test}`requirements.sdk.graph.search.agent_search`
    async fn separated_hit_windows_preserve_the_function_declaration() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("runner.rs");
        let code = "impl Runner {\n    fn run() {\n        setup();\n        hit(); hit();\n        cleanup();\n        hit();\n        finish();\n    }\n}";
        std::fs::write(&path, code).unwrap();
        let full = &code[code.find("fn run").unwrap()..code.rfind("\n}").unwrap()];
        let project = ProjectSources::new(project(full), Sources::new(root.path()));
        let context = ValidationContext::new(
            project.domain.as_ref(),
            project.domain.as_ref().cache().intern_node(String::new()),
        );
        let mut session = SessionTools::new(project.session());
        for (window, expected) in [
            (4, "fn run() {...    hit(); hit();\n...    hit();\n...}"),
            (0, "fn run() {...hit...hit...hit...}"),
        ] {
            let page = session
                .dispatch(
                    "search_text",
                    &json!({"pattern":"hit", "window_bytes":window}),
                )
                .await
                .unwrap();
            let page: Value = serde_json::from_str(&page.text).unwrap();
            assert_eq!(
                page["matches"],
                json!([{"tag":"1", "node":project.domain.cache().intern_node("runner.rs#impl[Runner]/fn[run]".into()), "file":"runner.rs", "selector":"impl[Runner]/fn[run]", "excerpt":expected}])
            );
            let read = session.read_node(&context, &page["matches"][0]).unwrap();
            let read: Value = serde_json::from_str(&read.text).unwrap();
            assert_eq!(read["text"], full);
            assert!(read["text"].as_str().unwrap().contains("cleanup();"));
            assert_eq!(
                project.domain.0.lock().unwrap().read_nodes.last().unwrap(),
                "runner.rs#impl[Runner]/fn[run]"
            );
            std::fs::write(&path, "fn changed() {}").unwrap();
        }
    }

    // {test}`requirements.sdk.graph.validation-context.read_source.inputs`
    #[tokio::test]
    async fn deterministic_source_reads_reuse_saved_results() {
        let domain = project("first");
        let config = crate::agent::AgentConfig::OpenAi(crate::agent::OpenAiConfig {
            api_key_env: Some("TBV_NEVER_REQUIRED_BY_DETERMINISTIC_SOURCE_READS".into()),
            ..Default::default()
        });
        let mut domain = domain;
        domain.1 = crate::domain::ValidationCache::with_agent(config);
        let first = validation::refresh_evidence(
            &domain,
            ReadSource(domain.cache().intern_node("source-node".into())),
        )
        .await
        .unwrap();
        assert_eq!(first.output(), "first");
        assert_eq!(
            first.dependencies(),
            &[Dependency::Node(
                domain.cache().intern_node("source-node".into()),
                Content::Text("first".into()).fingerprint()
            )]
        );
        let saved = validation::refresh_evidence(
            &domain,
            ReadSource(domain.cache().intern_node("source-node".into())),
        )
        .await
        .unwrap();
        assert_eq!(saved.output(), "first");
        domain.0.lock().unwrap().text = "second".into();
        let changed = validation::refresh_evidence(
            &domain,
            ReadSource(domain.cache().intern_node("source-node".into())),
        )
        .await
        .unwrap();
        assert_eq!(changed.output(), "second");
    }

    // {test}`requirements.sdk.graph.validation-context.read_source.fn`
    // {test}`requirements.sdk.graph.validation-context.read_source.behavior`
    // {test}`requirements.sdk.graph.validation-context.read_source.inputs`
    // {test}`requirements.sdk.sources.source_nodes`
    // {test}`requirements.sdk.sources.provider_observations`
    // {test}`requirements.sdk.graph.validation-context.ValidationContext.read_source`
    // {test}`requirements.sdk.graph.validation-context.ValidationContext`
    #[test]
    fn direct_source_reads_preserve_mapping_errors_and_record_only_successful_returns() {
        let domain = project("prepared contents");
        let inspection =
            ValidationContext::new(&domain, domain.cache().intern_node("source-node".into()));
        let supported = OutlineData::new("source.txt", String::new()).unwrap();
        for address in [
            OutlineData::new("unknown.txt", String::new()).unwrap(),
            OutlineData::new("source.txt", "line[unknown]".into()).unwrap(),
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
                domain.cache().intern_node("source-node".into()),
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
    // {test}`requirements.sdk.graph.search.search_structure.inputs`
    // {test}`requirements.sdk.graph.search.search_text.mcp`
    #[tokio::test]
    // {test}`requirements.sdk.graph.search.search_structure.mcp`
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
            sources: Sources::new(root.path()).into(),
            domain: domain.clone().into(),
        };
        let mut session = SessionTools::new(tools.session());
        let queries = [
            (
                "search_text",
                json!({"pattern":"fn chosen","window_bytes":25000}),
            ),
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
            let selector = "fn[chosen]";
            let snapshot = crate::outline::OutlineContent::from_source(
                domain.cache(),
                "code.rs",
                raw.as_str().into(),
            )
            .unwrap()
            .select(selector)
            .unwrap();
            assert_eq!(
                selected.dependencies,
                [Dependency::Node(
                    domain.cache().intern_node(format!("code.rs#{selector}")),
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
    // {test}`requirements.sdk.graph.validation-context.outline_source.mcp`
    // {test}`requirements.sdk.graph.validation-context.node_reference`
    async fn outline_tool_rejects_excluded_and_unreadable_sources_without_domain_reads() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("source.txt"), "first\nsecond\n").unwrap();
        std::fs::write(root.path().join(".gitignore"), "ignored.txt\n").unwrap();
        std::fs::write(root.path().join("ignored.txt"), "hidden").unwrap();
        std::fs::create_dir(root.path().join(".git")).unwrap();
        std::fs::write(root.path().join(".git/config"), "metadata").unwrap();
        let domain = project("domain text");
        let tools = ProjectSources {
            sources: Sources::new(root.path()).into(),
            domain: domain.clone().into(),
        };
        let mut session = SessionTools::new(tools.session());
        let listing = session.dispatch("list_sources", &json!({})).await.unwrap();
        let listing: Value = serde_json::from_str(&listing.text).unwrap();
        let files = tools.sources.list(&Default::default()).await.unwrap();
        let index = files
            .iter()
            .position(|p| p == std::path::Path::new("source.txt"))
            .unwrap();
        let reference = &listing["files"][index];
        let allowed = session.dispatch("outline_source", reference).await.unwrap();
        assert!(allowed.dependencies.is_empty());
        let outline: Value = serde_json::from_str(&allowed.text).unwrap();
        assert_eq!(outline["node"], reference["node"]);
        assert!(outline["items"][0]["node"].is_u64());
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
        std::fs::write(root.path().join("invalid.rs"), "fn {").unwrap();
        let listing = session.dispatch("list_sources", &json!({})).await.unwrap();
        let listing: Value = serde_json::from_str(&listing.text).unwrap();
        let files = tools.sources.list(&Default::default()).await.unwrap();
        let index = files
            .iter()
            .position(|p| p == std::path::Path::new("invalid.rs"))
            .unwrap();
        let recovered = session
            .dispatch("outline_source", &listing["files"][index])
            .await
            .unwrap();
        let outline: Value = serde_json::from_str(&recovered.text).unwrap();
        assert!(outline["items"][0]["node"].is_u64());
        assert_eq!(domain.0.lock().unwrap().reads, 0);
    }
}

impl<D: crate::domain::Domain> ToolFactory<D> for ProjectSources<D>
where
    D::Error: std::fmt::Display,
    D::NodeData: serde::Serialize + serde::de::DeserializeOwned + schemars::JsonSchema,
{
    fn session<'a>(&'a self) -> Box<dyn ToolSession<D> + 'a>
    where
        NodeId<D>: 'a,
        D::EdgeLabel: 'a,
    {
        Box::new(Session { tools: self })
    }
}

struct Session<'a, D: crate::domain::Domain> {
    tools: &'a ProjectSources<D>,
}

impl<D: crate::domain::Domain> ToolSession<D> for Session<'_, D>
where
    D::Error: std::fmt::Display,
    D::NodeData: serde::Serialize + serde::de::DeserializeOwned + schemars::JsonSchema,
{
    fn project(&self) -> Option<&ProjectSources<D>> {
        Some(self.tools)
    }

    // {impl}`requirements.sdk.graph.validation-context.list_sources.mcp`
    // {impl}`requirements.sdk.graph.validation-context.list_sources.mcp.description`
    // {impl}`requirements.sdk.graph.validation-context.outline_source.mcp`
    // {impl}`requirements.sdk.graph.validation-context.outline_source.mcp.description`
    // {impl}`requirements.sdk.graph.search.search_text.mcp`
    // {impl}`requirements.sdk.graph.search.search_text.mcp.description`
    // {impl}`requirements.sdk.graph.search.search_structure.mcp`
    // {impl}`requirements.sdk.graph.search.search_structure.mcp.description`
    // {impl}`requirements.sdk.graph.search.finish_search.mcp`
    // {impl}`requirements.sdk.graph.search.finish_search.mcp.description`
    fn definitions(&self) -> io::Result<Vec<ToolDefinition>> {
        [
            json!({"name":LIST_SOURCES,"description":"List available project file nodes.", "parameters":{"type":"object","properties":{}}}),
            json!({"name":OUTLINE_SOURCE,"description":"Inspect the outline of a file node.","parameters":super::tools::node_parameters::<NodeId<D>, D::NodeData>(None)}),
            json!({"name":SEARCH_TEXT,"description":"Find regular-expression matches in available files, optionally within a file or directory. Read selected nodes separately.","parameters":{"type":"object","required":["pattern"],"properties":{"pattern":{"type":"string"},"path":{"type":"string"},"window_bytes":{"type":"integer","minimum":0,"default":128},"limit":{"type":"integer","minimum":0}}}}),
            json!({"name":SEARCH_STRUCTURE,"description":"Find structural matches in available files, optionally within a file or directory. Read selected nodes separately.","parameters":{"type":"object","required":["language","rule"],"properties":{"language":{"type":"string"},"rule":{"type":"object"},"path":{"type":"string"},"limit":{"type":"integer","minimum":0}}}}),
        json!({
            "name":FINISH_SEARCH,
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

    // {impl}`requirements.sdk.graph.search.agent_search`
    // {impl}`requirements.sdk.graph.search.group_hits`
    // {impl}`requirements.sdk.graph.search.search_text.behavior`
    // {impl}`requirements.sdk.graph.search.search_structure.behavior`
    // {impl}`requirements.sdk.graph.search.search_text.inputs`
    // {impl}`requirements.sdk.graph.search.search_text.mcp`
    // {impl}`requirements.sdk.graph.search.search_structure.mcp`
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
        pages: &'a mut Pages<NodeId<D>, D::EdgeLabel>,
    ) -> BoxFuture<'a, io::Result<ToolDelivery<NodeId<D>, D::EdgeLabel>>> {
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
            let search_scope = || -> io::Result<SourceScope> {
                match args.get("path") {
                    Some(_) => Ok(SourceScope::under(string("path")?)),
                    None => Ok(SourceScope::all()),
                }
            };
            let limit = optional_number("limit")?;
            let (metadata, field, contents) = match name {
                LIST_SOURCES => {
                    let mut files = Vec::new();
                    for path in self
                        .tools
                        .sources
                        .list(&SourceScope::all())
                        .await
                        .map_err(io::Error::other)?
                    {
                        let id = OutlineData::root(&path);
                        let node = self.tools.intern_source(&id)?;
                        let data = self
                            .tools
                            .domain
                            .cache()
                            .node_data(node)
                            .expect("interned source");
                        files.push(json!({"node":node,"data":data.as_ref()}));
                    }
                    (json!({}), "files", json!(files))
                }
                OUTLINE_SOURCE => {
                    let node = super::tools::node_argument(
                        self.tools.domain.as_ref(),
                        Some(self.tools),
                        args,
                    )?;
                    let address = self
                        .tools
                        .address(node)
                        .filter(|id| id.selector.is_empty())
                        .ok_or_else(|| io::Error::other("expected a file-root node"))?;
                    let contents = self.tools.resolve_outline(&address)?;
                    let path = &address.path;
                    let mut items = Vec::new();
                    for item in &contents.outline().items {
                        let id = OutlineData {
                            path: path.clone(),
                            selector: item.selector.clone(),
                        };
                        let node = self.tools.intern_source(&id)?;
                        items.push(json!({"node":node,"kind":item.kind,"file":path,"selector":item.selector}));
                    }
                    (json!({"node":node}), "items", json!(items))
                }
                FINISH_SEARCH => return pages.finish_search(args),
                SEARCH_TEXT => {
                    let window = optional_number("window_bytes")?.unwrap_or(128);
                    let mut query = TextQuery::new(string("pattern")?.to_owned());
                    query.limit = limit;
                    query.scope = search_scope()?;
                    let found = self
                        .tools
                        .search_text(&query)
                        .await
                        .map_err(io::Error::other)?;
                    let mut groups = Vec::<(
                        crate::sources::SearchHit<NodeId<D>>,
                        Vec<crate::outline::Span>,
                    )>::new();
                    let mut indices = std::collections::HashMap::new();
                    for hit in found.matches {
                        let item = hit.item.item();
                        let key = (
                            hit.item.id().path.clone(),
                            item.selector.clone(),
                            item.span.start,
                            item.span.end,
                        );
                        let span = hit.span;
                        let index = *indices.entry(key).or_insert_with(|| {
                            let index = groups.len();
                            groups.push((hit, Vec::new()));
                            index
                        });
                        groups[index].1.push(span);
                    }
                    let entries = groups
                        .into_iter()
                        .map(|(hit, spans)| {
                            let (text, visible) = excerpt(&hit.item, &spans, window);
                            let entry =
                                json!({"node":serde_json::to_value(&hit.node)?,"file":hit.item.id().path,"selector":hit.item.id().selector,"excerpt":text});
                            Ok((
                                entry,
                                excerpt_dependencies(self.tools, &hit, &visible)?,
                            ))
                        })
                        .collect::<io::Result<Vec<_>>>()?;
                    return Ok(
                        pages.search(crate::sources::SearchResults::new(entries, found.complete))
                    );
                }
                SEARCH_STRUCTURE => {
                    let rule = args["rule"]
                        .as_object()
                        .ok_or_else(|| io::Error::other("rule must be an object"))?
                        .clone();
                    let mut query = StructuralQuery::new(string("language")?.into(), rule);
                    query.limit = limit;
                    query.scope = search_scope()?;
                    let found = self
                        .tools
                        .search_structure(&query)
                        .await
                        .map_err(io::Error::other)?;
                    let entries = found
                        .matches
                        .into_iter()
                        .map(|hit| {
                            let node = serde_json::to_value(&hit.node)?;
                            let excerpt = hit
                                .item
                                .source_span_redacted(hit.span)
                                .ok_or_else(|| io::Error::other("invalid structural match span"))?;
                            let entry = json!({"node": node,"file":hit.item.id().path,"selector":hit.item.id().selector, "excerpt": excerpt});
                            Ok((
                                entry,
                                excerpt_dependencies(self.tools, &hit, &[hit.span])?,
                            ))
                        })
                        .collect::<io::Result<Vec<_>>>()?;
                    return Ok(
                        pages.search(crate::sources::SearchResults::new(entries, found.complete))
                    );
                }
                _ => return Err(io::Error::other(format!("unknown source tool {name:?}"))),
            };
            Ok(pages.prepare(metadata, field, contents, Vec::new()))
        })
    }
}

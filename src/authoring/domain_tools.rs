//! Habeas-specific discovery operations registered on the standard SDK agent.
use super::*;
use crate::inputs::InputSelector;
use crate::sdk::{HabeasDomain, HabeasNode};
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::Arc;
use trust_but_verify::{
    agent::{AgentConfig, Tool},
    domain::Domain,
    request::ValidationContext,
    sources::Sources,
};

// {impl}`requirements.authoring.source_tools`
// {impl}`requirements.authoring.read_denial`
pub(super) fn configure(
    domain: &mut HabeasDomain,
    backend: &config::BackendConfig,
    github: crate::github::GithubIssues,
) -> io::Result<()> {
    domain.github = github.clone();
    let config: AgentConfig = serde_json::from_value(serde_json::to_value(backend)?)?;
    let cache = domain
        .outlines
        .validation
        .clone()
        .with_sources(Sources::new(domain.root.to_path_buf()));
    domain.outlines.validation = cache;
    domain.outlines.validation.set_agent(
        config
            .with_tool(Arc::new(LookupRequirements))
            .with_tool(Arc::new(RunNamedQuery))
            .with_tool(Arc::new(Github(github))),
    );
    Ok(())
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Lookup {
    selection: Option<String>,
}
struct LookupRequirements;
impl Tool<HabeasDomain, Lookup, Value> for LookupRequirements {
    fn name(&self) -> &str {
        "lookup_requirements"
    }
    fn description(&self) -> &str {
        "Find requirement and example nodes by optional selection, a name or glob such as requirements.sdk.**. Each result contains node and data. Pass it to read_node for the declaration or outgoing_edges for its references. References are starting points for a wider source search."
    }
    async fn execute(&self, cx: &ValidationContext<'_, HabeasDomain>, args: Lookup) -> Value {
        if let Some(selection) = &args.selection
            && let Err(error) = crate::definition_glob::validate(selection)
        {
            return json!({"error":error});
        }
        let graph = cx.graph();
        let nodes: Vec<_> = graph
            .definitions
            .iter()
            .filter(|(name, definition)| {
                definition.kind != DefinitionKind::Term
                    && args
                        .selection
                        .as_ref()
                        .is_none_or(|s| crate::definition_glob::matches(s, name))
            })
            .map(|(name, _)| {
                let data = HabeasNode::from(&InputSelector::Requirements {
                    requirements: name.clone(),
                });
                json!({"node": graph.cache().intern_node(data.clone()), "data": data})
            })
            .collect();
        json!(nodes)
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Query {
    query: String,
}
struct RunNamedQuery;
impl Tool<HabeasDomain, Query, Value> for RunNamedQuery {
    fn name(&self) -> &str {
        "run_named_query"
    }
    fn description(&self) -> &str {
        "Use a declared query to inspect the sites of a distributed requirement. Supply query as <requirement>/<query>, for example requirements.parser/functions. Each result contains node and data. Pass it to read_node to inspect its source."
    }
    async fn execute(&self, cx: &ValidationContext<'_, HabeasDomain>, args: Query) -> Value {
        let graph = cx.graph();
        let run = || -> Result<Vec<Value>, String> {
            let (requirement, name) = args
                .query
                .rsplit_once('/')
                .ok_or("expected <requirement>/<query>")?;
            let declaration = graph
                .definitions
                .get(requirement)
                .ok_or("unknown requirement")?;
            let queries = crate::structural::declared_queries(declaration)?;
            let catalog = graph.outlines.reference_catalog(&graph.root);
            let files = &catalog.as_ref().as_ref().map_err(Clone::clone)?.files;
            let sites = crate::structural::query_sites(
                &graph.root,
                files,
                &graph.outlines,
                &queries,
                name,
            )?;
            Ok(sites
                .into_iter()
                .map(|site| {
                    let data = HabeasNode::from(&InputSelector::Outline {
                        file: site.file,
                        item: site.item,
                    });
                    json!({"node":graph.cache().intern_node(data.clone()), "data":data})
                })
                .collect())
        };
        match run() {
            Ok(nodes) => json!(nodes),
            Err(error) => json!({"error":line_numbers::without_source_lines(&error)}),
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Issue {
    issue: String,
}
struct Github(crate::github::GithubIssues);
impl Tool<HabeasDomain, Issue, Value> for Github {
    fn name(&self) -> &str {
        "github"
    }
    fn description(&self) -> &str {
        "Inspect a project issue when reviewing an acknowledgment. Supply issue as owner/repository#number, for example org/project#42. The result includes its title, opening post and state."
    }
    async fn execute(&self, _: &ValidationContext<'_, HabeasDomain>, args: Issue) -> Value {
        crate::github::tool(&self.0, &json!({"issue":args.issue}))
            .unwrap_or_else(|error| json!({"error":error}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sites::tests::{RULE, SOURCE, project};

    // {test}`requirements.authoring.tool_descriptions.special_cases`
    // {test}`requirements.authoring.no_line_numbers`
    #[tokio::test]
    async fn named_query_errors_identify_the_source_without_exposing_a_line_number() {
        let temp = project(RULE, SOURCE);
        let mut graph = HabeasDomain::discover(temp.path()).unwrap();
        let rule = Arc::make_mut(&mut graph.definitions)
            .get_mut("requirements.rule")
            .unwrap();
        let fence = rule
            .raw_lines
            .iter()
            .position(|line| line == "```yaml")
            .unwrap();
        rule.raw_lines[fence] = "```toml".into();
        let diagnostic = crate::structural::declared_queries(rule).err().unwrap();
        assert!(
            super::line_numbers::cited(&diagnostic).is_some(),
            "{diagnostic}"
        );
        let subject =
            graph
                .cache()
                .intern_node(HabeasNode::Requirement(crate::sdk::RequirementId(
                    "requirements.rule".into(),
                )));
        let cx = ValidationContext::new(&graph, subject);
        let result = RunNamedQuery
            .execute(
                &cx,
                Query {
                    query: "requirements.rule/checkers".into(),
                },
            )
            .await;
        let message = result["error"].as_str().unwrap();
        assert!(
            message.contains("md/requirements.md#")
                && message
                    .contains("query checkers must contain exactly one fenced yaml code block"),
            "{message}"
        );
        assert!(super::line_numbers::cited(message).is_none());
    }

    // {test}`requirements.references.definition_globs`
    // {test}`requirements.references.authoring_inputs`
    // {test}`requirements.authoring.reference_index`
    // {test}`requirements.authoring.structural_search`
    #[tokio::test]
    // {test}`requirements.authoring.tool_descriptions.common_case`
    // {test}`requirements.authoring.tool_descriptions.special_cases`
    // {test}`requirements.authoring.no_line_numbers`
    // {test}`requirements.sdk.habeas.graph_discovery`
    async fn domain_discovery_returns_readable_nodes_without_recording_their_contents() {
        assert!(
            LookupRequirements
                .description()
                .contains("References are starting points for a wider source search")
        );
        let chapter = format!(
            "{RULE}\n:::{{example}} specific\nAn example. {{cite}}`requirements.rule`\n:::\n"
        );
        let source = format!(
            "{SOURCE}\n// {{impl}}`requirements.rule`\nfn implemented() {{}}\nmod nested {{\n// {{test}}`requirements.rule`\nfn regression() {{}}\n}}\n// :::{{fixme}} requirements.rule\n// This case is unfinished.\n// :::\nfn unfinished() {{}}\n"
        );
        let root = project(&chapter, &source);
        let discovered = HabeasDomain::discover(root.path()).unwrap();
        let domain = &discovered;
        let current =
            domain
                .cache()
                .intern_node(HabeasNode::Requirement(crate::sdk::RequirementId(
                    "requirements.rule".into(),
                )));
        let cx = ValidationContext::new(domain, current);
        let all = LookupRequirements
            .execute(&cx, Lookup { selection: None })
            .await;
        assert_eq!(all.as_array().unwrap().len(), 2);
        let selected = LookupRequirements
            .execute(
                &cx,
                Lookup {
                    selection: Some("requirements.s*".into()),
                },
            )
            .await;
        assert_eq!(selected.as_array().unwrap().len(), 1);
        assert_eq!(
            selected[0]["data"],
            json!({"Requirement":"requirements.specific"})
        );
        assert!(all.as_array().unwrap().contains(&selected[0]));
        let invalid = LookupRequirements
            .execute(
                &cx,
                Lookup {
                    selection: Some("[bad]".into()),
                },
            )
            .await;
        assert!(
            invalid["error"]
                .as_str()
                .unwrap()
                .contains("invalid definition glob")
        );
        let cites = domain
            .outgoing_edges(&current, crate::sdk::HabeasEdgeLabel::Cites)
            .unwrap();
        assert_eq!(
            serde_json::to_value(&cites).unwrap(),
            json!([selected[0]["node"]])
        );
        for (label, selector) in [
            (crate::sdk::HabeasEdgeLabel::Implements, "fn[implemented]"),
            (
                crate::sdk::HabeasEdgeLabel::Tests,
                "mod[nested]/fn[regression]",
            ),
            (crate::sdk::HabeasEdgeLabel::Acknowledges, "fn[unfinished]"),
        ] {
            let targets = domain.outgoing_edges(&current, label).unwrap();
            assert_eq!(targets.len(), 1);
            let target = domain.cache().node_data(targets[0]).unwrap();
            assert_eq!(domain.source_address(&target).unwrap().selector, selector);
        }
        let sites = RunNamedQuery
            .execute(
                &cx,
                Query {
                    query: "requirements.rule/checkers".into(),
                },
            )
            .await;
        for (query, reason) in [
            ("malformed", "expected <requirement>/<query>"),
            ("absent/query", "unknown requirement"),
            ("requirements.rule/nothing", "no query named \"nothing\""),
        ] {
            let result = RunNamedQuery
                .execute(
                    &cx,
                    Query {
                        query: query.into(),
                    },
                )
                .await;
            assert_eq!(result["error"], reason);
        }
        let github = Github(crate::github::GithubIssues::offline());
        let malformed = github
            .execute(
                &cx,
                Issue {
                    issue: "malformed".into(),
                },
            )
            .await;
        assert!(
            malformed["error"]
                .as_str()
                .unwrap()
                .contains("<owner>/<repo>#<number>")
        );
        let unavailable = github
            .execute(
                &cx,
                Issue {
                    issue: "example/repo#42".into(),
                },
            )
            .await;
        assert!(
            unavailable["error"]
                .as_str()
                .unwrap()
                .contains("cannot read GitHub issue example/repo#42")
        );
        let sites = sites.as_array().unwrap();
        assert_eq!(sites.len(), 3);
        for entry in sites.iter().chain(all.as_array().unwrap()) {
            assert_eq!(entry.as_object().unwrap().len(), 2);
            assert!(entry["node"].is_u64() && entry["data"].is_object());
            let text = entry["data"].to_string();
            assert!(
                !regex::Regex::new(r#"\.rs:\d|"(?:line|column|line_start|line_end)"\s*:"#)
                    .unwrap()
                    .is_match(&text)
            );
        }
        assert!(cx.dependencies().is_empty());
        for site in sites {
            let node = serde_json::from_value(site["node"].clone()).unwrap();
            let data = domain.cache().node_data(node).unwrap();
            assert_eq!(serde_json::to_value(&*data).unwrap(), site["data"]);
            assert!(
                domain
                    .source_address(&data)
                    .unwrap()
                    .selector
                    .contains("fn[check_")
            );
            cx.read(&node).unwrap();
        }
        assert_eq!(cx.dependencies().len(), 3);
        let example = serde_json::from_value(selected[0]["node"].clone()).unwrap();
        let contents = cx.read_snapshot(&example).unwrap();
        use trust_but_verify::domain::NodeContents;
        assert!(contents.content.format().contains("An example."));
        assert!(
            cx.dependencies()
                .contains(&trust_but_verify::graph::Dependency::Node(
                    example,
                    contents.fingerprint
                ))
        );
    }
}

//! Source tools share provider operations; search results remain discovery-only.

use super::{ToolDelivery, ToolFactory, ToolSession};
use crate::domain::Domain;
use crate::graph::Dependency;
use crate::resource::OutlineId;
use crate::resource::ResourceId;
use crate::source_engine::ParsedSource;
use crate::source_engine::SourceAnalysis;
use crate::source_engine::SourceProvider;
use crate::sources::SourceScope;
use crate::sources::StructuralQuery;
use crate::sources::TextQuery;
use futures::future::BoxFuture;
use rig_core::completion::ToolDefinition;
use serde_json::{Value, json};
use std::io;

// {cite}`habeas:util`
/// Maps a delivered parse snapshot into the consumer's graph observation.
/// The callback receives the exact bytes parsed, never a replacement read.
pub struct SourceTools<P, F> {
    pub analysis: SourceAnalysis<P>,
    pub observation: F,
}

impl<D, P, F> ToolFactory<D> for SourceTools<P, F>
where
    D: Domain,
    P: SourceProvider,
    P::Error: std::fmt::Display,
    F: Fn(&OutlineId, &ParsedSource) -> Result<Option<Dependency<D::NodeId, D::EdgeLabel>>, String>
        + Send
        + Sync,
{
    // {cite}`habeas:util`
    fn session<'a>(&'a self) -> Box<dyn ToolSession<D> + 'a>
    where
        D::NodeId: 'a,
        D::EdgeLabel: 'a,
    {
        Box::new(SourceSession {
            tools: self,
            pages: Vec::new(),
        })
    }
}

// {cite}`habeas:util`
struct SourceSession<'a, P, F, N, L> {
    tools: &'a SourceTools<P, F>,
    pages: Vec<ToolDelivery<N, L>>,
}

impl<D, P, F> ToolSession<D> for SourceSession<'_, P, F, D::NodeId, D::EdgeLabel>
where
    D: Domain,
    P: SourceProvider,
    P::Error: std::fmt::Display,
    F: Fn(&OutlineId, &ParsedSource) -> Result<Option<Dependency<D::NodeId, D::EdgeLabel>>, String>
        + Send
        + Sync,
{
    // {cite}`habeas:util`
    // {impl}`requirements.sdk.graph.context-tools.read_source.fn`
    // {impl}`requirements.sdk.graph.context-tools.list_sources.fn`
    // {impl}`requirements.sdk.graph.context-tools.outline_source.fn`
    // {impl}`requirements.sdk.graph.context-tools.search_text.fn`
    // {impl}`requirements.sdk.graph.context-tools.search_structure.fn`
    // {impl}`requirements.sdk.graph.context-tools.pages.fn`
    fn definitions(&self) -> io::Result<Vec<ToolDefinition>> {
        let resource = json!({"type":"object","required":["scheme","location"],
            "properties":{"scheme":{"type":"string"},"location":{"type":"string"}}});
        [
            json!({"name":"list_sources","description":"List source resource identities under a prefix. Discovery does not record graph inputs.",
                "parameters":{"type":"object","properties":{"prefix":{"type":"string"}}}}),
            json!({"name":"search_text","description":"Find regular-expression matches under a prefix. Results locate sources; use read_source to read identified contents. limit bounds matches.",
                "parameters":{"type":"object","required":["pattern"],"properties":{"pattern":{"type":"string"},"prefix":{"type":"string"},"context_lines":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":0}}}}),
            json!({"name":"search_structure","description":"Find structural matches using a language and ast-grep rule. Results locate sources; use read_source to read identified contents.",
                "parameters":{"type":"object","required":["language","rule"],"properties":{"language":{"type":"string"},"rule":{"type":"object"},"prefix":{"type":"string"},"limit":{"type":"integer","minimum":0}}}}),
            json!({"name":"outline_source","description":"Parse a resource and inspect its outline selectors. Discovery does not record graph inputs. Use read_source to inspect contents.",
                "parameters":{"type":"object","required":["resource"],"properties":{"resource":resource}}}),
            json!({"name":"read_source","description":"Read an outline node from a resource. selector is an outline selector, or empty for the whole file. Long contents have page IDs.",
                "parameters":{"type":"object","required":["resource"],"properties":{"resource":resource,"selector":{"type":"string"}}}}),
            json!({"name":"get_source_page","description":"Read another page from a previously prepared source result.",
                "parameters":{"type":"object","required":["page"],"properties":{"page":{"type":"integer","minimum":0}}}}),
        ].into_iter().map(|value| serde_json::from_value(value).map_err(io::Error::other)).collect()
    }
    // {impl}`requirements.sdk.graph.context-tools.read_source.behavior`
    // {impl}`requirements.sdk.graph.context-tools.read_source.inputs`
    // {impl}`requirements.sdk.graph.context-tools.list_sources.behavior`
    // {impl}`requirements.sdk.graph.context-tools.list_sources.inputs`
    // {impl}`requirements.sdk.graph.context-tools.outline_source.behavior`
    // {impl}`requirements.sdk.graph.context-tools.outline_source.inputs`
    // {impl}`requirements.sdk.graph.context-tools.search_text.behavior`
    // {impl}`requirements.sdk.graph.context-tools.search_text.inputs`
    // {impl}`requirements.sdk.graph.context-tools.search_structure.behavior`
    // {impl}`requirements.sdk.graph.context-tools.search_structure.inputs`
    // {impl}`requirements.sdk.graph.context-tools.pages.behavior`
    // {impl}`requirements.sdk.graph.context-tools.pages.inputs`
    fn dispatch<'a>(
        &'a mut self,
        name: &'a str,
        arguments: &'a Value,
    ) -> BoxFuture<'a, io::Result<ToolDelivery<D::NodeId, D::EdgeLabel>>> {
        Box::pin(async move {
            let invalid = |error: String| io::Error::other(error);
            let scope = SourceScope {
                prefix: arguments
                    .get("prefix")
                    .map(|value| {
                        value
                            .as_str()
                            .ok_or_else(|| invalid("prefix must be a string".into()))
                    })
                    .transpose()?
                    .unwrap_or("")
                    .into(),
            };
            let limit = arguments
                .get("limit")
                .map(|value| {
                    value
                        .as_u64()
                        .and_then(|n| usize::try_from(n).ok())
                        .ok_or_else(|| invalid("limit must be a nonnegative integer".into()))
                })
                .transpose()?;
            let provider = &self.tools.analysis.provider;
            let mut dependencies = Vec::new();
            let result = match name {
                "list_sources" => json!(
                    provider
                        .list(&scope)
                        .await
                        .map_err(|error| invalid(error.to_string()))?
                ),
                "search_text" => {
                    let pattern = arguments["pattern"]
                        .as_str()
                        .ok_or_else(|| invalid("pattern must be a string".into()))?;
                    let context_lines = arguments
                        .get("context_lines")
                        .map(|value| {
                            value
                                .as_u64()
                                .and_then(|n| usize::try_from(n).ok())
                                .ok_or_else(|| {
                                    invalid("context_lines must be a nonnegative integer".into())
                                })
                        })
                        .transpose()?
                        .unwrap_or(0);
                    let matches = provider
                        .search_text(&TextQuery {
                            scope,
                            pattern: pattern.into(),
                            context_lines,
                            limit,
                        })
                        .await
                        .map_err(|error| invalid(error.to_string()))?;
                    json!({"complete":matches.complete,"matches":matches.matches.into_iter().map(|m| json!({
                        "path":m.path,"line":m.line,"excerpt":m.excerpt
                    })).collect::<Vec<_>>()})
                }
                "search_structure" => {
                    let language = arguments["language"]
                        .as_str()
                        .ok_or_else(|| invalid("language must be a string".into()))?;
                    let rule = arguments["rule"]
                        .as_object()
                        .ok_or_else(|| invalid("rule must be an object".into()))?
                        .clone();
                    let matches = self
                        .tools
                        .analysis
                        .search_structure(&StructuralQuery {
                            scope,
                            language: language.into(),
                            rule,
                            limit,
                        })
                        .await
                        .map_err(|error| invalid(error.to_string()))?;
                    json!({"complete":matches.complete,"matches":matches.matches.into_iter().map(|m| json!({
                        "node":m.node,"excerpt":m.excerpt
                    })).collect::<Vec<_>>()})
                }
                "outline_source" | "read_source" => {
                    let resource: ResourceId =
                        serde_json::from_value(arguments["resource"].clone())
                            .map_err(io::Error::other)?;
                    let parsed = self
                        .tools
                        .analysis
                        .parse(&resource)
                        .await
                        .map_err(|error| invalid(error.to_string()))?;
                    let selector = if name == "read_source" {
                        arguments
                            .get("selector")
                            .map(|value| {
                                value
                                    .as_str()
                                    .ok_or_else(|| invalid("selector must be a string".into()))
                            })
                            .transpose()?
                            .unwrap_or("")
                    } else {
                        ""
                    };
                    let id = OutlineId {
                        resource,
                        selector: selector.into(),
                    };
                    let index = parsed
                        .outline
                        .resolve(selector)
                        .map_err(|error| invalid(error.to_string()))?;
                    if name == "read_source" {
                        if let Some(observation) =
                            (self.tools.observation)(&id, &parsed).map_err(invalid)?
                        {
                            dependencies.push(observation);
                        }
                    }
                    if name == "outline_source" {
                        json!({"resource":id.resource,"nodes":parsed.outline.nodes.iter().map(|node|
                            json!({"selector":node.selector,"kind":node.kind})).collect::<Vec<_>>()})
                    } else {
                        json!({"node":id,"text":parsed.outline.node_lines(parsed.source.text().map_err(|error| invalid(error.to_string()))?,index)})
                    }
                }
                "get_source_page" => {
                    let page = arguments["page"]
                        .as_u64()
                        .and_then(|n| usize::try_from(n).ok())
                        .ok_or_else(|| invalid("page must be an integer".into()))?;
                    let page = self
                        .pages
                        .get(page)
                        .ok_or_else(|| invalid("unknown source page".into()))?;
                    return Ok(ToolDelivery {
                        text: page.text.clone(),
                        dependencies: page.dependencies.clone(),
                    });
                }
                _ => return Err(invalid(format!("unknown source tool {name:?}"))),
            };
            let text = serde_json::to_string(&result).map_err(io::Error::other)?;
            let characters: Vec<char> = text.chars().collect();
            if characters.len() <= 8_192 {
                return Ok(ToolDelivery { text, dependencies });
            }
            let start = self.pages.len();
            for chunk in characters.chunks(8_192) {
                self.pages.push(ToolDelivery {
                    text: chunk.iter().collect(),
                    dependencies: dependencies.clone(),
                });
            }
            let end = self.pages.len();
            Ok(ToolDelivery {
                text:
                    json!({"text":self.pages[start].text,"pages":(start..end).collect::<Vec<_>>()})
                        .to_string(),
                dependencies,
            })
        })
    }
}

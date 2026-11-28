use super::*;
use crate::domain::Domain;
use crate::domain::NodeContents;
use crate::domain::NodeId;
use crate::graph::Dependency;

use crate::procedure::Agent;
use crate::procedure::AgentRequest;
use crate::request::ValidationContext;
use futures::future::BoxFuture;
use serde_json::json;

// {cite}`habeas:util`
/// A result and the exact observations acknowledged when it is delivered.
#[derive(Clone)]
pub struct ToolDelivery<N, L = ()> {
    pub text: String,
    pub input_count: usize,
    pub dependencies: Vec<Dependency<N, L>>,
}

pub trait ToolSession<D: Domain>: Send {
    // {cite}`habeas:util`
    fn definitions(&self) -> io::Result<Vec<ToolDefinition>>;
    fn project(&self) -> Option<&crate::sources::SourceProject<D>> {
        None
    }
    // {cite}`habeas:util`
    fn dispatch<'a>(
        &'a mut self,
        name: &'a str,
        arguments: &'a Value,
        pages: &'a mut Pages<NodeId<D>, D::EdgeLabel>,
    ) -> BoxFuture<'a, io::Result<ToolDelivery<NodeId<D>, D::EdgeLabel>>>;
}

pub(super) struct SessionTools<'a, D: Domain> {
    pub(super) extra: Box<dyn ToolSession<D> + 'a>,
    pages: Pages<NodeId<D>, D::EdgeLabel>,
}

impl<'a, D: Domain> SessionTools<'a, D> {
    pub(super) fn new(extra: Box<dyn ToolSession<D> + 'a>) -> Self {
        Self {
            extra,
            pages: Pages::new(),
        }
    }

    pub(super) fn outstanding_searches(&self) -> Vec<String> {
        self.pages.outstanding()
    }

    // {impl}`requirements.sdk.graph.agent-calls.judge.rendering`
    pub(super) fn read_node(
        &mut self,
        inspection: &ValidationContext<'_, D>,
        arguments: &Value,
    ) -> io::Result<ToolDelivery<NodeId<D>, D::EdgeLabel>>
    where
        D::Error: std::fmt::Display,
        D::NodeData: serde::de::DeserializeOwned,
    {
        let project = self.extra.project();
        let node = node_argument(inspection.graph(), project, arguments)?;
        let mut source = None;
        if let Some(project) = project
            && let Some(address) = project.address(node)
        {
            let item = project.resolve_outline(&address)?;
            let mut children = Vec::new();
            for child in item.children() {
                let id = crate::outline::OutlineData::new(&address.path, child.selector.clone())
                    .map_err(io::Error::other)?;
                let child_node = project.intern_source(&id)?;
                let mut entry =
                    json!({"node":child_node,"file":address.path,"selector":child.selector});
                if let Some(header) = child
                    .header_span
                    .and_then(|span| item.source_span_redacted(span))
                {
                    entry["header"] = json!(header);
                }
                children.push(entry);
            }
            source = Some((
                json!({"file":address.path,"selector":item.id().selector}),
                children,
            ));
        }
        let snapshot = inspection
            .prepare_read(&node)
            .map_err(|error| io::Error::other(error.to_string()))?;
        let text = crate::sources::redact_text(&snapshot.content.format()).into_owned();
        let observation = Dependency::Node(node, snapshot.fingerprint);
        Ok(match source {
            Some((metadata, children)) => {
                self.pages
                    .source_read(metadata, text, children, observation)
            }
            None => self
                .pages
                .prepare(json!({}), "text", Value::String(text), vec![observation]),
        })
    }

    // {impl}`requirements.sdk.graph.validation-context.pages.mcp`
    pub(super) async fn dispatch(
        &mut self,
        name: &str,
        args: &Value,
    ) -> io::Result<ToolDelivery<NodeId<D>, D::EdgeLabel>> {
        if name == "get_next_page" {
            return self.pages.get(args);
        }
        self.extra.dispatch(name, args, &mut self.pages).await
    }
}

/// Each agent call gets fresh tool state, including its own result pages.
pub trait ToolFactory<D: Domain>: Send + Sync {
    // {cite}`habeas:util`
    fn session<'a>(&'a self) -> Box<dyn ToolSession<D> + 'a>
    where
        NodeId<D>: 'a,
        D::EdgeLabel: 'a;
}

// {cite}`habeas:util`
pub struct NoTools;
impl<D: Domain> ToolFactory<D> for NoTools {
    // {cite}`habeas:util`
    fn session<'a>(&'a self) -> Box<dyn ToolSession<D> + 'a>
    where
        NodeId<D>: 'a,
        D::EdgeLabel: 'a,
    {
        Box::new(NoTools)
    }
}
impl<D: Domain> ToolSession<D> for NoTools {
    // {cite}`habeas:util`
    fn definitions(&self) -> io::Result<Vec<ToolDefinition>> {
        Ok(Vec::new())
    }
    // {cite}`habeas:util`
    fn dispatch<'a>(
        &'a mut self,
        name: &'a str,
        _: &'a Value,
        _: &'a mut Pages<NodeId<D>, D::EdgeLabel>,
    ) -> BoxFuture<'a, io::Result<ToolDelivery<NodeId<D>, D::EdgeLabel>>> {
        Box::pin(async move { Err(io::Error::other(format!("unknown tool {name:?}"))) })
    }
}

/// The default typed harness, with tracked graph tools and optional source tools.
pub struct ToolAgent<P, T = NoTools> {
    pub provider: P,
    pub tools: T,
}
impl<P> ToolAgent<P> {
    /// Use this provider with graph tools and no extra application tools.
    pub fn new(provider: P) -> Self {
        Self {
            provider,
            tools: NoTools,
        }
    }
}
impl<P, T> ToolAgent<P, T> {
    /// Replace the extra-tool factory; every call creates a fresh tool session.
    pub fn with_tools<U>(self, tools: U) -> ToolAgent<P, U> {
        ToolAgent {
            provider: self.provider,
            tools,
        }
    }
}

impl<D, P, T> Agent<D> for ToolAgent<P, T>
where
    D: Domain,
    D::NodeData: serde::Serialize + serde::de::DeserializeOwned + schemars::JsonSchema,
    D::EdgeLabel: serde::de::DeserializeOwned + schemars::JsonSchema + PartialEq,
    D::Error: std::fmt::Display,
    P: CompletionProvider,
    T: ToolFactory<D>,
{
    fn run<'a>(
        &'a self,
        request: AgentRequest<'a, NodeId<D>, D::Error>,
        inspection: &'a ValidationContext<'_, D>,
    ) -> BoxFuture<'a, Result<(), super::AgentError<D::Error>>> {
        Box::pin(self.run_with_domain_tools(request, inspection, &[]))
    }
}

impl<P: CompletionProvider, T> ToolAgent<P, T> {
    // {impl}`requirements.sdk.graph.agent-harness.tool_agent`
    pub(super) async fn run_with_domain_tools<D>(
        &self,
        request: AgentRequest<'_, NodeId<D>, D::Error>,
        inspection: &ValidationContext<'_, D>,
        domain_tools: &[std::sync::Arc<dyn crate::agent::RegisteredTool<D>>],
    ) -> Result<(), super::AgentError<D::Error>>
    where
        D: Domain,
        D::NodeData: serde::Serialize + serde::de::DeserializeOwned + schemars::JsonSchema,
        D::EdgeLabel: serde::de::DeserializeOwned + schemars::JsonSchema + PartialEq,
        D::Error: std::fmt::Display,
        T: ToolFactory<D>,
    {
        let submit = request.submission_name.clone();
        let tools = SessionTools::new(self.tools.session());
        let current = serde_json::to_string(&request.current_node).map_err(io::Error::other)?;
        let mut texts = Vec::new();
        if !request.message.is_empty() {
            texts.push(request.message);
        }
        for input in &request.inputs {
            let reference = serde_json::to_string(&input.node).map_err(io::Error::other)?;
            texts.push(format!("{}\nNode: {reference}", input.text));
        }
        let subject = format!(
            "{}\n\nFinish by calling {submit} with {{\"result\": ...}}. Text alone is not a result. You may inspect nodes by domain ID with read_node and outgoing_edges.\nCurrent node: {current}",
            texts.join("\n\n")
        );
        let mut session = GraphSession::new(inspection, domain_tools, tools);
        // The exact reusable prefix is the provider's leading preamble.
        let mut opening = CompletionRequest::new(Message::user(subject));
        if !request.prefix.is_empty() {
            opening = opening.preamble(request.prefix);
        }
        converse(
            inspection,
            &self.provider,
            &mut session,
            opening,
            request.inputs,
            request.result_schema,
            request.finish,
        )
        .await
    }
}

// {cite}`habeas:util`
pub(super) struct GraphSession<'a, 'd, D: Domain> {
    inspection: &'a ValidationContext<'d, D>,
    domain_tools: &'a [std::sync::Arc<dyn crate::agent::RegisteredTool<D>>],
    tools: SessionTools<'a, D>,
    pending: Vec<Dependency<NodeId<D>, D::EdgeLabel>>,
}

impl<'a, 'd, D: Domain> GraphSession<'a, 'd, D> {
    pub(super) fn new(
        inspection: &'a ValidationContext<'d, D>,
        domain_tools: &'a [std::sync::Arc<dyn crate::agent::RegisteredTool<D>>],
        tools: SessionTools<'a, D>,
    ) -> Self {
        Self {
            inspection,
            domain_tools,
            tools,
            pending: Vec::new(),
        }
    }
}

impl<D> Conversation for GraphSession<'_, '_, D>
where
    D: Domain,
    D::NodeData: serde::Serialize + serde::de::DeserializeOwned + schemars::JsonSchema,
    D::EdgeLabel: serde::de::DeserializeOwned + schemars::JsonSchema + PartialEq,
    D::Error: std::fmt::Display,
{
    type Output = Value;
    // {impl}`requirements.sdk.graph.validation-context.outgoing_edges.behavior`
    // {cite}`habeas:util`
    // {impl}`requirements.sdk.graph.validation-context.read_node.mcp.description`
    // {impl}`requirements.sdk.graph.validation-context.outgoing_edges.mcp.description`
    // {impl}`requirements.sdk.graph.validation-context.pages.mcp.description`
    // {impl}`requirements.sdk.graph.agent-harness.submission.mcp.description`
    // {impl}`requirements.sdk.graph.validation-context.pages.mcp`
    fn tools(&self, submission: Option<ToolDefinition>) -> io::Result<Vec<ToolDefinition>> {
        let mut tools: Vec<ToolDefinition> = [
            json!({"name":"read_node","description":"Read a graph node by ID, source address, or node data. Returns text and, when more remains, a next UUID. A delivered page records its enclosing node.",
                "parameters":node_parameters::<NodeId<D>,D::NodeData>(None)}),
            json!({"name":"outgoing_edges","description":"Inspect a known node's outgoing targets for the requested label, making those targets available to read_node.",
                "parameters":node_parameters::<NodeId<D>,D::NodeData>(Some(schemars::SchemaGenerator::subschema_for::<D::EdgeLabel>))}),
            json!({"name":"get_next_page","description":"Pass the UUID from next to read the next page in the same format. When next is absent, this is the last page.",
                "parameters":{"type":"object","required":["uuid"],"properties":{"uuid":{"type":"string","format":"uuid"}}}}),
        ].into_iter().map(|tool| serde_json::from_value(tool).map_err(io::Error::other)).collect::<io::Result<_>>()?;
        tools.extend(submission);
        tools.extend(self.tools.extra.definitions()?);
        if self.domain_tools.is_empty() {
            return Ok(tools);
        }
        let mut names: std::collections::HashSet<&str> =
            tools.iter().map(|tool| tool.name.as_str()).collect();
        names.extend(project_sources::TOOL_NAMES.iter().copied());
        for tool in self.domain_tools {
            if !names.insert(tool.name()) {
                return Err(io::Error::other(format!(
                    "duplicate agent tool name: {}",
                    tool.name()
                )));
            }
        }
        for tool in self.domain_tools {
            tools.push(tool.definition()?);
        }
        Ok(tools)
    }
    // {cite}`habeas:util`
    fn submit(&mut self, _: &str, arguments: &Value) -> io::Result<Submission<Value>> {
        let outstanding = self.tools.outstanding_searches();
        if !outstanding.is_empty() {
            return Ok(Submission::Retry(format!(
                "Call finish_search for every open search_id (use used:[] if none contributed), then resubmit: {}",
                outstanding.join(", ")
            )));
        }
        Ok(Submission::Accepted(arguments.clone()))
    }
    // {impl}`requirements.sdk.graph.agent-harness.fixed_tools`
    // {cite}`habeas:util`
    async fn dispatch(&mut self, name: &str, arguments: &Value) -> io::Result<(String, usize)> {
        match name {
            "read_node" => {
                let delivery = self.tools.read_node(self.inspection, arguments)?;
                self.pending.extend(delivery.dependencies);
                Ok((delivery.text, delivery.input_count))
            }
            "outgoing_edges" => {
                let node = node_argument(
                    self.inspection.graph(),
                    self.tools.extra.project(),
                    arguments,
                )?;
                let label: D::EdgeLabel =
                    serde_json::from_value(arguments["label"].clone()).map_err(io::Error::other)?;
                let snapshot = self
                    .inspection
                    .prepare_edges(&node, label.clone())
                    .map_err(|error| io::Error::other(error.to_string()))?;
                let targets = serde_json::to_value(snapshot.content)?;
                self.pending
                    .push(Dependency::OutgoingEdges(node, label, snapshot.fingerprint));
                Ok((json!({"targets":targets}).to_string(), 1))
            }
            _ => {
                if let Some(tool) = self.domain_tools.iter().find(|tool| tool.name() == name) {
                    return tool
                        .call(self.inspection, arguments)
                        .await
                        .map(|text| (text, 0));
                }
                let delivery = self.tools.dispatch(name, arguments).await?;
                self.pending.extend(delivery.dependencies);
                Ok((delivery.text, delivery.input_count))
            }
        }
    }
    fn delivered(&mut self) -> io::Result<()> {
        for observation in self.pending.drain(..) {
            match observation {
                Dependency::Node(node, fingerprint) => {
                    self.inspection.record_read(node, fingerprint)
                }
                Dependency::OutgoingEdges(node, label, fingerprint) => {
                    self.inspection.observe_edges(node, label, fingerprint)
                }
            }
        }
        Ok(())
    }
    // {cite}`habeas:util`
    fn cut_off(&mut self, response: &CompletionResponse, limit: u64) -> Vec<Message> {
        let results = response
            .tool_calls()
            .map(|call| tool_error(call, "not run: the call was cut off at the output limit"))
            .collect();
        let mut messages: Vec<Message> = response.message().into_iter().collect();
        messages.push(Message::tool_results(results));
        messages.push(Message::user(format!("The response was cut off at {limit} tokens. Continue with a shorter response and finish with submit_result.")));
        messages
    }
}

// {impl}`requirements.sdk.graph.agent-harness.submission.mcp`
// {impl}`requirements.sdk.graph.agent-harness.submission.mcp.description`
pub(super) fn submission_tool(mut schema: Value) -> ToolDefinition {
    let definitions = schema
        .as_object_mut()
        .and_then(|object| object.remove("$defs"));
    let mut parameters = json!({"type":"object", "additionalProperties":false,
                "required":["result"], "properties":{"result":schema}});
    if let Some(definitions) = definitions {
        parameters["$defs"] = definitions;
    }
    ToolDefinition::new(
        rig_core::message::ToolName::new("submit_result").expect("valid built-in tool name"),
        "Finish by submitting your structured result in result. Exactly one accepted submission ends this call.",
        parameters,
    )
}

pub(super) fn submitted_value(arguments: &Value) -> io::Result<Value> {
    let object = arguments
        .as_object()
        .filter(|object| object.len() == 1 && object.contains_key("result"))
        .ok_or_else(|| {
            io::Error::other(
                "Resubmit with exactly one field named `result` containing the answer.",
            )
        })?;
    Ok(object["result"].clone())
}

#[cfg(test)]
mod submission_tests {
    use super::*;

    // {test}`requirements.sdk.graph.agent-harness.submission.invalid`
    #[test]
    fn submission_envelopes_require_one_result_field() {
        for invalid in [
            Value::Null,
            json!([]),
            json!("answer"),
            json!(true),
            json!({}),
            json!({"other": true}),
            json!({"result": true, "extra": false}),
        ] {
            assert_eq!(
                submitted_value(&invalid).unwrap_err().to_string(),
                "Resubmit with exactly one field named `result` containing the answer."
            );
        }
        for result in [Value::Null, json!([]), json!({"nested": 1}), json!(false)] {
            assert_eq!(submitted_value(&json!({"result": result})).unwrap(), result);
        }
    }
}

// {impl}`requirements.sdk.graph.validation-context.node_reference.consistency`
// {impl}`requirements.sdk.graph.validation-context.node_reference`
pub(super) fn node_argument<D: Domain>(
    domain: &D,
    project: Option<&crate::sources::SourceProject<D>>,
    args: &Value,
) -> io::Result<NodeId<D>>
where
    D::NodeData: serde::de::DeserializeOwned,
{
    if let Some(path) = args.get("path") {
        if args.get("node").is_some() || args.get("data").is_some() {
            return Err(io::Error::other(
                "path cannot be combined with node or data",
            ));
        }
        let path = path
            .as_str()
            .ok_or_else(|| io::Error::other("path must be a string"))?;
        let location = args
            .get("location")
            .map(|v| {
                v.as_str()
                    .ok_or_else(|| io::Error::other("location must be a selector string"))
            })
            .transpose()?
            .unwrap_or("");
        let project =
            project.ok_or_else(|| io::Error::other("project sources are not configured"))?;
        let id = crate::outline::OutlineData::new(path, location.into())
            .map_err(|error| io::Error::other(format!("{path}#{location}: {}", error.message)))?;
        let item = project.resolve_outline(&id)?;
        return Ok(project.intern_source(item.id())?);
    }
    if args.get("location").is_some() {
        return Err(io::Error::other("location requires path"));
    }
    let data = args
        .get("data")
        .cloned()
        .map(serde_json::from_value::<D::NodeData>)
        .transpose()?;
    if let Some(value) = args.get("node") {
        let id = serde_json::from_value(value.clone())
            .map_err(|err| io::Error::other(format!("invalid node ID: {err}")))?;
        let stored = domain
            .cache()
            .node_data(id)
            .ok_or_else(|| io::Error::other("unassigned node ID"))?;
        if data.as_ref().is_some_and(|given| given != stored.as_ref()) {
            return Err(io::Error::other(
                "node ID and data identify different nodes",
            ));
        }
        return Ok(id);
    }
    data.map(|value| domain.cache().intern_node(value))
        .ok_or_else(|| io::Error::other("expected node, data, or path"))
}

// {impl}`requirements.sdk.graph.validation-context.node_reference`
pub(super) fn node_parameters<N: schemars::JsonSchema, Data: schemars::JsonSchema>(
    label_schema: Option<fn(&mut schemars::SchemaGenerator) -> schemars::Schema>,
) -> Value {
    let mut generator = schemars::SchemaGenerator::default();
    let mut parameters = json!({
        "type":"object",
        "if":{"required":["path"]},
        "then":{"not":{"anyOf":[{"required":["node"]},{"required":["data"]}]}},
        "else":{"anyOf":[{"required":["node"]},{"required":["data"]}],"not":{"required":["location"]}},
        "properties":{"node":generator.subschema_for::<N>(),"data":generator.subschema_for::<Data>(),"path":{"type":"string"},"location":{"type":"string"}}
    });
    if let Some(label) = label_schema {
        parameters["required"] = json!(["label"]);
        parameters["properties"]["label"] = json!(label(&mut generator));
    }
    let definitions = generator.take_definitions(true);
    if !definitions.is_empty() {
        parameters["$defs"] = Value::Object(definitions);
    }
    parameters
}

#[cfg(test)]
mod tests {
    use super::*;

    // {test}`requirements.sdk.graph-edges.outgoing_edges`
    // {test}`requirements.sdk.graph.validation-context.outgoing_edges.mcp`
    // {test}`requirements.sdk.graph.validation-context.outgoing_edges.behavior`
    #[test]
    fn nested_label_schema_references_resolve_from_the_tool_parameters_root() {
        #[derive(schemars::JsonSchema)]
        struct Label {
            metadata: Metadata,
        }
        #[derive(schemars::JsonSchema)]
        struct Metadata {
            kind: String,
        }
        let label = Label {
            metadata: Metadata {
                kind: "tests".into(),
            },
        };
        assert_eq!(label.metadata.kind, "tests");
        let parameters =
            node_parameters::<u32, String>(Some(schemars::SchemaGenerator::subschema_for::<Label>));
        let label = parameters
            .pointer(
                parameters["properties"]["label"]["$ref"]
                    .as_str()
                    .unwrap()
                    .strip_prefix('#')
                    .unwrap(),
            )
            .unwrap();
        let reference = label["properties"]["metadata"]["$ref"].as_str().unwrap();
        let definition = parameters
            .pointer(reference.strip_prefix('#').unwrap())
            .unwrap();
        assert_eq!(definition["properties"]["kind"]["type"], "string");
        assert!(parameters["properties"]["label"].get("$defs").is_none());
        assert_eq!(parameters["required"], json!(["label"]));
        let unit =
            node_parameters::<u32, String>(Some(schemars::SchemaGenerator::subschema_for::<()>));
        assert_eq!(unit["properties"]["label"]["type"], "null");
        assert!(unit.get("$defs").is_none());
    }
}

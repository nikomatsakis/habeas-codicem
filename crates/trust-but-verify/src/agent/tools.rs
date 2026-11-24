use super::*;
use crate::domain::Domain;
use crate::domain::NodeContents;
use crate::graph::Dependency;
use crate::graph::NodePath;
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
    pub dependencies: Vec<Dependency<N, L>>,
}

pub trait ToolSession<D: Domain>: Send {
    // {cite}`habeas:util`
    fn definitions(&self) -> io::Result<Vec<ToolDefinition>>;
    // {cite}`habeas:util`
    fn dispatch<'a>(
        &'a mut self,
        name: &'a str,
        arguments: &'a Value,
        pages: &'a mut Pages<D::NodeId, D::EdgeLabel>,
    ) -> BoxFuture<'a, io::Result<ToolDelivery<D::NodeId, D::EdgeLabel>>>;
}

pub(super) struct SessionTools<'a, D: Domain> {
    pub(super) extra: Box<dyn ToolSession<D> + 'a>,
    pages: Pages<D::NodeId, D::EdgeLabel>,
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

    pub(super) async fn dispatch(
        &mut self,
        name: &str,
        args: &Value,
    ) -> io::Result<ToolDelivery<D::NodeId, D::EdgeLabel>> {
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
        D::NodeId: 'a,
        D::EdgeLabel: 'a;
}

// {cite}`habeas:util`
pub struct NoTools;
impl<D: Domain> ToolFactory<D> for NoTools {
    // {cite}`habeas:util`
    fn session<'a>(&'a self) -> Box<dyn ToolSession<D> + 'a>
    where
        D::NodeId: 'a,
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
        _: &'a mut Pages<D::NodeId, D::EdgeLabel>,
    ) -> BoxFuture<'a, io::Result<ToolDelivery<D::NodeId, D::EdgeLabel>>> {
        Box::pin(async move { Err(io::Error::other(format!("unknown tool {name:?}"))) })
    }
}

// {impl}`requirements.sdk.graph.agent-harness.tool_agent`
/// The default typed harness, with tracked graph tools and optional source tools.
pub struct ToolAgent<P, T = NoTools> {
    pub provider: P,
    pub tools: T,
}
impl<P> ToolAgent<P> {
    // {impl}`requirements.sdk.graph.agent-harness.tool_agent`
    /// Use this provider with graph tools and no extra application tools.
    pub fn new(provider: P) -> Self {
        Self {
            provider,
            tools: NoTools,
        }
    }
}
impl<P, T> ToolAgent<P, T> {
    // {impl}`requirements.sdk.graph.agent-harness.tool_agent`
    /// Replace the extra-tool factory; every call creates a fresh tool session.
    pub fn with_tools<U>(self, tools: U) -> ToolAgent<P, U> {
        ToolAgent {
            provider: self.provider,
            tools,
        }
    }
}

// {impl}`requirements.sdk.graph.agent-calls.judge.submission`
// {impl}`requirements.sdk.graph.agent-calls.judge.caching`
// {impl}`requirements.sdk.graph.agent-calls.author_procedure`
// {impl}`requirements.sdk.graph.agent-harness.tool_agent`
impl<D, P, T> Agent<D> for ToolAgent<P, T>
where
    D: Domain,
    D::EdgeLabel: serde::de::DeserializeOwned + schemars::JsonSchema + PartialEq,
    D::Error: std::fmt::Display,
    P: CompletionProvider,
    T: ToolFactory<D>,
{
    // {impl}`requirements.sdk.graph.validation-context.shared_context`
    // {impl}`requirements.sdk.graph.agent-harness.submission`
    fn run<'a>(
        &'a self,
        request: AgentRequest<D::NodeId>,
        inspection: &'a ValidationContext<'_, D>,
    ) -> BoxFuture<'a, Result<Value, std::io::Error>> {
        Box::pin(async move {
            let submit = request.submission_name.clone();
            let mut known = vec![request.current_node];
            let mut pending = Vec::new();
            let mut texts = Vec::new();
            for input in request.inputs {
                if !known.contains(&input.node) {
                    known.push(input.node.clone());
                }
                texts.push(input.text);
                pending.push(Dependency::Node(input.node, input.fingerprint));
            }
            let subject = format!(
                "{}\n\nFinish by calling {submit} with {{\"result\": ...}}. Text alone is not a result. \
                You may inspect known nodes with read_node and outgoing_edges, using their path segments.\n\
                Current node: {}",
                texts.join("\n\n"),
                serde_json::to_string(&inspection.graph().node_path(&known[0])).expect("node path"),
            );
            let mut schema = request.result_schema;
            let definitions = schema
                .as_object_mut()
                .and_then(|object| object.remove("$defs"));
            let mut parameters = json!({"type":"object", "additionalProperties":false,
                "required":["result"], "properties":{"result":schema}});
            if let Some(definitions) = definitions {
                parameters["$defs"] = definitions;
            }
            let mut session = GraphSession {
                inspection,
                tools: SessionTools::new(self.tools.session()),
                known,
                pending,
                parameters,
            };
            // The exact reusable prefix is the provider's leading preamble.
            let mut opening = CompletionRequest::new(Message::user(subject));
            if !request.prefix.is_empty() {
                opening = opening.preamble(request.prefix);
            }
            converse(&self.provider, &mut session, opening, &submit).await
        })
    }
}

// {cite}`habeas:util`
// {impl}`requirements.sdk.graph.validation-context.shared_context`
struct GraphSession<'a, 'd, D: Domain> {
    inspection: &'a ValidationContext<'d, D>,
    tools: SessionTools<'a, D>,
    known: Vec<D::NodeId>,
    pending: Vec<Dependency<D::NodeId, D::EdgeLabel>>,
    parameters: Value,
}

impl<D> GraphSession<'_, '_, D>
where
    D: Domain,
    D::EdgeLabel: serde::de::DeserializeOwned + schemars::JsonSchema + PartialEq,
    D::Error: std::fmt::Display,
{
    // {cite}`habeas:util`
    fn node(&self, arguments: &Value) -> io::Result<D::NodeId> {
        let path: NodePath =
            serde_json::from_value(arguments["path"].clone()).map_err(io::Error::other)?;
        self.known
            .iter()
            .find(|node| self.inspection.graph().node_path(node) == path)
            .cloned()
            .ok_or_else(|| {
                io::Error::other("unknown node path; inspect outgoing_edges of a known node first")
            })
    }
}

impl<D> Conversation for GraphSession<'_, '_, D>
where
    D: Domain,
    D::EdgeLabel: serde::de::DeserializeOwned + schemars::JsonSchema + PartialEq,
    D::Error: std::fmt::Display,
{
    type Output = Value;
    // {cite}`habeas:util`
    // {impl}`requirements.sdk.graph.validation-context.read_node.mcp`
    // {impl}`requirements.sdk.graph.validation-context.read_node.mcp.description`
    // {impl}`requirements.sdk.graph.validation-context.outgoing_edges.mcp`
    // {impl}`requirements.sdk.graph.validation-context.outgoing_edges.mcp.description`
    // {impl}`requirements.sdk.graph.validation-context.pages.mcp`
    // {impl}`requirements.sdk.graph.validation-context.pages.mcp.description`
    // {impl}`requirements.sdk.graph.agent-harness.submission.mcp`
    // {impl}`requirements.sdk.graph.agent-harness.submission.mcp.description`
    // {impl}`requirements.sdk.graph.agent-harness.fixed_tools`
    fn tools(&self) -> io::Result<Vec<ToolDefinition>> {
        let path = json!({"type":"object","required":["segments"],"properties":{"segments":{"type":"array","items":{"type":"string"}}}});
        let mut tools: Vec<ToolDefinition> = [
            json!({"name":"read_node","description":"Read a known graph node. Returns text and, when more remains, a next UUID. A delivered page records its enclosing node.",
                "parameters":{"type":"object","required":["path"],"properties":{"path":path}}}),
            json!({"name":"outgoing_edges","description":"Inspect a known node's outgoing targets for the requested label, making those targets available to read_node.",
                "parameters":edge_parameters::<D::EdgeLabel>(&path)}),
            json!({"name":"get_next_page","description":"Pass the UUID from next to read the next page in the same format. When next is absent, this is the last page.",
                "parameters":{"type":"object","required":["uuid"],"properties":{"uuid":{"type":"string","format":"uuid"}}}}),
            json!({"name":"submit_result","description":"Finish by submitting your structured result in result. Exactly one accepted submission ends this call.",
                "parameters":self.parameters}),
        ].into_iter().map(|tool| serde_json::from_value(tool).map_err(io::Error::other)).collect::<io::Result<_>>()?;
        tools.extend(self.tools.extra.definitions()?);
        Ok(tools)
    }
    // {cite}`habeas:util`
    // {impl}`requirements.sdk.graph.agent-harness.submission.invalid`
    // {impl}`requirements.sdk.graph.search.finish_search.behavior`
    fn submit(&mut self, _: &str, arguments: &Value) -> io::Result<Submission<Value>> {
        let outstanding = self.tools.outstanding_searches();
        if !outstanding.is_empty() {
            return Ok(Submission::Retry(format!(
                "Call finish_search for every open search_id (use used:[] if none contributed), then resubmit: {}",
                outstanding.join(", ")
            )));
        }
        submitted_value(arguments).map(Submission::Accepted)
    }
    // {cite}`habeas:util`
    // {impl}`requirements.sdk.graph.agent-calls.judge.rendering`
    // {impl}`requirements.sdk.graph.validation-context.shared_context`
    // {impl}`requirements.sdk.graph.validation-context.read_node.behavior`
    // {impl}`requirements.sdk.graph.validation-context.read_node.inputs`
    // {impl}`requirements.sdk.graph.validation-context.outgoing_edges.behavior`
    // {impl}`requirements.sdk.graph.validation-context.outgoing_edges.inputs`
    // {impl}`requirements.sdk.graph.validation-context.pages.behavior`
    // {impl}`requirements.sdk.graph.validation-context.pages.inputs`
    async fn dispatch(&mut self, name: &str, arguments: &Value) -> io::Result<String> {
        match name {
            "read_node" => {
                let node = self.node(arguments)?;
                let snapshot = self
                    .inspection
                    .prepare_read(&node)
                    .map_err(|error| io::Error::other(error.to_string()))?;
                let text = crate::redaction::redact_text(&snapshot.content.format()).into_owned();
                let observation = Dependency::Node(node, snapshot.fingerprint);
                let delivery = self.tools.pages.prepare(
                    json!({}),
                    "text",
                    Value::String(text),
                    vec![observation],
                );
                self.pending.extend(delivery.dependencies);
                Ok(delivery.text)
            }
            "outgoing_edges" => {
                let node = self.node(arguments)?;
                let label: D::EdgeLabel =
                    serde_json::from_value(arguments["label"].clone()).map_err(io::Error::other)?;
                let snapshot = self
                    .inspection
                    .prepare_edges(&node, label.clone())
                    .map_err(|error| io::Error::other(error.to_string()))?;
                self.pending
                    .push(Dependency::OutgoingEdges(node, label, snapshot.fingerprint));
                let mut targets = Vec::new();
                for target in snapshot.content {
                    targets.push(self.inspection.graph().node_path(&target));
                    if !self.known.contains(&target) {
                        self.known.push(target);
                    }
                }
                Ok(json!({"targets":targets}).to_string())
            }
            _ => {
                let delivery = self.tools.dispatch(name, arguments).await?;
                self.pending.extend(delivery.dependencies);
                Ok(delivery.text)
            }
        }
    }
    // {impl}`requirements.sdk.graph.validation-context.shared_context`
    // {impl}`requirements.sdk.graph.validation-context.read_node.inputs`
    // {impl}`requirements.sdk.graph.validation-context.outgoing_edges.inputs`
    // {impl}`requirements.sdk.graph.validation-context.pages.inputs`
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
            .map(|call| {
                call.error_result(vec![ToolResultContent::text(
                    "not run: the call was cut off at the output limit",
                )])
            })
            .collect();
        let mut messages: Vec<Message> = response.message().into_iter().collect();
        messages.push(Message::tool_results(results));
        messages.push(Message::user(format!("The response was cut off at {limit} tokens. Continue with a shorter response and finish with submit_result.")));
        messages
    }
}

// {impl}`requirements.sdk.graph.agent-harness.submission.invalid`
fn submitted_value(arguments: &Value) -> io::Result<Value> {
    let object = arguments
        .as_object()
        .filter(|object| object.len() == 1 && object.contains_key("result"))
        .ok_or_else(|| io::Error::other("submit_result requires exactly result"))?;
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
                "submit_result requires exactly result"
            );
        }
        for result in [Value::Null, json!([]), json!({"nested": 1}), json!(false)] {
            assert_eq!(submitted_value(&json!({"result": result})).unwrap(), result);
        }
    }
}

// {cite}`habeas:util`
fn edge_parameters<L: schemars::JsonSchema>(path: &Value) -> Value {
    let mut label = serde_json::to_value(schemars::schema_for!(L)).expect("label schema");
    let definitions = label
        .as_object_mut()
        .and_then(|object| object.remove("$defs"));
    let mut parameters = json!({
        "type":"object",
        "required":["path","label"],
        "properties":{"path":path,"label":label},
    });
    if let Some(definitions) = definitions {
        parameters["$defs"] = definitions;
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
        let parameters = edge_parameters::<Label>(&json!({"type":"object"}));
        let reference = parameters["properties"]["label"]["properties"]["metadata"]["$ref"]
            .as_str()
            .unwrap();
        let definition = parameters
            .pointer(reference.strip_prefix('#').unwrap())
            .unwrap();
        assert_eq!(definition["properties"]["kind"]["type"], "string");
        assert!(parameters["properties"]["label"].get("$defs").is_none());
        assert_eq!(parameters["required"], json!(["path", "label"]));
        let unit = edge_parameters::<()>(&json!({"type":"object"}));
        assert_eq!(unit["properties"]["label"]["type"], "null");
        assert!(unit.get("$defs").is_none());
    }
}

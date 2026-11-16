use super::*;
use crate::{
    graph::{Dependency, Fingerprinting, Graph, InspectionContext, NodePath},
    procedure::{Agent, AgentInput, AgentInputRenderer, AgentRequest},
};
use futures::future::BoxFuture;
use serde_json::json;

/// A result and the exact observations acknowledged when it is delivered.
pub struct ToolDelivery<N> {
    pub text: String,
    pub dependencies: Vec<Dependency<N>>,
}

pub trait ToolSession<D: Graph>: Send {
    fn definitions(&self) -> io::Result<Vec<ToolDefinition>>;
    fn dispatch<'a>(
        &'a mut self,
        name: &'a str,
        arguments: &'a Value,
    ) -> BoxFuture<'a, io::Result<ToolDelivery<D::NodeId>>>;
}

/// Each agent call gets fresh tool state, including its own result pages.
pub trait ToolFactory<D: Graph>: Send + Sync {
    fn session<'a>(&'a self) -> Box<dyn ToolSession<D> + 'a>
    where
        D::NodeId: 'a;
}

pub struct NoTools;
impl<D: Graph> ToolFactory<D> for NoTools {
    fn session<'a>(&'a self) -> Box<dyn ToolSession<D> + 'a>
    where
        D::NodeId: 'a,
    {
        Box::new(NoTools)
    }
}
impl<D: Graph> ToolSession<D> for NoTools {
    fn definitions(&self) -> io::Result<Vec<ToolDefinition>> {
        Ok(Vec::new())
    }
    fn dispatch<'a>(
        &'a mut self,
        name: &'a str,
        _: &'a Value,
    ) -> BoxFuture<'a, io::Result<ToolDelivery<D::NodeId>>> {
        Box::pin(async move { Err(io::Error::other(format!("unknown tool {name:?}"))) })
    }
}

/// The default typed harness, with tracked graph tools and optional source tools.
pub struct ToolAgent<P, R, T = NoTools> {
    pub provider: P,
    pub renderer: R,
    pub tools: T,
}
impl<P, R> ToolAgent<P, R> {
    pub fn new(provider: P, renderer: R) -> Self {
        Self {
            provider,
            renderer,
            tools: NoTools,
        }
    }
}
impl<P, R, T> ToolAgent<P, R, T> {
    pub fn with_tools<U>(self, tools: U) -> ToolAgent<P, R, U> {
        ToolAgent {
            provider: self.provider,
            renderer: self.renderer,
            tools,
        }
    }
}

// {impl}`requirements.sdk.graph.agent-calls.judge.submission`
// {impl}`requirements.sdk.graph.agent-calls.judge.caching`
// {impl}`requirements.sdk.graph.agent-calls.author_procedure`
impl<D, P, R, T> Agent<D> for ToolAgent<P, R, T>
where
    D: Graph + Fingerprinting,
    D::Error: std::fmt::Display,
    P: CompletionProvider,
    R: AgentInputRenderer<D>,
    T: ToolFactory<D>,
{
    fn run<'a>(
        &'a self,
        request: AgentRequest<D::NodeId>,
        inspection: &'a InspectionContext<'_, D>,
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
                renderer: &self.renderer,
                tools: self.tools.session(),
                known,
                pending,
                parameters,
                pages: Vec::new(),
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

struct GraphSession<'a, 'd, D: Graph + Fingerprinting, R> {
    inspection: &'a InspectionContext<'d, D>,
    renderer: &'a R,
    tools: Box<dyn ToolSession<D> + 'a>,
    known: Vec<D::NodeId>,
    pending: Vec<Dependency<D::NodeId>>,
    parameters: Value,
    pages: Vec<(String, Dependency<D::NodeId>)>,
}

impl<D, R> GraphSession<'_, '_, D, R>
where
    D: Graph + Fingerprinting,
    D::Error: std::fmt::Display,
    R: AgentInputRenderer<D>,
{
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
    fn deliver_page(&mut self, page: usize) -> io::Result<String> {
        let (text, dependency) = self
            .pages
            .get(page)
            .ok_or_else(|| io::Error::other("unknown page"))?;
        self.pending.push(dependency.clone());
        Ok(json!({"page":page,"text":text}).to_string())
    }
}

impl<D, R> Conversation for GraphSession<'_, '_, D, R>
where
    D: Graph + Fingerprinting,
    D::Error: std::fmt::Display,
    R: AgentInputRenderer<D>,
{
    type Output = Value;
    fn tools(&self) -> io::Result<Vec<ToolDefinition>> {
        let path = json!({"type":"object","required":["segments"],"properties":{"segments":{"type":"array","items":{"type":"string"}}}});
        let mut tools: Vec<ToolDefinition> = [
            json!({"name":"read_node","description":"Read a known graph node. Returns text and page IDs when long. A delivered page records its enclosing node.",
                "parameters":{"type":"object","required":["path"],"properties":{"path":path}}}),
            json!({"name":"outgoing_edges","description":"Inspect a known node's complete ordered outgoing targets, making those targets available to read_node.",
                "parameters":{"type":"object","required":["path"],"properties":{"path":path}}}),
            json!({"name":"get_node_page","description":"Read another prepared node page by its page ID.",
                "parameters":{"type":"object","required":["page"],"properties":{"page":{"type":"integer","minimum":0}}}}),
            json!({"name":"submit_result","description":"Finish by submitting your structured result in result. Exactly one accepted submission ends this call.",
                "parameters":self.parameters}),
        ].into_iter().map(|tool| serde_json::from_value(tool).map_err(io::Error::other)).collect::<io::Result<_>>()?;
        tools.extend(self.tools.definitions()?);
        Ok(tools)
    }
    fn submit(&mut self, _: &str, arguments: &Value) -> io::Result<Submission<Value>> {
        let object = arguments
            .as_object()
            .filter(|object| object.len() == 1 && object.contains_key("result"))
            .ok_or_else(|| io::Error::other("submit_result requires exactly result"))?;
        Ok(Submission::Accepted(object["result"].clone()))
    }
    async fn dispatch(&mut self, name: &str, arguments: &Value) -> io::Result<String> {
        match name {
            "read_node" => {
                let node = self.node(arguments)?;
                let snapshot = self
                    .inspection
                    .prepare_read(&node)
                    .map_err(|error| io::Error::other(error.to_string()))?;
                let text = self
                    .renderer
                    .render(&AgentInput {
                        node: &node,
                        contents: &snapshot.content,
                    })
                    .map_err(|error| io::Error::other(error.to_string()))?;
                let observation = Dependency::Node(node, snapshot.fingerprint);
                // Split on character boundaries; every page retains the same snapshot.
                let chars: Vec<char> = text.chars().collect();
                let start = self.pages.len();
                for chunk in chars.chunks(8_192) {
                    self.pages
                        .push((chunk.iter().collect(), observation.clone()));
                }
                if chars.is_empty() {
                    self.pages.push((String::new(), observation));
                }
                let end = self.pages.len();
                let first = self.deliver_page(start)?;
                Ok(json!({"first":serde_json::from_str::<Value>(&first).expect("page"),"pages":(start..end).collect::<Vec<_>>()}).to_string())
            }
            "get_node_page" => {
                let page = arguments["page"]
                    .as_u64()
                    .and_then(|page| usize::try_from(page).ok())
                    .ok_or_else(|| io::Error::other("page must be a nonnegative page ID"))?;
                self.deliver_page(page)
            }
            "outgoing_edges" => {
                let node = self.node(arguments)?;
                let snapshot = self
                    .inspection
                    .graph()
                    .outgoing_edges(&node)
                    .map_err(|error| io::Error::other(error.to_string()))?;
                self.pending
                    .push(Dependency::OutgoingEdges(node, snapshot.fingerprint));
                let mut targets = Vec::new();
                for edge in snapshot.content {
                    targets.push(self.inspection.graph().node_path(&edge.target));
                    if !self.known.contains(&edge.target) {
                        self.known.push(edge.target);
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
    fn delivered(&mut self) -> io::Result<()> {
        for observation in self.pending.drain(..) {
            match observation {
                Dependency::Node(node, fingerprint) => {
                    self.inspection.observe_read(node, fingerprint)
                }
                Dependency::OutgoingEdges(node, fingerprint) => {
                    self.inspection.observe_edges(node, fingerprint)
                }
            }
        }
        Ok(())
    }
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

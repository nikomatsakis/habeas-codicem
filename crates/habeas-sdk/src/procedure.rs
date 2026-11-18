//! Typed agent calls with reusable context followed by explicit node inputs.

use crate::graph::*;
use futures::future::BoxFuture;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{borrow::Cow, future::IntoFuture, marker::PhantomData};

// {impl}`requirements.sdk.graph.agent-calls.agent_context`
pub trait Context {
    // {impl}`requirements.sdk.graph.agent-calls.agent_context`
    fn contents(&self) -> Cow<'_, String>;
}

impl<C: Context + ?Sized> Context for &C {
    // {impl}`requirements.sdk.graph.agent-calls.agent_context`
    fn contents(&self) -> Cow<'_, String> {
        (**self).contents()
    }
}
impl Context for String {
    // {impl}`requirements.sdk.graph.agent-calls.agent_context`
    fn contents(&self) -> Cow<'_, String> {
        Cow::Borrowed(self)
    }
}
impl<C: Context> Context for [C] {
    // {impl}`requirements.sdk.graph.agent-calls.agent_context`
    fn contents(&self) -> Cow<'_, String> {
        Cow::Owned(
            self.iter()
                .map(|c| c.contents().into_owned())
                .collect::<Vec<_>>()
                .join("\n\n"),
        )
    }
}
impl<C: Context> Context for Vec<C> {
    // {impl}`requirements.sdk.graph.agent-calls.agent_context`
    fn contents(&self) -> Cow<'_, String> {
        self.as_slice().contents()
    }
}
macro_rules! tuple_context {
    ($($type:ident:$index:tt),+) => {
        impl<$($type: Context),+> Context for ($($type,)+) {
            // {impl}`requirements.sdk.graph.agent-calls.agent_context`
            fn contents(&self) -> Cow<'_, String> {
                Cow::Owned(vec![$(self.$index.contents().into_owned()),+].join("\n\n"))
            }
        }
    };
}
tuple_context!(A:0, B:1);
tuple_context!(A:0, B:1, C:2);
tuple_context!(A:0, B:1, C:2, D:3);
tuple_context!(A:0, B:1, C:2, D:3, E:4);
tuple_context!(A:0, B:1, C:2, D:3, E:4, F:5);
tuple_context!(A:0, B:1, C:2, D:3, E:4, F:5, G:6);
tuple_context!(A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7);
tuple_context!(A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8);
tuple_context!(A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8, J:9);
tuple_context!(A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8, J:9, K:10);
tuple_context!(A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8, J:9, K:10, L:11);

// {impl}`requirements.sdk.graph.agent-calls.judge.rendering`
pub struct AgentInput<'i, D: Graph> {
    pub node: &'i D::NodeId,
    pub contents: &'i D::NodeContent,
}

pub trait AgentInputRenderer<D: Graph>: Send + Sync {
    // {impl}`requirements.sdk.graph.agent-calls.judge.rendering`
    fn render(&self, input: &AgentInput<'_, D>) -> Result<String, D::Error>;
}

// {cite}`habeas:util`
pub struct PreparedInput<N> {
    pub node: N,
    pub text: String,
    pub fingerprint: Fingerprint,
}

// {cite}`habeas:util`
/// A harness acknowledges delivered snapshots through `inspection.observe_read`,
/// including snapshots delivered before a later conversation error.
pub struct AgentRequest<N> {
    pub current_node: N,
    pub prefix: String,
    pub prefix_fingerprint: Fingerprint,
    pub inputs: Vec<PreparedInput<N>>,
    pub result_schema: Value,
    pub submission_name: String,
}

/// Returns one accepted structured tool submission; assistant text is an error.
pub trait Agent<D: Graph + Fingerprinting>: Send + Sync {
    // {cite}`habeas:util`
    fn run<'a>(
        &'a self,
        request: AgentRequest<D::NodeId>,
        inspection: &'a InspectionContext<'_, D>,
    ) -> BoxFuture<'a, Result<Value, std::io::Error>>;
}

// {impl}`requirements.sdk.graph.agent-calls.author_procedure`
pub struct ValidationContext<'d, D: Graph + Fingerprinting> {
    node: D::NodeId,
    inspection: InspectionContext<'d, D>,
    agent: Option<&'d dyn Agent<D>>,
    renderer: Option<&'d dyn AgentInputRenderer<D>>,
}

impl<'d, D: Graph + Fingerprinting> ValidationContext<'d, D> {
    // {cite}`habeas:util`
    pub fn new(graph: &'d D, node: D::NodeId) -> Self {
        Self::configured(graph, node, None, None)
    }

    // {cite}`habeas:util`
    pub fn with_agent(
        graph: &'d D,
        node: D::NodeId,
        agent: &'d dyn Agent<D>,
        renderer: &'d dyn AgentInputRenderer<D>,
    ) -> Self {
        Self::configured(graph, node, Some(agent), Some(renderer))
    }

    // {cite}`habeas:util`
    pub(crate) fn configured(
        graph: &'d D,
        node: D::NodeId,
        agent: Option<&'d dyn Agent<D>>,
        renderer: Option<&'d dyn AgentInputRenderer<D>>,
    ) -> Self {
        Self {
            node,
            inspection: InspectionContext::new(graph),
            agent,
            renderer,
        }
    }

    // {impl}`requirements.sdk.graph.agent-calls.author_procedure`
    pub fn inspection(&self) -> &InspectionContext<'d, D> {
        &self.inspection
    }

    // {impl}`requirements.sdk.graph.validation.root_inputs`
    pub fn node_id(&self) -> &D::NodeId {
        &self.node
    }

    // {impl}`requirements.sdk.graph.validation.root_inputs`
    pub fn successors(&self, label: &D::EdgeLabel) -> Result<Vec<D::NodeId>, D::Error>
    where
        D::EdgeLabel: PartialEq,
    {
        Ok(self
            .inspection
            .outgoing_edges(&self.node, label.clone())?
            .content)
    }
}

impl<'d, D: Validation> ValidationContext<'d, D> {
    pub fn ask<R>(&self) -> Ask<'_, 'd, D, R, ContextPhase>
    where
        R: DeserializeOwned + JsonSchema + Send,
    {
        Ask {
            ctx: self,
            contexts: Vec::new(),
            insertions: Vec::new(),
            state: PhantomData,
        }
    }
}

// {impl}`requirements.sdk.graph.agent-calls.judge.phases`
pub struct ContextPhase;
/// The reusable prefix is sealed after the first node insertion.
///
/// ```compile_fail
/// use habeas_sdk::{graph::Validation, procedure::ValidationContext};
/// fn too_late<D: Validation>(ctx: &ValidationContext<'_, D>) {
///     ctx.ask::<bool>().current_node_contents().context(String::new());
/// }
/// ```
///
/// ```compile_fail
/// use habeas_sdk::{graph::Validation, procedure::ValidationContext};
/// fn too_late<D: Validation>(ctx: &ValidationContext<'_, D>, label: D::EdgeLabel)
/// where D::EdgeLabel: PartialEq {
///     ctx.ask::<bool>().successors_content(label).context(String::new());
/// }
/// ```
// {test}`requirements.sdk.graph.agent-calls.judge.phases`
pub struct NodePhase;

// {cite}`habeas:util`
enum Insertion<L> {
    Current,
    Successors(L, fn(&L, &L) -> bool),
}

// {impl}`requirements.sdk.graph.agent-calls.judge`
pub struct Ask<'a, 'd, D: Validation, R, P> {
    ctx: &'a ValidationContext<'d, D>,
    contexts: Vec<String>,
    insertions: Vec<Insertion<D::EdgeLabel>>,
    state: PhantomData<fn() -> (R, P)>,
}

impl<'a, 'd, D: Validation, R> Ask<'a, 'd, D, R, ContextPhase> {
    // {impl}`requirements.sdk.graph.agent-calls.agent_context`
    pub fn context(mut self, value: impl Context) -> Self {
        self.contexts.push(value.contents().into_owned());
        self
    }
}

impl<'a, 'd, D: Validation, R, P> Ask<'a, 'd, D, R, P> {
    // {cite}`habeas:util`
    fn into_node_phase(self) -> Ask<'a, 'd, D, R, NodePhase> {
        Ask {
            ctx: self.ctx,
            contexts: self.contexts,
            insertions: self.insertions,
            state: PhantomData,
        }
    }

    // {impl}`requirements.sdk.graph.agent-calls.judge.phases`
    pub fn current_node_contents(mut self) -> Ask<'a, 'd, D, R, NodePhase> {
        self.insertions.push(Insertion::Current);
        self.into_node_phase()
    }

    pub fn successors_content(mut self, label: D::EdgeLabel) -> Ask<'a, 'd, D, R, NodePhase>
    where
        D::EdgeLabel: PartialEq,
    {
        self.insertions
            .push(Insertion::Successors(label, |a, b| a == b));
        self.into_node_phase()
    }
}

impl<'a, 'd, D: Validation, R, P> Ask<'a, 'd, D, R, P>
where
    R: DeserializeOwned + JsonSchema + Send + 'a,
    P: 'a,
    'd: 'a,
{
    // {impl}`requirements.sdk.graph.agent-calls.judge.node_contents`
    // {impl}`requirements.sdk.graph.agent-calls.model_capability`
    async fn execute(self) -> Result<R, ValidationError<D::Error>> {
        let agent = self.ctx.agent.ok_or(ValidationError::NoAgentConfigured)?;
        let renderer = self
            .ctx
            .renderer
            .ok_or(ValidationError::NoAgentConfigured)?;
        let mut inputs = Vec::new();
        for insertion in self.insertions {
            let nodes = match insertion {
                Insertion::Current => vec![self.ctx.node.clone()],
                Insertion::Successors(label, matches) => {
                    let snapshot = self
                        .ctx
                        .inspection
                        .prepare_edges(&self.ctx.node, label.clone())
                        .map_err(ValidationError::Graph)?;
                    self.ctx.inspection.observe_edges_by(
                        self.ctx.node.clone(),
                        label,
                        snapshot.fingerprint,
                        matches,
                    );
                    snapshot.content
                }
            };
            for node in nodes {
                let snapshot = self
                    .ctx
                    .inspection
                    .prepare_read(&node)
                    .map_err(ValidationError::Graph)?;
                let text = renderer
                    .render(&AgentInput {
                        node: &node,
                        contents: &snapshot.content,
                    })
                    .map_err(ValidationError::Graph)?;
                inputs.push(PreparedInput {
                    node,
                    text,
                    fingerprint: snapshot.fingerprint,
                });
            }
        }
        let prefix = self.contexts.join("\n\n");
        let prefix_fingerprint = context_fingerprint(&prefix);
        let request = AgentRequest {
            current_node: self.ctx.node.clone(),
            prefix,
            prefix_fingerprint,
            inputs,
            result_schema: serde_json::to_value(schemars::schema_for!(R))
                .map_err(|error| ValidationError::Agent(error.to_string()))?,
            submission_name: "submit_result".into(),
        };
        let value = agent
            .run(request, &self.ctx.inspection)
            .await
            .map_err(|error| ValidationError::Agent(error.to_string()))?;
        serde_json::from_value(value).map_err(|error| ValidationError::Agent(error.to_string()))
    }
}

macro_rules! await_phase {
    ($phase:ty) => {
        impl<'a, 'd, D: Validation, R> IntoFuture for Ask<'a, 'd, D, R, $phase>
        where
            R: DeserializeOwned + JsonSchema + Send + 'a,
            'd: 'a,
        {
            type Output = Result<R, ValidationError<D::Error>>;
            type IntoFuture = BoxFuture<'a, Self::Output>;
            fn into_future(self) -> Self::IntoFuture {
                Box::pin(self.execute())
            }
        }
    };
}
await_phase!(ContextPhase);
await_phase!(NodePhase);

// {impl}`requirements.sdk.graph.agent-calls.agent_context`
pub fn context_fingerprint(prefix: &str) -> Fingerprint {
    Fingerprint {
        scheme: "context-prefix-sha256-v1".into(),
        value: Sha256::digest(prefix.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    }
}

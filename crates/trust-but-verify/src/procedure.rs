//! Typed agent calls with reusable context followed by explicit node inputs.

use crate::domain::Domain;
use crate::domain::NodeContents;
use crate::domain::NodeId;
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
impl Context for str {
    // {impl}`requirements.sdk.graph.agent-calls.agent_context`
    fn contents(&self) -> Cow<'_, String> {
        Cow::Owned(self.to_owned())
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

// {cite}`habeas:util`
#[doc(hidden)]
pub struct PreparedInput<N> {
    pub node: N,
    pub text: String,
    pub fingerprint: Fingerprint,
}

// {cite}`habeas:util`
/// A harness acknowledges delivered snapshots through `inspection.observe_read`,
/// including snapshots delivered before a later conversation error.
#[doc(hidden)]
pub struct AgentRequest<N> {
    pub current_node: N,
    pub prefix: String,
    pub prefix_fingerprint: Fingerprint,
    pub inputs: Vec<PreparedInput<N>>,
    pub result_schema: Value,
    pub submission_name: String,
}

/// Returns one accepted structured tool submission; assistant text is an error.
#[doc(hidden)]
pub trait Agent<D: Domain>: Send + Sync {
    // {cite}`habeas:util`
    fn run<'a>(
        &'a self,
        request: AgentRequest<NodeId<D>>,
        inspection: &'a ValidationContext<'_, D>,
    ) -> BoxFuture<'a, Result<Value, std::io::Error>>;
}

// {impl}`requirements.sdk.graph.agent-calls.author_procedure`
pub struct ValidationContext<'d, D: Domain> {
    node: NodeId<D>,
    graph: &'d D,
    // Hold this lock only while reading or recording dependencies.
    observations: std::sync::Mutex<Vec<Dependency<NodeId<D>, D::EdgeLabel>>>,
    agent: Option<&'d dyn Agent<D>>,
}

impl<'d, D: Domain> ValidationContext<'d, D> {
    // {cite}`habeas:util`
    // {impl}`requirements.sdk.graph.validation-context.ValidationContext.new`
    pub fn new(graph: &'d D, node: NodeId<D>) -> Self {
        Self::configured(graph, node, None)
    }

    // {cite}`habeas:util`
    #[doc(hidden)]
    #[cfg(any(test, feature = "habeas-integration"))]
    pub(crate) fn with_agent(graph: &'d D, node: NodeId<D>, agent: &'d dyn Agent<D>) -> Self {
        Self::configured(graph, node, Some(agent))
    }

    // {cite}`habeas:util`
    // {impl}`requirements.sdk.graph.validation-context.shared_context`
    pub(crate) fn configured(
        graph: &'d D,
        node: NodeId<D>,
        agent: Option<&'d dyn Agent<D>>,
    ) -> Self {
        Self {
            node,
            graph,
            observations: Default::default(),
            agent,
        }
    }

    // {impl}`requirements.sdk.graph.validation-context.root_inputs`
    /// The source node this criterion examines.
    // {impl}`requirements.sdk.graph.validation-context.ValidationContext.node_id`
    pub fn node_id(&self) -> &NodeId<D> {
        &self.node
    }

    // {impl}`requirements.sdk.graph.validation-context.root_inputs`
    /// Select this source's successors and record their target-set fingerprint.
    // {impl}`requirements.sdk.graph.validation-context.ValidationContext.successors`
    pub fn successors(&self, label: &D::EdgeLabel) -> Result<Vec<NodeId<D>>, D::Error>
    where
        D::EdgeLabel: PartialEq,
    {
        Ok(self.outgoing_edges(&self.node, label.clone())?.content)
    }
}

// {impl}`requirements.sdk.graph.validation-context.tracked_inspection`
impl<'d, D: Domain> ValidationContext<'d, D> {
    // {cite}`habeas:util`
    // {impl}`requirements.sdk.graph.validation-context.ValidationContext.graph`
    pub fn graph(&self) -> &'d D {
        self.graph
    }

    // {cite}`habeas:util`
    // {impl}`requirements.sdk.graph.validation-context.read_node.behavior`
    // {impl}`requirements.sdk.graph.validation-context.read_node.fn`
    // {impl}`requirements.sdk.graph.validation-context.read_node.inputs`
    // {impl}`requirements.sdk.graph.validation-context.ValidationContext.read`
    // {impl}`requirements.sdk.graph.validation-context.shared_context`
    pub fn read(&self, node: &NodeId<D>) -> Result<D::NodeContent, D::Error> {
        Ok(self.read_snapshot(node)?.content)
    }

    // {impl}`requirements.sdk.graph.evidence.node_inputs`
    // {impl}`requirements.sdk.graph.snapshot`
    // {impl}`requirements.sdk.graph.validation-context.ValidationContext.read_snapshot`
    pub fn read_snapshot(&self, node: &NodeId<D>) -> Result<Snapshot<D::NodeContent>, D::Error> {
        let snapshot = self.prepare_read(node)?;
        self.record_read(*node, snapshot.fingerprint.clone());
        Ok(snapshot)
    }

    /// Prepare a retained node observation without claiming its delivery.
    pub(crate) fn prepare_read(
        &self,
        node: &NodeId<D>,
    ) -> Result<Snapshot<D::NodeContent>, D::Error> {
        let content = self.graph.node_content(node)?;
        let fingerprint = content.fingerprint();
        Ok(Snapshot {
            content,
            fingerprint,
        })
    }

    // {impl}`requirements.sdk.graph.validation-context.outgoing_edges.behavior`
    // {impl}`requirements.sdk.graph.validation-context.outgoing_edges.fn`
    // {impl}`requirements.sdk.graph.validation-context.outgoing_edges.inputs`
    // {impl}`requirements.sdk.graph.validation-context.snapshot`
    // {impl}`requirements.sdk.graph.validation-context.ValidationContext.outgoing_edges`
    pub fn outgoing_edges(
        &self,
        node: &NodeId<D>,
        label: D::EdgeLabel,
    ) -> Result<Snapshot<Vec<NodeId<D>>>, D::Error>
    where
        D::EdgeLabel: PartialEq,
    {
        let snapshot = self.prepare_edges(node, label.clone())?;
        self.observe_edges(*node, label, snapshot.fingerprint.clone());
        Ok(snapshot)
    }

    /// Prepare a relationship result without claiming delivery.
    // {cite}`habeas:util`
    pub(crate) fn prepare_edges(
        &self,
        node: &NodeId<D>,
        label: D::EdgeLabel,
    ) -> Result<Snapshot<Vec<NodeId<D>>>, D::Error> {
        let content = self.graph.outgoing_edges(node, label)?;
        let fingerprint = edge_fingerprint(self.graph, &content);
        Ok(Snapshot {
            content,
            fingerprint,
        })
    }

    /// Import the fingerprint of the exact snapshot delivered to the caller.
    // {impl}`requirements.sdk.graph.validation-context.shared_context`
    pub(crate) fn record_read(&self, node: NodeId<D>, fingerprint: Fingerprint) {
        let mut observations = self.observations.lock().expect("dependency trace poisoned");
        if !observations.iter().any(|dependency| {
            matches!(dependency,
            Dependency::Node(previous, hash) if previous == &node && hash == &fingerprint)
        }) {
            observations.push(Dependency::Node(node, fingerprint));
        }
    }

    /// Habeas adapter hook for a delivered, retained observation.
    #[doc(hidden)]
    #[cfg(feature = "habeas-integration")]
    pub fn observe_read(&self, node: NodeId<D>, fingerprint: Fingerprint) {
        self.record_read(node, fingerprint);
    }

    // {cite}`habeas:util`
    pub(crate) fn observe_edges(
        &self,
        node: NodeId<D>,
        label: D::EdgeLabel,
        fingerprint: Fingerprint,
    ) where
        D::EdgeLabel: PartialEq,
    {
        self.observe_edges_by(node, label, fingerprint, |a, b| a == b);
    }

    // {cite}`habeas:util`
    pub(crate) fn observe_edges_by(
        &self,
        node: NodeId<D>,
        label: D::EdgeLabel,
        fingerprint: Fingerprint,
        matches: fn(&D::EdgeLabel, &D::EdgeLabel) -> bool,
    ) {
        let mut observations = self.observations.lock().expect("dependency trace poisoned");
        if !observations.iter().any(|dependency| {
            matches!(dependency,
            Dependency::OutgoingEdges(previous, previous_label, hash)
            if previous == &node && matches(previous_label, &label) && hash == &fingerprint)
        }) {
            observations.push(Dependency::OutgoingEdges(node, label, fingerprint));
        }
    }

    // {cite}`habeas:util`
    // {impl}`requirements.sdk.graph.validation-context.ValidationContext.dependencies`
    pub fn dependencies(&self) -> Vec<Dependency<NodeId<D>, D::EdgeLabel>> {
        self.observations
            .lock()
            .expect("dependency trace poisoned")
            .clone()
    }

    // {impl}`requirements.sdk.graph.evidence.record`
    // {impl}`requirements.sdk.graph.validation-context.ValidationContext.finish`
    pub fn finish(
        &self,
        version: CriterionVersion,
        output: impl serde::Serialize,
    ) -> Result<Record<D::NodeData, D::EdgeLabel>, serde_json::Error> {
        Ok(Record {
            version: Some(version),
            dependencies: crate::graph::persist_dependencies(self.graph, &self.dependencies()),
            output: serde_json::to_value(output)?,
        })
    }
}

impl<D: Domain> ValidationContext<'_, D>
where
    D::Error: From<crate::sources::SourceError>,
{
    // {impl}`requirements.sdk.graph.validation-context.read_source.fn`
    // {impl}`requirements.sdk.sources.provider_observations`
    // {impl}`requirements.sdk.graph.validation-context.ValidationContext.read_source`
    pub fn read_source(
        &self,
        item: &crate::outline::OutlineData,
    ) -> Result<D::NodeContent, D::Error> {
        let node = self.graph.source_node(item)?;
        self.read(&self.graph.cache().intern_source(node, item))
    }
}

impl<'d, D: Domain> ValidationContext<'d, D> {
    /// Start a request whose submitted answer is decoded into R.
    // {impl}`requirements.sdk.graph.validation-context.ValidationContext.ask`
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
/// use trust_but_verify::{domain::Domain, request::ValidationContext};
/// fn too_late<D: Domain>(ctx: &ValidationContext<'_, D>) {
///     ctx.ask::<bool>().contents_of_current_node().context(String::new());
/// }
/// ```
///
/// ```compile_fail
/// use trust_but_verify::{domain::Domain, request::ValidationContext};
/// fn too_late<D: Domain>(ctx: &ValidationContext<'_, D>, label: D::EdgeLabel)
/// where D::EdgeLabel: PartialEq {
///     ctx.ask::<bool>().contents_of_successors(label).context(String::new());
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
pub struct Ask<'a, 'd, D: Domain, R, P> {
    ctx: &'a ValidationContext<'d, D>,
    contexts: Vec<String>,
    insertions: Vec<Insertion<D::EdgeLabel>>,
    state: PhantomData<fn() -> (R, P)>,
}

impl<'a, 'd, D: Domain, R> Ask<'a, 'd, D, R, ContextPhase> {
    // {impl}`requirements.sdk.graph.agent-calls.agent_context`
    pub fn context(mut self, value: impl Context) -> Self {
        self.contexts.push(value.contents().into_owned());
        self
    }
}

impl<'a, 'd, D: Domain, R, P> Ask<'a, 'd, D, R, P> {
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
    /// Queue the current node's contents and enter the node phase.
    pub fn contents_of_current_node(mut self) -> Ask<'a, 'd, D, R, NodePhase> {
        self.insertions.push(Insertion::Current);
        self.into_node_phase()
    }

    /// Queue contents of successors with this label and enter the node phase.
    pub fn contents_of_successors(mut self, label: D::EdgeLabel) -> Ask<'a, 'd, D, R, NodePhase>
    where
        D::EdgeLabel: PartialEq,
    {
        self.insertions
            .push(Insertion::Successors(label, |a, b| a == b));
        self.into_node_phase()
    }
}

impl<'a, 'd, D: Domain, R, P> Ask<'a, 'd, D, R, P>
where
    R: DeserializeOwned + JsonSchema + Send + 'a,
    P: 'a,
    D::Error: std::fmt::Display,
    D::EdgeLabel: DeserializeOwned + JsonSchema + PartialEq,
    'd: 'a,
{
    // {impl}`requirements.sdk.graph.agent-calls.judge.node_contents`
    // {impl}`requirements.sdk.graph.agent-calls.judge.rendering`
    // {impl}`requirements.sdk.graph.agent-calls.model_capability`
    // {impl}`requirements.sdk.graph.validation-context.shared_context`
    async fn execute(self) -> Result<R, ValidationError<D::Error>> {
        let agent = self
            .ctx
            .agent
            .or_else(|| {
                self.ctx
                    .graph
                    .cache()
                    .agent
                    .as_deref()
                    .map(|agent| agent.runner.as_ref() as &dyn Agent<D>)
            })
            .ok_or(ValidationError::NoAgentConfigured)?;
        let mut inputs = Vec::new();
        for insertion in self.insertions {
            let nodes = match insertion {
                Insertion::Current => vec![self.ctx.node],
                Insertion::Successors(label, matches) => {
                    let snapshot = self
                        .ctx
                        .prepare_edges(&self.ctx.node, label.clone())
                        .map_err(ValidationError::Graph)?;
                    self.ctx
                        .observe_edges_by(self.ctx.node, label, snapshot.fingerprint, matches);
                    snapshot.content
                }
            };
            for node in nodes {
                let snapshot = self
                    .ctx
                    .prepare_read(&node)
                    .map_err(ValidationError::Graph)?;
                let formatted = snapshot.content.format();
                let text = crate::redaction::Redaction::of_text(&formatted)
                    .apply(&formatted)
                    .into_owned();
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
            current_node: self.ctx.node,
            prefix,
            prefix_fingerprint,
            inputs,
            result_schema: serde_json::to_value(schemars::schema_for!(R))
                .map_err(|error| ValidationError::Agent(error.to_string()))?,
            submission_name: "submit_result".into(),
        };
        let value = agent
            .run(request, self.ctx)
            .await
            .map_err(|error| ValidationError::Agent(error.to_string()))?;
        serde_json::from_value(value).map_err(|error| ValidationError::Agent(error.to_string()))
    }
}

macro_rules! await_phase {
    ($phase:ty) => {
        impl<'a, 'd, D: Domain, R> IntoFuture for Ask<'a, 'd, D, R, $phase>
        where
            R: DeserializeOwned + JsonSchema + Send + 'a,
            'd: 'a,
            D::Error: std::fmt::Display,
            D::EdgeLabel: DeserializeOwned + JsonSchema + PartialEq,
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
#[doc(hidden)]
pub fn context_fingerprint(prefix: &str) -> Fingerprint {
    Fingerprint {
        scheme: "context-prefix-sha256-v1".into(),
        value: Sha256::digest(prefix.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    }
}

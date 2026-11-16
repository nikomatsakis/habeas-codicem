use super::*;
use crate::procedure::{Agent, AgentInputRenderer};

#[derive(Debug)]
pub enum EvidenceState<M> {
    Current(Judgment<M>),
    Missing,
    Stale,
}

/// Model-free checking with an application-supplied review-version resolver.
pub struct Checker<'d, D, F> {
    pub(crate) graph: &'d D,
    pub(crate) procedure: F,
}

impl<'d, D, F> Checker<'d, D, F>
where
    D: Graph + Fingerprinting + EvidenceStorage,
    F: Fn(&D::NodeId) -> Result<String, D::Error> + Send + Sync,
{
    // {impl}`requirements.sdk.graph.validation.procedure_identity`
    pub fn new(graph: &'d D, procedure: F) -> Self {
        Self { graph, procedure }
    }

    // {impl}`requirements.sdk.graph.validation.check_node`
    pub async fn check_node(
        &self,
        node: &D::NodeId,
    ) -> Result<EvidenceState<D::JudgmentMetadata>, D::Error> {
        let procedure = (self.procedure)(node)?;
        self.check_with_version(node, &procedure).await
    }

    pub(crate) async fn check_with_version(
        &self,
        node: &D::NodeId,
        procedure: &str,
    ) -> Result<EvidenceState<D::JudgmentMetadata>, D::Error> {
        let Some(record) = self.graph.load_evidence(node).await? else {
            return Ok(EvidenceState::Missing);
        };
        if record_is_current(self.graph, procedure, &record)? {
            Ok(EvidenceState::Current(record.output))
        } else {
            Ok(EvidenceState::Stale)
        }
    }
}

pub struct Validator<'d, D: Validation, F> {
    pub(crate) checker: Checker<'d, D, F>,
    agent: Option<&'d dyn Agent<D>>,
    renderer: Option<&'d dyn AgentInputRenderer<D>>,
}

impl<'d, D, F> Validator<'d, D, F>
where
    D: Validation + EvidenceStorage,
    F: Fn(&D::NodeId) -> Result<String, D::Error> + Send + Sync,
{
    pub fn new(graph: &'d D, procedure: F) -> Self {
        Self {
            checker: Checker::new(graph, procedure),
            agent: None,
            renderer: None,
        }
    }

    pub fn with_agent(
        mut self,
        agent: &'d dyn Agent<D>,
        renderer: &'d dyn AgentInputRenderer<D>,
    ) -> Self {
        self.agent = Some(agent);
        self.renderer = Some(renderer);
        self
    }

    pub async fn check_node(
        &self,
        node: &D::NodeId,
    ) -> Result<EvidenceState<D::JudgmentMetadata>, D::Error> {
        self.checker.check_node(node).await
    }

    // {impl}`requirements.sdk.graph.validation.validate_node`
    // {impl}`requirements.sdk.graph.validation.validate_node.errors`
    pub async fn validate_node(
        &self,
        node: &D::NodeId,
    ) -> Result<Judgment<D::JudgmentMetadata>, ValidationError<D::Error>> {
        self.validate_with_edges(node, None).await
    }

    pub(crate) async fn validate_with_edges(
        &self,
        node: &D::NodeId,
        edges: Option<Snapshot<Vec<Edge<D::NodeId, D::EdgeLabel>>>>,
    ) -> Result<Judgment<D::JudgmentMetadata>, ValidationError<D::Error>> {
        let graph = self.checker.graph;
        let procedure = (self.checker.procedure)(node).map_err(ValidationError::Graph)?;
        if let EvidenceState::Current(judgment) = self
            .checker
            .check_with_version(node, &procedure)
            .await
            .map_err(ValidationError::Graph)?
        {
            return Ok(judgment);
        }
        let ctx =
            ValidationContext::configured(graph, node.clone(), self.agent, self.renderer, edges);
        let output = graph.validate(&ctx).await?;
        let record = ctx.inspection().finish(procedure, output);
        graph
            .store_evidence(node, &record)
            .await
            .map_err(ValidationError::Graph)?;
        Ok(record.output)
    }
}

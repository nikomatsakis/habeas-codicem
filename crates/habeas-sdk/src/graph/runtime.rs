use super::*;
use crate::procedure::{Agent, AgentInputRenderer};
use std::sync::Arc;

// {impl}`requirements.sdk.graph.validation.check_node`
#[derive(Debug)]
pub enum EvidenceState<O> {
    Current(O),
    Missing,
    Stale,
}

/// Shared domain and agent access. Each execution owns its dependency trace.
// {impl}`requirements.sdk.graph.validation.validator`
pub struct Validator<D: Domain> {
    domain: D,
    agent: Option<Arc<dyn Agent<D>>>,
    renderer: Option<Arc<dyn AgentInputRenderer<D>>>,
}

impl<D: Domain> Validator<D> {
    // {impl}`requirements.sdk.graph.validation.validator`
    /// Own the domain used to check and execute criteria.
    pub fn new(domain: D) -> Self {
        Self {
            domain,
            agent: None,
            renderer: None,
        }
    }

    // {impl}`requirements.sdk.graph.validation.validator`
    /// Share this agent and input renderer with execution contexts.
    pub fn with_agent(
        mut self,
        agent: Arc<dyn Agent<D>>,
        renderer: Arc<dyn AgentInputRenderer<D>>,
    ) -> Self {
        self.agent = Some(agent);
        self.renderer = Some(renderer);
        self
    }

    // {impl}`requirements.sdk.graph.validation.check_node`
    // {impl}`requirements.sdk.graph.validation.procedure_identity`
    /// Inspect this criterion's saved answer and compare its revision and inputs.
    pub fn check<V: Criterion<D>>(
        &self,
        criterion: &V,
    ) -> impl Future<Output = Result<EvidenceState<V::Ok>, ValidationError<D::Error>>> + Send {
        // Read the descriptor before constructing the future: V needs no Sync.
        let node = criterion.source_node_id();
        let criterion_path = criterion.criterion_path();
        let revision = criterion.revision();
        async move {
            let Some(record) = self
                .domain
                .load_evidence(&node, &criterion_path)
                .await
                .map_err(ValidationError::Graph)?
            else {
                return Ok(EvidenceState::Missing);
            };
            if !record_is_current(&self.domain, &revision, &record)
                .map_err(ValidationError::Graph)?
            {
                return Ok(EvidenceState::Stale);
            }
            serde_json::from_value(record.output)
                .map(EvidenceState::Current)
                .map_err(ValidationError::Output)
        }
    }

    // {impl}`requirements.sdk.graph.validation.validate_node`
    // {impl}`requirements.sdk.graph.validation.validate_node.errors`
    // {impl}`requirements.sdk.graph.evidence.record.output`
    /// Reuse the current answer, or execute the criterion and save its result.
    pub fn verify<V: Criterion<D>>(
        &self,
        criterion: V,
    ) -> impl Future<Output = Result<V::Ok, ValidationError<D::Error>>> + Send {
        async move {
            if let EvidenceState::Current(answer) = self.check(&criterion).await? {
                return Ok(answer);
            }
            let node = criterion.source_node_id();
            let criterion_path = criterion.criterion_path();
            let revision = criterion.revision();
            let mut ctx = ValidationContext::configured(
                &self.domain,
                node.clone(),
                self.agent.as_deref(),
                self.renderer.as_deref(),
            );
            let answer = criterion.execute(&mut ctx).await?;
            let record = ctx
                .inspection()
                .finish(revision, &answer)
                .map_err(ValidationError::Output)?;
            self.domain
                .store_evidence(&node, &criterion_path, &record)
                .await
                .map_err(ValidationError::Graph)?;
            Ok(answer)
        }
    }
}

use super::*;
use crate::procedure::{Agent, AgentInputRenderer};

// {impl}`requirements.sdk.graph.validation.check_node`
#[derive(Debug)]
pub enum EvidenceState<O> {
    Current(O),
    Missing,
    Stale,
}

/// Shared domain and agent access. Each execution owns its dependency trace.
// {impl}`requirements.sdk.graph.validation.validator`
pub struct Validator<'d, D: Domain> {
    domain: &'d D,
    agent: Option<&'d dyn Agent<D>>,
    renderer: Option<&'d dyn AgentInputRenderer<D>>,
}

impl<'d, D: Domain> Validator<'d, D> {
    // {impl}`requirements.sdk.graph.validation.validator`
    pub fn new(domain: &'d D) -> Self {
        Self {
            domain,
            agent: None,
            renderer: None,
        }
    }

    // {impl}`requirements.sdk.graph.validation.validator`
    pub fn with_agent(
        mut self,
        agent: &'d dyn Agent<D>,
        renderer: &'d dyn AgentInputRenderer<D>,
    ) -> Self {
        self.agent = Some(agent);
        self.renderer = Some(renderer);
        self
    }

    // {impl}`requirements.sdk.graph.validation.check_node`
    // {impl}`requirements.sdk.graph.validation.procedure_identity`
    pub fn check<V: Validation<D>>(
        &self,
        validation: &V,
    ) -> impl Future<Output = Result<EvidenceState<V::Ok>, ValidationError<D::Error>>> + Send {
        // Read the descriptor before constructing the future: V needs no Sync.
        let node = validation.source_node_id();
        let criterion = validation.criterion_path();
        let revision = validation.revision();
        async move {
            let Some(record) = self
                .domain
                .load_evidence(&node, &criterion)
                .await
                .map_err(ValidationError::Graph)?
            else {
                return Ok(EvidenceState::Missing);
            };
            if !record_is_current(self.domain, &revision, &record)
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
    pub fn verify<V: Validation<D>>(
        &self,
        validation: V,
    ) -> impl Future<Output = Result<V::Ok, ValidationError<D::Error>>> + Send {
        async move {
            if let EvidenceState::Current(answer) = self.check(&validation).await? {
                return Ok(answer);
            }
            let node = validation.source_node_id();
            let criterion = validation.criterion_path();
            let revision = validation.revision();
            let mut ctx =
                ValidationContext::configured(self.domain, node.clone(), self.agent, self.renderer);
            let answer = validation.execute(&mut ctx).await?;
            let record = ctx
                .inspection()
                .finish(revision, &answer)
                .map_err(ValidationError::Output)?;
            self.domain
                .store_evidence(&node, &criterion, &record)
                .await
                .map_err(ValidationError::Graph)?;
            Ok(answer)
        }
    }
}

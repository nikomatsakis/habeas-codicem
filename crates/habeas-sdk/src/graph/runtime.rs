use super::*;
use crate::procedure::Agent;
use std::sync::Arc;

/// Shared domain and agent access. Each execution owns its dependency trace.
// {impl}`requirements.sdk.graph.validation.validator`
pub struct Validator<D: Domain> {
    domain: D,
    agent: Option<Arc<dyn Agent<D>>>,
}

impl<D: Domain> Validator<D> {
    // {impl}`requirements.sdk.graph.validation.validator`
    /// Own the domain used to check and execute criteria.
    pub fn new(domain: D) -> Self {
        Self {
            domain,
            agent: None,
        }
    }

    // {impl}`requirements.sdk.graph.validation.validator`
    /// Share this agent with execution contexts.
    pub fn with_agent(mut self, agent: Arc<dyn Agent<D>>) -> Self {
        self.agent = Some(agent);
        self
    }

    // {impl}`requirements.sdk.graph.validation.check_node`
    // {impl}`requirements.sdk.graph.validation.procedure_identity`
    // {impl}`requirements.sdk.graph.evidence.record.output`
    // {impl}`requirements.sdk.graph.evidence.inputs`
    /// Load this criterion's evidence and decode its answer.
    pub fn load_evidence<V: Criterion<D>>(
        &self,
        criterion: &V,
    ) -> impl Future<Output = Result<Option<Fingerprinted<D, V::Ok>>, ValidationError<D::Error>>> + Send
    {
        // Read the descriptor before constructing the future: V needs no Sync.
        let node = criterion.source_node_id();
        let criterion_path = criterion.criterion_path();
        async move {
            let Some(record) = self
                .domain
                .load_evidence(&node, &criterion_path)
                .await
                .map_err(ValidationError::Graph)?
            else {
                return Ok(None);
            };
            let output = serde_json::from_value(record.output).map_err(ValidationError::Output)?;
            Ok(Some(Fingerprinted {
                source: node,
                criterion: criterion_path,
                revision: record.revision,
                dependencies: record.dependencies,
                output,
            }))
        }
    }

    // {impl}`requirements.sdk.graph.validation.checking_records`
    /// Compare recorded input fingerprints with the current domain state.
    pub fn check_inputs<O>(&self, inputs: &Fingerprinted<D, O>) -> Result<bool, D::Error> {
        dependencies_are_current(&self.domain, &inputs.dependencies)
    }

    // {impl}`requirements.sdk.graph.validation.validate_node`
    // {impl}`requirements.sdk.graph.validation.validate_node.errors`
    // {impl}`requirements.sdk.graph.validation.procedure_identity`
    // {impl}`requirements.sdk.graph.evidence.inputs`
    /// Execute the criterion and return its completed answer and observed inputs.
    pub fn verify<V: Criterion<D>>(
        &self,
        criterion: V,
    ) -> impl Future<Output = Result<Fingerprinted<D, V::Ok>, ValidationError<D::Error>>> + Send
    {
        async move {
            let node = criterion.source_node_id();
            let criterion_path = criterion.criterion_path();
            let revision = criterion.revision();
            let mut ctx =
                ValidationContext::configured(&self.domain, node.clone(), self.agent.as_deref());
            let answer = criterion.execute(&mut ctx).await?;
            Ok(Fingerprinted {
                source: node,
                criterion: criterion_path,
                revision,
                dependencies: ctx.inspection().dependencies(),
                output: answer,
            })
        }
    }

    // {impl}`requirements.sdk.graph.validation.store_evidence`
    // {impl}`requirements.sdk.graph.evidence.record.output`
    /// Encode the answer and atomically replace its source/criterion record.
    pub fn store_evidence<O: Serialize>(
        &self,
        inputs: &Fingerprinted<D, O>,
    ) -> impl Future<Output = Result<(), ValidationError<D::Error>>> + Send {
        // Encode before constructing the future: O needs no Sync or Clone.
        let encoded = serde_json::to_value(&inputs.output).map(|output| Record {
            revision: inputs.revision.clone(),
            dependencies: inputs.dependencies.clone(),
            output,
        });
        let node = inputs.source.clone();
        let criterion_path = inputs.criterion.clone();
        async move {
            let record = encoded.map_err(ValidationError::Output)?;
            self.domain
                .store_evidence(&node, &criterion_path, &record)
                .await
                .map_err(ValidationError::Graph)
        }
    }
}

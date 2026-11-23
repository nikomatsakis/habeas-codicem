use super::*;
use crate::procedure::Agent;
use std::sync::Arc;

/// Shared domain and agent access. Each execution owns its dependency trace.
// {impl}`requirements.sdk.graph.validators.validator`
pub struct Validator<D: Domain> {
    domain: D,
    agent: Option<Arc<dyn Agent<D>>>,
    agent_config: Option<crate::agent::AgentConfig>,
    source_tools: Option<Arc<dyn crate::agent_runtime::ToolFactory<D>>>,
}

impl<D: Domain> Validator<D> {
    // {impl}`requirements.sdk.graph.validators.validator`
    /// Own the domain used to check and execute criteria.
    pub fn new(domain: D) -> Self {
        Self {
            domain,
            agent: None,
            agent_config: None,
            source_tools: None,
        }
    }

    // {impl}`requirements.sdk.graph.agent-harness.with_agent`
    /// Configure the SDK's supplied harness. Backend setup happens when an ask is awaited.
    pub fn with_agent(mut self, config: crate::agent::AgentConfig) -> Self
    where
        D: 'static,
        D::EdgeLabel: serde::de::DeserializeOwned + schemars::JsonSchema + PartialEq,
        D::Error: std::fmt::Display,
    {
        self.agent_config = Some(config.clone());
        self.agent = Some(Arc::new(
            crate::agent_runtime::ConfiguredAgent::<D>::with_tools(
                config,
                self.source_tools
                    .clone()
                    .unwrap_or_else(|| Arc::new(crate::agent_runtime::NoTools)),
            ),
        ));
        self
    }

    /// Integration hook for Habeas's assessment adapter and internal test providers.
    #[doc(hidden)]
    #[cfg(any(test, feature = "habeas-integration"))]
    pub fn with_agent_adapter(mut self, agent: Arc<dyn Agent<D>>) -> Self {
        self.agent = Some(agent);
        self
    }

    // {impl}`requirements.sdk.graph.evidence.refresh_evidence`
    /// Reuse compatible, current evidence, or evaluate and save a new answer.
    /// Errors from loading, decoding, checking, execution, or saving propagate.
    pub fn refresh_evidence<V: Criterion<D>>(
        &self,
        criterion: V,
    ) -> impl Future<Output = Result<Fingerprinted<D, V::Ok>, ValidationError<D::Error>>> + Send
    {
        async move {
            if let Some(saved) = self.load_evidence(&criterion).await? {
                if procedure_matches(saved.revision(), &criterion.revision())
                    && self.check_inputs(&saved).map_err(ValidationError::Graph)?
                {
                    return Ok(saved);
                }
            }

            let inputs = self.verify(criterion).await?;
            self.store_evidence(&inputs).await?;
            Ok(inputs)
        }
    }

    // {impl}`requirements.sdk.graph.evidence.check_node`
    // {impl}`requirements.sdk.graph.evidence.procedure_identity`
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

    // {impl}`requirements.sdk.graph.evidence.checking_records`
    /// Compare recorded input fingerprints with the current domain state.
    pub fn check_inputs<O>(&self, inputs: &Fingerprinted<D, O>) -> Result<bool, D::Error> {
        dependencies_are_current(&self.domain, &inputs.dependencies)
    }

    // {impl}`requirements.sdk.graph.validators.validate_node`
    // {impl}`requirements.sdk.graph.validators.validate_node.errors`
    // {impl}`requirements.sdk.graph.evidence.procedure_identity`
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

    // {impl}`requirements.sdk.graph.evidence.store_evidence`
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

impl<D: crate::domain::SourceNodes + 'static> Validator<D>
where
    D::EdgeLabel: serde::de::DeserializeOwned + schemars::JsonSchema + PartialEq,
    D::Error: std::fmt::Display,
{
    // {impl}`requirements.sdk.sources.provider.attachment`
    pub fn with_sources(mut self, sources: crate::sources::Sources) -> Self {
        let tools: Arc<dyn crate::agent_runtime::ToolFactory<D>> =
            Arc::new(crate::agent_runtime::project_sources::ProjectSources {
                sources,
                domain: self.domain.clone(),
            });
        self.source_tools = Some(tools.clone());
        if let Some(config) = &self.agent_config {
            self.agent = Some(Arc::new(
                crate::agent_runtime::ConfiguredAgent::<D>::with_tools(config.clone(), tools),
            ));
        }
        self
    }
}

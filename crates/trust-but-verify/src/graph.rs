//! Consumer-defined graphs, tracked observations, and validation.

use crate::domain::Domain;
use crate::domain::NodeContents;
use crate::domain::NodeId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::future::Future;

use crate::request::ValidationContext;

// {impl}`requirements.sdk.graph.fingerprint`
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Fingerprint {
    pub scheme: String,
    pub value: String,
}

impl Fingerprint {
    pub fn new(scheme: String, value: String) -> Self {
        Self { scheme, value }
    }
}

// {impl}`requirements.sdk.graph.snapshot`
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Snapshot<T> {
    pub content: T,
    pub fingerprint: Fingerprint,
}

impl<T> Snapshot<T> {
    pub fn new(content: T, fingerprint: Fingerprint) -> Self {
        Self {
            content,
            fingerprint,
        }
    }
}

/// An ordered path with read-only access to its segments.
///
/// ```compile_fail
/// use trust_but_verify::graph::NodePath;
/// let mut path = NodePath::new(vec!["part".into()]);
/// path.segments.clear();
/// ```
// {test}`requirements.sdk.graph.node_path_struct`
// {impl}`requirements.sdk.graph.node_path_struct`
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodePath {
    segments: Vec<String>,
}

impl NodePath {
    /// Construct a path, preserving segment order and boundaries.
    // {impl}`requirements.sdk.graph.node_path_struct`
    pub fn new(segments: Vec<String>) -> Self {
        Self { segments }
    }

    /// Read the ordered path segments.
    // {impl}`requirements.sdk.graph.node_path_struct`
    pub fn segments(&self) -> &[String] {
        &self.segments
    }
}

/// An ordered path with read-only access to its segments.
///
/// ```compile_fail
/// use trust_but_verify::graph::CriterionPath;
/// let mut path = CriterionPath::new(vec!["part".into()]);
/// path.segments.clear();
/// ```
// {test}`requirements.sdk.graph.criteria.criterion_path`
// {impl}`requirements.sdk.graph.criteria.criterion_path`
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CriterionPath {
    segments: Vec<String>,
}

impl CriterionPath {
    /// Construct a path, preserving segment order and boundaries.
    // {impl}`requirements.sdk.graph.criteria.criterion_path`
    pub fn new(segments: Vec<String>) -> Self {
        Self { segments }
    }

    /// Read the ordered path segments.
    // {impl}`requirements.sdk.graph.criteria.criterion_path`
    pub fn segments(&self) -> &[String] {
        &self.segments
    }
}

// {impl}`requirements.sdk.graph.evidence.dependencies`
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Dependency<N, L = ()> {
    Node(N, Fingerprint),
    OutgoingEdges(N, L, Fingerprint),
}

/// The application-defined version of a criterion's check and answer encoding.
// {impl}`requirements.sdk.graph.criteria.criterion_version`
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct CriterionVersion(String);

impl CriterionVersion {
    /// Construct a version for the current criterion. Panics if `value` is `"*"`.
    pub fn new(value: impl Into<String>) -> Self {
        Self::from_recorded(value).expect("* is reserved for unversioned evidence")
    }

    /// Decode a saved version string, where `"*"` stands for an unknown version.
    pub fn from_recorded(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        (value != "*").then_some(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// An unversioned legacy record passes this check. A known version must match exactly.
    // {impl}`requirements.sdk.graph.evidence.checking_records.version_match`
    pub fn is_compatible_with(&self, saved: Option<&Self>) -> bool {
        saved.is_none_or(|version| version == self)
    }
}

impl<'de> Deserialize<'de> for CriterionVersion {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::from_recorded(String::deserialize(deserializer)?)
            .ok_or_else(|| serde::de::Error::custom("expected a concrete criterion version"))
    }
}

// {impl}`requirements.sdk.graph.evidence.recorded_criterion_version`
mod stored_version {
    use super::CriterionVersion;
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(
        version: &Option<CriterionVersion>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(version.as_ref().map_or("*", CriterionVersion::as_str))
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<CriterionVersion>, D::Error> {
        Ok(CriterionVersion::from_recorded(String::deserialize(
            deserializer,
        )?))
    }
}

// {impl}`requirements.sdk.graph.evidence.record`
#[derive(Clone, Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Record<N, L = ()> {
    #[serde(rename = "revision", with = "stored_version")]
    pub version: Option<CriterionVersion>,
    pub dependencies: Vec<Dependency<N, L>>,
    pub output: serde_json::Value,
}

impl<N, L> Record<N, L> {
    pub fn new(
        version: Option<CriterionVersion>,
        dependencies: Vec<Dependency<N, L>>,
        output: serde_json::Value,
    ) -> Self {
        Self {
            version,
            dependencies,
            output,
        }
    }

    /// Check the criterion version and the current fingerprints of the saved inputs.
    // {impl}`requirements.sdk.graph.evidence.record.is_current`
    // {impl}`requirements.sdk.graph.evidence.checking_records`
    pub fn is_current<D>(&self, graph: &D, version: &CriterionVersion) -> Result<bool, D::Error>
    where
        D: Domain<NodeData = N, EdgeLabel = L>,
    {
        if !version.is_compatible_with(self.version.as_ref()) {
            return Ok(false);
        }
        dependencies_are_current(graph, &restore_dependencies(graph, &self.dependencies))
    }
}

/// A computed value and the fingerprints of its dependencies.
///
/// The captured evidence metadata is available through read-only accessors.
///
/// ```compile_fail,E0616
/// use trust_but_verify::{domain::Domain, graph::Fingerprinted};
/// fn change_revision<D: Domain>(answer: &mut Fingerprinted<D, bool>) {
///     answer.version = Some(trust_but_verify::graph::CriterionVersion::new("different rules"));
/// }
/// ```
///
/// ```compile_fail,E0616
/// use trust_but_verify::{domain::Domain, graph::{CriterionPath, Fingerprinted}};
/// fn change_source<D: Domain>(answer: &mut Fingerprinted<D, bool>, source: trust_but_verify::domain::NodeId<D>) {
///     answer.source = source;
/// }
/// ```
///
/// ```compile_fail,E0616
/// use trust_but_verify::{domain::Domain, graph::{CriterionPath, Fingerprinted}};
/// fn change_question<D: Domain>(answer: &mut Fingerprinted<D, bool>) {
///     answer.criterion = CriterionPath::new(vec![]);
/// }
/// ```
///
/// ```compile_fail,E0616
/// use trust_but_verify::{domain::Domain, graph::Fingerprinted};
/// fn change_inputs<D: Domain>(answer: &mut Fingerprinted<D, bool>) {
///     answer.dependencies.clear();
/// }
/// ```
///
/// ```compile_fail,E0616
/// use trust_but_verify::{domain::Domain, graph::Fingerprinted};
/// fn change_answer<D: Domain>(answer: &mut Fingerprinted<D, bool>) {
///     answer.output = false;
/// }
/// ```
// {test}`requirements.sdk.graph.evidence.inputs`
// {impl}`requirements.sdk.graph.evidence.inputs`
pub struct Fingerprinted<D: Domain, O> {
    source: NodeId<D>,
    criterion: CriterionPath,
    version: Option<CriterionVersion>,
    dependencies: Vec<Dependency<NodeId<D>, D::EdgeLabel>>,
    output: O,
}

// {impl}`requirements.sdk.graph.evidence.inputs`
impl<D: Domain, O> Fingerprinted<D, O> {
    /// The source node this answer concerns.
    pub fn source_node_id(&self) -> &NodeId<D> {
        &self.source
    }

    /// The question's path relative to that source.
    pub fn criterion_path(&self) -> &CriterionPath {
        &self.criterion
    }

    /// The review rules used to produce this answer.
    pub fn version(&self) -> Option<&CriterionVersion> {
        self.version.as_ref()
    }

    /// Recorded node-content and successor-set observations.
    pub fn dependencies(&self) -> &[Dependency<NodeId<D>, D::EdgeLabel>] {
        &self.dependencies
    }

    /// Borrow the completed answer.
    pub fn output(&self) -> &O {
        &self.output
    }

    /// Take ownership of the completed answer.
    pub fn into_output(self) -> O {
        self.output
    }
}

// {impl}`requirements.sdk.graph.criteria.execute`
#[derive(Debug)]
pub enum ValidationError<E> {
    Graph(E),
    NoAgentConfigured,
    Agent(String),
    Provider(std::io::Error),
    Output(serde_json::Error),
}

impl<E: std::fmt::Display> std::fmt::Display for ValidationError<E> {
    // {cite}`habeas:util`
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Graph(error) => error.fmt(f),
            Self::NoAgentConfigured => f.write_str("no agent is configured"),
            Self::Provider(error) => error.fmt(f),
            Self::Agent(error) => write!(f, "agent: {error}"),
            Self::Output(error) => write!(f, "output: {error}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for ValidationError<E> {
    // {cite}`habeas:util`
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Provider(error) => Some(error),
            Self::Graph(error) => Some(error),
            Self::Output(error) => Some(error),
            Self::Agent(_) | Self::NoAgentConfigured => None,
        }
    }
}

// {impl}`requirements.sdk.graph.criteria.criterion_trait`
pub trait Criterion<D: Domain>: Send + Sized {
    /// The completed answer, serialized for storage and reconstructed on reuse.
    // {impl}`requirements.sdk.graph.criteria.criterion_trait.answer`
    type Ok: Serialize + serde::de::DeserializeOwned + Send;

    /// The node this question is about.
    // {impl}`requirements.sdk.graph.criteria.criterion_trait.source_node_id`
    fn source_node_id(&self) -> NodeId<D>;
    /// Identify this question within the source node's saved evidence.
    // {impl}`requirements.sdk.graph.criteria.criterion_path`
    fn criterion_path(&self) -> CriterionPath;
    /// Version of the review rules and output encoding.
    /// Change it when an old answer should no longer be reused.
    // {impl}`requirements.sdk.graph.criteria.criterion_trait.version`
    fn version(&self) -> CriterionVersion;

    /// Select inputs through the context and compute the completed answer.
    // {impl}`requirements.sdk.graph.criteria.execute`
    fn execute(
        self,
        ctx: &mut ValidationContext<'_, D>,
    ) -> impl Future<Output = Result<Self::Ok, ValidationError<D::Error>>> + Send;
}

/// Provider, input, and tool usage accumulated during validation.
// {impl}`requirements.sdk.graph.running-criteria.UsageStats`
#[derive(Clone, Debug, Default, Serialize)]
#[non_exhaustive]
pub struct UsageStats {
    pub attempts: u64,
    pub retries: u64,
    pub failed_attempts: u64,
    pub prepared_inputs: u64,
    pub delivered_inputs: u64,
    pub tools: std::collections::BTreeMap<String, ToolUsage>,
    pub input_tokens: TokenCount,
    pub output_tokens: TokenCount,
    pub cached_input_tokens: TokenCount,
    pub cache_creation_input_tokens: TokenCount,
    pub reasoning_tokens: TokenCount,
}

impl UsageStats {
    pub(crate) fn attempt(
        retry: bool,
        failed: bool,
        usage: Option<&rig_core::completion::Usage>,
    ) -> Self {
        Self {
            attempts: 1,
            prepared_inputs: 0,
            delivered_inputs: 0,
            tools: Default::default(),
            retries: u64::from(retry),
            failed_attempts: u64::from(failed),
            input_tokens: TokenCount::measurement(usage.and_then(|u| u.input_tokens)),
            output_tokens: TokenCount::measurement(usage.and_then(|u| u.output_tokens)),
            cached_input_tokens: TokenCount::measurement(usage.and_then(|u| u.cached_input_tokens)),
            cache_creation_input_tokens: TokenCount::measurement(
                usage.and_then(|u| u.cache_creation_input_tokens),
            ),
            reasoning_tokens: TokenCount::measurement(usage.and_then(|u| u.reasoning_tokens)),
        }
    }

    pub(crate) fn accumulate(&mut self, other: &Self) {
        for (name, delta) in &other.tools {
            let tool = self.tools.entry(name.clone()).or_default();
            tool.calls += delta.calls;
            tool.result_bytes += delta.result_bytes;
        }
        self.attempts += other.attempts;
        self.retries += other.retries;
        self.prepared_inputs += other.prepared_inputs;
        self.delivered_inputs += other.delivered_inputs;
        self.failed_attempts += other.failed_attempts;
        self.input_tokens.accumulate(other.input_tokens);
        self.output_tokens.accumulate(other.output_tokens);
        self.cached_input_tokens
            .accumulate(other.cached_input_tokens);
        self.cache_creation_input_tokens
            .accumulate(other.cache_creation_input_tokens);
        self.reasoning_tokens.accumulate(other.reasoning_tokens);
    }
}

/// Invocations and UTF-8 result bytes accumulated for one tool.
// {impl}`requirements.sdk.graph.running-criteria.ToolUsage`
#[derive(Clone, Copy, Debug, Default, Serialize)]
#[non_exhaustive]
pub struct ToolUsage {
    pub calls: u64,
    pub result_bytes: u64,
}

/// Measured tokens and the number of attempts without a measurement for this category.
// {impl}`requirements.sdk.graph.running-criteria.TokenCount`
#[derive(Clone, Copy, Debug, Default, Serialize)]
#[non_exhaustive]
pub struct TokenCount {
    pub reported: u64,
    pub missing_attempts: u64,
}

impl TokenCount {
    fn measurement(value: Option<u64>) -> Self {
        Self {
            reported: value.unwrap_or_default(),
            missing_attempts: u64::from(value.is_none()),
        }
    }

    fn accumulate(&mut self, other: Self) {
        self.reported += other.reported;
        self.missing_attempts += other.missing_attempts;
    }
}

/// Executes criteria on a borrowed domain and counts the provider usage of this operation.
// {impl}`requirements.sdk.graph.running-criteria.Validator`
pub struct Validator<'d, D: Domain> {
    domain: &'d D,
    usage: UsageStats,
}

impl<'d, D: Domain> Validator<'d, D> {
    // {impl}`requirements.sdk.graph.running-criteria.Validator.new`
    pub fn new(domain: &'d D) -> Self {
        Self {
            domain,
            usage: UsageStats::default(),
        }
    }

    // {impl}`requirements.sdk.graph.running-criteria.Validator.usage`
    pub fn usage(&self) -> &UsageStats {
        &self.usage
    }
    // {impl}`requirements.sdk.graph.evidence.refresh_evidence`
    /// Reuse compatible, current evidence, or evaluate and save a new answer.
    /// Errors from loading, decoding, checking, execution, or saving propagate.
    // {impl}`requirements.sdk.graph.running-criteria.refresh_evidence`
    pub fn refresh_evidence<V: Criterion<D>>(
        &mut self,
        criterion: V,
    ) -> impl Future<Output = Result<Fingerprinted<D, V::Ok>, ValidationError<D::Error>>> + Send
    {
        async move {
            if let Some(saved) = self.load_evidence(&criterion).await? {
                if criterion.version().is_compatible_with(saved.version())
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
    // {impl}`requirements.sdk.graph.running-criteria.load_evidence`
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
                version: record.version,
                dependencies: restore_dependencies(self.domain, &record.dependencies),
                output,
            }))
        }
    }

    // {impl}`requirements.sdk.graph.evidence.checking_records`
    /// Compare recorded input fingerprints with the current domain state.
    // {impl}`requirements.sdk.graph.running-criteria.check_inputs`
    pub fn check_inputs<O>(&self, inputs: &Fingerprinted<D, O>) -> Result<bool, D::Error> {
        dependencies_are_current(self.domain, &inputs.dependencies)
    }

    // {impl}`requirements.sdk.graph.evidence.procedure_identity`
    // {impl}`requirements.sdk.graph.evidence.inputs`
    /// Execute the criterion and return its completed answer and observed inputs.
    // {impl}`requirements.sdk.graph.running-criteria.verify`
    // {impl}`requirements.sdk.graph.running-criteria.verify.concurrent`
    // {impl}`requirements.sdk.graph.validation-context.shared_context`
    pub fn verify<V: Criterion<D>>(
        &mut self,
        criterion: V,
    ) -> impl Future<Output = Result<Fingerprinted<D, V::Ok>, ValidationError<D::Error>>> + Send
    {
        self.execute(criterion, None)
    }

    // {impl}`requirements.sdk.graph.running-criteria.validate_node`
    // {impl}`requirements.sdk.graph.running-criteria.validate_node.errors`
    // {impl}`requirements.sdk.graph.running-criteria.Validator.usage`
    pub(crate) async fn execute<V: Criterion<D>>(
        &mut self,
        criterion: V,
        agent: Option<&dyn crate::request::Agent<D>>,
    ) -> Result<Fingerprinted<D, V::Ok>, ValidationError<D::Error>> {
        let node = criterion.source_node_id();
        let mut ctx = ValidationContext::configured(self.domain, node, agent);
        ctx.usage = Some(std::sync::Mutex::new(&mut self.usage));
        let criterion_path = criterion.criterion_path();
        let version = criterion.version();

        let answer = criterion.execute(&mut ctx).await?;
        Ok(Fingerprinted {
            source: node,
            criterion: criterion_path,
            version: Some(version),
            dependencies: ctx.dependencies(),
            output: answer,
        })
    }

    // {impl}`requirements.sdk.graph.evidence.store_evidence`
    // {impl}`requirements.sdk.graph.evidence.record.output`
    /// Encode the answer and atomically replace its source/criterion record.
    // {impl}`requirements.sdk.graph.running-criteria.store_evidence`
    pub fn store_evidence<O: Serialize>(
        &self,
        inputs: &Fingerprinted<D, O>,
    ) -> impl Future<Output = Result<(), ValidationError<D::Error>>> + Send {
        // Encode before constructing the future: O needs no Sync or Clone.
        let encoded = serde_json::to_value(&inputs.output).map(|output| Record {
            version: inputs.version.clone(),
            dependencies: persist_dependencies(self.domain, &inputs.dependencies),
            output,
        });
        let node = inputs.source;
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

// {impl}`requirements.sdk.graph.evidence.persisted_dependencies`
fn map_dependencies<N, M, L: Clone>(
    dependencies: &[Dependency<N, L>],
    mut map: impl FnMut(&N) -> M,
) -> Vec<Dependency<M, L>> {
    dependencies
        .iter()
        .map(|dependency| match dependency {
            Dependency::Node(node, fingerprint) => Dependency::Node(map(node), fingerprint.clone()),
            Dependency::OutgoingEdges(node, label, fingerprint) => {
                Dependency::OutgoingEdges(map(node), label.clone(), fingerprint.clone())
            }
        })
        .collect()
}

fn restore_dependencies<D: Domain>(
    domain: &D,
    dependencies: &[Dependency<D::NodeData, D::EdgeLabel>],
) -> Vec<Dependency<NodeId<D>, D::EdgeLabel>> {
    map_dependencies(dependencies, |data| {
        domain.cache().intern_node(data.clone())
    })
}

pub(crate) fn persist_dependencies<D: Domain>(
    domain: &D,
    dependencies: &[Dependency<NodeId<D>, D::EdgeLabel>],
) -> Vec<Dependency<D::NodeData, D::EdgeLabel>> {
    map_dependencies(dependencies, |id| {
        (*domain
            .cache()
            .node_data(*id)
            .expect("dependency belongs to this cache"))
        .clone()
    })
}

fn dependencies_are_current<D: Domain>(
    graph: &D,
    dependencies: &[Dependency<NodeId<D>, D::EdgeLabel>],
) -> Result<bool, D::Error> {
    for dependency in dependencies {
        let (current, previous) = match dependency {
            Dependency::Node(node, previous) => {
                let content = graph.node_content(node)?;
                (content.fingerprint(), previous)
            }
            Dependency::OutgoingEdges(node, label, previous) => (
                edge_fingerprint(graph, &graph.outgoing_edges(node, label.clone())?),
                previous,
            ),
        };
        if &current != previous {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Hash the target set without requiring serialization or hashing of opaque IDs.
/// JSON encodes segment boundaries and Unicode unambiguously; ordering and
/// duplicate edges do not affect membership. Empty selections have a digest too.
// {impl}`requirements.sdk.graph.validation-context.snapshot`
pub fn edge_fingerprint<D: Domain>(graph: &D, targets: &[NodeId<D>]) -> Fingerprint {
    let mut paths = targets
        .iter()
        .map(|node| graph.node_path(node).segments)
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    let bytes = serde_json::to_vec(&paths).expect("string paths serialize");
    Fingerprint {
        scheme: "node-path-set-sha256-v1".into(),
        value: Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    }
}

#[cfg(test)]
mod validation_tests;

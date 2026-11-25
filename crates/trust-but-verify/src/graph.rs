//! Consumer-defined graphs, tracked observations, and validation.

use crate::domain::Domain;
use crate::domain::NodeContents;
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
        D: Domain<NodeId = N, EdgeLabel = L>,
    {
        if !version.is_compatible_with(self.version.as_ref()) {
            return Ok(false);
        }
        dependencies_are_current(graph, &self.dependencies)
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
/// fn change_source<D: Domain>(answer: &mut Fingerprinted<D, bool>, source: D::NodeId) {
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
    source: D::NodeId,
    criterion: CriterionPath,
    version: Option<CriterionVersion>,
    dependencies: Vec<Dependency<D::NodeId, D::EdgeLabel>>,
    output: O,
}

// {impl}`requirements.sdk.graph.evidence.inputs`
impl<D: Domain, O> Fingerprinted<D, O> {
    /// The source node this answer concerns.
    pub fn source_node_id(&self) -> &D::NodeId {
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
    pub fn dependencies(&self) -> &[Dependency<D::NodeId, D::EdgeLabel>] {
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
    Output(serde_json::Error),
}

impl<E: std::fmt::Display> std::fmt::Display for ValidationError<E> {
    // {cite}`habeas:util`
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Graph(error) => error.fmt(f),
            Self::NoAgentConfigured => f.write_str("no agent is configured"),
            Self::Agent(error) => write!(f, "agent: {error}"),
            Self::Output(error) => write!(f, "output: {error}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for ValidationError<E> {
    // {cite}`habeas:util`
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
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
    fn source_node_id(&self) -> D::NodeId;
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

// {impl}`requirements.sdk.graph.evidence.refresh_evidence`
/// Reuse compatible, current evidence, or evaluate and save a new answer.
/// Errors from loading, decoding, checking, execution, or saving propagate.
// {impl}`requirements.sdk.graph.running-criteria.refresh_evidence`
pub fn refresh_evidence<D: Domain, V: Criterion<D>>(
    domain: &D,
    criterion: V,
) -> impl Future<Output = Result<Fingerprinted<D, V::Ok>, ValidationError<D::Error>>> + Send {
    async move {
        if let Some(saved) = load_evidence(domain, &criterion).await? {
            if criterion.version().is_compatible_with(saved.version())
                && check_inputs(domain, &saved).map_err(ValidationError::Graph)?
            {
                return Ok(saved);
            }
        }

        let inputs = verify(domain, criterion).await?;
        store_evidence(domain, &inputs).await?;
        Ok(inputs)
    }
}

// {impl}`requirements.sdk.graph.evidence.check_node`
// {impl}`requirements.sdk.graph.evidence.procedure_identity`
// {impl}`requirements.sdk.graph.evidence.record.output`
// {impl}`requirements.sdk.graph.evidence.inputs`
/// Load this criterion's evidence and decode its answer.
// {impl}`requirements.sdk.graph.running-criteria.load_evidence`
pub fn load_evidence<D: Domain, V: Criterion<D>>(
    domain: &D,
    criterion: &V,
) -> impl Future<Output = Result<Option<Fingerprinted<D, V::Ok>>, ValidationError<D::Error>>> + Send
{
    // Read the descriptor before constructing the future: V needs no Sync.
    let node = criterion.source_node_id();
    let criterion_path = criterion.criterion_path();
    async move {
        let Some(record) = domain
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
            dependencies: record.dependencies,
            output,
        }))
    }
}

// {impl}`requirements.sdk.graph.evidence.checking_records`
/// Compare recorded input fingerprints with the current domain state.
// {impl}`requirements.sdk.graph.running-criteria.check_inputs`
pub fn check_inputs<D: Domain, O>(
    domain: &D,
    inputs: &Fingerprinted<D, O>,
) -> Result<bool, D::Error> {
    dependencies_are_current(domain, &inputs.dependencies)
}

// {impl}`requirements.sdk.graph.running-criteria.validate_node`
// {impl}`requirements.sdk.graph.running-criteria.validate_node.errors`
// {impl}`requirements.sdk.graph.evidence.procedure_identity`
// {impl}`requirements.sdk.graph.evidence.inputs`
/// Execute the criterion and return its completed answer and observed inputs.
// {impl}`requirements.sdk.graph.running-criteria.verify`
// {impl}`requirements.sdk.graph.running-criteria.verify.concurrent`
pub fn verify<D: Domain, V: Criterion<D>>(
    domain: &D,
    criterion: V,
) -> impl Future<Output = Result<Fingerprinted<D, V::Ok>, ValidationError<D::Error>>> + Send {
    execute_with_context(
        ValidationContext::new(domain, criterion.source_node_id()),
        criterion,
    )
}

pub(crate) async fn execute_with_context<D: Domain, V: Criterion<D>>(
    mut ctx: ValidationContext<'_, D>,
    criterion: V,
) -> Result<Fingerprinted<D, V::Ok>, ValidationError<D::Error>> {
    let node = ctx.node_id().clone();
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
pub fn store_evidence<D: Domain, O: Serialize>(
    domain: &D,
    inputs: &Fingerprinted<D, O>,
) -> impl Future<Output = Result<(), ValidationError<D::Error>>> + Send {
    // Encode before constructing the future: O needs no Sync or Clone.
    let encoded = serde_json::to_value(&inputs.output).map(|output| Record {
        version: inputs.version.clone(),
        dependencies: inputs.dependencies.clone(),
        output,
    });
    let node = inputs.source.clone();
    let criterion_path = inputs.criterion.clone();
    async move {
        let record = encoded.map_err(ValidationError::Output)?;
        domain
            .store_evidence(&node, &criterion_path, &record)
            .await
            .map_err(ValidationError::Graph)
    }
}

fn dependencies_are_current<D: Domain>(
    graph: &D,
    dependencies: &[Dependency<D::NodeId, D::EdgeLabel>],
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
pub fn edge_fingerprint<D: Domain>(graph: &D, targets: &[D::NodeId]) -> Fingerprint {
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

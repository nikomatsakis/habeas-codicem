//! Habeas's interpretation of the SDK graph. Wire formats, selection rules,
//! and findings remain Habeas policy.

use crate::{
    evidence::Declaration,
    inputs::{InputSelector, OutlineCache, ResolvedInput},
};
use std::sync::Arc;
use std::{collections::BTreeMap, path::Path};
use trust_but_verify::domain::NodeContents;
use trust_but_verify::domain::ValidationCache;
use trust_but_verify::domain::{Domain, NodeId};
use trust_but_verify::graph::CriterionPath;
use trust_but_verify::graph::CriterionVersion;
use trust_but_verify::graph::Fingerprint;
use trust_but_verify::graph::NodePath;
#[cfg(test)]
use trust_but_verify::request::ValidationContext;

/// Source address as stored in Habeas evidence records.
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub(crate) struct OutlineId {
    pub resource: ResourceId,
    pub selector: String,
}

#[derive(
    Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub(crate) struct ResourceId {
    pub scheme: String,
    pub location: String,
}

// {cite}`habeas:util`
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub(crate) struct RequirementId(pub String);

// {cite}`habeas:util`
/// Habeas node identities preserve the source address stored in existing evidence.
// :::{fixme} requirements.sdk.habeas.github_nodes
// :github: nikomatsakis/habeas-codicem#26
// GitHub issue graph nodes are deferred. Stable owner/repository/number identity
// and retained title/opening-post/status contents have no implementation or tests.
// :::
// :::{fixme} requirements.sdk.habeas.github_nodes.inputs
// :github: nikomatsakis/habeas-codicem#26
// Tracked GitHub issue snapshots are deferred. Tests do not yet cover their
// redacted contents, field-sensitive fingerprints, or fetch-time/TTL independence.
// :::
// :::{fixme} requirements.sdk.habeas.fixme_graph
// :github: nikomatsakis/habeas-codicem#26
// FIXME graph nodes and tracking-issue edges are deferred. Named/unnamed
// acknowledgments, source ownership, and multiple issue targets have no tests.
// :::
// :::{fixme} requirements.sdk.habeas.github_nodes.open
// :github: nikomatsakis/habeas-codicem#26
// The tracked deterministic open-issue criterion is deferred. Tests do not yet
// verify completed open/closed assessments through the SDK without an agent.
// :::
// :::{fixme} requirements.sdk.habeas.github_nodes.related
// :github: nikomatsakis/habeas-codicem#26
// The agent-based issue-relevance criterion is deferred. Tests do not yet check
// tracking-edge selection, delivered FIXME/issue snapshots, or unrelated answers.
// :::
// :::{fixme} requirements.sdk.habeas.github_nodes.cache
// :github: nikomatsakis/habeas-codicem#26
// Application-selected TTL caching for graph issue contents is deferred. Tests
// do not yet cover reuse, expiry refresh/failure, or snapshot-consistent formatting
// and fingerprinting. The existing CLI issue-state cache is a separate interface.
// :::
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub(crate) enum HabeasNode {
    Requirement(RequirementId),
    ReferenceSet(RequirementId),
    RequirementSelection(String),
    Definition(String),
    #[serde(alias = "CitationReview")]
    Source(OutlineId),
    SiteReview {
        requirement: String,
        source: OutlineId,
    },
}

impl From<&InputSelector> for HabeasNode {
    // {cite}`habeas:util`
    fn from(selector: &InputSelector) -> Self {
        match selector {
            InputSelector::Outline { file, item } => Self::Source(OutlineId {
                resource: ResourceId {
                    scheme: "repo".into(),
                    location: file.clone(),
                },
                selector: item.clone(),
            }),
            InputSelector::Definition { definition } => Self::Definition(definition.clone()),
            InputSelector::Requirements { requirements } if requirements.contains('*') => {
                Self::RequirementSelection(requirements.clone())
            }
            InputSelector::Requirements { requirements } => {
                Self::Requirement(RequirementId(requirements.clone()))
            }
        }
    }
}

impl HabeasNode {
    // {cite}`habeas:util`
    pub(crate) fn selector(&self) -> Result<InputSelector, std::io::Error> {
        Ok(match self {
            Self::ReferenceSet(_) => {
                return Err(std::io::Error::other(
                    "reference-set nodes are not source inputs",
                ));
            }
            Self::Requirement(id) => InputSelector::Requirements {
                requirements: id.0.clone(),
            },
            Self::RequirementSelection(glob) => InputSelector::Requirements {
                requirements: glob.clone(),
            },
            Self::Definition(name) => InputSelector::Definition {
                definition: name.clone(),
            },
            Self::Source(source) | Self::SiteReview { source, .. } => {
                if source.resource.scheme != "repo" {
                    return Err(std::io::Error::other(format!(
                        "Habeas does not resolve resource scheme {:?}",
                        source.resource.scheme
                    )));
                }
                InputSelector::Outline {
                    file: source.resource.location.clone(),
                    item: source.selector.clone(),
                }
            }
        })
    }
}

// {cite}`habeas:util`
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub(crate) enum HabeasEdgeLabel {
    Implements,
    Cites,
    Tests,
    Acknowledges,
    Mandatory,
}

// {cite}`habeas:util`
#[derive(Clone)]
pub(crate) struct HabeasDomain {
    pub root: Arc<Path>,
    pub definitions: Arc<BTreeMap<String, Declaration>>,
    pub outlines: OutlineCache,
    relationships: Arc<
        std::sync::RwLock<
            std::collections::HashMap<(NodeId<Self>, HabeasEdgeLabel), Vec<NodeId<Self>>>,
        >,
    >,
}

pub(crate) struct HabeasNodeContent {
    fingerprint: Fingerprint,
    text: String,
}

impl HabeasNodeContent {
    fn prepare(node: &HabeasNode, resolved: ResolvedInput) -> std::io::Result<Self> {
        let (fingerprint, model) = if matches!(node, HabeasNode::ReferenceSet(_)) {
            (
                Fingerprint::new("habeas-reference-set-v1".into(), resolved.text[0].clone()),
                serde_json::json!({"reference_set": resolved.text[0]}),
            )
        } else {
            let selector = node.selector()?;
            (
                input_fingerprint(&selector.to_input(&resolved)),
                selector.to_model_input(&resolved),
            )
        };
        let text = serde_json::to_string(&model).map_err(std::io::Error::other)?;
        Ok(Self { fingerprint, text })
    }
}

impl NodeContents for HabeasNodeContent {
    // {impl}`requirements.sdk.graph.node_content.fingerprint`
    fn fingerprint(&self) -> Fingerprint {
        self.fingerprint.clone()
    }
    // {impl}`requirements.sdk.graph.node_content.format`
    // {impl}`requirements.sdk.graph.agent-calls.judge.rendering`
    fn format(&self) -> String {
        self.text.clone()
    }
}

fn project_source_path(path: &std::path::Path) -> bool {
    path.components()
        .any(|part| matches!(part, std::path::Component::Normal(_)))
        && path.components().all(|part| {
            matches!(
                part,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
}

// {impl}`requirements.sdk.graph.domain_types`
// {impl}`requirements.sdk.domain`
impl Domain for HabeasDomain {
    fn source_node(
        &self,
        item: &trust_but_verify::outline::OutlineData,
    ) -> Result<HabeasNode, trust_but_verify::sources::SourceError> {
        if !project_source_path(&item.path) {
            return Err(trust_but_verify::sources::SourceError::Invalid(
                "expected a project-relative file".into(),
            ));
        }
        trust_but_verify::outline::OutlineData::new(&item.path, item.selector.clone())
            .map_err(|e| trust_but_verify::sources::SourceError::Invalid(e.to_string()))?;
        let file = item.path.to_str().ok_or_else(|| {
            trust_but_verify::sources::SourceError::Invalid("source path is not UTF-8".into())
        })?;
        Ok(HabeasNode::from(&InputSelector::Outline {
            file: file.to_owned(),
            item: item.selector.clone(),
        }))
    }
    // {impl}`requirements.sdk.sources.source_nodes`
    fn source_address(
        &self,
        data: &Self::NodeData,
    ) -> Option<trust_but_verify::outline::OutlineData> {
        let HabeasNode::Source(id) = data else {
            return None;
        };
        if id.resource.scheme != "repo"
            || !project_source_path(std::path::Path::new(&id.resource.location))
        {
            return None;
        }
        trust_but_verify::outline::OutlineData::new(&id.resource.location, id.selector.clone()).ok()
    }
    fn cache(&self) -> &ValidationCache<Self> {
        &self.outlines.validation
    }
    type NodeData = HabeasNode;
    type EdgeLabel = HabeasEdgeLabel;

    type Error = std::io::Error;

    // {cite}`habeas:util`
    fn node_path(&self, node: &NodeId<Self>) -> NodePath {
        let data = self.cache().node_data(*node).expect("assigned node handle");
        let node = data.as_ref();
        let segments = match node {
            HabeasNode::Requirement(id) => vec!["requirement".into(), id.0.clone()],
            HabeasNode::ReferenceSet(id) => vec!["references".into(), id.0.clone()],
            HabeasNode::RequirementSelection(glob) => vec!["selection".into(), glob.clone()],
            HabeasNode::Definition(name) => vec!["definition".into(), name.clone()],
            HabeasNode::Source(id) => vec![
                "source".into(),
                id.resource.scheme.clone(),
                id.resource.location.clone(),
                id.selector.clone(),
            ],
            HabeasNode::SiteReview {
                requirement,
                source,
            } => vec![
                "site".into(),
                requirement.clone(),
                source.resource.scheme.clone(),
                source.resource.location.clone(),
                source.selector.clone(),
            ],
        };
        NodePath::new(segments)
    }

    // {cite}`habeas:util`
    fn outgoing_edges(
        &self,
        node: &NodeId<Self>,
        label: HabeasEdgeLabel,
    ) -> std::io::Result<Vec<NodeId<Self>>> {
        if let Some(targets) = self
            .relationships
            .read()
            .expect("relationship table poisoned")
            .get(&(*node, label.clone()))
        {
            return Ok(targets.clone());
        }
        let data = self
            .cache()
            .node_data(*node)
            .ok_or_else(|| std::io::Error::other("unassigned node handle"))?;
        let node = data.as_ref();
        let HabeasNode::Requirement(query) = node else {
            return Ok(vec![]);
        };
        if label == HabeasEdgeLabel::Mandatory {
            let sources = crate::authoring::inputs::AssessmentSources::prepare_with_context(
                self.clone(),
                &query.0,
                crate::github::GithubIssues::offline(),
            )?;
            return Ok(sources
                .model_mandatory_inputs()
                .iter()
                .filter_map(crate::inputs::InputSelector::from_input)
                .map(|selector| HabeasNode::from(&selector))
                .filter(|target| target != node)
                .map(|data| self.cache().intern_node(data))
                .collect());
        }
        let role = match label {
            HabeasEdgeLabel::Implements => "impl",
            HabeasEdgeLabel::Cites => {
                let catalog = self.outlines.reference_catalog(&self.root);
                let catalog = catalog
                    .as_ref()
                    .as_ref()
                    .map_err(|error| std::io::Error::other(error.clone()))?;
                return Ok(self
                    .citation_targets(catalog.references.iter().filter(|r| r.name == query.0))?
                    .into_iter()
                    .filter(|target| target != node)
                    .map(|target| self.cache().intern_node(target))
                    .collect());
            }
            HabeasEdgeLabel::Tests => "test",
            HabeasEdgeLabel::Acknowledges => "fixme",
            HabeasEdgeLabel::Mandatory => unreachable!(),
        };
        Ok(self
            .reference_entries(query)?
            .into_iter()
            .filter(|entry| entry.role == role)
            .map(|entry| {
                HabeasNode::Source(OutlineId {
                    resource: ResourceId {
                        scheme: "repo".into(),
                        location: entry.file,
                    },
                    selector: entry.item,
                })
            })
            .map(|data| self.cache().intern_node(data))
            .collect())
    }

    // {impl}`requirements.sdk.habeas.evidence_format.loading`
    async fn load_evidence(
        &self,
        node: &NodeId<Self>,
        criterion: &CriterionPath,
    ) -> std::io::Result<Option<trust_but_verify::graph::Record<HabeasNode, HabeasEdgeLabel>>> {
        let data = self
            .cache()
            .node_data(*node)
            .ok_or_else(|| std::io::Error::other("unassigned node handle"))?;
        let node = data.as_ref();
        if criterion != &node.review_criterion() {
            return Ok(None);
        }
        let Some(path) = node.evidence_path(&self.root) else {
            return Ok(None);
        };
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let value = serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
        crate::evidence_format::decode(&value, node).map(Some)
    }

    // {cite}`habeas:util`
    async fn store_evidence(
        &self,
        node: &NodeId<Self>,
        criterion: &CriterionPath,
        record: &trust_but_verify::graph::Record<HabeasNode, HabeasEdgeLabel>,
    ) -> std::io::Result<()> {
        let data = self
            .cache()
            .node_data(*node)
            .ok_or_else(|| std::io::Error::other("unassigned node handle"))?;
        let node = data.as_ref();
        if criterion != &node.review_criterion() {
            return Err(std::io::Error::other("unsupported Habeas review criterion"));
        }
        let path = node
            .evidence_path(&self.root)
            .ok_or_else(|| std::io::Error::other("this source node has no Habeas review record"))?;
        crate::evidence_format::write_atomic(&path, &crate::evidence_format::encode(record)?)
    }

    type NodeContent = HabeasNodeContent;

    // {impl}`requirements.sdk.graph.node_content`
    fn node_content(&self, node: &NodeId<Self>) -> std::io::Result<HabeasNodeContent> {
        let data = self
            .cache()
            .node_data(*node)
            .ok_or_else(|| std::io::Error::other("unassigned node handle"))?;
        let node = data.as_ref();
        if let HabeasNode::Source(_) = node {
            let InputSelector::Outline { file, item } = node.selector()? else {
                unreachable!()
            };
            let outline = self.outlines.source_node(&self.root, &file, &item)?;
            return Ok(HabeasNodeContent {
                fingerprint: outline.fingerprint(),
                text: outline.format(),
            });
        }
        let resolved = if let HabeasNode::ReferenceSet(id) = node {
            ResolvedInput {
                text: vec![self.reference_set(id)?],
                ranges: vec![],
                matches: vec![],
                node: None,
            }
        } else {
            node.selector()?
                .resolve(&self.root, &self.definitions, &self.outlines)
                .map_err(std::io::Error::other)?
        };
        HabeasNodeContent::prepare(node, resolved)
    }
}

impl HabeasDomain {
    /// Discover the repository using the cache this domain retains for its reviews.
    // {impl}`requirements.sdk.habeas.graph_discovery`
    pub(crate) fn discover(root: &Path) -> std::io::Result<Self> {
        let mut domain = Self {
            root: std::fs::canonicalize(root)?.into(),
            definitions: Default::default(),
            outlines: OutlineCache::default(),
            relationships: Default::default(),
        };
        domain.definitions = Arc::new(crate::evidence::load_definition_snapshots(
            &domain.root,
            &domain.outlines,
        )?);
        domain.populate_graph()?;
        Ok(domain)
    }

    fn citation_targets<'a>(
        &self,
        references: impl Iterator<Item = &'a crate::evidence::SourceReference>,
    ) -> std::io::Result<Vec<HabeasNode>> {
        let mut targets = Vec::new();
        for reference in references.filter(|r| r.kind == crate::ReferenceKind::Cite) {
            let selector = crate::authoring::inputs::reference_selector(
                &self.root,
                &self.definitions,
                &self.outlines,
                reference,
            )?;
            let target = HabeasNode::from(&selector);
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
        Ok(targets)
    }

    pub(crate) fn populate_graph(&mut self) -> std::io::Result<()> {
        let catalog = self.outlines.reference_catalog(&self.root);
        let catalog = catalog
            .as_ref()
            .as_ref()
            .map_err(|error| std::io::Error::other(error.clone()))?;
        let mut incoming = std::collections::HashMap::<&str, Vec<_>>::new();
        for reference in &catalog.references {
            incoming.entry(&reference.name).or_default().push(reference);
        }
        let definitions = self.definitions.clone();
        for (name, declaration) in definitions.iter() {
            if declaration.kind == crate::DefinitionKind::Term {
                self.cache()
                    .intern_node(HabeasNode::Definition(name.clone()));
                continue;
            }
            let subject = HabeasNode::Requirement(RequirementId(name.clone()));
            self.cache().intern_node(subject.clone());
            // A failed distributed query is reported when its requirement is inspected.
            let Ok(sites) = crate::evidence::definition_sites(
                &self.root,
                name,
                declaration,
                &catalog.files,
                &self.outlines,
            ) else {
                continue;
            };
            let references = incoming.remove(name.as_str()).unwrap_or_default();
            let citations = self.citation_targets(references.iter().copied())?;
            self.install_edges(
                subject.clone(),
                HabeasEdgeLabel::Cites,
                citations.into_iter(),
            );
            let mut entries = crate::evidence::reference_set_entries(
                &self.root,
                &self.outlines,
                name,
                declaration,
                references,
                &sites,
            );
            entries.sort();
            for (role, label) in [
                ("impl", HabeasEdgeLabel::Implements),
                ("test", HabeasEdgeLabel::Tests),
                ("fixme", HabeasEdgeLabel::Acknowledges),
            ] {
                self.install_edges(
                    subject.clone(),
                    label,
                    entries
                        .iter()
                        .filter(|entry| entry.role == role)
                        .map(|entry| {
                            HabeasNode::from(&InputSelector::Outline {
                                file: entry.file.clone(),
                                item: entry.item.clone(),
                            })
                        }),
                );
            }
        }
        Ok(())
    }

    pub(crate) fn new(
        root: &Path,
        definitions: &BTreeMap<String, Declaration>,
        outlines: &OutlineCache,
    ) -> Self {
        Self {
            root: root.into(),
            definitions: Arc::new(definitions.clone()),
            outlines: outlines.clone(),
            relationships: Default::default(),
        }
    }

    // {impl}`requirements.sdk.habeas.graph_discovery`
    pub(crate) fn install_edges(
        &mut self,
        source: HabeasNode,
        label: HabeasEdgeLabel,
        targets: impl Iterator<Item = HabeasNode>,
    ) {
        let source = self.cache().intern_node(source);
        let targets = targets
            .map(|target| self.cache().intern_node(target))
            .filter(|target| *target != source)
            .collect();
        self.relationships
            .write()
            .expect("relationship table poisoned")
            .insert((source, label), targets);
    }

    // {cite}`habeas:util`
    pub(crate) fn reference_entries(
        &self,
        query: &RequirementId,
    ) -> std::io::Result<Vec<crate::evidence::ReferenceSetEntry>> {
        let definition = self
            .definitions
            .get(&query.0)
            .ok_or_else(|| std::io::Error::other(format!("no requirement {:?}", query.0)))?;
        let catalog = self.outlines.reference_catalog(&self.root);
        let catalog = catalog
            .as_ref()
            .as_ref()
            .map_err(|error| std::io::Error::other(error.clone()))?;
        let references = &catalog.references;
        let files = &catalog.files;
        let sites = crate::evidence::definition_sites(
            &self.root,
            &query.0,
            definition,
            &files,
            &self.outlines,
        )?;
        let mut entries = crate::evidence::reference_set_entries(
            &self.root,
            &self.outlines,
            &query.0,
            definition,
            references,
            &sites,
        );
        entries.sort();
        Ok(entries)
    }
    // {cite}`habeas:util`
    pub(crate) fn reference_set(&self, query: &RequirementId) -> std::io::Result<String> {
        let entries = self.reference_entries(query)?;
        Ok(crate::evidence::digest_definition_entries(
            &self.definitions[&query.0],
            entries,
        ))
    }
}

impl HabeasNode {
    // {cite}`habeas:util`
    pub(crate) fn review_subject(subject: &crate::authoring::AuthoringSubject) -> Self {
        use crate::authoring::AuthoringSubject;
        let source = |file: &str, item: &str| OutlineId {
            resource: ResourceId {
                scheme: "repo".into(),
                location: file.into(),
            },
            selector: item.into(),
        };
        match subject {
            AuthoringSubject::Definition(name) => Self::Requirement(RequirementId(name.clone())),
            AuthoringSubject::Citation { file, item } => Self::Source(source(file, item)),
            AuthoringSubject::Site {
                requirement,
                file,
                item,
            } => Self::SiteReview {
                requirement: requirement.clone(),
                source: source(file, item),
            },
        }
    }

    // {impl}`requirements.sdk.habeas.citation_criterion`
    pub(crate) fn review_criterion(&self) -> CriterionPath {
        match self {
            Self::Source(_) => CriterionPath::new(vec!["citation-review".into()]),
            _ => review_criterion(),
        }
    }

    // {cite}`habeas:util`
    pub(crate) fn evidence_path(&self, root: &Path) -> Option<std::path::PathBuf> {
        match self {
            Self::Requirement(id) => Some(root.join("evidence").join(format!("{}.json", id.0))),
            Self::Source(id) if id.resource.scheme == "repo" => Some(root.join(
                crate::citations::review_path(&id.resource.location, &id.selector),
            )),
            Self::SiteReview {
                requirement,
                source,
            } => Some(root.join(crate::sites::review_path(
                requirement,
                &source.resource.location,
                &source.selector,
            ))),
            _ => None,
        }
    }
}

pub(crate) const REVIEW_VERSION: &str = "habeas-review-v1";

// {impl}`requirements.sdk.graph.criteria.criterion_trait.version`
pub(crate) fn review_version() -> CriterionVersion {
    CriterionVersion::new(REVIEW_VERSION)
}

// {impl}`requirements.sdk.graph.criteria.criterion_path`
pub(crate) fn review_criterion() -> CriterionPath {
    CriterionPath::new(vec!["review".into()])
}

// {cite}`habeas:util`
pub(crate) fn input_fingerprint(input: &serde_json::Value) -> Fingerprint {
    use sha2::{Digest, Sha256};
    Fingerprint::new(
        "habeas-input-v1".into(),
        Sha256::digest(serde_json::to_vec(input).expect("JSON input"))
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}

// {cite}`habeas:util`
/// CI resolves inputs through the same model-free API used by authoring.
/// Habeas subsequently reports its detailed, field-specific diagnostics.
pub(crate) fn resolve_input(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    outlines: &OutlineCache,
    selector: &InputSelector,
) -> Result<ResolvedInput, String> {
    selector.resolve(root, definitions, outlines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use trust_but_verify::graph::Dependency;
    use trust_but_verify::graph::Record;

    // {test}`requirements.sdk.habeas.graph_discovery`
    #[test]
    fn a_reference_discovered_before_its_declaration_keeps_its_node() {
        let root = crate::authoring::inputs::tests::project();
        fs::write(
            root.path().join("src/lib.rs"),
            "// {impl}`requirements.ready`\nfn helper() {}\n",
        )
        .unwrap();
        let mut domain =
            HabeasDomain::new(root.path(), &Default::default(), &OutlineCache::default());
        assert!(domain.definitions.is_empty());
        assert!(domain.relationships.read().unwrap().is_empty());
        let catalog = domain.outlines.reference_catalog(root.path());
        let reference = &catalog.as_ref().as_ref().unwrap().references[0];
        let from_reference = domain
            .cache()
            .intern_node(HabeasNode::Requirement(RequirementId(
                reference.name.clone(),
            )));
        domain.definitions = Arc::new(
            crate::evidence::load_definition_snapshots(root.path(), &domain.outlines).unwrap(),
        );
        domain.populate_graph().unwrap();
        let (name, _) = domain
            .definitions
            .get_key_value("requirements.ready")
            .unwrap();
        let from_declaration = domain
            .cache()
            .intern_node(HabeasNode::Requirement(RequirementId(name.clone())));
        assert_eq!(from_reference, from_declaration);
        let targets = domain
            .outgoing_edges(&from_reference, HabeasEdgeLabel::Implements)
            .unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(
            domain.node_path(&targets[0]).segments(),
            ["source", "repo", "src/lib.rs", "fn[helper]"]
        );
    }

    // {test}`requirements.sdk.domain`
    // {test}`requirements.sdk.graph.agent-calls.judge.rendering`
    // {test}`requirements.sdk.graph.node_content.format`
    #[test]
    fn domain_formats_supplied_contents_without_resolving_a_replacement() {
        let root = tempfile::tempdir().unwrap();
        let node = HabeasNode::from(&InputSelector::Outline {
            file: "missing.rs".into(),
            item: "fn[old]".into(),
        });
        let contents = ResolvedInput {
            text: vec!["already read source".into()],
            matches: vec![],
            ranges: vec![],
            node: Some(crate::inputs::ResolvedNode {
                path: root.path().join("missing.rs"),
                index: 0,
                kind: "fn".into(),
                hash: "supplied-snapshot".into(),
            }),
        };
        let contents = HabeasNodeContent::prepare(&node, contents).unwrap();
        let text = contents.format();
        assert!(text.contains("already read source"));
        assert!(text.contains("missing.rs"));
    }

    // {test}`requirements.sdk.sources.source_nodes`
    // {test}`requirements.sdk.graph.node_path_method`
    // {test}`requirements.sdk.graph.node_path_struct`
    // {test}`requirements.sdk.graph.domain_types`
    #[test]
    fn habeas_paths_preserve_opaque_segments_and_separate_all_node_kinds() {
        let root = tempfile::tempdir().unwrap();
        let definitions = BTreeMap::new();
        let outlines = OutlineCache::default();
        let domain = HabeasDomain::new(root.path(), &definitions, &outlines);
        let source = OutlineId {
            resource: ResourceId {
                scheme: "generated".into(),
                location: "opaque/../é".into(),
            },
            selector: "".into(),
        };
        let method = trust_but_verify::outline::OutlineData::new(
            "src/parser.rs",
            "impl[Parser]/fn[parse]".into(),
        )
        .unwrap();
        assert_eq!(
            domain.source_address(&domain.source_node(&method).unwrap()),
            Some(method.clone())
        );
        for (path, selector) in [
            ("../outside.rs", ""),
            ("/absolute.rs", ""),
            ("src/parser.rs", "fn["),
            ("", ""),
        ] {
            let mut address = method.clone();
            address.path = path.into();
            address.selector = selector.into();
            assert!(domain.source_node(&address).is_err());
            let data = HabeasNode::from(&InputSelector::Outline {
                file: path.into(),
                item: selector.into(),
            });
            assert!(domain.source_address(&data).is_none());
        }
        let cases = [
            (
                HabeasNode::Requirement(RequirementId("same".into())),
                vec!["requirement", "same"],
            ),
            (
                HabeasNode::ReferenceSet(RequirementId("same".into())),
                vec!["references", "same"],
            ),
            (
                HabeasNode::RequirementSelection("same".into()),
                vec!["selection", "same"],
            ),
            (
                HabeasNode::Definition("same".into()),
                vec!["definition", "same"],
            ),
            (
                HabeasNode::Source(source.clone()),
                vec!["source", "generated", "opaque/../é", ""],
            ),
            (
                HabeasNode::SiteReview {
                    requirement: "same".into(),
                    source: source.clone(),
                },
                vec!["site", "same", "generated", "opaque/../é", ""],
            ),
        ];
        let mut paths = std::collections::HashSet::new();
        for (node, expected) in cases {
            assert_eq!(domain.source_address(&node), None);
            let path = domain.node_path(&domain.cache().intern_node(node.clone()));
            assert_eq!(path.segments(), expected);
            assert!(paths.insert(path.clone()));
            let fresh_outlines = OutlineCache::default();
            let reconstructed =
                HabeasDomain::new(&domain.root, &domain.definitions, &fresh_outlines);
            assert_eq!(
                reconstructed.node_path(&reconstructed.cache().intern_node(node.clone())),
                path
            );
        }
        for changed in [
            OutlineId {
                resource: ResourceId {
                    scheme: "repo".into(),
                    ..source.resource.clone()
                },
                ..source.clone()
            },
            OutlineId {
                resource: ResourceId {
                    location: "other".into(),
                    ..source.resource.clone()
                },
                ..source.clone()
            },
            OutlineId {
                selector: "fn[kept]".into(),
                ..source
            },
        ] {
            assert!(paths.insert(
                domain.node_path(&domain.cache().intern_node(HabeasNode::Source(changed)))
            ));
        }
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }

    // {test}`requirements.sdk.graph-edges.habeas-test`
    // {test}`requirements.sdk.graph-edges.outgoing_edges`
    #[test]
    fn test_edges_select_only_tests_for_the_requested_requirement() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("md")).unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(root.path().join("book.toml"), "[book]\nsrc='md'\n").unwrap();
        fs::write(
            root.path().join("md/requirements.md"),
            ":::{req} ready\nReady works.\n:::\n:::{req} other\nOther works.\n:::\n",
        )
        .unwrap();
        fs::write(
            root.path().join("src/lib.rs"),
            "// {impl}`requirements.ready`\nfn implementation() {}\n\
             // {test}`requirements.ready`\n#[test]\nfn selected() {}\n\
             // {test}`requirements.other`\n#[test]\nfn unrelated() {}\n",
        )
        .unwrap();
        let definitions = crate::evidence::load_definitions(root.path()).unwrap();
        let outlines = OutlineCache::default();
        let domain = HabeasDomain::new(root.path(), &definitions, &outlines);
        let requirement = HabeasNode::Requirement(RequirementId("requirements.ready".into()));
        let selected = domain
            .outgoing_edges(
                &domain.cache().intern_node(requirement.clone()),
                HabeasEdgeLabel::Tests,
            )
            .unwrap();
        assert_eq!(
            selected
                .iter()
                .map(|id| (*domain.cache().node_data(*id).unwrap()).clone())
                .collect::<Vec<_>>(),
            vec![HabeasNode::Source(OutlineId {
                resource: ResourceId {
                    scheme: "repo".into(),
                    location: "src/lib.rs".into(),
                },
                selector: "fn[selected]".into(),
            })]
        );
        let implementations = domain
            .outgoing_edges(
                &domain.cache().intern_node(requirement.clone()),
                HabeasEdgeLabel::Implements,
            )
            .unwrap();
        assert_eq!(implementations.len(), 1);
        assert_ne!(selected, implementations);
        assert!(
            domain
                .outgoing_edges(&selected[0], HabeasEdgeLabel::Tests)
                .unwrap()
                .is_empty()
        );
    }

    // {test}`requirements.sdk.graph.evidence.evidence_storage`
    // {test}`requirements.sdk.habeas.evidence_format`
    // {test}`requirements.sdk.habeas.evidence_format.loading`
    // {test}`requirements.sdk.habeas.evidence_format.errors`
    // {test}`requirements.sdk.habeas.evidence_conversion.atomic`
    #[tokio::test]
    async fn domain_storage_round_trips_each_review_kind_and_preserves_records_on_errors() {
        let root = tempfile::tempdir().unwrap();
        let definitions = BTreeMap::new();
        let outlines = OutlineCache::default();
        let domain = HabeasDomain::new(root.path(), &definitions, &outlines);
        let source = OutlineId {
            resource: ResourceId {
                scheme: "repo".into(),
                location: "src/lib.rs".into(),
            },
            selector: "fn[kept]".into(),
        };
        let requirement = HabeasNode::Requirement(RequirementId("requirements.ready".into()));
        let cases = [
            (
                requirement.clone(),
                serde_json::json!({
                    "requirement":"The project MUST be ready.", "references":"0".repeat(64), "inputs":[]
                }),
            ),
            (
                HabeasNode::Source(source.clone()),
                serde_json::json!({
                    "file":"src/lib.rs", "item":"fn[kept]", "citations":[], "inputs":[]
                }),
            ),
            (
                HabeasNode::SiteReview {
                    requirement: "requirements.ready".into(),
                    source: source.clone(),
                },
                serde_json::json!({
                    "requirement":"The project MUST be ready.", "file":"src/lib.rs", "item":"fn[kept]",
                    "queries":[], "applies":true, "fixmes":[], "inputs":[]
                }),
            ),
        ];
        for (node, mut metadata) in cases {
            assert!(
                domain
                    .load_evidence(
                        &domain.cache().intern_node(node.clone()),
                        &node.review_criterion()
                    )
                    .await
                    .unwrap()
                    .is_none()
            );
            for result in [false, true] {
                metadata["determination"] = serde_json::json!(result);
                metadata["rationale"] = serde_json::json!("fixture review");
                let record = Record::new(
                    Some(CriterionVersion::new("test-review-v2")),
                    vec![Dependency::Node(
                        requirement.clone(),
                        Fingerprint::new("fixture".into(), "retained".into()),
                    )],
                    metadata.clone(),
                );
                domain
                    .store_evidence(
                        &domain.cache().intern_node(node.clone()),
                        &node.review_criterion(),
                        &record,
                    )
                    .await
                    .unwrap();
                let loaded = domain
                    .load_evidence(
                        &domain.cache().intern_node(node.clone()),
                        &node.review_criterion(),
                    )
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(loaded.version, record.version);
                assert_eq!(loaded.dependencies, record.dependencies);
                assert_eq!(loaded.output, record.output);
                let path = node.evidence_path(root.path()).unwrap();
                let original = fs::read(&path).unwrap();
                let other_criterion =
                    CriterionPath::new(vec!["another".into(), "criterion".into()]);
                assert!(
                    domain
                        .load_evidence(&domain.cache().intern_node(node.clone()), &other_criterion)
                        .await
                        .unwrap()
                        .is_none()
                );
                assert!(
                    domain
                        .store_evidence(
                            &domain.cache().intern_node(node.clone()),
                            &other_criterion,
                            &record
                        )
                        .await
                        .is_err()
                );
                assert_eq!(fs::read(&path).unwrap(), original);
                let mut inconsistent = record.clone();
                inconsistent.output["determination"] = serde_json::json!("invalid");
                assert!(
                    domain
                        .store_evidence(
                            &domain.cache().intern_node(node.clone()),
                            &node.review_criterion(),
                            &inconsistent
                        )
                        .await
                        .is_err()
                );
                assert_eq!(fs::read(&path).unwrap(), original);
                for no_record in [
                    HabeasNode::Source(source.clone()),
                    HabeasNode::Definition("term".into()),
                    HabeasNode::RequirementSelection("*".into()),
                ] {
                    assert!(
                        domain
                            .store_evidence(
                                &domain.cache().intern_node(no_record.clone()),
                                &crate::sdk::review_criterion(),
                                &record
                            )
                            .await
                            .is_err()
                    );
                    assert!(
                        domain
                            .load_evidence(
                                &domain.cache().intern_node(no_record.clone()),
                                &crate::sdk::review_criterion()
                            )
                            .await
                            .unwrap()
                            .is_none()
                    );
                }
            }
        }
        let path = requirement.evidence_path(root.path()).unwrap();
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        let record = Record::new(
            Some(CriterionVersion::new("test-review-v2")),
            vec![],
            serde_json::json!({
                "requirement":"ready", "references":"0".repeat(64), "inputs":[],
                "determination":true, "rationale":"fixture review"
            }),
        );
        assert!(
            domain
                .store_evidence(
                    &domain.cache().intern_node(requirement.clone()),
                    &crate::sdk::review_criterion(),
                    &record
                )
                .await
                .is_err()
        );
        assert!(path.is_dir());
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 3);
    }

    // {test}`requirements.sdk.graph.evidence.checking_records`
    // {test}`requirements.sdk.graph.domain_types`
    // {test}`requirements.sdk.graph.node_content`
    // {test}`requirements.sdk.graph.node_content.fingerprint`
    // {test}`requirements.sdk.graph.snapshot`
    #[test]
    fn requirement_edges_and_input_freshness_use_the_sdk() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("md")).unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(root.path().join("book.toml"), "[book]\nsrc='md'\n").unwrap();
        fs::write(
            root.path().join("md/requirements.md"),
            ":::{req} ready\nThe project MUST be ready.\n:::\n",
        )
        .unwrap();
        fs::write(root.path().join("src/lib.rs"), "fn ready() {}\n").unwrap();
        let definitions = crate::evidence::load_definitions(root.path()).unwrap();
        let outlines = OutlineCache::default();
        let domain = HabeasDomain::new(root.path(), &definitions, &outlines);
        let query = RequirementId("requirements.ready".into());
        let subject = HabeasNode::Requirement(query.clone());
        let ctx = ValidationContext::new(&domain, domain.cache().intern_node(subject.clone()));
        let references = HabeasNode::ReferenceSet(query.clone());
        let retained_references = ctx
            .read(&ctx.graph().cache().intern_node(references.clone()))
            .unwrap();
        let reference_fingerprint = retained_references.fingerprint();
        assert_eq!(
            reference_fingerprint,
            Fingerprint::new(
                "habeas-reference-set-v1".into(),
                domain
                    .reference_set(&RequirementId("requirements.ready".into()))
                    .unwrap()
            )
        );
        assert!(
            ctx.outgoing_edges(
                &ctx.graph().cache().intern_node(subject.clone()),
                HabeasEdgeLabel::Implements
            )
            .unwrap()
            .content
            .is_empty()
        );
        let selector = InputSelector::Requirements {
            requirements: query.0.clone(),
        };
        let retained = ctx
            .read(&ctx.graph().cache().intern_node(HabeasNode::from(&selector)))
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&retained.format()).unwrap(),
            serde_json::json!({
                "matches":["requirements.ready"],
                "requirements":"requirements.ready",
                "text":[":::{req} ready", "The project MUST be ready.", ":::"]
            })
        );
        assert_eq!(
            retained.fingerprint(),
            Fingerprint::new(
                "habeas-input-v1".into(),
                "a60363aa2a9f4d79aff6d511c59e574752e36810da394f6087a72c164adabf7c".into()
            )
        );
        assert_eq!(
            domain
                .node_content(&domain.cache().intern_node(HabeasNode::from(&selector)))
                .unwrap()
                .fingerprint(),
            retained.fingerprint(),
        );
        let record = ctx
            .finish(
                CriterionVersion::new("test-review-v1"),
                serde_json::json!({"determination": true, "rationale": "ready"}),
            )
            .unwrap();
        assert!(
            record
                .is_current(&domain, &CriterionVersion::new("test-review-v1"))
                .unwrap()
        );
        let reference = format!("// {{{}}}`requirements.ready`\nfn ready() {{}}\n", "impl");
        fs::write(root.path().join("src/lib.rs"), reference).unwrap();
        let fresh_outlines = OutlineCache::default();
        let changed = HabeasDomain::new(&domain.root, &domain.definitions, &fresh_outlines);
        assert_eq!(retained_references.fingerprint(), reference_fingerprint);
        assert_ne!(
            changed
                .node_content(&changed.cache().intern_node(references.clone()))
                .unwrap()
                .fingerprint(),
            reference_fingerprint
        );
        assert!(
            !record
                .is_current(&changed, &CriterionVersion::new("test-review-v1"))
                .unwrap()
        );
        let snapshot = changed
            .outgoing_edges(
                &changed.cache().intern_node(subject.clone()),
                HabeasEdgeLabel::Implements,
            )
            .unwrap();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(
            snapshot[0],
            changed
                .cache()
                .intern_node(HabeasNode::from(&InputSelector::Outline {
                    file: "src/lib.rs".into(),
                    item: "fn[ready]".into()
                }))
        );
        let source_node = snapshot[0].clone();
        let retained_source = changed.node_content(&source_node).unwrap();
        let retained_fingerprint = retained_source.fingerprint();
        fs::write(
            root.path().join("src/lib.rs"),
            "// {impl}`requirements.ready`\nfn ready() { changed(); }\n",
        )
        .unwrap();
        let current_outlines = OutlineCache::default();
        let current = HabeasDomain::new(&changed.root, &changed.definitions, &current_outlines);
        assert_eq!(retained_source.fingerprint(), retained_fingerprint);
        let data = changed.cache().node_data(source_node).unwrap();
        let current_source = current
            .node_content(&current.cache().intern_node((*data).clone()))
            .unwrap();
        assert_ne!(current_source.fingerprint(), retained_fingerprint);
    }
}

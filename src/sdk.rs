//! Habeas's interpretation of the SDK graph. Wire formats, selection rules,
//! and findings remain Habeas policy.

use crate::{
    evidence::Declaration,
    inputs::{InputSelector, OutlineCache, ResolvedInput},
};
use std::{collections::BTreeMap, path::Path};
use trust_but_verify::domain::Domain;
use trust_but_verify::domain::NodeContents;
use trust_but_verify::graph::CriterionPath;
use trust_but_verify::graph::Fingerprint;
use trust_but_verify::graph::InspectionContext;
use trust_but_verify::graph::NodePath;
use trust_but_verify::resource::OutlineId;
use trust_but_verify::resource::ResourceId;

// {cite}`habeas:util`
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct RequirementId(pub String);

// {cite}`habeas:util`
/// Habeas node identities use the SDK's common outline address for sources.
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
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum HabeasNode {
    Requirement(RequirementId),
    ReferenceSet(RequirementId),
    RequirementSelection(String),
    Definition(String),
    Source(OutlineId),
    CitationReview(OutlineId),
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
            Self::Source(source)
            | Self::CitationReview(source)
            | Self::SiteReview { source, .. } => {
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
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum HabeasEdgeLabel {
    Implements,
    Tests,
    Acknowledges,
    Mandatory,
}

// {cite}`habeas:util`
#[derive(Clone)]
pub(crate) struct HabeasDomain<'a> {
    pub root: &'a Path,
    pub definitions: &'a BTreeMap<String, Declaration>,
    pub outlines: &'a OutlineCache,
}

pub(crate) struct HabeasNodeContent {
    pub resolved: ResolvedInput,
    fingerprint: Fingerprint,
    text: String,
}

impl HabeasNodeContent {
    fn prepare(node: &HabeasNode, resolved: ResolvedInput) -> std::io::Result<Self> {
        let (fingerprint, model) = if matches!(node, HabeasNode::ReferenceSet(_)) {
            (
                Fingerprint {
                    scheme: "habeas-reference-set-v1".into(),
                    value: resolved.text[0].clone(),
                },
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
        Ok(Self {
            resolved,
            fingerprint,
            text,
        })
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

// {impl}`requirements.sdk.graph.domain_types`
// {impl}`requirements.sdk.domain`
impl Domain for HabeasDomain<'_> {
    type NodeId = HabeasNode;
    type EdgeLabel = HabeasEdgeLabel;

    type Error = std::io::Error;

    // {cite}`habeas:util`
    fn node_path(&self, node: &HabeasNode) -> NodePath {
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
            HabeasNode::CitationReview(id) => vec![
                "citation".into(),
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
        node: &HabeasNode,
        label: HabeasEdgeLabel,
    ) -> std::io::Result<Vec<HabeasNode>> {
        let HabeasNode::Requirement(query) = node else {
            return Ok(vec![]);
        };
        if label == HabeasEdgeLabel::Mandatory {
            let sources = crate::authoring::inputs::AssessmentSources::prepare_with_context(
                self.root,
                &query.0,
                crate::github::GithubIssues::offline(),
                self.definitions,
                self.outlines.clone(),
            )?;
            return Ok(sources
                .model_mandatory_inputs()
                .iter()
                .filter_map(crate::inputs::InputSelector::from_input)
                .map(|selector| HabeasNode::from(&selector))
                .filter(|target| target != node)
                .collect());
        }
        let role = match label {
            HabeasEdgeLabel::Implements => "impl",
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
            .collect())
    }

    // {impl}`requirements.sdk.habeas.evidence_format.loading`
    async fn load_evidence(
        &self,
        node: &HabeasNode,
        criterion: &CriterionPath,
    ) -> std::io::Result<Option<trust_but_verify::graph::Record<HabeasNode, HabeasEdgeLabel>>> {
        if criterion != &review_criterion() {
            return Ok(None);
        }
        let Some(path) = node.evidence_path(self.root) else {
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
        node: &HabeasNode,
        criterion: &CriterionPath,
        record: &trust_but_verify::graph::Record<HabeasNode, HabeasEdgeLabel>,
    ) -> std::io::Result<()> {
        if criterion != &review_criterion() {
            return Err(std::io::Error::other("unsupported Habeas review criterion"));
        }
        let path = node
            .evidence_path(self.root)
            .ok_or_else(|| std::io::Error::other("this source node has no Habeas review record"))?;
        crate::evidence_format::write_atomic(&path, &crate::evidence_format::encode(record)?)
    }

    type NodeContent = HabeasNodeContent;

    // {impl}`requirements.sdk.graph.node_content`
    fn node_content(&self, node: &HabeasNode) -> std::io::Result<HabeasNodeContent> {
        let resolved = if let HabeasNode::ReferenceSet(id) = node {
            ResolvedInput {
                text: vec![self.reference_set(id)?],
                ranges: vec![],
                matches: vec![],
                node: None,
            }
        } else {
            node.selector()?
                .resolve(self.root, self.definitions, self.outlines)
                .map_err(std::io::Error::other)?
        };
        HabeasNodeContent::prepare(node, resolved)
    }
}

impl HabeasDomain<'_> {
    // {cite}`habeas:util`
    pub(crate) fn reference_entries(
        &self,
        query: &RequirementId,
    ) -> std::io::Result<Vec<crate::evidence::ReferenceSetEntry>> {
        let definition = self
            .definitions
            .get(&query.0)
            .ok_or_else(|| std::io::Error::other(format!("no requirement {:?}", query.0)))?;
        let catalog = self.outlines.reference_catalog(self.root);
        let catalog = catalog
            .as_ref()
            .as_ref()
            .map_err(|error| std::io::Error::other(error.clone()))?;
        let references = &catalog.references;
        let files = &catalog.files;
        let sites = crate::evidence::definition_sites(
            self.root,
            &query.0,
            definition,
            &files,
            self.outlines,
        )?;
        let mut entries = crate::evidence::reference_set_entries(
            self.root,
            self.outlines,
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
            self.root,
            self.outlines,
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
            AuthoringSubject::Citation { file, item } => Self::CitationReview(source(file, item)),
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

    // {cite}`habeas:util`
    pub(crate) fn evidence_path(&self, root: &Path) -> Option<std::path::PathBuf> {
        match self {
            Self::Requirement(id) => Some(root.join("evidence").join(format!("{}.json", id.0))),
            Self::CitationReview(id) => Some(root.join(crate::citations::review_path(
                &id.resource.location,
                &id.selector,
            ))),
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

// {impl}`requirements.sdk.graph.validation.criterion_path`
pub(crate) fn review_criterion() -> CriterionPath {
    CriterionPath::new(vec!["review".into()])
}

// {cite}`habeas:util`
pub(crate) fn input_fingerprint(input: &serde_json::Value) -> Fingerprint {
    use sha2::{Digest, Sha256};
    Fingerprint {
        scheme: "habeas-input-v1".into(),
        value: Sha256::digest(serde_json::to_vec(input).expect("JSON input"))
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    }
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
    let domain = HabeasDomain {
        root,
        definitions,
        outlines,
    };
    InspectionContext::new(&domain)
        .read(&HabeasNode::from(selector))
        .map(|content| content.resolved)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use trust_but_verify::graph::Dependency;
    use trust_but_verify::graph::Record;

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
                kind: "fn",
                hash: "supplied-snapshot".into(),
            }),
        };
        let contents = HabeasNodeContent::prepare(&node, contents).unwrap();
        let text = contents.format();
        assert!(text.contains("already read source"));
        assert!(text.contains("missing.rs"));
    }

    // {test}`requirements.sdk.graph.node_path_method`
    // {test}`requirements.sdk.graph.node_path_struct`
    // {test}`requirements.sdk.graph.domain_types`
    #[test]
    fn habeas_paths_preserve_opaque_segments_and_separate_all_node_kinds() {
        let root = tempfile::tempdir().unwrap();
        let definitions = BTreeMap::new();
        let outlines = OutlineCache::default();
        let domain = HabeasDomain {
            root: root.path(),
            definitions: &definitions,
            outlines: &outlines,
        };
        let source = OutlineId {
            resource: ResourceId {
                scheme: "generated".into(),
                location: "opaque/../é".into(),
            },
            selector: "".into(),
        };
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
                HabeasNode::CitationReview(source.clone()),
                vec!["citation", "generated", "opaque/../é", ""],
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
            let path = domain.node_path(&node);
            assert_eq!(path.segments(), expected);
            assert!(paths.insert(path.clone()));
            let fresh_outlines = OutlineCache::default();
            let reconstructed = HabeasDomain {
                outlines: &fresh_outlines,
                ..domain
            };
            assert_eq!(reconstructed.node_path(&node), path);
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
            assert!(paths.insert(domain.node_path(&HabeasNode::Source(changed))));
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
        let domain = HabeasDomain {
            root: root.path(),
            definitions: &definitions,
            outlines: &outlines,
        };
        let requirement = HabeasNode::Requirement(RequirementId("requirements.ready".into()));
        let selected = domain
            .outgoing_edges(&requirement, HabeasEdgeLabel::Tests)
            .unwrap();
        assert_eq!(
            selected,
            vec![HabeasNode::Source(OutlineId {
                resource: ResourceId {
                    scheme: "repo".into(),
                    location: "src/lib.rs".into(),
                },
                selector: "fn[selected]".into(),
            })]
        );
        let implementations = domain
            .outgoing_edges(&requirement, HabeasEdgeLabel::Implements)
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
        let domain = HabeasDomain {
            root: root.path(),
            definitions: &definitions,
            outlines: &outlines,
        };
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
                HabeasNode::CitationReview(source.clone()),
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
                    .load_evidence(&node, &crate::sdk::review_criterion())
                    .await
                    .unwrap()
                    .is_none()
            );
            for result in [false, true] {
                metadata["determination"] = serde_json::json!(result);
                metadata["rationale"] = serde_json::json!("fixture review");
                let record = Record {
                    revision: "test-review-v2".into(),
                    dependencies: vec![Dependency::Node(
                        requirement.clone(),
                        Fingerprint {
                            scheme: "fixture".into(),
                            value: "retained".into(),
                        },
                    )],
                    output: metadata.clone(),
                };
                domain
                    .store_evidence(&node, &crate::sdk::review_criterion(), &record)
                    .await
                    .unwrap();
                let loaded = domain
                    .load_evidence(&node, &crate::sdk::review_criterion())
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(loaded.revision, record.revision);
                assert_eq!(loaded.dependencies, record.dependencies);
                assert_eq!(loaded.output, record.output);
                let path = node.evidence_path(root.path()).unwrap();
                let original = fs::read(&path).unwrap();
                let other_criterion =
                    CriterionPath::new(vec!["another".into(), "criterion".into()]);
                assert!(
                    domain
                        .load_evidence(&node, &other_criterion)
                        .await
                        .unwrap()
                        .is_none()
                );
                assert!(
                    domain
                        .store_evidence(&node, &other_criterion, &record)
                        .await
                        .is_err()
                );
                assert_eq!(fs::read(&path).unwrap(), original);
                let mut inconsistent = record.clone();
                inconsistent.output["determination"] = serde_json::json!("invalid");
                assert!(
                    domain
                        .store_evidence(&node, &crate::sdk::review_criterion(), &inconsistent)
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
                            .store_evidence(&no_record, &crate::sdk::review_criterion(), &record)
                            .await
                            .is_err()
                    );
                    assert!(
                        domain
                            .load_evidence(&no_record, &crate::sdk::review_criterion())
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
        let record = Record {
            revision: "test-review-v2".into(),
            dependencies: vec![],
            output: serde_json::json!({
                "requirement":"ready", "references":"0".repeat(64), "inputs":[],
                "determination":true, "rationale":"fixture review"
            }),
        };
        assert!(
            domain
                .store_evidence(&requirement, &crate::sdk::review_criterion(), &record)
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
        let domain = HabeasDomain {
            root: root.path(),
            definitions: &definitions,
            outlines: &outlines,
        };
        let query = RequirementId("requirements.ready".into());
        let ctx = InspectionContext::new(&domain);
        let subject = HabeasNode::Requirement(query.clone());
        let references = HabeasNode::ReferenceSet(query.clone());
        let retained_references = ctx.read(&references).unwrap();
        let reference_fingerprint = retained_references.fingerprint();
        assert_eq!(
            reference_fingerprint,
            Fingerprint {
                scheme: "habeas-reference-set-v1".into(),
                value: retained_references.resolved.text[0].clone(),
            }
        );
        assert!(
            ctx.outgoing_edges(&subject, HabeasEdgeLabel::Implements)
                .unwrap()
                .content
                .is_empty()
        );
        let selector = InputSelector::Requirements {
            requirements: query.0.clone(),
        };
        let retained = ctx.read(&HabeasNode::from(&selector)).unwrap();
        assert_eq!(
            selector.to_input(&retained.resolved),
            serde_json::json!({
                "matches":["requirements.ready"],
                "requirements":"requirements.ready",
                "text":[":::{req} ready", "The project MUST be ready.", ":::"]
            })
        );
        assert_eq!(
            retained.fingerprint(),
            Fingerprint {
                scheme: "habeas-input-v1".into(),
                value: "a60363aa2a9f4d79aff6d511c59e574752e36810da394f6087a72c164adabf7c".into(),
            }
        );
        assert_eq!(
            domain
                .node_content(&HabeasNode::from(&selector))
                .unwrap()
                .fingerprint(),
            retained.fingerprint(),
        );
        let record = ctx
            .finish(
                "test-review-v1".into(),
                serde_json::json!({"determination": true, "rationale": "ready"}),
            )
            .unwrap();
        assert!(
            trust_but_verify::graph::record_is_current(&domain, "test-review-v1", &record).unwrap()
        );
        let reference = format!("// {{{}}}`requirements.ready`\nfn ready() {{}}\n", "impl");
        fs::write(root.path().join("src/lib.rs"), reference).unwrap();
        let fresh_outlines = OutlineCache::default();
        let changed = HabeasDomain {
            outlines: &fresh_outlines,
            ..domain
        };
        assert_eq!(retained_references.fingerprint(), reference_fingerprint);
        assert_ne!(
            changed.node_content(&references).unwrap().fingerprint(),
            reference_fingerprint
        );
        assert!(
            !trust_but_verify::graph::record_is_current(&changed, "test-review-v1", &record)
                .unwrap()
        );
        let snapshot = changed
            .outgoing_edges(&subject, HabeasEdgeLabel::Implements)
            .unwrap();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(
            snapshot[0],
            HabeasNode::from(&InputSelector::Outline {
                file: "src/lib.rs".into(),
                item: "fn[ready]".into()
            })
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
        let current = HabeasDomain {
            outlines: &current_outlines,
            ..changed
        };
        assert_eq!(retained_source.fingerprint(), retained_fingerprint);
        let current_source = current.node_content(&source_node).unwrap();
        assert_ne!(current_source.fingerprint(), retained_fingerprint);
    }
}

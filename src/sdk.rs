//! Habeas's interpretation of the SDK graph. Wire formats, selection rules,
//! and findings remain Habeas policy.

use crate::{
    evidence::Declaration,
    inputs::{InputSelector, OutlineCache, ResolvedInput},
};
use habeas_sdk::{
    graph::{Edge, Fingerprint, Fingerprinting, Graph, InspectionContext, NodePath, Snapshot},
    resource::{OutlineId, ResourceId},
};
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct RequirementId(pub String);

/// Habeas node identities use the SDK's common outline address for sources.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum HabeasNode {
    Requirement(RequirementId),
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
    pub(crate) fn selector(&self) -> Result<InputSelector, std::io::Error> {
        Ok(match self {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HabeasEdgeLabel {
    Implements,
    Tests,
    Acknowledges,
    Mandatory,
}

pub(crate) struct HabeasDomain<'a> {
    pub root: &'a Path,
    pub definitions: &'a BTreeMap<String, Declaration>,
    pub outlines: &'a OutlineCache,
}

// {impl}`requirements.sdk.graph.domain_types`
impl Graph for HabeasDomain<'_> {
    type NodeId = HabeasNode;
    type EdgeLabel = HabeasEdgeLabel;
    type NodeContent = ResolvedInput;
    type JudgmentMetadata = serde_json::Value;
    type Error = std::io::Error;

    fn node_content(&self, node: &HabeasNode) -> Result<ResolvedInput, std::io::Error> {
        let node = node.selector()?;
        node.resolve(self.root, self.definitions, self.outlines)
            .map_err(std::io::Error::other)
    }

    fn node_path(&self, node: &HabeasNode) -> NodePath {
        let segments = match node {
            HabeasNode::Requirement(id) => vec!["requirement".into(), id.0.clone()],
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
        NodePath { segments }
    }

    fn outgoing_edges(
        &self,
        node: &HabeasNode,
    ) -> Result<Snapshot<Vec<Edge<HabeasNode, HabeasEdgeLabel>>>, std::io::Error> {
        let HabeasNode::Requirement(query) = node else {
            return Ok(Snapshot {
                content: Vec::new(),
                fingerprint: input_fingerprint(&serde_json::json!([])),
            });
        };
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
        let mut edges = Vec::new();
        for entry in &entries {
            let source = OutlineId {
                resource: ResourceId {
                    scheme: "repo".into(),
                    location: entry.file.clone(),
                },
                selector: entry.item.clone(),
            };
            let label = match entry.role {
                "impl" => HabeasEdgeLabel::Implements,
                "test" => HabeasEdgeLabel::Tests,
                "fixme" => HabeasEdgeLabel::Acknowledges,
                _ => continue,
            };
            edges.push(Edge {
                label,
                target: HabeasNode::Source(source),
            });
        }
        Ok(Snapshot {
            content: edges,
            fingerprint: Fingerprint {
                scheme: "habeas-reference-set-v1".into(),
                value: crate::evidence::digest_entries(entries),
            },
        })
    }
}

impl HabeasNode {
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

// {impl}`requirements.sdk.graph.validation.procedure_identity`
pub(crate) fn review_version(_: &HabeasNode) -> std::io::Result<String> {
    Ok(REVIEW_VERSION.into())
}

// {impl}`requirements.sdk.habeas.evidence_format.loading`
impl habeas_sdk::graph::EvidenceStorage for HabeasDomain<'_> {
    async fn load_evidence(
        &self,
        node: &HabeasNode,
    ) -> std::io::Result<Option<habeas_sdk::graph::Record<HabeasNode, serde_json::Value>>> {
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

    async fn store_evidence(
        &self,
        node: &HabeasNode,
        record: &habeas_sdk::graph::Record<HabeasNode, serde_json::Value>,
    ) -> std::io::Result<()> {
        let path = node
            .evidence_path(self.root)
            .ok_or_else(|| std::io::Error::other("this source node has no Habeas review record"))?;
        crate::evidence_format::write_atomic(&path, &crate::evidence_format::encode(record)?)
    }
}

// {impl}`requirements.sdk.graph.fingerprinting.fingerprint_content`
impl Fingerprinting for HabeasDomain<'_> {
    fn fingerprint(&self, node: &HabeasNode, content: &ResolvedInput) -> Fingerprint {
        input_fingerprint(
            &node
                .selector()
                .expect("resolved Habeas node")
                .to_input(content),
        )
    }
}

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
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // {test}`requirements.sdk.graph.evidence.checking_records`
    // {test}`requirements.sdk.graph.domain_types`
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
        assert!(ctx.outgoing_edges(&subject).unwrap().content.is_empty());
        let selector = InputSelector::Requirements {
            requirements: query.0.clone(),
        };
        ctx.read(&HabeasNode::from(&selector)).unwrap();
        let record = ctx.finish(
            "test-review-v1".into(),
            habeas_sdk::graph::Judgment {
                result: true,
                rationale: "ready".into(),
                metadata: serde_json::Value::Null,
            },
        );
        assert!(habeas_sdk::graph::record_is_current(&domain, "test-review-v1", &record).unwrap());
        let reference = format!("// {{{}}}`requirements.ready`\nfn ready() {{}}\n", "impl");
        fs::write(root.path().join("src/lib.rs"), reference).unwrap();
        let fresh_outlines = OutlineCache::default();
        let changed = HabeasDomain {
            outlines: &fresh_outlines,
            ..domain
        };
        assert!(
            !habeas_sdk::graph::record_is_current(&changed, "test-review-v1", &record).unwrap()
        );
        let snapshot = changed.outgoing_edges(&subject).unwrap();
        assert_eq!(snapshot.content.len(), 1);
        assert_eq!(snapshot.content[0].label, HabeasEdgeLabel::Implements);
        assert_eq!(
            snapshot.content[0].target,
            HabeasNode::from(&InputSelector::Outline {
                file: "src/lib.rs".into(),
                item: "fn[ready]".into()
            })
        );
    }
}

//! Habeas's interpretation of the SDK graph. Wire formats, selection rules,
//! and findings remain Habeas policy.

use crate::{
    evidence::Declaration,
    inputs::{InputSelector, OutlineCache, ResolvedInput},
};
use habeas_sdk::{
    graph::{CheckContext, CheckProcedure, Domain, Edge, Fingerprint, Snapshot},
    resource::{OutlineId, ResourceId},
};
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RequirementId(pub String);

/// Habeas node identities use the SDK's common outline address for sources.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HabeasNode {
    Requirement(RequirementId),
    RequirementSelection(String),
    Definition(String),
    Source(OutlineId),
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
    fn selector(&self) -> Result<InputSelector, std::io::Error> {
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
            Self::Source(source) => {
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

/// Each variant specifies its legal endpoints; no independent edge label is
/// stored. An outline address can identify an entire Markdown declaration too.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HabeasEdge {
    Implements(RequirementId, OutlineId),
    Tests(RequirementId, OutlineId),
    Acknowledges(RequirementId, OutlineId),
}

impl Edge<HabeasNode> for HabeasEdge {
    fn endpoints(&self) -> (HabeasNode, HabeasNode) {
        let (requirement, source) = match self {
            Self::Implements(r, s) | Self::Tests(r, s) | Self::Acknowledges(r, s) => (r, s),
        };
        (
            HabeasNode::Requirement(requirement.clone()),
            HabeasNode::Source(source.clone()),
        )
    }
}

pub(crate) struct HabeasDomain<'a> {
    pub root: &'a Path,
    pub definitions: &'a BTreeMap<String, Declaration>,
    pub outlines: &'a OutlineCache,
}

impl Domain for HabeasDomain<'_> {
    type NodeId = HabeasNode;
    type Edge = HabeasEdge;
    type Content = ResolvedInput;
    type Query = RequirementId;
    type QueryResult = Vec<HabeasEdge>;
    type Error = std::io::Error;

    fn read(&self, node: &HabeasNode) -> Result<Snapshot<ResolvedInput>, std::io::Error> {
        let node = node.selector()?;
        let content = node
            .resolve(self.root, self.definitions, self.outlines)
            .map_err(std::io::Error::other)?;
        // Preserve the existing consumer-defined equality: outline hashes,
        // exact declaration text, and the complete selection for a glob.
        let fingerprint = input_fingerprint(&node.to_input(&content));
        Ok(Snapshot {
            content,
            fingerprint,
        })
    }

    fn query(&self, query: &RequirementId) -> Result<Snapshot<Vec<HabeasEdge>>, std::io::Error> {
        let definition = self
            .definitions
            .get(&query.0)
            .ok_or_else(|| std::io::Error::other(format!("no requirement {:?}", query.0)))?;
        let mut errors = Vec::new();
        let references = crate::evidence::collect_references(self.root, &mut errors);
        let files = crate::evidence::discovered_files(self.root, &mut errors);
        if !errors.is_empty() {
            return Err(std::io::Error::other(errors.join("\n")));
        }
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
            &references,
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
            let edge = match entry.role {
                "impl" => HabeasEdge::Implements(query.clone(), source),
                "test" => HabeasEdge::Tests(query.clone(), source),
                "fixme" => HabeasEdge::Acknowledges(query.clone(), source),
                _ => continue,
            };
            if !edges.contains(&edge) {
                edges.push(edge);
            }
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
pub(crate) struct ResolveInput;

impl CheckProcedure<HabeasDomain<'_>> for ResolveInput {
    type Request = InputSelector;
    type Output = ResolvedInput;

    fn run(
        &self,
        request: &InputSelector,
        ctx: &mut CheckContext<'_, HabeasDomain<'_>>,
    ) -> Result<ResolvedInput, std::io::Error> {
        ctx.read(&HabeasNode::from(request))
    }
}

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
    ResolveInput
        .run(selector, &mut CheckContext::new(&domain))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
        let mut ctx = CheckContext::new(&domain);
        assert!(ctx.query(&query).unwrap().is_empty());
        let selector = InputSelector::Requirements {
            requirements: query.0.clone(),
        };
        ResolveInput.run(&selector, &mut ctx).unwrap();
        let record = ctx.finish("test-review-v1".into(), true);
        assert!(
            CheckContext::new(&domain)
                .record_is_current("test-review-v1", &record)
                .unwrap()
        );
        let reference = format!("// {{{}}}`requirements.ready`\nfn ready() {{}}\n", "impl");
        fs::write(root.path().join("src/lib.rs"), reference).unwrap();
        let fresh_outlines = OutlineCache::default();
        let changed = HabeasDomain {
            outlines: &fresh_outlines,
            ..domain
        };
        assert!(
            !CheckContext::new(&changed)
                .record_is_current("test-review-v1", &record)
                .unwrap()
        );
        let snapshot = changed.query(&query).unwrap();
        assert_eq!(snapshot.content.len(), 1);
        let (from, to) = snapshot.content[0].endpoints();
        assert_eq!(from, HabeasNode::from(&selector));
        assert_eq!(
            to,
            HabeasNode::from(&InputSelector::Outline {
                file: "src/lib.rs".into(),
                item: "fn[ready]".into()
            })
        );
    }
}

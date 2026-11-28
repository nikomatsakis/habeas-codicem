use super::*;
use crate::github::IssueRead;
use std::collections::BTreeSet;
use std::io;
use trust_but_verify::graph::{Criterion, ValidationError};
use trust_but_verify::request::{ValidationContext, VetResult};

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
pub(crate) struct FixmeId {
    pub(crate) file: String,
    pub(crate) item: String,
    pub(crate) subject: String,
    pub(super) ordinal: usize,
}

pub(super) struct Fixme {
    pub(super) body: String,
    issues: BTreeSet<String>,
}
pub(super) type Fixmes = BTreeMap<FixmeId, Fixme>;

impl HabeasDomain {
    fn acknowledgments(&self) -> io::Result<&Fixmes> {
        self.fixmes
            .get_or_init(|| {
                let catalog = self.outlines.reference_catalog(&self.root);
                let catalog = catalog.as_ref().as_ref().map_err(Clone::clone)?;
                let mut items = Fixmes::new();
                for r in &catalog.references {
                    if r.kind != crate::ReferenceKind::Fixme || !r.closed {
                        continue;
                    }
                    let Some(body) = &r.commentary else { continue };
                    let file = crate::evidence::display_path(&self.root, &r.file);
                    let parsed = self.outlines.parse(&self.root, &file);
                    let parsed = parsed.as_ref().as_ref().map_err(Clone::clone)?;
                    let item = if parsed.outline.language == "plain-text" {
                        String::new()
                    } else {
                        parsed.outline.nodes[parsed.reference_owner(r.line, r.column)]
                            .selector
                            .clone()
                    };
                    let mut id = FixmeId {
                        file,
                        item,
                        subject: r.name.clone(),
                        ordinal: 0,
                    };
                    while items.contains_key(&id) {
                        id.ordinal += 1;
                    }
                    let parsed = crate::fixme::FixmeBody::parse(body);
                    items.insert(
                        id,
                        Fixme {
                            body: crate::redaction::redact_text(body).into_owned(),
                            issues: parsed.issues.into_iter().collect(),
                        },
                    );
                }
                Ok(items)
            })
            .as_ref()
            .map_err(|e| io::Error::other(e.clone()))
    }

    pub(crate) fn acknowledgments_for_review(
        &self,
        file: &str,
        item: &str,
        subject: &str,
        body: &str,
    ) -> io::Result<Vec<NodeId<Self>>> {
        Ok(self
            .acknowledgments()?
            .iter()
            .filter(|(id, f)| {
                id.file == file
                    && id.item == (if item == "<root>" { "" } else { item })
                    && id.subject == subject
                    && f.body == body
            })
            .map(|(id, _)| self.cache().intern_node(HabeasNode::Fixme(id.clone())))
            .collect())
    }

    pub(super) fn fixme_edges(
        &self,
        node: &HabeasNode,
        label: HabeasEdgeLabel,
    ) -> io::Result<Vec<NodeId<Self>>> {
        let items = self.acknowledgments()?;
        let nodes = match (node, label) {
            (HabeasNode::Fixme(id), HabeasEdgeLabel::TrackingIssue) => items
                .get(id)
                .into_iter()
                .flat_map(|f| &f.issues)
                .map(|i| HabeasNode::GithubIssue(i.clone()))
                .collect(),
            (HabeasNode::Requirement(id), HabeasEdgeLabel::AcknowledgedGap) => items
                .keys()
                .filter(|f| f.subject == id.0)
                .cloned()
                .map(HabeasNode::Fixme)
                .collect(),
            (HabeasNode::Source(id), HabeasEdgeLabel::ContainsFixme)
                if id.resource.scheme == "repo" =>
            {
                items
                    .keys()
                    .filter(|f| f.file == id.resource.location && f.item == id.selector)
                    .cloned()
                    .map(HabeasNode::Fixme)
                    .collect()
            }
            _ => Vec::new(),
        };
        Ok(nodes
            .into_iter()
            .map(|n| self.cache().intern_node(n))
            .collect())
    }

    pub(super) fn fixme_content(&self, id: &FixmeId) -> io::Result<HabeasNodeContent> {
        let fixme = self
            .acknowledgments()?
            .get(id)
            .ok_or_else(|| io::Error::other("acknowledgment no longer exists"))?;
        Ok(HabeasNodeContent {
            fingerprint: input_fingerprint(&serde_json::json!(fixme.body)),
            text: fixme.body.clone(),
            issue_open: None,
        })
    }

    // {impl}`requirements.authoring.github_reads`
    pub(super) fn issue_content(&self, name: &str) -> io::Result<HabeasNodeContent> {
        let read = self.github.read(name);
        Self::issue_snapshot(name, &read)
    }
    fn issue_snapshot(name: &str, read: &IssueRead) -> io::Result<HabeasNodeContent> {
        let issue = match read {
            IssueRead::Found(issue) => issue,
            IssueRead::Missing => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("issue {name} does not exist"),
                ));
            }
            IssueRead::Failed(reason) => {
                return Err(io::Error::other(format!(
                    "cannot read issue {name}: {reason}"
                )));
            }
        };
        let snapshot = serde_json::json!({"title":crate::redaction::redact_text(&issue.title), "opening_post":crate::redaction::redact_text(&issue.body), "status": if issue.open {"open"} else {"closed"}});
        Ok(HabeasNodeContent {
            fingerprint: input_fingerprint(&snapshot),
            text: format!("{name}\n{snapshot}"),
            issue_open: Some(issue.open),
        })
    }
}

pub(crate) struct OpenIssue(pub NodeId<HabeasDomain>);
impl Criterion<HabeasDomain> for OpenIssue {
    type Ok = bool;
    fn source_node_id(&self) -> NodeId<HabeasDomain> {
        self.0
    }
    fn criterion_path(&self) -> CriterionPath {
        CriterionPath::new(vec!["github-issue-open".into()])
    }
    fn version(&self) -> CriterionVersion {
        CriterionVersion::new("issue-open-v1")
    }
    // {impl}`requirements.sdk.habeas.github_nodes.open`
    async fn execute(
        self,
        cx: &mut ValidationContext<'_, HabeasDomain>,
    ) -> Result<bool, ValidationError<io::Error>> {
        cx.read(&self.0)
            .map_err(ValidationError::Graph)?
            .issue_open
            .ok_or_else(|| ValidationError::Graph(io::Error::other("expected GitHub issue")))
    }
}

pub(crate) struct RelatedIssues(pub NodeId<HabeasDomain>);
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct IssueRelevance {
    pub issue: String,
    pub related: bool,
    pub rationale: String,
}
// {impl}`requirements.sdk.habeas.github_nodes.related`
impl Criterion<HabeasDomain> for RelatedIssues {
    type Ok = Vec<IssueRelevance>;
    fn source_node_id(&self) -> NodeId<HabeasDomain> {
        self.0
    }
    fn criterion_path(&self) -> CriterionPath {
        CriterionPath::new(vec!["fixme-issue-relevance".into()])
    }
    fn version(&self) -> CriterionVersion {
        CriterionVersion::new("fixme-issues-v1")
    }
    // {impl}`requirements.authoring.github_reads`
    async fn execute(
        self,
        cx: &mut ValidationContext<'_, HabeasDomain>,
    ) -> Result<Self::Ok, ValidationError<io::Error>> {
        let targets = cx
            .successors(&HabeasEdgeLabel::TrackingIssue)
            .map_err(ValidationError::Graph)?;
        if targets.is_empty() {
            return Ok(Vec::new());
        }
        // An unreachable issue is assumed open. Its title and post cannot be reviewed
        // until GitHub supplies them. Other issues on this acknowledgment still count.
        let mut available = Vec::new();
        let mut names = BTreeSet::new();
        for target in targets {
            let data = cx
                .graph()
                .cache()
                .node_data(target)
                .expect("issue successor");
            if let HabeasNode::GithubIssue(issue) = data.as_ref()
                && matches!(cx.graph().github.read(issue), IssueRead::Found(_))
            {
                names.insert(issue.clone());
                available.push(target);
            }
        }
        if available.is_empty() {
            return Ok(Vec::new());
        }
        let validate = move |rows: &Vec<IssueRelevance>| {
            let found: BTreeSet<_> = rows.iter().map(|row| row.issue.clone()).collect();
            if found == names
                && rows.len() == names.len()
                && rows.iter().all(|row| !row.rationale.trim().is_empty())
            {
                Ok(VetResult::Accept)
            } else {
                Ok(VetResult::CorrectAndTryAgain(
                    "Give one judgment and rationale per supplied issue".into(),
                ))
            }
        };
        let mut request = cx.ask::<Vec<IssueRelevance>>()
            .context("Does each tracking issue's title and opening post describe this FIXME's acknowledged gap? Return one judgment with a reason for every supplied issue, identified by owner/repository#number.")
            .vet_answer(validate).contents_of_current_node();
        for node in available {
            request = request.contents_of_node(node);
        }
        request.await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoring::test_backend::RecordingProvider;
    use crate::github::{GithubIssues, tests::FakeGithub};
    use serde_json::{Value, json};
    use std::fs;
    use trust_but_verify::agent::AgentConfig;
    use trust_but_verify::graph::{Dependency, Validator};

    const ISSUE: &str = "example/readiness#7";
    const API_PATH: &str = "/repos/example/readiness/issues/7";

    fn graph(root: &Path, github: &FakeGithub) -> HabeasDomain {
        let mut graph = HabeasDomain::new(root, &BTreeMap::new(), &OutlineCache::default());
        graph.github =
            GithubIssues::with_api(github.endpoint.clone(), Some(root.join("cache")), None);
        graph
    }

    fn expire(root: &Path) {
        let path = root.join("cache/example+readiness+7.json");
        let mut entry: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        entry["fetched"] = json!(0);
        fs::write(path, serde_json::to_vec(&entry).unwrap()).unwrap();
    }

    // {test}`requirements.sdk.habeas.github_nodes`
    // {test}`requirements.sdk.habeas.github_nodes.inputs`
    // {test}`requirements.sdk.habeas.github_nodes.open`
    // {test}`requirements.sdk.habeas.github_nodes.cache`
    #[tokio::test]
    async fn tracked_issue_status_and_fingerprints_follow_refreshed_redacted_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        let github = FakeGithub::start(&[]);
        let graph = graph(dir.path(), &github);
        let issue = graph
            .cache()
            .intern_node(HabeasNode::GithubIssue(ISSUE.into()));
        assert_eq!(graph.node_path(&issue).segments(), ["github", ISSUE]);
        let mut validator = Validator::new(&graph);
        let token = ["ghp_", "abcdefghijklmnopqrstuvwxyz0123456789"].concat();
        let mut remote = json!({"title":"Check readiness", "body":format!("Wait until ready {token}"), "state":"open"});
        github.respond(API_PATH, 200, remote.clone());
        let first = validator.verify(OpenIssue(issue)).await.unwrap();
        assert!(*first.output());
        let supplied = graph.node_content(&issue).unwrap();
        assert_eq!(
            first.dependencies(),
            &[Dependency::Node(issue, supplied.fingerprint())]
        );
        let (name, snapshot) = supplied.text.split_once('\n').unwrap();
        assert_eq!(name, ISSUE);
        let snapshot: Value = serde_json::from_str(snapshot).unwrap();
        assert_eq!(snapshot["title"], "Check readiness");
        assert_eq!(snapshot["status"], "open");
        assert!(
            snapshot["opening_post"]
                .as_str()
                .unwrap()
                .starts_with("Wait until ready [REDACTED")
        );
        assert!(!supplied.format().contains(&token));
        assert_eq!(github.count(), 1);
        expire(dir.path());
        assert!(validator.check_inputs(&first).unwrap());
        assert_eq!(github.count(), 2);
        let mut previous = first;
        for (field, value) in [
            ("title", "New title"),
            ("body", "New post"),
            ("state", "closed"),
        ] {
            remote[field] = json!(value);
            github.respond(API_PATH, 200, remote.clone());
            assert!(validator.check_inputs(&previous).unwrap());
            expire(dir.path());
            assert!(!validator.check_inputs(&previous).unwrap(), "{field}");
            previous = validator.verify(OpenIssue(issue)).await.unwrap();
            assert_eq!(*previous.output(), remote["state"] == "open");
            assert!(graph.node_content(&issue).unwrap().format().contains(value));
        }
        github.respond(API_PATH, 503, json!({}));
        expire(dir.path());
        let error = graph
            .node_content(&issue)
            .err()
            .expect("expired cache needs a successful refresh");
        assert!(error.to_string().contains("503"), "{error}");
    }

    fn write_fixmes(root: &Path) {
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "// :::{fixme} rules.ready\n// :github: example/readiness#7\n// :github: example/readiness#8\n// An assertion for readiness is missing.\n// :::\nfn ready() {}\n\n// :::{fixme}\n// :github: example/readiness#8\n// Need a queue test.\n// :::\nfn queue() {}\n").unwrap();
    }

    fn owned_fixme(graph: &HabeasDomain, function: &str) -> NodeId<HabeasDomain> {
        let item = graph
            .cache()
            .intern_node(HabeasNode::from(&InputSelector::Outline {
                file: "src/lib.rs".into(),
                item: format!("fn[{function}]"),
            }));
        let owned = graph
            .outgoing_edges(&item, HabeasEdgeLabel::ContainsFixme)
            .unwrap();
        let [fixme] = owned.as_slice() else {
            panic!("expected one FIXME on {function}, found {owned:?}")
        };
        *fixme
    }

    // {test}`requirements.sdk.habeas.fixme_graph`
    #[test]
    fn named_and_unnamed_fixmes_connect_their_owners_and_all_tracking_issues() {
        let dir = tempfile::tempdir().unwrap();
        write_fixmes(dir.path());
        let graph = graph(dir.path(), &FakeGithub::start(&[]));
        let named = owned_fixme(&graph, "ready");
        let unnamed = owned_fixme(&graph, "queue");
        assert_ne!(named, unnamed);
        let requirement = graph
            .cache()
            .intern_node(HabeasNode::Requirement(RequirementId("rules.ready".into())));
        assert_eq!(
            graph
                .outgoing_edges(&requirement, HabeasEdgeLabel::AcknowledgedGap)
                .unwrap(),
            [named]
        );
        let seven = graph
            .cache()
            .intern_node(HabeasNode::GithubIssue(ISSUE.into()));
        let eight = graph
            .cache()
            .intern_node(HabeasNode::GithubIssue("example/readiness#8".into()));
        assert_eq!(
            graph
                .outgoing_edges(&named, HabeasEdgeLabel::TrackingIssue)
                .unwrap(),
            [seven, eight]
        );
        assert_eq!(
            graph
                .outgoing_edges(&unnamed, HabeasEdgeLabel::TrackingIssue)
                .unwrap(),
            [eight]
        );
        assert!(
            graph
                .node_content(&named)
                .unwrap()
                .format()
                .contains("An assertion for readiness is missing")
        );
    }

    // {test}`requirements.sdk.habeas.github_nodes.related`
    #[tokio::test]
    async fn relevance_reviews_each_linked_issue_using_tracked_issue_and_fixme_contents() {
        let dir = tempfile::tempdir().unwrap();
        write_fixmes(dir.path());
        let github = FakeGithub::start(&[
            (
                API_PATH,
                200,
                json!({"state":"open","title":"Readiness check","body":"Assert after startup"}),
            ),
            (
                "/repos/example/readiness/issues/8",
                200,
                json!({"state":"open","title":"Queue behavior","body":"Measure capacity"}),
            ),
        ]);
        let mut graph = graph(dir.path(), &github);
        let other = "example/readiness#8";
        let reply = json!([{"issue":ISSUE, "related":true, "rationale":"Covers startup readiness"}, {"issue":other,"related":false,"rationale":"Only addresses capacity"}]);
        let model = RecordingProvider::new(vec![("submit_result", json!({"result":reply}))]);
        let config: AgentConfig =
            serde_json::from_value(serde_json::to_value(&*model).unwrap()).unwrap();
        graph.outlines.validation.set_agent(config);
        let fixme = owned_fixme(&graph, "ready");
        let mut validator = Validator::new(&graph);
        let verified = validator.verify(RelatedIssues(fixme)).await.unwrap();
        let judgments = verified.output();
        assert_eq!(
            judgments
                .iter()
                .map(|r| (r.issue.as_str(), r.related))
                .collect::<Vec<_>>(),
            [(ISSUE, true), (other, false)]
        );
        let prompt = model.requests.lock().unwrap()[0]["messages"].to_string();
        for text in [
            "An assertion for readiness is missing",
            "Readiness check",
            "Assert after startup",
            "Queue behavior",
            "Measure capacity",
        ] {
            assert!(prompt.contains(text), "{text} absent from {prompt}");
        }
        let seven = graph
            .cache()
            .intern_node(HabeasNode::GithubIssue(ISSUE.into()));
        let eight = graph
            .cache()
            .intern_node(HabeasNode::GithubIssue(other.into()));
        for id in [fixme, seven, eight] {
            assert!(verified.dependencies().contains(&Dependency::Node(
                id,
                graph.node_content(&id).unwrap().fingerprint()
            )));
        }
        assert!(verified.dependencies().iter().any(|d| matches!(d, Dependency::OutgoingEdges(id, HabeasEdgeLabel::TrackingIssue, _) if id == &fixme)));
        github.respond(
            API_PATH,
            200,
            json!({"state":"open","title":"Unrelated new title","body":"Assert after startup"}),
        );
        expire(dir.path());
        assert!(!validator.check_inputs(&verified).unwrap());
    }
}

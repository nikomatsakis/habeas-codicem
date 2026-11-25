use trust_but_verify::{
    domain::{Domain, NodeContents},
    graph::{CriterionPath, Fingerprint, NodePath, Record},
    storage::{FileEvidenceStorage, FileStorageError},
};

#[derive(Clone)]
pub struct StoredGraph(
    pub FileEvidenceStorage,
    pub trust_but_verify::domain::ValidationCache,
);

pub enum UnreadContents {}
impl NodeContents for UnreadContents {
    // :::{cite} habeas:coverage
    // UnreadContents is uninhabited: this storage-only fixture cannot construct
    // a value or call a content method. The implementation discharges the
    // Domain associated-type bound; its empty match has no executable case.
    // :::
    fn fingerprint(&self) -> Fingerprint {
        match *self {}
    }
    // :::{cite} habeas:coverage
    // Formatting requires a value of the uninhabited UnreadContents type.
    // This empty match has no executable case; storage tests deliberately
    // supply no content values and reject node reads at the domain boundary.
    // :::
    fn format(&self) -> String {
        match *self {}
    }
}

impl Domain for StoredGraph {
    fn cache(&self) -> &trust_but_verify::domain::ValidationCache {
        &self.1
    }
    type NodeId = String;
    type EdgeLabel = ();

    type Error = FileStorageError;

    fn node_path(&self, node: &String) -> NodePath {
        NodePath::new(vec!["consumer".into(), node.clone()])
    }
    // :::{cite} habeas:coverage
    // Domain requires this callback even for a storage-only consumer. The
    // delegation test must use only node_path; discovering edges would be
    // an unwanted graph access, which this guard rejects. Traversal fixtures
    // separately exercise edge discovery.
    // :::
    fn outgoing_edges(&self, _: &String, _: ()) -> Result<Vec<String>, Self::Error> {
        panic!("storage operations must not discover graph edges")
    }

    async fn load_evidence(
        &self,
        node: &String,
        criterion: &CriterionPath,
    ) -> Result<Option<Record<String>>, Self::Error> {
        self.0.load(&self.node_path(node), criterion).await
    }
    async fn store_evidence(
        &self,
        node: &String,
        criterion: &CriterionPath,
        record: &Record<String>,
    ) -> Result<(), Self::Error> {
        self.0.store(&self.node_path(node), criterion, record).await
    }

    type NodeContent = UnreadContents;

    // :::{cite} habeas:coverage
    // This storage-only consumer must delegate load/store using node_path
    // without resolving contents. Domain requires the callback, but invoking
    // this guard would violate the no-content-read invariant of the test.
    // Other graph fixtures exercise content resolution.
    // :::
    fn node_content(&self, _: &String) -> Result<UnreadContents, Self::Error> {
        panic!("storage operations must not read graph contents")
    }
}

# Filesystem evidence storage

Use `FileEvidenceStorage` to implement the evidence methods of `Domain` with a directory of
JSON records. The application chooses the directory and uses `Domain::node_path` to obtain
a stable [`NodePath`](graph.md#give-nodes-a-stable-path), paired with the validation's
`CriterionPath`. The helper handles serialization, file access, and replacing
saved {term}`requirements.sdk.graph.evidence.verification_evidence`.

## Configure the store

:::{req} interface
```rust,ignore
use std::path::PathBuf;
use serde::{Serialize, de::DeserializeOwned};
use trust_but_verify::graph::CriterionPath;
use trust_but_verify::graph::NodePath;
use trust_but_verify::graph::Record;

/// A directory of JSON records. Send + Sync; load/store futures are Send.
/// Defined in trust_but_verify::storage.
pub struct FileEvidenceStorage { /* private fields */ }

pub enum FileStorageError {
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl FileEvidenceStorage {
    /// Retain the root directory without performing filesystem I/O.
    pub fn new(root: impl Into<PathBuf>) -> Self;

    /// Deserialize a record, or return None if its file is missing.
    pub async fn load<N, L>(
        &self,
        path: &NodePath,
        criterion: &CriterionPath,
    ) -> Result<Option<Record<N, L>>, FileStorageError>
    where
        N: DeserializeOwned + Send,
        L: DeserializeOwned + Send;

    /// Atomically replace this source/criterion pair's record.
    pub async fn store<N, L>(
        &self,
        path: &NodePath,
        criterion: &CriterionPath,
        record: &Record<N, L>,
    ) -> Result<(), FileStorageError>
    where
        N: Serialize + Sync,
        L: Serialize + Sync;
}
```

`Record`, `Dependency`, and `Fingerprint` support Serde when their type
parameters do. `Criterion::Ok` is always serializable; its JSON value is
already in the record. Serialization bounds on node identities and edge labels
apply when using this helper.
:::

An application holds a store alongside its graph data. Its `load_evidence`
method obtains `let path = self.node_path(source)` and delegates to
`store.load(&path, criterion).await`; `store_evidence` delegates to
`store.store(&path, criterion, record).await`. It maps
`FileStorageError` into its own `Domain::Error`.

## Map node paths into the directory

The graph supplies stable, unique paths under
{cite}`requirements.sdk.graph.node_path_method`. The helper preserves segment
boundaries when mapping those paths to files.

:::{req} keys
For each `NodePath` segment, the helper uses `s` followed by the lowercase
hexadecimal encoding of that segment's UTF-8 bytes as a directory component.
After the encoded source components it inserts a literal `criteria` directory,
then encodes the `CriterionPath` segments by the same rule and stores
`record.json` inside the resulting directory under the configured root.
The unencoded separator preserves the source/criterion boundary: encoded
segments always begin with `s`. An empty source or criterion adds no components
for that part; an empty segment selects an `s` directory.
Segment contents, including slashes and `..`, are encoded as data rather than
interpreted as path components. Load and store use the same mapping. If the
filesystem cannot represent that path, the operation returns an I/O error
without truncating or otherwise changing the mapping.
:::

For example, source `["a"]` with criterion `["b", "c"]` maps to
`s61/criteria/s62/s63/record.json`, whereas source `["a", "b"]` with criterion
`["c"]` maps to `s61/s62/criteria/s63/record.json`. Both paths empty select
`criteria/record.json` under the root.

The configured root can be a relative or absolute directory.

## Load and replace records

:::{req} load
`load` deserializes the selected JSON file as `Record<N, L>`, preserving
SDK-owned fields. Node identities and edge labels round-trip through their
Serde implementations. The output remains a JSON value; the helper does not
decode it as a validation's result type.

| File state | Result |
| --- | --- |
| Missing file or root directory | `Ok(None)` |
| Other filesystem failure | `FileStorageError::Io` |
| Malformed JSON or incompatible record types | `FileStorageError::Json` |

Errors do not count as absent evidence. Loading creates no directories and
invokes no validation.
:::

:::{req} store
`store` creates missing root and encoded-segment directories, then serializes
the complete supplied record as JSON. Replacement is atomic: concurrent readers
observe either the old or new complete record, never a partial write.
Serialization failure leaves any existing record unchanged.

File-operation failures return `FileStorageError::Io`; serialization failures
return `FileStorageError::Json`. Concurrent stores use independent temporary
files, and successful competing writes leave one of the complete records.
After a failed write, the helper attempts to remove its temporary files.
A cleanup failure returns `FileStorageError::Io`. A successful store leaves
no temporary file belonging to that operation.
:::

Atomic replacement does not promise durability across power loss or recovery
of temporary files after a process crash.

The SDK still owns [freshness checking](graph/evidence.md#compare-the-saved-observations).
A successfully loaded record may be stale or contain a negative judgment.

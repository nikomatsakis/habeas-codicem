# Filesystem evidence storage

Use `FileEvidenceStorage` to implement `EvidenceStorage` with a directory of
JSON records. The application chooses the directory and uses `Graph::node_path` to obtain
a stable [`NodePath`](graph.md#give-nodes-a-stable-path). The helper handles serialization, file access, and replacing
saved records; it does not decide whether evidence is current.

## Configure the store

:::{req} interface
```rust,ignore
use std::path::PathBuf;
use serde::{Serialize, de::DeserializeOwned};
use habeas_sdk::graph::{NodePath, Record};

/// A directory of JSON records. Send + Sync; load/store futures are Send.
/// Defined in habeas_sdk::storage.
pub struct FileEvidenceStorage { /* private fields */ }

pub enum FileStorageError {
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl FileEvidenceStorage {
    /// Retain the root directory without performing filesystem I/O.
    pub fn new(root: impl Into<PathBuf>) -> Self;

    /// Deserialize a record, or return None if its file is missing.
    pub async fn load<N, M>(
        &self,
        path: &NodePath,
    ) -> Result<Option<Record<N, M>>, FileStorageError>
    where
        N: DeserializeOwned + Send,
        M: DeserializeOwned + Send;

    /// Atomically replace the record at this node path.
    pub async fn store<N, M>(
        &self,
        path: &NodePath,
        record: &Record<N, M>,
    ) -> Result<(), FileStorageError>
    where
        N: Serialize + Sync,
        M: Serialize + Sync;
}
```

`Record`, `Dependency`, `Fingerprint`, and `Judgment` support Serde
serialization and deserialization when their type parameters do. These bounds
belong to storage use; `Graph` and `EvidenceStorage` themselves do not require them.
:::

An application holds a store alongside its graph data. Its `load_evidence`
method obtains `let path = self.node_path(node)` and delegates to
`store.load(&path).await`; `store_evidence` delegates to
`store.store(&path, record).await`. It maps
`FileStorageError` into its own `Graph::Error`.

## Map node paths into the directory

The graph supplies stable, unique paths under
{cite}`requirements.sdk.graph.node_path_method`. The helper preserves segment
boundaries when mapping those paths to files. No requirement names, source
paths, or CLI conventions are built into it.

:::{req} keys
For each `NodePath` segment, the helper uses `s` followed by the lowercase
hexadecimal encoding of that segment's UTF-8 bytes as a directory component.
It preserves segment order and stores `record.json` inside the resulting
directory under the configured root. An empty sequence selects `record.json`
directly under the root; an empty segment selects an `s` directory.
Segment contents, including slashes and `..`, are encoded as data rather than
interpreted as path components. Load and store use the same mapping. If the
filesystem cannot represent that path, the operation returns an I/O error
without truncating or otherwise changing the mapping.
:::

For example, `["a", "b"]` maps to `s61/s62/record.json`, whereas `["a/b"]`
maps to `s612f62/record.json`. A node and its descendants can each have a record
because every record has its own fixed filename inside its node directory.

The configured directory is application-owned. It can be relative or absolute;
applications sharing it must agree on node paths and record types. The helper
does not enumerate records or apply an evidence-retention policy.

## Load and replace records

:::{req} load
`load` deserializes the selected JSON file as `Record<N, M>`, preserving
SDK-owned fields. Node identities and application metadata round-trip through
their Serde implementations.

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
of temporary files after a process crash. Applications needing transactions
across several nodes can implement `EvidenceStorage` with another backend.

The SDK still owns [freshness checking](graph/evidence.md#compare-the-saved-observations).
A successfully loaded record may be stale or contain a negative judgment.

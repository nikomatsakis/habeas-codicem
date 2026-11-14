# Filesystem evidence storage

Use `FileEvidenceStorage` to implement `EvidenceStorage` with a directory of
JSON records. The application chooses the directory and uses `Graph::node_path` to obtain
a stable [`NodePath`](graph.md#give-nodes-a-stable-path). The helper handles serialization, file access, and replacing
saved records; it does not decide whether evidence is current.

## Configure the store

:::{req} interface
`habeas_sdk::storage` MUST expose the following helper:

```rust,ignore
use std::path::PathBuf;
use serde::{Serialize, de::DeserializeOwned};
use habeas_sdk::graph::{NodePath, Record};

pub struct FileEvidenceStorage { /* private fields */ }

pub enum FileStorageError {
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl FileEvidenceStorage {
    pub fn new(root: impl Into<PathBuf>) -> Self;

    pub async fn load<N, M>(
        &self,
        path: &NodePath,
    ) -> Result<Option<Record<N, M>>, FileStorageError>
    where
        N: DeserializeOwned + Send,
        M: DeserializeOwned + Send;

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

The helper MUST be `Send + Sync`, and its load and store futures MUST be `Send`.
Construction MUST retain the supplied root without performing filesystem I/O.
The SDK's `Record`, `Dependency`, `Fingerprint`, and `Judgment` types MUST
support Serde serialization and deserialization when their type parameters do.
These bounds MUST NOT be imposed on `Graph` or `EvidenceStorage` themselves.
:::

An application holds a store alongside its graph data. Its `load_evidence`
method obtains `let path = self.node_path(node)` and delegates to
`store.load(&path).await`; `store_evidence` delegates to
`store.store(&path, record).await`. It maps
`FileStorageError` into its own `Graph::Error`.

## Map node paths into the directory

The graph supplies stable, unique paths under
{cite}`requirements.sdk.graph.node_path`. The helper preserves segment
boundaries when mapping those paths to files. No requirement names, source
paths, or CLI conventions are built into it.

:::{req} keys
For each `NodePath` segment, the helper MUST use `s` followed by the lowercase
hexadecimal encoding of that segment's UTF-8 bytes as a directory component.
It MUST preserve segment order and store the record as `record.json` inside
the resulting directory under the configured root. An empty sequence MUST
select `record.json` directly under the root; an empty segment MUST select
an `s` directory. Segment contents, including slashes and `..`, MUST NOT be
interpreted as filesystem path components. Load and store MUST use the same
mapping. If the filesystem cannot represent the resulting path, the operation
MUST return an I/O error rather than truncate or otherwise change the mapping.
:::

For example, `["a", "b"]` maps to `s61/s62/record.json`, whereas `["a/b"]`
maps to `s612f62/record.json`. A node and its descendants can each have a record
because every record has its own fixed filename inside its node directory.

The configured directory is application-owned. It can be relative or absolute;
applications sharing it must agree on node paths and record types. The helper
does not enumerate records or apply an evidence-retention policy.

## Load and replace records

:::{req} load
`load` MUST deserialize the selected JSON file as `Record<N, M>` using the
supplied types' Serde implementations. The helper MUST preserve the SDK-owned
fields; node identities and application metadata round-trip according to their
Serde implementations.
A missing file, including a missing root directory, MUST return `Ok(None)`.
Other filesystem failures MUST return `FileStorageError::Io`; malformed JSON
or a record incompatible with the requested types MUST return
`FileStorageError::Json`. Neither case MUST be treated as absent evidence.
Loading MUST NOT create directories or invoke validation.
:::

:::{req} store
`store` MUST create all missing directories needed for the selected record path,
including the configured root and the encoded segment directories, and serialize
the complete supplied record as JSON. It MUST replace the file
for the selected node path atomically: concurrent readers MUST observe an old or new
complete record, never a partially written record. Serialization failure MUST
leave any existing record unchanged. File-operation failures MUST be returned
as `FileStorageError::Io`, and serialization failures as `FileStorageError::Json`.
Concurrent stores MUST use independent temporary files; after successful
competing writes, the saved record MUST be one of those complete records.
The helper MUST attempt to remove its temporary files after a failed write.
A cleanup failure MUST be reported as `FileStorageError::Io`. Successful stores
MUST leave no temporary file belonging to that operation.
:::

Atomic replacement does not promise durability across power loss or recovery
of temporary files after a process crash. Applications needing transactions
across several nodes can implement `EvidenceStorage` with another backend.

The SDK still owns [freshness checking](graph/evidence.md#compare-the-saved-observations).
A successfully loaded record may be stale or contain a negative judgment.

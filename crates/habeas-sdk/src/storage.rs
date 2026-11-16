//! Atomic filesystem storage for graph evidence.

use crate::graph::{NodePath, Record};
use serde::{Serialize, de::DeserializeOwned};
use std::{io::Write, path::PathBuf};

#[derive(Debug)]
pub enum FileStorageError {
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for FileStorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Json(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for FileStorageError {}

// {impl}`requirements.sdk.filesystem-storage.interface`
pub struct FileEvidenceStorage {
    root: PathBuf,
}

impl FileEvidenceStorage {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    // {impl}`requirements.sdk.filesystem-storage.keys`
    pub fn record_path(&self, path: &NodePath) -> PathBuf {
        let mut result = self.root.clone();
        for segment in &path.segments {
            let encoded: String = segment
                .as_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            result.push(format!("s{encoded}"));
        }
        result.join("record.json")
    }

    // {impl}`requirements.sdk.filesystem-storage.load`
    pub async fn load<N, M>(
        &self,
        path: &NodePath,
    ) -> Result<Option<Record<N, M>>, FileStorageError>
    where
        N: DeserializeOwned + Send,
        M: DeserializeOwned + Send,
    {
        let bytes = match std::fs::read(self.record_path(path)) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(FileStorageError::Io(error)),
        };
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(FileStorageError::Json)
    }

    // {impl}`requirements.sdk.filesystem-storage.store`
    pub async fn store<N, M>(
        &self,
        path: &NodePath,
        record: &Record<N, M>,
    ) -> Result<(), FileStorageError>
    where
        N: Serialize + Sync,
        M: Serialize + Sync,
    {
        let bytes = serde_json::to_vec_pretty(record).map_err(FileStorageError::Json)?;
        let target = self.record_path(path);
        let parent = target.parent().expect("record path has a parent");
        std::fs::create_dir_all(parent).map_err(FileStorageError::Io)?;
        let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(FileStorageError::Io)?;
        if let Err(error) = temp.write_all(&bytes) {
            temp.close().map_err(FileStorageError::Io)?;
            return Err(FileStorageError::Io(error));
        }
        match temp.persist(target) {
            Ok(_) => Ok(()),
            Err(error) => {
                error.file.close().map_err(FileStorageError::Io)?;
                Err(FileStorageError::Io(error.error))
            }
        }
    }
}

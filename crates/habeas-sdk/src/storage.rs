//! Atomic filesystem storage for graph evidence.

use crate::graph::{NodePath, Record};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

// {impl}`requirements.sdk.filesystem-storage.interface`
#[derive(Debug)]
pub enum FileStorageError {
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for FileStorageError {
    // {cite}`habeas:util`
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Json(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for FileStorageError {}

// {impl}`requirements.sdk.filesystem-storage.interface`
// {impl}`requirements.sdk.graph.evidence.evidence_storage.filesystem`
pub struct FileEvidenceStorage {
    root: PathBuf,
}

impl FileEvidenceStorage {
    // {cite}`habeas:util`
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
        let temp = tempfile::NamedTempFile::new_in(parent).map_err(FileStorageError::Io)?;
        write_and_replace(temp, &bytes, &target)
    }
}

/// The temporary file operations are separate so their failure paths can be
/// exercised without depending on filesystem permissions or disk exhaustion.
// {cite}`habeas:util`
trait TemporaryRecord: Write + Sized {
    // {cite}`habeas:util`
    fn replace(self, target: &Path) -> Result<(), (Self, std::io::Error)>;
    // {cite}`habeas:util`
    fn cleanup(self) -> std::io::Result<()>;
}

impl TemporaryRecord for tempfile::NamedTempFile {
    // {cite}`habeas:util`
    fn replace(self, target: &Path) -> Result<(), (Self, std::io::Error)> {
        self.persist(target)
            .map(|_| ())
            .map_err(|error| (error.file, error.error))
    }
    // {cite}`habeas:util`
    fn cleanup(self) -> std::io::Result<()> {
        self.close()
    }
}

// {impl}`requirements.sdk.filesystem-storage.store`
fn write_and_replace<T: TemporaryRecord>(
    mut temporary: T,
    bytes: &[u8],
    target: &Path,
) -> Result<(), FileStorageError> {
    if let Err(error) = temporary.write_all(bytes) {
        temporary.cleanup().map_err(FileStorageError::Io)?;
        return Err(FileStorageError::Io(error));
    }
    match temporary.replace(target) {
        Ok(()) => Ok(()),
        Err((temporary, error)) => {
            temporary.cleanup().map_err(FileStorageError::Io)?;
            Err(FileStorageError::Io(error))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    struct FaultyTemporary {
        fail_write: bool,
        fail_replace: bool,
        fail_cleanup: bool,
        operations: Rc<RefCell<Vec<&'static str>>>,
    }
    impl Write for FaultyTemporary {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.operations.borrow_mut().push("write");
            if self.fail_write {
                Err(std::io::Error::other("write failure"))
            } else {
                Ok(bytes.len())
            }
        }
        // :::{cite} habeas:coverage
        // Write requires flush, but this unbuffered fault-injection fixture
        // tests write_all, replacement, and cleanup. write_and_replace does
        // not request flushing or durability; real temporary files are
        // unbuffered too. A flush call would change the tested operation
        // sequence, so the unused callback is an explicit guard.
        // :::
        fn flush(&mut self) -> std::io::Result<()> {
            panic!("replacement must not request a fixture flush")
        }
    }
    impl TemporaryRecord for FaultyTemporary {
        fn replace(self, _: &Path) -> Result<(), (Self, std::io::Error)> {
            self.operations.borrow_mut().push("replace");
            if self.fail_replace {
                Err((self, std::io::Error::other("replace failure")))
            } else {
                Ok(())
            }
        }
        fn cleanup(self) -> std::io::Result<()> {
            self.operations.borrow_mut().push("cleanup");
            if self.fail_cleanup {
                Err(std::io::Error::other("cleanup failure"))
            } else {
                Ok(())
            }
        }
    }

    // {test}`requirements.sdk.filesystem-storage.store`
    #[test]
    fn writing_and_replacing_report_each_io_failure_and_cleanup_failure() {
        for (fail_write, fail_replace, fail_cleanup, expected, operations) in [
            (
                true,
                false,
                false,
                "write failure",
                vec!["write", "cleanup"],
            ),
            (
                true,
                false,
                true,
                "cleanup failure",
                vec!["write", "cleanup"],
            ),
            (
                false,
                true,
                false,
                "replace failure",
                vec!["write", "replace", "cleanup"],
            ),
            (
                false,
                true,
                true,
                "cleanup failure",
                vec!["write", "replace", "cleanup"],
            ),
            (false, false, false, "", vec!["write", "replace"]),
        ] {
            let observed = Rc::new(RefCell::new(Vec::new()));
            let result = write_and_replace(
                FaultyTemporary {
                    fail_write,
                    fail_replace,
                    fail_cleanup,
                    operations: observed.clone(),
                },
                b"record",
                Path::new("record.json"),
            );
            if expected.is_empty() {
                result.unwrap();
            } else {
                assert!(
                    matches!(result, Err(FileStorageError::Io(error)) if error.to_string() == expected)
                );
            }
            assert_eq!(*observed.borrow(), operations);
        }
    }
}

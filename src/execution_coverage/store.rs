//! The current coverage set. Profiles and coverage maps live beside the database.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use super::{Attempt, Run};

const SCHEMA: i64 = 1;
pub(super) const DATABASE: &str = "current.sqlite";

// {impl}`requirements.test-coverage.storage.current`
// {impl}`requirements.test-coverage.storage.selection`
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub(super) struct TestIdentity {
    pub(super) binary: String,
    pub(super) test: String,
}

// Synthetic fixtures address invocation and attempt rows by their artifact
// directories. The JSON-looking paths are row selectors, never report files.
#[cfg(test)]
pub(super) mod fixtures {
    use super::*;

    // {cite}`habeas:util`
    pub(crate) fn save(path: &Path, value: &impl serde::Serialize) -> io::Result<()> {
        let mut value = serde_json::to_value(value)?;
        let run_path = if path.file_name().unwrap() == "run.json" {
            path.parent().unwrap()
        } else {
            path.parent().unwrap().parent().unwrap().parent().unwrap()
        };
        let store = Store::open(run_path.parent().unwrap())?;
        let run = run_path.file_name().unwrap().to_string_lossy();
        if path.file_name().unwrap() == "run.json" {
            let exists: bool = store
                .connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM invocations WHERE id=?1)",
                    [run.as_ref()],
                    |row| row.get(0),
                )
                .map_err(io::Error::other)?;
            if !exists {
                let active: Option<Option<String>> = store
                    .connection
                    .query_row("SELECT configuration FROM state", [], |row| row.get(0))
                    .optional()
                    .map_err(io::Error::other)?;
                if active.flatten().as_deref() != value["configuration"].as_str() {
                    store
                        .connection
                        .execute("DELETE FROM tests", [])
                        .map_err(io::Error::other)?;
                }
                store
                    .connection
                    .execute(
                        "INSERT INTO invocations(id,data) VALUES (?1,?2)",
                        params![run, value.to_string()],
                    )
                    .map_err(io::Error::other)?;
                store.connection.execute(
                    "INSERT INTO state(singleton,configuration,latest) VALUES (1,?1,?2)
                     ON CONFLICT(singleton) DO UPDATE SET configuration=excluded.configuration,latest=excluded.latest",
                    params![value["configuration"].as_str(), run],
                ).map_err(io::Error::other)?;
            } else {
                store
                    .connection
                    .execute(
                        "UPDATE invocations SET data=?1 WHERE id=?2",
                        params![value.to_string(), run],
                    )
                    .map_err(io::Error::other)?;
            }
        } else {
            let measurements = value.as_object_mut().unwrap().remove("processes").unwrap();
            value["processes"] = serde_json::json!([]);
            let old_id = path
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy();
            store
                .connection
                .execute(
                    "DELETE FROM tests WHERE invocation=?1 AND json_extract(metadata,'$.id')=?2",
                    params![run, old_id],
                )
                .map_err(io::Error::other)?;
            store.connection.execute(
                "INSERT INTO tests(binary,test,invocation,metadata,measurements) VALUES (?1,?2,?3,?4,?5)
                 ON CONFLICT(binary,test) DO UPDATE SET invocation=excluded.invocation,metadata=excluded.metadata,measurements=excluded.measurements",
                params![value["binary"].as_str(), value["test"].as_str(), run, value.to_string(), measurements.to_string()],
            ).map_err(io::Error::other)?;
        }
        Ok(())
    }

    // {cite}`habeas:util`
    pub(crate) fn load<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<T> {
        let run_path = if path.file_name().unwrap() == "run.json" {
            path.parent().unwrap()
        } else {
            path.parent().unwrap().parent().unwrap().parent().unwrap()
        };
        let store = Store::read(run_path.parent().unwrap())?;
        let run = run_path.file_name().unwrap().to_string_lossy();
        let mut value: serde_json::Value;
        if path.file_name().unwrap() == "run.json" {
            let data: String = store
                .connection
                .query_row(
                    "SELECT data FROM invocations WHERE id=?1",
                    [run.as_ref()],
                    |row| row.get(0),
                )
                .map_err(io::Error::other)?;
            value = serde_json::from_str(&data)?;
        } else {
            let id = path
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy();
            let (metadata, measurements): (String, Option<String>) = store.connection.query_row(
                "SELECT metadata,measurements FROM tests WHERE invocation=?1 AND json_extract(metadata,'$.id')=?2",
                params![run, id], |row| Ok((row.get(0)?, row.get(1)?)),
            ).map_err(io::Error::other)?;
            value = serde_json::from_str(&metadata)?;
            value["processes"] = measurements
                .map(|data| serde_json::from_str(&data))
                .transpose()?
                .unwrap_or(serde_json::json!([]));
        }
        serde_json::from_value(value).map_err(io::Error::other)
    }

    // {cite}`habeas:util`
    pub(crate) fn corrupt(path: &Path) {
        let run_path = if path.file_name().unwrap() == "run.json" {
            path.parent().unwrap()
        } else {
            path.parent().unwrap().parent().unwrap().parent().unwrap()
        };
        let store = Store::open(run_path.parent().unwrap()).unwrap();
        if path.file_name().unwrap() == "run.json" {
            store
                .connection
                .execute(
                    "UPDATE invocations SET data='{' WHERE id=?1",
                    [run_path.file_name().unwrap().to_str().unwrap()],
                )
                .unwrap();
        } else {
            store
                .connection
                .execute("UPDATE tests SET metadata='{'", [])
                .unwrap();
        }
    }
}

// Keep the lock separate from SQLite's short write transactions: nextest's
// process wrappers must be able to write while the parent owns collection.
// {impl}`requirements.test-coverage.storage.replace`
pub(super) struct CollectorLock(fs::File);

impl CollectorLock {
    // {impl}`requirements.test-coverage.storage.replace`
    pub(super) fn acquire(storage: &Path) -> io::Result<Self> {
        fs::create_dir_all(storage)?;
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(storage.join("collector.lock"))?;
        file.try_lock().map_err(|error| {
            io::Error::other(format!(
                "Another coverage collector owns {}: {error}",
                storage.display()
            ))
        })?;
        Ok(Self(file))
    }
}

impl Drop for CollectorLock {
    // {cite}`habeas:util`
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

// {impl}`requirements.test-coverage.storage`
pub(super) struct Store {
    connection: Connection,
    storage: PathBuf,
}

// {impl}`requirements.test-coverage.index.selection`
pub(super) struct Snapshot {
    pub(super) configuration: Option<String>,
    pub(super) runs: BTreeMap<String, Run>,
    pub(super) tests: Vec<StoredTest>,
    pub(super) latest: Option<String>,
}

// {impl}`requirements.test-coverage.storage.outcomes`
pub(super) struct StoredTest {
    pub(super) run: String,
    pub(super) metadata: Attempt,
    pub(super) measurements: Option<String>,
}

impl Store {
    // {impl}`requirements.test-coverage.storage`
    pub(super) fn open(storage: &Path) -> io::Result<Self> {
        fs::create_dir_all(storage)?;
        let connection = Connection::open(storage.join(DATABASE)).map_err(io::Error::other)?;
        connection
            .busy_timeout(Duration::from_secs(30))
            .map_err(io::Error::other)?;
        let schema: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(io::Error::other)?;
        if schema != 0 && schema != SCHEMA {
            return Err(io::Error::other(
                "Unsupported coverage store format; remove disposable coverage artifacts and recollect coverage.",
            ));
        }
        connection
            .pragma_update(None, "foreign_keys", true)
            .map_err(io::Error::other)?;
        // WAL commits stay atomic without flushing for every process-state
        // update. Coverage is disposable: a machine power failure may lose
        // recent results, which can be recollected.
        connection
            .pragma_update(None, "synchronous", "NORMAL")
            .map_err(io::Error::other)?;
        if schema == SCHEMA {
            return Ok(Self {
                connection,
                storage: storage.to_owned(),
            });
        }
        connection
            .execute_batch(
                "PRAGMA foreign_keys=ON;
             PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS invocations (
                 id TEXT PRIMARY KEY, data TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS state (
                 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
                 configuration TEXT,
                 latest TEXT NOT NULL REFERENCES invocations(id)
             );
             CREATE TABLE IF NOT EXISTS tests (
                 binary TEXT NOT NULL, test TEXT NOT NULL,
                 invocation TEXT NOT NULL REFERENCES invocations(id),
                 metadata TEXT NOT NULL, measurements TEXT,
                 PRIMARY KEY(binary, test)
             );
             PRAGMA user_version=1;",
            )
            .map_err(io::Error::other)?;
        Ok(Self {
            connection,
            storage: storage.to_owned(),
        })
    }

    // {impl}`requirements.test-coverage.check.measurement`
    pub(super) fn read(storage: &Path) -> io::Result<Self> {
        let connection = Connection::open_with_flags(
            storage.join(DATABASE),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        ).map_err(|error| io::Error::other(format!(
            "Current SQLite coverage store is unavailable: {error}. Recollect with `habeas coverage -- --workspace`; legacy JSON results are not imported."
        )))?;
        connection
            .busy_timeout(Duration::from_secs(30))
            .map_err(io::Error::other)?;
        let schema: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(io::Error::other)?;
        if schema != SCHEMA {
            return Err(io::Error::other(
                "Unsupported coverage store format; recollect coverage.",
            ));
        }
        Ok(Self {
            connection,
            storage: storage.to_owned(),
        })
    }

    // {impl}`requirements.test-coverage.storage.replace`
    // {impl}`requirements.test-coverage.storage.configuration_reset`
    // {impl}`requirements.test-coverage.storage.outcomes`
    pub(super) fn replace(
        &mut self,
        run: &Run,
        selected: &[TestIdentity],
        filtered: bool,
    ) -> io::Result<bool> {
        if filtered && selected.is_empty() {
            return Ok(false);
        }
        let transaction = self.connection.transaction().map_err(io::Error::other)?;
        let configuration: Option<Option<String>> = transaction
            .query_row(
                "SELECT configuration FROM state WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(io::Error::other)?;
        if !filtered || configuration.flatten().as_deref() != Some(&run.configuration) {
            transaction
                .execute("DELETE FROM tests", [])
                .map_err(io::Error::other)?;
        }
        transaction
            .execute(
                "INSERT INTO invocations(id,data) VALUES (?1,?2)",
                params![run.id, serde_json::to_string(run)?],
            )
            .map_err(io::Error::other)?;
        transaction.execute(
            "INSERT INTO state(singleton,configuration,latest) VALUES (1,?1,?2)
             ON CONFLICT(singleton) DO UPDATE SET configuration=excluded.configuration,latest=excluded.latest",
            params![run.configuration, run.id],
        ).map_err(io::Error::other)?;
        for identity in selected {
            let pending = Attempt {
                id: uuid::Uuid::new_v4().to_string(),
                binary: identity.binary.clone(),
                test: identity.test.clone(),
                attempt: String::new(),
                started: run.started,
                pid: 0,
                command: Vec::new(),
                outcome: "pending".into(),
                processes: Vec::new(),
                diagnostics: Vec::new(),
            };
            transaction.execute(
                "INSERT INTO tests(binary,test,invocation,metadata,measurements) VALUES (?1,?2,?3,?4,NULL)
                 ON CONFLICT(binary,test) DO UPDATE SET invocation=excluded.invocation,metadata=excluded.metadata,measurements=NULL",
                params![identity.binary, identity.test, run.id, serde_json::to_string(&pending)?],
            ).map_err(io::Error::other)?;
        }
        transaction.commit().map_err(io::Error::other)?;
        Ok(true)
    }

    // Empty focused runs record their outcome without changing configuration
    // or replacing any current tests.
    // {impl}`requirements.test-coverage.storage.outcomes`
    pub(super) fn record_empty(&mut self, run: &Run) -> io::Result<()> {
        let transaction = self.connection.transaction().map_err(io::Error::other)?;
        transaction
            .execute(
                "INSERT INTO invocations(id,data) VALUES (?1,?2)",
                params![run.id, serde_json::to_string(run)?],
            )
            .map_err(io::Error::other)?;
        transaction
            .execute(
                "INSERT INTO state(singleton,configuration,latest) VALUES (1,NULL,?1)
             ON CONFLICT(singleton) DO UPDATE SET latest=excluded.latest",
                [&run.id],
            )
            .map_err(io::Error::other)?;
        transaction.commit().map_err(io::Error::other)
    }

    // {impl}`requirements.test-coverage.storage.retries`
    pub(super) fn start_attempt(&self, run: &str, attempt: &Attempt) -> io::Result<()> {
        let count = self.connection.execute(
            "UPDATE tests SET metadata=?1,measurements=NULL WHERE binary=?2 AND test=?3 AND invocation=?4",
            params![serde_json::to_string(attempt)?, attempt.binary, attempt.test, run],
        ).map_err(io::Error::other)?;
        if count != 1 {
            return Err(io::Error::other(
                "Test was not selected in this coverage invocation.",
            ));
        }
        Ok(())
    }

    // {impl}`requirements.test-coverage.storage.outcomes`
    pub(super) fn update_attempt(
        &self,
        run: &str,
        attempt: &Attempt,
        measured: bool,
    ) -> io::Result<()> {
        // Copy only the small metadata; cloning every line count just to
        // discard it would undo the benefit of the separate measurements column.
        let metadata = Attempt {
            id: attempt.id.clone(),
            binary: attempt.binary.clone(),
            test: attempt.test.clone(),
            attempt: attempt.attempt.clone(),
            started: attempt.started,
            pid: attempt.pid,
            command: attempt.command.clone(),
            outcome: attempt.outcome.clone(),
            processes: Vec::new(),
            diagnostics: attempt.diagnostics.clone(),
        };
        let measurements = measured
            .then(|| serde_json::to_string(&attempt.processes))
            .transpose()?;
        let count = self
            .connection
            .execute(
                "UPDATE tests SET metadata=?1,measurements=?2 WHERE binary=?3 AND test=?4
             AND invocation=?5 AND json_extract(metadata,'$.id')=?6",
                params![
                    serde_json::to_string(&metadata)?,
                    measurements,
                    attempt.binary,
                    attempt.test,
                    run,
                    attempt.id
                ],
            )
            .map_err(io::Error::other)?;
        if count != 1 {
            return Err(io::Error::other("Coverage attempt has been superseded."));
        }
        Ok(())
    }

    // {impl}`requirements.test-coverage.storage.outcomes`
    pub(super) fn update_run(&self, run: &Run) -> io::Result<()> {
        let count = self
            .connection
            .execute(
                "UPDATE invocations SET data=?1 WHERE id=?2",
                params![serde_json::to_string(run)?, run.id],
            )
            .map_err(io::Error::other)?;
        if count != 1 {
            return Err(io::Error::other("Unknown coverage invocation."));
        }
        Ok(())
    }

    // Read metadata and measurements in a single transaction. Selecting only
    // metadata never reads or parses the large measurements column.
    // {impl}`requirements.test-coverage.index.selection`
    pub(super) fn snapshot(&mut self, measurements: bool) -> io::Result<Snapshot> {
        let transaction = self.connection.transaction().map_err(io::Error::other)?;
        let state: Option<(Option<String>, String)> = transaction
            .query_row(
                "SELECT configuration,latest FROM state WHERE singleton=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(io::Error::other)?;
        let mut runs = BTreeMap::new();
        {
            let mut query = transaction
                .prepare(
                    "SELECT id,data FROM invocations WHERE id IN
                 (SELECT invocation FROM tests UNION SELECT latest FROM state)",
                )
                .map_err(io::Error::other)?;
            let rows = query
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(io::Error::other)?;
            for row in rows {
                let (id, data) = row.map_err(io::Error::other)?;
                let run: Run = serde_json::from_str(&data)?;
                if run.id != id || run.schema != 1 || uuid::Uuid::parse_str(&id).is_err() {
                    return Err(io::Error::other(
                        "Invalid coverage invocation identity or format; recollect coverage.",
                    ));
                }
                runs.insert(id, run);
            }
        }
        let mut tests = Vec::new();
        {
            let mut query = transaction.prepare(if measurements {
                "SELECT binary,test,invocation,metadata,measurements FROM tests ORDER BY binary,test"
            } else {
                "SELECT binary,test,invocation,metadata,NULL FROM tests ORDER BY binary,test"
            }).map_err(io::Error::other)?;
            let rows = query
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                })
                .map_err(io::Error::other)?;
            for row in rows {
                let (binary, test, run, data, measurements) = row.map_err(io::Error::other)?;
                let metadata: Attempt = serde_json::from_str(&data)?;
                let invocation = runs
                    .get(&run)
                    .ok_or_else(|| io::Error::other("Missing coverage invocation metadata."))?;
                if metadata.binary != binary
                    || metadata.test != test
                    || uuid::Uuid::parse_str(&metadata.id).is_err()
                    || state
                        .as_ref()
                        .and_then(|(configuration, _)| configuration.as_ref())
                        != Some(&invocation.configuration)
                    || !metadata.processes.is_empty()
                    || !matches!(
                        metadata.outcome.as_str(),
                        "pending" | "running" | "success" | "failed" | "interrupted"
                    )
                {
                    return Err(io::Error::other(
                        "Invalid current coverage identity, state or configuration.",
                    ));
                }
                tests.push(StoredTest {
                    run,
                    metadata,
                    measurements,
                });
            }
        }
        if !tests.is_empty() && state.is_none()
            || state
                .as_ref()
                .is_some_and(|(_, latest)| !runs.contains_key(latest))
        {
            return Err(io::Error::other(
                "Missing active coverage configuration or invocation.",
            ));
        }
        transaction.commit().map_err(io::Error::other)?;
        Ok(Snapshot {
            configuration: state
                .as_ref()
                .and_then(|(configuration, _)| configuration.clone()),
            latest: state.map(|(_, latest)| latest),
            runs,
            tests,
        })
    }

    // {impl}`requirements.test-coverage.storage.cleanup`
    pub(super) fn cleanup(&mut self, preparing: Option<&str>) -> io::Result<()> {
        self.connection
            .execute(
                "DELETE FROM invocations WHERE id NOT IN
             (SELECT invocation FROM tests UNION SELECT latest FROM state)",
                [],
            )
            .map_err(io::Error::other)?;
        let snapshot = self.snapshot(false)?;
        let mut attempts: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for test in &snapshot.tests {
            attempts
                .entry(&test.run)
                .or_default()
                .insert(&test.metadata.id);
        }
        for entry in fs::read_dir(&self.storage)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if Some(name.as_ref()) == preparing {
                continue;
            }
            if entry.file_type()?.is_dir() {
                if !snapshot.runs.contains_key(name.as_ref()) {
                    fs::remove_dir_all(entry.path())?;
                } else if entry.path().join("attempts").is_dir() {
                    for attempt in fs::read_dir(entry.path().join("attempts"))? {
                        let attempt = attempt?;
                        if !attempts.get(name.as_ref()).is_some_and(|ids| {
                            ids.contains(attempt.file_name().to_string_lossy().as_ref())
                        }) {
                            fs::remove_dir_all(attempt.path())?;
                        }
                    }
                    for obsolete in ["setup", "tools"] {
                        let path = entry.path().join(obsolete);
                        if path.exists() {
                            fs::remove_dir_all(path)?;
                        }
                    }
                    let binaries = entry.path().join("binaries.json");
                    if binaries.exists() {
                        fs::remove_file(binaries)?;
                    }
                }
            } else if name == "selection-cache.json" {
                fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{FunctionExecution, Process};
    use super::*;

    fn identity(binary: &str, test: &str) -> TestIdentity {
        TestIdentity {
            binary: binary.into(),
            test: test.into(),
        }
    }

    fn invocation(configuration: &str, selected: &[TestIdentity]) -> Run {
        Run {
            schema: 1,
            id: uuid::Uuid::new_v4().to_string(),
            started: super::super::now(),
            command: vec!["cargo nextest run".into()],
            selection: selected.to_vec(),
            configuration: configuration.into(),
            configuration_inputs: BTreeMap::new(),
            sources: BTreeMap::from([("code.rs".into(), "fn code() {}".into())]),
            inputs: BTreeMap::new(),
            environment_inputs: BTreeMap::new(),
            revision: None,
            dirty: None,
            outcome: "interrupted".into(),
            diagnostics: vec![],
        }
    }

    fn attempt(test: &TestIdentity, number: &str, outcome: &str) -> Attempt {
        Attempt {
            id: uuid::Uuid::new_v4().to_string(),
            binary: test.binary.clone(),
            test: test.test.clone(),
            attempt: number.into(),
            started: super::super::now(),
            pid: 1,
            command: vec![],
            outcome: outcome.into(),
            processes: vec![],
            diagnostics: vec![],
        }
    }

    fn complete(store: &Store, run: &Run, test: &TestIdentity, count: u64) -> Attempt {
        let mut attempt = attempt(test, "1", "running");
        store.start_attempt(&run.id, &attempt).unwrap();
        let dir = store
            .storage
            .join(&run.id)
            .join("attempts")
            .join(&attempt.id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("1.profraw"), "raw profile").unwrap();
        fs::write(store.storage.join(&run.id).join("maps.json"), "shared maps").unwrap();
        attempt.outcome = "success".into();
        attempt.processes = vec![Process {
            pid: 1,
            profiles: vec!["1.profraw".into()],
            lines: BTreeMap::from([("code.rs".into(), BTreeMap::from([(1, count)]))]),
            functions: Some(vec![FunctionExecution {
                file: "code.rs".into(),
                line: 1,
                column: 1,
                count,
            }]),
            diagnostic: None,
        }];
        store.update_attempt(&run.id, &attempt, true).unwrap();
        attempt
    }

    // {test}`requirements.test-coverage.processes`
    // {test}`requirements.test-coverage.storage.current`
    // {test}`requirements.test-coverage.storage.outcomes`
    #[test]
    fn concurrent_writers_preserve_each_tests_state_and_measurements() {
        use std::sync::Barrier;

        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::open(temp.path()).unwrap();
        let selected: Vec<_> = (0..64)
            .map(|index| identity(&format!("binary-{index}"), "same_name"))
            .collect();
        let run = invocation("a", &selected);
        store.replace(&run, &selected, false).unwrap();
        let barrier = Barrier::new(selected.len());
        let attempts: Vec<_> = std::thread::scope(|scope| {
            let workers: Vec<_> = selected
                .iter()
                .enumerate()
                .map(|(index, test)| {
                    let barrier = &barrier;
                    let run = &run;
                    let storage = temp.path();
                    scope.spawn(move || {
                        let writer = Store::open(storage).unwrap();
                        barrier.wait();
                        complete(&writer, run, test, index as u64 + 1)
                    })
                })
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .collect()
        });

        let snapshot = store.snapshot(true).unwrap();
        assert_eq!(snapshot.tests.len(), selected.len());
        for (index, attempt) in attempts.iter().enumerate() {
            let result = snapshot
                .tests
                .iter()
                .find(|test| test.metadata.binary == selected[index].binary)
                .unwrap();
            assert_eq!(result.run, run.id);
            assert_eq!(result.metadata.id, attempt.id);
            assert_eq!(result.metadata.test, "same_name");
            assert_eq!(result.metadata.outcome, "success");
            let processes: Vec<Process> =
                serde_json::from_str(result.measurements.as_ref().unwrap()).unwrap();
            assert_eq!(processes.len(), 1);
            assert_eq!(processes[0].lines["code.rs"][&1], index as u64 + 1);
        }
    }

    // {test}`requirements.test-coverage.storage.current`
    // {test}`requirements.test-coverage.storage.replace`
    // {test}`requirements.test-coverage.storage.cleanup`
    // {test}`requirements.test-coverage.index.selection`
    // {test}`requirements.test-coverage.processes`
    #[test]
    fn focused_replacement_preserves_unselected_results_and_shared_artifacts() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::open(temp.path()).unwrap();
        let selected = [
            identity("lib", "same"),
            identity("integration", "same"),
            identity("lib", "removed"),
        ];
        let first = invocation("a", &selected);
        store.replace(&first, &selected, false).unwrap();
        let old: Vec<_> = selected
            .iter()
            .map(|test| complete(&store, &first, test, 7))
            .collect();
        let next = invocation("a", &selected[..1]);
        store.replace(&next, &selected[..1], true).unwrap();
        store.cleanup(None).unwrap();
        let snapshot = store.snapshot(true).unwrap();
        assert_eq!(snapshot.tests.len(), 3);
        let replaced = snapshot
            .tests
            .iter()
            .find(|test| test.metadata.binary == "lib" && test.metadata.test == "same")
            .unwrap();
        assert_eq!(replaced.metadata.outcome, "pending");
        assert!(replaced.measurements.is_none());
        assert_eq!(snapshot.runs[&first.id].sources, first.sources);
        for preserved in &old[1..] {
            let test = snapshot
                .tests
                .iter()
                .find(|test| test.metadata.id == preserved.id)
                .unwrap();
            assert_eq!(test.run, first.id);
            assert!(test.measurements.as_ref().unwrap().contains('7'));
            assert!(
                temp.path()
                    .join(&first.id)
                    .join("attempts")
                    .join(&preserved.id)
                    .join("1.profraw")
                    .is_file()
            );
        }
        assert!(
            !temp
                .path()
                .join(&first.id)
                .join("attempts")
                .join(&old[0].id)
                .exists()
        );
        assert!(temp.path().join(&first.id).join("maps.json").is_file());
        let full = invocation("a", &selected[..1]);
        store.replace(&full, &selected[..1], false).unwrap();
        store.cleanup(None).unwrap();
        assert_eq!(store.snapshot(false).unwrap().tests.len(), 1);
        assert!(!temp.path().join(&first.id).exists());
        let empty = invocation("a", &[]);
        store.replace(&empty, &[], false).unwrap();
        assert!(store.snapshot(true).unwrap().tests.is_empty());
    }

    // {test}`requirements.test-coverage.storage.configuration_reset`
    // {test}`requirements.test-coverage.storage.replace`
    // {test}`requirements.test-coverage.storage.outcomes`
    #[test]
    fn incompatible_builds_reset_every_test_but_empty_focused_runs_do_not() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::open(temp.path()).unwrap();
        let selected = [identity("lib", "a"), identity("lib", "b")];
        let original = invocation("a", &selected);
        store.replace(&original, &selected, false).unwrap();
        complete(&store, &original, &selected[0], 9);
        let empty = invocation("b", &[]);
        assert!(!store.replace(&empty, &[], true).unwrap());
        store.record_empty(&empty).unwrap();
        let snapshot = store.snapshot(true).unwrap();
        assert_eq!(snapshot.configuration.as_deref(), Some("a"));
        assert_eq!(snapshot.tests.len(), 2);
        assert_eq!(snapshot.latest.as_deref(), Some(empty.id.as_str()));
        assert!(
            snapshot
                .tests
                .iter()
                .any(|test| test.measurements.is_some())
        );
        let next = invocation("b", &selected[..1]);
        store.replace(&next, &selected[..1], true).unwrap();
        let snapshot = store.snapshot(true).unwrap();
        assert_eq!(snapshot.configuration.as_deref(), Some("b"));
        assert_eq!(snapshot.tests.len(), 1);
        assert!(snapshot.tests[0].measurements.is_none());
        let original_again = invocation("a", &selected[1..]);
        store
            .replace(&original_again, &selected[1..], true)
            .unwrap();
        let snapshot = store.snapshot(true).unwrap();
        assert_eq!(snapshot.tests.len(), 1);
        assert_eq!(snapshot.tests[0].metadata.test, "b");
        assert!(snapshot.tests[0].measurements.is_none());
        let two = invocation("a", &selected);
        store.replace(&two, &selected, false).unwrap();
        for test in &selected {
            complete(&store, &two, test, 7);
        }
        let full = invocation("b", &selected[..1]);
        store.replace(&full, &selected[..1], false).unwrap();
        let snapshot = store.snapshot(true).unwrap();
        assert_eq!(snapshot.configuration.as_deref(), Some("b"));
        assert_eq!(snapshot.tests.len(), 1);
        assert_eq!(snapshot.tests[0].metadata.test, "a");
        assert_eq!(snapshot.tests[0].metadata.outcome, "pending");
        assert!(snapshot.tests[0].measurements.is_none());
    }

    // {test}`requirements.test-coverage.storage.retries`
    // {test}`requirements.test-coverage.storage.outcomes`
    // {test}`requirements.test-coverage.index.selection`
    #[test]
    fn latest_started_retry_replaces_success_and_cannot_be_overwritten_by_old_conversion() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::open(temp.path()).unwrap();
        let selected = [identity("lib", "a")];
        let run = invocation("a", &selected);
        store.replace(&run, &selected, false).unwrap();
        let first = complete(&store, &run, &selected[0], 9);
        let mut retry = attempt(&selected[0], "2", "running");
        store.start_attempt(&run.id, &retry).unwrap();
        let snapshot = store.snapshot(true).unwrap();
        assert_eq!(snapshot.tests.len(), 1);
        assert_eq!(snapshot.tests[0].metadata.attempt, "2");
        assert_eq!(snapshot.tests[0].metadata.outcome, "running");
        assert!(snapshot.tests[0].measurements.is_none());
        assert!(store.update_attempt(&run.id, &first, true).is_err());
        retry.outcome = "failed".into();
        store.update_attempt(&run.id, &retry, false).unwrap();
        let snapshot = store.snapshot(true).unwrap();
        assert_eq!(snapshot.tests[0].metadata.outcome, "failed");
        assert!(snapshot.tests[0].measurements.is_none());
        let outside_selection = attempt(&identity("lib", "other"), "1", "running");
        assert!(store.start_attempt(&run.id, &outside_selection).is_err());
        retry.outcome = "interrupted".into();
        retry.processes = first.processes.clone();
        store.update_attempt(&run.id, &retry, true).unwrap();
        let snapshot = store.snapshot(true).unwrap();
        assert_eq!(snapshot.tests[0].metadata.outcome, "interrupted");
        assert!(snapshot.tests[0].measurements.is_some());
    }

    // {test}`requirements.test-coverage.storage.replace`
    // {test}`requirements.test-coverage.storage.configuration_reset`
    #[test]
    fn failed_atomic_replacement_preserves_results_configuration_and_invocation() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::open(temp.path()).unwrap();
        let selected = [identity("lib", "a")];
        let original = invocation("a", &selected);
        store.replace(&original, &selected, false).unwrap();
        let completed = complete(&store, &original, &selected[0], 9);
        store.connection.execute_batch(
            "CREATE TEMP TRIGGER reject_test BEFORE INSERT ON tests BEGIN SELECT RAISE(ABORT,'fixture write failure'); END;"
        ).unwrap();
        assert!(
            store
                .replace(&invocation("b", &selected), &selected, false)
                .is_err()
        );
        let snapshot = store.snapshot(true).unwrap();
        assert_eq!(snapshot.configuration.as_deref(), Some("a"));
        assert_eq!(snapshot.latest.as_deref(), Some(original.id.as_str()));
        assert_eq!(snapshot.tests[0].metadata.id, completed.id);
        assert!(snapshot.tests[0].measurements.is_some());
    }

    // {test}`requirements.test-coverage.index.selection`
    // {test}`requirements.test-coverage.check.measurement`
    #[test]
    fn metadata_queries_skip_measurements_and_snapshots_survive_subsequent_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::open(temp.path()).unwrap();
        let selected = [identity("lib", "a")];
        let first = invocation("a", &selected);
        store.replace(&first, &selected, false).unwrap();
        let completed = complete(&store, &first, &selected[0], 7);
        let mut reader = Store::read(temp.path()).unwrap();
        let snapshot = reader.snapshot(true).unwrap();
        store
            .replace(&invocation("b", &selected), &selected, false)
            .unwrap();
        assert_eq!(snapshot.configuration.as_deref(), Some("a"));
        assert_eq!(snapshot.tests[0].metadata.id, completed.id);
        let processes: Vec<Process> =
            serde_json::from_str(snapshot.tests[0].measurements.as_ref().unwrap()).unwrap();
        assert_eq!(processes[0].lines["code.rs"][&1], 7);
        store
            .connection
            .execute("UPDATE tests SET measurements='not valid JSON'", [])
            .unwrap();
        let metadata = reader.snapshot(false).unwrap();
        assert_eq!(metadata.configuration.as_deref(), Some("b"));
        assert!(metadata.tests[0].measurements.is_none());
        assert!(
            reader.snapshot(true).unwrap().tests[0]
                .measurements
                .as_ref()
                .unwrap()
                .contains("not valid")
        );
    }

    // {test}`requirements.test-coverage.index.selection`
    #[test]
    fn replacement_between_snapshot_queries_cannot_mix_configurations_or_results() {
        use rusqlite::trace::{TraceEvent, TraceEventCodes};
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let temp = tempfile::tempdir().unwrap();
        let mut writer = Store::open(temp.path()).unwrap();
        let selected = [identity("lib", "a")];
        let first = invocation("a", &selected);
        writer.replace(&first, &selected, false).unwrap();
        let original = complete(&writer, &first, &selected[0], 7);
        let second = invocation("b", &selected);
        let second_id = second.id.clone();
        let committed = Arc::new(AtomicBool::new(false));
        let signal = committed.clone();
        let mut reader = Store::read(temp.path()).unwrap();
        reader
            .connection
            .trace_v2(
                TraceEventCodes::SQLITE_TRACE_STMT,
                Some(move |event: TraceEvent<'_>| {
                    if let TraceEvent::Stmt(_, sql) = event
                        && sql.starts_with("SELECT id,data FROM invocations")
                        && !signal.load(Ordering::SeqCst)
                    {
                        // The state query has established the reader's snapshot.
                        // Commit replacement while that same read is in progress.
                        writer.replace(&second, &selected, false).unwrap();
                        complete(&writer, &second, &selected[0], 9);
                        signal.store(true, Ordering::SeqCst);
                    }
                }),
            )
            .unwrap();
        let snapshot = reader.snapshot(true).unwrap();
        // SQLite callbacks catch panics: this also verifies the writer succeeded.
        assert!(committed.load(Ordering::SeqCst));
        assert_eq!(snapshot.configuration.as_deref(), Some("a"));
        assert_eq!(snapshot.latest.as_deref(), Some(first.id.as_str()));
        assert_eq!(snapshot.tests.len(), 1);
        assert_eq!(snapshot.tests[0].run, first.id);
        assert_eq!(snapshot.tests[0].metadata.id, original.id);
        let measurements: Vec<Process> =
            serde_json::from_str(snapshot.tests[0].measurements.as_ref().unwrap()).unwrap();
        assert_eq!(measurements[0].lines["code.rs"][&1], 7);
        let next = reader.snapshot(true).unwrap();
        assert_eq!(next.configuration.as_deref(), Some("b"));
        assert_eq!(next.latest.as_deref(), Some(second_id.as_str()));
        assert_eq!(next.tests[0].run, second_id);
        assert_ne!(next.tests[0].metadata.id, original.id);
        let measurements: Vec<Process> =
            serde_json::from_str(next.tests[0].measurements.as_ref().unwrap()).unwrap();
        assert_eq!(measurements[0].lines["code.rs"][&1], 9);
    }

    // {test}`requirements.test-coverage.storage.cleanup`
    // {test}`requirements.test-coverage.failures.profiles`
    #[test]
    fn recovery_removes_orphans_and_cache_but_preserves_current_diagnostic_profiles() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::open(temp.path()).unwrap();
        let selected = [identity("lib", "a")];
        let run = invocation("a", &selected);
        store.replace(&run, &selected, false).unwrap();
        let mut completed = complete(&store, &run, &selected[0], 7);
        completed.outcome = "failed".into();
        store.update_attempt(&run.id, &completed, true).unwrap();
        let orphan = temp.path().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(orphan.join("attempts")).unwrap();
        fs::write(orphan.join("old.profraw"), "superseded").unwrap();
        fs::write(temp.path().join("selection-cache.json"), "obsolete").unwrap();
        store.cleanup(None).unwrap();
        assert!(!orphan.exists());
        assert!(!temp.path().join("selection-cache.json").exists());
        assert!(
            temp.path()
                .join(&run.id)
                .join("attempts")
                .join(&completed.id)
                .join("1.profraw")
                .exists()
        );
        assert_eq!(
            store.snapshot(true).unwrap().tests[0].metadata.outcome,
            "failed"
        );
    }

    // {test}`requirements.test-coverage.storage.replace`
    #[test]
    fn collector_ownership_is_released_when_the_owner_exits() {
        let temp = tempfile::tempdir().unwrap();
        let lock = CollectorLock::acquire(temp.path()).unwrap();
        assert!(CollectorLock::acquire(temp.path()).is_err());
        drop(lock);
        CollectorLock::acquire(temp.path()).unwrap();
    }
}

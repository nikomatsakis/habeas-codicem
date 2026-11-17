//! Disposable LLVM execution coverage. This is independent of evidence-input coverage.
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

mod book;
mod check;
mod configuration;
mod notes;
mod progress;
mod runner;

pub use book::marker_ranges;
pub(crate) use book::populate;
pub use check::{CheckReport, check, test};
pub use configuration::{configure, validate_setup, workspace_root};
pub use runner::{collect_process, compile_process, run};

// {impl}`requirements.test-coverage.check.guidance`
// {impl}`requirements.test-coverage.check.exceptions.setup`
pub const CHAPTER: &str = "# Test coverage\n\nCollect Rust unit and integration test coverage with `habeas coverage`.\nFor focused runs, forward nextest arguments with `habeas coverage -- -E 'test(name)'`.\nRun doctests separately with `cargo test --doc --workspace`.\n\nFor full verification, `habeas test` collects coverage for the whole workspace,\nthen checks it. In CI, run `habeas coverage -- --workspace` and, after it succeeds,\nrun `habeas coverage-check` as a separate step. Checking saved coverage runs no\ntests and calls no model; the static validation test needs no prior coverage.\n\nEvery first-party Rust file needs 80% instrumented-line coverage. Every function\nwith zero execution or no measurement needs an accepted explanation, including\nfunctions excluded by features or platform. Ordinary partly covered functions\nneed no exception when their file meets its target.\n\nUse a standard `habeas:coverage` citation with a substantive prose justification\nin fenced commentary or a Rust doc comment. A file-root citation excuses only\nthe file threshold; a function citation excuses only that function. Author its\nindependent citation review with `habeas author`; only a current accepted review\npermits an exception. No project declaration or special configuration is needed.\nCitations remain applicable across build configurations and can remain after\ncoverage improves. Exceptions never increase measured percentages.\n\nCoverage is disposable build output in the Cargo target directory. The index\ncombines each test's latest compatible attempt; source changes invalidate its\ncontribution. Building this book reads saved reports and never runs tests.\n\nThe overview maps project files to their line coverage percentages. Choose a\ntest to see only the files it executes, including its child processes. Expand\nthe optional details to inspect run metadata or individual source lines.\n\n:::{coverage-index}\n:::\n";

// {impl}`requirements.test-coverage.storage`
// {impl}`requirements.test-coverage.storage.provenance`
// {impl}`requirements.test-coverage.storage.configuration`
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Run {
    schema: u32,
    id: String,
    started: u128,
    command: Vec<String>,
    selection: Vec<String>,
    configuration: String,
    configuration_inputs: BTreeMap<String, String>,
    sources: BTreeMap<String, String>,
    inputs: BTreeMap<String, String>,
    environment_inputs: BTreeMap<String, Option<String>>,
    revision: Option<String>,
    dirty: Option<bool>,
    outcome: String,
    diagnostics: Vec<String>,
}

// {impl}`requirements.test-coverage.processes`
// {impl}`requirements.test-coverage.storage.outcomes`
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Attempt {
    id: String,
    binary: String,
    test: String,
    attempt: String,
    started: u128,
    pid: u32,
    command: Vec<String>,
    outcome: String,
    processes: Vec<Process>,
    diagnostics: Vec<String>,
}

// {impl}`requirements.test-coverage.processes.reports`
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Process {
    pid: u32,
    profiles: Vec<String>,
    lines: BTreeMap<String, BTreeMap<u32, u64>>,
    /// Absent in older line-only captures; those remain browsable in the book.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    functions: Option<Vec<FunctionExecution>>,
    diagnostic: Option<String>,
}

// {impl}`requirements.test-coverage.check.functions`
// {impl}`requirements.test-coverage.check.measurement`
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FunctionExecution {
    file: String,
    /// LLVM's entry-region start, with one-based line and byte column.
    line: u32,
    column: u32,
    count: u64,
}

// {cite}`habeas:util`
fn now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

// {cite}`habeas:util`
fn digest(bytes: &[u8]) -> String {
    // libgit2 hashes the actual bytes, including dirty and untracked inputs.
    // Its native implementation also keeps capture fast in debug builds.
    git2::Oid::hash_object(git2::ObjectType::Blob, bytes)
        .expect("cannot hash coverage input")
        .to_string()
}

// {cite}`habeas:util`
fn save(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    fs::write(&temporary, serde_json::to_vec_pretty(value)?)?;
    fs::rename(temporary, path)
}

// {cite}`habeas:util`
fn load<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<T> {
    serde_json::from_slice(&fs::read(path)?).map_err(io::Error::other)
}

// {cite}`habeas:util`
fn walk(root: &Path, excluded: &[PathBuf]) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !root.is_dir() {
        return Ok(files);
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if excluded.iter().any(|excluded| path.starts_with(excluded))
            || matches!(
                entry.file_name().to_str(),
                Some(".git" | "node_modules" | "book")
            )
        {
            continue;
        }
        if entry.file_type()?.is_dir() {
            files.extend(walk(&path, excluded)?);
        } else if entry.file_type()?.is_file()
            || (entry.file_type()?.is_symlink() && path.is_file())
        {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

// Includes untracked Rust sources too: adding a source invalidates the whole workspace.
fn workspace_inputs(root: &Path, target: &Path) -> io::Result<Vec<PathBuf>> {
    Ok(walk(root, &[target.to_path_buf(), root.join("target")])?
        .into_iter()
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "rs")
                || matches!(
                    path.file_name().and_then(|name| name.to_str()),
                    Some("Cargo.toml" | "Cargo.lock" | "rust-toolchain" | "rust-toolchain.toml")
                )
                || path.strip_prefix(root).is_ok_and(|relative| {
                    relative
                        .components()
                        .any(|part| part.as_os_str() == ".cargo")
                        && (path.extension().is_some_and(|ext| ext == "toml")
                            || path.file_name().is_some_and(|name| name == "config"))
                })
        })
        .collect())
}

// {cite}`habeas:util`
fn input_hashes(paths: &[PathBuf]) -> io::Result<BTreeMap<String, String>> {
    paths
        .iter()
        .map(|path| {
            Ok((
                path.to_string_lossy().into_owned(),
                input_digest(path, None)?,
            ))
        })
        .collect()
}

// {cite}`habeas:util`
fn input_digest(path: &Path, target: Option<&Path>) -> io::Result<String> {
    if path.is_dir() {
        let excluded = target
            .filter(|target| target.starts_with(path) && !path.starts_with(target))
            .map(|target| vec![target.to_path_buf()])
            .unwrap_or_default();
        let files = watched_files(path, &excluded)?;
        return Ok(digest(&serde_json::to_vec(&input_hashes(&files)?)?));
    }
    match fs::read(path) {
        Ok(bytes) => Ok(digest(&bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok("missing".into()),
        Err(error) => Err(error),
    }
}

// Build-script directory watches can include non-Rust files, including files
// beneath names excluded from ordinary first-party source discovery.
// {cite}`habeas:util`
fn watched_files(root: &Path, excluded: &[PathBuf]) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if excluded.iter().any(|excluded| path.starts_with(excluded)) || entry.file_name() == ".git"
        {
            continue;
        }
        if entry.file_type()?.is_dir() {
            files.extend(watched_files(&path, excluded)?);
        } else if path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

// Cargo derives these per package or target from inputs already captured in
// the manifest, compiler and feature ledger. Its runtime values describe the
// program launching this reader, rather than the package whose coverage it reads.
// {cite}`habeas:util`
fn cargo_derived_environment(name: &str) -> bool {
    matches!(
        name,
        "CARGO"
            | "CARGO_MANIFEST_DIR"
            | "CARGO_MANIFEST_PATH"
            | "CARGO_MANIFEST_LINKS"
            | "CARGO_CRATE_NAME"
            | "CARGO_BIN_NAME"
            | "CARGO_PRIMARY_PACKAGE"
            | "CARGO_TARGET_TMPDIR"
            | "OUT_DIR"
    ) || [
        "CARGO_PKG_",
        "CARGO_CFG_",
        "CARGO_FEATURE_",
        "CARGO_BIN_EXE_",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
}

// {impl}`requirements.test-coverage.storage.provenance`
fn fresh(run: &Run, root: &Path, target: &Path) -> io::Result<bool> {
    if run.schema != 1 {
        return Ok(false);
    }
    for path in workspace_inputs(root, target)? {
        if run.inputs.get(&path.to_string_lossy().into_owned()) != Some(&digest(&fs::read(path)?)) {
            return Ok(false);
        }
    }
    for (path, expected) in &run.inputs {
        if input_digest(Path::new(path), Some(target)).ok().as_ref() != Some(expected) {
            return Ok(false);
        }
    }
    for (name, expected) in &run.environment_inputs {
        // Also recognize these in previously saved reports.
        if !cargo_derived_environment(name) && &std::env::var(name).ok() != expected {
            return Ok(false);
        }
    }
    Ok(true)
}

// Cargo reads ancestor configuration from the invocation directory, then Cargo home.
// Keep missing candidates too, so adding a configuration invalidates older records.
// {cite}`habeas:util`
fn cargo_configs(project: &Path) -> Vec<PathBuf> {
    let mut paths = project
        .ancestors()
        .flat_map(|directory| {
            ["config", "config.toml"].map(|name| directory.join(".cargo").join(name))
        })
        .collect::<Vec<_>>();
    if let Some(home) = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))
    {
        paths.extend(["config", "config.toml"].map(|name| home.join(name)));
    }
    paths
}

// Read Cargo's target location without compiling or executing tests.
// {cite}`habeas:util`
pub fn target_directory(root: &Path) -> io::Result<PathBuf> {
    let project = fs::canonicalize(root)?;
    let root = workspace_root(&project)?;
    if let Some(path) =
        std::env::var_os("CARGO_TARGET_DIR").or_else(|| std::env::var_os("CARGO_BUILD_TARGET_DIR"))
    {
        let path = PathBuf::from(path);
        return Ok(if path.is_absolute() {
            path
        } else {
            project.join(path)
        });
    }
    for directory in project.ancestors() {
        for name in ["config", "config.toml"] {
            let path = directory.join(".cargo").join(name);
            if !path.is_file() {
                continue;
            }
            let source: toml::Table = fs::read_to_string(&path)?
                .parse()
                .map_err(io::Error::other)?;
            if let Some(target) = source
                .get("build")
                .and_then(|build| build.get("target-dir"))
                .and_then(toml::Value::as_str)
            {
                let path = PathBuf::from(target);
                return Ok(if path.is_absolute() {
                    path
                } else {
                    directory.join(path)
                });
            }
        }
    }
    if let Some(home) = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))
    {
        for name in ["config", "config.toml"] {
            let path = home.join(name);
            if !path.is_file() {
                continue;
            }
            let source: toml::Table = fs::read_to_string(&path)?
                .parse()
                .map_err(io::Error::other)?;
            if let Some(target) = source
                .get("build")
                .and_then(|build| build.get("target-dir"))
                .and_then(toml::Value::as_str)
            {
                let path = PathBuf::from(target);
                return Ok(if path.is_absolute() {
                    path
                } else {
                    home.parent().unwrap_or(&root).join(path)
                });
            }
        }
    }
    Ok(root.join("target"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // {test}`requirements.test-coverage.storage.provenance`
    // {test}`requirements.test-coverage.index.freshness`
    #[test]
    fn source_and_build_inputs_invalidate_on_addition_removal_and_dirty_content() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let target = root.join("target");
        fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(root.join("Cargo.lock"), "lock content").unwrap();
        fs::write(root.join("code.rs"), "fn original() {}\n").unwrap();
        fs::create_dir(root.join("generated-inputs")).unwrap();
        fs::create_dir(root.join("generated-inputs/node_modules")).unwrap();
        fs::write(root.join("generated-inputs/data"), "initial").unwrap();
        let mut inputs = input_hashes(&workspace_inputs(root, &target).unwrap()).unwrap();
        for path in [root.join("generated-inputs"), root.join("not-created-yet")] {
            inputs.insert(
                path.to_string_lossy().into_owned(),
                input_digest(&path, Some(&target)).unwrap(),
            );
        }
        let run = Run {
            schema: 1,
            id: "fixture".into(),
            started: 1,
            command: vec![],
            selection: vec![],
            configuration: "config".into(),
            configuration_inputs: BTreeMap::new(),
            sources: BTreeMap::new(),
            inputs,
            environment_inputs: BTreeMap::new(),
            revision: Some("unchanged revision".into()),
            dirty: Some(true),
            outcome: "success".into(),
            diagnostics: vec![],
        };
        assert!(fresh(&run, root, &target).unwrap());
        for (path, content) in [
            ("code.rs", "fn dirty() {}\n"),
            ("added.rs", "fn added() {}\n"),
            ("Cargo.lock", "different lock"),
            (
                "Cargo.toml",
                "[workspace]\n[workspace.package]\nversion = '2.0.0'\n",
            ),
            (
                "member/Cargo.toml",
                "[package]\nname = 'other'\nversion = '1.0.0'\n",
            ),
            ("rust-toolchain.toml", "[toolchain]\nchannel = 'stable'\n"),
            ("generated-inputs/new", "new watched input"),
            (
                "generated-inputs/node_modules/data",
                "watched non-Rust input",
            ),
            ("not-created-yet", "now created"),
        ] {
            let path = root.join(path);
            let previous = fs::read(&path).ok();
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, content).unwrap();
            assert!(!fresh(&run, root, &target).unwrap(), "{}", path.display());
            match previous {
                Some(previous) => fs::write(path, previous).unwrap(),
                None => fs::remove_file(path).unwrap(),
            }
            assert!(fresh(&run, root, &target).unwrap());
        }
        for name in ["Cargo.toml", "Cargo.lock"] {
            let path = root.join(name);
            let original = fs::read(&path).unwrap();
            fs::remove_file(&path).unwrap();
            assert!(!fresh(&run, root, &target).unwrap(), "{name}");
            fs::write(path, original).unwrap();
            assert!(fresh(&run, root, &target).unwrap());
        }
        fs::remove_file(root.join("code.rs")).unwrap();
        assert!(!fresh(&run, root, &target).unwrap());
    }
}

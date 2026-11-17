use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
};

use super::progress::Progress;
use super::{
    Attempt, FunctionExecution, Process, Run, configuration, digest, input_hashes, load, now, save,
    walk, workspace_inputs,
};

// {cite}`habeas:util`
fn output(command: &mut Command) -> io::Result<String> {
    let result = command.output()?;
    if !result.status.success() {
        return Err(io::Error::other(format!(
            "{command:?} failed: {}",
            String::from_utf8_lossy(&result.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&result.stdout).trim().to_owned())
}

// {cite}`habeas:util`
fn code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(1)
}

// {cite}`habeas:util`
struct Interrupts {
    interrupted: Arc<AtomicBool>,
    child: Arc<AtomicU32>,
    #[cfg(unix)]
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    #[cfg(unix)]
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Interrupts {
    // {cite}`habeas:util`
    fn new() -> io::Result<Self> {
        let interrupted = Arc::new(AtomicBool::new(false));
        let child = Arc::new(AtomicU32::new(0));
        #[cfg(unix)]
        let (shutdown, thread) = {
            let flag = interrupted.clone();
            let pid = child.clone();
            let (ready, receiver) = std::sync::mpsc::sync_channel(1);
            let (shutdown, mut stopped) = tokio::sync::oneshot::channel();
            let thread = std::thread::spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                runtime.block_on(async move {
                    let mut signal = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt()) {
                        Ok(signal) => signal,
                        Err(error) => { let _ = ready.send(Err(error)); return; }
                    };
                    let _ = ready.send(Ok(()));
                    loop {
                        tokio::select! {
                            _ = &mut stopped => break,
                            received = signal.recv() => {
                                if received.is_none() { break; }
                                let again = flag.swap(true, Ordering::SeqCst);
                                eprintln!("Coverage interrupted; stopping nextest and retaining collected profiles.");
                                let pid = pid.load(Ordering::SeqCst);
                                if pid != 0 {
                                    match Command::new("kill").args([if again { "-KILL" } else { "-INT" }, &pid.to_string()]).output() {
                                        Ok(result) if result.status.success() => {}
                                        Ok(result) => eprintln!("Cannot interrupt nextest: {}", String::from_utf8_lossy(&result.stderr)),
                                        Err(error) => eprintln!("Cannot interrupt nextest: {error}"),
                                    }
                                }
                            }
                        }
                    }
                });
            });
            receiver.recv().map_err(io::Error::other)??;
            (Some(shutdown), Some(thread))
        };
        Ok(Self {
            interrupted,
            child,
            #[cfg(unix)]
            shutdown,
            #[cfg(unix)]
            thread,
        })
    }

    // {cite}`habeas:util`
    fn execute(&self, command: &mut Command) -> io::Result<ExitStatus> {
        let mut child = command.spawn()?;
        self.child.store(child.id(), Ordering::SeqCst);
        let result = child.wait();
        self.child.store(0, Ordering::SeqCst);
        result
    }

    // {cite}`habeas:util`
    fn happened(&self) -> bool {
        self.interrupted.load(Ordering::SeqCst)
    }
}

impl Drop for Interrupts {
    // {cite}`habeas:util`
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            if let Some(shutdown) = self.shutdown.take() {
                let _ = shutdown.send(());
            }
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
}

fn options(args: &[String], names: &[&str]) -> Vec<String> {
    let mut result = Vec::new();
    for (index, arg) in args
        .iter()
        .take_while(|arg| arg.as_str() != "--")
        .enumerate()
    {
        for name in names {
            if arg == name {
                if let Some(value) = args.get(index + 1) {
                    result.push(value.clone());
                }
            } else if let Some(value) = arg.strip_prefix(&format!("{name}=")) {
                result.push(value.to_owned());
            } else if name.len() == 2
                && name.starts_with('-')
                && !name.starts_with("--")
                && let Some(value) = arg.strip_prefix(name).filter(|value| !value.is_empty())
            {
                result.push(value.to_owned());
            }
        }
    }
    result
}

// {cite}`habeas:util`
fn option(args: &[String], names: &[&str]) -> Option<String> {
    options(args, names).pop()
}

// {cite}`habeas:util`
fn features(args: &[String]) -> String {
    options(args, &["--features", "-F"])
        .iter()
        .flat_map(|value| value.split([',', ' ']))
        .filter(|part| !part.is_empty())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(",")
}

// {cite}`habeas:util`
fn has(args: &[String], flag: &str) -> bool {
    args.iter()
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| arg == flag)
}

// {impl}`requirements.test-coverage.failures.prerequisites`
fn tools(compiler: &str) -> io::Result<(PathBuf, PathBuf, String)> {
    let version = output(Command::new("cargo").args(["nextest", "--version"]))
        .map_err(|error| io::Error::other(format!("cargo nextest >= 0.9.116 is required; install with `cargo install cargo-nextest --locked`: {error}")))?;
    let numbers = version
        .split_whitespace()
        .nth(1)
        .unwrap_or("")
        .split('.')
        .map(|number| number.parse::<u32>().unwrap_or(0))
        .collect::<Vec<_>>();
    if numbers.as_slice() < [0, 9, 116].as_slice() {
        return Err(io::Error::other(
            "cargo nextest >= 0.9.116 is required; update with `cargo install cargo-nextest --locked`",
        ));
    }
    let rustc = output(Command::new(compiler).arg("-vV"))?;
    let host = rustc
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .ok_or_else(|| io::Error::other("rustc did not report its host"))?;
    let sysroot = output(Command::new(compiler).args(["--print", "sysroot"]))?;
    let bin = PathBuf::from(sysroot)
        .join("lib/rustlib")
        .join(host)
        .join("bin");
    let cov = bin.join(format!("llvm-cov{}", std::env::consts::EXE_SUFFIX));
    let profdata = bin.join(format!("llvm-profdata{}", std::env::consts::EXE_SUFFIX));
    let objcopy = bin.join(format!("llvm-objcopy{}", std::env::consts::EXE_SUFFIX));
    let readobj = bin.join(format!("llvm-readobj{}", std::env::consts::EXE_SUFFIX));
    for path in [&cov, &profdata, &objcopy, &readobj] {
        let version = output(Command::new(path).arg("--version")).map_err(|error| {
            io::Error::other(format!("matching Rust LLVM coverage tooling is required; run `rustup component add llvm-tools-preview`: {error}"))
        })?;
        let llvm = rustc
            .lines()
            .find_map(|line| line.strip_prefix("LLVM version: "))
            .unwrap_or("");
        let major = llvm.split('.').next().unwrap_or("");
        if major.is_empty() || !version.contains(&format!("version {major}.")) {
            return Err(io::Error::other(format!(
                "{} must match rustc's LLVM {llvm}; run `rustup component add llvm-tools-preview` for the active toolchain",
                path.display()
            )));
        }
    }
    Ok((cov, profdata, rustc))
}

// Instruments workspace rustc invocations without replacing Cargo's resolved rustflags.
// {impl}`requirements.test-coverage.run`
// {impl}`requirements.test-coverage.storage.configuration`
pub fn compile_process(args: &[OsString]) -> io::Result<i32> {
    let compiler = args
        .first()
        .ok_or_else(|| io::Error::other("missing rustc invocation"))?;
    let prior = std::env::var_os("HABEAS_PREVIOUS_WORKSPACE_WRAPPER");
    let mut command = Command::new(prior.as_ref().unwrap_or(compiler));
    if prior.is_some() {
        command.arg(compiler);
    }
    command.args(&args[1..]);
    // Queries don't build an instrumented program.
    if args.iter().any(|arg| arg == "--crate-name") {
        command.args(["-C", "instrument-coverage"]);
        if let (Some(ledger), Some(package)) = (
            std::env::var_os("HABEAS_COVERAGE_FEATURE_LEDGER"),
            std::env::var_os("CARGO_MANIFEST_DIR"),
        ) {
            let arguments = args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            let features = arguments
                .windows(2)
                .filter(|pair| pair[0] == "--cfg")
                .filter_map(|pair| pair[1].strip_prefix("feature="))
                .map(|feature| feature.trim_matches('"').to_owned())
                .collect::<std::collections::BTreeSet<_>>();
            let package = format!(
                "{} [{}]",
                package.to_string_lossy(),
                option(&arguments, &["--target"]).unwrap_or_else(|| "host".into())
            );
            let ledger = PathBuf::from(ledger);
            fs::create_dir_all(&ledger)?;
            save(
                &ledger.join(format!("{}.json", digest(package.as_bytes()))),
                &(package, features),
            )?;
        }
    }
    command.env_remove("LLVM_PROFILE_FILE");
    Ok(code(command.status()?))
}

// {impl}`requirements.test-coverage.run.configuration`
fn verify_configuration(root: &Path, args: &[String]) -> io::Result<()> {
    for flag in [
        "--tool-config-file",
        "--archive-file",
        "--binaries-metadata",
        "--cargo-metadata",
        "--target-dir-remap",
        "--build-dir-remap",
    ] {
        if option(args, &[flag]).is_some() {
            return Err(io::Error::other(format!(
                "{flag} cannot be proven to use this instrumented build and collector; use the workspace configuration for `habeas coverage`"
            )));
        }
    }
    let profile = option(args, &["--profile", "-P"])
        .or_else(|| std::env::var("NEXTEST_PROFILE").ok())
        .unwrap_or_else(|| "default".to_owned());
    let path = option(args, &["--config-file"])
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(".config/nextest.toml"));
    configuration::check(
        &fs::read_to_string(&path).map_err(|error| {
            io::Error::other(format!("{}: {error}; run `habeas setup`", path.display()))
        })?,
        &path,
        &profile,
    )
}

fn build_configuration(
    invocation: &Path,
    args: &[String],
    rustc: &str,
) -> io::Result<BTreeMap<String, String>> {
    let mut inputs = BTreeMap::new();
    inputs.insert("rustc".into(), rustc.into());
    inputs.insert(
        "target".into(),
        option(args, &["--target"])
            .or(cargo_override_setting(invocation, args, "target")?)
            .or_else(|| std::env::var("CARGO_BUILD_TARGET").ok())
            .or(cargo_build_setting(invocation, "target")?)
            .unwrap_or_else(|| {
                rustc
                    .lines()
                    .find_map(|line| line.strip_prefix("host: "))
                    .unwrap_or("unknown host")
                    .to_owned()
            }),
    );
    inputs.insert(
        "cargo-profile".into(),
        option(args, &["--cargo-profile"]).unwrap_or_else(|| {
            if has(args, "--release") || has(args, "-r") {
                "release".into()
            } else {
                "test".into()
            }
        }),
    );
    inputs.insert("features".into(), features(args));
    inputs.insert(
        "all-features".into(),
        has(args, "--all-features").to_string(),
    );
    inputs.insert(
        "no-default-features".into(),
        has(args, "--no-default-features").to_string(),
    );
    inputs.insert(
        "instrumentation".into(),
        "rustc-workspace-wrapper:-C instrument-coverage;v1".into(),
    );
    inputs.insert(
        "cargo-overrides".into(),
        serde_json::to_string(&options(args, &["--config"]))?,
    );
    for (name, value) in std::env::vars().filter(|(name, _)| {
        name.contains("RUSTFLAGS")
            || name.contains("RUSTC")
            || name.starts_with("CARGO_PROFILE_")
            || name.starts_with("CARGO_TARGET_")
            || name == "RUSTUP_TOOLCHAIN"
    }) {
        inputs.insert(name, value);
    }
    // Cargo config can supply rustflags, target, linker and profile settings.
    for path in super::cargo_configs(invocation)
        .into_iter()
        .filter(|path| path.is_file())
    {
        let config: toml::Table = fs::read_to_string(&path)?
            .parse()
            .map_err(io::Error::other)?;
        if let Some(environment) = config.get("env") {
            inputs.insert(
                format!("{}.env", path.display()),
                digest(environment.to_string().as_bytes()),
            );
        }
        let compiler = config
            .into_iter()
            .filter(|(key, _)| matches!(key.as_str(), "build" | "target" | "profile"))
            .collect::<toml::Table>();
        if !compiler.is_empty() {
            inputs.insert(
                path.to_string_lossy().into_owned(),
                toml::to_string(&compiler).map_err(io::Error::other)?,
            );
        }
    }
    Ok(inputs)
}

// {cite}`habeas:util`
fn cargo_build_setting(invocation: &Path, key: &str) -> io::Result<Option<String>> {
    for path in super::cargo_configs(invocation)
        .into_iter()
        .filter(|path| path.is_file())
    {
        let config: toml::Table = fs::read_to_string(path)?
            .parse()
            .map_err(io::Error::other)?;
        if let Some(value) = config.get("build").and_then(|build| build.get(key)) {
            return Ok(Some(
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string()),
            ));
        }
    }
    Ok(None)
}

// {cite}`habeas:util`
fn cargo_override_setting(
    invocation: &Path,
    args: &[String],
    key: &str,
) -> io::Result<Option<String>> {
    for value in options(args, &["--config"]).into_iter().rev() {
        let source = if value.contains('=') {
            value
        } else {
            fs::read_to_string(invocation.join(value))?
        };
        let config: toml::Table = source.parse().map_err(io::Error::other)?;
        if let Some(value) = config.get("build").and_then(|build| build.get(key)) {
            return Ok(Some(
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string()),
            ));
        }
    }
    Ok(None)
}

// Execution-only flags conflict with nextest's --no-run preparation. Preserve
// them on the actual run, where nextest owns their meaning.
// {cite}`habeas:util`
fn preparation_args(args: &[OsString]) -> Vec<OsString> {
    let mut result = Vec::new();
    let mut index = 0;
    while index < args.len() && args[index] != "--" {
        let arg = args[index].to_string_lossy();
        let name = arg.split('=').next().unwrap();
        if matches!(name, "--fail-fast" | "--ff" | "--no-fail-fast" | "--nff") {
            index += 1;
        } else if matches!(name, "--max-fail" | "--debugger" | "--tracer") {
            index += if arg.contains('=') { 1 } else { 2 };
        } else {
            result.push(args[index].clone());
            index += 1;
        }
    }
    result.extend_from_slice(&args[index.min(args.len())..]);
    result
}

// {impl}`requirements.test-coverage.run`
// {impl}`requirements.test-coverage.storage`
// {impl}`requirements.test-coverage.processes.reports`
// {impl}`requirements.test-coverage.processes.progress.preparation`
// {impl}`requirements.test-coverage.storage.provenance`
// {impl}`requirements.test-coverage.storage.configuration`
// {impl}`requirements.test-coverage.storage.outcomes`
// {impl}`requirements.test-coverage.failures`
pub fn run(project: &Path, supplied: &[String]) -> io::Result<i32> {
    let root = configuration::workspace_root(project)?;
    let invocation = std::env::current_dir()?;
    let args = if supplied.is_empty() {
        vec!["--workspace".to_owned()]
    } else {
        supplied.to_vec()
    };
    verify_configuration(&root, &args)?;
    let compiler_command = cargo_override_setting(&invocation, &args, "rustc")?
        .or(std::env::var("RUSTC")
            .ok()
            .or(std::env::var("CARGO_BUILD_RUSTC").ok()))
        .or(cargo_build_setting(&invocation, "rustc")?)
        .unwrap_or_else(|| "rustc".into());
    let (cov, profdata, rustc) = tools(&compiler_command)?;
    let mut metadata_command = Command::new("cargo");
    metadata_command
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(&invocation);
    for flag in [
        "--locked",
        "--offline",
        "--frozen",
        "--all-features",
        "--no-default-features",
    ] {
        if has(&args, flag) {
            metadata_command.arg(flag);
        }
    }
    for flag in ["--manifest-path"] {
        if let Some(value) = option(&args, &[flag]) {
            metadata_command.arg(flag).arg(value);
        }
    }
    let requested_features = features(&args);
    if !requested_features.is_empty() {
        metadata_command.arg("--features").arg(requested_features);
    }
    for value in options(&args, &["--config"]) {
        metadata_command.arg("--config").arg(value);
    }
    let metadata: serde_json::Value = serde_json::from_str(&output(&mut metadata_command)?)?;
    if metadata["workspace_root"].as_str().map(Path::new) != Some(root.as_path()) {
        return Err(io::Error::other(
            "--manifest-path selects a different Cargo workspace; cannot establish its collector configuration from this Habeas project",
        ));
    }
    let target = option(&args, &["--target-dir"])
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(metadata["target_directory"].as_str().unwrap()));
    let target = if target.is_absolute() {
        target
    } else {
        invocation.join(target)
    };
    fs::create_dir_all(&target)?;
    let target = fs::canonicalize(target)?;
    let dir = target
        .join("habeas/coverage")
        .join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(dir.join("attempts"))?;
    fs::create_dir_all(dir.join("tools"))?;
    fs::create_dir_all(dir.join("setup"))?;
    // A per-run copy avoids races with concurrent builds of the CLI itself.
    let executable = std::env::current_exe()?;
    // Keep the compiler-wrapper path stable for identical CLI builds, so a
    // focused rerun can reuse Cargo's instrumented compilation cache.
    let executable_metadata = fs::metadata(&executable)?;
    let compiler_key = digest(
        format!(
            "{}:{}:{:?}",
            executable.display(),
            executable_metadata.len(),
            executable_metadata.modified()?
        )
        .as_bytes(),
    );
    let compiler = target
        .join("habeas/tools")
        .join(compiler_key)
        .join(format!("habeas-rustc{}", std::env::consts::EXE_SUFFIX));
    let collector = compiler
        .parent()
        .unwrap()
        .join(format!("habeas{}", std::env::consts::EXE_SUFFIX));
    if !collector.is_file() {
        fs::create_dir_all(collector.parent().unwrap())?;
        let temporary = collector.with_extension(uuid::Uuid::new_v4().to_string());
        fs::copy(&executable, &temporary)?;
        fs::rename(temporary, &collector)?;
    }
    if !compiler.is_file() {
        fs::create_dir_all(compiler.parent().unwrap())?;
        let temporary = compiler.with_extension(uuid::Uuid::new_v4().to_string());
        fs::copy(&executable, &temporary)?;
        fs::rename(temporary, &compiler)?;
    }
    let config = build_configuration(&invocation, &args, &rustc)?;
    let mut inputs = input_hashes(&workspace_inputs(&root, &target)?)?;
    inputs.extend(input_hashes(&super::cargo_configs(&invocation))?);
    for value in options(&args, &["--config"])
        .into_iter()
        .filter(|value| !value.contains('='))
    {
        inputs.extend(input_hashes(&[invocation.join(value)])?);
    }
    let mut run = Run {
        schema: 1,
        id: dir.file_name().unwrap().to_string_lossy().into_owned(),
        started: now(),
        command: std::iter::once("cargo nextest run".into())
            .chain(args.iter().cloned())
            .collect(),
        selection: args.clone(),
        configuration: digest(&serde_json::to_vec(&config)?),
        configuration_inputs: config,
        sources: BTreeMap::new(),
        inputs,
        environment_inputs: BTreeMap::new(),
        revision: output(
            Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&root),
        )
        .ok(),
        dirty: output(
            Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(&root),
        )
        .ok()
        .map(|status| !status.is_empty()),
        outcome: "interrupted".into(),
        diagnostics: Vec::new(),
    };
    save(&dir.join("run.json"), &run)?;
    let interrupts = Interrupts::new()?;
    let instrumented = target.join("habeas/instrumented").join(&run.configuration);
    let mut paths = vec![collector.parent().unwrap().to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    // Keep compilation in its own cache, including when native Cargo settings
    // select an external build directory. Coverage storage keeps the chosen target.
    let forwarded = {
        let split = args
            .iter()
            .position(|arg| arg == "--")
            .unwrap_or(args.len());
        let mut forwarded = Vec::new();
        let mut index = 0;
        while index < split {
            if args[index] == "--target-dir" {
                index += 2;
                continue;
            }
            if args[index].starts_with("--target-dir=") {
                index += 1;
                continue;
            }
            forwarded.push(args[index].clone());
            index += 1;
        }
        forwarded.push(format!("--target-dir={}", instrumented.display()));
        forwarded.push("--config".into());
        forwarded.push(format!(
            "build.build-dir={}",
            toml::Value::String(instrumented.to_string_lossy().into_owned())
        ));
        forwarded.extend_from_slice(&args[split..]);
        forwarded
    };
    let mut command = Command::new("cargo");
    command
        .args(["nextest", "run"])
        .args(forwarded)
        .current_dir(&invocation)
        .env("HABEAS_COVERAGE_RUN", &dir)
        .env("LLVM_PROFILE_FILE", dir.join("setup/%p-%m.profraw"))
        .env("CARGO_TARGET_DIR", &instrumented)
        .env("CARGO_BUILD_BUILD_DIR", &instrumented)
        .env("RUSTC_WORKSPACE_WRAPPER", &compiler)
        .env(
            "HABEAS_COVERAGE_FEATURE_LEDGER",
            instrumented.join("features"),
        )
        .env(
            "PATH",
            std::env::join_paths(paths).map_err(io::Error::other)?,
        );
    if let Some(previous) = std::env::var_os("RUSTC_WORKSPACE_WRAPPER") {
        command.env("HABEAS_PREVIOUS_WORKSPACE_WRAPPER", previous);
    }
    // The instrumented cache is for this compilation. Tests that invoke Cargo
    // must still observe the caller's build environment.
    let test_environment = [
        "CARGO_TARGET_DIR",
        "CARGO_BUILD_BUILD_DIR",
        "RUSTC_WORKSPACE_WRAPPER",
    ]
    .map(|name| (name, std::env::var(name).ok()))
    .into_iter()
    .collect::<BTreeMap<_, _>>();
    command.env(
        "HABEAS_COVERAGE_TEST_BUILD_ENV",
        serde_json::to_string(&test_environment)?,
    );
    // Capture dep-info and source text after compilation but before any test
    // process can mutate source or generated inputs.
    let mut build = Command::new(command.get_program());
    let mut build_args = preparation_args(
        &command
            .get_args()
            .map(|arg| arg.to_os_string())
            .collect::<Vec<_>>(),
    );
    let separator = build_args
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(build_args.len());
    if !has(&args, "--no-run") {
        build_args.insert(separator, "--no-run".into());
    }
    build.args(build_args).current_dir(&invocation);
    for (key, value) in command.get_envs() {
        match value {
            Some(value) => {
                build.env(key, value);
            }
            None => {
                build.env_remove(key);
            }
        }
    }
    let build_status = match interrupts.execute(&mut build) {
        Ok(status) => status,
        Err(error) => {
            run.outcome = "failed".into();
            run.diagnostics
                .push(format!("Cannot start instrumented compilation: {error}"));
            save(&dir.join("run.json"), &run)?;
            return Err(error);
        }
    };
    if !build_status.success() {
        run.outcome = if interrupts.happened() {
            "interrupted"
        } else {
            "failed"
        }
        .into();
        run.diagnostics.push("Instrumented compilation or nextest configuration validation failed before test startup.".into());
        save(&dir.join("run.json"), &run)?;
        return Ok(code(build_status));
    }
    let preparation: io::Result<()> = (|| {
        let progress = Progress::preparation();
        capture_inputs(&root, &target, &instrumented, &mut run)?;
        let mut features = BTreeMap::new();
        for path in walk(&instrumented.join("features"), &[])? {
            let (package, enabled): (String, std::collections::BTreeSet<String>) = load(&path)?;
            features.insert(package, enabled);
        }
        run.configuration_inputs
            .insert("enabled-features".into(), serde_json::to_string(&features)?);
        run.configuration = digest(&serde_json::to_vec(&run.configuration_inputs)?);
        progress.stage("Preparing coverage: capturing maps");
        snapshot_maps(&instrumented, &dir, &cov)
    })();
    if let Err(error) = preparation {
        run.outcome = if interrupts.happened() {
            "interrupted"
        } else {
            "failed"
        }
        .into();
        let message = format!("Coverage preparation failed before test startup: {error}");
        run.diagnostics.push(message.clone());
        save(&dir.join("run.json"), &run)?;
        return Err(io::Error::other(message));
    }
    save(&dir.join("run.json"), &run)?;
    if interrupts.happened() {
        return Ok(130);
    }
    let execution = interrupts.execute(&mut command);
    let status = match execution {
        Ok(status) => {
            run.outcome = if interrupts.happened() || status.code().is_none() {
                "interrupted"
            } else if status.success() {
                "success"
            } else {
                "failed"
            }
            .into();
            if interrupts.happened() {
                130
            } else {
                code(status)
            }
        }
        Err(error) => {
            run.diagnostics.push(error.to_string());
            run.outcome = "failed".into();
            1
        }
    };
    let collection = finish(&root, &dir, &cov, &profdata, &mut run);
    if let Err(error) = collection {
        run.diagnostics.push(error.to_string());
        if run.outcome == "success" {
            run.outcome = "failed".into();
        }
    }
    if fs::read_dir(dir.join("attempts"))?.next().is_none() {
        run.diagnostics.push(
            if has(&args, "--no-run") {
                "Compilation-only run (--no-run): no test attempt started."
            } else if status == 0 || status == 4 {
                // Nextest's NO_TESTS_RUN exit code.
                "Empty test selection: no test attempt started."
            } else {
                "Nextest stopped before test startup; no test attempt was recorded."
            }
            .into(),
        );
    }
    save(&dir.join("run.json"), &run)?;
    eprintln!("Coverage retained in {}", dir.display());
    for diagnostic in &run.diagnostics {
        eprintln!("coverage: {diagnostic}");
    }
    if status != 0 {
        Ok(status)
    } else if run.outcome == "success" {
        Ok(0)
    } else {
        Ok(1)
    }
}

// {impl}`requirements.test-coverage.processes`
// {impl}`requirements.test-coverage.storage.outcomes`
// {impl}`requirements.test-coverage.failures`
pub fn collect_process(args: &[OsString]) -> io::Result<i32> {
    let executable = args
        .first()
        .ok_or_else(|| io::Error::other("coverage-process requires a test command"))?;
    let Some(run) = std::env::var_os("HABEAS_COVERAGE_RUN") else {
        return Ok(code(Command::new(executable).args(&args[1..]).status()?));
    };
    let dir = PathBuf::from(run)
        .join("attempts")
        .join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&dir)?;
    let mut attempt = Attempt {
        id: dir.file_name().unwrap().to_string_lossy().into_owned(),
        binary: std::env::var("NEXTEST_BINARY_ID").map_err(io::Error::other)?,
        test: std::env::var("NEXTEST_TEST_NAME").map_err(io::Error::other)?,
        attempt: std::env::var("NEXTEST_ATTEMPT").map_err(io::Error::other)?,
        started: now(),
        pid: 0,
        command: args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect(),
        outcome: "interrupted".into(),
        processes: Vec::new(),
        diagnostics: Vec::new(),
    };
    save(&dir.join("attempt.json"), &attempt)?;
    let mut command = Command::new(executable);
    command
        .args(&args[1..])
        .env("LLVM_PROFILE_FILE", dir.join("%p-%m.profraw"));
    if let Ok(environment) = std::env::var("HABEAS_COVERAGE_TEST_BUILD_ENV") {
        let environment: BTreeMap<String, Option<String>> = serde_json::from_str(&environment)?;
        for (name, value) in environment {
            match value {
                Some(value) => {
                    command.env(name, value);
                }
                None => {
                    command.env_remove(name);
                }
            }
        }
    }
    for name in [
        "HABEAS_COVERAGE_RUN",
        "HABEAS_COVERAGE_FEATURE_LEDGER",
        "HABEAS_PREVIOUS_WORKSPACE_WRAPPER",
        "HABEAS_COVERAGE_TEST_BUILD_ENV",
    ] {
        command.env_remove(name);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            attempt.outcome = "failed".into();
            attempt
                .diagnostics
                .push(format!("Cannot start test process: {error}"));
            save(&dir.join("attempt.json"), &attempt)?;
            return Err(error);
        }
    };
    attempt.pid = child.id();
    save(&dir.join("attempt.json"), &attempt)?;
    let status = match child.wait() {
        Ok(status) => status,
        Err(error) => {
            attempt.outcome = "failed".into();
            attempt
                .diagnostics
                .push(format!("Cannot wait for test process: {error}"));
            save(&dir.join("attempt.json"), &attempt)?;
            return Err(error);
        }
    };
    attempt.outcome = if status.success() {
        "success"
    } else if status.code().is_none() {
        "interrupted"
    } else {
        "failed"
    }
    .into();
    save(&dir.join("attempt.json"), &attempt)?;
    Ok(code(status))
}

type EnvironmentInputs = BTreeMap<String, Option<String>>;

fn dependency_inputs(
    instrumented: &Path,
    root: &Path,
    target: &Path,
) -> io::Result<(Vec<PathBuf>, EnvironmentInputs)> {
    let mut inputs = Vec::new();
    let mut environment = BTreeMap::new();
    let manifests = workspace_inputs(root, target)?
        .into_iter()
        .filter(|path| path.file_name().is_some_and(|name| name == "Cargo.toml"))
        .collect::<Vec<_>>();
    let mut package_roots = manifests
        .iter()
        .filter_map(|manifest| manifest.parent())
        .map(Path::to_path_buf)
        .collect::<Vec<_>>();
    let files = walk(instrumented, &[])?;
    for path in &files {
        if path.extension().is_some_and(|extension| extension == "d") {
            let source = fs::read_to_string(path)?;
            // Rust's makefile dependency list, including paths escaped with backslashes.
            for line in source.lines().filter(|line| !line.starts_with('#')) {
                let Some((_, deps)) = line.split_once(": ") else {
                    continue;
                };
                let mut current = String::new();
                let mut escape = false;
                for character in deps.chars().chain(std::iter::once(' ')) {
                    if escape {
                        current.push(character);
                        escape = false;
                    } else if character == '\\' {
                        escape = true;
                    } else if character.is_whitespace() {
                        if !current.is_empty() {
                            let input = PathBuf::from(std::mem::take(&mut current));
                            if input.is_absolute() && input.is_file() {
                                inputs.push(input);
                            } else if !input.is_absolute() {
                                for package in &package_roots {
                                    let candidate = package.join(&input);
                                    if candidate.is_file() {
                                        inputs.push(candidate);
                                    }
                                }
                            }
                        }
                    } else {
                        current.push(character);
                    }
                }
            }
            for line in source
                .lines()
                .filter_map(|line| line.strip_prefix("# env-dep:"))
            {
                let name = line.split('=').next().unwrap_or(line);
                if !super::cargo_derived_environment(name) {
                    environment.insert(name.into(), std::env::var(name).ok());
                }
            }
        }
    }
    // Locate dependency package roots too, for relative build-script inputs.
    for input in &inputs {
        if let Some(package) = input
            .ancestors()
            .skip(1)
            .find(|directory| directory.join("Cargo.toml").is_file())
        {
            package_roots.push(package.to_path_buf());
        }
    }
    package_roots.sort();
    package_roots.dedup();
    let mut packages = BTreeMap::new();
    for package in package_roots {
        if let Some(name) = fs::read_to_string(package.join("Cargo.toml"))
            .ok()
            .and_then(|source| source.parse::<toml::Table>().ok())
            .and_then(|config| {
                config
                    .get("package")
                    .and_then(|package| package.get("name"))
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned)
            })
        {
            packages.insert(name, package);
        }
    }
    for path in files {
        if path.file_name().is_some_and(|name| name == "output") {
            let package_name = path
                .parent()
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str())
                .and_then(|name| name.rsplit_once('-'))
                .map(|(name, _)| name);
            let package = package_name
                .and_then(|name| packages.get(name))
                .map(PathBuf::as_path)
                .unwrap_or(root);
            for line in fs::read_to_string(&path)?.lines() {
                if let Some(name) = line
                    .strip_prefix("cargo:rerun-if-env-changed=")
                    .or_else(|| line.strip_prefix("cargo::rerun-if-env-changed="))
                    && !super::cargo_derived_environment(name)
                {
                    environment.insert(name.into(), std::env::var(name).ok());
                }
                if let Some(input) = line
                    .strip_prefix("cargo:rerun-if-changed=")
                    .or_else(|| line.strip_prefix("cargo::rerun-if-changed="))
                {
                    let input = PathBuf::from(input);
                    inputs.push(if input.is_absolute() {
                        input
                    } else {
                        package.join(input)
                    });
                }
            }
        }
    }
    inputs.sort();
    inputs.dedup();
    Ok((inputs, environment))
}

// {cite}`habeas:util`
fn instrumented_objects(directory: &Path, readobj: &Path) -> io::Result<Vec<(PathBuf, String)>> {
    let mut objects = Vec::new();
    for path in walk(directory, &[])? {
        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|extension| !matches!(extension, "exe" | "dll" | "so" | "dylib"))
        {
            continue;
        }
        let mut magic = [0; 4];
        if fs::File::open(&path)?.read_exact(&mut magic).is_err() {
            continue;
        }
        if magic != *b"\x7fELF"
            && !magic.starts_with(b"MZ")
            && !matches!(
                magic,
                [0xfe, 0xed, 0xfa, _] | [_, 0xfa, 0xed, 0xfe] | [0xca, 0xfe, 0xba, 0xbe]
            )
        {
            continue;
        }
        // LLVM reads the section table directly, avoiding a scan of gigabytes
        // of debug information in a workspace's test executables.
        let sections = output(
            Command::new(readobj)
                .args(["--sections", "--notes"])
                .arg(&path),
        )?;
        if sections.contains("__llvm_covmap") || sections.contains(".lcovm") {
            objects.push((path, sections));
        }
    }
    Ok(objects)
}

// {cite}`habeas:util`
#[derive(serde::Serialize, serde::Deserialize)]
struct Map {
    path: String,
    binary_ids: Vec<String>,
}

// Keep compact matching coverage maps as well as normalized reports, so an
// interrupted run's raw profiles are still usable after a later rebuild.
// {impl}`requirements.test-coverage.processes.reports`
fn snapshot_maps(instrumented: &Path, dir: &Path, cov: &Path) -> io::Result<()> {
    let objcopy = cov.with_file_name(format!("llvm-objcopy{}", std::env::consts::EXE_SUFFIX));
    let readobj = cov.with_file_name(format!("llvm-readobj{}", std::env::consts::EXE_SUFFIX));
    fs::create_dir_all(dir.join("maps"))?;
    let mut maps = Vec::new();
    for (index, (object, notes)) in instrumented_objects(instrumented, &readobj)?
        .into_iter()
        .enumerate()
    {
        let path = format!("maps/{index}.o");
        let captured = dir.join(&path);
        let mut magic = [0; 4];
        fs::File::open(&object)?.read_exact(&mut magic)?;
        let mut copy = Command::new(&objcopy);
        if magic == *b"\x7fELF" {
            copy.args([
                "--only-section=__llvm_covmap",
                "--only-section=__llvm_covfun",
                "--only-section=__llvm_prf_names",
                "--only-section=.note.gnu.build-id",
            ]);
        } else {
            copy.arg("--strip-debug");
        }
        output(copy.arg(&object).arg(&captured))?;
        let binary_ids = notes
            .lines()
            .filter_map(|line| line.trim().strip_prefix("Build ID: "))
            .map(str::to_owned)
            .collect();
        maps.push(Map { path, binary_ids });
    }
    save(&dir.join("maps.json"), &maps)
}

// {impl}`requirements.test-coverage.storage.provenance`
// {impl}`requirements.test-coverage.processes.reports`
fn capture_inputs(
    root: &Path,
    target: &Path,
    instrumented: &Path,
    run: &mut Run,
) -> io::Result<()> {
    let (mut dependencies, environment) = dependency_inputs(instrumented, root, target)?;
    // The first compilation can create Cargo.lock. Keep pre-build identities
    // for existing inputs, and capture workspace inputs newly created by Cargo.
    dependencies.extend(workspace_inputs(root, target)?);
    let mut failures = Vec::new();
    let discovered = crate::evidence::discovered_files(root, &mut failures);
    if !failures.is_empty() {
        return Err(io::Error::other(failures.join("\n")));
    }
    let sources = crate::rust_coverage::rust_files(root, &discovered);
    for path in &dependencies {
        let hash = super::input_digest(path, Some(target))?;
        run.inputs
            .entry(path.to_string_lossy().into_owned())
            .or_insert(hash);
    }
    run.environment_inputs = environment;
    for path in &sources {
        if path.starts_with(root) && !path.starts_with(root.join("vendor")) {
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            run.sources.insert(relative, fs::read_to_string(path)?);
        }
    }
    Ok(())
}

// {impl}`requirements.test-coverage.processes.reports`
// {impl}`requirements.test-coverage.storage.provenance`
// {impl}`requirements.test-coverage.processes.progress`
fn finish(root: &Path, dir: &Path, cov: &Path, profdata: &Path, run: &mut Run) -> io::Result<()> {
    let maps: Vec<Map> = load(&dir.join("maps.json"))?;
    let mut attempts = fs::read_dir(dir.join("attempts"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<io::Result<Vec<_>>>()?;
    attempts.sort();
    let progress = Progress::conversion(attempts.len());
    // LLVM exports can use substantial memory. Limit concurrent conversions
    // independently of nextest's test concurrency.
    let workers = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(4);
    let results = parallel_collect(
        &attempts,
        workers,
        |attempt| finish_attempt(attempt, dir, &maps, cov, profdata, root, &run.sources),
        |_| progress.completed(),
    );
    let mut failed = false;
    for (attempt_dir, result) in attempts.iter().zip(results) {
        match result {
            Ok(complete) => failed |= !complete,
            Err(error) => {
                run.diagnostics
                    .push(format!("{}: {error}", attempt_dir.display()));
                failed = true;
            }
        }
    }
    if failed {
        return Err(io::Error::other(
            "Some process coverage is missing or unreadable; partial profiles and reports have been retained.",
        ));
    }
    Ok(())
}

// {cite}`habeas:util`
fn parallel_collect<T: Sync, R: Send>(
    items: &[T],
    workers: usize,
    work: impl Fn(&T) -> R + Sync,
    mut completed: impl FnMut(usize),
) -> Vec<R> {
    let next = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let (sender, receiver) = std::sync::mpsc::channel();
        for _ in 0..workers.max(1).min(items.len()) {
            let sender = sender.clone();
            let next = &next;
            let work = &work;
            scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(index) else {
                        break;
                    };
                    if sender.send((index, work(item))).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
        let mut results: Vec<Option<R>> =
            std::iter::repeat_with(|| None).take(items.len()).collect();
        for (index, result) in receiver {
            results[index] = Some(result);
            completed(index);
        }
        results
            .into_iter()
            .map(|result| result.expect("every conversion returned a result"))
            .collect()
    })
}

// {cite}`habeas:util`
fn finish_attempt(
    attempt_dir: &Path,
    dir: &Path,
    maps: &[Map],
    cov: &Path,
    profdata: &Path,
    root: &Path,
    sources: &BTreeMap<String, String>,
) -> io::Result<bool> {
    let mut attempt: Attempt = load(&attempt_dir.join("attempt.json"))?;
    let mut failed = false;
    let mut profiles: BTreeMap<u32, Vec<PathBuf>> = BTreeMap::new();
    for path in walk(attempt_dir, &[])? {
        if path
            .extension()
            .is_some_and(|extension| extension == "profraw")
            && let Some(pid) = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.split('-').next())
                .and_then(|pid| pid.parse().ok())
        {
            profiles.entry(pid).or_default().push(path);
        }
    }
    if profiles.is_empty() {
        attempt
            .diagnostics
            .push("No LLVM profile was written; this attempt's coverage is unavailable.".into());
        failed = true;
    }
    for (pid, paths) in profiles {
        let merged = attempt_dir.join(format!("{pid}.profdata"));
        let result = output(
            Command::new(profdata)
                .args(["merge", "-sparse"])
                .args(&paths)
                .arg("-o")
                .arg(&merged),
        )
        .and_then(|_| {
            let info = output(
                Command::new(profdata)
                    .args(["show", "--binary-ids"])
                    .arg(&merged),
            )?;
            let ids = info
                .split("Binary IDs:")
                .nth(1)
                .unwrap_or("")
                .split_whitespace()
                .collect::<Vec<_>>();
            let objects = maps
                .iter()
                .filter(|map| {
                    ids.is_empty()
                        || map.binary_ids.is_empty()
                        || map.binary_ids.iter().any(|id| ids.contains(&id.as_str()))
                })
                .map(|map| dir.join(&map.path))
                .collect::<Vec<_>>();
            export(cov, &merged, &objects, root, sources)
        });
        let (lines, functions, diagnostic) = match result {
            Ok((lines, functions)) => (lines, Some(functions), None),
            Err(error) => {
                failed = true;
                (BTreeMap::new(), None, Some(error.to_string()))
            }
        };
        attempt.processes.push(Process {
            pid,
            profiles: paths
                .iter()
                .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
                .collect(),
            lines,
            functions,
            diagnostic,
        });
    }
    if attempt
        .processes
        .iter()
        .all(|process| process.pid != attempt.pid)
    {
        attempt.diagnostics.push(
            "The test process's own profile is missing; child profiles do not replace it.".into(),
        );
        failed = true;
    }
    save(&attempt_dir.join("attempt.json"), &attempt)?;
    Ok(!failed)
}

// {cite}`habeas:util`
type ProcessCoverage = (BTreeMap<String, BTreeMap<u32, u64>>, Vec<FunctionExecution>);

// {impl}`requirements.test-coverage.check.functions`
fn export(
    cov: &Path,
    profile: &Path,
    objects: &[PathBuf],
    root: &Path,
    sources: &BTreeMap<String, String>,
) -> io::Result<ProcessCoverage> {
    let first = objects
        .first()
        .ok_or_else(|| io::Error::other("No instrumented executable coverage maps were found"))?;
    let mut command = Command::new(cov);
    command
        .arg("export")
        .arg(first)
        .arg("-format=lcov")
        .arg(format!("-instr-profile={}", profile.display()));
    for object in &objects[1..] {
        command.arg("-object").arg(object);
    }
    let exported = output(&mut command)?;
    let mut lines: BTreeMap<String, BTreeMap<u32, u64>> = BTreeMap::new();
    let mut file = None;
    for line in exported.lines() {
        if let Some(path) = line.strip_prefix("SF:") {
            file = Path::new(path)
                .strip_prefix(root)
                .ok()
                .filter(|relative| !relative.starts_with("vendor"))
                .filter(|relative| {
                    relative
                        .extension()
                        .is_some_and(|extension| extension == "rs")
                })
                .map(|relative| relative.to_string_lossy().into_owned())
                .filter(|relative| sources.contains_key(relative));
        } else if let (Some(file), Some(counts)) = (&file, line.strip_prefix("DA:")) {
            let mut parts = counts.split(',');
            let line = parts
                .next()
                .and_then(|part| part.parse::<u32>().ok())
                .ok_or_else(|| io::Error::other("invalid LLVM coverage line number"))?;
            let count = parts
                .next()
                .and_then(|part| part.parse::<u64>().ok())
                .ok_or_else(|| io::Error::other("invalid LLVM execution count"))?;
            lines
                .entry(file.clone())
                .or_default()
                .entry(line)
                .and_modify(|current| *current = (*current).max(count))
                .or_insert(count);
        }
    }
    let mut command = Command::new(cov);
    command
        .arg("export")
        .arg(first)
        .arg(format!("-instr-profile={}", profile.display()));
    for object in &objects[1..] {
        command.arg("-object").arg(object);
    }
    let functions = function_counts(&output(&mut command)?, root, sources)?;
    Ok((lines, functions))
}

// {impl}`requirements.test-coverage.check.functions`
fn function_counts(
    json: &str,
    root: &Path,
    sources: &BTreeMap<String, String>,
) -> io::Result<Vec<FunctionExecution>> {
    // Deserialize only counters and entry locations, rather than retaining the
    // much larger JSON file-segment and expansion maps in memory or on disk.
    // {cite}`habeas:util`
    #[derive(serde::Deserialize)]
    struct Export {
        data: Vec<Data>,
    }
    // {cite}`habeas:util`
    #[derive(serde::Deserialize)]
    struct Data {
        functions: Vec<Function>,
    }
    // {cite}`habeas:util`
    #[derive(serde::Deserialize)]
    struct Function {
        count: u64,
        filenames: Vec<String>,
        regions: Vec<Vec<u64>>,
    }
    let export: Export = serde_json::from_str(json)?;
    let mut counts = Vec::new();
    for function in export.data.into_iter().flat_map(|data| data.functions) {
        // Region kind 0 is a code region. Expansion/gap regions are not an
        // explicit function's entry. Matching uses BOTH line and byte column.
        let Some(entry) = function
            .regions
            .iter()
            .find(|region| region.len() == 8 && region[7] == 0)
        else {
            continue;
        };
        let filename = function
            .filenames
            .get(entry[5] as usize)
            .ok_or_else(|| io::Error::other("invalid LLVM function file index"))?;
        let Some(file) = Path::new(filename)
            .strip_prefix(root)
            .ok()
            .filter(|path| !path.starts_with("vendor"))
            .map(|path| path.to_string_lossy().into_owned())
            .filter(|path| sources.contains_key(path))
        else {
            continue;
        };
        let line = u32::try_from(entry[0])
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| io::Error::other("invalid LLVM function entry line"))?;
        let column = u32::try_from(entry[1])
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| io::Error::other("invalid LLVM function entry column"))?;
        counts.push(FunctionExecution {
            file,
            line,
            column,
            count: function.count,
        });
    }
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;

    // {test}`requirements.test-coverage.processes`
    // {test}`requirements.test-coverage.processes.reports`
    // {test}`requirements.test-coverage.failures`
    // {test}`requirements.test-coverage.processes.progress`
    #[test]
    fn parallel_conversion_is_bounded_and_retains_every_result_in_order() {
        use std::sync::{Condvar, Mutex};
        use std::time::Duration;

        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let visits: Vec<_> = (0..8).map(|_| AtomicUsize::new(0)).collect();
        let finished: Vec<_> = (0..8).map(|_| AtomicBool::new(false)).collect();
        let rendezvous = (Mutex::new(false), Condvar::new());
        let mut completed = 0;
        let results = parallel_collect(
            &(0..8).collect::<Vec<_>>(),
            2,
            |&index| {
                visits[index].fetch_add(1, Ordering::SeqCst);
                let concurrent = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(concurrent, Ordering::SeqCst);
                if index < 2 {
                    let mut met = rendezvous.0.lock().unwrap();
                    if concurrent == 2 {
                        *met = true;
                        rendezvous.1.notify_all();
                    }
                    let (met, timeout) = rendezvous
                        .1
                        .wait_timeout_while(met, Duration::from_secs(5), |met| !*met)
                        .unwrap();
                    assert!(
                        *met && !timeout.timed_out(),
                        "conversion was not concurrent"
                    );
                }
                active.fetch_sub(1, Ordering::SeqCst);
                finished[index].store(true, Ordering::SeqCst);
                if index == 3 {
                    Err("unreadable profile")
                } else {
                    Ok(index)
                }
            },
            |index| {
                assert!(
                    finished[index].load(Ordering::SeqCst),
                    "progress advanced before conversion finished"
                );
                completed += 1;
            },
        );
        assert_eq!(peak.load(Ordering::SeqCst), 2);
        assert!(visits.iter().all(|count| count.load(Ordering::SeqCst) == 1));
        assert_eq!(completed, 8, "failed work also completes a conversion");
        assert_eq!(
            results,
            (0..8)
                .map(|index| if index == 3 {
                    Err("unreadable profile")
                } else {
                    Ok(index)
                })
                .collect::<Vec<_>>()
        );
    }

    // {test}`requirements.test-coverage.storage.outcomes`
    #[test]
    fn empty_conversion_starts_no_work_and_reports_no_completions() {
        let results: Vec<()> =
            parallel_collect::<(), _>(&[], 4, |_| panic!("no work"), |_| panic!("no completions"));
        assert!(results.is_empty());
        assert_eq!(parallel_collect(&[7], 0, |value| *value, |_| {}), vec![7]);
    }

    // {test}`requirements.test-coverage.storage.configuration`
    #[test]
    fn compiler_inputs_determine_compatibility_and_scheduling_does_not() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let rustc = "rustc 1.97.0\nhost: test-host\nLLVM version: 22.1.6\n";
        let baseline =
            build_configuration(root, &["--target".into(), "test-host".into()], rustc).unwrap();
        let key = |inputs: &BTreeMap<String, String>| digest(&serde_json::to_vec(inputs).unwrap());
        assert_eq!(baseline["rustc"], rustc);
        assert_eq!(baseline["target"], "test-host");
        assert_eq!(baseline["cargo-profile"], "test");
        assert!(baseline["instrumentation"].contains("-C instrument-coverage"));
        for (args, field, value) in [
            (
                vec!["--target", "wasm32-unknown-unknown"],
                "target",
                "wasm32-unknown-unknown",
            ),
            (
                vec!["--target", "test-host", "--release"],
                "cargo-profile",
                "release",
            ),
            (
                vec!["--target", "test-host", "-r"],
                "cargo-profile",
                "release",
            ),
            (
                vec!["--target", "test-host", "--cargo-profile", "custom"],
                "cargo-profile",
                "custom",
            ),
            (
                vec!["--target", "test-host", "--all-features"],
                "all-features",
                "true",
            ),
            (
                vec!["--target", "test-host", "--no-default-features"],
                "no-default-features",
                "true",
            ),
        ] {
            let args = args.into_iter().map(String::from).collect::<Vec<_>>();
            let inputs = build_configuration(root, &args, rustc).unwrap();
            assert_eq!(inputs[field], value);
            assert_ne!(key(&inputs), key(&baseline), "{args:?}");
        }
        let different_compiler = build_configuration(
            root,
            &["--target".into(), "test-host".into()],
            &rustc.replace("1.97.0", "1.98.0"),
        )
        .unwrap();
        assert_ne!(key(&different_compiler), key(&baseline));
        let scheduled = build_configuration(
            root,
            &[
                "--target".into(),
                "test-host".into(),
                "--profile".into(),
                "focused".into(),
            ],
            rustc,
        )
        .unwrap();
        assert_eq!(key(&scheduled), key(&baseline));
        fs::create_dir(root.join(".cargo")).unwrap();
        let config = root.join(".cargo/config.toml");
        fs::write(
            &config,
            "[build]\nrustflags = ['-C', 'opt-level=1']\n[env]\nBUILD_INPUT = 'one'\n",
        )
        .unwrap();
        let configured =
            build_configuration(root, &["--target".into(), "test-host".into()], rustc).unwrap();
        assert!(configured[config.to_str().unwrap()].contains("opt-level=1"));
        assert_ne!(key(&configured), key(&baseline));
        fs::write(
            &config,
            "[build]\nrustflags = ['-C', 'opt-level=2']\n[env]\nBUILD_INPUT = 'two'\n",
        )
        .unwrap();
        let changed =
            build_configuration(root, &["--target".into(), "test-host".into()], rustc).unwrap();
        assert_ne!(key(&changed), key(&configured));

        // A child has its own environment, without changing any other test's process.
        if std::env::var_os("HABEAS_CONFIGURATION_TEST_CHILD").is_some() {
            assert_eq!(baseline["RUSTFLAGS"], "-Copt-level=3");
            assert_eq!(baseline["CARGO_PROFILE_TEST_DEBUG"], "false");
            assert_eq!(baseline["CARGO_TARGET_TEST_HOST_LINKER"], "fixture-linker");
            assert_ne!(
                key(&baseline),
                std::env::var("HABEAS_CONFIGURATION_BASELINE").unwrap()
            );
            return;
        }
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "execution_coverage::runner::tests::compiler_inputs_determine_compatibility_and_scheduling_does_not"])
            .env("HABEAS_CONFIGURATION_TEST_CHILD", "1")
            .env("HABEAS_CONFIGURATION_BASELINE", key(&baseline))
            .env("RUSTFLAGS", "-Copt-level=3")
            .env("CARGO_PROFILE_TEST_DEBUG", "false")
            .env("CARGO_TARGET_TEST_HOST_LINKER", "fixture-linker")
            .output().unwrap();
        assert!(
            child.status.success(),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        assert!(String::from_utf8_lossy(&child.stdout).contains("1 passed"));
    }
}

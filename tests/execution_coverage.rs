//! Real nextest/LLVM process coverage, using a dependency-free Cargo fixture.
mod common;

use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

// {cite}`habeas:util`
fn fixture() -> TempDir {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for directory in ["src", "tests", "md"] {
        fs::create_dir_all(root.join(directory)).unwrap();
    }
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = 'coverage-fixture'\nversion = '0.1.0'\nedition = '2024'\n[features]\nalpha = []\nbeta = []\n",
    )
    .unwrap();
    fs::write(root.join("book.toml"), "[book]\nsrc = 'md'\n").unwrap();
    fs::write(root.join("md/SUMMARY.md"), "# Summary\n").unwrap();
    fs::write(
        root.join("src/lib.rs"),
        concat!(
            "pub fn value(flag: bool) -> u32 {\n",
            "    if flag {\n        10\n    } else {\n        20\n    }\n}\n",
            "#[cfg(test)] #[test] fn same_name() { assert_eq!(value(true), 10); }\n",
            "#[cfg(test)] #[test] fn second() { assert_eq!(value(false), 20); }\n",
        ),
    )
    .unwrap();
    fs::write(root.join("tests/processes.rs"), concat!(
        "use std::process::Command;\n",
        "#[test] fn same_name() { assert_eq!(coverage_fixture::value(false), 20); }\n",
        "#[test] fn child_process() {\n",
        "    if std::env::var_os(\"COVERAGE_CHILD\").is_some() {\n",
        "        assert_eq!(coverage_fixture::value(true), 10);\n",
        "    } else {\n",
        "        assert!(Command::new(std::env::current_exe().unwrap()).args([\"--exact\", \"child_process\"]).env(\"COVERAGE_CHILD\", \"1\").status().unwrap().success());\n",
        "    }\n}\n",
        "#[test] fn retries() { assert_eq!(std::env::var(\"NEXTEST_ATTEMPT\").unwrap(), \"2\"); }\n",
    )).unwrap();
    common::coverage_setup(root);
    temp
}

// {cite}`habeas:util`
fn coverage(root: &Path, args: &[&str]) -> Output {
    coverage_with_environment(root, args, &[])
}

// {cite}`habeas:util`
fn coverage_with_environment(root: &Path, args: &[&str], environment: &[(&str, &str)]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_habeas"))
        .arg("coverage")
        .arg("--")
        .args(args)
        .current_dir(root)
        .env("CARGO_NET_OFFLINE", "true")
        .envs(environment.iter().copied())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("HABEAS_PREVIOUS_WORKSPACE_WRAPPER")
        .env_remove("HABEAS_COVERAGE_RUN")
        .env_remove("HABEAS_COVERAGE_FEATURE_LEDGER")
        .output()
        .unwrap()
}

// {cite}`habeas:util`
fn runs_in(storage: &Path) -> Vec<(std::path::PathBuf, Value)> {
    let connection = rusqlite::Connection::open_with_flags(
        storage.join("current.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let mut query = connection
        .prepare("SELECT id,data FROM invocations ORDER BY json_extract(data,'$.started')")
        .unwrap();
    query
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .map(|row| {
            let (id, data) = row.unwrap();
            (storage.join(id), serde_json::from_str(&data).unwrap())
        })
        .collect()
}

// {cite}`habeas:util`
fn runs(root: &Path) -> Vec<(std::path::PathBuf, Value)> {
    runs_in(&root.join("target/habeas/coverage"))
}

// {cite}`habeas:util`
fn attempts(run: &Path) -> Vec<Value> {
    let connection = rusqlite::Connection::open_with_flags(
        run.parent().unwrap().join("current.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let mut query = connection
        .prepare("SELECT metadata,measurements FROM tests WHERE invocation=?1 ORDER BY binary,test")
        .unwrap();
    query
        .query_map([run.file_name().unwrap().to_str().unwrap()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .unwrap()
        .map(|row| {
            let (data, measurements) = row.unwrap();
            let mut attempt: Value = serde_json::from_str(&data).unwrap();
            attempt["processes"] = measurements
                .map(|data| serde_json::from_str(&data).unwrap())
                .unwrap_or(serde_json::json!([]));
            attempt
        })
        .collect()
}

// {cite}`habeas:util`
fn assert_profiles_present(run: &Path, attempt: &Value) {
    let profiles: Vec<_> = attempt["processes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|process| process["profiles"].as_array().unwrap())
        .collect();
    assert!(
        !profiles.is_empty(),
        "expected retained raw profiles for {}",
        attempt["test"]
    );
    for profile in profiles {
        let path = run
            .join("attempts")
            .join(attempt["id"].as_str().unwrap())
            .join(profile.as_str().unwrap());
        assert_eq!(path.extension().unwrap(), "profraw");
        assert!(fs::metadata(&path).unwrap().len() > 0, "{}", path.display());
    }
}

// {cite}`habeas:util`
fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

// {test}`requirements.test-coverage.processes.progress`
// {test}`requirements.test-coverage.processes.progress.preparation`
// {test}`requirements.test-coverage.processes.progress.terminal`
#[cfg(target_os = "linux")]
#[test]
fn coverage_progress_on_a_pty_preserves_reports_and_plain_output_policy() {
    use std::io::Read;
    use std::os::fd::FromRawFd;

    #[link(name = "util")]
    unsafe extern "C" {
        fn openpty(
            master: *mut std::ffi::c_int,
            slave: *mut std::ffi::c_int,
            name: *mut std::ffi::c_char,
            termios: *const std::ffi::c_void,
            winsize: *const std::ffi::c_void,
        ) -> std::ffi::c_int;
    }

    let temp = fixture();
    let root = temp.path();
    let source = fs::read_to_string(root.join("src/lib.rs")).unwrap();
    fs::write(root.join("src/lib.rs"), format!("{source}\n#[test] fn deliberate_failure() {{ panic!(\"fixture failure\"); }}\nunsafe extern \"C\" {{ fn _exit(status: i32) -> !; }}\n#[test] fn missing_profile() {{ unsafe {{ _exit(0) }} }}\n")).unwrap();
    let run_on_pty = |term: &str, filter: &str| {
        let mut master = -1;
        let mut slave = -1;
        // The returned descriptors are owned by this test and transferred
        // exactly once into File. Default terminal settings are sufficient.
        assert_eq!(
            unsafe {
                openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null(),
                )
            },
            0
        );
        let mut master = unsafe { fs::File::from_raw_fd(master) };
        let slave = unsafe { fs::File::from_raw_fd(slave) };
        let reader = std::thread::spawn(move || {
            let mut output = Vec::new();
            if let Err(error) = master.read_to_end(&mut output) {
                // Linux returns EIO when the last slave descriptor closes.
                assert_eq!(error.raw_os_error(), Some(5));
            }
            output
        });
        let child = Command::new(env!("CARGO_BIN_EXE_habeas"))
            .args(["coverage", "--", "-E", filter, "--color=never"])
            .current_dir(root)
            .env("CARGO_NET_OFFLINE", "true")
            .env("TERM", term)
            .env("NO_COLOR", "1")
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("CARGO_BUILD_TARGET_DIR")
            .env_remove("RUSTC_WORKSPACE_WRAPPER")
            .env_remove("HABEAS_PREVIOUS_WORKSPACE_WRAPPER")
            .env_remove("HABEAS_COVERAGE_RUN")
            .env_remove("HABEAS_COVERAGE_FEATURE_LEDGER")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(slave)
            .spawn()
            .unwrap();
        let mut output = child.wait_with_output().unwrap();
        output.stderr = reader.join().unwrap();
        output
    };
    let assert_conversion_cleared = |text: &str| {
        let count = text.rfind("1/1 attempts").unwrap();
        let location = text.find("Coverage retained in ").unwrap();
        assert!(text[count..location].contains("\x1b[2K"), "{text}");
        assert!(
            !text[location..].contains("\x1b["),
            "final reports must remain plain and readable: {text}"
        );
    };
    for term in ["xterm", "dumb"] {
        let output = run_on_pty(term, "test(second)");
        assert_success(&output);
        let text = String::from_utf8_lossy(&output.stderr);
        if term == "xterm" {
            assert!(
                text.contains("Preparing coverage: recording inputs"),
                "{text}"
            );
            assert!(
                text.contains("Preparing coverage: capturing maps"),
                "{text}"
            );
            assert!(text.contains("Converting coverage"), "{text}");
            assert!(text.contains("1/1 attempts"), "{text}");
            assert!(text.contains("[00:"), "{text}");
            assert!(
                text.contains("\x1b["),
                "PTY progress should update in place"
            );
            assert!(!text.contains("\x1b[3"), "NO_COLOR must suppress color");
            assert_conversion_cleared(&text);
        } else {
            assert!(!text.contains("Converting coverage"), "{text}");
            assert!(!text.contains("\x1b["), "{text}");
        }
        assert!(text.contains("Coverage retained in "), "{text}");
        let saved = runs(root);
        let records = attempts(&saved.last().unwrap().0);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["outcome"], "success");
        assert_eq!(records[0]["processes"].as_array().unwrap().len(), 1);
        assert!(
            !records[0]["processes"][0]["lines"]
                .as_object()
                .unwrap()
                .is_empty()
        );
    }
    let output = coverage(root, &["-E", "test(second)", "--color=never"]);
    assert_success(&output);
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(!text.contains("Converting coverage"), "{text}");
    assert!(!text.contains("\x1b["), "{text}");
    let saved = runs(root);
    let first = attempts(&saved[0].0);
    for (run, _) in &saved[1..] {
        let records = attempts(run);
        assert_eq!(
            records[0]["processes"][0]["lines"],
            first[0]["processes"][0]["lines"]
        );
        assert_eq!(
            records[0]["processes"][0]["functions"],
            first[0]["processes"][0]["functions"]
        );
    }
    for (filter, succeeds, outcome, processes) in [
        ("test(child_process)", true, "success", 2),
        ("test(deliberate_failure)", false, "failed", 1),
        ("test(missing_profile)", false, "success", 0),
    ] {
        let output = run_on_pty("xterm", filter);
        assert_eq!(
            output.status.success(),
            succeeds,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8_lossy(&output.stderr);
        assert_conversion_cleared(&text);
        if processes == 0 {
            assert!(text.contains("coverage: "), "{text}");
            assert!(
                text.contains("partial profiles and reports have been retained"),
                "{text}"
            );
        }
        let saved = runs(root);
        let records = attempts(&saved.last().unwrap().0);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["outcome"], outcome);
        assert_eq!(records[0]["processes"].as_array().unwrap().len(), processes);
        for process in records[0]["processes"].as_array().unwrap() {
            assert!(!process["lines"].as_object().unwrap().is_empty());
            assert!(process["functions"].is_array());
        }
    }
    // An error during preparation must also clear the stage before the CLI
    // prints its diagnostic, without manufacturing test attempts.
    // Cargo does not compile this unreferenced module, but source capture
    // must read it as UTF-8. This fails during preparation regardless of
    // permissions or Cargo's handling of missing build-script watch paths.
    let input = root.join("src/uncompiled.rs");
    fs::write(&input, [0xff]).unwrap();
    assert_eq!(
        fs::read_to_string(&input).unwrap_err().kind(),
        std::io::ErrorKind::InvalidData
    );
    let output = run_on_pty("xterm", "test(second)");
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{text}");
    let stage = text.rfind("Preparing coverage: recording inputs").unwrap();
    let diagnostic = text.find("Coverage preparation failed").unwrap();
    assert!(text[stage..diagnostic].contains("\x1b[2K"), "{text}");
    assert!(!text[diagnostic..].contains("\x1b["), "{text}");
    let saved = runs(root);
    assert_eq!(saved.last().unwrap().1["outcome"], "failed");
    assert!(
        !attempts(&saved.last().unwrap().0).is_empty(),
        "earlier current results survive preparation failure"
    );
}

// {cite}`habeas:util`
fn full_flow(root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_habeas"))
        .arg("test")
        .current_dir(root)
        .env("CARGO_NET_OFFLINE", "true")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("HABEAS_PREVIOUS_WORKSPACE_WRAPPER")
        .env_remove("HABEAS_COVERAGE_RUN")
        .env_remove("HABEAS_COVERAGE_FEATURE_LEDGER")
        .output()
        .unwrap()
}

// {cite}`habeas:util`
fn saved_check(root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_habeas"))
        .arg("coverage-check")
        .current_dir(root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .output()
        .unwrap()
}

// {cite}`habeas:util`
fn model_tripwire(
    root: &Path,
) -> (
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
    std::sync::mpsc::Sender<()>,
    std::thread::JoinHandle<()>,
) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    fs::write(root.join("habeas.toml"), format!(
        "version = 1\n[authoring]\ndefault = 'trap'\n[authoring.configs.trap]\nbackend = 'ollama'\nmodel = 'must-not-be-invoked'\nendpoint = 'http://{}'\n",
        listener.local_addr().unwrap()
    )).unwrap();
    let count = std::sync::Arc::new(AtomicUsize::new(0));
    let observed = count.clone();
    let (stop, stopped) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    observed.fetch_add(1, Ordering::SeqCst);
                    let _ = stream.write_all(b"HTTP/1.1 500 Unexpected model request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => panic!("{error}"),
            }
            if !matches!(
                stopped.try_recv(),
                Err(std::sync::mpsc::TryRecvError::Empty)
            ) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    });
    (count, stop, thread)
}

// {test}`requirements.test-coverage.test`
// {test}`requirements.test-coverage.check`
// {test}`requirements.test-coverage.check.functions`
// {test}`requirements.test-coverage.check.guidance`
#[test]
fn full_test_flow_checks_real_function_counters_after_all_tests() {
    let temp = fixture();
    let root = temp.path();
    let (requests, stop, server) = model_tripwire(root);
    fs::create_dir(root.join("evidence")).unwrap();
    fs::write(root.join("evidence/sentinel.txt"), "existing evidence").unwrap();
    let original_evidence = retained_files(&root.join("evidence"));
    let nextest = root.join(".config/nextest.toml");
    let mut configuration = fs::read_to_string(&nextest)
        .unwrap()
        .parse::<toml_edit::DocumentMut>()
        .unwrap();
    configuration["profile"]["default"]["default-filter"] = toml_edit::value("kind(lib)");
    fs::write(nextest, configuration.to_string()).unwrap();
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        format!("{manifest}\n[dev-dependencies]\ntokio = {{ version = '1', features = ['macros', 'rt'] }}\n"),
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn value(flag: bool) -> u32 {\n if flag { 10 } else { 20 }\n}\n#[cfg(test)] #[test] fn a() { assert_eq!(value(true), 10); }\n#[cfg(test)] #[test] fn b() { assert_eq!(value(false), 20); }\n#[cfg(test)] #[tokio::test] async fn asynchronous() { assert_eq!(value(true), 10); }\n").unwrap();
    fs::write(
        root.join("tests/processes.rs"),
        "#[test] fn integration() { assert_eq!(coverage_fixture::value(true),10); }\n",
    )
    .unwrap();
    let output = full_flow(root);
    assert_success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("Coverage check passed"));
    let checked = saved_check(root);
    assert_success(&checked);
    assert!(String::from_utf8_lossy(&checked.stdout).contains("Coverage check passed"));
    assert_eq!(retained_files(&root.join("evidence")), original_evidence);
    let saved = runs(root);
    assert_eq!(saved.len(), 1);
    assert_eq!(
        attempts(&saved[0].0).len(),
        4,
        "the full workspace includes sync and async unit tests AND integration targets"
    );
    for attempt in attempts(&saved[0].0) {
        for process in attempt["processes"].as_array().unwrap() {
            assert!(
                process["functions"]
                    .as_array()
                    .is_some_and(|functions| !functions.is_empty())
            );
        }
    }
    fs::write(root.join("src/lib.rs"), "pub fn value(flag: bool) -> u32 { if flag { 10 } else { 20 } }\npub fn outer() { fn hidden() {} let f = || {}; f(); }\n#[cfg(test)] #[test] fn a() { value(true); outer(); }\n#[cfg(test)] #[test] fn b() { value(false); }\n#[cfg(test)] #[tokio::test] async fn asynchronous() { assert_eq!(value(true), 10); }\n").unwrap();
    let output = full_flow(root);
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("#fn[outer]/fn[hidden]: zero execution"),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!stdout.contains("#fn[outer]: zero execution"), "{stdout}");
    assert_eq!(
        runs(root).last().unwrap().1["outcome"],
        "success",
        "tests succeeded; the policy caused failure"
    );
    let checked = saved_check(root);
    assert!(!checked.status.success());
    assert!(String::from_utf8_lossy(&checked.stdout).contains("Coverage check failed"));
    assert_eq!(retained_files(&root.join("evidence")), original_evidence);
    stop.send(()).unwrap();
    server.join().unwrap();
    assert_eq!(
        requests.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "verification must never call a model"
    );
}

// {test}`requirements.test-coverage.test`
#[test]
fn failed_suite_stops_full_flow_before_the_coverage_check() {
    let temp = fixture();
    let root = temp.path();
    fs::write(root.join("src/lib.rs"),"pub fn value(_: bool) -> u32 { 10 }\n#[cfg(test)] #[test] fn failure() { panic!(\"deliberate fixture failure\"); }\n").unwrap();
    fs::write(
        root.join("tests/processes.rs"),
        "#[test] fn integration() { coverage_fixture::value(true); }\n",
    )
    .unwrap();
    let output = full_flow(root);
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("Coverage check"));
    assert_eq!(runs(root).last().unwrap().1["outcome"], "failed");
}

// {cite}`habeas:util`
fn retained_files(root: &Path) -> std::collections::BTreeMap<std::path::PathBuf, git2::Oid> {
    let mut files = std::collections::BTreeMap::new();
    let mut pending = vec![root.to_owned()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if matches!(
                path.file_name().unwrap().to_str(),
                Some("current.sqlite-wal" | "current.sqlite-shm")
            ) {
                // SQLite may create coordination files for a read-only WAL
                // snapshot. They are not captured coverage or a selection cache.
                continue;
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    git2::Oid::hash_object(git2::ObjectType::Blob, &fs::read(path).unwrap())
                        .unwrap(),
                );
            }
        }
    }
    files
}

// {test}`requirements.test-coverage.index.render_only`
// {test}`requirements.test-coverage.index.selection`
// {test}`requirements.test-coverage.check`
#[cfg(unix)]
#[test]
fn book_rendering_preserves_retained_coverage_and_never_launches_tests() {
    use mdbook_preprocessor::book::{Book, Chapter};
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Stdio;
    let temp = fixture();
    let root = temp.path();
    let marker = root.join("test-executed");
    let source = root.join("src/lib.rs");
    fs::write(
        &source,
        fs::read_to_string(&source).unwrap().replace(
            "fn second() {",
            &format!("fn second() {{ std::fs::write({marker:?}, b\"executed\").unwrap();"),
        ),
    )
    .unwrap();
    assert_success(&coverage(root, &["-E", "test(second)"]));
    assert!(
        marker.exists(),
        "the captured test must exercise the execution sentinel"
    );
    fs::remove_file(&marker).unwrap();
    let storage = root.join("target/habeas/coverage");
    let before = retained_files(&storage);
    let bin = root.join("read-only-tools");
    fs::create_dir(&bin).unwrap();
    let forbidden = root.join("forbidden-command");
    let calls = root.join("metadata-calls");
    fs::write(&calls, "").unwrap();
    let cargo = bin.join("cargo");
    fs::write(&cargo, "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$HABEAS_CARGO_CALL_LOG\"\nif [ \"$1\" != metadata ]; then\n  printf '%s\\n' \"$*\" >> \"$HABEAS_FORBIDDEN_COMMAND\"\n  exit 99\nfi\nexec \"$HABEAS_REAL_CARGO\" \"$@\"\n").unwrap();
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
    let tool_path = std::env::join_paths(&paths).unwrap();
    let checked = Command::new(env!("CARGO_BIN_EXE_habeas"))
        .arg("coverage-check")
        .current_dir(root)
        .env("PATH", &tool_path)
        .env("HABEAS_REAL_CARGO", env!("CARGO"))
        .env("HABEAS_CARGO_CALL_LOG", &calls)
        .env("HABEAS_FORBIDDEN_COMMAND", &forbidden)
        .env("CARGO_NET_OFFLINE", "true")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .output()
        .unwrap();
    assert!(
        !checked.status.success(),
        "filtered coverage leaves unmeasured functions"
    );
    assert!(
        String::from_utf8_lossy(&checked.stdout).contains("File execution coverage"),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    assert!(!marker.exists(), "standalone coverage-check ran a test");
    assert!(
        !forbidden.exists(),
        "standalone coverage-check launched compilation or collection"
    );
    assert_eq!(
        retained_files(&storage),
        before,
        "standalone coverage-check modified captured data"
    );
    let mut book = Book::new();
    book.push_item(Chapter::new(
        "Coverage",
        ":::{coverage-index}\n:::\n".into(),
        "coverage.md",
        vec![],
    ));
    let input = serde_json::json!([{
        "root": root, "config": {"book":{"src":"md"}}, "renderer":"html", "mdbook_version":"0.5.3"
    }, book]);
    let mut child = Command::new(env!("CARGO_BIN_EXE_habeas"))
        .arg("mdbook")
        .current_dir(root)
        .env("PATH", &tool_path)
        .env("HABEAS_REAL_CARGO", env!("CARGO"))
        .env("HABEAS_CARGO_CALL_LOG", &calls)
        .env("HABEAS_FORBIDDEN_COMMAND", &forbidden)
        .env("CARGO_NET_OFFLINE", "true")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_success(&output);
    let rendered: Book = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        rendered
            .chapters()
            .find(|chapter| chapter.name == "Documented coverage gaps")
            .unwrap()
            .content
            .contains("No documented coverage gaps are present.")
    );
    assert!(
        fs::read_to_string(calls)
            .unwrap()
            .lines()
            .all(|line| line.starts_with("metadata "))
    );
    assert!(
        !forbidden.exists(),
        "rendering invoked a test or coverage command"
    );
    assert!(!marker.exists(), "rendering ran a captured test binary");
    assert_eq!(
        retained_files(&storage),
        before,
        "rendering modified collected coverage"
    );
}

// {test}`requirements.test-coverage.storage.provenance`
// {test}`requirements.test-coverage.index.freshness`
#[test]
fn real_build_captures_dependency_files_and_environment_for_read_only_freshness_checks() {
    let temp = fixture();
    let root = temp.path();
    fs::write(root.join("included data.txt"), "included").unwrap();
    fs::write(root.join("build-input.data"), "build input").unwrap();
    fs::write(
        root.join("build.rs"),
        "fn main() {\nprintln!(\"cargo:rerun-if-changed=build-input.data\");\nprintln!(\"cargo:rerun-if-env-changed=HABEAS_FIXTURE_BUILD_ENV\");\n}\n",
    )
    .unwrap();
    let source = root.join("src/lib.rs");
    fs::write(
        &source,
        fs::read_to_string(&source).unwrap().replace(
            "fn second() {",
            "fn second() { assert_eq!(include_str!(\"../included data.txt\"), \"included\"); assert_eq!(env!(\"HABEAS_FIXTURE_RUST_ENV\"), \"compiler-value\");",
        ),
    )
    .unwrap();
    assert_success(
        &Command::new(env!("CARGO_BIN_EXE_habeas"))
            .args(["coverage", "--", "-E", "test(second)"])
            .current_dir(root)
            .env("HABEAS_FIXTURE_RUST_ENV", "compiler-value")
            .env("HABEAS_FIXTURE_BUILD_ENV", "build-value")
            .env("CARGO_NET_OFFLINE", "true")
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("CARGO_BUILD_TARGET_DIR")
            .env_remove("RUSTC_WORKSPACE_WRAPPER")
            .env_remove("HABEAS_PREVIOUS_WORKSPACE_WRAPPER")
            .env_remove("HABEAS_COVERAGE_RUN")
            .env_remove("HABEAS_COVERAGE_FEATURE_LEDGER")
            .output()
            .unwrap(),
    );
    let saved = runs(root);
    assert_eq!(saved.len(), 1);
    let recorded_inputs = saved[0].1["inputs"].as_object().unwrap();
    for file in [
        "included data.txt",
        "build-input.data",
        "build.rs",
        "Cargo.lock",
    ] {
        assert!(
            recorded_inputs.keys().any(|path| {
                fs::canonicalize(path).ok() == Some(fs::canonicalize(root.join(file)).unwrap())
            }),
            "capture omitted {file}: {:?}",
            recorded_inputs.keys().collect::<Vec<_>>()
        );
    }
    assert_eq!(
        saved[0].1["environment_inputs"]["HABEAS_FIXTURE_RUST_ENV"],
        "compiler-value"
    );
    assert_eq!(
        saved[0].1["environment_inputs"]["HABEAS_FIXTURE_BUILD_ENV"],
        "build-value"
    );
    let check = |compiler: &str, build: &str| {
        let output = Command::new(env!("CARGO_BIN_EXE_habeas"))
            .arg("coverage-check")
            .current_dir(root)
            .env("HABEAS_FIXTURE_RUST_ENV", compiler)
            .env("HABEAS_FIXTURE_BUILD_ENV", build)
            .env("CARGO_NET_OFFLINE", "true")
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("CARGO_BUILD_TARGET_DIR")
            .output()
            .unwrap();
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    let storage = root.join("target/habeas/coverage");
    let before = retained_files(&storage);
    assert!(!check("compiler-value", "build-value").contains("stale coverage"));
    for file in ["included data.txt", "build-input.data"] {
        let path = root.join(file);
        let original = fs::read(&path).unwrap();
        fs::write(&path, "changed").unwrap();
        assert!(
            check("compiler-value", "build-value").contains("stale coverage"),
            "{file}"
        );
        fs::write(path, original).unwrap();
        assert!(!check("compiler-value", "build-value").contains("stale coverage"));
    }
    for (compiler, build) in [("changed", "build-value"), ("compiler-value", "changed")] {
        assert!(check(compiler, build).contains("stale coverage"));
    }
    assert_eq!(retained_files(&storage), before);
}

// {test}`requirements.test-coverage.storage.replace`
// {test}`requirements.test-coverage.run`
// {test}`requirements.test-coverage.processes`
// {test}`requirements.test-coverage.processes.reports`
// {test}`requirements.test-coverage.storage`
// {test}`requirements.test-coverage.storage.provenance`
// {test}`requirements.test-coverage.storage.configuration`
// {test}`requirements.test-coverage.storage.outcomes`
// {test}`requirements.test-coverage.storage.current`
// {test}`requirements.test-coverage.storage.cleanup`
// {test}`requirements.test-coverage.storage.selection`
// {test}`requirements.test-coverage.failures`
// {test}`requirements.test-coverage.failures.profiles`
#[test]
fn real_parallel_processes_retries_child_profiles_and_filtered_reruns() {
    let temp = fixture();
    let root = temp.path();
    let source = fs::read_to_string(root.join("src/lib.rs")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        format!(
            "{source}\n{}",
            r#"
#[cfg(test)] fn overlap(own: &str, other: &str) {
    if std::env::var_os("HABEAS_FIXTURE_OVERLAP").is_none() { return; }
    std::fs::write(own, "started").unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !std::path::Path::new(other).exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(std::path::Path::new(other).exists(), "other test did not overlap");
}
"#
        )
        .replace(
            "fn same_name() {",
            "fn same_name() { overlap(\"same.started\", \"second.started\");",
        )
        .replace(
            "fn second() {",
            "fn second() { overlap(\"second.started\", \"same.started\");",
        ),
    )
    .unwrap();
    let processes = fs::read_to_string(root.join("tests/processes.rs")).unwrap();
    fs::write(
        root.join("tests/processes.rs"),
        processes.replace(
            "#[test] fn retries() {",
            "#[test] fn retries() { coverage_fixture::value(std::env::var(\"NEXTEST_ATTEMPT\").unwrap() == \"1\");",
        ),
    ).unwrap();
    assert_success(&coverage_with_environment(
        root,
        &["--workspace", "--retries", "1", "-j", "2", "--no-fail-fast"],
        &[("HABEAS_FIXTURE_OVERLAP", "1")],
    ));
    assert!(root.join("same.started").is_file());
    assert!(root.join("second.started").is_file());
    let saved = runs(root);
    assert_eq!(saved.len(), 1);
    let first = saved[0].0.clone();
    let first_configuration = saved[0].1["configuration"].clone();
    assert_eq!(
        saved[0].1["id"],
        first.file_name().unwrap().to_str().unwrap()
    );
    assert!(saved[0].1["started"].as_u64().unwrap() > 0);
    assert_eq!(saved[0].1["command"][0], "cargo nextest run");
    assert!(
        saved[0].1["command"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("--workspace"))
    );
    assert_eq!(saved[0].1["outcome"], "success");
    assert_eq!(saved[0].1["selection"].as_array().unwrap().len(), 5);
    assert_eq!(
        saved[0].1["selection"],
        serde_json::json!([
            {"binary":"coverage-fixture", "test":"same_name"},
            {"binary":"coverage-fixture", "test":"second"},
            {"binary":"coverage-fixture::processes", "test":"child_process"},
            {"binary":"coverage-fixture::processes", "test":"retries"},
            {"binary":"coverage-fixture::processes", "test":"same_name"},
        ])
    );
    assert!(saved[0].1["revision"].is_null());
    assert!(saved[0].1["dirty"].is_null());
    assert!(
        saved[0].1["inputs"]
            .get(root.join("src/lib.rs").to_str().unwrap())
            .is_some()
    );
    assert!(
        saved[0].1["configuration_inputs"]["rustc"]
            .as_str()
            .unwrap()
            .contains("LLVM version:")
    );
    assert!(
        !saved[0].1["configuration_inputs"]["target"]
            .as_str()
            .unwrap()
            .is_empty()
    );
    assert_eq!(saved[0].1["configuration_inputs"]["cargo-profile"], "test");
    assert!(
        saved[0].1["sources"]["src/lib.rs"]
            .as_str()
            .unwrap()
            .contains("pub fn value")
    );
    assert!(first.join("maps.json").is_file());
    assert!(!first.join("run.json").exists());
    let records = attempts(&first);
    assert_eq!(records.len(), 5);
    let retry = records
        .iter()
        .find(|record| record["test"] == "retries")
        .unwrap();
    assert_eq!(retry["attempt"], "2");
    assert_eq!(retry["outcome"], "success");
    assert_eq!(
        retry["processes"].as_array().unwrap().len(),
        1,
        "retry processes are not combined"
    );
    assert_eq!(retry["processes"][0]["pid"], retry["pid"]);
    assert_eq!(retry["processes"][0]["lines"]["src/lib.rs"]["3"], 0);
    assert!(
        retry["processes"][0]["lines"]["src/lib.rs"]["5"]
            .as_u64()
            .unwrap()
            > 0
    );
    let child = records
        .iter()
        .find(|record| record["test"] == "child_process")
        .unwrap();
    assert_eq!(child["processes"].as_array().unwrap().len(), 2);
    let binaries = records
        .iter()
        .filter(|record| record["test"] == "same_name")
        .map(|record| record["binary"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        binaries,
        std::collections::BTreeSet::from(["coverage-fixture", "coverage-fixture::processes"])
    );
    assert_eq!(
        fs::read_dir(first.join("attempts")).unwrap().count(),
        5,
        "superseded retry profiles are removed"
    );
    for record in records.iter().filter(|record| {
        record["binary"] == "coverage-fixture"
            && matches!(record["test"].as_str(), Some("same_name" | "second"))
    }) {
        let counts = &record["processes"][0]["lines"]["src/lib.rs"];
        let true_branch = counts["3"].as_u64().unwrap();
        let false_branch = counts["5"].as_u64().unwrap();
        if record["test"] == "same_name" {
            assert!(true_branch > 0);
            assert_eq!(false_branch, 0);
        } else {
            assert_eq!(true_branch, 0);
            assert!(false_branch > 0);
        }
    }
    let mut profile_paths = std::collections::BTreeSet::new();
    for record in &records {
        assert_profiles_present(&first, record);
        assert!(
            !first
                .join("attempts")
                .join(record["id"].as_str().unwrap())
                .join("attempt.json")
                .exists()
        );
        for process in record["processes"].as_array().unwrap() {
            assert!(process["diagnostic"].is_null());
            assert!(!process["lines"].as_object().unwrap().is_empty());
            for profile in process["profiles"].as_array().unwrap() {
                assert!(
                    profile_paths.insert(profile.as_str().unwrap()),
                    "distinct processes cannot share a raw profile"
                );
                assert!(
                    profile
                        .as_str()
                        .unwrap()
                        .starts_with(&format!("{}-", process["pid"].as_u64().unwrap())),
                    "profile belongs to a different process"
                );
                assert!(
                    first
                        .join("attempts")
                        .join(record["id"].as_str().unwrap())
                        .join(profile.as_str().unwrap())
                        .is_file()
                );
            }
        }
    }
    let old_second = records
        .iter()
        .find(|record| record["test"] == "second")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    assert_success(&coverage(root, &["-E", "test(second)"]));
    let saved = runs(root);
    assert_eq!(saved.len(), 2);
    assert_eq!(attempts(&first).len(), 4);
    assert_eq!(attempts(&saved[1].0).len(), 1);
    assert!(!first.join("attempts").join(old_second).exists());
    assert_eq!(saved[1].1["configuration"], first_configuration);
    let replaced_run = saved[1].0.clone();
    let config = root.join(".config/nextest.toml");
    let mut source = fs::read_to_string(&config).unwrap();
    source.push_str("\n[profile.focused]\ntest-threads = 1\n");
    fs::write(config, source).unwrap();
    assert_success(&coverage(
        root,
        &["--profile", "focused", "-E", "test(second)"],
    ));
    let saved = runs(root);
    assert_eq!(saved.len(), 2);
    assert_eq!(
        saved.last().unwrap().1["configuration"],
        first_configuration
    );
    assert!(!replaced_run.exists());
    let failed = coverage(root, &["-E", "test(retries)", "--retries", "0"]);
    assert!(!failed.status.success());
    let saved = runs(root);
    let newest = saved.last().unwrap();
    assert_eq!(newest.1["outcome"], "failed");
    let failed_attempt = attempts(&newest.0)[0].clone();
    assert_eq!(failed_attempt["outcome"], "failed");
    assert_profiles_present(&newest.0, &failed_attempt);
    let failed_run = newest.0.clone();
    assert_eq!(attempts(&first).len(), 3);
    assert_success(&coverage(root, &["-E", "test(retries)", "--retries", "1"]));
    assert!(
        !failed_run.exists(),
        "a later attempt replaces retained failure profiles"
    );
    let saved = runs(root);
    let before_empty: Vec<_> = saved.iter().flat_map(|(run, _)| attempts(run)).collect();
    for empty_args in [
        vec!["-E", "test(definitely_missing)", "--no-tests", "pass"],
        vec!["-E", "test(definitely_missing)"],
    ] {
        let output = coverage(root, &empty_args);
        assert_eq!(
            output.status.code(),
            Some(if empty_args.len() == 4 { 0 } else { 4 })
        );
        let saved = runs(root);
        assert_eq!(
            saved
                .iter()
                .flat_map(|(run, _)| attempts(run))
                .collect::<Vec<_>>(),
            before_empty
        );
        assert!(
            saved.last().unwrap().1["diagnostics"][0]
                .as_str()
                .unwrap()
                .contains("Empty test selection")
        );
    }
    assert_success(&coverage(
        root,
        &["--features", "alpha", "-Fbeta", "-E", "test(second)"],
    ));
    let saved = runs(root);
    assert_eq!(
        saved.len(),
        1,
        "incompatible configuration drops every previous result"
    );
    assert_ne!(saved[0].1["configuration"], first_configuration);
    assert!(!first.exists());
    let enabled: Value = serde_json::from_str(
        saved[0].1["configuration_inputs"]["enabled-features"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert!(enabled.as_object().unwrap().values().any(|features| {
        features
            .as_array()
            .unwrap()
            .contains(&Value::String("alpha".into()))
            && features
                .as_array()
                .unwrap()
                .contains(&Value::String("beta".into()))
    }));
    assert_success(&coverage(
        root,
        &[
            "--config",
            "build.build-dir='custom-build'",
            "-E",
            "test(second)",
        ],
    ));
    assert!(!root.join("custom-build").exists());
    let before_no_run = runs(root);
    let prior_attempt = attempts(&before_no_run[0].0);
    let output = coverage(
        root,
        &["--features", "alpha", "--no-run", "-E", "test(second)"],
    );
    assert_success(&output);
    assert!(String::from_utf8_lossy(&output.stderr).contains("Compilation-only"));
    assert_eq!(runs(root), before_no_run);
    assert_eq!(attempts(&before_no_run[0].0), prior_attempt);
    fs::write(
        root.join("src/lib.rs"),
        "compile_error!(\"deliberate build failure\");\n",
    )
    .unwrap();
    let output = coverage(root, &["--features", "alpha", "-E", "test(second)"]);
    assert!(!output.status.success());
    assert_eq!(runs(root), before_no_run);
    assert_eq!(attempts(&before_no_run[0].0), prior_attempt);
}

// {test}`requirements.test-coverage.run`
// {test}`requirements.test-coverage.storage`
// {test}`requirements.test-coverage.storage.selection`
#[test]
fn configured_retries_filters_profiles_and_libtest_arguments_are_preserved() {
    let temp = fixture();
    let root = temp.path();
    let path = root.join(".config/nextest.toml");
    let mut config: toml_edit::DocumentMut = fs::read_to_string(&path).unwrap().parse().unwrap();
    config["profile"]["default"]["retries"] = toml_edit::value(1);
    config["profile"]["default"]["default-filter"] = toml_edit::value("test(retries)");
    config["profile"]["focused"]["default-filter"] = toml_edit::value("test(second)");
    fs::write(path, config.to_string()).unwrap();
    assert_success(&coverage(root, &[]));
    let saved = runs(root);
    let records = attempts(&saved[0].0);
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0]["attempt"], "2");
    assert!(records.iter().all(|attempt| attempt["test"] == "retries"));
    assert!(
        records
            .iter()
            .any(|attempt| attempt["outcome"] == "success")
    );
    assert_success(&coverage(
        root,
        &["--profile", "focused", "--", "--exact", "second"],
    ));
    let saved = runs(root);
    let records = attempts(&saved.last().unwrap().0);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["test"], "second");
    assert_eq!(
        saved.last().unwrap().1["selection"],
        serde_json::json!([{"binary":"coverage-fixture", "test":"second"}])
    );
    assert_eq!(saved[0].1["configuration"], saved[1].1["configuration"]);
}

// {test}`requirements.test-coverage.storage.selection`
// {test}`requirements.test-coverage.storage`
// {test}`requirements.test-coverage.storage.replace`
#[test]
fn package_binary_partition_and_ignored_selection_replace_only_their_actual_test_identities() {
    let temp = fixture();
    let root = temp.path();
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        format!("{manifest}\n[workspace]\nmembers=['member']\n"),
    )
    .unwrap();
    fs::create_dir_all(root.join("member/src")).unwrap();
    fs::write(
        root.join("member/Cargo.toml"),
        "[package]\nname='workspace-member'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::write(
        root.join("member/src/lib.rs"),
        "#[test] fn member_runs() {}\n",
    )
    .unwrap();
    let source = fs::read_to_string(root.join("src/lib.rs")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        format!("{source}\n#[test] #[ignore] fn ignored_only() {{}}\n"),
    )
    .unwrap();
    assert_success(&coverage(root, &["--workspace", "--retries", "1"]));
    let baseline = runs(root);
    let original: Vec<Value> = baseline.iter().flat_map(|(run, _)| attempts(run)).collect();
    assert_eq!(original.len(), 6);
    let mut expected = original.clone();
    for (args, identities) in [
        (
            vec!["-p", "workspace-member"],
            serde_json::json!([{"binary":"workspace-member", "test":"member_runs"}]),
        ),
        (
            vec!["--workspace", "--lib", "-E", "test(second)"],
            serde_json::json!([{"binary":"coverage-fixture", "test":"second"}]),
        ),
        (
            vec![
                "--workspace",
                "--test",
                "processes",
                "-E",
                "test(same_name)",
            ],
            serde_json::json!([{"binary":"coverage-fixture::processes", "test":"same_name"}]),
        ),
        (
            vec!["--workspace", "--run-ignored", "only"],
            serde_json::json!([{"binary":"coverage-fixture", "test":"ignored_only"}]),
        ),
    ] {
        assert_success(&coverage(root, &args));
        let saved = runs(root);
        let latest = saved.last().unwrap();
        assert_eq!(latest.1["selection"], identities, "arguments: {args:?}");
        let replacements = attempts(&latest.0);
        assert_eq!(replacements.len(), identities.as_array().unwrap().len());
        for replacement in &replacements {
            expected.retain(|old| {
                old["binary"] != replacement["binary"] || old["test"] != replacement["test"]
            });
            expected.push(replacement.clone());
        }
        let mut actual: Vec<_> = saved.iter().flat_map(|(run, _)| attempts(run)).collect();
        let key = |value: &Value| {
            (
                value["binary"].as_str().unwrap().to_owned(),
                value["test"].as_str().unwrap().to_owned(),
            )
        };
        expected.sort_by_key(key);
        actual.sort_by_key(key);
        assert_eq!(
            actual, expected,
            "unselected metadata and measurements must remain unchanged"
        );
        assert_eq!(latest.1["configuration"], baseline[0].1["configuration"]);
    }
    let mut partitions = std::collections::BTreeSet::new();
    for partition in ["hash:1/2", "hash:2/2"] {
        assert_success(&coverage(
            root,
            &[
                "--workspace",
                "--partition",
                partition,
                "--retries",
                "1",
                "--no-tests",
                "pass",
            ],
        ));
        let saved = runs(root);
        let latest = saved.last().unwrap();
        let actual = attempts(&latest.0);
        assert_eq!(
            latest.1["selection"],
            serde_json::Value::Array(
                actual
                    .iter()
                    .map(|record| {
                        serde_json::json!({"binary":record["binary"], "test":record["test"]})
                    })
                    .collect()
            )
        );
        for identity in latest.1["selection"].as_array().unwrap() {
            assert!(
                partitions.insert((
                    identity["binary"].as_str().unwrap().to_owned(),
                    identity["test"].as_str().unwrap().to_owned()
                )),
                "partitions must be disjoint"
            );
        }
    }
    assert_eq!(
        partitions,
        original
            .iter()
            .map(|record| (
                record["binary"].as_str().unwrap().to_owned(),
                record["test"].as_str().unwrap().to_owned(),
            ))
            .collect(),
        "the two partitions together select all non-ignored workspace tests"
    );
    assert_eq!(
        runs(root).iter().flat_map(|(run, _)| attempts(run)).count(),
        7,
        "partition reruns preserve the unselected ignored-test result"
    );
}

// {test}`requirements.test-coverage.storage.selection`
// {test}`requirements.test-coverage.storage.replace`
// {test}`requirements.test-coverage.storage.outcomes`
// {test}`requirements.test-coverage.check.measurement`
#[test]
fn fail_fast_leaves_every_unstarted_selected_test_pending_without_old_measurements() {
    let temp = fixture();
    let root = temp.path();
    fs::write(root.join("tests/processes.rs"), "").unwrap();
    fs::write(
        root.join("src/lib.rs"),
        r#"
#[test] fn a_failure() {
    assert!(std::env::var_os("HABEAS_FAIL_FIXTURE").is_none());
}
#[test] fn b_selected() { assert_eq!(std::hint::black_box(1), 1); }
#[test] fn c_selected() { assert_eq!(std::hint::black_box(2), 2); }
"#,
    )
    .unwrap();
    let config = root.join(".config/nextest.toml");
    let source = fs::read_to_string(&config).unwrap();
    fs::write(
        &config,
        format!(
            "{source}\n[[profile.default.overrides]]\nfilter='test(a_failure)'\npriority=100\n"
        ),
    )
    .unwrap();
    assert_success(&coverage(root, &["--workspace", "-j1"]));
    let old = runs(root)[0].0.clone();
    assert!(
        attempts(&old)
            .iter()
            .all(|attempt| !attempt["processes"].as_array().unwrap().is_empty())
    );
    let output = Command::new(env!("CARGO_BIN_EXE_habeas"))
        .args(["coverage", "--", "--workspace", "-j1", "--fail-fast"])
        .current_dir(root)
        .env("HABEAS_FAIL_FIXTURE", "1")
        .env("CARGO_NET_OFFLINE", "true")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("HABEAS_COVERAGE_RUN")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!old.exists());
    let saved = runs(root);
    assert_eq!(saved.len(), 1);
    let records = attempts(&saved[0].0);
    assert_eq!(records.len(), 3);
    for record in records {
        if record["test"] == "a_failure" {
            assert_eq!(record["outcome"], "failed");
        } else {
            assert_eq!(record["outcome"], "pending", "{record}");
            assert!(record["processes"].as_array().unwrap().is_empty());
        }
    }
    let checked = saved_check(root);
    assert!(!checked.status.success());
    let text = String::from_utf8_lossy(&checked.stdout);
    assert!(text.contains("pending"), "{text}");
    assert!(
        !text.contains("100.0%"),
        "partial results must not be presented as complete"
    );
}

// {test}`requirements.test-coverage.storage.selection`
// {test}`requirements.test-coverage.storage.replace`
#[cfg(unix)]
#[test]
fn selection_discovery_failure_preserves_existing_results_and_configuration() {
    use std::os::unix::fs::PermissionsExt;
    let temp = fixture();
    let root = temp.path();
    assert_success(&coverage(root, &["-E", "test(second)"]));
    let before = runs(root);
    let measurements = attempts(&before[0].0);
    let tools = root.join("failing-list");
    fs::create_dir(&tools).unwrap();
    let cargo = tools.join("cargo");
    fs::write(&cargo, "#!/bin/sh\nif [ \"$1\" = nextest ] && [ \"$2\" = list ]; then\n  echo 'synthetic selection discovery failure' >&2\n  exit 63\nfi\nexec \"$HABEAS_REAL_CARGO\" \"$@\"\n").unwrap();
    fs::set_permissions(cargo, fs::Permissions::from_mode(0o755)).unwrap();
    let mut paths = vec![tools];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
    let output = Command::new(env!("CARGO_BIN_EXE_habeas"))
        .args([
            "coverage",
            "--",
            "--features",
            "alpha",
            "-E",
            "test(second)",
        ])
        .current_dir(root)
        .env("PATH", std::env::join_paths(paths).unwrap())
        .env("HABEAS_REAL_CARGO", env!("CARGO"))
        .env("CARGO_NET_OFFLINE", "true")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("HABEAS_COVERAGE_RUN")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("selection discovery failed"));
    assert_eq!(runs(root), before);
    assert_eq!(attempts(&before[0].0), measurements);
}

// {test}`requirements.test-coverage.storage.replace`
#[test]
fn default_coverage_and_test_replace_removed_tests() {
    let temp = fixture();
    let root = temp.path();
    fs::write(root.join("tests/processes.rs"), "").unwrap();
    for full in [false, true] {
        let source = root.join("src/lib.rs");
        fs::write(
            &source,
            "#[test] fn obsolete() { assert!(std::hint::black_box(true)); }",
        )
        .unwrap();
        assert_success(&coverage(root, &["-E", "test(obsolete)"]));
        let prior = runs(root)
            .into_iter()
            .find(|(run, _)| attempts(run).iter().any(|a| a["test"] == "obsolete"))
            .unwrap()
            .0;
        fs::write(
            &source,
            "#[test] fn current() { assert!(std::hint::black_box(true)); }",
        )
        .unwrap();
        let output = if full {
            full_flow(root)
        } else {
            coverage(root, &[])
        };
        assert_success(&output);
        assert!(!prior.exists());
        let saved = runs(root);
        assert_eq!(saved.len(), 1);
        let current = attempts(&saved[0].0);
        assert_eq!(current.len(), 1);
        assert_eq!(current[0]["test"], "current");
    }
}

// {test}`requirements.test-coverage.storage.cleanup`
// {test}`requirements.test-coverage.failures`
// {test}`requirements.test-coverage.failures.profiles`
#[cfg(unix)]
#[test]
fn cleanup_failure_is_reported_and_next_collection_removes_orphans() {
    use std::os::unix::fs::PermissionsExt;
    let temp = fixture();
    let root = temp.path();
    assert_success(&coverage(root, &["-E", "test(second)"]));
    let before = runs(root);
    let original = attempts(&before[0].0);
    let orphan = root
        .join("target/habeas/coverage")
        .join(uuid::Uuid::new_v4().to_string());
    fs::create_dir(&orphan).unwrap();
    fs::write(orphan.join("obsolete.profraw"), "orphaned profile").unwrap();
    fs::set_permissions(&orphan, fs::Permissions::from_mode(0o000)).unwrap();
    let output = coverage(root, &["-E", "test(second)"]);
    fs::set_permissions(&orphan, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Permission denied"));
    assert_eq!(runs(root), before);
    assert_eq!(attempts(&before[0].0), original);
    assert_success(&coverage(root, &["-E", "test(second)"]));
    assert!(!orphan.exists());
}

// {test}`requirements.test-coverage.storage.cleanup`
#[cfg(unix)]
#[test]
fn cleanup_failures_before_and_after_execution_invalidate_the_run() {
    use std::os::unix::fs::PermissionsExt;
    let temp = fixture();
    let root = temp.path();
    assert_success(&coverage(root, &["-E", "test(second)"]));
    let old = &runs(root)[0].0;
    let attempt = &attempts(old)[0];
    // The first cleanup retains current attempts. Replacement retires this one.
    let blocked = old
        .join("attempts")
        .join(attempt["id"].as_str().unwrap())
        .join("blocked");
    fs::create_dir(&blocked).unwrap();
    fs::write(blocked.join("old.profraw"), b"obsolete").unwrap();
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).unwrap();
    let failed = coverage(root, &["-E", "test(second)"]);
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("Permission denied"));
    let saved = runs(root);
    assert_eq!(saved.len(), 1);
    assert_ne!(saved[0].0, *old);
    assert_eq!(saved[0].1["outcome"], "failed");
    assert!(
        saved[0].1["diagnostics"]
            .to_string()
            .contains("cleanup failed before test execution")
    );
    assert!(
        attempts(&saved[0].0)[0]["processes"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // A test can create an orphan only after the pre-execution cleanup has finished.
    fs::write(root.join("tests/processes.rs"), "").unwrap();
    fs::write(root.join("src/lib.rs"), r#"
#[test] fn create_orphan() {
    use std::os::unix::fs::PermissionsExt;
    let orphan = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/habeas/coverage/orphan");
    std::fs::create_dir(&orphan).unwrap();
    std::fs::write(orphan.join("obsolete.profraw"), b"obsolete").unwrap();
    std::fs::set_permissions(orphan, std::fs::Permissions::from_mode(0o000)).unwrap();
}
"#).unwrap();
    let failed = coverage(root, &[]);
    let orphan = root.join("target/habeas/coverage/orphan");
    fs::set_permissions(&orphan, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("Coverage cleanup failed: "));
    let saved = runs(root);
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].1["outcome"], "failed");
    assert!(
        saved[0].1["diagnostics"]
            .to_string()
            .contains("Coverage cleanup failed: ")
    );
    let current = attempts(&saved[0].0);
    assert_eq!(current.len(), 1);
    assert_eq!(current[0]["test"], "create_orphan");
    assert_eq!(current[0]["outcome"], "success");
}

// {test}`requirements.test-coverage.failures`
// {test}`requirements.test-coverage.storage.outcomes`
#[cfg(unix)]
#[test]
fn a_build_that_cannot_start_preserves_the_current_set() {
    use std::os::unix::{fs::PermissionsExt, fs::symlink};
    let temp = fixture();
    let root = temp.path();
    let sysroot = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .unwrap();
    assert_success(&sysroot);
    let toolchain = std::path::PathBuf::from(String::from_utf8(sysroot.stdout).unwrap().trim());
    let bin = root.join("fake-tools");
    fs::create_dir(&bin).unwrap();
    symlink(toolchain.join("bin/rustc"), bin.join("rustc")).unwrap();
    let nextest = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|directory| directory.join("cargo-nextest"))
        .find(|path| path.is_file())
        .expect("cargo-nextest is installed");
    symlink(nextest, bin.join("cargo-nextest")).unwrap();
    let cargo = bin.join("cargo");
    fs::write(&cargo, "#!/bin/sh\nif [ \"$1\" = metadata ]; then\n\"$HABEAS_TEST_CARGO_PATH\" \"$@\"\nresult=$?\n/bin/rm \"$0\"\nexit \"$result\"\nfi\nexec \"$HABEAS_TEST_CARGO_PATH\" \"$@\"\n").unwrap();
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_habeas"))
        .arg("coverage")
        .arg("--")
        .args(["-E", "test(second)"])
        .current_dir(root)
        .env(
            "PATH",
            std::env::join_paths([bin.as_path(), Path::new("/usr/bin"), Path::new("/bin")])
                .unwrap(),
        )
        .env("HABEAS_TEST_CARGO_PATH", toolchain.join("bin/cargo"))
        .env("CARGO_NET_OFFLINE", "true")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        runs(root).is_empty(),
        "failed preparation does not establish a new coverage set"
    );
}

// {test}`requirements.test-coverage.run`
// {test}`requirements.test-coverage.storage`
// {test}`requirements.test-coverage.storage.provenance`
#[test]
fn no_arguments_selects_all_workspace_members_with_configured_parallelism_and_git_metadata() {
    let temp = fixture();
    let root = temp.path();
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        format!("{manifest}\n[workspace]\nmembers = ['member']\ndefault-members = ['.']\n"),
    )
    .unwrap();
    fs::create_dir_all(root.join("member/src")).unwrap();
    fs::write(
        root.join("member/Cargo.toml"),
        "[package]\nname = 'workspace-member'\nversion = '0.1.0'\nedition = '2024'\n",
    )
    .unwrap();
    fs::write(
        root.join("member/src/lib.rs"),
        "#[test] fn member_runs() {}\n",
    )
    .unwrap();
    fs::write(root.join("tests/processes.rs"), "").unwrap();
    fs::write(
        root.join("src/lib.rs"),
        r#"
fn rendezvous(own: &str, other: &str) {
    std::fs::write(own, "started").unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !std::path::Path::new(other).exists() {
        assert!(std::time::Instant::now() < deadline, "tests did not run in parallel");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
#[test] fn parallel_a() { rendezvous("a.started", "b.started"); }
#[test] fn parallel_b() { rendezvous("b.started", "a.started"); }
"#,
    )
    .unwrap();
    let config = root.join(".config/nextest.toml");
    let mut source: toml_edit::DocumentMut = fs::read_to_string(&config).unwrap().parse().unwrap();
    source["profile"]["default"]["test-threads"] = toml_edit::value(2);
    fs::write(config, source.to_string()).unwrap();
    // Cargo's configured target is also where retained coverage must live.
    fs::create_dir_all(root.join(".cargo")).unwrap();
    fs::write(
        root.join(".cargo/config.toml"),
        "[build]\ntarget-dir = 'custom-target'\n",
    )
    .unwrap();
    fs::write(
        root.join(".gitignore"),
        "custom-target/\n*.started\nCargo.lock\n",
    )
    .unwrap();
    let repo = git2::Repository::init(root).unwrap();
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let signature = git2::Signature::now("Coverage fixture", "fixture@example.test").unwrap();
    let revision = repo
        .commit(Some("HEAD"), &signature, &signature, "Fixture", &tree, &[])
        .unwrap();
    let source = fs::read_to_string(root.join("src/lib.rs")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        format!("{source}\n// dirty source bytes\n"),
    )
    .unwrap();
    assert_success(&coverage(root, &[]));
    assert!(!root.join("target/habeas/coverage").exists());
    let storage = root.join("custom-target/habeas/coverage");
    let saved = runs_in(&storage);
    let run_path = saved[0].0.clone();
    let run = &saved[0].1;
    assert_eq!(run["revision"], revision.to_string());
    assert_eq!(run["dirty"], true);
    assert_eq!(run["selection"].as_array().unwrap().len(), 3);
    let records = attempts(&run_path);
    assert_eq!(records.len(), 3);
    assert!(
        records
            .iter()
            .any(|attempt| attempt["test"] == "member_runs")
    );
    assert!(
        records
            .iter()
            .all(|attempt| attempt["outcome"] == "success")
    );
    fs::write(root.join("src/lib.rs"), source).unwrap();
    assert_success(&coverage(root, &["--workspace", "-E", "test(member_runs)"]));
    let saved = runs_in(&storage);
    let clean_run = &saved.last().unwrap().1;
    assert_eq!(clean_run["revision"], revision.to_string());
    assert_eq!(clean_run["dirty"], false);
}

// {test}`requirements.test-coverage.failures`
// {test}`requirements.test-coverage.storage.outcomes`
// {test}`requirements.test-coverage.processes.reports`
// {test}`requirements.test-coverage.failures.profiles`
// {test}`requirements.test-coverage.test`
#[cfg(unix)]
#[test]
fn passing_tests_with_missing_or_corrupt_profiles_fail_collection_and_retain_partial_reports() {
    let temp = fixture();
    let root = temp.path();
    fs::write(root.join("tests/processes.rs"), "").unwrap();
    fs::write(root.join("src/lib.rs"), r#"
#[test] fn good_profile() { assert_eq!(std::hint::black_box(1), 1); }
unsafe extern "C" { fn _exit(status: i32) -> !; }
#[test] fn missing_profile() { unsafe { _exit(0) } }
#[test] fn corrupt_profile() {
    let template = std::path::PathBuf::from(std::env::var("LLVM_PROFILE_FILE").unwrap());
    std::fs::write(template.parent().unwrap().join(format!("{}-corrupt.profraw", std::process::id())), "not an LLVM profile").unwrap();
}
"#).unwrap();
    let output = coverage(root, &["--workspace"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("partial profiles and reports have been retained")
    );
    let combined = full_flow(root);
    assert!(!combined.status.success());
    assert!(!String::from_utf8_lossy(&combined.stdout).contains("Coverage check"));
    assert!(
        String::from_utf8_lossy(&combined.stderr)
            .contains("partial profiles and reports have been retained")
    );
    let saved = runs(root);
    assert_eq!(saved[0].1["outcome"], "failed");
    let records = attempts(&saved[0].0);
    assert_eq!(records.len(), 3);
    assert!(
        records
            .iter()
            .all(|attempt| attempt["outcome"] == "success"),
        "test execution itself succeeds: {records:?}"
    );
    let missing = records
        .iter()
        .find(|attempt| attempt["test"] == "missing_profile")
        .unwrap();
    assert!(
        missing["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| diagnostic
                .as_str()
                .unwrap()
                .contains("No LLVM profile was written"))
    );
    let corrupt = records
        .iter()
        .find(|attempt| attempt["test"] == "corrupt_profile")
        .unwrap();
    assert!(
        corrupt["processes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|process| !process["diagnostic"].is_null()
                && process["lines"].as_object().unwrap().is_empty())
    );
    let good = records
        .iter()
        .find(|attempt| attempt["test"] == "good_profile")
        .unwrap();
    assert!(
        !good["processes"][0]["lines"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    for attempt in [&good, &corrupt] {
        assert_profiles_present(&saved[0].0, attempt);
        for profile in attempt["processes"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|process| process["profiles"].as_array().unwrap())
        {
            assert!(
                saved[0]
                    .0
                    .join("attempts")
                    .join(attempt["id"].as_str().unwrap())
                    .join(profile.as_str().unwrap())
                    .is_file()
            );
        }
    }
}

// {test}`requirements.test-coverage.storage.outcomes`
// {test}`requirements.test-coverage.failures`
#[test]
fn a_process_that_cannot_start_is_failed_with_retained_attempt_metadata() {
    let temp = fixture();
    let root = temp.path();
    assert_success(&coverage(root, &["-E", "test(second)"]));
    let run = runs(root)[0].0.clone();
    let output = Command::new(env!("CARGO_BIN_EXE_habeas"))
        .arg("coverage-process")
        .arg(root.join("nonexistent-test-program"))
        .env("HABEAS_COVERAGE_RUN", &run)
        .env("NEXTEST_BINARY_ID", "coverage-fixture")
        .env("NEXTEST_TEST_NAME", "second")
        .env("NEXTEST_ATTEMPT", "2")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let records = attempts(&run);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["outcome"], "failed");
    assert_eq!(records[0]["attempt"], "2");
    assert!(records[0]["processes"].as_array().unwrap().is_empty());
    assert!(
        records[0]["diagnostics"][0]
            .as_str()
            .unwrap()
            .contains("Cannot start test process")
    );
}

// {test}`requirements.test-coverage.storage.outcomes`
// {test}`requirements.test-coverage.storage.provenance`
// {test}`requirements.test-coverage.storage.selection`
// {test}`requirements.test-coverage.failures`
#[cfg(unix)]
#[test]
fn unreadable_build_inputs_fail_preparation_before_any_tests_start() {
    let temp = fixture();
    let root = temp.path();
    assert_success(&coverage(root, &["-E", "test(second)"]));
    let before = runs(root);
    let measurements = attempts(&before[0].0);
    // An uncompiled Rust source is hashed as bytes before compilation, then
    // read as text during source capture. Invalid UTF-8 reaches that stage
    // and fails without relying on Unix permission bits.
    let input = root.join("src/uncompiled.rs");
    fs::write(&input, [0xff]).unwrap();
    assert_eq!(
        fs::read_to_string(&input).unwrap_err().kind(),
        std::io::ErrorKind::InvalidData
    );
    let output = coverage(root, &["--features", "alpha", "-E", "test(second)"]);
    assert!(
        !output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("Coverage preparation failed"));
    assert_eq!(runs(root), before);
    assert_eq!(attempts(&before[0].0), measurements);
}

// {test}`requirements.test-coverage.storage.outcomes`
// {test}`requirements.test-coverage.storage.replace`
// {test}`requirements.test-coverage.failures`
#[cfg(unix)]
#[test]
fn interrupting_the_parent_preserves_started_attempt_metadata() {
    let temp = fixture();
    let root = temp.path();
    fs::write(
        root.join("src/lib.rs"),
        r#"
#[test] fn waits() {
    if std::env::var_os("HABEAS_WAIT_FIXTURE").is_none() { return; }
    std::fs::write("started", "ready").unwrap();
    loop { std::thread::sleep(std::time::Duration::from_millis(20)); }
}
#[test] fn unstarted() { std::fs::write("unstarted", "ran").unwrap(); }
"#,
    )
    .unwrap();
    fs::write(root.join("tests/processes.rs"), "").unwrap();
    let config = root.join(".config/nextest.toml");
    let source = fs::read_to_string(&config).unwrap();
    fs::write(
        &config,
        format!("{source}\n[[profile.default.overrides]]\nfilter='test(waits)'\npriority=100\n"),
    )
    .unwrap();
    assert_success(&coverage(root, &["--workspace", "-j1"]));
    let old = runs(root)[0].0.clone();
    assert_eq!(attempts(&old).len(), 2);
    fs::remove_file(root.join("unstarted")).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_habeas"))
        .args(["coverage", "--", "--workspace", "-j1"])
        .current_dir(root)
        .env("CARGO_NET_OFFLINE", "true")
        .env("HABEAS_WAIT_FIXTURE", "1")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("HABEAS_COVERAGE_RUN")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while !root.join("started").exists() && std::time::Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("coverage exited before the test started: {status}");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    if !root.join("started").exists() {
        child.kill().unwrap();
        child.wait().unwrap();
        panic!("test did not start before the readiness deadline");
    }
    assert!(
        Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("coverage did not finish after interruption");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    assert_eq!(status.code(), Some(130));
    let saved = runs(root);
    assert_eq!(saved.len(), 1);
    assert!(!old.exists());
    assert_eq!(saved[0].1["outcome"], "interrupted");
    let records = attempts(&saved[0].0);
    assert_eq!(records.len(), 2);
    let started = records
        .iter()
        .find(|record| record["test"] == "waits")
        .unwrap();
    assert_eq!(started["outcome"], "interrupted");
    let pending = records
        .iter()
        .find(|record| record["test"] == "unstarted")
        .unwrap();
    assert_eq!(pending["outcome"], "pending");
    assert_eq!(pending["pid"], 0);
    assert!(pending["processes"].as_array().unwrap().is_empty());
    assert!(!root.join("unstarted").exists());
    let result = saved_check(root);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stdout).contains("pending"));
    assert!(!String::from_utf8_lossy(&result.stdout).contains("100.0%"));
}

// {test}`requirements.test-coverage.storage.replace`
// {test}`requirements.test-coverage.storage.outcomes`
// {test}`requirements.test-coverage.failures`
#[cfg(unix)]
#[test]
fn execution_startup_failure_leaves_prior_selected_results_pending_without_measurements() {
    use std::os::unix::{fs::PermissionsExt, fs::symlink};
    let temp = fixture();
    let root = temp.path();
    assert_success(&coverage(root, &["-E", "test(second)"]));
    let old = runs(root)[0].0.clone();
    assert!(
        !attempts(&old)[0]["processes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let tools = root.join("disappearing-cargo");
    fs::create_dir(&tools).unwrap();
    let cargo = tools.join("cargo");
    fs::write(
        &cargo,
        r#"#!/bin/sh
if [ "$1" = nextest ] && [ "$2" = list ]; then
  for arg in "$@"; do
    if [ "$arg" = --binaries-metadata ]; then
      "$HABEAS_REAL_CARGO" "$@"
      result=$?
      /bin/rm "$0"
      exit "$result"
    fi
  done
fi
exec "$HABEAS_REAL_CARGO" "$@"
"#,
    )
    .unwrap();
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
    let rustc = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|path| path.join("rustc"))
        .find(|path| path.is_file())
        .unwrap();
    let nextest = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|path| path.join("cargo-nextest"))
        .find(|path| path.is_file())
        .unwrap();
    symlink(rustc, tools.join("rustc")).unwrap();
    symlink(nextest, tools.join("cargo-nextest")).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_habeas"))
        .args(["coverage", "--", "-E", "test(second)"])
        .current_dir(root)
        .env("HABEAS_REAL_CARGO", env!("CARGO"))
        .env("CARGO_NET_OFFLINE", "true")
        .env(
            "PATH",
            std::env::join_paths([tools.as_path(), Path::new("/usr/bin"), Path::new("/bin")])
                .unwrap(),
        )
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("HABEAS_COVERAGE_RUN")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(
        !cargo.exists(),
        "selection discovery succeeded before removing the executable"
    );
    let saved = runs(root);
    assert_eq!(saved.len(), 1);
    assert!(!old.exists());
    assert_eq!(saved[0].1["outcome"], "failed");
    assert!(
        saved[0].1["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message.as_str().unwrap().contains("No such file"))
    );
    let records = attempts(&saved[0].0);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["test"], "second");
    assert_eq!(records[0]["outcome"], "pending");
    assert_eq!(records[0]["pid"], 0);
    assert!(records[0]["processes"].as_array().unwrap().is_empty());
    assert_eq!(
        fs::read_dir(saved[0].0.join("attempts")).unwrap().count(),
        0
    );
    let result = saved_check(root);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stdout).contains("pending"));
    assert!(!String::from_utf8_lossy(&result.stdout).contains("100.0%"));
}

// {test}`requirements.test-coverage.storage.replace`
// {test}`requirements.test-coverage.storage.configuration_reset`
// {test}`requirements.test-coverage.failures`
#[cfg(unix)]
#[test]
fn a_failed_replacement_commit_reports_the_error_without_launching_execution() {
    use std::os::unix::fs::PermissionsExt;
    let temp = fixture();
    let root = temp.path();
    assert_success(&coverage(root, &["-E", "test(second)"]));
    let before = runs(root);
    let measurements = attempts(&before[0].0);
    let database =
        rusqlite::Connection::open(root.join("target/habeas/coverage/current.sqlite")).unwrap();
    database.execute_batch("CREATE TRIGGER reject_replacement BEFORE INSERT ON tests BEGIN SELECT RAISE(ABORT,'synthetic commit failure'); END;").unwrap();
    drop(database);
    let tools = root.join("execution-sentinel");
    fs::create_dir(&tools).unwrap();
    let cargo = tools.join("cargo");
    fs::write(
        &cargo,
        r#"#!/bin/sh
if [ "$1" = nextest ] && [ "$2" = run ]; then
  for arg in "$@"; do
    if [ "$arg" = --binaries-metadata ]; then
      echo started > execution-started
    fi
  done
fi
exec "$HABEAS_REAL_CARGO" "$@"
"#,
    )
    .unwrap();
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
    let mut paths = vec![tools];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
    let result = Command::new(env!("CARGO_BIN_EXE_habeas"))
        .args(["coverage", "--", "--workspace", "--features", "alpha"])
        .current_dir(root)
        .env("PATH", std::env::join_paths(paths).unwrap())
        .env("HABEAS_REAL_CARGO", env!("CARGO"))
        .env("CARGO_NET_OFFLINE", "true")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("HABEAS_COVERAGE_RUN")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("synthetic commit failure"));
    assert!(
        !root.join("execution-started").exists(),
        "no execution command may start after the failed commit"
    );
    assert_eq!(runs(root), before);
    assert_eq!(attempts(&before[0].0), measurements);
    assert_profiles_present(&before[0].0, &measurements[0]);
}

// {test}`requirements.test-coverage.failures.prerequisites`
#[cfg(unix)]
#[test]
fn unavailable_and_unsupported_tools_explain_installation() {
    use std::os::unix::fs::PermissionsExt;
    let temp = fixture();
    let root = temp.path();
    let bin = root.join("fake-tools");
    fs::create_dir(&bin).unwrap();
    let invoke = || {
        Command::new(env!("CARGO_BIN_EXE_habeas"))
            .arg("coverage")
            .current_dir(root)
            .env("PATH", &bin)
            .env_remove("RUSTC")
            .env_remove("CARGO_BUILD_RUSTC")
            .output()
            .unwrap()
    };
    let missing = invoke();
    assert!(!missing.status.success());
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("cargo install cargo-nextest --locked")
    );
    fs::write(
        bin.join("cargo"),
        "#!/bin/sh\nprintf 'cargo-nextest 0.9.115\\n'\n",
    )
    .unwrap();
    fs::set_permissions(bin.join("cargo"), fs::Permissions::from_mode(0o755)).unwrap();
    let old = invoke();
    assert!(!old.status.success());
    let diagnostic = String::from_utf8_lossy(&old.stderr);
    assert!(diagnostic.contains(">= 0.9.116"));
    assert!(diagnostic.contains("cargo install cargo-nextest --locked"));
    fs::write(
        bin.join("cargo"),
        "#!/bin/sh\nprintf 'cargo-nextest 0.9.116\\n'\n",
    )
    .unwrap();
    fs::write(bin.join("rustc"), format!("#!/bin/sh\nif [ \"$1\" = '-vV' ]; then\nprintf 'host: missing-host\\nLLVM version: 22.1.6\\n'\nelse\nprintf '{}\\n'\nfi\n", root.display())).unwrap();
    fs::set_permissions(bin.join("rustc"), fs::Permissions::from_mode(0o755)).unwrap();
    let llvm = invoke();
    assert!(!llvm.status.success());
    assert!(
        String::from_utf8_lossy(&llvm.stderr).contains("rustup component add llvm-tools-preview")
    );
    let llvm_bin = root.join("lib/rustlib/missing-host/bin");
    fs::create_dir_all(&llvm_bin).unwrap();
    for tool in ["llvm-cov", "llvm-profdata", "llvm-objcopy", "llvm-readobj"] {
        fs::write(
            llvm_bin.join(tool),
            "#!/bin/sh\nprintf 'LLVM version 21.0.0\\n'\n",
        )
        .unwrap();
        fs::set_permissions(llvm_bin.join(tool), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mismatched = invoke();
    assert!(!mismatched.status.success());
    let diagnostic = String::from_utf8_lossy(&mismatched.stderr);
    assert!(
        diagnostic.contains("must match rustc's LLVM 22.1.6")
            && diagnostic.contains("rustup component add llvm-tools-preview"),
        "{diagnostic}"
    );
    assert!(!root.join("target/habeas/coverage").exists());
}

// {test}`requirements.test-coverage.run.configuration`
// {test}`requirements.test-coverage.setup.validation`
// {test}`requirements.test-coverage.setup.validation.static`
#[test]
fn invalid_effective_collector_fails_before_any_tests_or_coverage_run() {
    let temp = fixture();
    let root = temp.path();
    habeas::execution_coverage::validate_setup(root).unwrap(); // no completed run required
    let path = root.join(".config/nextest.toml");
    let mut source = fs::read_to_string(&path).unwrap();
    source.push_str("\n[scripts.wrapper.custom]\ncommand = ['false']\ntarget-runner = 'within-wrapper'\n\n[[profile.fast.scripts]]\nfilter = 'test(second)'\nrun-wrapper = 'custom'\n");
    fs::write(&path, &source).unwrap();
    let failed = coverage(root, &["--profile", "fast"]);
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("profile.fast.scripts"));
    assert!(!root.join("target/habeas/coverage").exists());
    let alternate = root.join("alternate-nextest.toml");
    fs::write(
        &alternate,
        source.replace("coverage-process", "wrong-command"),
    )
    .unwrap();
    let selected = coverage(root, &["--config-file", "alternate-nextest.toml"]);
    assert!(!selected.status.success());
    assert!(String::from_utf8_lossy(&selected.stderr).contains("alternate-nextest.toml"));
    let selected = Command::new(env!("CARGO_BIN_EXE_habeas"))
        .arg("coverage")
        .current_dir(root)
        .env("NEXTEST_PROFILE", "fast")
        .output()
        .unwrap();
    assert!(!selected.status.success());
    assert!(String::from_utf8_lossy(&selected.stderr).contains("profile.fast.scripts"));
    for args in [vec!["--profile=fast"], vec!["-Pfast"], vec!["-P", "fast"]] {
        let selected = coverage(root, &args);
        assert!(!selected.status.success());
        assert!(String::from_utf8_lossy(&selected.stderr).contains("profile.fast.scripts"));
    }
    let selected = Command::new(env!("CARGO_BIN_EXE_habeas"))
        .args(["coverage", "--", "--profile", "fast"])
        .current_dir(root)
        .env("NEXTEST_PROFILE", "default")
        .output()
        .unwrap();
    assert!(!selected.status.success());
    assert!(String::from_utf8_lossy(&selected.stderr).contains("profile.fast.scripts"));
    for flag in [
        "--tool-config-file",
        "--archive-file",
        "--binaries-metadata",
        "--cargo-metadata",
        "--target-dir-remap",
        "--build-dir-remap",
    ] {
        let failed = coverage(root, &[flag, "unverified-input"]);
        assert!(!failed.status.success(), "{flag}");
        let diagnostic = String::from_utf8_lossy(&failed.stderr);
        assert!(
            diagnostic.contains(flag) && diagnostic.contains("cannot be proven"),
            "{flag}: {diagnostic}"
        );
    }
    assert!(!root.join("target/habeas/coverage").exists());
    // The default integration remains valid, while the effective fast profile is not.
    habeas::execution_coverage::validate_setup(root).unwrap();
    let selected = Command::new(env!("CARGO_BIN_EXE_habeas"))
        .args(["coverage", "--", "--profile", "default", "--no-run"])
        .current_dir(root)
        .env("NEXTEST_PROFILE", "fast")
        .env("CARGO_NET_OFFLINE", "true")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .output()
        .unwrap();
    assert_success(&selected);
    assert!(String::from_utf8_lossy(&selected.stderr).contains("Compilation-only run"));
    fs::write(&path, source.replace("coverage-process", "wrong-command")).unwrap();
    let errors = habeas::validate_repository(root).unwrap().errors.join("\n");
    assert!(
        errors.contains(".config/nextest.toml")
            && errors.contains("habeas setup")
            && errors.contains("scripts.wrapper.habeas-coverage")
    );
}

// {test}`requirements.test-coverage.setup.validation`
// {test}`requirements.test-coverage.setup.validation.static`
#[test]
fn missing_unreadable_malformed_or_disabled_collector_is_a_shared_validation_error() {
    let temp = fixture();
    let root = temp.path();
    let path = root.join(".config/nextest.toml");
    let source = fs::read_to_string(&path).unwrap();
    let assert_error = || {
        let errors = habeas::validate_repository(root).unwrap().errors.join("\n");
        assert!(
            errors.contains(".config/nextest.toml") && errors.contains("habeas setup"),
            "{errors}"
        );
        assert!(!root.join("target/habeas/coverage").exists());
    };
    fs::remove_file(&path).unwrap();
    assert_error();
    fs::create_dir(&path).unwrap(); // the file cannot be read as text
    assert_error();
    fs::remove_dir(&path).unwrap();
    fs::write(&path, "[invalid").unwrap();
    assert_error();
    fs::write(
        &path,
        source
            .replace("experimental = ['wrapper-scripts']", "experimental = []")
            .replace("experimental = [\"wrapper-scripts\"]", "experimental = []"),
    )
    .unwrap();
    assert_error();
    let mut missing_wrapper = source.parse::<toml::Table>().unwrap();
    missing_wrapper["scripts"]["wrapper"]
        .as_table_mut()
        .unwrap()
        .remove("habeas-coverage");
    fs::write(&path, toml::to_string(&missing_wrapper).unwrap()).unwrap();
    assert_error();
    let mut missing_rule = source.parse::<toml::Table>().unwrap();
    missing_rule["profile"]["default"]
        .as_table_mut()
        .unwrap()
        .remove("scripts");
    fs::write(&path, toml::to_string(&missing_rule).unwrap()).unwrap();
    assert_error();
    fs::write(&path, source).unwrap();
    habeas::execution_coverage::validate_setup(root).unwrap();
}

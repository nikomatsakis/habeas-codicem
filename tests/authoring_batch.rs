use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tempfile::TempDir;

// {cite}`habeas:util`
#[derive(Default)]
struct Requests {
    subjects: Vec<String>,
    active: usize,
    peak: usize,
    first_pair_overlapped: bool,
    /// Each request's subject with when it arrived and when it was answered.
    timeline: Vec<(String, Instant, Option<Instant>)>,
}

// {cite}`habeas:util`
#[derive(Clone, Copy)]
enum Behavior {
    Judgments {
        failing_subject: Option<&'static str>,
    },
    DeniedRead {
        subject: &'static str,
        hold_other_requests: bool,
    },
    #[cfg_attr(not(unix), allow(dead_code))]
    MixedJudgments,
    #[cfg_attr(not(unix), allow(dead_code))]
    WaitAfterFirst,
    /// The subject's first request is throttled with HTTP 429.
    ThrottledOnce { subject: &'static str },
    /// A judgment is false when the prompt shows a closed issue.
    ClosedIssueIsFalse,
}

// {cite}`habeas:util`
struct Server {
    endpoint: String,
    requests: Arc<(Mutex<Requests>, Condvar)>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    // {cite}`habeas:util`
    fn start(require_overlap: bool, failing_subject: Option<&'static str>) -> Self {
        Self::start_with(require_overlap, Behavior::Judgments { failing_subject })
    }

    // {cite}`habeas:util`
    fn start_with(require_overlap: bool, behavior: Behavior) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new((Mutex::new(Requests::default()), Condvar::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_requests = requests.clone();
        let worker_stop = stop.clone();
        let thread = thread::spawn(move || {
            let mut workers = Vec::new();
            while !worker_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let requests = worker_requests.clone();
                        let stop = worker_stop.clone();
                        workers.push(thread::spawn(move || {
                            serve(stream, requests, require_overlap, behavior, stop);
                        }));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("cannot accept mock request: {error}"),
                }
            }
            for worker in workers {
                worker.join().unwrap();
            }
        });
        Self {
            endpoint,
            requests,
            stop,
            thread: Some(thread),
        }
    }

    // {cite}`habeas:util`
    fn subjects(&self) -> Vec<String> {
        self.requests.0.lock().unwrap().subjects.clone()
    }

    /// Each request's subject, arrival, and answer, in arrival order.
    // {cite}`habeas:util`
    fn timeline(&self) -> Vec<(String, Instant, Option<Instant>)> {
        self.requests.0.lock().unwrap().timeline.clone()
    }
}

impl Drop for Server {
    // {test}`requirements.authoring.concurrency`
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}

// {cite}`habeas:util`
fn serve(
    mut stream: TcpStream,
    requests: Arc<(Mutex<Requests>, Condvar)>,
    require_overlap: bool,
    behavior: Behavior,
    stop: Arc<AtomicBool>,
) {
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut reader = BufReader::new(&mut stream);
    let mut length = None;
    loop {
        let mut line = String::new();
        // A client cancelled by an interruption may close a connection it
        // opened before sending a request; that connection is abandoned.
        if !matches!(reader.read_line(&mut line), Ok(read) if read > 0) {
            return;
        }
        if line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = Some(value.trim().parse::<usize>().unwrap());
        }
    }
    let mut bytes = vec![0; length.expect("request content length")];
    reader.read_exact(&mut bytes).unwrap();
    let request: Value = serde_json::from_slice(&bytes).unwrap();
    // The subject message follows the system prompt.
    let prompt = request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["role"] == "user")
        .and_then(|message| message["content"].as_str())
        .unwrap();
    let subject = prompt
        .lines()
        .find_map(|line| {
            line.strip_prefix("Subject: ")
                .or_else(|| {
                    // A citation review names its item before its citations.
                    line.strip_prefix("Item under review: ")
                        .map(|rest| rest.split(" Citations to check:").next().unwrap())
                })
                .or_else(|| {
                    // A site review names its site before its requirement.
                    line.strip_prefix("Site under review: ")
                        .map(|rest| rest.split(" Requirement:").next().unwrap())
                })
        })
        .expect("assessment subject")
        .to_owned();

    let (lock, arrived) = &*requests;
    let mut state = lock.lock().unwrap();
    let first_for_subject = !state.subjects.contains(&subject);
    state.subjects.push(subject.clone());
    let position = state.timeline.len();
    state.timeline.push((subject.clone(), Instant::now(), None));
    let first_request = state.subjects.len() == 1;
    state.active += 1;
    state.peak = state.peak.max(state.active);
    arrived.notify_all();
    if require_overlap && state.subjects.len() == 1 {
        let (next, _) = arrived
            .wait_timeout_while(state, Duration::from_secs(3), |state| {
                state.subjects.len() < 2
            })
            .unwrap();
        state = next;
        state.first_pair_overlapped = state.subjects.len() >= 2;
    }
    drop(state);
    // The denied subject's first request reads a denied file; once the
    // denial comes back as a tool error, it submits like the others.
    let denied = first_for_subject
        && matches!(behavior, Behavior::DeniedRead { subject: denied, .. } if subject == denied);
    let wait_for_interrupt = matches!(behavior, Behavior::WaitAfterFirst) && !first_request;
    let held = wait_for_interrupt
        || (!denied
            && matches!(
                behavior,
                Behavior::DeniedRead {
                    hold_other_requests: true,
                    ..
                }
            ));
    if wait_for_interrupt {
        while !stop.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(10));
        }
    } else if held {
        // Keep the second assessment in flight until the denied assessment has
        // failed and a queued assessment takes its slot.
        let mut state = lock.lock().unwrap();
        while state.subjects.len() < 3 && !stop.load(Ordering::Relaxed) {
            let (next, _) = arrived
                .wait_timeout(state, Duration::from_millis(50))
                .unwrap();
            state = next;
        }
    }
    // Keep requests in flight long enough for an incorrectly unbounded batch to
    // exceed its configured concurrency, without assuming task launch order.
    thread::sleep(Duration::from_millis(40));
    let failed = matches!(behavior, Behavior::Judgments { failing_subject: Some(name) } if subject == name)
        || (matches!(behavior, Behavior::MixedJudgments)
            && subject == "example requirements.alpha_case");
    let determination = match (behavior, subject.as_str()) {
        (Behavior::MixedJudgments, "requirement requirements.alpha") => json!(false),
        (Behavior::MixedJudgments, "requirement requirements.beta") => json!({"FIXME": ["#123"]}),
        (Behavior::ClosedIssueIsFalse, _) if prompt.contains("\"state\": \"closed\"") => {
            json!(false)
        }
        _ => json!(true),
    };
    let rationale = if matches!(behavior, Behavior::MixedJudgments)
        && subject == "requirement requirements.alpha"
    {
        "Fixture false rationale:\n\n- The implementation omits a boundary case.\n- Add an assertion for that case.".to_owned()
    } else {
        format!(
            "Fixture judgment for {subject}; src/lib.rs supplies the implementation and test inputs."
        )
    };
    let throttled = first_for_subject
        && matches!(behavior, Behavior::ThrottledOnce { subject: name } if subject == name);
    let response = if throttled {
        json!({"error": "fixture provider is throttling"})
    } else if failed {
        json!({"error": "fixture model rejected this subject"})
    } else if denied {
        json!({
            "model":"fixture-model", "created_at":"2026-01-01T00:00:00Z",
            "message":{"role":"assistant", "content":"", "tool_calls":[{
                "id":"denied-read", "type":"function", "function":{
                    "name":"habeas", "arguments":{"file":"evidence/private.txt", "line_start":1}
                }
            }]},
            "done":true, "done_reason":"stop", "prompt_eval_count":11, "eval_count":7
        })
    } else {
        let mut arguments = json!({"determination": determination, "rationale": rationale});
        if prompt.contains("Site under review: ") {
            arguments["applies"] = json!(true);
        }
        json!({
            "model":"fixture-model", "created_at":"2026-01-01T00:00:00Z",
            "message":{"role":"assistant", "content":"", "tool_calls":[{
                "id":"judgment", "type":"function", "function":{
                    "name":"submit_judgment", "arguments":arguments
                }
            }]},
            "done":true, "done_reason":"stop", "prompt_eval_count":11, "eval_count":7
        })
    };
    let body = response.to_string();
    let (status, retry_after) = if throttled {
        ("429 Too Many Requests", "Retry-After: 0\r\n")
    } else if failed {
        ("400 Bad Request", "")
    } else {
        ("200 OK", "")
    };
    let mut state = lock.lock().unwrap();
    let sent = write!(
        stream,
        "HTTP/1.1 {status}\r\n{retry_after}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    if !held {
        sent.unwrap();
    }
    state.timeline[position].2 = Some(Instant::now());
    state.active -= 1;
}

// {cite}`habeas:util`
struct Fixture {
    _temp: TempDir,
    project: PathBuf,
    config_root: PathBuf,
    state: PathBuf,
    config: PathBuf,
    config_text: String,
}

impl Fixture {
    // {cite}`habeas:util`
    fn new(endpoint: &str, concurrency: Option<usize>) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        let config_root = temp.path().join("config");
        let state = temp.path().join("state");
        for directory in [
            project.join("md"),
            project.join("src"),
            config_root.join("habeas"),
        ] {
            fs::create_dir_all(directory).unwrap();
        }
        fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = 'batch-fixture'\nversion = '0.1.0'\nedition = '2024'\n",
        )
        .unwrap();
        fs::write(project.join("book.toml"), "[book]\nsrc = 'md'\n").unwrap();
        fs::write(
            project.join("md/SUMMARY.md"),
            "# Summary\n\n- [Requirements](requirements.md)\n",
        )
        .unwrap();
        fs::write(project.join("md/requirements.md"), ":::{req} alpha\nalpha MUST return one.\n:::\n\n:::{req} beta\nbeta MUST return two.\n:::\n\n:::{example} alpha_case\nCalling alpha returns one. {cite}`requirements.alpha`\n:::\n\n:::{req} delta\ndelta MUST return four.\n:::\n\n:::{req} gamma\ngamma MUST return three.\n:::\n\n:::{example} gamma_case\nCalling gamma returns three. {cite}`requirements.gamma`\n:::\n").unwrap();
        fs::write(project.join("src/lib.rs"), "// {impl}`requirements.alpha`\npub fn alpha() -> u8 { 1 }\n\n// {impl}`requirements.beta`\npub fn beta() -> u8 { 2 }\n\n// {test}`requirements.alpha_case`\n#[test]\nfn alpha_case() { assert_eq!(alpha(), 1); }\n\n// {impl}`requirements.gamma`\npub fn gamma() -> u8 { 3 }\n\n// {impl}`requirements.delta`\npub fn delta() -> u8 { 4 }\n\n// {test}`requirements.gamma_case`\n#[test]\nfn gamma_case() { assert_eq!(gamma(), 3); }\n").unwrap();
        let mut config_text = format!(
            "version = 1\n[authoring]\ndefault = 'mock'\n[authoring.configs.mock]\npolicy = 'once'\nbackend = 'ollama'\nmodel = 'fixture-model'\nendpoint = '{endpoint}'\n"
        );
        if let Some(concurrency) = concurrency {
            config_text.push_str(&format!("concurrency = {concurrency}\n"));
        }
        let config = config_root.join("habeas/config.toml");
        fs::write(&config, &config_text).unwrap();
        Self {
            _temp: temp,
            project,
            config_root,
            state,
            config,
            config_text,
        }
    }

    // {cite}`habeas:util`
    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }

    // {cite}`habeas:util`
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_habeas"));
        command
            .current_dir(&self.project)
            .env("XDG_CONFIG_HOME", &self.config_root)
            .env("APPDATA", &self.config_root)
            .env("XDG_STATE_HOME", &self.state)
            .env("LOCALAPPDATA", &self.state)
            // No test contacts GitHub: nothing listens on this port, so
            // issues are unreachable and count as open.
            .env("HABEAS_GITHUB_API_URL", "http://127.0.0.1:9");
        command
    }

    // {cite}`habeas:util`
    #[cfg(unix)]
    fn run_in_pty(&self, interrupt: bool, term: &str) -> Output {
        self.run_in_pty_with_args(interrupt, term, &["author", "--all"])
    }

    // {cite}`habeas:util`
    #[cfg(unix)]
    fn run_in_pty_with_args(&self, interrupt: bool, term: &str, args: &[&str]) -> Output {
        // Keep stdout piped: the bar belongs to stderr and must detect that
        // terminal independently. Python's stdlib provides a portable Unix PTY.
        let script = r#"
import errno
import fcntl
import os
import pty
import re
import select
import signal
import struct
import subprocess
import sys
import termios
import time

master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 24, 160, 0, 0))
process = subprocess.Popen(
    [sys.argv[1], *sys.argv[3:]],
    stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=slave,
)
os.close(slave)
captured = bytearray()
interrupted = False
deadline = time.monotonic() + 20
try:
    while True:
        if time.monotonic() >= deadline:
            raise RuntimeError('habeas PTY test did not finish within 20 seconds')
        readable, _, _ = select.select([master], [], [], 0.1)
        if not readable:
            continue
        try:
            chunk = os.read(master, 65536)
        except OSError as error:
            if error.errno == errno.EIO:
                break
            raise
        if not chunk:
            break
        captured.extend(chunk)
        plain = re.sub(rb'\x1b\[[0-?]*[ -/]*[@-~]', b'', captured)
        if sys.argv[2] == 'interrupt' and not interrupted and re.search(rb'Authoring[^\r\n]*\b[1-5]/6 finished\b', plain):
            process.send_signal(signal.SIGINT)
            interrupted = True
    stdout, _ = process.communicate(timeout=5)
finally:
    os.close(master)
    if process.poll() is None:
        process.kill()
        process.wait()
sys.stdout.buffer.write(stdout)
sys.stderr.buffer.write(captured)
if sys.argv[2] == 'interrupt' and not interrupted:
    raise RuntimeError('no partial progress frame was shown before completion')
raise SystemExit(process.returncode if process.returncode >= 0 else 128 - process.returncode)
"#;
        Command::new("python3")
            .arg("-c")
            .arg(script)
            .arg(env!("CARGO_BIN_EXE_habeas"))
            .arg(if interrupt { "interrupt" } else { "complete" })
            .args(args)
            .current_dir(&self.project)
            .env("XDG_CONFIG_HOME", &self.config_root)
            .env("APPDATA", &self.config_root)
            .env("XDG_STATE_HOME", &self.state)
            .env("LOCALAPPDATA", &self.state)
            .env("HABEAS_GITHUB_API_URL", "http://127.0.0.1:9")
            .env("TERM", term)
            .env("NO_COLOR", "1")
            .env("CLICOLOR_FORCE", "1")
            .output()
            .expect("PTY progress tests require python3 on Unix")
    }

    // {cite}`habeas:util`
    fn evidence(&self, name: &str) -> Value {
        serde_json::from_slice(
            &fs::read(self.project.join(format!("evidence/{name}.json"))).unwrap(),
        )
        .unwrap()
    }

    // {cite}`habeas:util`
    fn usage(&self) -> Vec<Value> {
        let path = self.state.join(if cfg!(windows) {
            "habeas/state/authoring/usage"
        } else {
            "habeas/authoring/usage"
        });
        fs::read_dir(path)
            .unwrap()
            .map(|entry| serde_json::from_slice(&fs::read(entry.unwrap().path()).unwrap()).unwrap())
            .collect()
    }
}

// {test}`requirements.authoring.batch`
// {test}`requirements.authoring.concurrency`
// {test}`requirements.authoring.usage_storage`
// {test}`requirements.ci.determination`
// {test}`requirements.authoring.progress`
// {test}`requirements.configuration.loading`
// {test}`requirements.authoring.configuration`
// {test}`requirements.authoring.refresh`
#[test]
fn batch_fills_evidence_preserves_current_records_and_reauthors_only_changed_inputs() {
    let server = Server::start(true, None);
    let fixture = Fixture::new(&server.endpoint, Some(2));
    let source = fixture.project.join("src/lib.rs");
    fs::write(
        &source,
        fs::read_to_string(&source).unwrap().replace(
            "pub fn beta()",
            "// :::{fixme} requirements.beta\n// :github: example/app#123\n// Beta has no test yet.\n// :::\npub fn beta()",
        ),
    )
    .unwrap();
    let output = fixture.run(&["author", "--all", "--config", "mock"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(server.subjects().len(), 6);
    let report = habeas::validate_repository(&fixture.project).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(!fixture.project.join("evidence/rust").exists());
    let mut beta = fixture.evidence("requirements.beta");
    beta["determination"] = json!({"FIXME":["example/app#123"]});
    beta["rationale"] = "The source explicitly acknowledges the missing beta test.".into();
    fs::write(
        fixture.project.join("evidence/requirements.beta.json"),
        serde_json::to_vec_pretty(&beta).unwrap(),
    )
    .unwrap();
    let example = fixture.evidence("requirements.alpha_case");
    let gamma = fixture.evidence("requirements.gamma");
    let gamma_case = fixture.evidence("requirements.gamma_case");
    let usage = fixture.usage();
    assert_eq!(usage.len(), 6);
    assert!(
        usage
            .iter()
            .all(|run| run["outcome"] == "completed" && run["evidence_written"] == true)
    );
    {
        let requests = server.requests.0.lock().unwrap();
        assert_eq!(
            requests.peak, 2,
            "the configured concurrency bounds in-flight requests"
        );
        assert!(
            requests.first_pair_overlapped,
            "a second assessment must start before the first model response"
        );
    }

    fs::write(&fixture.config, "invalid TOML [").unwrap();
    let no_op = fixture.run(&["author", "--all"]);
    assert!(
        no_op.status.success(),
        "{}",
        String::from_utf8_lossy(&no_op.stderr)
    );
    fs::remove_file(&fixture.config).unwrap();
    let no_config = fixture.run(&["author", "--all"]);
    assert!(
        no_config.status.success(),
        "{}",
        String::from_utf8_lossy(&no_config.stderr)
    );
    #[cfg(unix)]
    {
        let no_op_terminal = fixture.run_in_pty(false, "xterm-256color");
        assert!(
            no_op_terminal.status.success(),
            "{}",
            String::from_utf8_lossy(&no_op_terminal.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&no_op_terminal.stdout).trim(),
            "0 authored; 6 skipped as current; 0 failed"
        );
        assert!(!String::from_utf8_lossy(&no_op_terminal.stderr).contains("0/0 finished"));
    }
    assert_eq!(server.subjects().len(), 6);
    assert_eq!(fixture.usage().len(), 6);

    fs::write(&fixture.config, &fixture.config_text).unwrap();
    let changed = fs::read_to_string(&source)
        .unwrap()
        .replace("-> u8 { 1 }", "-> u8 { 1_u8 }");
    fs::write(source, changed).unwrap();
    let updated = fixture.run(&["author", "--all"]);
    assert!(
        updated.status.success(),
        "{}",
        String::from_utf8_lossy(&updated.stderr)
    );
    assert_eq!(server.subjects().len(), 7);
    assert_eq!(
        server.subjects().last().unwrap(),
        "requirement requirements.alpha"
    );
    assert_eq!(fixture.usage().len(), 7);
    assert_eq!(
        String::from_utf8_lossy(&updated.stdout).trim(),
        "1 authored; 5 skipped as current; 0 failed; policy once"
    );
    assert_eq!(fixture.evidence("requirements.beta"), beta);
    assert_eq!(fixture.evidence("requirements.alpha_case"), example);
    assert_eq!(fixture.evidence("requirements.gamma"), gamma);
    assert_eq!(fixture.evidence("requirements.gamma_case"), gamma_case);
    let report = habeas::validate_repository(&fixture.project).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
}

// {test}`requirements.authoring.concurrency`
// {test}`requirements.authoring.batch`
#[test]
fn ollama_batches_are_serial_by_default() {
    let server = Server::start(false, None);
    let fixture = Fixture::new(&server.endpoint, None);
    let output = fixture.run(&["author", "--all"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(server.subjects().len(), 6);
    assert_eq!(server.requests.0.lock().unwrap().peak, 1);
}

// {test}`requirements.authoring.concurrency`
#[test]
fn throttled_assessment_retries_within_itself_while_others_proceed() {
    let server = Server::start_with(
        false,
        Behavior::ThrottledOnce {
            subject: "requirement requirements.alpha",
        },
    );
    let fixture = Fixture::new(&server.endpoint, Some(2));
    let output = fixture.run(&["author", "--all"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Six assessments plus one retried request, never more than two at once.
    let subjects = server.subjects();
    assert_eq!(subjects.len(), 7, "{subjects:?}");
    assert_eq!(
        subjects
            .iter()
            .filter(|subject| *subject == "requirement requirements.alpha")
            .count(),
        2
    );
    assert!(server.requests.0.lock().unwrap().peak <= 2);
    assert_eq!(
        fixture.evidence("requirements.alpha")["determination"],
        true
    );

    // The retry stays inside the throttled assessment's own conversation and
    // usage record; no other assessment is restarted or failed.
    let usage = fixture.usage();
    assert_eq!(usage.len(), 6);
    assert!(usage.iter().all(|run| run["outcome"] == "completed"));
    let alpha = usage
        .iter()
        .find(|run| run["subject"] == "requirements.alpha")
        .unwrap();
    assert_eq!(alpha["request_count"], 2);
    assert_eq!(alpha["requests"][0]["outcome"], "failed");
    assert_eq!(alpha["requests"][1]["outcome"], "completed");
    assert!(
        usage
            .iter()
            .filter(|run| run["subject"] != "requirements.alpha")
            .all(|run| run["request_count"] == 1)
    );
}

// {test}`requirements.authoring.concurrency`
// {test}`requirements.authoring.batch`
// {test}`requirements.authoring.reporting`
// {test}`requirements.authoring.usage_storage`
// {test}`requirements.authoring.progress`
#[test]
fn failed_assessment_keeps_other_results_and_reports_failure() {
    let server = Server::start(true, Some("requirement requirements.alpha"));
    let fixture = Fixture::new(&server.endpoint, Some(2));
    let output = fixture.run(&["author", "--all"]);
    assert!(!output.stderr.contains(&b'\x1b'));
    assert!(!output.stderr.contains(&b'\r'));
    assert!(!output.status.success());
    assert_eq!(server.subjects().len(), 6);
    assert!(
        !fixture
            .project
            .join("evidence/requirements.alpha.json")
            .exists()
    );
    assert_eq!(fixture.evidence("requirements.beta")["determination"], true);
    assert_eq!(
        fixture.evidence("requirements.alpha_case")["determination"],
        true
    );
    assert_eq!(
        fixture.evidence("requirements.gamma")["determination"],
        true
    );
    let usage = fixture.usage();
    assert_eq!(usage.len(), 6);
    assert_eq!(
        usage
            .iter()
            .filter(|run| run["outcome"] == "failed")
            .count(),
        1
    );
    assert_eq!(
        usage
            .iter()
            .filter(|run| run["outcome"] == "completed")
            .count(),
        5
    );
    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(diagnostics.contains("requirements.alpha"), "{diagnostics}");
    assert!(
        diagnostics.contains("fixture model rejected"),
        "{diagnostics}"
    );
}

// {test}`requirements.authoring.progress`
// {test}`requirements.authoring.persistence`
// {test}`requirements.authoring.batch`
// {test}`requirements.authoring.reporting`
#[cfg(unix)]
#[test]
fn terminal_progress_distinguishes_judgments_from_authoring_errors() {
    let server = Server::start_with(true, Behavior::MixedJudgments);
    let fixture = Fixture::new(&server.endpoint, Some(2));
    let output = fixture.run_in_pty(false, "xterm-256color");
    assert!(!output.status.success(), "{output:?}");
    assert_eq!(server.subjects().len(), 6);
    let terminal = String::from_utf8_lossy(&output.stderr);
    assert!(terminal.contains('\x1b'), "no terminal redraw: {terminal}");
    assert!(
        !regex::Regex::new(r"\x1b\[[0-9;]*m")
            .unwrap()
            .is_match(&terminal),
        "NO_COLOR must override forced color: {terminal}"
    );
    let escape = regex::Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").unwrap();
    let plain = escape.replace_all(&terminal, "");
    let counted_progress = regex::Regex::new(r"\b[0-9]+/[1-9][0-9]*\b").unwrap();
    assert!(
        plain.split(['\r', '\n']).any(|line| {
            line.contains("Inspecting evidence")
                && line.contains('%')
                && counted_progress.is_match(line)
        }),
        "inspection must show counted progress before authoring: {plain}"
    );
    assert!(plain.contains("100%"), "{plain}");
    assert!(plain.contains("6/6 finished"), "{plain}");
    assert!(plain.contains("Finished"), "{plain}");
    let final_counts = plain
        .split(['\r', '\n'])
        .rfind(|line| line.contains("true ") && line.contains("errors "))
        .expect("final outcome counts in the terminal progress display");
    assert!(
        !final_counts.contains("Error:"),
        "the final progress line must end before command diagnostics: {final_counts}"
    );
    for count in [
        "true 3", "false 1", "FIXME 1", "errors 1", "0 active", "0 queued",
    ] {
        assert!(
            final_counts.contains(count),
            "missing {count}: {final_counts}"
        );
    }
    assert!(plain.contains("fixture model rejected"), "{plain}");
    assert!(!plain.contains(": true (usage:"), "{plain}");
    assert!(
        plain.contains("requirements.alpha: false (usage:"),
        "{plain}"
    );
    assert!(!plain.contains("Fixture judgment for"), "{plain}");
    assert!(!plain.contains("Fixture false rationale"), "{plain}");
    assert!(plain.contains("requirements.beta: FIXME"), "{plain}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        "5 authored; 0 skipped as current; 1 failed; policy once"
    );
    assert_eq!(
        fixture.evidence("requirements.alpha")["determination"],
        false
    );
    assert_eq!(
        fixture.evidence("requirements.beta")["determination"],
        json!({"FIXME": ["#123"]})
    );
}

// {test}`requirements.authoring.reporting`
// {test}`requirements.authoring.progress`
// {test}`requirements.authoring.batch`
#[test]
fn batch_reports_false_rationales_only_when_verbose() {
    for verbose in [false, true] {
        let server = Server::start_with(true, Behavior::MixedJudgments);
        let fixture = Fixture::new(&server.endpoint, Some(2));
        let args = if verbose {
            vec!["author", "--all", "--verbose"]
        } else {
            vec!["author", "--all"]
        };
        let output = fixture.run(&args);
        assert!(!output.status.success(), "{output:?}");
        assert_eq!(server.subjects().len(), 6);
        let diagnostics = String::from_utf8_lossy(&output.stderr);
        assert!(!diagnostics.contains('\x1b'), "{diagnostics}");
        assert!(!diagnostics.contains(": true (usage:"), "{diagnostics}");
        assert!(
            diagnostics.contains("requirements.alpha: false (usage:"),
            "{diagnostics}"
        );
        let rationale = fixture.evidence("requirements.alpha")["rationale"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(diagnostics.contains(&rationale), verbose, "{diagnostics}");
        assert!(
            diagnostics.contains("requirements.beta: FIXME"),
            "{diagnostics}"
        );
        assert!(
            diagnostics.contains("fixture model rejected"),
            "{diagnostics}"
        );
        assert!(!diagnostics.contains("pub fn alpha()"), "{diagnostics}");
        let true_rationale = fixture.evidence("requirements.gamma")["rationale"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(!diagnostics.contains(&true_rationale), "{diagnostics}");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "5 authored; 0 skipped as current; 1 failed; policy once"
        );
    }
}

// {test}`requirements.authoring.reporting`
// {test}`requirements.authoring.progress`
#[cfg(unix)]
#[test]
fn verbose_false_rationale_is_readable_above_terminal_progress() {
    let server = Server::start_with(true, Behavior::MixedJudgments);
    let fixture = Fixture::new(&server.endpoint, Some(2));
    let output = fixture.run_in_pty_with_args(false, "xterm-256color", &["author", "--all", "-v"]);
    assert!(!output.status.success(), "{output:?}");
    assert_eq!(server.subjects().len(), 6);
    let terminal = String::from_utf8_lossy(&output.stderr);
    let escape = regex::Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").unwrap();
    let plain = escape.replace_all(&terminal, "").replace('\r', "");
    let rationale = fixture.evidence("requirements.alpha")["rationale"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(plain.contains("requirements.alpha: false"), "{plain}");
    assert!(plain.contains(&rationale), "{plain}");
    assert!(!plain.contains("pub fn alpha()"), "{plain}");
    assert!(!plain.contains(": true (usage:"), "{plain}");
    assert!(plain.contains("6/6 finished"), "{plain}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "5 authored; 0 skipped as current; 1 failed; policy once"
    );
}

// {test}`requirements.authoring.progress`
// {test}`requirements.authoring.batch`
#[cfg(unix)]
#[test]
fn dumb_terminal_keeps_plain_batch_diagnostics() {
    let server = Server::start(false, None);
    let fixture = Fixture::new(&server.endpoint, Some(2));
    let output = fixture.run_in_pty(false, "dumb");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(server.subjects().len(), 6);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains('\x1b'), "{stderr}");
    assert!(
        stderr.contains("Authoring 6 assessments under the once policy with concurrency 2"),
        "{stderr}"
    );
    assert!(!stderr.contains("6/6 finished"), "{stderr}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "6 authored; 0 skipped as current; 0 failed; policy once"
    );
}

// {test}`requirements.authoring.progress`
// {test}`requirements.authoring.usage_storage`
// {test}`requirements.authoring.batch`
// {test}`requirements.authoring.concurrency`
#[cfg(unix)]
#[test]
fn interrupted_terminal_progress_keeps_unstarted_work_incomplete() {
    // Concurrency one is the Ollama default: only one assessment is running.
    for (overlap, concurrency) in [(true, 2), (false, 1)] {
        assert_interruption_stops_the_batch(overlap, concurrency);
    }
}

// {cite}`habeas:util`
#[cfg(unix)]
fn assert_interruption_stops_the_batch(overlap: bool, concurrency: usize) {
    let server = Server::start_with(overlap, Behavior::WaitAfterFirst);
    let fixture = Fixture::new(&server.endpoint, Some(concurrency));
    let output = fixture.run_in_pty(true, "xterm-256color");
    assert!(!output.status.success(), "{output:?}");
    let terminal = String::from_utf8_lossy(&output.stderr);
    let escape = regex::Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").unwrap();
    let plain = escape.replace_all(&terminal, "");
    assert!(plain.contains("Stopped"), "{plain}");
    assert!(plain.contains("assessments not started"), "{plain}");
    let stopped = &plain[plain.rfind("Stopped").unwrap()..];
    assert!(!stopped.contains("100%"), "{stopped}");
    assert!(!stopped.contains("6/6 finished"), "{stopped}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 authored;"));
    assert!(server.subjects().len() < 6);
    let usage = fixture.usage();
    assert_eq!(
        usage
            .iter()
            .filter(|run| run["evidence_written"] == true)
            .count(),
        1
    );
    // The assessment running at the interruption is cancelled without writing.
    assert!(
        usage
            .iter()
            .any(|run| run["outcome"] == "cancelled" && run["evidence_written"] == false),
        "{usage:?}"
    );
    let written = fs::read_dir(fixture.project.join("evidence"))
        .unwrap()
        .count();
    assert_eq!(written, 1, "only the finished assessment wrote evidence");
}

// {test}`requirements.authoring.batch`
// {test}`requirements.authoring.read_denial`
// {test}`requirements.authoring.usage_storage`
#[test]
fn a_denied_read_returns_an_error_and_its_assessment_continues() {
    let server = Server::start_with(
        false,
        Behavior::DeniedRead {
            subject: "example requirements.alpha_case",
            hold_other_requests: false,
        },
    );
    let fixture = Fixture::new(&server.endpoint, Some(1));
    fs::create_dir(fixture.project.join("evidence")).unwrap();
    fs::write(
        fixture.project.join("evidence/private.txt"),
        "generated output\n",
    )
    .unwrap();
    let output = fixture.run(&["author", "--all"]);
    assert!(output.status.success(), "{output:?}");
    let subjects = server.subjects();
    // The denied assessment made a second request after the tool error.
    assert_eq!(
        subjects
            .iter()
            .filter(|subject| *subject == "example requirements.alpha_case")
            .count(),
        2,
        "{subjects:?}"
    );
    for name in [
        "requirements.alpha",
        "requirements.alpha_case",
        "requirements.beta",
        "requirements.gamma",
    ] {
        assert_eq!(fixture.evidence(name)["determination"], true, "{name}");
    }
    let usage = fixture.usage();
    assert!(
        usage.iter().all(|run| run["outcome"] == "completed"),
        "{usage:?}"
    );
    let denied = usage
        .iter()
        .find(|run| run["subject"] == "requirements.alpha_case")
        .unwrap();
    assert_eq!(denied["request_count"], 2);
    assert_eq!(denied["tool_failures"]["habeas"], 1);
    // The denied file is not recorded.
    assert!(
        !fixture
            .evidence("requirements.alpha_case")
            .to_string()
            .contains("private.txt")
    );
}

// {test}`requirements.authoring.batch`
// {test}`requirements.authoring.concurrency`
// {test}`requirements.authoring.read_denial.mandatory`
// {test}`requirements.authoring.persistence`
// {test}`requirements.authoring.usage_storage`
#[test]
fn a_denied_mandatory_input_aborts_only_its_assessment() {
    let server = Server::start_with(
        false,
        Behavior::Judgments {
            failing_subject: None,
        },
    );
    // One at a time, so the assessments after the aborted one were queued.
    let fixture = Fixture::new(&server.endpoint, Some(1));
    fs::write(fixture.project.join("private.txt"), "private\n").unwrap();
    fs::write(
        fixture.project.join("habeas.toml"),
        "version = 1\n\n[evidence]\ndisallow = [\"private.txt\"]\n",
    )
    .unwrap();
    // The workspace disallows this anchor, a mandatory input of
    // `requirements.alpha`.
    let chapter = fixture.project.join("md/requirements.md");
    let text = fs::read_to_string(&chapter).unwrap();
    fs::write(
        &chapter,
        text.replacen(
            ":::{req} alpha\n",
            ":::{req} alpha\n:impl: private.txt\n",
            1,
        ),
    )
    .unwrap();
    let output = fixture.run(&["author", "--all"]);
    assert!(!output.status.success(), "{output:?}");
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains("private.txt"), "{diagnostic}");
    // It made no request; the others ran and kept their evidence.
    let subjects = server.subjects();
    assert!(
        !subjects
            .iter()
            .any(|subject| subject == "requirement requirements.alpha"),
        "{subjects:?}"
    );
    assert!(
        !fixture
            .project
            .join("evidence/requirements.alpha.json")
            .exists()
    );
    for name in [
        "requirements.alpha_case",
        "requirements.beta",
        "requirements.gamma",
    ] {
        assert_eq!(fixture.evidence(name)["determination"], true, "{name}");
    }
    let usage = fixture.usage();
    let aborted = usage
        .iter()
        .find(|run| run["subject"] == "requirements.alpha")
        .expect("the aborted assessment has a usage record");
    assert_eq!(aborted["outcome"], "failed");
    assert_eq!(aborted["request_count"], 0);
    assert_eq!(aborted["evidence_written"], false);
    // With --stdout it emits no evidence JSON.
    let single = fixture.run(&["author", "requirements.alpha", "--stdout"]);
    assert!(!single.status.success(), "{single:?}");
    assert!(single.stdout.is_empty(), "{single:?}");
}

// {test}`requirements.authoring.read_denial.mandatory`
// {test}`requirements.authoring.read_denial.untracked`
#[test]
fn an_untracked_mandatory_input_is_reported_with_how_to_stage_it() {
    let server = Server::start_with(
        false,
        Behavior::Judgments {
            failing_subject: None,
        },
    );
    let fixture = Fixture::new(&server.endpoint, Some(1));
    let chapter = fixture.project.join("md/requirements.md");
    let text = fs::read_to_string(&chapter).unwrap();
    fs::write(
        &chapter,
        text.replacen(":::{req} alpha\n", ":::{req} alpha\n:impl: notes.txt\n", 1),
    )
    .unwrap();
    for args in [["init", "--quiet"], ["add", "."]] {
        let git = Command::new("git")
            .current_dir(&fixture.project)
            .args(args)
            .output()
            .unwrap();
        assert!(git.status.success(), "{git:?}");
    }
    fs::write(fixture.project.join("notes.txt"), "notes\n").unwrap();
    let output = fixture.run(&["author", "requirements.alpha", "--stdout"]);
    assert!(!output.status.success(), "{output:?}");
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    for text in ["notes.txt", "untracked", "git add", "allow rule"] {
        assert!(diagnostic.contains(text), "{text}: {diagnostic}");
    }
    assert!(server.subjects().is_empty());
}

// {test}`requirements.authoring.batch`
// {test}`requirements.ci.determination`
// {test}`requirements.authoring.refresh`
#[test]
fn batch_preserves_current_false_judgments_and_reauthors_stale_ones() {
    let server = Server::start(false, None);
    let fixture = Fixture::new(&server.endpoint, Some(2));
    let output = fixture.run(&["author", "--all"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(server.subjects().len(), 6);
    let mark_false = |name: &str| {
        let path = fixture.project.join(format!("evidence/{name}.json"));
        let mut record = fixture.evidence(name);
        record["determination"] = json!(false);
        record["rationale"] = json!("The implementation omits a boundary case.");
        fs::write(&path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
        fs::read(path).unwrap()
    };
    let alpha = mark_false("requirements.alpha");
    mark_false("requirements.gamma");

    let current = fixture.run(&["author", "--all"]);
    assert!(
        current.status.success(),
        "{}",
        String::from_utf8_lossy(&current.stderr)
    );
    assert_eq!(server.subjects().len(), 6, "{:?}", server.subjects());
    assert_eq!(
        String::from_utf8_lossy(&current.stdout).trim(),
        "0 authored; 6 skipped as current; 0 failed"
    );
    assert_eq!(
        fs::read(fixture.project.join("evidence/requirements.alpha.json")).unwrap(),
        alpha
    );
    assert_eq!(
        fixture.evidence("requirements.gamma")["determination"],
        false
    );

    let source = fixture.project.join("src/lib.rs");
    fs::write(
        &source,
        fs::read_to_string(&source)
            .unwrap()
            .replace("-> u8 { 3 }", "-> u8 { 3_u8 }"),
    )
    .unwrap();
    let stale = fixture.run(&["author", "--all"]);
    assert!(
        stale.status.success(),
        "{}",
        String::from_utf8_lossy(&stale.stderr)
    );
    let subjects = server.subjects();
    assert_eq!(subjects.len(), 7, "{subjects:?}");
    assert_eq!(subjects[6], "requirement requirements.gamma");
    assert_eq!(
        String::from_utf8_lossy(&stale.stdout).trim(),
        "1 authored; 5 skipped as current; 0 failed; policy once"
    );
    assert_eq!(
        fixture.evidence("requirements.gamma")["determination"],
        true
    );
    assert_eq!(
        fs::read(fixture.project.join("evidence/requirements.alpha.json")).unwrap(),
        alpha
    );
}

/// A new reference inside a node already among a record's inputs changes no
/// recorded hash, but changes the reference set, so the record is stale.
// {test}`requirements.authoring.batch`
// {test}`requirements.ci.reference_set`
// {test}`requirements.authoring.refresh`
#[test]
fn batch_reauthors_a_false_judgment_when_its_reference_set_changes() {
    let server = Server::start(false, None);
    let fixture = Fixture::new(&server.endpoint, Some(2));
    let output = fixture.run(&["author", "--all"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(server.subjects().len(), 6);
    for name in ["requirements.alpha", "requirements.delta"] {
        assert_eq!(
            fixture.evidence(name)["references"],
            habeas::reference_set_digest_for(&fixture.project, name).unwrap()
        );
    }
    let path = fixture.project.join("evidence/requirements.delta.json");
    let mut record = fixture.evidence("requirements.delta");
    record["determination"] = json!(false);
    record["rationale"] = json!("The behavior has no {test} reference.");
    fs::write(&path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();

    let source = fixture.project.join("src/lib.rs");
    fs::write(
        &source,
        fs::read_to_string(&source).unwrap().replace(
            "pub fn delta() -> u8 { 4 }",
            "pub fn delta() -> u8 {\n    // {test}`requirements.delta`\n    4\n}",
        ),
    )
    .unwrap();
    let errors = habeas::validate_repository(&fixture.project)
        .unwrap()
        .errors;
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(
        errors[0].starts_with("evidence/requirements.delta.json: determination is false")
            && errors[1].starts_with("evidence/requirements.delta.json: reference set changed"),
        "{errors:?}"
    );

    let stale = fixture.run(&["author", "--all"]);
    assert!(
        stale.status.success(),
        "{}",
        String::from_utf8_lossy(&stale.stderr)
    );
    let subjects = server.subjects();
    assert_eq!(subjects.len(), 7, "{subjects:?}");
    assert_eq!(subjects[6], "requirement requirements.delta");
    assert_eq!(
        String::from_utf8_lossy(&stale.stdout).trim(),
        "1 authored; 5 skipped as current; 0 failed; policy once"
    );
    assert_eq!(
        fixture.evidence("requirements.delta")["references"],
        habeas::reference_set_digest_for(&fixture.project, "requirements.delta").unwrap()
    );
    let report = habeas::validate_repository(&fixture.project).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
}

// {test}`requirements.references.site_review`
// {test}`requirements.authoring.batch`
// {test}`requirements.authoring.refresh`
#[test]
fn site_reviews_are_authored_by_requirement_and_orphans_deleted_by_all() {
    let server = Server::start(false, None);
    let fixture = Fixture::new(&server.endpoint, None);
    let chapter = fs::read_to_string(fixture.project.join("md/requirements.md")).unwrap();
    fs::write(
        fixture.project.join("md/requirements.md"),
        format!(
            "{chapter}\n:::::{{req}} spread\nEach public function MUST return a constant.\n\n::::{{sites}} public\n```yaml\nlanguage: rust\nrule: {{ kind: function_item, regex: \"^pub fn (alpha|beta)\" }}\nmatch: [\"pub fn alpha() {{}}\"]\nno_match: [\"fn alpha() {{}}\"]\n```\n::::\n:::::\n"
        ),
    )
    .unwrap();
    let output = fixture.run(&["author", "sites:requirements.spread"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut subjects = server.subjects();
    subjects.sort();
    assert_eq!(subjects, ["src/lib.rs fn[alpha]", "src/lib.rs fn[beta]"]);
    let review = fixture
        .project
        .join("evidence/sites/requirements.spread/src/lib.rs/fn[alpha].json");
    let record: Value = serde_json::from_str(&fs::read_to_string(&review).unwrap()).unwrap();
    assert_eq!(record["applies"], true);
    assert_eq!(record["queries"], json!(["public"]));
    // Current reviews are kept; an unknown requirement is an error.
    let output = fixture.run(&["author", "sites:requirements.spread"]);
    assert!(output.status.success());
    assert_eq!(server.subjects().len(), 2);
    // A single named site review that is current is kept under `--missing`,
    // the default, without a model or configuration.
    fs::write(&fixture.config, "not toml [").unwrap();
    let site = "sites:requirements.spread/src/lib.rs#fn[alpha]";
    for args in [&["author", site][..], &["author", site, "--missing"]] {
        let output = fixture.run(args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            format!("{site}: evidence is current; 1 skipped as current (use --force to reassess)")
        );
    }
    assert_eq!(server.subjects().len(), 2, "no model was invoked");
    fs::write(&fixture.config, &fixture.config_text).unwrap();
    let output = fixture.run(&["author", "sites:requirements.alpha"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("no distributed requirement"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // `--all` deletes a review whose site is gone, without configuration.
    let orphan = fixture
        .project
        .join("evidence/sites/requirements.spread/src/lib.rs/fn[gone].json");
    fs::write(&orphan, "{}").unwrap();
    fs::remove_file(&fixture.config).unwrap();
    let output = fixture.run(&["author", "--all"]);
    assert!(!orphan.exists());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(
            "Deleted orphaned review evidence/sites/requirements.spread/src/lib.rs/fn[gone].json"
        ),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(review.exists());
}

// {test}`requirements.authoring.batch`
#[test]
fn batch_stdout_is_rejected_before_authoring() {
    let server = Server::start(false, None);
    let fixture = Fixture::new(&server.endpoint, None);
    let output = fixture.run(&["author", "--all", "--stdout"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--stdout"));
    assert!(server.subjects().is_empty());
    assert!(!fixture.project.join("evidence").exists());
}

// {test}`requirements.authoring.batch`
// {test}`requirements.ci.definition_errors`
// {test}`requirements.references.rust_item_coverage`
#[test]
fn discovery_errors_fail_before_configuration_or_inference() {
    for (declarations, source, location, diagnostic) in [
        ("", "fn broken(", "src/lib.rs", "cannot parse Rust source"),
        (
            ":::{req} unclosed\nThis MUST be valid.\n",
            "",
            "md/requirements.md",
            "missing a closing colon fence",
        ),
    ] {
        let fixture = Fixture::new("http://127.0.0.1:9", None);
        fs::write(fixture.project.join("md/requirements.md"), declarations).unwrap();
        fs::write(fixture.project.join("src/lib.rs"), source).unwrap();
        fs::remove_file(&fixture.config).unwrap();
        let output = fixture.run(&["author", "--all"]);
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(location), "{stderr}");
        assert!(stderr.contains(diagnostic), "{stderr}");
        assert!(!stderr.contains("no authoring configuration"), "{stderr}");
        assert!(!fixture.state.exists());
        assert!(!fixture.project.join("evidence").exists());
    }
}

// {test}`requirements.authoring.batch`
#[cfg(unix)]
#[test]
fn unreadable_reference_sources_fail_the_batch_before_inference() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new("http://127.0.0.1:9", None);
    let unreadable = fixture.project.join("notes.txt");
    fs::write(&unreadable, "a reference source\n").unwrap();
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read(&unreadable).is_ok() {
        // Permission bits do not restrict this user (for example, root).
        return;
    }
    fs::remove_file(&fixture.config).unwrap();
    let output = fixture.run(&["author", "--all"]);
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("notes.txt: cannot read reference source"),
        "{stderr}"
    );
    assert!(!stderr.contains("no authoring configuration"), "{stderr}");
    assert!(!fixture.state.exists());
    assert!(!fixture.project.join("evidence").exists());
}

/// Replays terminal output onto a screen, handling the carriage returns,
/// line clears, and cursor movement that progress displays use.
// {cite}`habeas:util`
#[cfg(unix)]
fn final_screen(output: &[u8]) -> String {
    let text = String::from_utf8_lossy(output);
    let mut lines = vec![String::new()];
    let (mut row, mut column) = (0usize, 0usize);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => column = 0,
            '\n' => {
                row += 1;
                column = 0;
                if lines.len() <= row {
                    lines.push(String::new());
                }
            }
            '\x1b' if chars.peek() == Some(&'[') => {
                chars.next();
                let mut parameters = String::new();
                let mut command = ' ';
                for next in chars.by_ref() {
                    if next.is_ascii_alphabetic() {
                        command = next;
                        break;
                    }
                    parameters.push(next);
                }
                let count = parameters.parse::<usize>().unwrap_or(1);
                match command {
                    'A' => row = row.saturating_sub(count),
                    'B' => row += count,
                    'K' if parameters == "2" => lines[row].clear(),
                    'K' => lines[row] = lines[row].chars().take(column).collect(),
                    'J' => {
                        lines[row] = lines[row].chars().take(column).collect();
                        lines.truncate(row + 1);
                    }
                    _ => {}
                }
                while lines.len() <= row {
                    lines.push(String::new());
                }
            }
            c if !c.is_control() => {
                let mut cells = lines[row].chars().collect::<Vec<_>>();
                cells.resize(cells.len().max(column), ' ');
                if column < cells.len() {
                    cells[column] = c;
                } else {
                    cells.push(c);
                }
                lines[row] = cells.into_iter().collect();
                column += 1;
            }
            _ => {}
        }
    }
    lines
        .iter()
        .map(|line| line.trim_end())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

// {test}`requirements.authoring.progress`
#[cfg(unix)]
#[test]
fn empty_batches_and_early_errors_clear_the_terminal_display() {
    for case in ["empty", "configuration error", "discovery error"] {
        let fixture = Fixture::new("http://127.0.0.1:9", Some(1));
        match case {
            "empty" => {
                fs::write(fixture.project.join("md/requirements.md"), "").unwrap();
                fs::write(fixture.project.join("src/lib.rs"), "").unwrap();
            }
            "configuration error" => fs::write(&fixture.config, "invalid TOML [").unwrap(),
            _ => fs::write(fixture.project.join("src/lib.rs"), "fn broken(").unwrap(),
        }
        let output = fixture.run_in_pty(false, "xterm-256color");
        assert_eq!(
            output.status.success(),
            case == "empty",
            "{case}: {output:?}"
        );
        let raw = String::from_utf8_lossy(&output.stderr);
        assert!(
            raw.contains("Phase:"),
            "{case}: inspection was displayed: {raw:?}"
        );
        let screen = final_screen(&output.stderr);
        for transient in ["Phase:", "Inspecting", "finished"] {
            assert!(
                !screen.contains(transient),
                "{case}: {transient} left on screen:\n{screen}\nraw: {raw:?}"
            );
        }
        if case != "empty" {
            assert!(!screen.is_empty(), "{case}: the error remains visible");
        }
    }
}

// {test}`requirements.references.citation_review`
// {test}`requirements.authoring.batch`
// {test}`requirements.authoring.refresh`
#[test]
fn batch_authors_citation_reviews_and_preserves_current_ones() {
    let server = Server::start(false, None);
    let fixture = Fixture::new(&server.endpoint, None);
    let source = fixture.project.join("src/lib.rs");
    let cite = "// {cite}`requirements.alpha`\n".to_owned();
    fs::write(
        &source,
        format!(
            "{}{cite}pub fn helper() {{}}\n",
            fs::read_to_string(&source).unwrap()
        ),
    )
    .unwrap();
    let output = fixture.run(&["author", "--all"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let subjects = server.subjects();
    assert_eq!(subjects.len(), 7, "{subjects:?}");
    assert!(
        subjects.contains(&"src/lib.rs fn[helper]".to_owned()),
        "{subjects:?}"
    );
    let review: Value = serde_json::from_slice(
        &fs::read(
            fixture
                .project
                .join("evidence/citations/src/lib.rs/fn[helper].json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        review["citations"],
        json!([{"role": "cite", "name": "requirements.alpha", "commentary": ""}])
    );
    let report = habeas::validate_repository(&fixture.project).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);

    let again = fixture.run(&["author", "--all"]);
    assert_eq!(
        String::from_utf8_lossy(&again.stdout).trim(),
        "0 authored; 7 skipped as current; 0 failed"
    );
    // A single named citation review that is current is kept under
    // `--missing`, the default, without a model or configuration.
    fs::write(&fixture.config, "not toml [").unwrap();
    let citation = "citations:src/lib.rs#fn[helper]";
    for args in [
        &["author", citation][..],
        &["author", citation, "--missing"],
    ] {
        let output = fixture.run(args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            format!(
                "{citation}: evidence is current; 1 skipped as current (use --force to reassess)"
            )
        );
    }
    assert_eq!(server.subjects().len(), 7, "no model was invoked");
    fs::write(&fixture.config, &fixture.config_text).unwrap();
    // A citation review can also be named on its own.
    let single = fixture.run(&["author", "citations:src/lib.rs#fn[helper]", "--stdout"]);
    assert!(
        single.status.success(),
        "{}",
        String::from_utf8_lossy(&single.stderr)
    );
    let printed: Value = serde_json::from_slice(&single.stdout).unwrap();
    assert_eq!(printed["item"], "fn[helper]");
    assert_eq!(server.subjects().len(), 8);
}

// {test}`requirements.authoring.confirmation`
#[test]
fn confirm_runs_a_second_assessment_with_its_own_usage_record() {
    let server = Server::start(false, None);
    let fixture = Fixture::new(&server.endpoint, None);
    let output = fixture.run(&["author", "requirements.alpha", "--policy", "confirm"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        server.subjects(),
        [
            "requirement requirements.alpha",
            "requirement requirements.alpha"
        ]
    );
    let usage = fixture.usage();
    let mut subjects = usage
        .iter()
        .map(|run| run["subject"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    subjects.sort();
    assert_eq!(
        subjects,
        [
            "requirements.alpha",
            "requirements.alpha (second assessment)"
        ]
    );
    // Agreeing assessments record the first unchanged.
    assert!(
        !fixture.evidence("requirements.alpha")["rationale"]
            .as_str()
            .unwrap()
            .contains("Confirmation")
    );
}

// {test}`requirements.authoring.confirmation`
// {test}`requirements.authoring.batch`
#[test]
fn confirm_with_all_runs_a_second_assessment_for_each_batch_subject() {
    let server = Server::start(false, None);
    let fixture = Fixture::new(&server.endpoint, None);
    let output = fixture.run(&["author", "--all", "--policy", "confirm"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut subjects = server.subjects();
    subjects.sort();
    assert!(!subjects.is_empty());
    for pair in subjects.chunks(2) {
        assert_eq!(pair.len(), 2, "{subjects:?}");
        assert_eq!(pair[0], pair[1], "{subjects:?}");
    }
    let usage = fixture.usage();
    let seconds = usage
        .iter()
        .filter(|run| {
            run["subject"]
                .as_str()
                .unwrap()
                .ends_with(" (second assessment)")
        })
        .count();
    assert_eq!(seconds * 2, subjects.len(), "{usage:?}");
    assert_eq!(usage.len(), subjects.len(), "{usage:?}");
}

/// Under `--missing`, the default, a named subject or `--all` keeps current
/// records without a model or configuration; `--force` reassesses them, and
/// `--stdout` implies it.
// {test}`requirements.authoring.refresh`
// {test}`requirements.authoring.invocation`
// {test}`requirements.authoring.batch`
// {test}`requirements.references.coverage.report`
#[test]
fn missing_keeps_current_records_and_force_reassesses_every_selected_subject() {
    let server = Server::start(false, None);
    let fixture = Fixture::new(&server.endpoint, None);
    let first = fixture.run(&["author", "--all"]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(server.subjects().len(), 6);
    let alpha = fixture.evidence("requirements.alpha");

    // Without a usable configuration, current records are still kept: no
    // configuration is resolved when nothing needs assessment.
    fs::write(&fixture.config, "not toml [").unwrap();
    // An item nothing reads is warned about even when nothing is assessed.
    fs::write(fixture.project.join("src/extra.rs"), "pub fn orphan() {}\n").unwrap();
    for args in [
        &["author", "requirements.alpha"][..],
        &["author", "requirements.alpha", "--missing"],
        &["author-evidence", "requirements.alpha_case"],
    ] {
        let output = fixture.run(args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            format!(
                "{}: evidence is current; 1 skipped as current (use --force to reassess)",
                args[1]
            )
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("src/extra.rs:1: function orphan has no recent coverage"),
            "{args:?}: {stderr}"
        );
    }
    for args in [&["author", "--all"][..], &["author", "--all", "--missing"]] {
        let output = fixture.run(args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "0 authored; 6 skipped as current; 0 failed"
        );
    }
    assert_eq!(server.subjects().len(), 6, "no model was invoked");
    assert_eq!(fixture.usage().len(), 6);
    assert_eq!(fixture.evidence("requirements.alpha"), alpha);

    // `--missing` and `--force` are mutually exclusive, and `--stdout`
    // already implies `--force`.
    for args in [
        &["author", "requirements.alpha", "--missing", "--force"][..],
        &["author", "--all", "--missing", "--force"],
        &["author", "requirements.alpha", "--stdout", "--missing"],
    ] {
        let output = fixture.run(args);
        assert!(!output.status.success(), "{args:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("cannot be used with"),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(server.subjects().len(), 6);

    fs::write(&fixture.config, &fixture.config_text).unwrap();
    let forced = fixture.run(&["author", "requirements.alpha", "--force"]);
    assert!(
        forced.status.success(),
        "{}",
        String::from_utf8_lossy(&forced.stderr)
    );
    assert_eq!(
        server.subjects().last().unwrap(),
        "requirement requirements.alpha"
    );
    assert_eq!(server.subjects().len(), 7);
    let stdout = fixture.run(&["author", "requirements.alpha", "--stdout"]);
    assert!(
        stdout.status.success(),
        "{}",
        String::from_utf8_lossy(&stdout.stderr)
    );
    let printed: Value = serde_json::from_slice(&stdout.stdout).unwrap();
    assert_eq!(printed["requirement"], alpha["requirement"]);
    assert_eq!(server.subjects().len(), 8, "--stdout implies --force");
    let all = fixture.run(&["author", "--all", "--force"]);
    assert!(
        all.status.success(),
        "{}",
        String::from_utf8_lossy(&all.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&all.stdout).trim(),
        "6 authored; 0 skipped as current; 0 failed; policy once"
    );
    assert_eq!(server.subjects().len(), 14);
    let report = habeas::validate_repository(&fixture.project).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
}

/// The policy comes from `--policy`, then the configuration's `policy`,
/// then defaults to `confirm`; the report and every usage record name it,
/// and an unknown name is rejected before any model request.
// {test}`requirements.authoring.policy`
// {test}`requirements.authoring.configuration_structure`
// {test}`requirements.authoring.confirmation`
#[test]
fn policy_comes_from_the_flag_then_the_configuration_then_defaults_to_confirm() {
    let server = Server::start(false, None);
    let fixture = Fixture::new(&server.endpoint, None);
    let unconfigured = fixture.config_text.replace("policy = 'once'\n", "");
    let cases: [(&str, &[&str], &str, usize); 4] = [
        (&unconfigured, &[], "confirm", 2),
        (&unconfigured, &["--policy", "once"], "once", 1),
        (
            &fixture.config_text.replace("'once'", "'confirm'"),
            &["--policy", "once"],
            "once",
            1,
        ),
        (&fixture.config_text, &[], "once", 1),
    ];
    let run_ids = || {
        if fixture.state.exists() {
            fixture
                .usage()
                .into_iter()
                .map(|run| run["run_id"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        }
    };
    for (config, policy, expected, conversations) in cases {
        fs::write(&fixture.config, config).unwrap();
        let before = server.subjects().len();
        let earlier = run_ids();
        let mut args = vec!["author", "requirements.alpha", "--force"];
        args.extend_from_slice(policy);
        let output = fixture.run(&args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains(&format!("; policy {expected}")),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert_eq!(server.subjects().len() - before, conversations, "{args:?}");
        // Each conversation's usage record identifies the policy.
        let usage = fixture
            .usage()
            .into_iter()
            .filter(|run| !earlier.contains(&run["run_id"].as_str().unwrap().to_owned()))
            .collect::<Vec<_>>();
        assert_eq!(usage.len(), conversations, "{args:?}");
        assert!(
            usage.iter().all(|run| run["policy"] == expected),
            "{args:?}: {usage:?}"
        );
    }
    // The batch summary names the policy too.
    fs::write(&fixture.config, &unconfigured).unwrap();
    let batch = fixture.run(&["author", "--all", "--force", "--policy", "once"]);
    assert_eq!(
        String::from_utf8_lossy(&batch.stdout).trim(),
        "6 authored; 0 skipped as current; 0 failed; policy once"
    );

    let before = server.subjects().len();
    let flag = fixture.run(&[
        "author",
        "requirements.alpha",
        "--force",
        "--policy",
        "twice",
    ]);
    assert!(!flag.status.success());
    let stderr = String::from_utf8_lossy(&flag.stderr);
    assert!(
        stderr.contains("unknown assessment policy \"twice\""),
        "{stderr}"
    );
    fs::write(
        &fixture.config,
        fixture.config_text.replace("'once'", "'twice'"),
    )
    .unwrap();
    for args in [
        &["author", "requirements.alpha", "--force"][..],
        &["author", "--all", "--force"],
    ] {
        let output = fixture.run(args);
        assert!(!output.status.success(), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(&fixture.config.display().to_string()) && stderr.contains("policy"),
            "{args:?}: {stderr}"
        );
    }
    assert_eq!(
        server.subjects().len(),
        before,
        "rejected before any request"
    );
}

/// A local stand-in for the GitHub API serving one issue,
/// `example/queue#125`, whose state a test can change; other issues do not
/// exist.
// {test}`requirements.authoring.github_reads`
struct FakeGithub {
    endpoint: String,
    state: Arc<Mutex<&'static str>>,
    requests: Arc<Mutex<usize>>,
}

impl FakeGithub {
    // {test}`requirements.authoring.github_reads`
    fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new("open"));
        let requests = Arc::new(Mutex::new(0));
        let (served_state, served) = (state.clone(), requests.clone());
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut reader = BufReader::new(&mut stream);
                let mut request = String::new();
                reader.read_line(&mut request).unwrap();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                }
                *served.lock().unwrap() += 1;
                let (status, body) = if request.starts_with("GET /repos/example/queue/issues/125 ")
                {
                    let state = *served_state.lock().unwrap();
                    (
                        "200 OK",
                        json!({"state": state, "title": "Zero capacity", "body": ""}),
                    )
                } else {
                    ("404 Not Found", json!({"message": "Not Found"}))
                };
                let body = body.to_string();
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        Self {
            endpoint,
            state,
            requests,
        }
    }

    // {cite}`habeas:util`
    fn requests(&self) -> usize {
        *self.requests.lock().unwrap()
    }
}

// {test}`requirements.ci.fixme_closed_issue`
// {test}`requirements.cli.fixme_issues`
// {test}`requirements.authoring.github_reads`
// {test}`requirements.authoring.batch`
#[test]
fn closing_an_acknowledged_issue_makes_the_fixme_record_stale_and_reassessed() {
    let server = Server::start_with(false, Behavior::ClosedIssueIsFalse);
    let github = FakeGithub::start();
    let fixture = Fixture::new(&server.endpoint, None);
    let source = fixture.project.join("src/lib.rs");
    fs::write(
        &source,
        fs::read_to_string(&source).unwrap().replace(
            "pub fn beta()",
            "// :::{fixme} requirements.beta\n// :github: example/queue#125\n// Zero capacity is untested.\n// :::\npub fn beta()",
        ),
    )
    .unwrap();
    let run = |args: &[&str]| {
        let output = fixture
            .command()
            .env("HABEAS_GITHUB_API_URL", &github.endpoint)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    run(&["author", "--all"]);
    assert_eq!(server.subjects().len(), 6);
    // The issue was open, so record the acknowledged gap as current FIXME
    // evidence.
    let mut beta = fixture.evidence("requirements.beta");
    beta["determination"] = json!({"FIXME": ["example/queue#125"]});
    fs::write(
        fixture.project.join("evidence/requirements.beta.json"),
        serde_json::to_vec_pretty(&beta).unwrap(),
    )
    .unwrap();
    let cache = fixture.state.join(if cfg!(windows) {
        "habeas/state/github/example+queue+125.json"
    } else {
        "habeas/github/example+queue+125.json"
    });
    assert!(
        cache.exists(),
        "GitHub reads are cached under the state root"
    );
    let status = run(&["status"]);
    assert!(!status.contains("stale"), "{status}");
    let reads = github.requests();

    *github.state.lock().unwrap() = "closed";
    // The cached open state is reused for an hour.
    assert!(!run(&["status"]).contains("stale"));
    assert_eq!(github.requests(), reads);
    fs::remove_file(&cache).unwrap();
    let status = run(&["status"]);
    assert_eq!(github.requests(), reads + 1);
    let row = status
        .lines()
        .find(|line| line.starts_with("| `requirements.beta`"))
        .unwrap_or_else(|| panic!("{status}"));
    assert!(row.contains("Invalid"), "{status}");
    for expected in [
        "evidence/requirements.beta.json: stale: issue example/queue#125, named by the acknowledging {fixme} to requirements.beta at src/lib.rs:5, is closed",
        "src/lib.rs:5: {fixme} names example/queue#125, which is closed",
        "src/lib.rs:9: fn beta fails: its {fixme} at line 5 names example/queue#125, which is closed",
    ] {
        assert!(status.contains(expected), "{expected}\n{status}");
    }
    // Repository validation, which does not contact GitHub, still accepts it.
    let report = habeas::validate_repository(&fixture.project).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(github.requests(), reads + 1);

    run(&["author", "--all"]);
    let subjects = server.subjects();
    assert_eq!(subjects.len(), 7, "{subjects:?}");
    assert_eq!(subjects[6], "requirement requirements.beta");
    let beta = fixture.evidence("requirements.beta");
    assert_eq!(beta["determination"], json!(false), "{beta}");
    // Issue states are supplied with inputs but never recorded.
    assert!(!beta.to_string().contains("github_issues"), "{beta}");
    for run in fixture.usage() {
        assert!(!run.to_string().contains("github_issues"), "{run}");
    }
}

/// A state root inside the project, directly or through a symlink, is not
/// used for the GitHub cache: `status` still reads GitHub, uncached, and
/// `author` refuses the location before writing anything there.
// {test}`requirements.configuration.state_location`
#[test]
fn github_reads_do_not_cache_in_a_state_root_inside_the_project() {
    let server = Server::start(false, None);
    let github = FakeGithub::start();
    let fixture = Fixture::new(&server.endpoint, None);
    let source = fixture.project.join("src/lib.rs");
    fs::write(
        &source,
        fs::read_to_string(&source).unwrap().replace(
            "pub fn beta()",
            "// :::{fixme} requirements.beta\n// :github: example/queue#125\n// Zero capacity is untested.\n// :::\npub fn beta()",
        ),
    )
    .unwrap();
    let inside = fixture.project.join("state");
    let mut states = vec![(inside.clone(), inside.clone())];
    #[cfg(unix)]
    {
        let alias = fixture.project.parent().unwrap().join("state-alias");
        std::os::unix::fs::symlink(&fixture.project, &alias).unwrap();
        states.push((alias.join("state"), inside.clone()));
    }
    for (state, written) in states {
        let run = |args: &[&str]| {
            fixture
                .command()
                .env("XDG_STATE_HOME", &state)
                .env("LOCALAPPDATA", &state)
                .env("HABEAS_GITHUB_API_URL", &github.endpoint)
                .args(args)
                .output()
                .unwrap()
        };
        let before = github.requests();
        let status = run(&["status"]);
        assert!(
            status.status.success(),
            "{}",
            String::from_utf8_lossy(&status.stderr)
        );
        assert!(github.requests() > before, "status still reads GitHub");
        assert!(!written.exists(), "{state:?}");
        let author = run(&["author", "--all"]);
        assert!(!author.status.success());
        assert!(
            String::from_utf8_lossy(&author.stderr)
                .contains("must be outside the project repository"),
            "{}",
            String::from_utf8_lossy(&author.stderr)
        );
        assert!(!written.exists(), "{state:?}");
    }
    assert!(server.subjects().is_empty());
}

/// Checks that the first assessment of each kind starts before any other of
/// its kind, and that the others start only once its first request has been
/// answered, returning the requests of each kind in arrival order.
// {cite}`habeas:util`
fn assert_warmed_first(server: &Server) -> Vec<Vec<(String, Instant, Option<Instant>)>> {
    let timeline = server.timeline();
    let mut kinds = Vec::new();
    for kind in ["requirement ", "example "] {
        let requests = timeline
            .iter()
            .filter(|(subject, ..)| subject.starts_with(kind))
            .cloned()
            .collect::<Vec<_>>();
        let (first, arrived, answered) = &requests[0];
        let answered = answered.expect("the warming request was answered");
        for (subject, other_arrived, _) in &requests[1..] {
            if subject != first {
                assert!(
                    *other_arrived >= answered,
                    "{subject} started before {first}'s first request finished"
                );
            }
            assert!(other_arrived >= arrived);
        }
        kinds.push(requests);
    }
    kinds
}

// {test}`requirements.authoring.cache_warmup`
#[test]
fn a_batch_warms_each_kind_with_one_assessment_before_starting_the_others() {
    let server = Server::start(false, None);
    let fixture = Fixture::new(&server.endpoint, Some(8));
    let output = fixture.run(&["author", "--all"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let kinds = assert_warmed_first(&server);
    assert_eq!(kinds[0].len(), 4, "{:?}", kinds[0]);
    assert_eq!(kinds[1].len(), 2, "{:?}", kinds[1]);
    // The others of a kind then run together.
    let state = server.requests.0.lock().unwrap();
    assert!(state.peak >= 3, "peak {}", state.peak);
}

// {test}`requirements.authoring.cache_warmup`
#[test]
fn a_failed_first_request_still_releases_its_kind() {
    let server = Server::start_with(
        false,
        Behavior::ThrottledOnce {
            subject: "requirement requirements.alpha",
        },
    );
    let fixture = Fixture::new(&server.endpoint, Some(8));
    let output = fixture.run(&["author", "--all"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let kinds = assert_warmed_first(&server);
    let requirements = &kinds[0];
    assert_eq!(requirements[0].0, "requirement requirements.alpha");
    // The other requirements start after the throttled request, without
    // waiting for the retry, which backs off for at least half a second.
    let retry = requirements[1..]
        .iter()
        .find(|(subject, ..)| subject == "requirement requirements.alpha")
        .expect("the throttled request is retried");
    let others = requirements
        .iter()
        .filter(|(subject, ..)| subject != "requirement requirements.alpha")
        .collect::<Vec<_>>();
    assert_eq!(others.len(), 3);
    assert!(others.iter().all(|(_, arrived, _)| *arrived < retry.1));
}

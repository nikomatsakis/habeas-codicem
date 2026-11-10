use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tempfile::TempDir;

const SUBJECT: &str = "requirements.ready";
const REQUIREMENT: &str = concat!(
    "SECRET_REQUIREMENT_TEXT MUST implement {",
    "term}`terms.readiness`."
);
const MODEL: &str = "fixture:q4_K_M";
const AUTH: &str = "SECRET_AUTH_VALUE";

// {cite}`habeas:util`
struct Request {
    headers: String,
    body: Value,
}

/// A request's system prompt and subject message, as Ollama's leading
/// `system` and `user` messages or Bedrock's `system` blocks and first
/// message.
// {cite}`habeas:util`
fn opening(body: &Value) -> (String, String) {
    let text = |value: &Value| value.as_str().unwrap().to_owned();
    if body.get("system").is_some() {
        (
            text(&body["system"][0]["text"]),
            text(&body["messages"][0]["content"][0]["text"]),
        )
    } else {
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["role"], "user");
        (
            text(&body["messages"][0]["content"]),
            text(&body["messages"][1]["content"]),
        )
    }
}

/// A request's system prompt and subject message joined, for checks that
/// text reaches the model in either.
// {cite}`habeas:util`
fn prompt_text(body: &Value) -> String {
    let (system, subject) = opening(body);
    format!("{system}\n\n{subject}")
}

// {cite}`habeas:util`
struct Server {
    endpoint: String,
    thread: JoinHandle<Vec<Request>>,
}

impl Server {
    // {cite}`habeas:util`
    fn start(responses: Vec<Value>) -> Self {
        Self::start_with(responses, || {})
    }

    // {test}`requirements.authoring.persistence`
    fn start_with(responses: Vec<Value>, mut before_reply: impl FnMut() + Send + 'static) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let thread = thread::spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
                let deadline = Instant::now() + Duration::from_secs(15);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                Instant::now() < deadline,
                                "model did not send the expected request"
                            );
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("cannot accept mock request: {error}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut reader = BufReader::new(&mut stream);
                let mut headers = String::new();
                loop {
                    let mut line = String::new();
                    assert!(reader.read_line(&mut line).unwrap() > 0);
                    if line == "\r\n" {
                        break;
                    }
                    headers.push_str(&line);
                }
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .expect("request has content length");
                let mut bytes = vec![0; length];
                reader.read_exact(&mut bytes).unwrap();
                requests.push(Request {
                    headers,
                    body: serde_json::from_slice(&bytes).unwrap(),
                });
                before_reply();
                let body = response.to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            requests
        });
        Self { endpoint, thread }
    }

    // {cite}`habeas:util`
    fn finish(self) -> Vec<Request> {
        self.thread.join().unwrap()
    }
}

// {test}`requirements.authoring.invocation`
struct Fixture {
    temp: TempDir,
    project: PathBuf,
    config: PathBuf,
    state: PathBuf,
    evidence: PathBuf,
    original: String,
}

impl Fixture {
    // {test}`requirements.authoring.persistence`
    fn new(endpoint: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        let config = temp.path().join("config");
        let state = temp.path().join("state");
        for path in [
            project.join("md"),
            project.join("src"),
            project.join("evidence"),
            config.join("habeas"),
        ] {
            fs::create_dir_all(path).unwrap();
        }
        fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = 'fixture'\nversion = '0.1.0'\n",
        )
        .unwrap();
        fs::write(project.join("book.toml"), "[book]\nsrc = 'md'\n").unwrap();
        fs::write(
            project.join("md/SUMMARY.md"),
            "# Summary\n\n- [Requirements](requirements.md)\n- [Terms](terms.md)\n",
        )
        .unwrap();
        fs::write(
            project.join("md/requirements.md"),
            format!(":::{{req}} ready\n{REQUIREMENT}\n:::\n"),
        )
        .unwrap();
        fs::write(
            project.join("md/terms.md"),
            ":::{definition} readiness\n:phrase: readiness\n\nSECRET_DEFINITION_TEXT.\n:::\n",
        )
        .unwrap();
        fs::write(project.join("src/lib.rs"), concat!("// {", "impl}`requirements.ready`\n// :::{fixme} requirements.ready\n// :github: example/app#125\n// One acknowledged gap.\n// :::\npub fn ready() {}\n// {", "test}`requirements.ready`\nfn ready_test() {}\n")).unwrap();
        fs::write(
            project.join("extra.txt"),
            (1..=12)
                .map(|line| {
                    if line == 6 {
                        "SECRET_SOURCE_PAYLOAD needle".to_owned()
                    } else {
                        format!("context line {line}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let original = json!({"requirement":REQUIREMENT,"determination":false,
            "rationale":"OLD_RATIONALE_MUST_NOT_REACH_MODEL", "inputs":[]})
        .to_string();
        let evidence = project.join(format!("evidence/{SUBJECT}.json"));
        fs::write(&evidence, &original).unwrap();
        fs::write(config.join("habeas/config.toml"), format!(
            "version = 1\n[authoring]\ndefault = 'mock'\n[authoring.configs.mock]\npolicy = 'once'\nbackend = 'ollama'\nmodel = '{MODEL}'\nendpoint = '{endpoint}'\napi_key_env = 'HABEAS_TEST_MODEL_KEY'\n[authoring.configs.mock.options]\ntemperature = 0.25\nnum_ctx = 4096\nthink = false\n")).unwrap();
        Self {
            temp,
            project,
            config,
            state,
            evidence,
            original,
        }
    }

    // {test}`requirements.authoring.invocation`
    // {test}`requirements.authoring.deterministic_validation`
    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }

    // {test}`requirements.authoring.invocation`
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_habeas"));
        command
            .current_dir(&self.project)
            .env("XDG_CONFIG_HOME", &self.config)
            .env("XDG_STATE_HOME", &self.state)
            .env("APPDATA", &self.config)
            .env("LOCALAPPDATA", &self.state)
            .env("HOME", self.temp.path())
            .env("HABEAS_TEST_MODEL_KEY", AUTH)
            // No test contacts GitHub: nothing listens on this port, so
            // issues are unreachable and count as open.
            .env("HABEAS_GITHUB_API_URL", "http://127.0.0.1:9");
        command
    }

    // {cite}`habeas:util`
    fn usage(&self) -> Value {
        let directory = if cfg!(windows) {
            self.state.join("habeas/state/authoring/usage")
        } else {
            self.state.join("habeas/authoring/usage")
        };
        let files = fs::read_dir(directory)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(files.len(), 1);
        serde_json::from_slice(&fs::read(files[0].path()).unwrap()).unwrap()
    }
}

fn call(id: &str, name: &str, arguments: Value) -> Value {
    json!({"id":id,"type":"function","function":{"name":name,"arguments":arguments}})
}

fn reply(calls: Vec<Value>) -> Value {
    json!({"model":MODEL,"created_at":"2026-01-01T00:00:00Z",
        "message":{"role":"assistant","content":"review complete","tool_calls":calls},
        "done":true,"done_reason":"stop","prompt_eval_count":11,"eval_count":7})
}

// {test}`requirements.authoring.complete_initial_bundle`
// {test}`requirements.authoring.invocation`
// {test}`requirements.authoring.additional_read`
// {test}`requirements.authoring.acknowledged_gap`
// {test}`requirements.authoring.configuration_precedence`
// {test}`requirements.authoring.rig_configuration_mapping`
// {test}`requirements.authoring.configuration_storage`
// {test}`requirements.authoring.initial_prompt`
// {test}`requirements.ci.review_criteria`
// {test}`requirements.authoring.delivered_inputs`
// {test}`requirements.authoring.usage_storage`
// {test}`requirements.authoring.usage_inputs`
// {test}`requirements.authoring.usage_requests`
// {test}`requirements.authoring.mandatory_inputs`
// {test}`requirements.authoring.github_reads`
// {test}`requirements.authoring.invocation`
// {test}`requirements.configuration.serialization`
#[test]
fn ollama_cli_delivers_sources_records_fixme_and_keeps_usage_content_free() {
    let server = Server::start(vec![
        reply(vec![
            call("list", "list_files", json!({})),
            call(
                "search",
                "ripgrep",
                json!({"path":"extra.txt","pattern":"needle"}),
            ),
            call(
                "read",
                "habeas",
                json!({"file":"extra.txt","line_start":10}),
            ),
            call(
                "repeat",
                "habeas",
                json!({"file":"extra.txt","line_start":10}),
            ),
            call(
                "missing",
                "habeas",
                json!({"file":"absent.txt","line_start":1}),
            ),
        ]),
        reply(vec![call(
            "submit",
            "submit_judgment",
            json!({
            "determination":{"FIXME":["example/app#125"]},"rationale":"SECRET_RATIONALE: src/lib.rs:2 documents the gap."}),
        )]),
    ]);
    let fixture = Fixture::new(&server.endpoint);
    let user_config = fixture.config.join("habeas/config.toml");
    let configured = fs::read(&user_config).unwrap();
    let output = fixture.run(&["author-evidence", SUBJECT]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // The user configuration, with its environment-resolved key, is not
    // rewritten.
    assert_eq!(fs::read(&user_config).unwrap(), configured);
    let requests = server.finish();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert!(request.headers.starts_with("POST /api/chat HTTP/1.1\r\n"));
        assert!(request.headers.to_ascii_lowercase().contains(&format!(
            "authorization: bearer {}",
            AUTH.to_ascii_lowercase()
        )));
        assert_eq!(request.body["model"], MODEL);
        assert_eq!(request.body["options"]["temperature"], 0.25);
        assert_eq!(request.body["options"]["num_ctx"], 4096);
        assert_eq!(request.body["think"], false);
        assert_eq!(request.body["stream"], false);
    }
    let prompt = prompt_text(&requests[0].body);
    for text in [
        "Assess whether a project's source code and tests satisfy one requirement.",
        REQUIREMENT,
        "SECRET_DEFINITION_TEXT",
        "fn ready()",
        "fn ready_test()",
        "submit_judgment exactly once",
    ] {
        assert!(prompt.contains(text), "prompt missing {text}");
    }
    assert!(!prompt.contains("OLD_RATIONALE_MUST_NOT_REACH_MODEL"));
    let mut tools = requests[0].body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    tools.sort();
    assert_eq!(
        tools,
        [
            "ast_grep",
            "github",
            "habeas",
            "list_files",
            "next_page",
            "outline",
            "references",
            "ripgrep",
            "submit_judgment"
        ]
    );
    let results = requests[1].body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "tool")
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 5);
    assert!(results.iter().any(|message| {
        message["content"]
            .as_str()
            .unwrap()
            .contains("SECRET_SOURCE_PAYLOAD")
    }));
    assert!(
        results
            .iter()
            .any(|message| message["content"].as_str().unwrap().contains("absent.txt"))
    );

    let evidence: Value = serde_json::from_slice(&fs::read(&fixture.evidence).unwrap()).unwrap();
    assert_eq!(
        evidence["determination"],
        json!({"FIXME":["example/app#125"]})
    );
    assert_eq!(
        evidence["rationale"],
        "SECRET_RATIONALE: src/lib.rs:2 documents the gap."
    );
    assert_eq!(evidence["requirement"], REQUIREMENT);
    let inputs = evidence["inputs"].as_array().unwrap();
    // The `{fixme}` reached the model and is covered by the recorded input
    // for the function it belongs to.
    assert!(prompt.contains("// :github: example/app#125"));
    assert!(prompt.contains("// One acknowledged gap."));
    // GitHub is unreachable, so the issue is supplied as open, but the state
    // is not recorded.
    assert!(
        prompt.contains(
            "\"github_issues\": [\n      {\n        \"issue\": \"example/app#125\",\n        \"state\": \"open\"\n      }\n    ]"
        ),
        "{prompt}"
    );
    assert!(!evidence.to_string().contains("github_issues"));
    assert!(!fixture.usage().to_string().contains("github_issues"));
    assert!(
        inputs
            .iter()
            .any(|input| input["file"] == "src/lib.rs" && input["item"] == "fn[ready]"),
        "{inputs:#?}"
    );
    // CI accepts the FIXME judgment: its evidence has no validation error.
    let report = habeas::validate_repository(&fixture.project).unwrap();
    let label = format!("evidence/{SUBJECT}.json");
    assert!(
        !report.errors.iter().any(|error| error.contains(&label)),
        "{:#?}",
        report.errors
    );
    // The same record with a false determination is rejected.
    let mut rejected = evidence.clone();
    rejected["determination"] = json!(false);
    let accepted = fs::read(&fixture.evidence).unwrap();
    fs::write(&fixture.evidence, rejected.to_string()).unwrap();
    let report = habeas::validate_repository(&fixture.project).unwrap();
    assert!(
        report.errors.iter().any(|error| error.contains(&label)),
        "{:#?}",
        report.errors
    );
    fs::write(&fixture.evidence, accepted).unwrap();
    assert!(
        inputs
            .iter()
            .any(|input| input["definition"] == "terms.readiness")
    );
    let extra = inputs
        .iter()
        .filter(|input| input["file"] == "extra.txt")
        .collect::<Vec<_>>();
    // The search only locates lines and records nothing; the reads cover
    // lines 10-12, one plain-text line node each, and the repeated delivery
    // is recorded once, by selector and hash only.
    assert_eq!(
        extra,
        habeas::outline_inputs_for_lines(&fixture.project, "extra.txt", 10, 12)
            .unwrap()
            .iter()
            .collect::<Vec<_>>(),
        "repeated delivery is deduplicated in evidence"
    );
    assert!(
        !evidence["inputs"]
            .to_string()
            .contains("SECRET_SOURCE_PAYLOAD")
    );
    assert!(
        !inputs
            .iter()
            .any(|input| input["file"] == "absent.txt" || input["file"] == ".")
    );

    let usage = fixture.usage();
    assert_eq!(usage["outcome"], "completed");
    assert_eq!(usage["determination"], "FIXME");
    assert_eq!(usage["evidence_written"], true);
    assert_eq!(usage["request_count"], 2);
    assert_eq!(usage["tool_call_count"], 6);
    assert_eq!(usage["tool_calls"]["habeas"], 3);
    assert_eq!(usage["tool_failures"]["habeas"], 1);
    // The directory listing counts as a tool call but is not an input.
    assert_eq!(usage["tool_calls"]["list_files"], 1);
    assert_eq!(usage["tool_calls"]["ripgrep"], 1);
    let usage_inputs = usage["inputs"].as_array().unwrap();
    assert!(
        !usage_inputs
            .iter()
            .any(|input| input["origin"] == "list_files"),
        "{usage_inputs:?}"
    );
    // Search results are not inputs either.
    assert!(
        !usage_inputs
            .iter()
            .any(|input| input["origin"] == "ripgrep"),
        "{usage_inputs:?}"
    );
    assert!(
        usage_inputs
            .iter()
            .any(|input| input["origin"] == "habeas" && input["unique"] == true),
        "{usage_inputs:?}"
    );
    assert_eq!(usage["tokens"]["input"], 22);
    assert_eq!(usage["tokens"]["output"], 14);
    assert_eq!(usage["tokens"]["complete"], true);
    assert!(
        usage["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|input| input["origin"] == "habeas"
                && input["unique"] == false
                && input["sent_in_request"] == 1)
    );
    assert!(
        usage["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|input| input["origin"] == "mandatory"
                && input["kind"] == "definition"
                && input["bytes"].as_u64().unwrap() > 0)
    );
    let serialized = usage.to_string();
    for secret in [
        "SECRET_REQUIREMENT_TEXT",
        "SECRET_DEFINITION_TEXT",
        "SECRET_SOURCE_PAYLOAD",
        "SECRET_RATIONALE",
        AUTH,
        "#125",
    ] {
        assert!(
            !serialized.contains(secret),
            "usage contains source or credential content: {secret}"
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(MODEL) && stdout.contains("Evidence:") && stdout.contains("Usage:"));
    // By default the report names the subject and backend and identifies the
    // evidence file it wrote.
    assert!(
        stdout.starts_with(&format!("{SUBJECT}: ollama / {MODEL}")),
        "{stdout}"
    );
    let evidence = stdout
        .lines()
        .find_map(|line| line.strip_prefix("Evidence: "))
        .unwrap();
    assert!(
        evidence.ends_with(&format!("evidence/{SUBJECT}.json")),
        "{stdout}"
    );
    assert_eq!(
        fs::canonicalize(evidence).unwrap(),
        fs::canonicalize(fixture.project.join(format!("evidence/{SUBJECT}.json"))).unwrap()
    );
}

// {test}`requirements.configuration.workspace_location`
// {test}`requirements.authoring.configuration`
// {test}`requirements.configuration.workspace_structure`
// {test}`requirements.authoring.configuration_precedence`
// {test}`requirements.authoring.credentials`
// {test}`requirements.authoring.provider`
// {test}`requirements.authoring.rig_configuration_mapping`
// {test}`requirements.authoring.invocation`
// {test}`requirements.authoring.usage_storage`
// {test}`requirements.configuration.serialization`
#[test]
fn authoring_uses_nearest_workspace_configuration_without_a_user_file() {
    let server = Server::start(vec![reply(vec![call(
        "submit",
        "submit_judgment",
        json!({"determination":true,"rationale":"The supplied sources support the subject."}),
    )])]);
    let fixture = Fixture::new(&server.endpoint);
    fs::remove_file(fixture.config.join("habeas/config.toml")).unwrap();
    fs::write(
        fixture.project.join("habeas.toml"),
        "version = 1\n[authoring]\ndefault = 'missing-parent-choice'\n",
    )
    .unwrap();
    let nested = fixture.project.join("src/nested");
    fs::create_dir(&nested).unwrap();
    let workspace = nested.join("habeas.toml");
    fs::write(
        &workspace,
        format!(
            "version = 1\n[authoring]\ndefault = 'workspace'\n[authoring.configs.workspace]\npolicy = 'once'\nbackend = 'ollama'\nmodel = 'workspace-model'\nendpoint = '{}'\napi_key = 'WORKSPACE_LITERAL_KEY'\n",
            server.endpoint
        ),
    )
    .unwrap();
    let configurations = [fixture.project.join("habeas.toml"), workspace.clone()];
    let before = configurations
        .iter()
        .map(|path| fs::read(path).unwrap())
        .collect::<Vec<_>>();
    let output = fixture
        .command()
        .current_dir(&nested)
        .args(["author", SUBJECT])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    // Running the configured feature rewrites neither configuration file,
    // and creates no user configuration.
    for (path, before) in configurations.iter().zip(&before) {
        assert_eq!(&fs::read(path).unwrap(), before, "{}", path.display());
    }
    assert!(!fixture.config.join("habeas/config.toml").exists());
    let requests = server.finish();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].body["model"], "workspace-model");
    assert!(requests[0].headers.contains("WORKSPACE_LITERAL_KEY"));
    assert!(
        !requests[0]
            .body
            .to_string()
            .contains("WORKSPACE_LITERAL_KEY")
    );
    let usage = fixture.usage();
    assert_eq!(usage["model"], "workspace-model");
    for text in [
        usage.to_string(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ] {
        assert!(!text.contains("WORKSPACE_LITERAL_KEY"), "{text}");
    }
}

// {test}`requirements.authoring.read_denial`
// {test}`requirements.authoring.file_discovery`
// {test}`requirements.authoring.text_search`
// {test}`requirements.authoring.structural_search`
// {test}`requirements.authoring.source_tools`
// {test}`requirements.authoring.persistence`
// {test}`requirements.authoring.usage_storage`
// {test}`requirements.authoring.usage_requests`
#[test]
fn explicit_untracked_reads_abort_without_another_request_or_evidence_write() {
    for (tool, arguments) in [
        ("habeas", json!({"file":"untracked.txt", "line_start":1})),
        (
            "ripgrep",
            json!({"path":"untracked.txt", "pattern":"hidden"}),
        ),
        (
            "ast_grep",
            json!({"path":"untracked.txt", "language":"rust", "rule":{"rule":{"pattern":"hidden"}}}),
        ),
        ("list_files", json!({"path":"untracked-directory"})),
    ] {
        let server = Server::start(vec![reply(vec![call("denied", tool, arguments)])]);
        let fixture = Fixture::new(&server.endpoint);
        for args in [["init", "--quiet"], ["add", "."]] {
            let git = Command::new("git")
                .current_dir(&fixture.project)
                .args(args)
                .output()
                .unwrap();
            assert!(git.status.success(), "{git:?}");
        }
        fs::write(fixture.project.join("untracked.txt"), "hidden source\n").unwrap();
        fs::create_dir(fixture.project.join("untracked-directory")).unwrap();
        fs::write(
            fixture.project.join("untracked-directory/hidden.txt"),
            "hidden source\n",
        )
        .unwrap();

        let output = fixture.run(&["author", SUBJECT, "--stdout"]);
        assert!(!output.status.success(), "{tool}: {output:?}");
        assert!(output.stdout.is_empty(), "{tool}: {output:?}");
        let diagnostics = String::from_utf8_lossy(&output.stderr);
        assert!(diagnostics.contains("untracked"), "{tool}: {diagnostics}");
        assert_eq!(
            fs::read_to_string(&fixture.evidence).unwrap(),
            fixture.original
        );
        assert_eq!(server.finish().len(), 1);
        let usage = fixture.usage();
        assert_eq!(usage["outcome"], "failed", "{tool}: {usage}");
        assert_eq!(usage["request_count"], 1, "{tool}: {usage}");
        assert_eq!(usage["tool_failures"][tool], 1, "{tool}: {usage}");
        assert_eq!(usage["evidence_written"], false, "{tool}: {usage}");
        assert!(usage["determination"].is_null(), "{tool}: {usage}");
    }
}

// {test}`requirements.authoring.negative_assessment`
// {test}`requirements.authoring.usage_storage`
// {test}`requirements.authoring.persistence`
// {test}`requirements.ci.fixme_with_unacknowledged_gap`
// {test}`requirements.authoring.reporting`
#[test]
fn false_is_a_completed_assessment() {
    let server = Server::start(vec![reply(vec![call(
        "submit",
        "submit_judgment",
        json!({"determination":false,"rationale":"src/lib.rs documents the zero-capacity gap as FIXME(#125), but push also overwrites a full positive-capacity queue without acknowledging that defect."}),
    )])]);
    let fixture = Fixture::new(&server.endpoint);
    fs::write(fixture.project.join("md/requirements.md"), ":::{req} ready\nInserting into a full queue MUST return false without overwriting its entries, including at zero capacity.\n:::\n").unwrap();
    fs::write(fixture.project.join("src/lib.rs"), concat!("// {", "impl}`requirements.ready`\n// FIXME(#125): zero-capacity insertion panics.\npub fn push(queue: &mut [u8], value: u8) -> bool {\n    queue[0] = value;\n    true\n}\n")).unwrap();
    let output = fixture.run(&["author-evidence", SUBJECT, "--config", "mock"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let requests = server.finish();
    let prompt = prompt_text(&requests[0].body);
    assert!(prompt.contains("and false otherwise."));
    assert!(prompt.contains("FIXME(#125): zero-capacity insertion panics"));
    assert!(prompt.contains("queue[0] = value"));
    let evidence: Value = serde_json::from_slice(&fs::read(&fixture.evidence).unwrap()).unwrap();
    assert_eq!(evidence["determination"], false);
    assert!(!evidence["inputs"].as_array().unwrap().is_empty());
    assert!(
        evidence["rationale"]
            .as_str()
            .unwrap()
            .contains("without acknowledging")
    );
    // Outline inputs record selectors and hashes, never the source text.
    assert!(
        evidence["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|input| input["item"].is_string() && input["hash"].is_string())
    );
    assert!(!evidence["inputs"].to_string().contains("FIXME(#125)"));
    assert_eq!(fixture.usage()["outcome"], "completed");
    assert_eq!(fixture.usage()["determination"], "false");
    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(&format!("{SUBJECT}: false")), "{stderr}");
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains(&format!("{SUBJECT}: false")),
        "{diagnostics}"
    );
    assert!(
        !diagnostics.contains("without acknowledging"),
        "{diagnostics}"
    );
    // CI rejects the false judgment even though a FIXME acknowledges one gap.
    let report = habeas::validate_repository(&fixture.project).unwrap();
    let evidence_file = format!("evidence/{SUBJECT}.json");
    let rejection = report
        .errors
        .iter()
        .find(|error| error.starts_with(&evidence_file) && error.contains("determination is false"))
        .unwrap_or_else(|| panic!("{:?}", report.errors));
    assert!(rejection.contains("without acknowledging"), "{rejection}");
}

// {test}`requirements.authoring.negative_assessment`
#[test]
fn negative_assessment_records_additional_reads_and_fails_validation() {
    let rationale =
        "src/lib.rs implements readiness, but extra.txt line 6 shows the needle case is unhandled.";
    let server = Server::start(vec![
        reply(vec![call(
            "read",
            "habeas",
            json!({"file":"extra.txt","line_start":6,"line_end":6}),
        )]),
        reply(vec![call(
            "submit",
            "submit_judgment",
            json!({"determination":false,"rationale":rationale}),
        )]),
    ]);
    let fixture = Fixture::new(&server.endpoint);
    let output = fixture.run(&["author", SUBJECT]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let requests = server.finish();
    assert_eq!(requests.len(), 2, "the model made one additional read");

    let evidence: Value = serde_json::from_slice(&fs::read(&fixture.evidence).unwrap()).unwrap();
    assert_eq!(evidence["determination"], false);
    assert_eq!(evidence["rationale"], rationale);
    let inputs = evidence["inputs"].as_array().unwrap();
    // Every supplied input is saved: the mandatory sources and the additional read.
    assert!(
        inputs.iter().any(|input| input["file"] == "src/lib.rs"),
        "mandatory source input missing from {inputs:?}"
    );
    assert!(
        inputs
            .iter()
            .any(|input| input["definition"] == "terms.readiness")
    );
    for line in habeas::outline_inputs_for_lines(&fixture.project, "extra.txt", 6, 6).unwrap() {
        assert!(inputs.contains(&line), "additional read {line} missing");
    }
    assert_eq!(fixture.usage()["outcome"], "completed");
    assert_eq!(fixture.usage()["determination"], "false");

    // Ordinary evidence validation reports the written false determination.
    let report = habeas::validate_repository(&fixture.project).unwrap();
    assert!(
        report.errors.iter().any(|error| error.contains(&format!(
            "evidence/{SUBJECT}.json: determination is false: {rationale}"
        ))),
        "{:?}",
        report.errors
    );
}

/// The `index`th text block of requirement `name` in the authoring chapter.
// {test}`requirements.authoring.initial_prompt`
fn approved_text(name: &str, index: usize) -> &'static str {
    let chapter = include_str!("../md/requirements/authoring.md");
    let opening = format!(":::{{req}} {name}\n");
    let start = chapter.find(&opening).unwrap() + opening.len();
    let body = &chapter[start..];
    let body = &body[..body.find("\n:::\n").unwrap()];
    let block = body.split("```text\n").nth(index + 1).unwrap();
    block.split("\n```").next().unwrap()
}

// {test}`requirements.authoring.initial_prompt`
// {test}`requirements.authoring.prompt_parts`
#[test]
fn initial_prompt_substitutes_subject_and_mandatory_inputs_into_the_template() {
    let template = approved_text("initial_prompt", 0);
    let subject_template = approved_text("initial_prompt", 1);
    for (subject, kind) in [
        (SUBJECT, "requirement"),
        ("requirements.ready_case", "example"),
    ] {
        let server = Server::start(vec![reply(vec![call(
            "submit",
            "submit_judgment",
            json!({"determination":true,"rationale":"covered"}),
        )])]);
        let fixture = Fixture::new(&server.endpoint);
        fs::write(
            fixture.project.join("md/requirements.md"),
            format!(
                ":::{{req}} ready\n{REQUIREMENT}\n:::\n\n:::{{example}} ready_case\nCalling ready succeeds. {{cite}}`requirements.ready`\n:::\n"
            ),
        )
        .unwrap();
        let output = fixture.run(&["author", subject]);
        assert!(output.status.success(), "{output:?}");
        let requests = server.finish();
        let (system, prompt) = opening(&requests[0].body);

        let task = if kind == "requirement" {
            format!(
                "{}\n{}",
                approved_text("requirement_task", 0),
                approved_text("testing_bar", 0)
            )
        } else {
            approved_text("example_task", 0).to_owned()
        };
        // Apart from the substitutions, the system prompt is the first block
        // as written, with nothing that names the subject.
        let expected = template
            .replace("{{subject_kind}}", kind)
            .replace(
                "{{habeas_background}}",
                approved_text("habeas_background", 0),
            )
            .replace("{{reference_roles}}", approved_text("habeas_background", 1))
            .replace("{{subject_task}}", &task)
            .replace("{{judgment_rules}}", approved_text("judgment_rules", 0));
        assert!(!expected.contains("{{"), "{expected}");
        assert_eq!(system, expected);
        assert!(!system.contains(subject), "{system}");
        let (before, after) = subject_template.split_once("{{mandatory_inputs}}").unwrap();
        // The subject is not distributed, so its distributed task is empty.
        let before = before
            .replace("{{subject_kind}}", kind)
            .replace("{{subject_name}}", subject)
            .replace("{{distributed_task}}", "");
        assert!(
            before.contains(&format!("\nSubject: {kind} {subject}\n")),
            "{before}"
        );
        assert!(!before.contains("{{") && !after.contains("{{"));
        // Apart from the substitutions, the subject message is the second
        // block as written.
        assert!(prompt.starts_with(&before), "{prompt}");
        assert!(prompt.ends_with(after), "{prompt}");
        let rendered = &prompt[before.len()..prompt.len() - after.len()];

        // The mandatory inputs are a JSON array of evidence inputs.
        let inputs: Vec<Value> = serde_json::from_str(rendered).unwrap();
        assert!(!inputs.is_empty());
        for input in &inputs {
            let forms = [
                ("file", &["item", "hash", "text"][..]),
                ("requirements", &["matches", "text"][..]),
                ("definition", &["text"][..]),
            ];
            let matching = forms
                .iter()
                .filter(|(key, _)| input.get(key).is_some())
                .collect::<Vec<_>>();
            assert_eq!(matching.len(), 1, "one evidence-input form: {input}");
            for field in matching[0].1 {
                assert!(input.get(field).is_some(), "{input} lacks {field}");
            }
        }
        let defined = |name: &str| {
            inputs
                .iter()
                .any(|input| input["definition"] == name || input["requirements"] == name)
        };
        assert!(defined(subject), "{inputs:?}");
        if kind == "example" {
            assert!(defined(SUBJECT), "cited requirement supplied: {inputs:?}");
        } else {
            assert!(defined("terms.readiness"), "{inputs:?}");
        }
    }
}

// {test}`requirements.authoring.invocation`
// {test}`requirements.authoring.batch`
#[test]
fn author_rejects_unknown_subjects_and_conflicting_targets() {
    let fixture = Fixture::new("http://127.0.0.1:1");
    let output = fixture.run(&["author", "requirements.absent"]);
    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("requirements.absent"), "{stderr}");
    for args in [
        ["author", SUBJECT, "--all"],
        ["author", "--all", "--stdout"],
        ["author-evidence", SUBJECT, "--all"],
    ] {
        let output = fixture.run(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("cannot be used with"), "{stderr}");
    }
    assert_eq!(
        fs::read_to_string(&fixture.evidence).unwrap(),
        fixture.original
    );
}

// {test}`requirements.authoring.batch`
#[test]
fn bundled_skills_direct_the_agent_to_the_authoring_harness() {
    // habeas-evidence is the home of the authoring workflow.
    assert_phrases(
        "habeas-evidence",
        &[
            "`habeas author --all` assesses every definition whose evidence is missing, stale, or invalid",
            "Never write or edit evidence JSON, determinations, rationales, or inputs yourself, and never hand that to another agent",
            "Fix the source, tests, references, or requirement text",
        ],
    );
    // The other skills run the command and defer to habeas-evidence.
    assert_phrases(
        "habeas-requirements",
        &[
            "`habeas author` has a model, the reviewer, assess each one",
            "Follow the habeas-evidence workflow to author evidence and fix the gaps it reports",
            "Never write evidence JSON, rationales, determinations, or inputs yourself, and never ask another agent to",
        ],
    );
    assert_phrases(
        "habeas-bootstrap-requirements",
        &[
            "`habeas author --all`",
            "follow the habeas-evidence workflow for this area's definitions",
            "Never write evidence yourself or ask another agent to",
        ],
    );
}

// {test}`requirements.cli.internal_detail_guidance`
// {test}`requirements.cli.skill_structure`
#[test]
fn bundled_skills_direct_the_reference_procedure_and_report_items_without_one() {
    // habeas-requirements is the home of the reference procedure; its steps
    // come in order.
    assert_in_order(
        "habeas-requirements",
        &[
            "a type does when the requirement constrains its fields, variants, serialized form, or validation",
            "Choose each item's references by this procedure",
            "designed to verify the requirement or example, use `{test}`",
            "if the item is a root of the requirement's implementation, use `{impl}`",
            "re-checked or kept in sync when the requirement or example changes, use `{cite}`",
            "cite the built-in `habeas:util` when the item serves other code and carries out no requirement itself",
            "such as a library calling a trait implementation it requires",
            "An item that does neither needs a requirement or is not needed; report it to the user instead of adding a reference",
            "If no code reaches the item, it is dead code: report it to the user instead of deleting it unasked",
            "Otherwise it carries out behavior that needs a requirement: report it with a proposed requirement for that behavior",
        ],
    );
    assert_phrases(
        "habeas-requirements",
        &[
            "propose a requirement stating the invariant and implement it with `{impl}`",
            "Don't propose grouping requirements that state no checkable obligation",
            "Code must not implement observable behavior that no requirement specifies",
            "There is no waiver",
        ],
    );
    // The other skills refer to the procedure rather than restate it.
    assert_phrases(
        "habeas-evidence",
        &[
            "references by the procedure in the habeas-requirements skill",
            "or `habeas:util` for a helper that serves its callers",
            "Report an item that does neither to the user: it needs a requirement or is not needed",
        ],
    );
    assert_phrases(
        "habeas-bootstrap-requirements",
        &[
            "references by the procedure in the habeas-requirements skill",
            "write a requirement for the invariant they check",
        ],
    );
    for name in ["habeas-evidence", "habeas-bootstrap-requirements"] {
        assert!(
            !bundled_skill(name).contains("fields, variants, serialized form, or validation"),
            "{name} restates the reference procedure"
        );
    }
    for name in SKILLS {
        let text = bundled_skill(name);
        for grouping in [
            "internal.config_parsing",
            "catch-all",
            "internal-detail requirement",
        ] {
            assert!(!text.contains(grouping), "{name} suggests {grouping:?}");
        }
        for waiver in ["may waive", "can waive", "waiver applies", "is exempt"] {
            assert!(
                !text.to_lowercase().contains(waiver),
                "{name} offers a waiver"
            );
        }
    }
}

// {test}`requirements.cli.flip_guidance`
// {test}`requirements.cli.skill_structure`
#[test]
fn bundled_skills_treat_disagreeing_assessments_as_findings() {
    const GOAL: &str = "The goal is that independent assessments of the same subject reach the same determination, because the requirement, code, and tests leave no gap a careful reviewer could find and no reading a careful reviewer could take differently";
    // habeas-evidence is the home: the goal leads and the procedure follows.
    assert_in_order(
        "habeas-evidence",
        &[
            "## Disagreeing assessments",
            GOAL,
            "Assessments that disagree are findings against that goal",
            "Read both assessments",
            "Decide whether one assessment found a real gap or the requirement text admits more than one reading",
            "Fix the source or tests, or tighten the requirement text, reviewing substantive changes with the user",
            "Don't re-run for a different result",
            "record it in the audit log as the Rules say",
        ],
    );
    assert_phrases(
        "habeas-evidence",
        &[
            "record the old text, new text, reason, and the rationale that prompted the change in the project's audit log, such as `audits/requirement-rewordings/`",
        ],
    );
    // A rewording for precision is reviewed with the user before it is logged.
    assert_in_order(
        "habeas-requirements",
        &[
            "Review every other change to agreed text with the user first too, even a rewording for precision",
            "then log it as the habeas-evidence Rules say",
        ],
    );
    assert_phrases(
        "habeas-evidence",
        &["When the user agrees to reword a requirement for precision"],
    );
    // The other skills point to it rather than restate it.
    for name in ["habeas-requirements", "habeas-bootstrap-requirements"] {
        let text = bundled_skill(name);
        assert!(
            text.contains("\"Disagreeing assessments\" in the habeas-evidence skill"),
            "{name} does not refer to habeas-evidence"
        );
        assert!(!text.contains(GOAL), "{name} restates the goal");
    }
}

// {test}`requirements.authoring.usage_storage`
#[cfg(unix)]
#[test]
fn usage_record_identifies_its_run_and_unusable_state_is_reported() {
    let server = Server::start(vec![reply(vec![call(
        "submit",
        "submit_judgment",
        json!({"determination":true,"rationale":"covered"}),
    )])]);
    let fixture = Fixture::new(&server.endpoint);
    let output = fixture.run(&["author", SUBJECT]);
    assert!(output.status.success(), "{output:?}");
    server.finish();
    let directory = fixture.state.join("habeas/authoring/usage");
    let files = fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(files.len(), 1);
    let usage: Value = serde_json::from_slice(&fs::read(&files[0]).unwrap()).unwrap();
    let run_id = usage["run_id"].as_str().unwrap();
    assert!(!run_id.is_empty());
    assert_eq!(
        files[0].file_name().unwrap().to_str().unwrap(),
        format!("{run_id}.json"),
        "the record is named after its run"
    );
    assert_eq!(usage["version"], 1);
    assert_eq!(
        usage["project_root"],
        fixture.project.canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(usage["subject"], SUBJECT);
    assert_eq!(usage["subject_kind"], "requirement");
    assert_eq!(usage["backend"], "ollama");
    assert_eq!(usage["model"], MODEL);
    assert_eq!(usage["evidence_output"], "file");
    let start = usage["start_time_ms"].as_u64().unwrap();
    let end = usage["end_time_ms"].as_u64().unwrap();
    assert!(start > 0 && start <= end, "{usage}");

    // A state location that cannot be determined or created is reported
    // before any model request, and no evidence is written.
    let blocker = fixture.temp.path().join("not-a-directory");
    fs::write(&blocker, "").unwrap();
    for (variable, value) in [
        ("XDG_STATE_HOME", Some(blocker.as_os_str())),
        ("XDG_STATE_HOME", None),
    ] {
        let mut command = fixture.command();
        match value {
            Some(value) => command.env(variable, value),
            None => command.env_remove(variable).env_remove("HOME"),
        };
        let output = command
            .args(["author", SUBJECT, "--force"])
            .output()
            .unwrap();
        assert!(!output.status.success(), "{value:?}: {output:?}");
        assert!(!output.stderr.is_empty(), "{value:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{value:?}: {output:?}");
    }
    let evidence: Value = serde_json::from_slice(&fs::read(&fixture.evidence).unwrap()).unwrap();
    assert_eq!(evidence["rationale"], "covered", "only the first run wrote");
}

// {test}`requirements.authoring.usage_storage`
#[test]
fn usage_record_of_an_early_failure_identifies_its_subject_kind() {
    // The configuration selection fails before sources are prepared or a
    // provider is connected.
    let fixture = Fixture::new("http://127.0.0.1:1");
    let output = fixture
        .command()
        .args(["author", SUBJECT, "--force", "--config", "missing"])
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    let directory = fixture.state.join("habeas/authoring/usage");
    let files = fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(files.len(), 1, "{output:?}");
    let usage: Value = serde_json::from_slice(&fs::read(&files[0]).unwrap()).unwrap();
    assert_eq!(usage["subject"], SUBJECT);
    assert_eq!(usage["subject_kind"], "requirement", "{usage}");
    assert_eq!(usage["outcome"], "failed", "{usage}");
    assert_eq!(usage["evidence_written"], false, "{usage}");
    assert!(usage["end_time_ms"].as_u64().is_some(), "{usage}");
}

// {test}`requirements.authoring.usage_storage`
#[test]
fn usage_state_inside_the_project_is_refused_before_creating_it() {
    let fixture = Fixture::new("http://127.0.0.1:1");
    let inside = fixture.project.join("state");
    for args in [vec!["author", SUBJECT], vec!["author", SUBJECT, "--stdout"]] {
        let output = fixture
            .command()
            .env("XDG_STATE_HOME", &inside)
            .env("LOCALAPPDATA", &inside)
            .args(&args)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("authoring usage state must be outside the project repository"),
            "{stderr}"
        );
        assert!(output.stdout.is_empty(), "{output:?}");
        assert!(!inside.exists(), "state directory created inside project");
    }
    assert_eq!(
        fs::read_to_string(&fixture.evidence).unwrap(),
        fixture.original
    );
}

// {test}`requirements.authoring.reporting`
// {test}`requirements.authoring.invocation`
// {test}`requirements.authoring.persistence`
#[test]
fn verbose_single_author_reports_only_false_rationale_without_source_inputs() {
    for (determination, flag) in [(false, "-v"), (false, "--verbose"), (true, "-v")] {
        let rationale = "REVIEW_RATIONALE_ONLY:\n\n- The captured test misses an interesting boundary.\n- Add an assertion for that boundary.";
        let server = Server::start(vec![reply(vec![call(
            "submit",
            "submit_judgment",
            json!({"determination": determination, "rationale": rationale}),
        )])]);
        let fixture = Fixture::new(&server.endpoint);
        let output = fixture.run(&["author", SUBJECT, flag]);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(server.finish().len(), 1);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            stderr.contains(&format!("{SUBJECT}: false")),
            !determination,
            "{stderr}"
        );
        assert_eq!(stderr.contains(rationale), !determination, "{stderr}");
        assert!(!stdout.contains(rationale), "{stdout}");
        for text in [&stdout, &stderr] {
            assert!(!text.contains("SECRET_REQUIREMENT_TEXT"), "{text}");
            assert!(!text.contains("SECRET_DEFINITION_TEXT"), "{text}");
            assert!(!text.contains("pub fn ready()"), "{text}");
            assert!(!text.contains("— true"), "{text}");
            assert!(!text.contains(&format!("{SUBJECT}: true")), "{text}");
        }
        assert!(stdout.contains("Evidence:"), "{stdout}");
        assert!(stdout.contains("Usage:"), "{stdout}");
        let evidence: Value =
            serde_json::from_slice(&fs::read(&fixture.evidence).unwrap()).unwrap();
        assert_eq!(evidence["determination"], determination);
        assert_eq!(evidence["rationale"], rationale);
    }
}

// {test}`requirements.authoring.invocation`
// {test}`requirements.authoring.persistence`
// {test}`requirements.authoring.delivered_inputs`
// {test}`requirements.authoring.usage_storage`
// {test}`requirements.authoring.reporting`
// {test}`requirements.ci.schema`
#[test]
fn stdout_emits_only_evidence_json_without_changing_project_files() {
    for (determination, kind, existing_evidence, verbose) in [
        (json!(true), "true", true, false),
        (json!(false), "false", true, false),
        (json!({"FIXME":["#125"]}), "FIXME", true, false),
        (json!(true), "true", false, false),
        (json!(false), "false", true, true),
        (json!(true), "true", true, true),
    ] {
        let rationale = "The supplied sources explain this judgment.";
        let server = Server::start(vec![reply(vec![call(
            "submit",
            "submit_judgment",
            json!({"determination":determination,"rationale":rationale}),
        )])]);
        let fixture = Fixture::new(&server.endpoint);
        if !existing_evidence {
            fs::remove_dir_all(fixture.project.join("evidence")).unwrap();
        }
        let mut args = vec!["author-evidence", SUBJECT, "--stdout"];
        if verbose {
            args.push("--verbose");
        }
        let output = fixture.run(&args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(server.finish().len(), 1);
        let evidence: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(output.stdout.ends_with(b"\n"));
        assert_eq!(evidence["requirement"], REQUIREMENT);
        assert_eq!(evidence["determination"], determination);
        assert_eq!(evidence["rationale"], rationale);
        let inputs = evidence["inputs"].as_array().unwrap();
        assert!(inputs.iter().any(|input| input["requirements"] == SUBJECT));
        assert!(
            inputs
                .iter()
                .any(|input| input["definition"] == "terms.readiness")
        );
        assert!(
            inputs
                .iter()
                .any(|input| input["file"] == "src/lib.rs" && input["item"].is_string())
        );
        // The record has exactly the schema's fields, and outline inputs carry
        // only their file, selector, and hash, never source text.
        let keys = |value: &Value| {
            let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
            keys.sort();
            keys
        };
        assert_eq!(
            keys(&evidence),
            [
                "determination",
                "inputs",
                "rationale",
                "references",
                "requirement"
            ]
        );
        for input in inputs.iter().filter(|input| input.get("item").is_some()) {
            assert_eq!(keys(input), ["file", "hash", "item"], "{input}");
        }
        if existing_evidence {
            assert_eq!(
                fs::read_to_string(&fixture.evidence).unwrap(),
                fixture.original
            );
        } else {
            assert!(!fixture.project.join("evidence").exists());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(&format!("{SUBJECT}: ollama / {MODEL}")),
            "{stderr}"
        );
        assert!(!stderr.contains("— true"), "{stderr}");
        assert!(!stderr.contains(&format!("{SUBJECT}: true")), "{stderr}");
        assert_eq!(
            stderr.contains(&format!("{SUBJECT}: false")),
            kind == "false",
            "{stderr}"
        );
        if kind == "FIXME" {
            assert!(stderr.contains("— FIXME"), "{stderr}");
        }
        assert_eq!(
            stderr.contains(rationale),
            verbose && kind == "false",
            "{stderr}"
        );
        assert!(!stderr.contains("SECRET_REQUIREMENT_TEXT"), "{stderr}");
        assert!(!stderr.contains("SECRET_DEFINITION_TEXT"), "{stderr}");
        assert!(stderr.contains("Evidence: stdout\n"));
        let usage_path = stderr
            .lines()
            .find_map(|line| line.strip_prefix("Usage: "))
            .expect("stderr reports usage location");
        assert!(std::path::Path::new(usage_path).is_file());
        let usage = fixture.usage();
        assert_eq!(usage["outcome"], "completed");
        assert_eq!(usage["determination"], kind);
        assert_eq!(usage["evidence_output"], "stdout");
        assert_eq!(usage["evidence_written"], true);
    }
}

// {test}`requirements.authoring.submission_count`
// {test}`requirements.authoring.submission`
// {test}`requirements.authoring.usage_storage`
#[test]
fn missing_duplicate_and_invalid_submissions_preserve_existing_evidence() {
    let valid =
        json!({"determination":true,"rationale":"The reviewed sources support the subject."});
    for calls in [
        vec![],
        vec![
            call("one", "submit_judgment", valid.clone()),
            call("two", "submit_judgment", valid),
        ],
        vec![call(
            "bad",
            "submit_judgment",
            json!({"determination":true,"rationale":""}),
        )],
        vec![call(
            "empty",
            "submit_judgment",
            json!({"determination":{"FIXME":[]},"rationale":"No gap identified."}),
        )],
    ] {
        for stdout in [false, true] {
            let server = Server::start(vec![reply(calls.clone())]);
            let fixture = Fixture::new(&server.endpoint);
            let mut args = vec!["author-evidence", SUBJECT];
            if stdout {
                args.push("--stdout");
            }
            let output = fixture.run(&args);
            assert!(!output.status.success());
            assert!(output.stdout.is_empty());
            server.finish();
            assert_eq!(
                fs::read_to_string(&fixture.evidence).unwrap(),
                fixture.original
            );
            let usage = fixture.usage();
            assert_eq!(usage["outcome"], "failed");
            assert_eq!(
                usage["evidence_output"],
                if stdout { "stdout" } else { "file" }
            );
            assert_eq!(usage["evidence_written"], false);
            assert_eq!(usage["request_count"], 1);
            assert!(usage["determination"].is_null());
        }
    }
}

// {test}`requirements.authoring.deterministic_validation`
// {test}`requirements.configuration.loading`
#[test]
fn ordinary_status_ignores_invalid_user_authoring_configuration() {
    let fixture = Fixture::new("http://127.0.0.1:1");
    fs::write(
        fixture.config.join("habeas/config.toml"),
        "this is not TOML [",
    )
    .unwrap();
    let output = fixture.run(&["status", SUBJECT, "--all"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains(SUBJECT));
    assert!(!fixture.state.exists());
    assert_eq!(
        fs::read_to_string(&fixture.evidence).unwrap(),
        fixture.original
    );
}

// {test}`requirements.authoring.deterministic_validation`
#[test]
fn missing_or_stale_evidence_fails_validation_without_contacting_the_provider() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let fixture = Fixture::new(&format!("http://{}", listener.local_addr().unwrap()));
    let changed = format!(":::{{req}} ready\n{REQUIREMENT} Changed.\n:::\n");
    for (case, expected) in [("missing", "missing for requirement"), ("stale", "changed")] {
        if case == "missing" {
            fs::remove_file(&fixture.evidence).unwrap();
        } else {
            fs::write(&fixture.evidence, &fixture.original).unwrap();
            fs::write(fixture.project.join("md/requirements.md"), &changed).unwrap();
        }
        let report = habeas::validate_repository(&fixture.project).unwrap();
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.contains(SUBJECT) && error.contains(expected)),
            "{case}: {:?}",
            report.errors
        );
        // Status and book rendering need no authoring credentials.
        let status = fixture
            .command()
            .env_remove("HABEAS_TEST_MODEL_KEY")
            .args(["status", "--all"])
            .output()
            .unwrap();
        assert!(status.status.success(), "{case}: {status:?}");
        assert!(
            String::from_utf8_lossy(&status.stdout).contains(SUBJECT),
            "{case}: {status:?}"
        );

        let mut book = mdbook_preprocessor::book::Book::new();
        book.push_item(mdbook_preprocessor::book::Chapter::new(
            "Requirements",
            fs::read_to_string(fixture.project.join("md/requirements.md")).unwrap(),
            "requirements.md",
            Vec::new(),
        ));
        let context = mdbook_preprocessor::PreprocessorContext::new(
            fixture.project.clone(),
            mdbook_preprocessor::config::Config::default(),
            "html".to_owned(),
        );
        let mut render = fixture
            .command()
            .env_remove("HABEAS_TEST_MODEL_KEY")
            .arg("mdbook")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        serde_json::to_writer(render.stdin.take().unwrap(), &(&context, &book)).unwrap();
        let rendered = render.wait_with_output().unwrap();
        assert!(rendered.status.success(), "{case}: {rendered:?}");
        let rendered: Value = serde_json::from_slice(&rendered.stdout).unwrap();
        assert!(rendered.to_string().contains(SUBJECT), "{case}");
    }
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
        "validation, status, or rendering contacted the provider"
    );
    assert!(!fixture.state.exists(), "no authoring usage was recorded");
    assert_eq!(
        fs::read_to_string(&fixture.evidence).unwrap(),
        fixture.original,
        "validation did not author evidence"
    );
}

// {test}`requirements.authoring.provider`
// {test}`requirements.authoring.usage_storage`
// {test}`requirements.authoring.rig_configuration_mapping`
// {test}`requirements.authoring.configuration_precedence`
// {test}`requirements.authoring.initial_prompt`
// {test}`requirements.authoring.usage_requests`
// {test}`requirements.authoring.configuration_storage`
// {test}`requirements.authoring.source_tools`
// {test}`requirements.authoring.submission`
// {test}`requirements.authoring.delivered_inputs`
#[test]
fn bedrock_cli_uses_selected_profile_region_and_converse_tools() {
    for typed in [false, true] {
        let mut read_call = json!({"toolUseId":"bedrock-read","name":"habeas",
        "input":{"file":"extra.txt","line_start":1,"line_end":2}});
        let mut submit_call = json!({"toolUseId":"bedrock-submit","name":"submit_judgment",
        "input":{"determination":true,"rationale":"The supplied sources support the subject."}});
        if typed {
            read_call["type"] = json!("tool_use");
            submit_call["type"] = json!("tool_use");
        }
        let first_message = json!({"role":"assistant","content":[
            {"text":"Reading source."},{"toolUse":read_call}
        ]});
        let server = Server::start(vec![
            json!({
                "output":{"message":first_message.clone()},
                "stopReason":"tool_use",
                "usage":{"inputTokens":19,"outputTokens":5,"totalTokens":24},
                "metrics":{"latencyMs":1}
            }),
            json!({
                "output":{"message":{"role":"assistant","content":[{"toolUse":submit_call}]}},
                "stopReason":"tool_use",
                "usage":{"inputTokens":19,"outputTokens":5,"totalTokens":24},
                "metrics":{"latencyMs":1}
            }),
        ]);
        let fixture = Fixture::new(&server.endpoint);
        let credentials = fixture.temp.path().join("aws-credentials");
        let aws_config = fixture.temp.path().join("aws-config");
        fs::write(&credentials, "[default]\naws_access_key_id = WRONG_DEFAULT_ACCESS_KEY\naws_secret_access_key = wrong-default-secret\n[habeas-test]\naws_access_key_id = FAKE_PROFILE_ACCESS_KEY\naws_secret_access_key = fake-profile-secret-for-local-test-only\n").unwrap();
        fs::write(&aws_config, "[profile habeas-test]\nregion = eu-west-1\n").unwrap();
        fs::write(fixture.config.join("habeas/config.toml"), "version = 1\n[authoring]\ndefault = 'cloud'\n[authoring.configs.cloud]\npolicy = 'once'\nbackend = 'bedrock'\nmodel = 'fixture-bedrock-model'\naws_profile = 'habeas-test'\nregion = 'us-west-2'\n[authoring.configs.cloud.options]\ntemperature = 0.25\nmaxTokens = 321\ntopP = 0.75\nstopSequences = ['END']\ntop_k = 12\n").unwrap();
        let mut command = fixture.command();
        for key in [
            "AWS_ACCESS_KEY_ID",
            "AWS_SECRET_ACCESS_KEY",
            "AWS_SESSION_TOKEN",
            "AWS_SECURITY_TOKEN",
            "AWS_PROFILE",
            "AWS_DEFAULT_PROFILE",
            "AWS_REGION",
            "AWS_DEFAULT_REGION",
            "AWS_ENDPOINT_URL",
            "AWS_IGNORE_CONFIGURED_ENDPOINT_URLS",
            "AWS_BEARER_TOKEN_BEDROCK",
        ] {
            command.env_remove(key);
        }
        let output = command
            .env("AWS_CONFIG_FILE", aws_config)
            .env("AWS_SHARED_CREDENTIALS_FILE", credentials)
            .env("AWS_ENDPOINT_URL_BEDROCK_RUNTIME", &server.endpoint)
            .env("AWS_EC2_METADATA_DISABLED", "true")
            .args(["author-evidence", SUBJECT])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let requests = server.finish();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1].body["messages"][1], first_message);
        let result = &requests[1].body["messages"][2]["content"][0]["toolResult"];
        assert_eq!(result["toolUseId"], "bedrock-read");
        let delivered = result["content"][0]["text"].as_str().unwrap();
        assert!(delivered.contains("extra.txt") && delivered.contains("context line 1"));
        assert!(
            requests[0]
                .headers
                .starts_with("POST /model/fixture-bedrock-model/converse HTTP/1.1\r\n")
        );
        assert!(
            requests[0]
                .headers
                .contains("Credential=FAKE_PROFILE_ACCESS_KEY/")
        );
        assert!(
            requests[0]
                .headers
                .contains("/us-west-2/bedrock/aws4_request")
        );
        assert!(!requests[0].headers.contains("WRONG_DEFAULT_ACCESS_KEY"));
        let body = &requests[0].body;
        assert_eq!(
            body["inferenceConfig"],
            json!({"temperature":0.25,"maxTokens":321,"topP":0.75,"stopSequences":["END"]})
        );
        assert_eq!(body["additionalModelRequestFields"], json!({"top_k":12}));
        let prompt = prompt_text(body);
        assert!(
            prompt.contains("Assess whether a project's source code")
                && prompt.contains(REQUIREMENT)
        );
        let tool_specs = body["toolConfig"]["tools"].as_array().unwrap();
        for tool in tool_specs {
            let schema = &tool["toolSpec"]["inputSchema"]["json"];
            assert_eq!(schema["type"], "object");
            for unsupported in ["oneOf", "allOf", "anyOf"] {
                assert!(
                    schema.get(unsupported).is_none(),
                    "Bedrock rejects {unsupported} at the root of {}'s input schema",
                    tool["toolSpec"]["name"]
                );
            }
        }
        let habeas_schema = &tool_specs
            .iter()
            .find(|tool| tool["toolSpec"]["name"] == "habeas")
            .unwrap()["toolSpec"]["inputSchema"]["json"];
        assert_eq!(habeas_schema["additionalProperties"], false);
        for field in ["file", "item", "requirements", "definition"] {
            assert_eq!(habeas_schema["properties"][field]["type"], "string");
        }
        for field in ["line_start", "line_end"] {
            assert_eq!(habeas_schema["properties"][field]["type"], "integer");
            assert_eq!(habeas_schema["properties"][field]["minimum"], 1);
        }
        let judgment_schema = &tool_specs
            .iter()
            .find(|tool| tool["toolSpec"]["name"] == "submit_judgment")
            .unwrap()["toolSpec"]["inputSchema"]["json"]["properties"]["determination"];
        let outcomes = judgment_schema["oneOf"].as_array().unwrap();
        assert!(outcomes.iter().any(|outcome| outcome["type"] == "boolean"));
        let fixme = &outcomes
            .iter()
            .find(|outcome| outcome["type"] == "object")
            .unwrap()["properties"]["FIXME"];
        assert_eq!(fixme["type"], "array");
        assert_eq!(fixme["minItems"], 1);
        assert_eq!(fixme["items"]["type"], "string");
        assert_eq!(fixme["items"]["minLength"], 1);
        let mut tools = tool_specs
            .iter()
            .map(|tool| tool["toolSpec"]["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        tools.sort();
        assert_eq!(
            tools,
            [
                "ast_grep",
                "github",
                "habeas",
                "list_files",
                "next_page",
                "outline",
                "references",
                "ripgrep",
                "submit_judgment"
            ]
        );
        let evidence: Value =
            serde_json::from_slice(&fs::read(&fixture.evidence).unwrap()).unwrap();
        assert_eq!(evidence["determination"], true);
        assert!(
            evidence["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|input| input["file"] == "extra.txt")
                .collect::<Vec<_>>()
                == habeas::outline_inputs_for_lines(&fixture.project, "extra.txt", 1, 2)
                    .unwrap()
                    .iter()
                    .collect::<Vec<_>>()
        );
        let usage = fixture.usage();
        assert_eq!(usage["backend"], "bedrock");
        assert_eq!(usage["model"], "fixture-bedrock-model");
        assert_eq!(usage["tokens"]["input"], 38);
        assert_eq!(usage["tokens"]["output"], 10);
        assert_eq!(usage["request_count"], 2);
        assert_eq!(usage["tool_calls"]["habeas"], 1);
        assert_eq!(usage["tool_calls"]["submit_judgment"], 1);
        assert_eq!(usage["outcome"], "completed");
    }
}

/// Runs one Bedrock assessment that reads a file and then submits, against
/// model `model`, returning the two request bodies and the usage record.
// {cite}`habeas:util`
fn bedrock_run(model: &str) -> (Vec<Value>, Value) {
    let server = Server::start(vec![
        json!({
            "output":{"message":{"role":"assistant","content":[
                {"reasoningContent":{"reasoningText":{"text":"Read the extra source.","signature":"sig"}}},
                {"toolUse":
                {"toolUseId":"read","name":"habeas","input":{"file":"extra.txt","line_start":1}}}]}},
            "stopReason":"tool_use",
            "usage":{"inputTokens":7,"outputTokens":5,"totalTokens":1012,
                "cacheReadInputTokens":0,"cacheWriteInputTokens":1000},
            "metrics":{"latencyMs":1}
        }),
        json!({
            "output":{"message":{"role":"assistant","content":[{"toolUse":
                {"toolUseId":"submit","name":"submit_judgment","input":
                    {"determination":true,"rationale":"The supplied sources support the subject."}}}]}},
            "stopReason":"tool_use",
            "usage":{"inputTokens":9,"outputTokens":5,"totalTokens":1214,
                "cacheReadInputTokens":1000,"cacheWriteInputTokens":200},
            "metrics":{"latencyMs":1}
        }),
    ]);
    let fixture = Fixture::new(&server.endpoint);
    fs::write(
        fixture.config.join("habeas/config.toml"),
        format!("version = 1\n[authoring]\ndefault = 'cloud'\n[authoring.configs.cloud]\npolicy = 'once'\nbackend = 'bedrock'\nmodel = '{model}'\nregion = 'us-west-2'\n"),
    )
    .unwrap();
    let mut command = fixture.command();
    for key in [
        "AWS_SESSION_TOKEN",
        "AWS_SECURITY_TOKEN",
        "AWS_PROFILE",
        "AWS_DEFAULT_PROFILE",
        "AWS_ENDPOINT_URL",
        "AWS_IGNORE_CONFIGURED_ENDPOINT_URLS",
        "AWS_BEARER_TOKEN_BEDROCK",
    ] {
        command.env_remove(key);
    }
    let output = command
        .env("AWS_CONFIG_FILE", fixture.temp.path().join("no-aws-config"))
        .env(
            "AWS_SHARED_CREDENTIALS_FILE",
            fixture.temp.path().join("no-aws-credentials"),
        )
        .env("AWS_ACCESS_KEY_ID", "FAKE_ACCESS_KEY")
        .env("AWS_SECRET_ACCESS_KEY", "fake-secret")
        .env("AWS_ENDPOINT_URL_BEDROCK_RUNTIME", &server.endpoint)
        .env("AWS_EC2_METADATA_DISABLED", "true")
        .args(["author", SUBJECT])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bodies = server
        .finish()
        .into_iter()
        .map(|request| request.body)
        .collect();
    (bodies, fixture.usage())
}

/// Removes every `cachePoint` block from a Converse request body.
// {cite}`habeas:util`
fn without_cache_points(body: &Value) -> Value {
    let mut body = body.clone();
    let strip = |blocks: &mut Value| {
        if let Some(blocks) = blocks.as_array_mut() {
            blocks.retain(|block| block.get("cachePoint").is_none());
        }
    };
    strip(&mut body["system"]);
    for message in body["messages"].as_array_mut().unwrap() {
        strip(&mut message["content"]);
    }
    body
}

// {test}`requirements.authoring.cache_checkpoints`
// {test}`requirements.authoring.usage_requests`
// {test}`requirements.authoring.prompt_parts`
#[test]
fn claude_requests_carry_cache_checkpoints_and_record_cache_usage() {
    let checkpoint = json!({"cachePoint":{"type":"default"}});
    let (cached, usage) = bedrock_run("us.anthropic.claude-sonnet-4-5-20250929-v1:0");
    assert_eq!(cached.len(), 2);
    for body in &cached {
        // One checkpoint follows the system prompt...
        let system = body["system"].as_array().unwrap();
        assert_eq!(system.len(), 2, "{body}");
        assert!(system[0]["text"].as_str().unwrap().starts_with("# Task"));
        assert_eq!(system[1], checkpoint);
        // ...and one ends the last message, and there are no others.
        let messages = body["messages"].as_array().unwrap();
        let last = messages.last().unwrap()["content"].as_array().unwrap();
        assert_eq!(last.last().unwrap(), &checkpoint, "{body}");
        assert_eq!(body.to_string().matches("cachePoint").count(), 2, "{body}");
    }
    assert_eq!(cached[1]["messages"].as_array().unwrap().len(), 3);
    // The second request follows a reasoning turn and still ends in a
    // checkpoint.
    assert!(
        cached[1]["messages"][1]
            .to_string()
            .contains("reasoningContent")
    );
    // The checkpoints change nothing else: a model without caching gets the
    // same request without them.
    let (plain, _) = bedrock_run("fixture-uncached-model");
    assert!(!plain[0].to_string().contains("cachePoint"));
    for (cached, plain) in cached.iter().zip(&plain) {
        assert_eq!(&without_cache_points(cached), plain);
    }
    // Cache reads and writes are recorded per request and in total.
    let requests = usage["requests"].as_array().unwrap();
    assert_eq!(requests[0]["tokens"]["cached_input"], 0);
    assert_eq!(requests[0]["tokens"]["cache_creation_input"], 1000);
    assert_eq!(requests[1]["tokens"]["cached_input"], 1000);
    assert_eq!(requests[1]["tokens"]["cache_creation_input"], 200);
    assert_eq!(usage["tokens"]["cached_input"], 1000);
    assert_eq!(usage["tokens"]["cache_creation_input"], 1200);
}

// {test}`requirements.authoring.persistence`
// {test}`requirements.authoring.usage_storage`
#[test]
fn failed_evidence_write_keeps_the_accepted_judgment_in_usage() {
    let fixture = Fixture::new("http://unconfigured");
    let evidence_path = fixture.evidence.clone();
    let server = Server::start_with(
        vec![reply(vec![call(
            "submit",
            "submit_judgment",
            json!({
                "determination":{"FIXME":["#125"]},"rationale":"The source documents the remaining gap."
            }),
        )])],
        move || {
            fs::remove_file(&evidence_path).unwrap();
            fs::create_dir(&evidence_path).unwrap();
        },
    );
    let config_path = fixture.config.join("habeas/config.toml");
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace("http://unconfigured", &server.endpoint);
    fs::write(config_path, config).unwrap();
    let output = fixture.run(&["author-evidence", SUBJECT]);
    assert!(!output.status.success());
    assert_eq!(server.finish().len(), 1);
    assert!(fixture.evidence.is_dir());
    let usage = fixture.usage();
    assert_eq!(usage["outcome"], "failed");
    assert_eq!(usage["determination"], "FIXME");
    assert_eq!(usage["evidence_written"], false);
    assert_eq!(usage["request_count"], 1);
    assert_eq!(usage["requests"][0]["outcome"], "completed");
}

/// Answers one request with an HTTP error status and returns its JSON body.
// {cite}`habeas:util`
fn reject_once(status: &'static str, body: &'static str) -> (String, JoinHandle<Value>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let thread = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut reader = BufReader::new(&mut stream);
        let mut length = 0;
        loop {
            let mut line = String::new();
            assert!(reader.read_line(&mut line).unwrap() > 0);
            if line == "\r\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':')
                && name.eq_ignore_ascii_case("content-length")
            {
                length = value.trim().parse().unwrap();
            }
        }
        let mut bytes = vec![0; length];
        reader.read_exact(&mut bytes).unwrap();
        write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        serde_json::from_slice(&bytes).unwrap()
    });
    (endpoint, thread)
}

// {test}`requirements.authoring.configuration_storage`
#[test]
fn provider_rejection_of_inference_options_is_reported() {
    let (endpoint, server) = reject_once(
        "400 Bad Request",
        r#"{"error":"PROVIDER_REJECTED_OPTION num_ctx"}"#,
    );
    let fixture = Fixture::new(&endpoint);
    let output = fixture.run(&["author", SUBJECT]);
    assert!(!output.status.success());
    let body = server.join().unwrap();
    // The configured options were passed through to the provider unchanged.
    assert_eq!(body["options"]["num_ctx"], 4096);
    assert_eq!(body["options"]["temperature"], 0.25);
    assert_eq!(body["model"], MODEL);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("PROVIDER_REJECTED_OPTION"), "{stderr}");
    assert_eq!(
        fs::read_to_string(&fixture.evidence).unwrap(),
        fixture.original
    );
    assert_eq!(fixture.usage()["outcome"], "failed");

    // Rig-side rejection of a Bedrock option is reported before any request.
    let fixture = Fixture::new("http://unused");
    fs::write(
        fixture.config.join("habeas/config.toml"),
        "version = 1\n[authoring]\ndefault = 'cloud'\n[authoring.configs.cloud]\npolicy = 'once'\nbackend = 'bedrock'\nmodel = 'm'\nregion = 'us-west-2'\n[authoring.configs.cloud.options]\ntemperature = 'hot'\n",
    )
    .unwrap();
    let output = fixture
        .command()
        .env("AWS_ACCESS_KEY_ID", "FAKE")
        .env("AWS_SECRET_ACCESS_KEY", "fake")
        .env("AWS_ENDPOINT_URL_BEDROCK_RUNTIME", "http://127.0.0.1:9")
        .env("AWS_EC2_METADATA_DISABLED", "true")
        .args(["author", SUBJECT])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Bedrock options.temperature must be a number"),
        "{stderr}"
    );
    assert_eq!(
        fs::read_to_string(&fixture.evidence).unwrap(),
        fixture.original
    );
}

/// Returns the first request's prompt and its sorted tool names and descriptions.
// {test}`requirements.authoring.prompt_parts`
fn prompt_and_tools(body: &Value, bedrock: bool) -> (String, Vec<(String, String)>) {
    let (tools, spec) = if bedrock {
        (&body["toolConfig"]["tools"], "toolSpec")
    } else {
        (&body["tools"], "function")
    };
    let mut tools = tools
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| {
            (
                tool[spec]["name"].as_str().unwrap().to_owned(),
                tool[spec]["description"].as_str().unwrap().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    tools.sort();
    (prompt_text(body), tools)
}

// {test}`requirements.authoring.rig_configuration_mapping`
// {test}`requirements.authoring.provider`
#[test]
fn both_adapters_send_the_same_prompt_and_tools_with_default_connections() {
    let submit =
        json!({"determination":true,"rationale":"The supplied sources support the subject."});

    // Ollama with no authentication configured sends no credential.
    let server = Server::start(vec![reply(vec![call(
        "submit",
        "submit_judgment",
        submit.clone(),
    )])]);
    let fixture = Fixture::new(&server.endpoint);
    fs::write(
        fixture.config.join("habeas/config.toml"),
        format!(
            "version = 1\n[authoring]\ndefault = 'local'\n[authoring.configs.local]\npolicy = 'once'\nbackend = 'ollama'\nmodel = '{MODEL}'\nendpoint = '{}'\n",
            server.endpoint
        ),
    )
    .unwrap();
    let output = fixture.run(&["author", SUBJECT]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let requests = server.finish();
    assert_eq!(requests.len(), 1);
    assert!(
        !requests[0]
            .headers
            .to_ascii_lowercase()
            .contains("authorization:")
    );
    assert_eq!(requests[0].body["model"], MODEL);
    let ollama = prompt_and_tools(&requests[0].body, false);

    // Bedrock without a profile or region uses the SDK default chain.
    let server = Server::start(vec![json!({
        "output":{"message":{"role":"assistant","content":[{"toolUse":
            {"toolUseId":"submit","name":"submit_judgment","input":submit}}]}},
        "stopReason":"tool_use",
        "usage":{"inputTokens":1,"outputTokens":1,"totalTokens":2},
        "metrics":{"latencyMs":1}
    })]);
    let fixture = Fixture::new(&server.endpoint);
    fs::write(
        fixture.config.join("habeas/config.toml"),
        "version = 1\n[authoring]\ndefault = 'cloud'\n[authoring.configs.cloud]\npolicy = 'once'\nbackend = 'bedrock'\nmodel = 'default-chain-model'\n",
    )
    .unwrap();
    let mut command = fixture.command();
    for key in [
        "AWS_SESSION_TOKEN",
        "AWS_SECURITY_TOKEN",
        "AWS_PROFILE",
        "AWS_DEFAULT_PROFILE",
        "AWS_DEFAULT_REGION",
        "AWS_ENDPOINT_URL",
        "AWS_IGNORE_CONFIGURED_ENDPOINT_URLS",
        "AWS_BEARER_TOKEN_BEDROCK",
    ] {
        command.env_remove(key);
    }
    let output = command
        .env("AWS_CONFIG_FILE", fixture.temp.path().join("no-aws-config"))
        .env(
            "AWS_SHARED_CREDENTIALS_FILE",
            fixture.temp.path().join("no-aws-credentials"),
        )
        .env("AWS_ACCESS_KEY_ID", "DEFAULT_CHAIN_ACCESS_KEY")
        .env("AWS_SECRET_ACCESS_KEY", "default-chain-secret")
        .env("AWS_REGION", "ap-south-1")
        .env("AWS_ENDPOINT_URL_BEDROCK_RUNTIME", &server.endpoint)
        .env("AWS_EC2_METADATA_DISABLED", "true")
        .args(["author", SUBJECT])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let requests = server.finish();
    assert_eq!(requests.len(), 1);
    let headers = &requests[0].headers;
    assert!(headers.starts_with("POST /model/default-chain-model/converse HTTP/1.1\r\n"));
    assert!(headers.contains("Credential=DEFAULT_CHAIN_ACCESS_KEY/"));
    assert!(headers.contains("/ap-south-1/bedrock/aws4_request"));
    let bedrock = prompt_and_tools(&requests[0].body, true);

    // Both fixtures are identical projects, so the requests must match.
    assert_eq!(ollama, bedrock);
    assert_eq!(ollama.1.len(), 9);
}

/// Asserts that authoring failed before or while recording usage history and
/// did not report a recorded success.
// {cite}`habeas:util`
fn assert_unrecorded_failure(output: &Output, expected: &str) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stdout}{stderr}");
    assert!(stderr.contains(expected), "{stderr}");
    assert!(!stdout.contains("Usage:"), "{stdout}");
    assert!(!stdout.contains("1 authored"), "{stdout}");
}

// {test}`requirements.configuration.state_location`
#[cfg(unix)]
#[test]
fn undeterminable_unwritable_or_project_state_fails_before_a_model_request() {
    let server = Server::start(Vec::new());
    let fixture = Fixture::new(&server.endpoint);
    for args in [
        &["author", SUBJECT, "--config", "mock"][..],
        &["author", "--all", "--config", "mock"][..],
    ] {
        let output = fixture
            .command()
            .env_remove("XDG_STATE_HOME")
            .env_remove("HOME")
            .args(args)
            .output()
            .unwrap();
        assert_unrecorded_failure(&output, "cannot determine Habeas");

        let blocked = fixture.temp.path().join("state-file");
        fs::write(&blocked, "not a directory").unwrap();
        let output = fixture
            .command()
            .env("XDG_STATE_HOME", &blocked)
            .args(args)
            .output()
            .unwrap();
        assert_unrecorded_failure(&output, "cannot create authoring state directory");
        assert_eq!(fs::read_to_string(&blocked).unwrap(), "not a directory");

        let inside = fixture.project.join("state");
        let output = fixture
            .command()
            .env("XDG_STATE_HOME", &inside)
            .args(args)
            .output()
            .unwrap();
        assert_unrecorded_failure(&output, "must be outside the project repository");
        assert!(!inside.join("habeas/authoring").exists());
    }
    assert!(server.finish().is_empty());
    assert_eq!(
        fs::read_to_string(&fixture.evidence).unwrap(),
        fixture.original
    );
}

// {test}`requirements.configuration.state_location`
#[cfg(unix)]
#[test]
fn state_reaching_the_project_through_a_symlink_is_refused() {
    let server = Server::start(Vec::new());
    let fixture = Fixture::new(&server.endpoint);
    // A state root that is itself a symlink into the project, and one whose
    // nearest existing ancestor is such a symlink.
    let alias = fixture.temp.path().join("state-alias");
    std::os::unix::fs::symlink(&fixture.project, &alias).unwrap();
    for state in [alias.clone(), alias.join("not/yet/created")] {
        let output = fixture
            .command()
            .env("XDG_STATE_HOME", &state)
            .args(["author", SUBJECT, "--config", "mock"])
            .output()
            .unwrap();
        assert_unrecorded_failure(&output, "must be outside the project repository");
        assert!(!fixture.project.join("habeas").exists());
        assert!(!fixture.project.join("not").exists());
    }
    assert!(server.finish().is_empty());
}

// {test}`requirements.configuration.state_location`
#[cfg(unix)]
#[test]
fn usage_write_failures_are_reported_without_claiming_recorded_history() {
    use std::os::unix::fs::PermissionsExt;
    for args in [&["author", SUBJECT][..], &["author", "--all"][..]] {
        let fixture = Fixture::new("http://unconfigured");
        let usage = fixture.state.join("habeas/authoring/usage");
        let server = Server::start_with(
            vec![reply(vec![call(
                "submit",
                "submit_judgment",
                json!({"determination":true,"rationale":"The supplied sources support the subject."}),
            )])],
            move || {
                for entry in fs::read_dir(&usage).unwrap() {
                    let path = entry.unwrap().path();
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
                }
            },
        );
        let config_path = fixture.config.join("habeas/config.toml");
        let config = fs::read_to_string(&config_path)
            .unwrap()
            .replace("http://unconfigured", &server.endpoint);
        fs::write(config_path, config).unwrap();
        let output = fixture.command().args(args).output().unwrap();
        assert_eq!(server.finish().len(), 1);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{stderr}");
        assert!(stderr.contains("cannot record"), "{stderr}");
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains("Usage:"),
            "{output:?}"
        );
    }
}

// {test}`requirements.authoring.configuration_precedence`
// {test}`requirements.authoring.credentials`
#[test]
fn missing_or_empty_api_key_env_fails_before_any_model_request() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let fixture = Fixture::new(&format!("http://{}", listener.local_addr().unwrap()));
    for value in [None, Some("")] {
        let mut command = fixture.command();
        match value {
            None => command.env_remove("HABEAS_TEST_MODEL_KEY"),
            Some(value) => command.env("HABEAS_TEST_MODEL_KEY", value),
        };
        let output = command.args(["author", SUBJECT]).output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{value:?}: {stderr}");
        assert!(
            stderr.contains("HABEAS_TEST_MODEL_KEY"),
            "{value:?}: {stderr}"
        );
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
            "{value:?}: the model endpoint was contacted"
        );
        assert_eq!(
            fs::read_to_string(&fixture.evidence).unwrap(),
            fixture.original
        );
    }
}

// {test}`requirements.authoring.mandatory_inputs`
#[test]
fn unresolvable_mandatory_inputs_fail_before_any_model_request() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let fixture = Fixture::new(&format!("http://{}", listener.local_addr().unwrap()));
    // The subject's own declaration is a mandatory input that cannot be read.
    fs::write(
        fixture.project.join("habeas.toml"),
        "version = 1\n[evidence]\ndisallow = ['md/requirements.md']\n",
    )
    .unwrap();
    let output = fixture.run(&["author", SUBJECT]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("md/requirements.md"), "{stderr}");
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
        "the model endpoint was contacted"
    );
    assert_eq!(
        fs::read_to_string(&fixture.evidence).unwrap(),
        fixture.original
    );
}

// {test}`requirements.cli.distributed_guidance`
#[test]
fn bundled_skills_guide_distributed_requirements_with_do_and_dont_lists() {
    let reference =
        include_str!("../assets/skills/habeas-requirements/references/distributed-requirements.md");
    let list = |heading: &str| {
        let (_, rest) = reference.split_once(heading).unwrap();
        let list = rest.split("\n## ").next().unwrap();
        list.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    let (dos, donts) = (list("\n## Do\n"), list("\n## Don't\n"));
    for phrase in [
        "Make a requirement distributed only when its obligation applies on its own at each site; when particular code carries out the requirement as a whole, use `{impl}` there.",
        "Separate obligations each site upholds from obligations particular code carries out, and mark that code `{impl}`.",
        "check that the site queries select every place the obligation applies and no others",
        "Write a violation query, with `within` naming the site query, for every kind of construct that could break the obligation",
        "Give every query `match` and `no_match` snippets that test its conditions the way you would test code",
        "Describe sites exactly as the site queries select them. When a query relies on a convention, such as how a variable is named, state the convention as an obligation",
        "Present the queries and snippets with the requirement text when reviewing it with the user.",
        "Run `habeas author --all` to author the site reviews.",
    ] {
        assert!(dos.contains(phrase), "Do list lacks {phrase:?}");
    }
    for phrase in [
        "Don't describe sites more broadly than the queries select",
        "Don't state a per-site obligation that a reviewer cannot check from the site and the helpers it calls",
        "Don't reference the distributed requirement from a site just because it is a site; sites need none.",
        "Don't re-run a false site review for a different result; fix the site, or tighten the queries or the text",
    ] {
        assert!(donts.contains(phrase), "Don't list lacks {phrase:?}");
    }
    // The reference explains distributed requirements: what they are, the
    // two query kinds, and each query's keys.
    assert_in_order(
        "habeas-requirements",
        &[
            "# Distributed requirements",
            "A distributed requirement instead declares queries that find its sites, and Habeas reviews each site separately",
            "## Queries",
            "`:::{sites} <name>` selects the places the obligation applies",
            "`:::{violations} <name>` selects constructs likely to break it",
            "Each needs `within: <site query>`",
            "Each body has these keys: `language`",
            "`within`, required for violation queries; and `match` and `no_match` snippet lists",
        ],
    );
    assert_phrases(
        "habeas-requirements",
        &["`habeas author --all` authors a site review for each site"],
    );
    assert_phrases(
        "habeas-evidence",
        &[
            "1. If it does apply, fix the code at the site.",
            "2. If the site should not be covered, narrow the site query or tighten the text describing sites; if the obligation's text admits a reading under which the site fails, tighten that text",
            "Don't re-run a false site review for a different result.",
        ],
    );
    assert_phrases(
        "habeas-bootstrap-requirements",
        &["propose a distributed requirement instead of marking each site `{impl}`"],
    );
}

// {test}`requirements.cli.skill_structure`
#[test]
fn bundled_skills_open_with_a_workflow_and_name_their_neighbors() {
    let skills = [
        (
            "habeas-requirements",
            include_str!("../assets/skills/habeas-requirements/SKILL.md"),
        ),
        (
            "habeas-evidence",
            include_str!("../assets/skills/habeas-evidence/SKILL.md"),
        ),
        (
            "habeas-bootstrap-requirements",
            include_str!("../assets/skills/habeas-bootstrap-requirements/SKILL.md"),
        ),
    ];
    for (name, text) in skills {
        let description = text
            .lines()
            .find_map(|line| line.strip_prefix("description: "))
            .unwrap_or_else(|| panic!("{name} has no description"));
        assert!(
            description.starts_with("Use when") || description.starts_with("Use before"),
            "{name}: {description}"
        );
        for (other, _) in skills.iter().filter(|(other, _)| *other != name) {
            assert!(description.contains(other), "{name} does not name {other}");
        }
        // The first section after the title is the workflow, as ordered steps.
        let first = text.lines().find(|line| line.starts_with("## ")).unwrap();
        assert_eq!(first, "## Workflow", "{name}");
        let workflow = text.split("## Workflow\n").nth(1).unwrap();
        let workflow = workflow.split("\n## ").next().unwrap();
        let first_step = workflow
            .find("- [ ] 1. ")
            .unwrap_or_else(|| panic!("{name}"));
        let second_step = workflow
            .find("- [ ] 2. ")
            .unwrap_or_else(|| panic!("{name}"));
        assert!(first_step < second_step, "{name}");
    }

    // Each topic's home holds its section.
    let [(_, requirements), (_, evidence), (_, bootstrap)] = skills;
    for heading in [
        "## Writing requirements",
        "## Choosing references",
        "## What the review checks",
    ] {
        assert!(requirements.contains(heading), "{heading}");
    }
    for heading in [
        "### FIXME",
        "### Citation reviews",
        "### Site reviews",
        "## Disagreeing assessments",
    ] {
        assert!(evidence.contains(heading), "{heading}");
    }
    assert!(bootstrap.contains("## Proposing requirements"));

    // Each reference file is linked with a statement of when to read it.
    let collapsed = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    for (skill, link) in [
        (
            requirements,
            "read [references/distributed-requirements.md](references/distributed-requirements.md) before drafting",
        ),
        (
            requirements,
            "read [references/syntax-details.md](references/syntax-details.md)",
        ),
        (
            evidence,
            "If authoring fails on configuration, credentials, throttling, or a denied read, read [references/authoring-failures.md](references/authoring-failures.md)",
        ),
    ] {
        assert!(collapsed(skill).contains(link), "{link}");
    }
}

/// The bundled skills, by name.
// {test}`requirements.cli.behavior_review_guidance`
const SKILLS: [&str; 3] = [
    "habeas-requirements",
    "habeas-evidence",
    "habeas-bootstrap-requirements",
];

/// A bundled skill together with the reference files it links to, with line
/// wrapping collapsed so phrases can be matched across lines.
// {cite}`habeas:util`
fn bundled_skill(name: &str) -> String {
    let files: &[&str] = match name {
        "habeas-requirements" => &[
            include_str!("../assets/skills/habeas-requirements/SKILL.md"),
            include_str!(
                "../assets/skills/habeas-requirements/references/distributed-requirements.md"
            ),
            include_str!("../assets/skills/habeas-requirements/references/syntax-details.md"),
        ],
        "habeas-evidence" => &[
            include_str!("../assets/skills/habeas-evidence/SKILL.md"),
            include_str!("../assets/skills/habeas-evidence/references/authoring-failures.md"),
        ],
        "habeas-bootstrap-requirements" => &[include_str!(
            "../assets/skills/habeas-bootstrap-requirements/SKILL.md"
        )],
        _ => panic!("unknown skill {name}"),
    };
    files
        .iter()
        .flat_map(|file| file.split_whitespace())
        .collect::<Vec<_>>()
        .join(" ")
}

// {cite}`habeas:util`
fn assert_phrases(name: &str, phrases: &[&str]) {
    let text = bundled_skill(name);
    for phrase in phrases {
        assert!(text.contains(phrase), "{name} lacks {phrase:?}");
    }
}

/// Asserts that the skill contains each phrase, each after the previous one.
// {cite}`habeas:util`
fn assert_in_order(name: &str, phrases: &[&str]) {
    let text = bundled_skill(name);
    let mut from = 0;
    for phrase in phrases {
        let at = text[from..]
            .find(phrase)
            .unwrap_or_else(|| panic!("{name} lacks {phrase:?} after byte {from}"));
        from += at + phrase.len();
    }
}

// {test}`requirements.cli.behavior_review_guidance`
#[test]
fn bundled_skills_direct_behavior_review_against_requirements() {
    // habeas-requirements is the home of the review criteria: every observable
    // branch, and no unspecified behavior.
    assert_phrases(
        "habeas-requirements",
        &[
            "Every end-user-observable result, including errors and input handling, must follow from some agreed requirement",
            "check that the code adds no observable behavior beyond the requirements",
        ],
    );
    assert_phrases(
        "habeas-bootstrap-requirements",
        &[
            "distinct observable branches",
            "every observable branch has been reviewed",
        ],
    );
    // The requirement and example review criteria, including test adequacy and
    // justified absence of tests.
    assert_phrases(
        "habeas-requirements",
        &[
            "## What the review checks",
            "does not implement completely and exactly as stated",
            "code that carries out part of the requirement but that the reviewer cannot reach from its `{impl}` items and found only by searching, including code that only cites it",
            "observable behavior in `{impl}` code that no requirement implemented there specifies",
            "obligations, boundaries, and failure cases no test runs and asserts",
            "A test may be omitted only for code with no logic of its own",
            "An example's review checks that tests' setup and assertions cover its scenario and each expected outcome",
        ],
    );
    // The three outcomes and the documented-gap convention; habeas-evidence is
    // the home of FIXME judgments.
    assert_phrases(
        "habeas-requirements",
        &[
            "judges each requirement `true`, `false`, or FIXME",
            "To acknowledge a gap the user agrees to defer, use a `{fixme}` reference",
            "One acknowledged gap does not excuse others",
        ],
    );
    assert_phrases(
        "habeas-evidence",
        &[
            "A FIXME judgment, such as `{\"FIXME\": [\"rust-lang/rust#123\"]}`, means `{fixme}` references to the definition acknowledge every remaining gap",
            "// :github: rust-lang/rust#123",
            "A plain `FIXME` or `TODO` comment does not count",
            "Validation accepts it like `true` when all other checks pass",
        ],
    );
    // Reference roles, stated in habeas-requirements and referred to elsewhere.
    assert_phrases(
        "habeas-requirements",
        &[
            "designed to verify the requirement or example, use `{test}`",
            "if the item is a root of the requirement's implementation, use `{impl}`",
            "`{cite}` needs no coverage and does not count as implementing anything",
        ],
    );
    assert_phrases(
        "habeas-evidence",
        &["references by the procedure in the habeas-requirements skill"],
    );
    assert_phrases(
        "habeas-bootstrap-requirements",
        &["references by the procedure in the habeas-requirements skill"],
    );
    // Interacting requirements, and a citation alone is not an explanation.
    assert_phrases(
        "habeas-requirements",
        &[
            "If a behavior follows only from several requirements together, mark each with `{impl}` where it applies and add fenced commentary explaining how they combine",
            "a reference alone would not say why `^C` is accepted",
        ],
    );
    assert_phrases(
        "habeas-bootstrap-requirements",
        &[
            "Where one behavior follows from several requirements, mark each with `{impl}` where it applies and explain how they combine",
        ],
    );
}

// {test}`requirements.cli.behavior_review_guidance`
// {test}`requirements.cli.internal_detail_guidance`
// {test}`requirements.cli.skill_structure`
#[test]
fn bundled_skills_mark_roots_and_handle_uncovered_items() {
    const ROOT: &str = "A *root* of a requirement is an item that carries out part of it and that no other `{impl}` item for that requirement reaches";
    // habeas-requirements is the home of roots: reach goes downward only, and
    // types and test helpers follow the same rule.
    assert_in_order(
        "habeas-requirements",
        &[
            "what an item calls, the types it uses, and the implementations of trait methods it calls, and what those reach in turn",
            "its callers don't count",
            ROOT,
            "leave what it reaches, functions and types alike, without a reference to that requirement",
            "Roots are per requirement",
            "Helpers a test reaches need no reference",
            "Don't remove an existing `{impl}` from code a root reaches unless the user asks",
        ],
    );
    // Coverage comes from a reference or a current read, never a search, and
    // reads exist only after authoring.
    assert_phrases(
        "habeas-requirements",
        &[
            "If nothing gives the item a reference, and after `habeas author` nothing reads it either",
            "or by a current evidence record, citation review, or site review that read it or an outline node",
            "Search results (ripgrep and ast_grep matches) only locate code and cover nothing",
            "Reads exist only after `habeas author`, so author before treating an item as uncovered",
            "the rationale names the item that should carry `{impl}`",
        ],
    );
    // habeas-evidence handles items still uncovered after authoring, acting on
    // the coverage-loss review.
    assert_phrases(
        "habeas-evidence",
        &[
            "Leave uncovered Rust items until after step 4",
            "When a rationale names code the reviewer found only by searching, add `{impl}` to the item it names",
            "Editing an item without a reference makes every record that read it stale",
        ],
    );
    assert_in_order(
        "habeas-evidence",
        &[
            "### Uncovered Rust items",
            "reports uncovered after `habeas author --all`",
            "If `habeas status` shows a coverage-loss review beside the item, act on each finding",
            "`root`: add `{impl}` of that definition to the item",
            "`not needed`: before changing anything, determine from its rationale and the code whether the item is dead code, serves its callers, or the earlier read was mistaken",
            "Report dead code to the user instead of deleting it unasked",
            "An item that serves other code, carrying out no requirement itself, gets `` // {cite}`habeas:util` ``",
            "add references by the procedure in the habeas-requirements skill",
            "never re-run or `--force` authoring just to get an item read",
        ],
    );
    assert_phrases(
        "habeas-bootstrap-requirements",
        &[
            "`{test}` on tests and `{impl}` on roots; code a root or test reaches needs none",
            "Leave uncovered items for now",
        ],
    );
    // The other skills refer to the definition of a root rather than restate it.
    for name in ["habeas-evidence", "habeas-bootstrap-requirements"] {
        assert!(!bundled_skill(name).contains(ROOT), "{name} restates roots");
    }
}

// {test}`requirements.cli.bootstrap_skill`
#[test]
fn bootstrap_skill_is_bundled_and_directs_complete_inventories() {
    // Bundled: Symposium publishes every skill under assets/skills, and this one
    // is discoverable there by its name.
    let manifest: toml::Value =
        toml::from_str(include_str!("../Cargo.toml")).expect("Cargo.toml parses");
    let skills = manifest["package"]["metadata"]["symposium"]["skills"]
        .as_array()
        .unwrap();
    assert!(
        skills
            .iter()
            .any(|skill| skill["source"]["path"].as_str() == Some("assets/skills")),
        "{skills:?}"
    );
    let raw = include_str!("../assets/skills/habeas-bootstrap-requirements/SKILL.md");
    assert!(
        raw.starts_with(
            "---\nname: habeas-bootstrap-requirements\ndescription: Use when adopting Habeas in an existing codebase"
        ),
        "{raw}"
    );
    assert_phrases(
        "habeas-bootstrap-requirements",
        &[
            // Read the code and tests.
            "Read the existing requirements, documentation, public entry points, implementation, and tests",
            "Follow behavior through the code",
            // Map every first-party test.
            "every first-party runnable test or test case",
            "Record the behavior each test checks; fill in requirement names once they are installed",
            "Every test must end up with a `{test}` reference to a requirement or example",
            // Map every end-user-visible source unit.
            "every function, method, struct, enum, and union, listed from the source rather than from validation's uncovered items",
            "public APIs, commands, handlers, UI actions, configuration, outputs, errors, and distinct observable branches",
            "A tagged entry point doesn't mean every observable branch beneath it has a requirement",
            "Give every inventoried test and each requirement's roots their references",
        ],
    );
    // Report gaps; no claim of completeness before an independent recount.
    assert_in_order(
        "habeas-bootstrap-requirements",
        &[
            "recount both inventories from the source and tests without your lists",
            "- <item or behavior> (<file>:<line>): <reason>; <open question>",
            "Claim 100% only when both inventories have no unmapped items",
            "Don't silently exclude difficult cases",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill`
#[test]
fn bundled_skills_require_agreed_requirements_before_code() {
    // Draft IDs and exact text, review in turn, obtain agreement, install, and
    // only then code.
    assert_in_order(
        "habeas-requirements",
        &[
            "the smallest set of new or changed obligations",
            "full names, and exact fenced text",
            "Write the drafts into the chapter as uncommitted edits",
            "Review each new or changed requirement with the user in turn",
            "present its exact text",
            "point out ambiguity, obligations that are hard to test or that nothing would enforce, and conflicts with other requirements",
            "propose specific improvements to accept, revise, or reject",
            "apply the decisions before moving on",
            "no implementation or test code, and no commit of the drafts, until the user agrees",
            "a general request to build the feature does not",
            "Make sure the book contains exactly the approved text",
            "Implement, add tests",
        ],
    );
    // Substantive changes to agreed obligations.
    assert_phrases(
        "habeas-requirements",
        &[
            "If implementation shows that an approved obligation must change substantively, review the revised text with the user before coding the change",
        ],
    );
    assert_phrases(
        "habeas-evidence",
        &[
            "Review every change to an agreed requirement's text with the user first, as the habeas-requirements skill describes, even a rewording for precision",
        ],
    );
    // Inferred requirements are reviewed with the user, area by area, before
    // installation.
    assert_in_order(
        "habeas-bootstrap-requirements",
        &[
            "every requirement starts as a proposal",
            "Repeat steps 4 to 7 for each area",
            "Propose requirements for the area",
            "Review them with the user in turn as habeas-requirements step 3 describes",
            "a general request to bootstrap is not agreement to specific text",
            "don't commit a requirement or tag code against it until the user agrees to its text",
            "Install the agreed requirements in the area's chapter",
            "Tag the area's tests and root items",
        ],
    );
    // Missing or false evidence is acceptable; no placeholder evidence.
    assert_phrases(
        "habeas-evidence",
        &[
            "each missing evidence record or false judgment expected because implementation is pending",
            "Never write or edit evidence JSON, determinations, rationales, or inputs yourself",
        ],
    );
}

// {test}`requirements.authoring.coverage_loss`
// {test}`requirements.references.rust_item_coverage`
#[test]
fn replacing_a_record_reviews_items_it_no_longer_covers_and_status_shows_the_result() {
    let server = Server::start(vec![
        reply(vec![call(
            "submit",
            "submit_judgment",
            json!({"determination": true, "rationale": "Ready is implemented."}),
        )]),
        reply(vec![call(
            "review",
            "submit_coverage_review",
            json!({"findings": [{
                "definition": SUBJECT,
                "finding": "root",
                "rationale": "helper carries out\nreadiness itself.",
            }]}),
        )]),
    ]);
    let fixture = Fixture::new(&server.endpoint);
    fs::write(
        fixture.project.join("src/helper.rs"),
        "fn helper() {}\n\nfn other() {}\n",
    )
    .unwrap();
    // The record being replaced read `helper`, so it was covered.
    let read = habeas::outline_input(&fixture.project, "src/helper.rs", "fn[helper]").unwrap();
    fs::write(
        &fixture.evidence,
        json!({"requirement": REQUIREMENT, "determination": false,
            "rationale": "PREVIOUS_RATIONALE relied on helper.", "inputs": [read]})
        .to_string(),
    )
    .unwrap();
    let uncovered = "src/helper.rs:1: function helper is uncovered";
    let status = fixture.run(&["status"]);
    let text = String::from_utf8_lossy(&status.stdout).into_owned();
    assert!(!text.contains(uncovered), "{text}");

    let output = fixture.run(&["author", SUBJECT]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // The review is reported, with each definition's finding.
    assert!(
        stdout.contains(&format!(
            "Coverage-loss review of src/helper.rs fn[helper]: no longer covered\n  {SUBJECT}: root: helper carries out readiness itself."
        )),
        "{stdout}"
    );
    let requests = server.finish();
    assert_eq!(requests.len(), 2);
    // An independent conversation with the source tools and its own
    // submission tool; its mandatory inputs are the item and the
    // declaration, and its prompt gives the replaced record's rationale.
    let (system, subject) = opening(&requests[1].body);
    assert!(
        system.contains("decide whether the item is a root of the definition's"),
        "{system}"
    );
    assert!(
        subject.contains("Item under review: src/helper.rs fn[helper]"),
        "{subject}"
    );
    assert!(
        subject.contains(&format!("Definitions to decide: [\"{SUBJECT}\"]")),
        "{subject}"
    );
    assert!(
        subject.contains("PREVIOUS_RATIONALE relied on helper."),
        "{subject}"
    );
    assert!(subject.contains("fn helper() {}"), "{subject}");
    assert!(subject.contains("SECRET_REQUIREMENT_TEXT"), "{subject}");
    assert!(!subject.contains("fn other()"), "{subject}");
    let tools = requests[1].body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(tools.contains(&"habeas") && tools.contains(&"ripgrep"));
    assert!(tools.contains(&"submit_coverage_review"));
    assert!(!tools.contains(&"submit_judgment"));
    // The result is not evidence: the new record does not mention it.
    let evidence = fs::read_to_string(&fixture.evidence).unwrap();
    assert!(!evidence.contains("helper"), "{evidence}");

    // Status shows the latest result beside the uncovered item...
    let status = fixture.run(&["status"]);
    let text = String::from_utf8_lossy(&status.stdout).into_owned();
    assert!(
        text.contains(&format!(
            "{uncovered}: it has no requirement or example reference, and no evidence record or review read it\n  latest coverage-loss review:\n    {SUBJECT}: root: helper carries out readiness itself."
        )),
        "{text}"
    );
    // ...until the item is covered again.
    fs::write(
        fixture.project.join("src/helper.rs"),
        concat!(
            "// {",
            "impl}`requirements.ready`\nfn helper() {}\n\nfn other() {}\n"
        ),
    )
    .unwrap();
    let status = fixture.run(&["status"]);
    let text = String::from_utf8_lossy(&status.stdout).into_owned();
    assert!(!text.contains("coverage-loss review"), "{text}");
}

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

struct Request {
    headers: String,
    body: Value,
}

struct Server {
    endpoint: String,
    thread: JoinHandle<Vec<Request>>,
}

impl Server {
    // {test}`requirements.authoring.provider`
    // {test}`requirements.authoring.rig_configuration_mapping`
    fn start(responses: Vec<Value>) -> Self {
        Self::start_with(responses, || {})
    }

    // {test}`requirements.authoring.provider`
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

    // {test}`requirements.authoring.provider`
    fn finish(self) -> Vec<Request> {
        self.thread.join().unwrap()
    }
}

struct Fixture {
    temp: TempDir,
    project: PathBuf,
    config: PathBuf,
    state: PathBuf,
    evidence: PathBuf,
    original: String,
}

impl Fixture {
    // {test}`requirements.authoring.complete_initial_bundle`
    // {test}`requirements.authoring.persistence`
    // {test}`requirements.authoring.configuration`
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
        fs::write(project.join("src/lib.rs"), concat!("// {", "impl}`requirements.ready`\n// FIXME(#125): one acknowledged gap.\npub fn ready() {}\n// {", "test}`requirements.ready`\nfn ready_test() {}\n")).unwrap();
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
            "version = 1\n[authoring]\ndefault = 'mock'\n[authoring.configs.mock]\nbackend = 'ollama'\nmodel = '{MODEL}'\nendpoint = '{endpoint}'\napi_key_env = 'HABEAS_TEST_MODEL_KEY'\n[authoring.configs.mock.options]\ntemperature = 0.25\nnum_ctx = 4096\nthink = false\n")).unwrap();
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
    // {test}`requirements.authoring.configuration_precedence`
    // {test}`requirements.authoring.deterministic_validation`
    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }

    // {test}`requirements.authoring.invocation`
    // {test}`requirements.authoring.configuration_precedence`
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_habeas"));
        command
            .current_dir(&self.project)
            .env("XDG_CONFIG_HOME", &self.config)
            .env("XDG_STATE_HOME", &self.state)
            .env("APPDATA", &self.config)
            .env("LOCALAPPDATA", &self.state)
            .env("HOME", self.temp.path())
            .env("HABEAS_TEST_MODEL_KEY", AUTH);
        command
    }

    // {test}`requirements.authoring.usage_storage`
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

// {test}`requirements.authoring.submission`
fn call(id: &str, name: &str, arguments: Value) -> Value {
    json!({"id":id,"type":"function","function":{"name":name,"arguments":arguments}})
}

// {test}`requirements.authoring.provider`
// {test}`requirements.authoring.usage_requests`
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
            "determination":{"FIXME":["#125"]},"rationale":"SECRET_RATIONALE: src/lib.rs:2 documents the gap."}),
        )]),
    ]);
    let fixture = Fixture::new(&server.endpoint);
    let output = fixture.run(&["author-evidence", SUBJECT]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
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
    let prompt = requests[0].body["messages"][0]["content"].as_str().unwrap();
    for text in [
        "Bring your expert judgment",
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
            "habeas",
            "list_files",
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
    assert_eq!(evidence["determination"], json!({"FIXME":["#125"]}));
    assert_eq!(evidence["requirement"], REQUIREMENT);
    let inputs = evidence["inputs"].as_array().unwrap();
    assert!(
        inputs
            .iter()
            .any(|input| input["definition"] == "terms.readiness")
    );
    let extra = inputs
        .iter()
        .filter(|input| input["file"] == "extra.txt")
        .collect::<Vec<_>>();
    // The search window covers lines 1-11 and the reads lines 10-12, one
    // plain-text line node each; overlapping and repeated deliveries are
    // recorded once, by selector and hash only.
    assert_eq!(
        extra,
        habeas::outline_inputs_for_lines(&fixture.project, "extra.txt", 1, 12)
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
            "version = 1\n[authoring]\ndefault = 'workspace'\n[authoring.configs.workspace]\nbackend = 'ollama'\nmodel = 'workspace-model'\nendpoint = '{}'\napi_key = 'WORKSPACE_LITERAL_KEY'\n",
            server.endpoint
        ),
    )
    .unwrap();
    let output = fixture
        .command()
        .current_dir(&nested)
        .args(["author", SUBJECT])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
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
    let prompt = requests[0].body["messages"][0]["content"].as_str().unwrap();
    assert!(prompt.contains("Choose false for other gaps or insufficient evidence"));
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

// {test}`requirements.authoring.initial_prompt`
#[test]
fn initial_prompt_substitutes_subject_and_mandatory_inputs_into_the_template() {
    let chapter = include_str!("../md/requirements/authoring.md");
    let start = chapter
        .find("Bring your expert judgment to the task")
        .unwrap();
    let template = chapter[start..].split("\n```").next().unwrap();
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
        let prompt = requests[0].body["messages"][0]["content"].as_str().unwrap();

        let (before, after) = template.split_once("{{mandatory_inputs}}").unwrap();
        let before = before
            .replace("{{subject_kind}}", kind)
            .replace("{{subject_name}}", subject);
        assert!(
            before.contains(&format!("\nSubject: {kind} {subject}\n")),
            "{before}"
        );
        assert!(!before.contains("{{") && !after.contains("{{"));
        // Apart from the substitutions, the prompt is the template as written.
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
    for skill in [
        include_str!("../assets/skills/habeas-requirements/SKILL.md"),
        include_str!("../assets/skills/habeas-evidence/SKILL.md"),
        include_str!("../assets/skills/habeas-bootstrap-requirements/SKILL.md"),
    ] {
        assert!(skill.contains("habeas author --all"), "{skill}");
        assert!(skill.contains("must not compose"), "{skill}");
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
        let output = command.args(["author", SUBJECT]).output().unwrap();
        assert!(!output.status.success(), "{value:?}: {output:?}");
        assert!(!output.stderr.is_empty(), "{value:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{value:?}: {output:?}");
    }
    let evidence: Value = serde_json::from_slice(&fs::read(&fixture.evidence).unwrap()).unwrap();
    assert_eq!(evidence["rationale"], "covered", "only the first run wrote");
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
        fs::write(fixture.config.join("habeas/config.toml"), "version = 1\n[authoring]\ndefault = 'cloud'\n[authoring.configs.cloud]\nbackend = 'bedrock'\nmodel = 'fixture-bedrock-model'\naws_profile = 'habeas-test'\nregion = 'us-west-2'\n[authoring.configs.cloud.options]\ntemperature = 0.25\nmaxTokens = 321\ntopP = 0.75\nstopSequences = ['END']\ntop_k = 12\n").unwrap();
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
        let prompt = body["messages"][0]["content"][0]["text"].as_str().unwrap();
        assert!(prompt.contains("Bring your expert judgment") && prompt.contains(REQUIREMENT));
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
                "habeas",
                "list_files",
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
// {test}`requirements.authoring.configuration_storage`
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
        "version = 1\n[authoring]\ndefault = 'cloud'\n[authoring.configs.cloud]\nbackend = 'bedrock'\nmodel = 'm'\nregion = 'us-west-2'\n[authoring.configs.cloud.options]\ntemperature = 'hot'\n",
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
// {test}`requirements.authoring.rig_configuration_mapping`
fn prompt_and_tools(body: &Value, bedrock: bool) -> (String, Vec<(String, String)>) {
    let (prompt, tools, spec) = if bedrock {
        (
            &body["messages"][0]["content"][0]["text"],
            &body["toolConfig"]["tools"],
            "toolSpec",
        )
    } else {
        (&body["messages"][0]["content"], &body["tools"], "function")
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
    (prompt.as_str().unwrap().to_owned(), tools)
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
            "version = 1\n[authoring]\ndefault = 'local'\n[authoring.configs.local]\nbackend = 'ollama'\nmodel = '{MODEL}'\nendpoint = '{}'\n",
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
        "version = 1\n[authoring]\ndefault = 'cloud'\n[authoring.configs.cloud]\nbackend = 'bedrock'\nmodel = 'default-chain-model'\n",
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
    assert_eq!(ollama.1.len(), 6);
}

/// Asserts that authoring failed before or while recording usage history and
/// did not report a recorded success.
// {test}`requirements.configuration.state_location`
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

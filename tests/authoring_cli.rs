mod common;

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
const REQUIREMENT: &str = "SECRET_REQUIREMENT_TEXT MUST implement {term}`terms.readiness`.";
const MODEL: &str = "fixture:q4_K_M";
const AUTH: &str = "SECRET_AUTH_VALUE";

// {cite}`habeas:util`
struct Request {
    headers: String,
    body: Value,
}

/// A request's system prompt and subject message, as Ollama's leading
/// `system` and `user` messages, Bedrock's `system` blocks and first
/// message, or OpenAI's instructions and input message.
// {cite}`habeas:util`
fn opening(body: &Value) -> (String, String) {
    let text = |value: &Value| value.as_str().unwrap().to_owned();
    if body.get("system").is_some() {
        (
            text(&body["system"][0]["text"]),
            text(&body["messages"][0]["content"][0]["text"]),
        )
    } else if body.get("input").is_some() {
        assert_eq!(body["input"][0]["role"], "user");
        (
            text(&body["instructions"]),
            text(&body["input"][0]["content"][0]["text"]),
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
                // Accepted sockets can inherit the listener's nonblocking mode
                // (macOS). Wait for request bytes using the read timeout below.
                stream.set_nonblocking(false).unwrap();
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
                let mut response = response;
                if response == json!({"__finish_latest_search":true}) {
                    let messages = requests.last().unwrap().body["messages"]
                        .as_array()
                        .unwrap();
                    let search = messages
                        .iter()
                        .rev()
                        .filter_map(|message| {
                            serde_json::from_str::<Value>(message["content"].as_str()?).ok()
                        })
                        .find(|result| result.get("matches").is_some())
                        .unwrap();
                    response = reply(vec![call(
                        "finish",
                        "finish_search",
                        json!({"search_id":search["search_id"],"used":[]}),
                    )]);
                }
                let status = response
                    .as_object_mut()
                    .and_then(|object| object.remove("__http_status"))
                    .and_then(|value| value.as_u64())
                    .unwrap_or(200);
                let body = response.to_string();
                write!(stream, "HTTP/1.1 {status} Mock Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
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
        fs::write(project.join("src/lib.rs"), "// {impl}`requirements.ready`\n// :::{fixme} requirements.ready\n// :github: example/app#125\n// One acknowledged gap.\n// :::\npub fn ready() {}\n// {test}`requirements.ready`\nfn ready_test() {}\n").unwrap();
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

// {cite}`habeas:util`
fn call(id: &str, name: &str, arguments: Value) -> Value {
    json!({"id":id,"type":"function","function":{"name":name,"arguments":arguments}})
}

// {cite}`habeas:util`
fn reply(calls: Vec<Value>) -> Value {
    json!({"model":MODEL,"created_at":"2026-01-01T00:00:00Z",
        "message":{"role":"assistant","content":"review complete","tool_calls":calls},
        "done":true,"done_reason":"stop","prompt_eval_count":11,"eval_count":7})
}

// {test}`requirements.authoring.mandatory_inputs`
// {test}`requirements.authoring.reference_index`
// {test}`requirements.authoring.delivered_inputs`
// {test}`requirements.authoring.fixme_inputs`
// {test}`requirements.authoring.complete_initial_bundle`
#[test]
fn implementation_discovery_has_no_text_or_evidence_until_the_model_requests_a_read() {
    let server = Server::start(vec![
        reply(vec![call(
            "locate",
            "references",
            json!({"selection": SUBJECT, "roles": ["impl"]}),
        )]),
        reply(vec![call(
            "read",
            "habeas",
            json!({"file": "src/lib.rs", "item": "fn[ready]"}),
        )]),
        reply(vec![call(
            "submit",
            "submit_judgment",
            json!({"determination": true, "rationale": "The tests check the requirement."}),
        )]),
    ]);
    let fixture = Fixture::new(&server.endpoint);
    let output = fixture.run(&["author", SUBJECT]);
    assert!(output.status.success(), "{output:?}");
    let requests = server.finish();
    assert_eq!(requests.len(), 3);
    let prompt = prompt_text(&requests[0].body);
    assert!(prompt.contains("fn ready_test()"));
    assert!(prompt.contains("One acknowledged gap."));
    assert!(!prompt.contains("pub fn ready()"));
    let tool_result = |request: &Request| -> Value {
        let message = request.body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|message| message["role"] == "tool")
            .unwrap();
        serde_json::from_str(message["content"].as_str().unwrap()).unwrap()
    };
    let locations = tool_result(&requests[1]);
    assert!(locations.to_string().contains("fn[ready]"), "{locations:#}");
    for text in [
        REQUIREMENT,
        "One acknowledged gap.",
        "pub fn ready()",
        "\"input\"",
        "\"inputs\"",
    ] {
        assert!(!locations.to_string().contains(text), "{locations:#}");
    }
    let read = tool_result(&requests[2]);
    assert!(read["input"]["text"].to_string().contains("pub fn ready()"));
    assert_eq!(read["input"]["file"], "src/lib.rs");
    assert_eq!(read["input"]["item"], "fn[ready]");
    assert_eq!(
        read["input"]["hash"],
        habeas::outline_input(&fixture.project, "src/lib.rs", "fn[ready]").unwrap()["hash"]
    );
    let record: Value = serde_json::from_slice(&fs::read(&fixture.evidence).unwrap()).unwrap();
    let inputs = record["inputs"].as_array().unwrap();
    assert_eq!(
        inputs
            .iter()
            .filter(|input| input["file"] == "src/lib.rs" && input["item"] == "fn[ready]")
            .count(),
        1
    );
    assert!(!fixture.usage()["inputs"].to_string().contains("references"));
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
// {test}`requirements.ci.fixme_implementation_gap`
// {test}`requirements.sdk.habeas.assessment_policy`
// {test}`requirements.authoring.finish_search.empty`
// {test}`requirements.authoring.finish_search.completion`
#[test]
// {test}`requirements.authoring.persistence`
// {test}`requirements.authoring.reporting`
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
        json!({"__finish_latest_search":true}),
        reply(vec![call(
            "submit",
            "submit_judgment",
            json!({
            "determination":{"FIXME":["example/app#125"]},"rationale":"SECRET_RATIONALE: `src/lib.rs#fn[ready]` documents the gap."}),
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
    assert_eq!(requests.len(), 3);
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
        "Assess whether a project's tests adequately check one requirement.",
        REQUIREMENT,
        "SECRET_DEFINITION_TEXT",
        "fn ready_test()",
        "You MUST end by calling submit_judgment",
    ] {
        assert!(prompt.contains(text), "prompt missing {text}");
    }
    assert!(!prompt.contains("pub fn ready()"), "{prompt}");
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
            "finish_search",
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
        "SECRET_RATIONALE: `src/lib.rs#fn[ready]` documents the gap."
    );
    assert_eq!(evidence["requirement"], REQUIREMENT);
    let inputs = evidence["inputs"].as_array().unwrap();
    assert!(
        inputs
            .iter()
            .any(|input| input["file"] == "src/lib.rs" && input["item"] == "fn[ready_test]"),
        "{inputs:#?}"
    );
    // The acknowledgment reaches the model as metadata, without the
    // containing implementation. Its body is protected by the reference set.
    assert!(prompt.contains(":github: example/app#125"));
    assert!(prompt.contains("One acknowledged gap."));
    // GitHub is unreachable, so the issue is supplied as open, but the state
    // is not recorded.
    assert!(
        prompt.contains("\"reference\": \"example/app#125\"")
            && prompt.contains("\"state\": \"open\""),
        "{prompt}"
    );
    assert!(!evidence.to_string().contains("github_issues"));
    assert!(!fixture.usage().to_string().contains("github_issues"));
    assert!(
        !inputs
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
    assert_eq!(usage["request_count"], 3);
    assert_eq!(usage["tool_call_count"], 7);
    assert_eq!(usage["tool_calls"]["habeas"], 3);
    assert_eq!(usage["tool_failures"]["habeas"], 1);
    // The directory listing counts as a tool call but is not an input.
    assert_eq!(usage["tool_calls"]["list_files"], 1);
    assert_eq!(usage["tool_calls"]["ripgrep"], 1);
    assert_eq!(usage["tool_calls"]["finish_search"], 1);
    let usage_inputs = usage["inputs"].as_array().unwrap();
    assert!(
        !usage_inputs
            .iter()
            .any(|input| input["origin"] == "list_files"),
        "{usage_inputs:?}"
    );
    // Search snippets count toward usage, without becoming evidence inputs.
    assert!(
        usage_inputs.iter().any(|input| input["origin"] == "ripgrep"
            && input["kind"] == "search"
            && input["bytes"].as_u64().unwrap() > 0
            && input["lines"].as_u64().unwrap() > 0),
        "{usage_inputs:?}"
    );
    assert!(
        usage_inputs
            .iter()
            .any(|input| input["origin"] == "habeas" && input["unique"] == true),
        "{usage_inputs:?}"
    );
    assert_eq!(usage["tokens"]["input"], 33);
    assert_eq!(usage["tokens"]["output"], 21);
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

// {test}`requirements.authoring.acknowledged_gap`
#[test]
fn acknowledged_empty_input_boundary_is_explained_and_accepted() {
    let rationale = "`src/lib.rs#fn[ready_test]` checks nonempty input but omits the required empty-string boundary. The acknowledgment tracks that test gap as example/app#125.";
    let server = Server::start(vec![reply(vec![call(
        "submit",
        "submit_judgment",
        json!({"determination":{"FIXME":["example/app#125"]},"rationale":rationale}),
    )])]);
    let fixture = Fixture::new(&server.endpoint);
    common::coverage_setup(&fixture.project);
    fs::write(fixture.project.join("md/requirements.md"), ":::{req} ready\nThe readiness check MUST accept every string, including the empty string.\n:::\n").unwrap();
    fs::write(fixture.project.join("src/lib.rs"), "// {impl}`requirements.ready`\n// :::{fixme} requirements.ready\n// :github: example/app#125\n// The empty-string boundary is not tested.\n// :::\npub fn ready(input: &str) -> bool { !input.is_empty() }\n// {test}`requirements.ready`\nfn ready_test() { assert!(ready(\"nonempty\")); }\n").unwrap();
    let output = fixture.run(&["author", SUBJECT]);
    assert!(output.status.success(), "{output:?}");
    let requests = server.finish();
    assert_eq!(requests.len(), 1);
    let prompt = prompt_text(&requests[0].body);
    assert!(prompt.contains("The empty-string boundary is not tested."));
    assert!(prompt.contains("fn ready_test()"));
    assert!(prompt.contains("example/app#125"));
    assert!(!prompt.contains("!input.is_empty()"));
    let record: Value = serde_json::from_slice(&fs::read(&fixture.evidence).unwrap()).unwrap();
    assert_eq!(
        record["determination"],
        json!({"FIXME":["example/app#125"]})
    );
    assert_eq!(record["rationale"], rationale);
    assert!(
        record["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|input| input["file"] == "src/lib.rs" && input["item"] == "fn[ready_test]")
    );
    assert!(
        !record["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|input| input["file"] == "src/lib.rs" && input["item"] == "fn[ready]")
    );
    let report = habeas::validate_repository(&fixture.project).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
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
// {test}`requirements.configuration.documentation`
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

// {test}`requirements.authoring.invocation`
// {test}`requirements.authoring.configuration`
#[test]
fn an_explicit_configuration_selects_the_nondefault_workspace_profile() {
    let server = Server::start(vec![reply(vec![call(
        "submit",
        "submit_judgment",
        json!({"determination":true,"rationale":"The supplied sources support the subject."}),
    )])]);
    let fixture = Fixture::new(&server.endpoint);
    fs::remove_file(fixture.config.join("habeas/config.toml")).unwrap();
    let nested = fixture.project.join("src/nested");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("habeas.toml"), format!(
        "version = 1\n[authoring]\ndefault = 'default'\n[authoring.configs.default]\nbackend = 'ollama'\nmodel = 'WRONG_DEFAULT_MODEL'\nendpoint = 'http://127.0.0.1:9'\n[authoring.configs.alternate]\npolicy = 'once'\nbackend = 'ollama'\nmodel = 'alternate-model'\nendpoint = '{}'\napi_key = 'ALTERNATE_TEST_KEY'\n", server.endpoint
    )).unwrap();
    let output = fixture
        .command()
        .current_dir(&nested)
        .args(["author", SUBJECT, "--config", "alternate", "--force"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let requests = server.finish();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].body["model"], "alternate-model");
    assert!(requests[0].headers.contains("ALTERNATE_TEST_KEY"));
    assert_eq!(fixture.usage()["model"], "alternate-model");
    assert_eq!(fixture.usage()["backend"], "ollama");
}

// {test}`requirements.authoring.configuration`
#[test]
fn version_only_configurations_cannot_author_needed_evidence() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let fixture = Fixture::new(&format!("http://{}", listener.local_addr().unwrap()));
    fs::write(fixture.config.join("habeas/config.toml"), "version = 1\n").unwrap();
    fs::write(fixture.project.join("habeas.toml"), "version = 1\n").unwrap();
    let before = fs::read(&fixture.evidence).unwrap();
    for args in [&["author", SUBJECT][..], &["author", "--all"]] {
        let output = fixture.run(args);
        assert!(!output.status.success(), "{args:?}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("no authoring configuration"),
            "{output:?}"
        );
        assert_eq!(fs::read(&fixture.evidence).unwrap(), before);
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
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
// {test}`requirements.authoring.read_denial.untracked`
// {test}`requirements.authoring.read_denial`
#[test]
fn explicit_untracked_reads_return_an_untracked_error_and_the_assessment_continues() {
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
        let server = Server::start(vec![
            reply(vec![call("denied", tool, arguments)]),
            reply(vec![call(
                "submit",
                "submit_judgment",
                json!({"determination": true, "rationale": "The tracked sources support it."}),
            )]),
        ]);
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
        assert!(output.status.success(), "{tool}: {output:?}");
        // The model was told the path is untracked and how to fix it, and
        // the file was neither delivered nor recorded.
        let requests = server.finish();
        assert_eq!(requests.len(), 2, "{tool}");
        let second = requests[1].body.to_string();
        assert!(
            second.contains("is untracked") || second.contains("untracked, so"),
            "{tool}: {second}"
        );
        assert!(second.contains("git add"), "{tool}: {second}");
        assert!(!second.contains("hidden source"), "{tool}: {second}");
        let evidence = String::from_utf8_lossy(&output.stdout);
        assert!(!evidence.contains("untracked"), "{tool}: {evidence}");
        let usage = fixture.usage();
        assert_eq!(usage["outcome"], "completed", "{tool}: {usage}");
        assert_eq!(usage["request_count"], 2, "{tool}: {usage}");
        assert_eq!(usage["tool_failures"][tool], 1, "{tool}: {usage}");
    }
}

// {test}`requirements.authoring.negative_assessment`
// {test}`requirements.sdk.habeas.assessment_policy`
// {test}`requirements.authoring.usage_storage`
// {test}`requirements.authoring.persistence`
// {test}`requirements.ci.fixme_with_unacknowledged_gap`
// {test}`requirements.authoring.reporting`
#[test]
fn false_is_a_completed_assessment() {
    let server = Server::start(vec![reply(vec![call(
        "submit",
        "submit_judgment",
        json!({"determination":false,"rationale":"The test checks refusal for a full positive-capacity queue, without acknowledging missing checks for preservation and zero capacity."}),
    )])]);
    let fixture = Fixture::new(&server.endpoint);
    fs::write(fixture.project.join("md/requirements.md"), ":::{req} ready\nInserting into a full queue MUST return false without overwriting its entries, including at zero capacity.\n:::\n").unwrap();
    fs::write(fixture.project.join("src/lib.rs"), "// {impl}`requirements.ready`\n// FIXME(#125): zero-capacity insertion panics.\npub fn push(queue: &mut [u8], value: u8) -> bool {\n    queue[0] = value;\n    true\n}\n\n// {test}`requirements.ready`\n#[test]\nfn full_queue() { let mut queue = [1]; assert!(!push(&mut queue, 2)); }\n").unwrap();
    let output = fixture.run(&["author-evidence", SUBJECT, "--config", "mock"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let requests = server.finish();
    let prompt = prompt_text(&requests[0].body);
    assert!(prompt.contains("and false otherwise."));
    assert!(!prompt.contains("FIXME(#125): zero-capacity insertion panics"));
    assert!(!prompt.contains("queue[0] = value"));
    assert!(prompt.contains("fn full_queue()"));
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
// {test}`requirements.sdk.habeas.assessment_policy`
#[test]
fn negative_assessment_records_additional_reads_and_fails_validation() {
    let rationale =
        "src/lib.rs implements readiness, but `extra.txt` shows the needle case is unhandled.";
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

// {test}`requirements.authoring.negative_assessment`
// {test}`requirements.sdk.habeas.assessment_policy`
// {test}`requirements.sdk.graph.criteria.result_interpretation`
// {test}`requirements.cli.status`
#[test]
fn completed_inadequate_expiration_review_is_current_and_reported_by_status() {
    let rationale = "No test covers expired tokens.";
    let server = Server::start(vec![reply(vec![call(
        "submit",
        "submit_judgment",
        json!({"determination":false,"rationale":rationale}),
    )])]);
    let fixture = Fixture::new(&server.endpoint);
    common::coverage_setup(&fixture.project);
    fs::write(
        fixture.project.join("md/requirements.md"),
        ":::{req} ready\nExpired tokens MUST be rejected.\n:::\n",
    )
    .unwrap();
    fs::write(
        fixture.project.join("src/lib.rs"),
        "// {impl}`requirements.ready`\n\
         pub fn valid_token(now: u64, expiration: u64) -> bool { now < expiration }\n\
         // {test}`requirements.ready`\n\
         #[test]\n\
         fn unexpired_token() { assert!(valid_token(10, 20)); }\n",
    )
    .unwrap();
    let authored = fixture.run(&["author", SUBJECT]);
    assert!(authored.status.success(), "{authored:?}");
    assert_eq!(server.finish().len(), 1);
    assert_eq!(fixture.usage()["outcome"], "completed");
    let saved = fs::read(&fixture.evidence).unwrap();
    let value: Value = serde_json::from_slice(&saved).unwrap();
    assert_eq!(value["determination"], false);
    assert_eq!(value["rationale"], rationale);
    assert_eq!(value["review_version"], "habeas-review-v1");

    let report = habeas::validate_repository(&fixture.project).unwrap();
    assert_eq!(
        report.errors,
        [format!(
            "evidence/{SUBJECT}.json: determination is false: {rationale}"
        )],
        "the completed review is rejected solely by HC's assessment policy"
    );
    let status = fixture.run(&["status", SUBJECT, "--all"]);
    assert!(status.status.success(), "{status:?}");
    let text = String::from_utf8(status.stdout).unwrap();
    assert!(text.contains("Determination false"), "{text}");
    assert!(text.contains(rationale), "{text}");
    assert!(!text.contains("Invalid"), "{text}");
    assert!(!text.contains("changed"), "{text}");
    assert_eq!(
        fs::read(&fixture.evidence).unwrap(),
        saved,
        "status must preserve the saved completed answer"
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
// {test}`requirements.authoring.habeas_background`
// {test}`requirements.authoring.fixme_inputs`
// {test}`requirements.authoring.invocation`
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
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.starts_with(&format!("{subject}: ollama / {MODEL}")),
            "{stdout}"
        );
        let evidence_path = stdout
            .lines()
            .find_map(|line| line.strip_prefix("Evidence: "))
            .unwrap();
        assert_eq!(
            std::path::Path::new(evidence_path),
            fixture.project.join(format!("evidence/{subject}.json"))
        );
        let record: Value = serde_json::from_slice(&fs::read(evidence_path).unwrap()).unwrap();
        let field = if kind == "requirement" {
            "requirement"
        } else {
            "example"
        };
        let other = if kind == "requirement" {
            "example"
        } else {
            "requirement"
        };
        assert!(record.get(other).is_none(), "{record}");
        assert_eq!(
            record[field],
            if kind == "requirement" {
                REQUIREMENT
            } else {
                "Calling ready succeeds. {cite}`requirements.ready`"
            }
        );
        let usage_path = stdout
            .lines()
            .find_map(|line| line.strip_prefix("Usage: "))
            .unwrap();
        assert!(std::path::Path::new(usage_path).is_file(), "{stdout}");
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
        let acknowledgments: Value = serde_json::from_str(
            prompt
                .split_once("# Acknowledgments\n\n")
                .unwrap()
                .1
                .split_once("\n\n# Mandatory inputs")
                .unwrap()
                .0,
        )
        .unwrap();
        if kind == "requirement" {
            assert_eq!(
                acknowledgments,
                json!([{
                    "body": ":github: example/app#125\n One acknowledged gap.",
                    "file": "src/lib.rs",
                    "issues": [{"reference": "example/app#125", "state": "open"}],
                    "item": "fn[ready]",
                }])
            );
        } else {
            assert_eq!(acknowledgments, json!([]));
        }
        let (before, after) = subject_template.split_once("{{mandatory_inputs}}").unwrap();
        // The subject is not distributed, so its distributed task is empty.
        let before = before
            .replace("{{subject_kind}}", kind)
            .replace("{{subject_name}}", subject)
            .replace("{{distributed_task}}", "")
            .replace(
                "{{acknowledgments}}",
                &serde_json::to_string_pretty(&acknowledgments).unwrap(),
            );
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

// {test}`requirements.authoring.usage_storage`
#[cfg(unix)]
#[test]
// {test}`requirements.configuration.state_location`
fn usage_record_identifies_its_run_and_unusable_state_is_reported() {
    let server = Server::start(vec![
        reply(vec![call(
            "submit",
            "submit_judgment",
            json!({"determination":true,"rationale":"covered"}),
        )]);
        2
    ]);
    let fixture = Fixture::new(&server.endpoint);
    let output = fixture.run(&["author", SUBJECT]);
    assert!(output.status.success(), "{output:?}");
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

    let first_bytes = fs::read(&files[0]).unwrap();
    let second = fixture.run(&["author", SUBJECT, "--force"]);
    assert!(second.status.success(), "{second:?}");
    assert_eq!(fs::read(&files[0]).unwrap(), first_bytes);
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 2);
    server.finish();

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
    assert_eq!(
        evidence["rationale"], "covered",
        "failed runs preserved the evidence"
    );
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
        common::coverage_setup(&fixture.project);
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
        let errors = habeas::validate_repository(&fixture.project)
            .unwrap()
            .errors;
        assert_eq!(
            errors
                .iter()
                .any(|error| error.contains("determination is false")),
            !determination,
            "{errors:?}"
        );
        if determination {
            assert!(errors.is_empty(), "{errors:?}");
        }
    }
}

// {test}`requirements.authoring.invocation`
// {test}`requirements.authoring.persistence`
// {test}`requirements.authoring.delivered_inputs`
// {test}`requirements.authoring.usage_storage`
// {test}`requirements.authoring.reporting`
// {test}`requirements.sdk.habeas.evidence_format`
// {test}`requirements.sdk.habeas.evidence_conversion.writes`
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
                "dependencies",
                "determination",
                "format_version",
                "inputs",
                "rationale",
                "references",
                "requirement",
                "review_version"
            ]
        );
        assert_eq!(evidence["format_version"], 1);
        assert_ne!(evidence["review_version"], "*");
        assert!(!evidence["review_version"].as_str().unwrap().is_empty());
        assert!(evidence["dependencies"].as_array().unwrap().len() >= inputs.len());
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
            let count = if calls.is_empty() { 2 } else { 1 };
            let server = Server::start(vec![reply(calls.clone()); count]);
            let fixture = Fixture::new(&server.endpoint);
            let mut args = vec!["author-evidence", SUBJECT];
            if stdout {
                args.push("--stdout");
            }
            let output = fixture.run(&args);
            assert!(!output.status.success());
            assert!(output.stdout.is_empty());
            assert_eq!(server.finish().len(), count);
            if calls.is_empty() {
                assert!(String::from_utf8_lossy(&output.stderr).contains("even after a reminder"));
            }
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
            // A response without a submission is reminded once.
            assert_eq!(usage["request_count"], if calls.is_empty() { 2 } else { 1 });
            assert!(usage["determination"].is_null());
        }
    }
}

// {test}`requirements.authoring.deterministic_validation`
// {test}`requirements.configuration.loading`
// {test}`requirements.sdk.habeas.evidence_conversion.writes`
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
// {test}`requirements.authoring.configuration`
// {test}`requirements.sdk.habeas.evidence_conversion.writes`
// {test}`requirements.sdk.habeas.assessment_policy`
#[test]
fn missing_or_stale_evidence_fails_validation_without_contacting_the_provider() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let fixture = Fixture::new(&format!("http://{}", listener.local_addr().unwrap()));
    let changed = format!(":::{{req}} ready\n{REQUIREMENT} Changed.\n:::\n");
    let config_path = fixture.config.join("habeas/config.toml");
    let configured = fs::read_to_string(&config_path).unwrap();
    for config in [&configured[..], "version = 1\n"] {
        fs::write(&config_path, config).unwrap();
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
// {test}`requirements.authoring.output_limit`
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
        // A configured maxTokens is used as given on every request.
        assert_eq!(requests[1].body["inferenceConfig"]["maxTokens"], 321);
        assert_eq!(body["additionalModelRequestFields"], json!({"top_k":12}));
        let prompt = prompt_text(body);
        assert!(
            prompt.contains("Assess whether a project's tests adequately check")
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
                "finish_search",
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
    let (output, bodies, usage) = bedrock_exchange(
        model,
        vec![
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
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (bodies, usage)
}

/// Runs one Bedrock assessment against model `model` whose endpoint answers
/// with `responses` in turn, returning the command output, the request
/// bodies, and the usage record.
// {cite}`habeas:util`
fn bedrock_exchange(model: &str, responses: Vec<Value>) -> (Output, Vec<Value>, Value) {
    bedrock_exchange_with_limit(model, responses, None)
}

// {cite}`habeas:util`
fn bedrock_exchange_with_limit(
    model: &str,
    responses: Vec<Value>,
    max_tokens: Option<u64>,
) -> (Output, Vec<Value>, Value) {
    let server = Server::start(responses);
    let fixture = Fixture::new(&server.endpoint);
    fs::write(
        fixture.config.join("habeas/config.toml"),
        format!("version = 1\n[authoring]\ndefault = 'cloud'\n[authoring.configs.cloud]\npolicy = 'once'\nbackend = 'bedrock'\nmodel = '{model}'\nregion = 'us-west-2'\n"),
    )
    .unwrap();
    if let Some(limit) = max_tokens {
        let path = fixture.config.join("habeas/config.toml");
        fs::write(
            &path,
            format!(
                "{}[authoring.configs.cloud.options]\nmaxTokens = {limit}\n",
                fs::read_to_string(&path).unwrap()
            ),
        )
        .unwrap();
    }
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
    let bodies = server
        .finish()
        .into_iter()
        .map(|request| request.body)
        .collect();
    (output, bodies, fixture.usage())
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

// {test}`requirements.authoring.output_limit`
#[test]
fn bedrock_requests_without_max_tokens_ask_for_at_most_16k_output_tokens() {
    for model in [
        "us.anthropic.claude-sonnet-4-5-20250929-v1:0",
        "fixture-uncached-model",
    ] {
        let (bodies, usage) = bedrock_run(model);
        assert_eq!(usage["outcome"], "completed");
        assert_eq!(bodies.len(), 2);
        for body in &bodies {
            assert_eq!(
                body["inferenceConfig"],
                json!({"maxTokens":16384}),
                "{body}"
            );
            assert!(body.get("additionalModelRequestFields").is_none(), "{body}");
        }
    }
}

/// A Converse response that stopped at the output limit partway through a
/// submission whose rationale was cut off.
// {cite}`habeas:util`
fn cut_off_submission(id: &str) -> Value {
    json!({
        "output":{"message":{"role":"assistant","content":[
            {"text":"Submitting."},
            {"toolUse":{"toolUseId":id,"name":"submit_judgment","input":{"determination":true}}}
        ]}},
        "stopReason":"max_tokens",
        "usage":{"inputTokens":7,"outputTokens":16384,"totalTokens":16391},
        "metrics":{"latencyMs":1}
    })
}

/// The user message Habeas sends after a response cut off at 16,384 tokens.
const LIMIT_NOTE: &str = "Your previous response reached the 16384-token output limit and was cut off,\nso its tool call was incomplete. Keep within the limit: make your rationale\nshorter, then call the tool again.";

// {test}`requirements.authoring.output_limit`
#[test]
fn a_configured_bedrock_limit_controls_requests_and_the_continuation_note() {
    let mut cut_off = cut_off_submission("cut");
    cut_off["usage"] = json!({"inputTokens":7,"outputTokens":321,"totalTokens":328});
    let (output, bodies, usage) = bedrock_exchange_with_limit(
        "fixture-uncached-model",
        vec![
            cut_off,
            json!({
                "output":{"message":{"role":"assistant","content":[{"toolUse":
                    {"toolUseId":"submit","name":"submit_judgment","input":
                        {"determination":true,"rationale":"Short."}}}]}},
                "stopReason":"tool_use",
                "usage":{"inputTokens":9,"outputTokens":5,"totalTokens":14},
                "metrics":{"latencyMs":1}
            }),
        ],
        Some(321),
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(bodies.len(), 2);
    for body in &bodies {
        assert_eq!(body["inferenceConfig"]["maxTokens"], 321);
    }
    let note = bodies[1]["messages"].as_array().unwrap().last().unwrap();
    assert_eq!(
        note["content"][1]["text"],
        LIMIT_NOTE.replace("16384", "321")
    );
    assert_eq!(usage["request_count"], 2);
    assert_eq!(usage["outcome"], "completed");
}

// {test}`requirements.authoring.output_limit`
// {test}`requirements.authoring.submission`
#[test]
fn a_response_cut_off_at_the_output_limit_is_continued_once() {
    let mut cut_off = cut_off_submission("cut");
    cut_off["output"]["message"]["content"].as_array_mut().unwrap().push(
        json!({"toolUse":{"toolUseId":"read-cut","name":"habeas","input":{"file":"src/lib.rs"}}}),
    );
    let (output, bodies, usage) = bedrock_exchange(
        "us.anthropic.claude-sonnet-4-5-20250929-v1:0",
        vec![
            cut_off,
            json!({
                "output":{"message":{"role":"assistant","content":[{"toolUse":
                    {"toolUseId":"submit","name":"submit_judgment","input":
                        {"determination":true,"rationale":"Short."}}}]}},
                "stopReason":"tool_use",
                "usage":{"inputTokens":9,"outputTokens":5,"totalTokens":14},
                "metrics":{"latencyMs":1}
            }),
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bodies: Vec<_> = bodies.iter().map(without_cache_points).collect();
    assert_eq!(bodies.len(), 2);
    // The same conversation continues: the cut-off turn as sent, then one
    // user message answering its incomplete call and carrying the note.
    let first = bodies[0]["messages"].as_array().unwrap();
    let second = bodies[1]["messages"].as_array().unwrap();
    assert_eq!(second.len(), first.len() + 2);
    assert_eq!(&second[..first.len()], first.as_slice());
    let cut = &second[first.len()];
    assert_eq!(cut["role"], "assistant");
    assert_eq!(cut["content"][1]["toolUse"]["toolUseId"], "cut");
    let note = &second[first.len() + 1];
    assert_eq!(note["role"], "user");
    let content = note["content"].as_array().unwrap();
    assert_eq!(content.len(), 3, "{note}");
    assert_eq!(content[0]["toolResult"]["toolUseId"], "cut");
    assert_eq!(content[0]["toolResult"]["status"], "error");
    assert!(
        content[0]["toolResult"].to_string().contains("not run"),
        "{note}"
    );
    assert_eq!(content[1]["toolResult"]["toolUseId"], "read-cut");
    assert_eq!(content[1]["toolResult"]["status"], "error");
    assert!(
        content[1]["toolResult"].to_string().contains("not run"),
        "{note}"
    );
    assert_eq!(content[2]["text"], LIMIT_NOTE);
    // Both requests are recorded, and the cut-off call never ran.
    assert_eq!(usage["request_count"], 2);
    assert_eq!(usage["requests"].as_array().unwrap().len(), 2);
    assert_eq!(usage["tool_calls"]["submit_judgment"], 2);
    assert_eq!(usage["tool_skipped"]["submit_judgment"], 1);
    assert_eq!(usage["tool_skipped"]["habeas"], 1);
    assert_eq!(usage["outcome"], "completed");
}

// {test}`requirements.authoring.output_limit`
#[test]
fn a_second_response_at_the_output_limit_fails_the_assessment() {
    let (output, bodies, usage) = bedrock_exchange(
        "fixture-uncached-model",
        vec![cut_off_submission("cut"), cut_off_submission("again")],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("incomplete"), "{stderr}");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("1 authored"));
    assert_eq!(bodies.len(), 2);
    let last = bodies[1]["messages"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(last["content"][1]["text"], LIMIT_NOTE);
    assert_eq!(usage["request_count"], 2);
    assert_eq!(usage["tool_skipped"]["submit_judgment"], 1);
    assert_eq!(usage["outcome"], "failed");
}

// {test}`requirements.authoring.output_limit`
#[test]
fn a_context_window_overflow_fails_without_a_continuation() {
    let mut overflow = cut_off_submission("overflow");
    overflow["stopReason"] = json!("model_context_window_exceeded");
    let (output, bodies, usage) = bedrock_exchange("fixture-uncached-model", vec![overflow]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("context window"), "{stderr}");
    assert_eq!(bodies.len(), 1);
    assert_eq!(usage["request_count"], 1);
    assert_eq!(usage["outcome"], "failed");
}

/// An Ollama reply that stopped at the output limit partway through a
/// submission.
// {cite}`habeas:util`
fn ollama_cut_off_reply() -> Value {
    let mut cut = reply(vec![call(
        "cut",
        "submit_judgment",
        json!({"determination":true}),
    )]);
    cut["done_reason"] = json!("length");
    cut
}

// {test}`requirements.authoring.output_limit`
#[test]
fn a_stop_at_an_output_limit_that_is_not_known_fails_the_assessment() {
    // The fixture's Ollama configuration sets no `num_predict`, so the
    // limit the response reached is not known.
    let server = Server::start(vec![ollama_cut_off_reply()]);
    let fixture = Fixture::new(&server.endpoint);
    let output = fixture.run(&["author", SUBJECT]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("incomplete") && stderr.contains("does not set"),
        "{stderr}"
    );
    assert_eq!(server.finish().len(), 1);
    assert_eq!(
        fs::read_to_string(&fixture.evidence).unwrap(),
        fixture.original
    );
    let usage = fixture.usage();
    assert_eq!(usage["request_count"], 1);
    assert_eq!(usage["outcome"], "failed");
}

// {test}`requirements.authoring.output_limit`
#[test]
fn a_configured_ollama_num_predict_is_the_known_output_limit() {
    let server = Server::start(vec![
        ollama_cut_off_reply(),
        reply(vec![call(
            "submit",
            "submit_judgment",
            json!({"determination":true,"rationale":"Short."}),
        )]),
    ]);
    let fixture = Fixture::new(&server.endpoint);
    let config_path = fixture.config.join("habeas/config.toml");
    let config = fs::read_to_string(&config_path).unwrap();
    fs::write(&config_path, format!("{config}num_predict = 512\n")).unwrap();
    let output = fixture.run(&["author", SUBJECT]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let requests = server.finish();
    assert_eq!(requests.len(), 2);
    let last = requests[1].body["messages"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .to_string();
    assert!(
        last.contains("reached the 512-token output limit"),
        "{last}"
    );
    assert_eq!(fixture.usage()["outcome"], "completed");
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

/// Returns the first request's prompt and sorted tool definitions, removing
/// only the provider-specific envelopes around each definition.
// {test}`requirements.authoring.prompt_parts`
fn prompt_and_tools(body: &Value) -> (String, Vec<(String, String, Value)>) {
    let tools = body
        .get("toolConfig")
        .map_or(&body["tools"], |config| &config["tools"]);
    let mut tools = tools
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| {
            let spec = tool
                .get("toolSpec")
                .or_else(|| tool.get("function"))
                .unwrap_or(tool);
            let parameters = spec
                .get("inputSchema")
                .map_or(&spec["parameters"], |schema| &schema["json"]);
            (
                spec["name"].as_str().unwrap().to_owned(),
                spec["description"].as_str().unwrap().to_owned(),
                parameters.clone(),
            )
        })
        .collect::<Vec<_>>();
    tools.sort_by(|left, right| left.0.cmp(&right.0));
    (prompt_text(body), tools)
}

// {test}`requirements.authoring.rig_configuration_mapping`
// {test}`requirements.authoring.provider`
// {test}`requirements.authoring.configuration_precedence`
#[test]
fn all_adapters_preserve_prompt_tools_and_evidence_with_default_connections() {
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
    let ollama = prompt_and_tools(&requests[0].body);
    let ollama_evidence: Value =
        serde_json::from_slice(&fs::read(&fixture.evidence).unwrap()).unwrap();

    // Bedrock without a profile or region uses the SDK default chain.
    let server = Server::start(vec![json!({
        "output":{"message":{"role":"assistant","content":[{"toolUse":
            {"toolUseId":"submit","name":"submit_judgment","input":submit.clone()}}]}},
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
    let bedrock = prompt_and_tools(&requests[0].body);

    let bedrock_evidence: Value =
        serde_json::from_slice(&fs::read(&fixture.evidence).unwrap()).unwrap();

    // OpenAI uses its default credential environment variable.
    let server = Server::start(vec![json!({
        "id":"resp_submit", "object":"response", "status":"completed",
        "model":"fixture-openai", "output":[{
            "type":"function_call", "id":"fc_submit", "call_id":"submit",
            "name":"submit_judgment", "arguments":submit.to_string(), "status":"completed"
        }],
        "usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}
    })]);
    let fixture = Fixture::new(&server.endpoint);
    fs::write(
        fixture.config.join("habeas/config.toml"),
        format!("version = 1\n[authoring]\ndefault = 'api'\n[authoring.configs.api]\npolicy = 'once'\nbackend = 'openai'\nmodel = 'fixture-openai'\nendpoint = '{}'\n", server.endpoint),
    ).unwrap();
    let output = fixture
        .command()
        .env("OPENAI_API_KEY", AUTH)
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
    assert!(
        requests[0]
            .headers
            .starts_with("POST /responses HTTP/1.1\r\n")
    );
    assert!(requests[0].headers.contains(&format!("Bearer {AUTH}")));
    assert_eq!(requests[0].body["model"], "fixture-openai");
    let openai = prompt_and_tools(&requests[0].body);
    let openai_evidence: Value =
        serde_json::from_slice(&fs::read(&fixture.evidence).unwrap()).unwrap();

    // Identical projects and submissions must preserve both request semantics
    // and the entire evidence document, including inputs and reference hashes.
    assert_eq!(ollama, bedrock);
    assert_eq!(ollama, openai);
    assert_eq!(ollama.1.len(), 10);
    assert_eq!(ollama_evidence["requirement"], REQUIREMENT);
    assert_eq!(ollama_evidence["determination"], true);
    assert_eq!(ollama_evidence["rationale"], submit["rationale"]);
    assert_eq!(ollama_evidence, bedrock_evidence);
    assert_eq!(ollama_evidence, openai_evidence);
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
    let config_path = fixture.config.join("habeas/config.toml");
    let original_config = fs::read_to_string(&config_path).unwrap();
    for backend in ["ollama", "openai"] {
        fs::write(
            &config_path,
            original_config.replace("backend = 'ollama'", &format!("backend = '{backend}'")),
        )
        .unwrap();
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

/// The bundled skills, by name.
// {cite}`habeas:util`
const SKILLS: [&str; 5] = [
    "habeas",
    "habeas-requirements",
    "habeas-evidence",
    "habeas-bootstrap-requirements",
    "habeas-refactor",
];

/// The files of a bundled skill, by path relative to its directory: its
/// `SKILL.md` first, then every file under its `references/` directory.
// {cite}`habeas:util`
fn skill_files(name: &str) -> &'static [(&'static str, &'static str)] {
    match name {
        "habeas" => &[
            ("SKILL.md", include_str!("../assets/skills/habeas/SKILL.md")),
            (
                "references/ci.md",
                include_str!("../assets/skills/habeas/references/ci.md"),
            ),
            (
                "references/distributed-requirements.md",
                include_str!("../assets/skills/habeas/references/distributed-requirements.md"),
            ),
            (
                "references/syntax-details.md",
                include_str!("../assets/skills/habeas/references/syntax-details.md"),
            ),
        ],
        "habeas-requirements" => &[
            (
                "SKILL.md",
                include_str!("../assets/skills/habeas-requirements/SKILL.md"),
            ),
            (
                "references/review-prompt.md",
                include_str!("../assets/skills/habeas-requirements/references/review-prompt.md"),
            ),
        ],
        "habeas-evidence" => &[
            (
                "SKILL.md",
                include_str!("../assets/skills/habeas-evidence/SKILL.md"),
            ),
            (
                "references/authoring-failures.md",
                include_str!("../assets/skills/habeas-evidence/references/authoring-failures.md"),
            ),
            (
                "references/fixme.md",
                include_str!("../assets/skills/habeas-evidence/references/fixme.md"),
            ),
            (
                "references/reviews.md",
                include_str!("../assets/skills/habeas-evidence/references/reviews.md"),
            ),
        ],
        "habeas-bootstrap-requirements" => &[(
            "SKILL.md",
            include_str!("../assets/skills/habeas-bootstrap-requirements/SKILL.md"),
        )],
        "habeas-refactor" => &[(
            "SKILL.md",
            include_str!("../assets/skills/habeas-refactor/SKILL.md"),
        )],
        _ => panic!("unknown skill {name}"),
    }
}

/// Text with line wrapping collapsed, so phrases can be matched across lines.
// {cite}`habeas:util`
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A bundled skill's raw `SKILL.md`.
// {cite}`habeas:util`
fn skill_md(name: &str) -> &'static str {
    skill_files(name)[0].1
}

/// One file of a bundled skill, raw.
// {cite}`habeas:util`
fn skill_file(name: &str, path: &str) -> &'static str {
    skill_files(name)
        .iter()
        .find(|(file, _)| *file == path)
        .unwrap_or_else(|| panic!("{name} has no {path}"))
        .1
}

/// A bundled skill: its `SKILL.md` together with its reference files,
/// collapsed.
// {cite}`habeas:util`
fn bundled_skill(name: &str) -> String {
    let files = skill_files(name)
        .iter()
        .map(|(_, text)| *text)
        .collect::<Vec<_>>();
    collapse(&files.join("\n"))
}

/// A bundled skill's front-matter description.
// {cite}`habeas:util`
fn description(name: &str) -> &'static str {
    skill_md(name)
        .lines()
        .find_map(|line| line.strip_prefix("description: "))
        .unwrap_or_else(|| panic!("{name} has no description"))
}

/// The collapsed body of a Markdown section, from the line after `heading` to
/// the next heading of the same or a higher level outside a code fence.
// {cite}`habeas:util`
fn section(raw: &str, heading: &str) -> String {
    let level = heading.chars().take_while(|c| *c == '#').count();
    let mut lines = raw.lines().skip_while(|line| *line != heading);
    assert!(lines.next().is_some(), "no section {heading:?}");
    let mut fence: Option<usize> = None;
    let mut body = Vec::new();
    for line in lines {
        let ticks = line.trim_start().chars().take_while(|c| *c == '`').count();
        match fence {
            Some(open) if ticks >= open && line.trim().chars().all(|c| c == '`') => fence = None,
            Some(_) => {}
            None if ticks >= 3 => fence = Some(ticks),
            None => {
                let hashes = line.chars().take_while(|c| *c == '#').count();
                if hashes > 0 && hashes <= level && line[hashes..].starts_with(' ') {
                    break;
                }
            }
        }
        body.push(line);
    }
    collapse(&body.join("\n"))
}

/// The numbered steps of a bundled skill's "Workflow" section, each with its
/// number and collapsed text, including its indented continuation lines.
// {cite}`habeas:util`
fn workflow_steps(name: &str) -> Vec<(u32, String)> {
    let (_, rest) = skill_md(name)
        .split_once("\n## Workflow\n")
        .unwrap_or_else(|| panic!("{name} has no workflow"));
    let mut steps: Vec<(u32, Vec<&str>)> = Vec::new();
    let mut open = false;
    for line in rest.lines() {
        if line.starts_with("## ") {
            break;
        }
        let numbered = line
            .strip_prefix("- [ ] ")
            .and_then(|step| step.split_once(". "))
            .and_then(|(number, text)| Some((number.parse().ok()?, text)));
        if let Some((number, text)) = numbered {
            steps.push((number, vec![text]));
            open = true;
            continue;
        }
        if !line.is_empty() && !line.starts_with(' ') {
            open = false;
        }
        if open {
            steps.last_mut().unwrap().1.push(line);
        }
    }
    steps
        .into_iter()
        .map(|(number, lines)| (number, collapse(&lines.join("\n"))))
        .collect()
}

/// The text of one numbered workflow step.
// {cite}`habeas:util`
fn step(name: &str, number: u32) -> String {
    workflow_steps(name)
        .into_iter()
        .find(|(n, _)| *n == number)
        .unwrap_or_else(|| panic!("{name} has no step {number}"))
        .1
}

/// The number of the workflow step that contains `phrase`.
// {cite}`habeas:util`
fn step_of(name: &str, phrase: &str) -> u32 {
    workflow_steps(name)
        .into_iter()
        .find(|(_, text)| text.contains(phrase))
        .unwrap_or_else(|| panic!("no step of {name} contains {phrase:?}"))
        .0
}

// {cite}`habeas:util`
fn assert_text(label: &str, text: &str, phrases: &[&str]) {
    for phrase in phrases {
        assert!(text.contains(phrase), "{label} lacks {phrase:?}");
    }
}

/// Asserts that the text contains each phrase, each after the previous one.
// {cite}`habeas:util`
fn assert_text_in_order(label: &str, text: &str, phrases: &[&str]) {
    let mut from = 0;
    for phrase in phrases {
        let at = text[from..]
            .find(phrase)
            .unwrap_or_else(|| panic!("{label} lacks {phrase:?} after byte {from}"));
        from += at + phrase.len();
    }
}

// {cite}`habeas:util`
fn assert_phrases(name: &str, phrases: &[&str]) {
    assert_text(name, &bundled_skill(name), phrases);
}

// {cite}`habeas:util`
fn assert_in_order(name: &str, phrases: &[&str]) {
    assert_text_in_order(name, &bundled_skill(name), phrases);
}

/// Phrases that state part of a topic in full, by the skill that is its home.
// {cite}`habeas:util`
fn home_markers(home: &str) -> &'static [&'static str] {
    match home {
        "habeas" => &[
            "| Task | Skill |",
            "A selection is a full name or a glob in which `*` matches any characters",
            "Its full name is the chapter path relative to that directory",
            "Fence a reference to attach commentary",
            "`:::{violations} <name>` selects constructs likely to break it",
            "Each site-query match makes a site",
            "omit a leading `reqs/` and a trailing `/index` or `/README`",
            "silently defines nothing",
            "needs a longer outer fence such as `::::{req}`",
            "Profiles, reports, and captured source under the Cargo target directory",
            "Install prerequisites",
        ],
        "habeas-requirements" => &[
            "Write drafts from the request straight into the chapter",
            "For each group, start a read-only subagent",
            "**Gate:** no implementation or test code",
            "A *root* is an item that carries out part of a requirement",
            "## What the review checks",
            "# Requirement group review",
            "**Compound:** a requirement that lists several independent forms",
        ],
        "habeas-evidence" => &[
            "A judgment is one of three outcomes",
            "`habeas author --all` assesses every definition whose evidence is missing, stale, or invalid",
            "## Fixing findings",
            "Recorded reads track evidence freshness, without Git-history grace periods or unreferenced-item warnings",
            "A FIXME judgment, such as",
            "gets a citation review. For project requirements and examples it checks that",
            "For built-ins it checks their permitted forms and stated criteria",
            "A site the requirement does not apply to is judged true",
            "The goal is that independent assessments of the same subject reach the same determination",
            "describing the change relative to the prior commit rather than the drafts that led to it",
        ],
        "habeas-bootstrap-requirements" => &[
            "every first-party runnable test or test case",
            "## Proposing requirements",
            "Independent recount:",
        ],
        "habeas-refactor" => &[
            "habeas refactor --dry-run",
            "`--map 'src/config.rs#fn[load]=src/config/load.rs#fn[load]'`",
            "\"Would leave ... unchanged\" lists records a mapping affects that fail a check",
        ],
        _ => panic!("unknown skill {home}"),
    }
}

/// Asserts that `home` states each of its markers and no other skill does.
// {cite}`habeas:util`
fn assert_home(home: &str) {
    assert_phrases(home, home_markers(home));
    for other in SKILLS.into_iter().filter(|other| *other != home) {
        let text = bundled_skill(other);
        for marker in home_markers(home) {
            assert!(
                !text.contains(marker),
                "{other} restates {home}'s {marker:?}"
            );
        }
    }
}

// {test}`requirements.cli.skill_structure`
#[test]
fn bundled_skills_are_five_skill_directories_each_home_to_its_topics() {
    // Symposium publishes every skill under assets/skills.
    let manifest: toml::Value =
        toml::from_str(include_str!("../Cargo.toml")).expect("Cargo.toml parses");
    let published = manifest["package"]["metadata"]["symposium"]["skills"]
        .as_array()
        .unwrap();
    assert!(
        published
            .iter()
            .any(|skill| skill["source"]["path"].as_str() == Some("assets/skills")),
        "{published:?}"
    );
    // A skill is its SKILL.md and the files under its references/ directory;
    // the files these tests read are exactly those.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/skills");
    let mut dirs = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    dirs.sort();
    let mut expected = SKILLS.map(String::from).to_vec();
    expected.sort();
    assert_eq!(dirs, expected);
    for name in SKILLS {
        assert!(
            skill_md(name).starts_with(&format!("---\nname: {name}\ndescription: ")),
            "{name} front matter"
        );
        let mut on_disk = vec!["SKILL.md".to_owned()];
        if let Ok(entries) = fs::read_dir(root.join(name).join("references")) {
            for entry in entries {
                let file = entry.unwrap().file_name().into_string().unwrap();
                on_disk.push(format!("references/{file}"));
            }
        }
        on_disk.sort();
        let mut listed = skill_files(name)
            .iter()
            .map(|(path, _)| path.to_string())
            .collect::<Vec<_>>();
        listed.sort();
        assert_eq!(on_disk, listed, "{name}");
    }
    // Each topic is stated in full in exactly one skill.
    for home in SKILLS {
        assert_home(home);
    }
}

// {test}`requirements.test-coverage.guidance`
// {test}`requirements.test-coverage.guidance.doctests`
// {test}`requirements.test-coverage.guidance.artifacts`
// {test}`requirements.test-coverage.check.guidance`
#[test]
fn bundled_skills_default_to_process_coverage_and_keep_artifacts_disposable() {
    for name in ["habeas", "habeas-evidence", "habeas-bootstrap-requirements"] {
        let text = bundled_skill(name)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        for required in [
            "`habeas coverage`",
            "`cargo nextest`",
            "focused runs",
            "`cargo test --doc --workspace`",
            "disposable",
            "Cargo target directory",
        ] {
            assert!(text.contains(required), "{name} omits {required}");
        }
        assert!(
            text.contains("by default") || text.contains("default to"),
            "{name}"
        );
        assert!(
            text.to_lowercase().contains("never stage or commit"),
            "{name}"
        );
        if name != "habeas" {
            assert!(text.contains("Follow the habeas skill's \"Testing and coverage artifacts\""));
        }
    }
    let home = bundled_skill("habeas");
    assert_text(
        "default coverage directive",
        &collapse(&home),
        &[
            "Use `habeas coverage` by default for every Rust unit and integration test run, including focused runs (`habeas coverage -- -E 'test(name)'`)",
            "The workflow uses `cargo nextest` for parallel process runs and retains independent process coverage",
            "Run doctests separately with `cargo test --doc --workspace`",
        ],
    );
    for name in ["habeas-evidence", "habeas-bootstrap-requirements"] {
        assert_text(
            name,
            &collapse(&bundled_skill(name)),
            &[
                "Follow the habeas skill's \"Testing and coverage artifacts\": default to `habeas coverage` for unit, integration and focused runs through `cargo nextest`",
                "run doctests separately with `cargo test --doc --workspace`",
            ],
        );
    }
    assert_text(
        "evidence check guidance",
        &collapse(&bundled_skill("habeas-evidence")),
        &[
            "Use `habeas test` for full verification after the changes.",
            "recommend the separate collection and check steps",
            "[CI example](../habeas/references/ci.md)",
            "collection alone does not enforce the policy",
        ],
    );
    assert_text(
        "bootstrap check guidance",
        &collapse(&bundled_skill("habeas-bootstrap-requirements")),
        &[
            "Run `habeas test` for full-workspace collection and post-suite coverage checking",
            "Recommend separate successful collection and `habeas coverage-check` steps in project CI",
            "[CI example](../habeas/references/ci.md)",
            "Collection alone does not enforce the coverage policy",
        ],
    );
    assert_text(
        "approved implementation SOP",
        &collapse(include_str!("../agent-sops/approved-implementation.sop.md")),
        &[
            "Use `cargo habeas coverage -- <nextest arguments>` for every Rust unit and integration test run, including focused or filtered runs",
            "It executes parallel processes through `cargo nextest` and retains coverage for each process",
            "Run doctests separately with `cargo test --doc --workspace`",
            "profiles, reports, and source snapshots under the Cargo target directory are disposable build output: never stage or commit them to Git",
            "`cargo habeas test` collects workspace coverage and then enforces the coverage policy",
            "Recommend separate collection and `habeas coverage-check` steps in project CI",
            "[CI example](../assets/skills/habeas/references/ci.md)",
            "Collection alone does not enforce the coverage policy",
        ],
    );
    for required in [
        "Profiles, reports, and captured source",
        "cargo install cargo-nextest --locked",
        "rustup component add llvm-tools-preview",
        "target/habeas/coverage/",
    ] {
        assert!(home.contains(required), "coverage home omits {required}");
    }
}

// {test}`requirements.test-coverage.setup.bootstrap`
#[test]
fn bootstrap_checks_and_repairs_coverage_setup_and_preserves_conflicts() {
    let text = bundled_skill("habeas-bootstrap-requirements");
    for required in [
        "Before bootstrapping evidence, check coverage readiness with `habeas status`",
        "saved `.config/nextest.toml` collector and default-profile rules",
        "Use `habeas setup` to establish or repair",
        "Report conflicting nextest settings for review",
        "preserve project-specific settings",
    ] {
        assert!(text.contains(required), "bootstrap omits {required}");
    }
}

// {test}`requirements.cli.skill_structure.refer`
#[test]
fn bundled_skills_refer_to_a_topic_home_instead_of_restating_it() {
    for (name, referrals) in [
        (
            "habeas",
            &[
                "the habeas-requirements skill says which to use",
                "the habeas-evidence skill's `references/fixme.md`",
                "(habeas-evidence, \"Rules\")",
            ][..],
        ),
        (
            "habeas-requirements",
            &[
                "The habeas skill has the directive and reference syntax",
                "use `{fixme}` as the habeas-evidence skill's `references/fixme.md` describes",
                "follow the habeas-evidence skill to author evidence",
                "follow its \"Disagreeing assessments\"",
                "is a habeas-refactor task",
            ][..],
        ),
        (
            "habeas-evidence",
            &[
                "\"What the review checks\" in the habeas-requirements skill",
                "\"Choosing references\" in the habeas-requirements skill",
                "goes through habeas-requirements steps 3 to 5",
                "(habeas-refactor)",
                "the habeas skill's `references/distributed-requirements.md`",
            ][..],
        ),
        (
            "habeas-bootstrap-requirements",
            &[
                "Read the habeas skill for directive syntax, and the habeas-requirements skill",
                "through habeas-requirements steps 3 to 5",
                "\"Choosing references\" in the habeas-requirements skill",
                "the habeas-evidence skill's `references/authoring-failures.md`",
                "the habeas skill's `references/distributed-requirements.md`",
                "\"Disagreeing assessments\" in the habeas-evidence skill",
            ][..],
        ),
        (
            "habeas-refactor",
            &[
                "following the habeas-evidence skill",
                "under the habeas-requirements skill",
                "needs reassessment with habeas-evidence",
            ][..],
        ),
    ] {
        assert_phrases(name, referrals);
        // No skill restates what another skill is the home of.
        let text = bundled_skill(name);
        for home in SKILLS.into_iter().filter(|home| *home != name) {
            for marker in home_markers(home) {
                assert!(!text.contains(marker), "{name} restates {marker:?}");
            }
        }
    }
}

// {test}`requirements.cli.skill_structure.habeas`
#[test]
fn the_habeas_skill_is_home_of_routing_inspection_syntax_and_shared_gotchas() {
    let raw = skill_md("habeas");
    assert_text(
        "routing",
        &section(raw, "## Workflow"),
        &["| Task | Skill |"],
    );
    assert_text(
        "inspecting",
        &section(raw, "## Inspecting"),
        &["`habeas status [<selection>]`", "`habeas info <selection>`"],
    );
    assert_text(
        "syntax",
        &section(raw, "## Requirements, examples, and definitions"),
        &[
            "fenced `{req}` directive",
            "An `{example}`",
            "A `{definition}`",
        ],
    );
    assert_text(
        "references",
        &section(raw, "## References"),
        &[
            "Write a reference with a full name in a comment on the relevant source line",
            "Fence a reference",
        ],
    );
    assert_text(
        "gotchas",
        &section(raw, "## Gotchas"),
        &["Reference discovery and authoring read only files in the Git index"],
    );
    assert_text(
        "distributed requirements",
        skill_file("habeas", "references/distributed-requirements.md"),
        &["# Distributed requirements"],
    );
    assert_text(
        "syntax details",
        skill_file("habeas", "references/syntax-details.md"),
        &["## Full names in nested chapters", "## Anchors"],
    );
    assert_home("habeas");
}

// {test}`requirements.cli.skill_structure.requirements`
#[test]
fn the_requirements_skill_is_home_of_drafting_review_references_and_criteria() {
    let raw = skill_md("habeas-requirements");
    for (phrase, topic) in [
        ("Draft. Write drafts", "drafting"),
        ("Review. Sort the new and changed requirements", "reviewing"),
        ("**Gate:**", "agreeing"),
    ] {
        assert!(
            workflow_steps("habeas-requirements")
                .iter()
                .any(|(_, text)| text.contains(phrase)),
            "{topic}"
        );
    }
    assert_text(
        "choosing references",
        &section(raw, "## Choosing references"),
        &["A *root* is"],
    );
    assert_text(
        "what the review checks",
        &section(raw, "## What the review checks"),
        &[
            "obligations, boundaries, and failure cases no test runs and asserts",
            "The supervising agent must still check that the code fulfills each applicable requirement",
        ],
    );
    assert_text(
        "requirement review criteria",
        &section(
            skill_file("habeas-requirements", "references/review-prompt.md"),
            "## Criteria",
        ),
        &["**Compound:**", "**Checkable:**", "**One reading:**"],
    );
    assert_home("habeas-requirements");
}

// {test}`requirements.cli.skill_structure.evidence`
#[test]
fn the_evidence_skill_is_home_of_authoring_findings_and_assessment_outcomes() {
    let raw = skill_md("habeas-evidence");
    assert_text(
        "authoring workflow",
        &section(raw, "## Workflow"),
        &["`habeas author --all`"],
    );
    assert_text(
        "fixing findings",
        &section(raw, "## Fixing findings"),
        &[
            "| Finding | Action |",
            "### Missing test and example references",
        ],
    );
    assert_text(
        "disagreeing assessments",
        &section(raw, "## Disagreeing assessments"),
        &["The goal is"],
    );
    assert_text(
        "commit message",
        &section(raw, "## Rules"),
        &["in the message of the commit that makes it"],
    );
    assert_text(
        "FIXME judgments",
        skill_file("habeas-evidence", "references/fixme.md"),
        &["# FIXME judgments"],
    );
    let reviews = skill_file("habeas-evidence", "references/reviews.md");
    assert_text(
        "reviews",
        reviews,
        &["## Citation reviews", "## Site reviews"],
    );
    assert_home("habeas-evidence");
}

// {test}`requirements.cli.skill_structure.bootstrap`
#[test]
fn the_bootstrap_skill_is_home_of_deriving_an_initial_requirement_set() {
    let raw = skill_md("habeas-bootstrap-requirements");
    for heading in [
        "## Inventories",
        "## Proposing requirements",
        "## Tagging",
        "## Report",
    ] {
        section(raw, heading);
    }
    assert_home("habeas-bootstrap-requirements");
}

// {test}`requirements.cli.skill_structure.refactor`
#[test]
fn the_refactor_skill_is_home_of_moves_and_requirement_renames() {
    assert_text(
        "habeas-refactor workflow",
        &section(skill_md("habeas-refactor"), "## Workflow"),
        &[
            "move the `{req}` directive to its new chapter",
            "a file or directory: `--map src/config.rs=src/settings.rs`",
            "a node: `--map",
            "a definition: `--map requirements.cli.check=requirements.core.check`",
        ],
    );
    assert_phrases(
        "habeas-refactor",
        &["Renaming a requirement's ID is a definition mapping"],
    );
    assert_home("habeas-refactor");
}

/// A bundled skill's front matter as key and raw value pairs, one per line.
/// Panics unless every line is a `key: value` pair whose value, if it is a
/// plain (unquoted) YAML scalar, contains no `": "`, which YAML rejects there.
// {cite}`habeas:util`
fn front_matter(name: &str) -> Vec<(&'static str, &'static str)> {
    let raw = skill_md(name);
    let body = raw
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
        .unwrap_or_else(|| panic!("{name} has no front matter"))
        .0;
    body.lines()
        .map(|line| {
            let (key, value) = line
                .split_once(": ")
                .unwrap_or_else(|| panic!("{name}: {line:?} is not `key: value`"));
            assert!(
                !key.is_empty()
                    && key
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                "{name}: key {key:?}"
            );
            let quoted = value.len() >= 2
                && ((value.starts_with('"') && value.ends_with('"'))
                    || (value.starts_with('\'') && value.ends_with('\'')));
            if !quoted {
                assert!(
                    !value.contains(": ") && !value.ends_with(':'),
                    "{name}: plain YAML scalar for {key} contains \": \": {value}"
                );
                assert!(
                    !value.contains(" #"),
                    "{name}: {key} contains a YAML comment"
                );
            }
            (key, value)
        })
        .collect()
}

// {test}`requirements.cli.skill_conventions`
#[test]
fn bundled_skill_descriptions_say_when_to_use_them() {
    // Each skill's front matter is valid YAML with the name and description
    // Symposium requires; a skill whose front matter fails to parse is skipped.
    for name in SKILLS {
        let fields = front_matter(name);
        let field = |key: &str| {
            let values = fields
                .iter()
                .filter(|(k, _)| *k == key)
                .map(|(_, v)| *v)
                .collect::<Vec<_>>();
            assert_eq!(values.len(), 1, "{name}: {key}");
            values[0]
        };
        assert_eq!(field("name"), name);
        let description = field("description");
        assert!(!description.trim().is_empty(), "{name}");
        assert!(
            description.chars().count() <= 1024,
            "{name}: description has {} characters",
            description.chars().count()
        );
        assert_eq!(description, self::description(name));
    }
    for (name, when) in [
        (
            "habeas",
            "Use at the start of any work in a project that tracks requirements with Habeas",
        ),
        (
            "habeas-requirements",
            "Use during design sessions and before changing code, tests, or requirement text",
        ),
        (
            "habeas-evidence",
            "Use when Habeas evidence needs creating or refreshing",
        ),
        (
            "habeas-bootstrap-requirements",
            "Use when adopting Habeas in an existing codebase",
        ),
        (
            "habeas-refactor",
            "Use when a change moves code, tests, or requirements",
        ),
    ] {
        let description = description(name);
        assert!(description.starts_with(when), "{name}: {description}");
        // A skill a description names is a bundled one.
        for word in description.split(|c: char| !(c.is_alphanumeric() || c == '-')) {
            if word.starts_with("habeas-") {
                assert!(SKILLS.contains(&word), "{name} names {word}");
            }
        }
    }
    // Skills an agent could confuse name the other to use instead.
    for (name, instead) in [
        ("habeas-requirements", "use habeas-refactor"),
        ("habeas-requirements", "use habeas-evidence"),
        ("habeas-evidence", "use habeas-refactor first"),
        ("habeas-bootstrap-requirements", "use habeas-evidence"),
        ("habeas-refactor", "use habeas-requirements"),
    ] {
        assert!(description(name).contains(instead), "{name}: {instead}");
    }
}

// {test}`requirements.cli.skill_conventions.workflow`
#[test]
fn bundled_skills_open_with_their_workflow_as_ordered_steps() {
    for name in SKILLS {
        let raw = skill_md(name);
        let first = raw.lines().find(|line| line.starts_with("## ")).unwrap();
        assert_eq!(first, "## Workflow", "{name}");
        let numbers = workflow_steps(name)
            .into_iter()
            .map(|(number, _)| number)
            .collect::<Vec<_>>();
        assert!(numbers.len() >= 2, "{name}: {numbers:?}");
        assert_eq!(
            numbers,
            (1..=numbers.len() as u32).collect::<Vec<_>>(),
            "{name}"
        );
    }
}

// {test}`requirements.cli.skill_conventions.next`
#[test]
fn bundled_skill_workflows_end_by_naming_the_skill_for_the_next_task() {
    for (name, next) in [
        // The habeas skill's next task is the one its routing table picks.
        ("habeas", &["Follow that skill"][..]),
        (
            "habeas-requirements",
            &["Next, follow the habeas-evidence skill"][..],
        ),
        (
            "habeas-evidence",
            &[
                "Next: for findings that need code or tests, or requirement text that must change, return to the habeas-requirements skill",
            ][..],
        ),
        (
            "habeas-bootstrap-requirements",
            &["Next: for gap-list items the user decides, follow the habeas-requirements skill"][..],
        ),
        (
            "habeas-refactor",
            &[
                "Next, reassess what remains stale with `habeas author --all`, following the habeas-evidence skill",
                "then make any behavior or text changes as separate commits under the habeas-requirements skill",
            ][..],
        ),
    ] {
        let (_, last) = workflow_steps(name).pop().unwrap();
        assert_text(&format!("{name}'s last step"), &last, next);
    }
    // The table the habeas skill routes from names every other skill.
    let table = step("habeas", 1);
    for name in SKILLS.into_iter().filter(|name| *name != "habeas") {
        assert!(table.contains(&format!("| {name} |")), "{name}");
    }
}

// {test}`requirements.cli.skill_conventions.references`
#[test]
fn bundled_skills_say_when_to_read_each_reference_file() {
    let links = [
        (
            "habeas",
            "references/ci.md",
            "Use the [CI example](references/ci.md) when adding these steps to an existing workflow; setup configures the collector and static validation test, while CI integration belongs in the project's workflow",
        ),
        (
            "habeas",
            "references/distributed-requirements.md",
            "For an obligation that applies separately at each of many sites, such as a convention every validation function follows, read [references/distributed-requirements.md](references/distributed-requirements.md) before drafting it, and whenever you edit a `{req}` that contains `{sites}` or `{violations}`",
        ),
        (
            "habeas",
            "references/syntax-details.md",
            "If a file that implements or tests a requirement cannot carry a comment, such as a shipped skill or a stylesheet copied into other projects, or a requirement lives in a nested chapter, read [references/syntax-details.md](references/syntax-details.md)",
        ),
        (
            "habeas-requirements",
            "references/review-prompt.md",
            "For each group, start a read-only subagent, all in parallel. Tell it to read this skill's `references/review-prompt.md`",
        ),
        (
            "habeas-evidence",
            "references/authoring-failures.md",
            "If authoring fails on configuration, credentials, throttling, or a denied mandatory input, read [references/authoring-failures.md](references/authoring-failures.md)",
        ),
        (
            "habeas-evidence",
            "references/fixme.md",
            "| a real gap the user agrees to defer, a FIXME judgment, or a `{fixme}` issue that closed | read [references/fixme.md](references/fixme.md) |",
        ),
        (
            "habeas-evidence",
            "references/reviews.md",
            "| false citation or site review | read [references/reviews.md](references/reviews.md) |",
        ),
        (
            "habeas-evidence",
            "../habeas/references/ci.md",
            "For CI integration, recommend the separate collection and check steps in the Habeas [CI example](../habeas/references/ci.md); collection alone does not enforce the policy.",
        ),
        (
            "habeas-bootstrap-requirements",
            "../habeas/references/ci.md",
            "Recommend separate successful collection and `habeas coverage-check` steps in project CI, using the Habeas [CI example](../habeas/references/ci.md). Collection alone does not enforce the coverage policy.",
        ),
        (
            "habeas",
            "references/fixme.md",
            "`{fixme}` acknowledges a deferred gap (the habeas-evidence skill's `references/fixme.md`).",
        ),
        (
            "habeas-requirements",
            "references/fixme.md",
            "To defer a requirement gap the user agrees to, use `{fixme}` as the habeas-evidence skill's `references/fixme.md` describes.",
        ),
        (
            "habeas-bootstrap-requirements",
            "references/authoring-failures.md",
            "If it fails, read the habeas-evidence skill's `references/authoring-failures.md`.",
        ),
        (
            "habeas-bootstrap-requirements",
            "references/fixme.md",
            "the resulting false judgment stays on the gap list, or gets a `{fixme}` reference if the user defers it (the habeas-evidence skill's `references/fixme.md`).",
        ),
        (
            "habeas-bootstrap-requirements",
            "references/distributed-requirements.md",
            "When a convention applies on its own at many sites, such as how every validation function continues after a failed check, propose a distributed requirement instead of marking each site `{impl}`; read the habeas skill's `references/distributed-requirements.md`.",
        ),
        (
            "habeas-evidence",
            "references/authoring-failures.md",
            "If authoring fails on configuration, credentials, throttling, or a denied mandatory input, read [references/authoring-failures.md](references/authoring-failures.md).",
        ),
    ];
    for (name, _, link) in &links {
        assert!(
            collapse(skill_md(name)).contains(link),
            "{name} lacks {link:?}"
        );
    }
    let reference_guidance = [
        (
            "habeas-requirements",
            "references/review-prompt.md",
            "**Distributed:** for a requirement with `{sites}` or `{violations}` queries, apply the Do and Don't lists in the habeas skill's `references/distributed-requirements.md`.",
        ),
        (
            "habeas-evidence",
            "references/reviews.md",
            "A distributed requirement (the habeas skill's `references/distributed-requirements.md`) gets a site review for each site, authored by `habeas author --all` or, for one requirement, `habeas author sites:<full-name>`.",
        ),
        (
            "habeas",
            "references/distributed-requirements.md",
            "Don't re-run a false site review for a different result; fix the site, or tighten the queries or the text, as the habeas-evidence skill's `references/reviews.md` describes.",
        ),
    ];
    for (name, file, guidance) in &reference_guidance {
        let (_, text) = skill_files(name)
            .iter()
            .find(|(path, _)| path == file)
            .unwrap();
        assert!(
            collapse(text).contains(guidance),
            "{name}/{file} lacks {guidance:?}"
        );
    }
    let pointers = regex::Regex::new(
        r"\]\((?:[^)]*/)?references/[^)]*\.md(?:#[^)]*)?\)|`[^`\n]*references/[^`\n]*\.md`",
    )
    .unwrap();
    for name in SKILLS {
        let text = collapse(skill_md(name));
        for (file, source) in skill_files(name) {
            let source = collapse(source);
            for pointer in pointers.find_iter(&source) {
                let guidance = if *file == "SKILL.md" {
                    links
                        .iter()
                        .filter(|(skill, _, _)| *skill == name)
                        .map(|(_, _, sentence)| *sentence)
                        .collect::<Vec<_>>()
                } else {
                    reference_guidance
                        .iter()
                        .filter(|(skill, path, _)| *skill == name && path == file)
                        .map(|(_, _, sentence)| *sentence)
                        .collect::<Vec<_>>()
                };
                assert!(
                    guidance.iter().any(|sentence| {
                        source.match_indices(sentence).any(|(start, sentence)| {
                            start <= pointer.start() && pointer.end() <= start + sentence.len()
                        })
                    }),
                    "{name}/{file}: reference without checked reading guidance: {}",
                    pointer.as_str()
                );
            }
        }
        for (path, _) in &skill_files(name)[1..] {
            let found = links
                .iter()
                .filter(|(skill, file, _)| *skill == name && file == path)
                .collect::<Vec<_>>();
            assert!(!found.is_empty(), "{name} links {path} with no check here");
            for (_, _, link) in found {
                assert!(text.contains(link), "{name} lacks {link:?}");
            }
        }
    }
}

// {test}`requirements.cli.overview_skill`
#[test]
fn the_habeas_skill_is_used_at_the_start_of_any_work_in_a_habeas_project() {
    let description = description("habeas");
    assert!(description.starts_with(
        "Use at the start of any work in a project that tracks requirements with Habeas"
    ));
    for sign in [
        "a `habeas` dev-dependency",
        "a `habeas.toml`",
        "`{req}` directives in its mdBook",
        "`{impl}`/`{test}`/`{cite}` references in code",
    ] {
        assert!(description.contains(sign), "{sign}");
    }
}

// {test}`requirements.cli.overview_skill.routing`
#[test]
fn the_habeas_skill_routes_each_task_to_the_skill_that_covers_it() {
    let rows = skill_md("habeas")
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('|'))
        .collect::<Vec<_>>();
    for (task, skill) in [
        (
            &[
                "A design session",
                "any change to behavior, code, tests, or requirement text",
                "including renaming code in place",
            ][..],
            "habeas-requirements",
        ),
        (
            &[
                "Moving code, tests, or requirements to another file, chapter, or outline node",
                "renaming a requirement",
            ][..],
            "habeas-refactor",
        ),
        (
            &[
                "Creating or refreshing evidence",
                "`habeas coverage` or `habeas status` reports Habeas findings",
            ][..],
            "habeas-evidence",
        ),
        (
            &["Adopting Habeas in an existing codebase"][..],
            "habeas-bootstrap-requirements",
        ),
    ] {
        let row = rows
            .iter()
            .find(|row| task.iter().all(|part| row.contains(part)))
            .unwrap_or_else(|| panic!("no row for {task:?}"));
        let cells = row
            .split('|')
            .map(str::trim)
            .filter(|cell| !cell.is_empty())
            .collect::<Vec<_>>();
        assert_eq!(cells.last(), Some(&skill), "{row}");
    }
    assert!(step("habeas", 1).starts_with("Pick the skill for the task"));
}

// {test}`requirements.cli.overview_skill.move_then_edit`
#[test]
fn the_habeas_skill_does_a_move_that_also_edits_as_the_move_first() {
    assert_text(
        "habeas workflow",
        &step("habeas", 2),
        &[
            "A change that both moves and edits code, tests, or requirements is the move first (habeas-refactor), then the edit under the skill the table gives for it",
        ],
    );
}

// {test}`requirements.cli.overview_skill.setup`
#[test]
fn the_habeas_skill_asks_before_setup_and_does_no_work_without_it() {
    assert_text(
        "habeas workflow",
        &step("habeas", 1),
        &[
            "If Habeas is not configured yet (no `tests/habeas.rs` calling `habeas::validate()`), ask the user before running `habeas setup`",
            "if setup is not completed, do no Habeas requirements work in the project",
        ],
    );
}

// {test}`requirements.cli.overview_skill.new_project`
#[test]
fn the_habeas_skill_starts_a_new_project_with_the_requirements_skill() {
    let routing = step("habeas", 1);
    assert_text(
        "habeas workflow",
        &routing,
        &[
            "Adopting Habeas in an existing codebase, or most of the code has no references | habeas-bootstrap-requirements",
            "A new project with little code starts with habeas-requirements, not habeas-bootstrap-requirements, even though most of its code has no references",
        ],
    );
}

// {test}`requirements.cli.requirements_skill`
#[test]
fn the_requirements_skill_is_used_for_design_and_before_any_change() {
    let description = description("habeas-requirements");
    for phrase in [
        "Use during design sessions and before changing code, tests, or requirement text in a project that tracks requirements with Habeas",
        "even when the user only asks for a feature, bug fix,",
        "and never says \"requirement\"",
    ] {
        assert!(description.contains(phrase), "{phrase}");
    }
}

// {test}`requirements.cli.requirements_review_skill`
#[test]
fn the_requirements_skill_commits_agreed_requirements_before_code() {
    const SKILL: &str = "habeas-requirements";
    assert_phrases(
        SKILL,
        &[
            "Every end-user-observable result of the code, including errors and input handling, must follow from a requirement the user agreed to",
        ],
    );
    let draft = step_of(SKILL, "Write drafts");
    let review = step_of(SKILL, "start a read-only subagent");
    let gate = step_of(
        SKILL,
        "**Gate:** no implementation or test code, and no commit, until the user has agreed to, rejected, or deferred every group",
    );
    let commit = step_of(
        SKILL,
        "then commit it, all groups together, before any code",
    );
    let implement = step_of(SKILL, "Implement and test");
    assert!(draft < review && review < gate && gate < commit && commit < implement);
}

// {test}`requirements.cli.requirements_review_skill.agreed_text`
#[test]
fn the_requirements_skill_reviews_every_change_to_agreed_text() {
    assert!(
        description("habeas-requirements")
            .contains("before changing code, tests, or requirement text")
    );
    assert_eq!(
        step_of(
            "habeas-requirements",
            "Sort the new and changed requirements into groups"
        ),
        3
    );
    assert_phrases(
        "habeas-requirements",
        &[
            "Every change to agreed text, including a rewording for precision or removing a requirement, goes through steps 3 to 5 before it is committed",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.no_change`
#[test]
fn the_requirements_skill_skips_drafting_when_no_requirement_changes() {
    const SKILL: &str = "habeas-requirements";
    assert_text(
        "step 1",
        &step(SKILL, 1),
        &[
            "If the change adds, alters, or removes no observable behavior",
            "or an existing requirement already specifies it (most bug fixes), tell the user which requirement covers it, or that none is needed, and go to step 6",
        ],
    );
    assert_eq!(step_of(SKILL, "Implement and test"), 6);
}

// {test}`requirements.cli.requirements_review_skill.drafts`
#[test]
fn the_requirements_skill_writes_drafts_as_unstaged_edits_without_markers() {
    assert_text(
        "draft step",
        &step(
            "habeas-requirements",
            step_of("habeas-requirements", "Write drafts"),
        ),
        &[
            "Write drafts from the request straight into the chapter as uncommitted, unstaged edits, with no draft marker",
            "For a new chapter, add it to `SUMMARY.md` and `git add -N` it",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.drafts.rough`
#[test]
fn the_requirements_skill_allows_rough_drafts_before_review() {
    assert_text(
        "draft step",
        &step(
            "habeas-requirements",
            step_of("habeas-requirements", "Write drafts"),
        ),
        &[
            "Drafts may be high-level, compound, or incomplete",
            "Ask the user only what the request can't answer; the review in step 3 finds the rest",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.drafts.foreign_edits`
#[test]
fn the_requirements_skill_asks_about_foreign_edits_before_drafting() {
    const SKILL: &str = "habeas-requirements";
    let ask = step_of(
        SKILL,
        "If it has uncommitted edits you did not make in this task, ask the user whether they belong to it",
    );
    assert!(step(SKILL, ask).contains("Run `git status` on the book's source directory"));
    assert!(ask < step_of(SKILL, "Write drafts"));
}

// {test}`requirements.cli.requirements_review_skill.review`
#[test]
fn the_requirements_skill_reviews_each_group_with_a_parallel_read_only_subagent() {
    const SKILL: &str = "habeas-requirements";
    let review = step_of(SKILL, "start a read-only subagent");
    assert_text(
        "review step",
        &step(SKILL, review),
        &[
            "Sort the new and changed requirements into groups",
            "requirements that cite each other or describe one feature",
            "For each group, start a read-only subagent, all in parallel",
        ],
    );
    assert!(review < step_of(SKILL, "Present each group to the user"));
}

// {test}`requirements.cli.requirements_review_skill.review.inputs`
#[test]
fn the_requirements_skill_gives_reviewers_the_prompt_names_and_brief() {
    assert_text(
        "review step",
        &step(
            "habeas-requirements",
            step_of("habeas-requirements", "start a read-only subagent"),
        ),
        &[
            "Tell it to read this skill's `references/review-prompt.md` and the habeas skill's `SKILL.md` by absolute path",
            "give it the group's full names and a brief: what the user asked for, the decisions already made, and the open choices already noted",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.review.fallback`
#[test]
fn the_requirements_skill_reviews_groups_itself_without_subagents() {
    const SKILL: &str = "habeas-requirements";
    let fallback = step_of(
        SKILL,
        "If you can't start subagents, review each group yourself against the prompt, one group at a time",
    );
    assert!(fallback < step_of(SKILL, "Present each group to the user"));
}

// {test}`requirements.cli.requirements_review_skill.present`
#[test]
fn the_requirements_skill_presents_exact_text_and_design_questions() {
    assert_phrases(
        "habeas-requirements",
        &[
            "Present each group to the user: its exact text, including any `{sites}` and `{violations}` queries, and the design questions the review returned, each with its options and your recommendation",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.present.apply`
#[test]
fn the_requirements_skill_applies_and_reconciles_reviews_before_presenting() {
    const SKILL: &str = "habeas-requirements";
    let apply = step_of(SKILL, "Apply the revised text the reviews return");
    assert_text(
        "review step",
        &step(SKILL, apply),
        &[
            "reconcile reviews that clash, such as two that split the same requirement differently",
            "a clash whose resolution changes what is promised becomes a design question",
        ],
    );
    assert!(apply < step_of(SKILL, "Present each group to the user"));
}

// {test}`requirements.cli.requirements_review_skill.present.mechanical`
#[test]
fn the_requirements_skill_shows_mechanical_changes_without_asking() {
    assert_phrases(
        "habeas-requirements",
        &[
            "Show text that was already agreed or that the user supplied as a diff against that text, then a \"Mechanical:\" list of the changes",
            "Don't raise mechanical changes to new text as questions",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.present.outside`
#[test]
fn the_requirements_skill_shows_edits_outside_a_group_and_added_examples() {
    assert_in_order(
        "habeas-requirements",
        &[
            "as a diff against that text, then a \"Mechanical:\" list",
            "Show the edits reviewers made outside their group the same way, and list added `{example}`s, which each need a test",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.present.rereview`
#[test]
fn the_requirements_skill_rereviews_a_group_a_decision_extends() {
    assert_text_in_order(
        "present step",
        &step(
            "habeas-requirements",
            step_of("habeas-requirements", "Present each group to the user"),
        ),
        &[
            "Apply the user's decisions",
            "If a decision adds an obligation no reviewer saw, review that group again",
            "**Gate:**",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.agreement`
#[test]
fn the_requirements_skill_gates_commit_and_code_on_every_group_being_decided() {
    assert_phrases(
        "habeas-requirements",
        &[
            "**Gate:** no implementation or test code, and no commit, until the user has agreed to, rejected, or deferred every group",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.agreement.what_counts`
#[test]
fn the_requirements_skill_says_what_counts_as_agreement() {
    assert_phrases(
        "habeas-requirements",
        &[
            "Exact text the user supplied, or explicit agreement earlier in the conversation, counts; a general request to build the feature does not",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.agreement.no_user`
#[test]
fn the_requirements_skill_stops_and_reports_when_it_cannot_ask_the_user() {
    assert_phrases(
        "habeas-requirements",
        &[
            "If you cannot ask the user in this session, stop, leave the drafts uncommitted, and report them with the design questions",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.commit`
#[test]
fn the_requirements_skill_commits_every_group_together_before_code() {
    const SKILL: &str = "habeas-requirements";
    let commit = step_of(
        SKILL,
        "then commit it, all groups together, before any code",
    );
    assert!(step(SKILL, commit).contains("leave out unrelated edits found in step 1"));
    assert!(commit > step_of(SKILL, "**Gate:**"));
}

// {test}`requirements.cli.requirements_review_skill.commit.revert`
#[test]
fn the_requirements_skill_reverts_rejected_and_deferred_drafts_before_committing() {
    assert_text_in_order(
        "commit step",
        &step(
            "habeas-requirements",
            step_of("habeas-requirements", "all groups together"),
        ),
        &[
            "Revert drafts the user rejected or deferred (`git restore -p`) and list them in your report",
            "then commit it",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.commit.check`
#[test]
fn the_requirements_skill_checks_the_book_holds_exactly_the_agreed_text() {
    assert_phrases(
        "habeas-requirements",
        &["Check with `git diff` that the book holds exactly the agreed text, then commit it"],
    );
}

// {test}`requirements.cli.requirements_review_skill.commit.validation`
#[test]
fn the_requirements_skill_expects_the_requirements_commit_to_fail_validation() {
    assert_phrases(
        "habeas-requirements",
        &[
            "This commit fails validation until evidence is authored; that is expected, as only the pull request as a whole must pass",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.commit.design_session`
#[test]
fn the_requirements_skill_stops_after_the_commit_in_a_design_session() {
    assert_text_in_order(
        "commit step",
        &step(
            "habeas-requirements",
            step_of("habeas-requirements", "all groups together"),
        ),
        &[
            "then commit it",
            "In a design session with no code to write, stop here",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.text_change`
#[test]
fn the_requirements_skill_stops_dependent_work_when_agreed_text_must_change() {
    assert_phrases(
        "habeas-requirements",
        &[
            "If implementation shows that agreed text must change, stop work that depends on it and take the changed requirements through steps 3 to 5 first",
        ],
    );
    for (number, start) in [(3, "Review."), (4, "Present"), (5, "Commit.")] {
        assert!(step("habeas-requirements", number).starts_with(start));
    }
}

// {test}`requirements.cli.requirements_review_skill.text_change.uncovered`
#[test]
fn the_requirements_skill_drafts_a_requirement_for_uncovered_behavior() {
    assert_phrases(
        "habeas-requirements",
        &["Behavior no requirement covers goes back to step 2"],
    );
    assert!(step("habeas-requirements", 2).starts_with("Draft."));
}

// {test}`requirements.cli.requirements_review_skill.text_change.rename`
#[test]
fn the_requirements_skill_renames_requirements_through_refactor_before_editing() {
    assert_phrases(
        "habeas-requirements",
        &[
            "Renaming or moving a committed requirement is a habeas-refactor task, agreed with the user and committed before its text changes",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.code_rename`
#[test]
fn the_requirements_skill_needs_no_review_for_renaming_code_in_place() {
    assert_phrases(
        "habeas-requirements",
        &[
            "Restructuring code in place, including renaming an item, needs no requirement review, but references move with the code and the evidence is reassessed",
        ],
    );
}

/// The review prompt the habeas-requirements skill supplies, raw.
// {cite}`habeas:util`
fn review_prompt() -> &'static str {
    skill_file("habeas-requirements", "references/review-prompt.md")
}

/// The review prompt's text from `start` up to `end`, collapsed.
// {cite}`habeas:util`
fn review_prompt_between(start: &str, end: &str) -> String {
    let text = collapse(review_prompt());
    let (_, rest) = text.split_once(start).unwrap_or_else(|| panic!("{start}"));
    let (span, _) = rest.split_once(end).unwrap_or_else(|| panic!("{end}"));
    span.to_owned()
}

// {test}`requirements.cli.requirements_review_skill.review_prompt`
#[test]
fn the_review_prompt_makes_mechanical_changes_and_returns_design_questions() {
    assert_text(
        "review prompt",
        &collapse(review_prompt()),
        &[
            "Don't edit files; make your changes in the revised text you return",
            "**Mechanical changes:** make them yourself in the revised text, and never phrase them as questions",
            "**Design questions:** return these; the user decides",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.review_prompt.context`
#[test]
fn the_review_prompt_reads_the_syntax_drafts_chapter_and_related_requirements() {
    assert_text(
        "review prompt",
        &collapse(review_prompt()),
        &[
            "Read the habeas skill's `SKILL.md` first for the directive syntax",
            "The drafts are uncommitted edits in the book: `git diff` on the book's source directory shows them",
            "Read the surrounding chapter and every requirement the group cites or overlaps",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.review_prompt.brief`
#[test]
fn the_review_prompt_does_not_reraise_the_brief() {
    assert_text_in_order(
        "review prompt",
        &collapse(review_prompt()),
        &[
            "the decisions already made, and the open choices already noted",
            "Don't re-raise those",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.review_prompt.mechanical`
#[test]
fn the_review_prompt_lists_the_mechanical_changes() {
    assert_text(
        "mechanical changes",
        &review_prompt_between("**Mechanical changes:**", "**Design questions:**"),
        &[
            "Split a compound requirement",
            "Clarify an obligation whose intent is clear while preserving whether behavior is required, recommended, or permitted",
            "Do not automatically convert ordinary prose or commented API declarations to RFC keywords, or strengthen or weaken what the draft promises",
            "Fix wording whose resolution is obvious: vague terms, inconsistent terms, an ambiguity with one sensible reading",
            "Add an `{example}` where a concrete case pins an obligation down",
            "Update other requirements a change makes stale",
        ],
    );
    assert_text(
        "mechanical changes",
        &review_prompt_between("**Mechanical changes:**", "**Design questions:**"),
        &["make them yourself in the revised text, and never phrase them as questions"],
    );
}

// {test}`requirements.cli.requirements_review_skill.review_prompt.names`
#[test]
fn the_review_prompt_never_renames_and_names_split_parts() {
    assert_text(
        "mechanical changes",
        &review_prompt_between("**Mechanical changes:**", "**Design questions:**"),
        &[
            "Never rename or move a committed requirement; if its name is wrong, raise it as a design question",
            "When you split one, keep its full name on the core obligation and name the parts `<name>.<part>`",
            "List every new name",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.review_prompt.design`
#[test]
fn the_review_prompt_lists_the_design_questions_with_options_and_recommendation() {
    assert_text(
        "design questions",
        &review_prompt_between("**Design questions:**", "## Finding gaps"),
        &[
            "A gap: a case the text says nothing about, where either answer is plausible",
            "A conflict with another requirement; cite it by full name",
            "Whether behavior is required, recommended, or permitted, when the choice depends on the user's intent",
            "An obligation nothing could check, where any fix changes what is promised",
            "For each question give why it matters, the options, and your recommendation",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.review_prompt.compound`
#[test]
fn the_review_prompt_splits_compound_requirements() {
    assert_text(
        "criteria",
        &section(review_prompt(), "## Criteria"),
        &[
            "**Compound:** a requirement that lists several independent forms, checks, or outputs",
            "An additional promise introduced with \"also\" is a hint, regardless of whether the text uses RFC keywords",
            "could one part be implemented, change, or fail while another holds? Then split it",
            "Move shared detail, such as a template or a format, into its own requirement that the others cite",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.review_prompt.criteria`
#[test]
fn the_review_prompt_criteria_find_ambiguous_uncheckable_and_prescriptive_text() {
    assert_text(
        "criteria",
        &section(review_prompt(), "## Criteria"),
        &[
            "**Checkable:** a reviewer can judge the obligation from the requirement text, the code, and the tests, and something would fail if it were broken",
            "**One reading:** text a careful reviewer could read two ways is a finding",
            "**Outcome, not implementation:** the text says what holds",
            "Public API declarations specify an observable interface",
            "their accompanying comments can specify semantic guarantees",
            "Distinguish these from prescribing internal implementation details that the contract does not need",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.review_prompt.distributed`
#[test]
fn the_review_prompt_applies_the_distributed_guidance() {
    assert_text(
        "criteria",
        &section(review_prompt(), "## Criteria"),
        &[
            "for a requirement with `{sites}` or `{violations}` queries, apply the Do and Don't lists in the habeas skill's `references/distributed-requirements.md`",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.review_prompt.gaps`
#[test]
fn the_review_prompt_finds_gaps_case_by_case() {
    assert_text(
        "finding gaps",
        &section(review_prompt(), "## Finding gaps"),
        &[
            "For each requirement, list the inputs and states it applies to, its boundaries and failure cases (empty, missing, duplicate, invalid), and its observable outputs",
            "Then list the observable behavior the brief implies that no requirement in the group or the book states",
            "Each case the text does not decide is a gap: resolve it mechanically if only one answer is sensible, otherwise ask",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.review_prompt.examples`
#[test]
fn the_review_prompt_works_a_mechanical_example_and_a_design_question() {
    assert_text_in_order(
        "examples",
        &section(review_prompt(), "## Examples"),
        &[
            "Compound, with a mechanical fix:",
            "> The `refactor` command accepts",
            "Split into",
            "Not a question",
            "A design question:",
            "Options:",
            "Recommend",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.review_prompt.report`
#[test]
fn the_review_prompt_gives_a_report_template() {
    assert_text_in_order(
        "report",
        &section(review_prompt(), "## Report"),
        &[
            "### Revised text",
            "as complete directives ready to paste, including new split requirements",
            "### Edits outside the group",
            "### Mechanical changes",
            "list new full names and added examples",
            "### Design questions",
        ],
    );
}

// {test}`requirements.cli.requirements_review_skill.review_prompt.no_questions`
#[test]
fn the_review_prompt_rejects_a_report_with_only_wording_changes() {
    assert_text_in_order(
        "report",
        &section(review_prompt(), "## Report"),
        &[
            "A report with only wording changes means you have not looked hard enough for gaps and for conflicts",
            "Look again",
            "If you still find no design questions, write \"No design questions.\"",
        ],
    );
}

// {test}`requirements.cli.bootstrap_skill`
#[test]
fn bootstrap_skill_is_bundled_and_directs_complete_inventories() {
    assert!(SKILLS.contains(&"habeas-bootstrap-requirements"));
    assert!(
        description("habeas-bootstrap-requirements")
            .starts_with("Use when adopting Habeas in an existing codebase"),
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
            "every function, method, struct, enum, and union, listed from the source to discover behavior no requirement states",
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

// {test}`requirements.cli.bootstrap_skill.review`
#[test]
fn bootstrap_skill_reviews_inferred_requirements_before_committing_or_tagging() {
    const SKILL: &str = "habeas-bootstrap-requirements";
    assert_phrases(SKILL, &["every requirement starts as a proposal"]);
    let propose = step_of(SKILL, "Propose requirements for the area");
    let review = step_of(
        SKILL,
        "Review, agree, and commit them through habeas-requirements steps 3 to 5",
    );
    let tag = step_of(SKILL, "Tag the area's tests and root items");
    assert!(propose < review && review < tag);
    assert!(step(SKILL, propose).contains("proposing any changes to their text"));
    assert_text(
        "review step",
        &step(SKILL, review),
        &[
            "a general request to bootstrap is not agreement to specific text",
            "don't commit a requirement or tag code against it until the user agrees to its text",
            "Remove drafts the user rejects, defers, or has not decided from the chapter, and keep them on the gap list",
        ],
    );
    // Each area's requirements are committed together, as habeas-requirements
    // commits all groups, before that area is tagged.
    assert!(
        section(skill_md(SKILL), "## Workflow")
            .contains(&format!("Repeat steps {propose} to 7 for each area"))
    );
    assert_phrases("habeas-requirements", &["commit it, all groups together"]);
}

// {test}`requirements.cli.pending_evidence_guidance`
#[test]
fn the_evidence_skill_reports_findings_expected_while_implementation_is_pending() {
    assert_text(
        "report step",
        &step(
            "habeas-evidence",
            step_of(
                "habeas-evidence",
                "Stop and report each finding that needs the user",
            ),
        ),
        &[
            "a missing record or false judgment whose rationale names only implementation that is still pending, which is expected",
            "Don't keep iterating on those",
        ],
    );
}

// {test}`requirements.cli.pending_evidence_guidance.no_placeholder`
#[test]
fn the_evidence_skill_never_writes_placeholder_evidence_for_pending_work() {
    assert_text_in_order(
        "report step",
        &step(
            "habeas-evidence",
            step_of(
                "habeas-evidence",
                "Stop and report each finding that needs the user",
            ),
        ),
        &[
            "names only implementation that is still pending",
            "never write placeholder evidence",
        ],
    );
}

// {test}`requirements.authoring.batch`
#[test]
fn bundled_skills_direct_the_agent_to_the_authoring_harness() {
    // habeas-evidence is the home of the authoring workflow.
    assert_phrases(
        "habeas-evidence",
        &[
            "`habeas author --all` assesses every definition whose evidence is missing, stale, or invalid and skips current ones",
            "Never write or edit evidence JSON, determinations, rationales, or inputs yourself, and never hand that to another agent",
            "Fix the source, tests, references, or requirement text instead",
        ],
    );
    // The other skills run the command or defer to habeas-evidence.
    assert_phrases(
        "habeas-requirements",
        &["Next, follow the habeas-evidence skill to author evidence and fix what it reports"],
    );
    assert_phrases(
        "habeas-bootstrap-requirements",
        &[
            "Citation reviews are authored only by `habeas author --all`",
            "Follow the habeas-evidence workflow for this area's definitions",
            "Never write evidence yourself or ask another agent to",
        ],
    );
    assert_phrases(
        "habeas",
        &["Never write or edit evidence yourself, or ask another agent to"],
    );
}

// {test}`requirements.cli.info`
#[test]
fn bundled_skills_inspect_definitions_with_habeas_info() {
    assert_phrases(
        "habeas",
        &[
            "`habeas info <selection>` prints requirement and example text, evidence state, rationale, inputs, and validation findings",
        ],
    );
    assert_phrases(
        "habeas-requirements",
        &["`habeas info <selection>` prints a requirement's text and state"],
    );
    assert_phrases(
        "habeas-evidence",
        &["`habeas info <name>` shows one definition's judgment, rationale, and findings"],
    );
}

// {test}`requirements.references.anchors.guidance`
#[test]
fn bundled_skills_limit_anchors_to_files_whose_items_cannot_carry_references() {
    assert_text(
        "syntax details",
        &section(
            skill_file("habeas", "references/syntax-details.md"),
            "## Anchors",
        ),
        &[
            "When a file that implements or tests a requirement cannot carry a reference",
            "Don't anchor a file whose items can carry a reference themselves",
        ],
    );
}

// {test}`requirements.cli.internal_detail_guidance`
// {test}`requirements.cli.behavior_review_guidance`
// {test}`requirements.references.procedure`
#[test]
fn bundled_skills_direct_the_reference_procedure_and_allow_unmarked_helpers() {
    // habeas-requirements is the home of the reference procedure; its steps
    // come in order.
    assert_text_in_order(
        "choosing references",
        &section(skill_md("habeas-requirements"), "## Choosing references"),
        &[
            "a type does when the requirement constrains its fields, variants, serialized form, or validation",
            "For each requirement or example an item could reference",
            "1. A test designed to verify it gets `{test}`",
            "2. Otherwise a root gets `{impl}`",
            "3. Otherwise, code that should be re-checked when the requirement changes gets `{cite}`",
            "4. Otherwise, leave it unmarked",
            "An optional `` // {cite}`habeas:util` `` can explain that an item serves other code; it is not required bookkeeping",
            "propose a requirement stating the invariant and `{impl}` it",
            "Don't propose grouping requirements that state no checkable obligation",
            "An item that neither carries out a requirement nor serves other code needs a requirement or is not needed: report it to the user with a proposed requirement, or as dead code if nothing reaches it. Don't delete it unasked",
        ],
    );
    // The other skills refer to the procedure rather than restate it.
    assert_phrases(
        "habeas-evidence",
        &[
            "Ordinary functions, types, trait methods, and test helpers need no individual book connection",
            "Keep meaningful `{impl}` roots and optional explanatory citations",
        ],
    );
    assert_phrases(
        "habeas-bootstrap-requirements",
        &[
            "\"Choosing references\" in the habeas-requirements skill",
            "write a requirement for the invariant they check",
            "An item that neither carries out a requirement nor serves other code goes on the gap list: it needs a requirement or is not needed",
        ],
    );
    for name in SKILLS
        .into_iter()
        .filter(|name| *name != "habeas-requirements")
    {
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
        let reference_guidance = text.replace("no explanation can waive them", "");
        for waiver in ["may waive", "can waive", "waiver applies", "is exempt"] {
            assert!(
                !reference_guidance.to_lowercase().contains(waiver),
                "{name} offers a waiver"
            );
        }
    }
}

// {test}`requirements.cli.behavior_review_guidance`
// {test}`requirements.references.procedure`
#[test]
fn bundled_skills_direct_behavior_review_against_requirements() {
    // The three outcomes.
    assert_phrases(
        "habeas-evidence",
        &[
            "A judgment is one of three outcomes",
            "`true`: the reviewed tests adequately check the definition, or its implementations support its structural obligations, with no gap under",
            "`false`: there is a gap that no `{fixme}` reference acknowledges",
            "FIXME: `{fixme}` references acknowledge every remaining gap",
        ],
    );
    // Every observable branch, and no unspecified behavior.
    assert_phrases(
        "habeas-requirements",
        &[
            "Every end-user-observable result of the code, including errors and input handling, must follow from a requirement the user agreed to",
        ],
    );
    assert_phrases(
        "habeas-bootstrap-requirements",
        &[
            "distinct observable branches",
            "every observable branch has been reviewed",
        ],
    );
    assert_phrases(
        "habeas-evidence",
        &["Behavior with no requirement needs one agreed with the user"],
    );
    // The requirement and example review criteria, including test adequacy and
    // justified absence of tests; habeas-requirements is their home.
    assert_text(
        "what the review checks",
        &section(skill_md("habeas-requirements"), "## What the review checks"),
        &[
            "The intended review checks stated structure and behavior independently",
            "implementation roots support signatures, relationships, ownership, and required",
            "delegation; tests support behavioral obligations",
            "Missing implementation or",
            "test edges must not hide an obligation needing that evidence",
            "Examples and distributed requirements retain",
            "their existing review tasks",
            "Issue #51",
            "remaining independent-check and regression-test gaps",
            "assertions whose expected results contradict the requirement",
            "too weak to rule out plausible requirement-violating behavior",
            "an applicable recommendation with inadequate tests",
            "text with two equally supported readings that lead to different testing judgments",
            "adequacy gap, explain what requirement-violating behavior those tests would accept",
            "check whether another reviewed test rules it out",
            "map each obligation assessed here to concrete test inputs and assertions",
            "Identify delegated obligations and established testing exemptions separately",
            "a standalone test designed to verify the subject without `{test}`",
            "The supervising agent must still check that the code fulfills each applicable requirement",
            "obligations, boundaries, and failure cases no test runs and asserts",
            "A test may be omitted only for code with no logic of its own, or code that needs network services, real time, or process-wide state no project test sets up",
            "an obligation that a value stays unchanged, distinct, or present always needs a test",
            "An example's review checks that tests' setup and assertions cover its scenario and each expected outcome",
            "One acknowledged gap does not excuse others",
        ],
    );
    assert_phrases(
        "habeas-evidence",
        &[
            "A false rationale names one of the gaps listed in \"What the review checks\" in the habeas-requirements skill",
        ],
    );
    // The three outcomes and the documented-gap convention; habeas-evidence is
    // the home of FIXME judgments.
    assert_phrases(
        "habeas-evidence",
        &[
            "| false judgment | read the rationale",
            "A FIXME judgment, such as `{\"FIXME\": [\"rust-lang/rust#123\"]}`, means `{fixme}` references to the definition acknowledge every remaining gap",
            "Validation accepts it like `true` when all other checks pass",
            "// :github: rust-lang/rust#123",
            "A plain `FIXME` or `TODO` comment does not count",
        ],
    );
    assert_phrases(
        "habeas-requirements",
        &["To defer a requirement gap the user agrees to, use `{fixme}`"],
    );
    // Roots get `{impl}`, tests `{test}`, and `{cite}` only for related code.
    assert_text(
        "choosing references",
        &section(skill_md("habeas-requirements"), "## Choosing references"),
        &[
            "1. A test designed to verify it gets `{test}`",
            "2. Otherwise a root gets `{impl}`. Leave what it reaches unmarked",
            "`{cite}` implements nothing. For source items outside the book, citation reviews check project citations' roles and built-ins' stated criteria",
            "fence outside a requirement or example opts a book passage into concordance review",
            "its claims must be supported by the cited declarations",
            "The outermost eligible fence groups nested commentary citations into one review",
        ],
    );
    assert_phrases(
        "habeas-bootstrap-requirements",
        &["`{test}` on tests and `{impl}` on roots; code a root or test reaches needs none"],
    );
    // Interacting requirements, and a citation alone is not an explanation.
    assert_phrases(
        "habeas-requirements",
        &[
            "If a behavior follows only from several requirements together, mark each with `{impl}` where it applies and add fenced commentary saying how they combine",
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
// {test}`requirements.references.procedure`
#[test]
fn bundled_skills_mark_roots_without_requiring_helper_bookkeeping() {
    const ROOT: &str = "A *root* is an item that carries out part of a requirement and that no other `{impl}` item for it reaches";
    // habeas-requirements is the home of roots: reach goes downward only.
    assert_text_in_order(
        "choosing references",
        &section(skill_md("habeas-requirements"), "## Choosing references"),
        &[
            "what an item calls, the types it uses, and the trait methods it calls, transitively",
            "Callers don't count",
            ROOT,
            "Roots are per requirement",
            "Helpers the test calls need nothing",
            "Leave what it reaches unmarked",
            "Don't remove an existing `{impl}` from code a root reaches unless the user asks",
        ],
    );
    assert_text(
        "requirements gotchas",
        &section(skill_md("habeas-requirements"), "## Gotchas"),
        &[
            "every runnable test needs a `{test}` reference",
            "Ordinary functions, types, trait methods, and test helpers need no individual book connection",
            "Recorded reads track evidence freshness",
        ],
    );
    assert_text(
        "test and example references",
        &section(
            skill_md("habeas-evidence"),
            "### Missing test and example references",
        ),
        &[
            "needs a `{test}` reference",
            "A Cargo example needs to `{cite}` a named example in the book",
            "without Git-history grace periods or unreferenced-item warnings",
        ],
    );
    assert_phrases(
        "habeas-evidence",
        &[
            "| rationale identifies an unmarked standalone test | add `{test}` for the requirement or example it verifies",
        ],
    );
    for name in [
        "habeas",
        "habeas-evidence",
        "habeas-requirements",
        "habeas-bootstrap-requirements",
    ] {
        assert!(!bundled_skill(name).contains("status --coverage"), "{name}");
    }
    // The other skills refer to the definition of a root rather than restate it.
    for name in SKILLS
        .into_iter()
        .filter(|name| *name != "habeas-requirements")
    {
        assert!(!bundled_skill(name).contains(ROOT), "{name} restates roots");
    }
}

// {test}`requirements.cli.flip_guidance`
#[test]
fn bundled_skills_treat_disagreeing_assessments_as_findings() {
    const GOAL: &str = "The goal is that independent assessments of the same subject reach the same determination, because the requirement, code, and tests leave no gap a careful reviewer could find and no reading a careful reviewer could take differently";
    // habeas-evidence is the home: the goal leads and the procedure follows.
    assert_text_in_order(
        "disagreeing assessments",
        &section(skill_md("habeas-evidence"), "## Disagreeing assessments"),
        &[
            GOAL,
            "Assessments that disagree are findings against that goal",
            "Read both assessments",
            "Decide whether one found a real gap or the requirement text admits more than one reading",
            "Fix the source or tests, or tighten the requirement text through habeas-requirements steps 3 to 5",
        ],
    );
    // Never re-run for a different determination; a change to agreed text is
    // reviewed with the user, then explained in its commit message.
    assert_text_in_order(
        "rules",
        &section(skill_md("habeas-evidence"), "## Rules"),
        &[
            "Never re-run an assessment to get a different result",
            "Fix the source, tests, references, or requirement text instead",
            "A change to agreed requirement text, even a rewording, goes through habeas-requirements steps 3 to 5 (subagent review, the user's agreement, commit) first",
            "Then give the reason, and the rationale that prompted it, in the message of the commit that makes it, describing the change relative to the prior commit rather than the drafts that led to it",
        ],
    );
    assert_phrases(
        "habeas-requirements",
        &[
            "For a change to agreed text, the message gives the reason and the rationale that prompted it, relative to the prior commit (habeas-evidence, \"Rules\")",
        ],
    );
    // The other skills point to it rather than restate it.
    assert_phrases(
        "habeas-requirements",
        &["When assessments disagree, follow its \"Disagreeing assessments\""],
    );
    assert_phrases(
        "habeas-bootstrap-requirements",
        &["follow \"Disagreeing assessments\" in the habeas-evidence skill"],
    );
    assert_text(
        "habeas gotchas",
        &section(skill_md("habeas"), "## Gotchas"),
        &[
            "When assessments of the same subject disagree, follow \"Disagreeing assessments\" in the habeas-evidence skill",
        ],
    );
    let (_, last) = workflow_steps("habeas-refactor").pop().unwrap();
    assert_text(
        "habeas-refactor's last step",
        &last,
        &[
            "following the habeas-evidence skill, and if assessments disagree, its \"Disagreeing assessments\"",
        ],
    );
    for name in SKILLS.into_iter().filter(|name| *name != "habeas-evidence") {
        assert!(
            !bundled_skill(name).contains(GOAL),
            "{name} restates the goal"
        );
    }
    // No skill keeps a separate log of requirement changes.
    for name in SKILLS {
        assert!(
            !bundled_skill(name).contains("audit log"),
            "{name} mentions an audit log"
        );
    }
}

/// The habeas skill's reference on distributed requirements, raw.
// {cite}`habeas:util`
fn distributed_reference() -> &'static str {
    skill_file("habeas", "references/distributed-requirements.md")
}

// {test}`requirements.cli.distributed_guidance`
// {test}`requirements.cli.behavior_review_guidance`
#[test]
fn bundled_skills_explain_distributed_requirements() {
    assert_phrases(
        "habeas",
        &[
            "read [references/distributed-requirements.md](references/distributed-requirements.md) before drafting it",
        ],
    );
    // What they are, the two query kinds, and each query's keys.
    assert_text_in_order(
        "distributed requirements",
        &collapse(distributed_reference()),
        &[
            "# Distributed requirements",
            "A distributed requirement instead declares queries that find its sites, and Habeas reviews each site separately",
            "## Queries",
            "`:::{sites} <name>` selects the places the obligation applies",
            "`:::{violations} <name>` selects constructs likely to break it",
            "Each needs `within: <site query>`",
            "Each body has these keys: `language`",
            "`within`, required for violation queries; and `match` and `no_match` snippet lists",
            "Each site-query match makes a site",
        ],
    );
    assert_text(
        "distributed query contract",
        &collapse(distributed_reference()),
        &[
            "one fenced `yaml` block and a name unique within the requirement",
            "`css`, `markdown`, `toml`, or `yaml`",
            "optional `paths` and `exclude`, lists of project-relative files or directories",
            "an ast-grep `rule`, with optional `utils` and `constraints`",
            "A violation match is reported to the site that contains it",
            "Don't reference the distributed requirement from a site just because it is a site",
            "A site review reads its site, which covers it for Rust item coverage",
            "`habeas author --all` authors a site review for each site",
            "`habeas coverage` runs every snippet",
            "Query directives belong inside a requirement",
            "a single identifier",
            "invalid YAML",
            "unknown query keys",
            "a nonexistent site query",
            "unique across both query kinds",
            "`within` belongs only to violation queries",
            "unsupported languages",
            "rules that cannot compile",
            "empty `match` or `no_match` lists",
            "snippets that fail to parse or have the wrong match result",
            "violation queries without a site query",
            "only matches inside that query's sites count",
            "`match` snippets for what it selects",
            "`no_match` snippets for nearby code it must not select",
            "a site query that matches nothing in the project",
        ],
    );
}

// {test}`requirements.cli.distributed_guidance.when`
#[test]
fn bundled_skills_make_a_requirement_distributed_only_for_per_site_obligations() {
    assert_text(
        "Do list",
        &section(distributed_reference(), "## Do"),
        &[
            "Make a requirement distributed only when its obligation applies on its own at each site; when particular code carries out the requirement as a whole, use `{impl}` there.",
            "Separate obligations each site upholds from obligations particular code carries out, and mark that code `{impl}`.",
        ],
    );
}

// {test}`requirements.cli.distributed_guidance.queries`
#[test]
fn bundled_skills_direct_exact_site_queries_violation_queries_and_snippets() {
    assert_text(
        "Do list",
        &section(distributed_reference(), "## Do"),
        &[
            "check that the site queries select every place the obligation applies and no others",
            "Write a violation query, with `within` naming the site query, for every kind of construct that could break the obligation",
            "Give every query `match` and `no_match` snippets that test its conditions",
            "This applies to violation queries too.",
        ],
    );
}

// {test}`requirements.cli.distributed_guidance.site_text`
#[test]
fn bundled_skills_describe_sites_as_queries_select_them_with_checkable_obligations() {
    assert_text(
        "Do list",
        &section(distributed_reference(), "## Do"),
        &[
            "Describe sites exactly as the site queries select them. When a query relies on a convention, such as how a variable is named, state the convention as an obligation",
        ],
    );
    assert_text(
        "Don't list",
        &section(distributed_reference(), "## Don't"),
        &[
            "Don't describe sites more broadly than the queries select",
            "Don't state a per-site obligation that a reviewer cannot check from the site and the helpers it calls",
        ],
    );
}

// {test}`requirements.cli.distributed_guidance.reviews`
// {test}`requirements.references.procedure`
#[test]
fn bundled_skills_explain_site_references_and_site_reviews() {
    assert_text(
        "Do list",
        &section(distributed_reference(), "## Do"),
        &["Let `habeas author --all` author the site reviews."],
    );
    assert_text(
        "Don't list",
        &section(distributed_reference(), "## Don't"),
        &[
            "Don't reference the distributed requirement from a site just because it is a site; sites need none.",
            "Don't re-run a false site review for a different result; fix the site, or tighten the queries or the text",
        ],
    );
    assert_text(
        "distributed review",
        &section(distributed_reference(), "## Review"),
        &["`habeas author --all` authors a site review for each site"],
    );
    assert_text_in_order(
        "site reviews",
        &section(
            skill_file("habeas-evidence", "references/reviews.md"),
            "## Site reviews",
        ),
        &[
            "authored by `habeas author --all`",
            "1. If it does apply, fix the code at the site.",
            "2. If the site should not be covered, narrow the site query or tighten the text describing sites; if the obligation's text admits a reading under which the site fails, tighten that text",
            "take them through habeas-requirements steps 3 to 5",
        ],
    );
}

// {test}`requirements.cli.distributed_guidance.lists`
#[test]
fn bundled_skills_give_distributed_guidance_as_do_and_dont_lists() {
    let reference = distributed_reference();
    for (heading, prefix) in [("## Do", "- "), ("## Don't", "- Don't ")] {
        let (_, rest) = reference.split_once(&format!("\n{heading}\n")).unwrap();
        let list = rest.split("\n## ").next().unwrap();
        let items = list
            .lines()
            .filter(|line| line.starts_with("- "))
            .collect::<Vec<_>>();
        assert!(items.len() >= 2, "{heading}");
        for item in &items {
            assert!(item.starts_with(prefix), "{heading}: {item}");
            if heading == "## Do" {
                assert!(!item.starts_with("- Don't "), "{heading}: {item}");
            }
        }
    }
    assert_text(
        "Do list",
        &section(reference, "## Do"),
        &[
            "- Make a requirement distributed only when",
            "- Separate obligations each site upholds",
            "- Write a violation query",
            "- Give every query `match` and `no_match` snippets",
            "check that the site queries select every place the obligation applies and no others",
            "- Describe sites exactly as the site queries select them",
            "- Let `habeas author --all` author the site reviews.",
        ],
    );
    assert_text(
        "Don't list",
        &section(reference, "## Don't"),
        &[
            "- Don't describe sites more broadly",
            "- Don't state a per-site obligation",
            "- Don't reference the distributed requirement from a site",
            "- Don't re-run a false site review for a different result",
        ],
    );
}

// {test}`requirements.cli.distributed_guidance.bootstrap`
#[test]
fn bootstrap_skill_proposes_distributed_requirements_for_conventions() {
    assert_text(
        "proposing requirements",
        &section(
            skill_md("habeas-bootstrap-requirements"),
            "## Proposing requirements",
        ),
        &["propose a distributed requirement instead of marking each site `{impl}`"],
    );
}

// {test}`requirements.cli.refactor_skill`
#[test]
fn the_refactor_skill_is_used_for_moves_and_requirement_renames() {
    let description = description("habeas-refactor");
    for phrase in [
        "Use when a change moves code, tests, or requirements in a Habeas project to another file, chapter, or outline node",
        "splitting or merging modules or chapters",
        "moving or renaming a requirement",
        "so that evidence whose content is unchanged is not invalidated",
    ] {
        assert!(description.contains(phrase), "{phrase}");
    }
}

// {test}`requirements.cli.refactor_skill.workflow`
#[test]
fn the_refactor_skill_moves_first_then_updates_evidence_with_habeas_refactor() {
    assert_in_order(
        "habeas-refactor",
        &[
            "Make the move with your own edits",
            "Change nothing else in this step",
            "`habeas refactor --dry-run` with one `--map <old>=<new>` for every moved file or directory, node, and definition name",
            "Review the report",
            "lists the records it updates, with each code span in their rationales that still mentions an old path",
            "lists records a mapping affects that fail a check",
            "Run the same command without `--dry-run`",
            "`habeas status`. Only the records the report left unchanged",
            "Commit the move, the rewritten references, and the `evidence/` changes together, before any change to behavior or text",
            "Next, reassess what remains stale with `habeas author --all`, following the habeas-evidence skill",
        ],
    );
}

// {test}`requirements.cli.refactor_skill.limits`
#[test]
fn the_refactor_skill_never_hand_edits_evidence_and_a_rename_is_not_a_move() {
    assert_text(
        "refactor gotchas",
        &section(skill_md("habeas-refactor"), "## Gotchas"),
        &[
            "Never edit evidence files by hand to follow a move",
            "A rename is not a move. Renaming a function, type, or other item changes its code",
            "its evidence needs reassessment",
            "Editing moved code or text in the same step leaves every record that read it stale",
        ],
    );
}

// {test}`requirements.authoring.provider`
// {test}`requirements.authoring.rig_configuration_mapping`
// {test}`requirements.authoring.usage_requests`
// {test}`requirements.authoring.configuration_storage`
// {test}`requirements.authoring.usage_storage`
// {test}`requirements.authoring.configuration_precedence`
// {test}`requirements.authoring.cache_checkpoints`
#[test]
// {test}`requirements.authoring.usage_storage`
fn openai_responses_replays_tool_calls_and_records_usage() {
    fn assert_no_checkpoints(value: &Value) {
        match value {
            Value::Object(fields) => {
                for (key, value) in fields {
                    assert!(
                        !matches!(key.as_str(), "cachePoint" | "cache_control"),
                        "explicit cache checkpoint: {key}"
                    );
                    if key == "type" {
                        assert!(
                            !matches!(value.as_str(), Some("cache_point" | "cache_checkpoint")),
                            "explicit cache checkpoint block: {value}"
                        );
                    }
                    assert_no_checkpoints(value);
                }
            }
            Value::Array(values) => {
                for value in values {
                    assert_no_checkpoints(value);
                }
            }
            _ => {}
        }
    }

    let response = |id: &str, name: &str, arguments: Value| {
        json!({
            "id": format!("resp_{id}"), "object":"response", "status":"completed",
            "model":"fixture-openai", "output":[{
                "type":"function_call", "id":format!("fc_{id}"), "call_id":id,
                "name":name, "arguments":arguments.to_string(), "status":"completed"
            }],
            "usage":{"input_tokens":11,"output_tokens":7,"total_tokens":18,
                "input_tokens_details":{"cached_tokens":3},
                "output_tokens_details":{"reasoning_tokens":2}}
        })
    };
    let server = Server::start(vec![
        response(
            "read",
            "habeas",
            json!({"file":"extra.txt","line_start":10}),
        ),
        response(
            "submit",
            "submit_judgment",
            json!({"determination":false,"rationale":"Missing implementation."}),
        ),
    ]);
    let fixture = Fixture::new(&server.endpoint);
    fs::write(fixture.config.join("habeas/config.toml"), format!(
        "version = 1\n[authoring]\ndefault = 'mock'\n[authoring.configs.mock]\nbackend = 'openai'\nmodel = 'fixture-openai'\npolicy = 'once'\nendpoint = '{}'\napi_key_env = 'HABEAS_TEST_MODEL_KEY'\n[authoring.configs.mock.options]\nmax_output_tokens = 1024\nreasoning = {{ effort = 'low' }}\n", server.endpoint)).unwrap();
    let output = fixture.run(&["author-evidence", SUBJECT]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let requests = server.finish();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert!(request.headers.starts_with("POST /responses HTTP/1.1\r\n"));
        assert!(request.headers.contains(&format!("Bearer {AUTH}")));
        assert_eq!(request.body["model"], "fixture-openai");
        assert_eq!(request.body["max_output_tokens"], 1024);
        assert_eq!(request.body["reasoning"]["effort"], "low");
        assert_eq!(request.body["tools"].as_array().unwrap().len(), 10);
        assert_no_checkpoints(&request.body);
    }
    assert!(
        !requests[0].body["instructions"]
            .as_str()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        requests[0].body["instructions"],
        requests[1].body["instructions"]
    );
    assert_eq!(requests[0].body["tools"], requests[1].body["tools"]);
    let initial_input = requests[0].body["input"].as_array().unwrap();
    let history = requests[1].body["input"].as_array().unwrap();
    assert_eq!(history.len(), initial_input.len() + 2);
    assert_eq!(&history[..initial_input.len()], initial_input.as_slice());
    assert_eq!(history[initial_input.len()]["type"], "function_call");
    assert_eq!(history[initial_input.len()]["call_id"], "read");
    assert_eq!(
        history[initial_input.len() + 1]["type"],
        "function_call_output"
    );
    assert_eq!(history[initial_input.len() + 1]["call_id"], "read");
    let usage = fixture.usage();
    assert_eq!(usage["backend"], "openai");
    assert_eq!(usage["outcome"], "completed");
    assert_eq!(usage["request_count"], 2);
    assert_eq!(usage["tokens"]["input"], 22);
    assert_eq!(usage["tokens"]["output"], 14);
    assert_eq!(usage["tokens"]["cached_input"], 6);
    assert_eq!(usage["tokens"]["reasoning"], 4);
    assert!(!usage.to_string().contains(AUTH));
}

// {test}`requirements.authoring.openai_defaults`
// {test}`requirements.authoring.credentials`
// {test}`requirements.authoring.adaptive`
#[test]
fn openai_key_only_profile_defaults_to_sol_and_accepts_overrides() {
    mock_server_waits_for_remaining_request_headers();
    for (extra, model, effort, max_tokens) in [
        ("", "gpt-6.1-sol", "low", 16_384),
        ("model = 'gpt-6-luna'\n", "gpt-6-luna", "low", 16_384),
        (
            "model = 'gpt-6-luna'\n[authoring.configs.api.options]\nmax_output_tokens = 8192\n[authoring.configs.api.options.reasoning]\neffort = 'medium'\n",
            "gpt-6-luna",
            "medium",
            8192,
        ),
    ] {
        let submit = json!({"determination":false,"rationale":"Fixture assessment."});
        let server = Server::start(vec![json!({
            "id":"resp_submit", "object":"response", "status":"completed",
            "model":model, "output":[{
                "type":"function_call", "id":"fc_submit", "call_id":"submit",
                "name":"submit_judgment", "arguments":submit.to_string(), "status":"completed"
            }],
            "usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}
        })]);
        let fixture = Fixture::new(&server.endpoint);
        // Only the endpoint is fixture-specific; policy/concurrency/model are optional.
        fs::write(fixture.config.join("habeas/config.toml"), format!(
            "version = 1\n[authoring]\ndefault = 'api'\n[authoring.configs.api]\nbackend = 'openai'\napi_key = '{AUTH}'\nendpoint = '{}'\n{extra}", server.endpoint)).unwrap();
        let output = fixture.run(&["author", SUBJECT]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let requests = server.finish();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].body["model"], model);
        assert_eq!(requests[0].body["reasoning"]["effort"], effort);
        assert_eq!(requests[0].body["max_output_tokens"], max_tokens);
        assert!(requests[0].headers.contains(&format!("Bearer {AUTH}")));
        let usage = fixture.usage();
        assert_eq!(usage["model"], model);
        assert_eq!(usage["policy"], "once");
        assert!(!usage.to_string().contains(AUTH));
    }
}

// {cite}`habeas:util`
fn mock_server_waits_for_remaining_request_headers() {
    let server = Server::start(vec![json!({"ok":true})]);
    let mut client =
        std::net::TcpStream::connect(server.endpoint.trim_start_matches("http://")).unwrap();
    client
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    client.write_all(b"POST /api/chat HTTP/1.1\r\n").unwrap();
    // No response or EOF may arrive while the request is still incomplete.
    let error = client.read(&mut [0]).unwrap_err();
    assert!(matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    ));
    client.write_all(b"Content-Length: 2\r\n\r\n{}").unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200 "), "{response}");
    let requests = server.finish();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].body, json!({}));
}

// {test}`requirements.authoring.adaptive`
// {test}`requirements.authoring.openai_defaults`
// {test}`requirements.authoring.policy`
#[test]
// {test}`requirements.authoring.usage_storage`
// {test}`requirements.authoring.confirmation`
fn adaptive_routes_fresh_assessments_and_records_each_model() {
    let response = |arguments: Value| {
        json!({
            "id":"resp_submit", "object":"response", "status":"completed",
            "model":"mock", "output":[{
                "type":"function_call", "id":"fc_submit", "call_id":"submit",
                "name":"submit_judgment", "arguments":arguments.to_string(), "status":"completed"
            }], "usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}
        })
    };
    for (existing_fixme, first, policy, expected_models, reason) in [
        (
            false,
            json!({"determination":true,"rationale":"Good."}),
            "",
            vec!["gpt-5.4-mini"],
            None,
        ),
        (
            false,
            json!({"determination":false,"rationale":"MINI_ONLY_RATIONALE_SENTINEL"}),
            "",
            vec!["gpt-5.4-mini", "gpt-6.1-sol"],
            Some("mini_false"),
        ),
        (
            false,
            json!({"determination":{"FIXME":["Gap"]},"rationale":"MINI_ONLY_RATIONALE_SENTINEL"}),
            "",
            vec!["gpt-5.4-mini", "gpt-6.1-sol"],
            Some("mini_fixme"),
        ),
        (
            false,
            json!({"bad":"submission"}),
            "",
            vec!["gpt-5.4-mini", "gpt-6.1-sol"],
            Some("mini_assessment_failed"),
        ),
        (
            true,
            json!({"determination":true,"rationale":"Good."}),
            "",
            vec!["gpt-6.1-sol"],
            Some("existing_fixme"),
        ),
        (
            false,
            json!({"determination":true,"rationale":"FIRST_AGREEMENT_RATIONALE"}),
            "policy = 'confirm'\n",
            vec!["gpt-5.4-mini", "gpt-5.4-mini"],
            None,
        ),
    ] {
        let mut replies = vec![response(first)];
        if expected_models.len() == 2 {
            replies.push(response(json!({"determination":true,"rationale":"Good."})));
        }
        let server = Server::start(replies);
        let fixture = Fixture::new(&server.endpoint);
        if !existing_fixme {
            fs::write(fixture.project.join("src/lib.rs"), "// {impl}`requirements.ready`\npub fn ready() {}\n// {test}`requirements.ready`\nfn ready_test() {}\n// A plain FIXME is not an acknowledgment.\n").unwrap();
        }
        fs::write(fixture.config.join("habeas/config.toml"), format!(
            "version = 1\n[authoring]\ndefault = 'api'\n[authoring.configs.api]\nbackend = 'openai'\nmodel = 'adaptive'\n{policy}endpoint = '{}'\napi_key_env = 'HABEAS_TEST_MODEL_KEY'\n[authoring.configs.api.options]\nmax_output_tokens = 8192\nreasoning = {{ effort = 'medium' }}\n", server.endpoint)).unwrap();
        let output = fixture.run(&["author", SUBJECT]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let requests = server.finish();
        assert_eq!(
            requests
                .iter()
                .map(|r| r.body["model"].as_str().unwrap())
                .collect::<Vec<_>>(),
            expected_models
        );
        for request in &requests {
            assert!(
                request
                    .headers
                    .to_lowercase()
                    .contains(&format!("authorization: bearer {}", AUTH.to_lowercase()))
            );
            assert!(
                !request
                    .body
                    .to_string()
                    .contains("MINI_ONLY_RATIONALE_SENTINEL")
            );
            assert_eq!(
                request.body["input"].as_array().unwrap().len(),
                1,
                "fresh conversation"
            );
            assert_eq!(request.body["reasoning"]["effort"], "medium");
            assert_eq!(request.body["max_output_tokens"], 8192);
        }
        let evidence: Value =
            serde_json::from_slice(&fs::read(&fixture.evidence).unwrap()).unwrap();
        assert_eq!(evidence["determination"], true);
        if !policy.is_empty() {
            assert_eq!(evidence["rationale"], "FIRST_AGREEMENT_RATIONALE");
        }
        let logs: Vec<Value> = fs::read_dir(fixture.state.join("habeas/authoring/usage"))
            .unwrap()
            .map(|entry| serde_json::from_slice(&fs::read(entry.unwrap().path()).unwrap()).unwrap())
            .collect();
        let expected_policy = if policy.is_empty() { "once" } else { "confirm" };
        assert!(
            logs.iter().all(|log| log["policy"] == expected_policy),
            "{logs:?}"
        );
        let parent = logs
            .iter()
            .find(|log| log["subject"] == SUBJECT && log["model"] == "adaptive")
            .unwrap();
        assert_eq!(parent["routing_reason"].as_str(), reason);
        assert!(!parent["attempts"].as_array().unwrap().is_empty());
        let actual: Vec<_> = logs
            .iter()
            .filter(|log| log["model"] != "adaptive")
            .collect();
        assert_eq!(actual.len(), expected_models.len());
        let ids: std::collections::BTreeSet<_> = actual
            .iter()
            .map(|log| log["run_id"].as_str().unwrap())
            .collect();
        assert_eq!(ids.len(), actual.len());
        let links: Vec<_> = logs
            .iter()
            .flat_map(|log| log["attempts"].as_array().into_iter().flatten())
            .filter_map(Value::as_str)
            .collect();
        for id in ids {
            assert!(
                links
                    .iter()
                    .any(|path| path.ends_with(&format!("{id}.json")))
            );
        }
        for model in expected_models {
            assert!(
                logs.iter()
                    .any(|log| log["model"] == model && log["request_count"] == 1)
            );
        }
    }
}

// {test}`requirements.authoring.adaptive`
// {test}`requirements.authoring.persistence`
// {test}`requirements.authoring.policy`
#[test]
// {test}`requirements.authoring.confirmation`
fn adaptive_sol_failure_is_final_and_confirm_uses_sol_verification() {
    let response = |name: &str, arguments: Value| {
        json!({
            "id":"resp", "object":"response", "status":"completed", "model":"mock",
            "output":[{"type":"function_call", "id":"fc", "call_id":"call", "name":name,
                "arguments":arguments.to_string(), "status":"completed"}],
            "usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}
        })
    };
    let assert_policy = |fixture: &Fixture, expected: &str| {
        for path in fs::read_dir(fixture.state.join("habeas/authoring/usage")).unwrap() {
            let log: Value =
                serde_json::from_slice(&fs::read(path.unwrap().path()).unwrap()).unwrap();
            assert_eq!(log["policy"], expected);
        }
    };
    let good = response(
        "submit_judgment",
        json!({"determination":true,"rationale":"Good."}),
    );
    let bad = response(
        "submit_judgment",
        json!({"determination":false,"rationale":"Gap."}),
    );
    for confirm in [false, true] {
        let replies = if confirm {
            vec![
                good.clone(),
                bad.clone(),
                bad.clone(),
                response(
                    "submit_verification",
                    json!({
                "real":false,"acknowledged":false,"acknowledgments":[],"rationale":"No real gap."}),
                ),
            ]
        } else {
            vec![
                bad.clone(),
                response("submit_judgment", json!({"invalid":true})),
            ]
        };
        let server = Server::start(replies);
        let fixture = Fixture::new(&server.endpoint);
        fs::write(fixture.project.join("src/lib.rs"), "// {impl}`requirements.ready`\npub fn ready() {}\n// {test}`requirements.ready`\nfn ready_test() {}\n").unwrap();
        fs::write(fixture.config.join("habeas/config.toml"), format!(
            "version = 1\n[authoring]\ndefault = 'api'\n[authoring.configs.api]\nbackend = 'openai'\nmodel = 'adaptive'\nendpoint = '{}'\napi_key_env = 'HABEAS_TEST_MODEL_KEY'\n", server.endpoint)).unwrap();
        let output = if confirm {
            fixture.run(&["author", SUBJECT, "--policy", "confirm"])
        } else {
            fixture.run(&["author", SUBJECT])
        };
        let requests = server.finish();
        assert_policy(&fixture, if confirm { "confirm" } else { "once" });
        let models: Vec<_> = requests
            .iter()
            .map(|r| r.body["model"].as_str().unwrap())
            .collect();
        if confirm {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                models,
                ["gpt-5.4-mini", "gpt-5.4-mini", "gpt-6.1-sol", "gpt-6.1-sol"]
            );
            let evidence: Value =
                serde_json::from_slice(&fs::read(&fixture.evidence).unwrap()).unwrap();
            assert_eq!(evidence["determination"], true);
            assert!(
                evidence["rationale"]
                    .as_str()
                    .unwrap()
                    .contains("assessments disagreed")
            );
        } else {
            assert!(!output.status.success());
            assert_eq!(models, ["gpt-5.4-mini", "gpt-6.1-sol"]);
            assert_eq!(
                fs::read_to_string(&fixture.evidence).unwrap(),
                fixture.original
            );
        }
    }
    for policy in ["once", "confirm"] {
        let server = Server::start(vec![
            response(
                "submit_judgment",
                json!({"determination":false,"rationale":"Mini found a gap."}),
            ),
            response(
                "submit_judgment",
                json!({"determination":false,"rationale":"Sol confirms the gap."}),
            ),
        ]);
        let fixture = Fixture::new(&server.endpoint);
        configure_adaptive_fixture(&fixture, &server.endpoint);
        let output = fixture.run(&["author", SUBJECT, "--policy", policy]);
        assert!(output.status.success(), "{output:?}");
        assert_policy(&fixture, policy);
        let requests = server.finish();
        assert_eq!(
            requests
                .iter()
                .map(|request| request.body["model"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["gpt-5.4-mini", "gpt-6.1-sol"]
        );
        let evidence: Value =
            serde_json::from_slice(&fs::read(&fixture.evidence).unwrap()).unwrap();
        assert_eq!(evidence["determination"], false);
        assert_eq!(evidence["rationale"], "Sol confirms the gap.");
    }
}

// {cite}`habeas:util`
fn configure_adaptive_fixture(fixture: &Fixture, endpoint: &str) {
    fs::write(fixture.project.join("src/lib.rs"), "// {impl}`requirements.ready`\npub fn ready() {}\n// {test}`requirements.ready`\nfn ready_test() {}\n").unwrap();
    fs::write(fixture.config.join("habeas/config.toml"), format!(
        "version = 1\n[authoring]\ndefault = 'api'\n[authoring.configs.api]\nbackend = 'openai'\nmodel = 'adaptive'\nendpoint = '{endpoint}'\napi_key_env = 'HABEAS_TEST_MODEL_KEY'\n")).unwrap();
}

// {test}`requirements.authoring.adaptive`
#[test]
fn adaptive_preflight_and_provider_errors_never_attempt_sol() {
    for failure in [
        "credential",
        "mandatory",
        "storage",
        "provider",
        "connection",
    ] {
        let server = (failure == "provider").then(|| Server::start(vec![json!({
            "__http_status":401,"error":{"message":"fixture rejected credential","type":"invalid_request_error","code":"invalid_api_key"}
        })]));
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = server
            .as_ref()
            .map(|s| s.endpoint.clone())
            .unwrap_or_else(|| format!("http://{}", listener.local_addr().unwrap()));
        let listener = if failure == "connection" {
            drop(listener);
            None
        } else {
            Some(listener)
        };
        let fixture = Fixture::new(&endpoint);
        configure_adaptive_fixture(&fixture, &endpoint);
        if failure == "mandatory" {
            fs::write(
                fixture.project.join("habeas.toml"),
                "version = 1\n[evidence]\ndisallow = ['md/requirements.md']\n",
            )
            .unwrap();
        }
        if failure == "storage" {
            fs::write(&fixture.state, "not a directory").unwrap();
        }
        let mut command = fixture.command();
        if failure == "credential" {
            command.env_remove("HABEAS_TEST_MODEL_KEY");
        }
        let output = command.args(["author", SUBJECT]).output().unwrap();
        assert!(!output.status.success(), "{failure}");
        assert_eq!(
            fs::read_to_string(&fixture.evidence).unwrap(),
            fixture.original
        );
        if let Some(server) = server {
            let requests = server.finish();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].body["model"], "gpt-5.4-mini");
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("fixture rejected credential")
            );
        } else if let Some(listener) = listener {
            assert!(matches!(listener.accept(),Err(e) if e.kind()==std::io::ErrorKind::WouldBlock));
        }
        if let Ok(paths) = fs::read_dir(fixture.state.join("habeas/authoring/usage")) {
            for path in paths {
                let log: Value =
                    serde_json::from_slice(&fs::read(path.unwrap().path()).unwrap()).unwrap();
                assert_ne!(log["model"], "gpt-6.1-sol", "{failure}: {log}");
            }
        }
    }
}

// {test}`requirements.authoring.adaptive`
#[cfg(unix)]
#[test]
fn adaptive_cancellation_does_not_escalate_or_replace_evidence() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let fixture = Fixture::new(&endpoint);
    configure_adaptive_fixture(&fixture, &endpoint);
    let child = fixture
        .command()
        .args(["author", SUBJECT])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let _connection = loop {
        if let Ok((stream, _)) = listener.accept() {
            break stream;
        }
        assert!(Instant::now() < deadline, "no initial mini request");
        thread::sleep(Duration::from_millis(5));
    };
    assert!(
        Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cancel"));
    assert_eq!(
        fs::read_to_string(&fixture.evidence).unwrap(),
        fixture.original
    );
    assert!(matches!(listener.accept(),Err(e) if e.kind()==std::io::ErrorKind::WouldBlock));
    for entry in fs::read_dir(fixture.state.join("habeas/authoring/usage")).unwrap() {
        let log: Value = serde_json::from_slice(&fs::read(entry.unwrap().path()).unwrap()).unwrap();
        assert_ne!(log["model"], "gpt-6.1-sol");
    }
}

// {test}`requirements.authoring.adaptive`
#[test]
fn adaptive_unsubmitted_conversation_escalates_once_with_fresh_input() {
    let empty = json!({"id":"resp","object":"response","status":"completed","model":"mock","output":[],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}});
    let good = json!({"id":"resp","object":"response","status":"completed","model":"mock","output":[{"type":"function_call","id":"fc","call_id":"call","name":"submit_judgment","arguments":json!({"determination":true,"rationale":"Good."}).to_string(),"status":"completed"}],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}});
    let server = Server::start(vec![empty.clone(), empty, good]);
    let fixture = Fixture::new(&server.endpoint);
    configure_adaptive_fixture(&fixture, &server.endpoint);
    let output = fixture.run(&["author", SUBJECT]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let requests = server.finish();
    assert_eq!(
        requests
            .iter()
            .map(|r| r.body["model"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["gpt-5.4-mini", "gpt-5.4-mini", "gpt-6.1-sol"]
    );
    assert_eq!(requests[2].body["input"].as_array().unwrap().len(), 1);
    assert!(!requests[2].body.to_string().contains("Reminder:"));
}

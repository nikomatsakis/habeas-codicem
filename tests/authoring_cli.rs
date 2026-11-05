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
        ["habeas", "list_files", "ripgrep", "submit_judgment"]
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
    assert_eq!(
        extra.len(),
        2,
        "repeated delivery is deduplicated in evidence"
    );
    assert!(
        extra
            .iter()
            .any(|input| input["line_start"] == 1 && input["line_end"] == 11)
    );
    assert!(
        extra
            .iter()
            .any(|input| input["line_start"] == 10 && input["line_end"] == 12)
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

// {test}`requirements.authoring.negative_assessment`
// {test}`requirements.authoring.persistence`
// {test}`requirements.ci.fixme_with_unacknowledged_gap`
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
    assert!(evidence["inputs"].to_string().contains("FIXME(#125)"));
    assert_eq!(fixture.usage()["outcome"], "completed");
    assert_eq!(fixture.usage()["determination"], "false");
}

// {test}`requirements.authoring.invocation`
// {test}`requirements.authoring.persistence`
// {test}`requirements.authoring.delivered_inputs`
// {test}`requirements.authoring.usage_storage`
#[test]
fn stdout_emits_only_evidence_json_without_changing_project_files() {
    for (determination, kind, existing_evidence) in [
        (json!(true), "true", true),
        (json!(false), "false", true),
        (json!({"FIXME":["#125"]}), "FIXME", true),
        (json!(true), "true", false),
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
        let output = fixture.run(&["author-evidence", SUBJECT, "--stdout"]);
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
                .any(|input| input["rust_file"] == "src/lib.rs")
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
        assert!(stderr.contains(&format!("{SUBJECT}: ollama / {MODEL} — {kind}")));
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

// {test}`requirements.authoring.provider`
// {test}`requirements.authoring.rig_configuration_mapping`
// {test}`requirements.authoring.configuration_precedence`
// {test}`requirements.authoring.initial_prompt`
// {test}`requirements.authoring.usage_requests`
// {test}`requirements.authoring.configuration_storage`
// {test}`requirements.authoring.source_tools`
// {test}`requirements.authoring.submission`
#[test]
fn bedrock_cli_uses_selected_profile_region_and_converse_tools() {
    let server = Server::start(vec![json!({
        "output":{"message":{"role":"assistant","content":[{"toolUse":{
            "toolUseId":"bedrock-submit","name":"submit_judgment",
            "input":{"determination":true,"rationale":"The supplied sources support the subject."}
        }}]}},
        "stopReason":"tool_use",
        "usage":{"inputTokens":19,"outputTokens":5,"totalTokens":24},
        "metrics":{"latencyMs":1}
    })]);
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
    assert_eq!(requests.len(), 1);
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
    for field in [
        "file",
        "rust_file",
        "rust_item",
        "requirements",
        "definition",
    ] {
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
        ["habeas", "list_files", "ripgrep", "submit_judgment"]
    );
    let evidence: Value = serde_json::from_slice(&fs::read(&fixture.evidence).unwrap()).unwrap();
    assert_eq!(evidence["determination"], true);
    let usage = fixture.usage();
    assert_eq!(usage["backend"], "bedrock");
    assert_eq!(usage["model"], "fixture-bedrock-model");
    assert_eq!(usage["tokens"]["input"], 19);
    assert_eq!(usage["tokens"]["output"], 5);
    assert_eq!(usage["outcome"], "completed");
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

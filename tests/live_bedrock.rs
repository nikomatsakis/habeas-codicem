use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

use aws_sdk_bedrockruntime::config::ProvideCredentials;
use habeas::config::{self, BackendConfig, BedrockConfig, ResolvedAuthoringConfig};
use serde_json::Value;

const SUBJECT: &str = "requirements.parity";
const REQUIREMENT: &str = "The is_even function in src/parity.rs MUST return true exactly when its unsigned integer argument is even. Its implementation and tests are in that file.";

// {test}`requirements.integration.live_bedrock_test`
fn report(message: impl std::fmt::Display) {
    // Direct stderr writes remain visible when libtest captures passing tests.
    let _ = writeln!(io::stderr().lock(), "{message}");
}

// {test}`requirements.integration.live_bedrock_test`
// {test}`requirements.authoring.configuration_precedence`
fn selected_config() -> Result<Option<ResolvedAuthoringConfig>, Box<dyn Error>> {
    let root = fs::canonicalize(env!("CARGO_MANIFEST_DIR"))?;
    let path = config::user_config_path()?;
    let user = config::load_user_config(&path)?;
    let explicit = match std::env::var("HABEAS_LIVE_CONFIG") {
        Ok(name) => Some(name),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => return Err(error.into()),
    };
    let Some(authoring) = user.as_ref().and_then(|user| user.authoring.as_ref()) else {
        return if explicit.is_some() {
            config::resolve_authoring_from(&path, user.as_ref(), &root, explicit.as_deref())
                .map(Some)
                .map_err(Into::into)
        } else {
            Ok(None)
        };
    };
    let preferred = explicit
        .as_deref()
        .or_else(|| {
            authoring
                .projects
                .get(&root)
                .map(|entry| entry.config.as_str())
        })
        .or(authoring.default.as_deref());
    if let Some(name) = preferred {
        let selected = config::resolve_authoring_from(&path, user.as_ref(), &root, Some(name))?;
        if matches!(selected.backend, BackendConfig::Bedrock(_)) {
            return Ok(Some(selected));
        }
        if explicit.is_some() {
            return Err(
                format!("HABEAS_LIVE_CONFIG={name:?} must select a Bedrock configuration").into(),
            );
        }
    }
    let name = authoring.configs.iter().find_map(|(name, config)| {
        matches!(config.backend, BackendConfig::Bedrock(_)).then_some(name)
    });
    name.map(|name| {
        config::resolve_authoring_from(&path, user.as_ref(), &root, Some(name)).map_err(Into::into)
    })
    .transpose()
}

// {test}`requirements.integration.live_bedrock_test`
// {test}`requirements.authoring.configuration_precedence`
// {test}`requirements.authoring.credentials`
fn credentials_available(config: &BedrockConfig) -> bool {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest());
        if let Some(profile) = &config.aws_profile {
            loader = loader.profile_name(profile);
        }
        if let Some(region) = &config.region {
            loader = loader.region(aws_config::Region::new(region.clone()));
        }
        if let Some(credentials) = &config.credentials {
            loader = loader.credentials_provider(aws_sdk_bedrockruntime::config::Credentials::new(
                credentials.access_key_id.expose_secret(),
                credentials.secret_access_key.expose_secret(),
                credentials.session_token.as_ref().map(|value| value.expose_secret().to_owned()),
                None,
                "Habeas configuration",
            ));
        }
        let sdk = loader.load().await;
        let Some(provider) = sdk.credentials_provider() else {
            report("warning: skipping live Bedrock test: no AWS credential provider");
            return false;
        };
        if let Err(error) = provider.provide_credentials().await {
            report(format!(
                "warning: skipping live Bedrock test: AWS credentials unavailable: {error}"
            ));
            return false;
        }
        assert!(
            sdk.region().is_some(),
            "live Bedrock test has credentials but no AWS region; set a profile region or the authoring configuration's region"
        );
        true
    })
}

// {test}`requirements.integration.live_bedrock_test`
// {test}`requirements.authoring.delivered_inputs`
// {test}`requirements.authoring.provider`
// {test}`requirements.authoring.source_tools`
// {test}`requirements.authoring.submission`
// {test}`requirements.authoring.persistence`
// {test}`requirements.authoring.usage_storage`
#[test]
fn live_bedrock_authoring() {
    if std::env::var_os("CI").is_some() {
        report("warning: skipping live Bedrock test: CI is set");
        return;
    }
    let selected = selected_config()
        .expect("cannot select live Bedrock configuration")
        .expect("no Bedrock authoring configuration; add one to your Habeas user config (see md/reference.md), or set CI=1 to disable live testing");
    let BackendConfig::Bedrock(backend) = &selected.backend else {
        unreachable!("selection only returns Bedrock configurations");
    };
    if !credentials_available(backend) {
        return;
    }
    report(format!(
        "live Bedrock test: configuration {:?}, model {:?}",
        selected.name, backend.model
    ));

    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    for directory in [project.join("md"), project.join("src")] {
        fs::create_dir_all(directory).unwrap();
    }
    fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = 'live-fixture'\nversion = '0.1.0'\nedition = '2024'\n",
    )
    .unwrap();
    fs::write(project.join("book.toml"), "[book]\nsrc = 'md'\n").unwrap();
    fs::write(
        project.join("md/SUMMARY.md"),
        "# Summary\n\n- [Requirements](requirements.md)\n",
    )
    .unwrap();
    fs::write(
        project.join("md/requirements.md"),
        format!(":::{{req}} parity\n{REQUIREMENT}\n:::\n"),
    )
    .unwrap();
    // No citation points to this file, so assessing its implementation requires
    // an additional source read instead of only the mandatory initial inputs.
    fs::write(project.join("src/parity.rs"), "pub fn is_even(value: u32) -> bool {\n    value % 2 == 0\n}\n\n#[test]\nfn parity_cases() {\n    assert!(is_even(0));\n    assert!(!is_even(1));\n    assert!(is_even(2));\n    assert!(!is_even(u32::MAX));\n}\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_habeas"))
        .current_dir(&project)
        .args([
            "author-evidence",
            SUBJECT,
            "--config",
            &selected.name,
            "--stdout",
        ])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    report(&stderr);
    assert!(
        output.status.success(),
        "live Bedrock authoring failed: {stderr}"
    );
    let evidence: Value = serde_json::from_slice(&output.stdout).expect("stdout is evidence JSON");
    assert_eq!(evidence["requirement"], REQUIREMENT);
    habeas::judgment::Judgment::from_value(&evidence["determination"]).unwrap();
    assert!(!evidence["rationale"].as_str().unwrap().trim().is_empty());
    assert!(
        evidence["inputs"].as_array().unwrap().iter().any(|input| {
            input["file"] == "src/parity.rs"
                && input["item"].is_string()
                && input["hash"].is_string()
                && input.get("text").is_none()
        }),
        "model must inspect the fixture's source through a read tool"
    );
    assert!(!project.join("evidence").exists());
    let usage_path = stderr
        .lines()
        .find_map(|line| line.strip_prefix("Usage: "))
        .expect("reported usage path");
    let usage: Value = serde_json::from_slice(&fs::read(usage_path).unwrap()).unwrap();
    assert_eq!(usage["backend"], "bedrock");
    assert_eq!(usage["model"], backend.model);
    assert_eq!(usage["outcome"], "completed");
    assert_eq!(usage["evidence_output"], "stdout");
    assert_eq!(usage["evidence_written"], true);
    assert!(usage["request_count"].as_u64().unwrap() >= 2);
    assert_eq!(usage["tool_calls"]["submit_judgment"], 1);
    assert!(
        usage["tool_calls"]["habeas"].as_u64().unwrap_or(0)
            + usage["tool_calls"]["ripgrep"].as_u64().unwrap_or(0)
            > 0
    );
}

// {test}`requirements.integration.live_bedrock_test`
fn skip_probe(root: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", "live_bedrock_authoring"]);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("AWS_") {
            command.env_remove(key);
        }
    }
    command
        .env_remove("CI")
        .env_remove("HABEAS_LIVE_CONFIG")
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("APPDATA", root.join("config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("LOCALAPPDATA", root.join("state"))
        .env("AWS_CONFIG_FILE", root.join("aws-config"))
        .env("AWS_SHARED_CREDENTIALS_FILE", root.join("aws-credentials"))
        .env("AWS_EC2_METADATA_DISABLED", "true")
        .env("AWS_ENDPOINT_URL_BEDROCK_RUNTIME", "http://127.0.0.1:9");
    command
}

// {test}`requirements.integration.live_bedrock_test`
#[test]
fn live_test_prerequisites_are_checked_without_inference() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("config/habeas")).unwrap();
    let config_path = root.join("config/habeas/config.toml");
    fs::write(&config_path, "invalid TOML [").unwrap();
    let ci = skip_probe(root).env("CI", "true").output().unwrap();
    assert!(
        ci.status.success(),
        "{}",
        String::from_utf8_lossy(&ci.stderr)
    );
    assert!(String::from_utf8_lossy(&ci.stderr).contains("skipping live Bedrock test: CI is set"));
    let malformed = skip_probe(root).output().unwrap();
    assert!(!malformed.status.success());
    assert!(
        String::from_utf8_lossy(&malformed.stdout)
            .contains("cannot select live Bedrock configuration")
    );

    fs::remove_file(&config_path).unwrap();
    for contents in [
        None,
        Some("version = 1\n"),
        Some(
            "version = 1\n[authoring.configs.local]\nbackend = 'ollama'\nmodel = 'fixture-model'\n",
        ),
    ] {
        if let Some(contents) = contents {
            fs::write(&config_path, contents).unwrap();
        }
        let missing_config = skip_probe(root).output().unwrap();
        assert!(!missing_config.status.success());
        assert!(
            String::from_utf8_lossy(&missing_config.stdout)
                .contains("no Bedrock authoring configuration")
        );
        assert!(
            !String::from_utf8_lossy(&missing_config.stderr).contains("skipping live Bedrock test")
        );
    }
    fs::remove_file(&config_path).unwrap();
    let explicit_missing = skip_probe(root)
        .env("HABEAS_LIVE_CONFIG", "missing")
        .output()
        .unwrap();
    assert!(!explicit_missing.status.success());
    assert!(
        String::from_utf8_lossy(&explicit_missing.stdout)
            .contains("cannot select live Bedrock configuration")
    );

    fs::write(&config_path, "version = 1\n[authoring.configs.cloud]\nbackend = 'bedrock'\nmodel = 'fixture-model'\naws_profile = 'missing-credentials'\nregion = 'us-east-1'\n").unwrap();
    fs::write(
        root.join("aws-config"),
        "[profile missing-credentials]\nregion = us-east-1\n",
    )
    .unwrap();
    fs::write(root.join("aws-credentials"), "").unwrap();
    let missing_credentials = skip_probe(root).output().unwrap();
    assert!(
        missing_credentials.status.success(),
        "{}",
        String::from_utf8_lossy(&missing_credentials.stderr)
    );
    assert!(
        String::from_utf8_lossy(&missing_credentials.stderr)
            .contains("AWS credentials unavailable")
    );
    assert!(!String::from_utf8_lossy(&missing_credentials.stderr).contains("Usage:"));

    let provider_failure = skip_probe(root)
        .env("AWS_ACCESS_KEY_ID", "FAKE_LIVE_TEST_ACCESS_KEY")
        .env("AWS_SECRET_ACCESS_KEY", "fake-live-test-secret")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&provider_failure.stderr);
    assert!(!provider_failure.status.success(), "{stderr}");
    assert!(
        stderr.contains("live Bedrock test: configuration"),
        "{stderr}"
    );
    assert!(
        String::from_utf8_lossy(&provider_failure.stdout).contains("live Bedrock authoring failed")
    );
    assert!(!stderr.contains("skipping live Bedrock test"), "{stderr}");
}

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::Path;

use habeas::config::{
    BackendConfig, BackendKind, ConfigPlatform, ProjectConfig, UserConfig, load_user_config,
    resolve_authoring_from, state_root_from, user_config_path_from,
};

// {test}`requirements.configuration.user_location`
// {test}`requirements.configuration.state_location`
#[test]
fn locations_use_absolute_xdg_values_and_platform_fallbacks() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let config = dir.path().join("config");
    let state = dir.path().join("state");
    let vars = BTreeMap::from([
        ("HOME", home.as_os_str().to_owned()),
        ("XDG_CONFIG_HOME", config.as_os_str().to_owned()),
        ("XDG_STATE_HOME", state.as_os_str().to_owned()),
    ]);
    let lookup = |name: &str| vars.get(name).cloned();
    assert_eq!(
        user_config_path_from(ConfigPlatform::Unix, lookup).unwrap(),
        config.join("habeas/config.toml")
    );
    assert_eq!(
        state_root_from(ConfigPlatform::Unix, lookup).unwrap(),
        state.join("habeas")
    );
    for invalid_xdg in ["", "relative"] {
        let lookup = |name: &str| {
            Some(if name == "HOME" {
                home.as_os_str().to_owned()
            } else {
                OsString::from(invalid_xdg)
            })
        };
        assert_eq!(
            user_config_path_from(ConfigPlatform::Unix, lookup).unwrap(),
            home.join(".config/habeas/config.toml")
        );
        assert_eq!(
            state_root_from(ConfigPlatform::Unix, lookup).unwrap(),
            home.join(".local/state/habeas")
        );
    }
    let windows = |name: &str| match name {
        "APPDATA" => Some(config.as_os_str().to_owned()),
        "LOCALAPPDATA" => Some(state.as_os_str().to_owned()),
        _ => None,
    };
    assert_eq!(
        user_config_path_from(ConfigPlatform::Windows, windows).unwrap(),
        config.join("habeas/config.toml")
    );
    assert_eq!(
        state_root_from(ConfigPlatform::Windows, windows).unwrap(),
        state.join("habeas/state")
    );
    assert!(user_config_path_from(ConfigPlatform::Unix, |_| None).is_err());
    assert!(state_root_from(ConfigPlatform::Windows, |_| None).is_err());
}

// {test}`requirements.configuration.structure`
// {test}`requirements.configuration.loading`
// {test}`requirements.configuration.serialization`
#[test]
fn absent_and_minimal_configuration_are_distinct_and_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("user.toml");
    assert!(load_user_config(&path).unwrap().is_none());
    assert!(!path.exists());
    let original = "# keep this comment\nversion = 1\n";
    fs::write(&path, original).unwrap();
    let config = load_user_config(&path).unwrap().unwrap();
    assert!(config.authoring.is_none());
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    assert_eq!(
        UserConfig::parse(&config.to_toml().unwrap(), &path).unwrap(),
        config
    );
    let book = "[book]\nsrc = 'docs'\n";
    assert!(
        ProjectConfig::from_book_toml(book, &path)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        ProjectConfig::from_book_toml(&format!("{book}\n[habeas]\n"), &path).unwrap(),
        Some(ProjectConfig {})
    );
    assert!(ProjectConfig::from_book_toml("[habeas]\nversion = 1\n", &path).is_err());
}

// {test}`requirements.authoring.configuration_structure`
// {test}`requirements.authoring.configuration_serialization`
// {test}`requirements.configuration.serialization`
#[test]
fn backend_configuration_round_trips_without_materializing_defaults() {
    let source = r#"
version = 1
[authoring]
default = "Cloud"
[authoring.configs.Cloud]
backend = "bedrock"
model = "arn:aws:bedrock:us-west-2:123:inference-profile/exact-id"
aws_profile = "research"
region = "us-west-2"
[authoring.configs.Cloud.options]
temperature = 0.2
max_tokens = 777
enabled = true
labels = ["a", "b"]
[authoring.configs.Cloud.options.custom]
arbitrary = "provider-defined"
[authoring.configs.local]
backend = "ollama"
model = "tiny:latest"
"#;
    let path = Path::new("settings.toml");
    let config = UserConfig::parse(source, path).unwrap();
    let authoring = config.authoring.as_ref().unwrap();
    assert!(authoring.projects.is_empty());
    let local = &authoring.configs["local"].backend;
    let BackendConfig::Ollama(ollama) = local else {
        panic!("wrong backend")
    };
    assert_eq!(ollama.endpoint(), "http://localhost:11434");
    assert_eq!(ollama.endpoint, None);
    assert!(ollama.options.is_empty());
    assert_eq!(local.model_ref().model, "tiny:latest");
    assert_eq!(local.model_ref().backend, BackendKind::Ollama);
    let json = authoring.configs["Cloud"].backend.options_json().unwrap();
    assert_eq!(json["custom"]["arbitrary"], "provider-defined");
    assert_eq!(json["max_tokens"], 777);
    let serialized = config.to_toml().unwrap();
    assert!(!serialized.contains("endpoint"));
    assert!(!serialized.contains("http://localhost"));
    assert_eq!(UserConfig::parse(&serialized, path).unwrap(), config);
}

// {test}`requirements.authoring.configuration_precedence`
// {test}`requirements.authoring.configuration_storage`
// {test}`requirements.authoring.rig_configuration_mapping`
#[test]
fn explicit_project_and_default_selections_choose_whole_configs() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let config_path = root.join("config.toml");
    let mut config = UserConfig::parse(
        r#"
version = 1
[authoring]
default = "default"
[authoring.configs.default]
backend = "bedrock"
model = "cloud-id"
aws_profile = "named"
[authoring.configs.project]
backend = "ollama"
model = "project:tag"
endpoint = "http://server:11434"
[authoring.configs.explicit]
backend = "ollama"
model = "explicit:tag"
"#,
        &config_path,
    )
    .unwrap();
    assert_eq!(
        resolve_authoring_from(&config_path, Some(&config), &root, None)
            .unwrap()
            .name,
        "default"
    );
    config.authoring.as_mut().unwrap().projects.insert(
        root.clone(),
        habeas::config::ProjectSelection {
            config: "project".to_owned(),
        },
    );
    let project =
        resolve_authoring_from(&config_path, Some(&config), &root.join("."), None).unwrap();
    assert_eq!(project.name, "project");
    assert_eq!(project.project_root, root);
    let BackendConfig::Ollama(ollama) = project.backend else {
        panic!("wrong backend")
    };
    assert_eq!(ollama.endpoint(), "http://server:11434");
    let explicit =
        resolve_authoring_from(&config_path, Some(&config), &root, Some("explicit")).unwrap();
    let BackendConfig::Ollama(ollama) = explicit.backend else {
        panic!("wrong backend")
    };
    assert_eq!(ollama.model, "explicit:tag");
    assert_eq!(ollama.endpoint, None);
    let error = resolve_authoring_from(&config_path, Some(&config), &root, Some("DEFAULT"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("DEFAULT"));
    assert!(error.contains("config.toml"));
    assert!(error.contains("editing-the-file"));
    assert!(
        resolve_authoring_from(&config_path, None, &root, None)
            .unwrap_err()
            .to_string()
            .contains("no authoring configuration")
    );
}

// {test}`requirements.configuration.loading`
// {test}`requirements.authoring.configuration_storage`
#[test]
fn invalid_schema_and_feature_values_report_the_configuration_path() {
    let path = Path::new("/settings/habeas/config.toml");
    for source in [
        "version = 2",
        "version = '1'",
        "",
        "version = 1\nunknown = true",
        "version = 1\n[authoring]",
        "version = 1\n[authoring]\nconfigs = {}\ndefault = '_bad'",
        "version = 1\n[authoring.configs.local]\nbackend = 'ollama'\nmodel = ''",
        "version = 1\n[authoring.configs.local]\nbackend = 'ollama'\nmodel = 'm'\nunknown = true",
        "version = 1\n[authoring.configs.local]\nbackend = 'unsupported'\nmodel = 'm'",
        "version = 1\n[authoring.configs.local]\nbackend = 'ollama'\nmodel = 'm'\napi_key_env = 'BAD=KEY'",
        "version = 1\n[authoring.configs.local]\nbackend = 'ollama'\nmodel = 'm'\n[authoring.configs.local.options]\nvalue = nan",
        "version = 1\n[authoring.configs.local]\nbackend = 'ollama'\nmodel = 'm'\n[authoring.configs.local.options]\nmodel = 'override'",
        "version = 1\n[authoring]\nconfigs = {}\n[authoring.projects.relative]\nconfig = 'local'",
    ] {
        let error = UserConfig::parse(source, path).unwrap_err().to_string();
        assert!(error.contains("/settings/habeas/config.toml"), "{error}");
    }
    let valid = UserConfig::parse("version = 1\n[authoring]\nconfigs = {}", path).unwrap();
    assert!(valid.authoring.unwrap().configs.is_empty());
}

// {test}`requirements.authoring.configuration_precedence`
// {test}`requirements.configuration.serialization`
#[test]
fn ollama_authentication_resolves_separately_and_rejects_missing_values() {
    let config = UserConfig::parse(
        "version = 1\n[authoring.configs.remote]\nbackend = 'ollama'\nmodel = 'm'\napi_key_env = 'MODEL_KEY'",
        Path::new("config.toml"),
    ).unwrap();
    let BackendConfig::Ollama(ollama) =
        &config.authoring.as_ref().unwrap().configs["remote"].backend
    else {
        panic!("wrong backend")
    };
    assert!(ollama.api_key_from(|_| None).is_err());
    assert!(ollama.api_key_from(|_| Some(OsString::new())).is_err());
    assert_eq!(
        ollama
            .api_key_from(|name| (name == "MODEL_KEY").then(|| OsString::from("private-value")))
            .unwrap()
            .as_deref(),
        Some("private-value")
    );
    let serialized = config.to_toml().unwrap();
    assert!(serialized.contains("MODEL_KEY"));
    assert!(!serialized.contains("private-value"));
}

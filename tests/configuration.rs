use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::Path;

use habeas::config::{
    BackendConfig, BackendKind, ConfigPlatform, ProjectConfig, UserConfig, WorkspaceConfig,
    load_user_config, load_workspace_config, load_workspace_evidence, resolve_authoring_from,
    resolve_authoring_with_workspace_from, state_root_from, user_config_path_from,
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
// {test}`requirements.authoring.configuration_precedence`
// {test}`requirements.authoring.configuration_storage`
// {test}`requirements.authoring.concurrency`
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
    assert!(!serialized.contains("concurrency"));
    assert_eq!(UserConfig::parse(&serialized, path).unwrap(), config);
}

// {test}`requirements.authoring.concurrency`
// {test}`requirements.authoring.configuration_structure`
// {test}`requirements.authoring.configuration_serialization`
// {test}`requirements.authoring.configuration_precedence`
// {test}`requirements.authoring.configuration_storage`
// {test}`requirements.configuration.loading`
// {test}`requirements.configuration.serialization`
#[test]
fn authoring_concurrency_defaults_overrides_and_invalid_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    for backend in ["bedrock", "ollama"] {
        let base =
            format!("version = 1\n[authoring.configs.model]\nbackend = '{backend}'\nmodel = 'm'\n");
        let defaults = UserConfig::parse(&base, &path).unwrap();
        let resolved =
            resolve_authoring_from(&path, Some(&defaults), dir.path(), Some("model")).unwrap();
        assert_eq!(resolved.concurrency, None);
        let expected = if backend == "bedrock" {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        } else {
            1
        };
        assert_eq!(resolved.effective_concurrency(), expected);
        assert!(!defaults.to_toml().unwrap().contains("concurrency"));

        let configured = UserConfig::parse(&format!("{base}concurrency = 7\n"), &path).unwrap();
        let resolved =
            resolve_authoring_from(&path, Some(&configured), dir.path(), Some("model")).unwrap();
        assert_eq!(resolved.concurrency.unwrap().get(), 7);
        assert_eq!(resolved.effective_concurrency(), 7);
        let serialized = configured.to_toml().unwrap();
        assert!(serialized.contains("concurrency = 7"));
        assert_eq!(UserConfig::parse(&serialized, &path).unwrap(), configured);
        assert!(resolved.backend.options().is_empty());

        for invalid in ["0", "-1", "1.5", "'2'"] {
            let error = UserConfig::parse(&format!("{base}concurrency = {invalid}\n"), &path)
                .unwrap_err()
                .to_string();
            assert!(error.contains(&path.display().to_string()), "{error}");
            assert!(error.contains("concurrency"), "{error}");
        }
    }
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
// {test}`requirements.authoring.configuration_serialization`
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
// {test}`requirements.authoring.credentials`
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

// {test}`requirements.configuration.workspace_location`
// {test}`requirements.authoring.configuration_serialization`
// {test}`requirements.authoring.configuration_structure`
// {test}`requirements.configuration.workspace_structure`
// {test}`requirements.configuration.loading`
// {test}`requirements.configuration.serialization`
#[test]
fn workspace_discovery_uses_nearest_file_without_merging_or_rewriting_it() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let nested = project.join("nested/deep");
    fs::create_dir_all(&nested).unwrap();
    let outer = temp.path().join("habeas.toml");
    fs::write(&outer, "version = 1\n[evidence]\nallow = ['outer']\n").unwrap();
    let inherited = load_workspace_config(&project).unwrap().unwrap();
    assert_eq!(inherited.path, outer);
    assert_eq!(inherited.root, temp.path().canonicalize().unwrap());
    let path = project.join("nested/habeas.toml");
    let text = "# preserved\nversion = 1\n[authoring]\ndefault = 'cloud'\n[evidence]\ndisallow = ['private']\n";
    fs::write(&path, text).unwrap();
    let closest = load_workspace_config(&nested).unwrap().unwrap();
    assert_eq!(closest.path, path);
    assert_eq!(closest.root, path.parent().unwrap());
    assert!(
        closest
            .config
            .authoring
            .as_ref()
            .unwrap()
            .configs
            .is_empty()
    );
    assert!(closest.config.evidence.as_ref().unwrap().allow.is_empty());
    assert_eq!(
        closest.config.evidence.as_ref().unwrap().disallow,
        vec![Path::new("private")]
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
    let serialized = closest.config.to_toml().unwrap();
    assert!(!serialized.contains("configs"));
    assert_eq!(
        WorkspaceConfig::parse(&serialized, &path).unwrap(),
        closest.config
    );

    fs::write(&path, "version = 1\n[evidence\n").unwrap();
    let error = load_workspace_config(&nested).unwrap_err().to_string();
    assert!(error.contains(path.to_str().unwrap()), "{error}");
    assert!(error.contains("invalid TOML"), "{error}");
    fs::remove_file(path).unwrap();
    fs::remove_file(outer).unwrap();
    assert!(load_workspace_config(&nested).unwrap().is_none());
}

// {test}`requirements.configuration.workspace_structure`
// {test}`requirements.configuration.evidence_paths`
// {test}`requirements.configuration.loading`
// {test}`requirements.authoring.credentials`
#[test]
fn evidence_configuration_ignores_unused_authoring_settings_and_detects_credentials() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("habeas.toml");
    fs::write(&path, "version = 1\n[evidence]\nallow = ['generated', '../shared']\ndisallow = ['generated/private']\n[authoring.configs.cloud]\nbackend = 'unknown'\ncredentials = 'DO_NOT_PRINT_THIS_CREDENTIAL'\n").unwrap();
    let policy = load_workspace_evidence(temp.path()).unwrap().unwrap();
    assert_eq!(policy.path, path);
    assert_eq!(policy.root, temp.path().canonicalize().unwrap());
    assert_eq!(
        policy.evidence.allow,
        vec![Path::new("generated"), Path::new("../shared")]
    );
    assert!(policy.contains_credentials);
    let error = load_workspace_config(temp.path()).unwrap_err().to_string();
    assert!(!error.contains("DO_NOT_PRINT_THIS_CREDENTIAL"), "{error}");

    fs::write(&path, "version = 1\n[authoring]\ndefault = 123\n").unwrap();
    assert!(
        !load_workspace_evidence(temp.path())
            .unwrap()
            .unwrap()
            .contains_credentials
    );
    assert!(load_workspace_config(temp.path()).is_err());
    for source in [
        "version = 2",
        "version = 1\nunknown = true",
        "version = 1\n[evidence]\nallow = ['']",
        "version = 1\n[evidence]\ndisallow = [42]",
        "version = 1\n[evidence]\nunknown = []",
    ] {
        fs::write(&path, source).unwrap();
        let error = load_workspace_evidence(temp.path())
            .unwrap_err()
            .to_string();
        assert!(error.contains(path.to_str().unwrap()), "{error}");
    }
}

// {test}`requirements.configuration.workspace_structure`
// {test}`requirements.authoring.configuration`
// {test}`requirements.authoring.configuration_precedence`
// {test}`requirements.authoring.configuration_storage`
// {test}`requirements.configuration.serialization`
#[test]
fn workspace_defaults_and_configs_combine_with_existing_user_selections() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let user_path = root.join("user.toml");
    let mut user = UserConfig::parse("version = 1\n[authoring]\ndefault = 'user'\n[authoring.configs.user]\nbackend = 'bedrock'\nmodel = 'user-model'\naws_profile = 'user-profile'\n[authoring.configs.shared]\nbackend = 'bedrock'\nmodel = 'old-model'\nregion = 'us-west-2'\nconcurrency = 7\n", &user_path).unwrap();
    let path = root.join("habeas.toml");
    fs::write(&path, "version = 1\n[authoring]\ndefault = 'workspace'\n[authoring.configs.workspace]\nbackend = 'ollama'\nmodel = 'workspace-model'\n[authoring.configs.shared]\nbackend = 'ollama'\nmodel = 'replacement'\n").unwrap();
    let mut workspace = load_workspace_config(&root).unwrap().unwrap();
    let selected = resolve_authoring_with_workspace_from(
        &user_path,
        Some(&user),
        Some(&workspace),
        &root,
        None,
    )
    .unwrap();
    assert_eq!(selected.name, "workspace");
    assert_eq!(selected.config_path, path);
    let replaced = resolve_authoring_with_workspace_from(
        &user_path,
        Some(&user),
        Some(&workspace),
        &root,
        Some("shared"),
    )
    .unwrap();
    assert_eq!(replaced.config_path, path);
    assert_eq!(replaced.concurrency, None);
    assert!(
        matches!(&replaced.backend, BackendConfig::Ollama(config) if config.model == "replacement" && config.endpoint.is_none())
    );

    user.authoring.as_mut().unwrap().projects.insert(
        root.clone(),
        habeas::config::ProjectSelection {
            config: "user".into(),
        },
    );
    let project = resolve_authoring_with_workspace_from(
        &user_path,
        Some(&user),
        Some(&workspace),
        &root,
        None,
    )
    .unwrap();
    assert_eq!(project.name, "user");
    assert_eq!(project.config_path, user_path);
    let explicit = resolve_authoring_with_workspace_from(
        &user_path,
        Some(&user),
        Some(&workspace),
        &root,
        Some("workspace"),
    )
    .unwrap();
    assert_eq!(explicit.name, "workspace");
    user.authoring.as_mut().unwrap().projects.clear();
    workspace.config.authoring.as_mut().unwrap().default = Some("user".into());
    let user_reference = resolve_authoring_with_workspace_from(
        &user_path,
        Some(&user),
        Some(&workspace),
        &root,
        None,
    )
    .unwrap();
    assert_eq!(user_reference.name, "user");
    assert_eq!(user_reference.config_path, user_path);
    workspace.config.authoring.as_mut().unwrap().default = None;
    assert_eq!(
        resolve_authoring_with_workspace_from(
            &user_path,
            Some(&user),
            Some(&workspace),
            &root,
            None
        )
        .unwrap()
        .name,
        "user"
    );
    workspace.config.authoring.as_mut().unwrap().default = Some("workspace".into());
    let workspace_only =
        resolve_authoring_with_workspace_from(&user_path, None, Some(&workspace), &root, None)
            .unwrap();
    assert_eq!(workspace_only.name, "workspace");
    let error = resolve_authoring_with_workspace_from(
        &user_path,
        Some(&user),
        Some(&workspace),
        &root,
        Some("missing"),
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains(path.to_str().unwrap()) && error.contains("missing"),
        "{error}"
    );
}

// {test}`requirements.authoring.credentials`
// {test}`requirements.authoring.configuration_serialization`
// {test}`requirements.configuration.serialization`
// {test}`requirements.configuration.loading`
// {test}`requirements.authoring.configuration_structure`
#[test]
fn explicit_credentials_round_trip_but_diagnostics_and_debug_redact_them() {
    let path = Path::new("habeas.toml");
    let source = "version = 1\n[authoring.configs.cloud]\nbackend = 'bedrock'\nmodel = 'm'\n[authoring.configs.cloud.credentials]\naccess_key_id = 'PRIVATE_ACCESS_KEY'\nsecret_access_key = 'PRIVATE_SECRET_KEY'\nsession_token = 'PRIVATE_SESSION'\n[authoring.configs.local]\nbackend = 'ollama'\nmodel = 'm'\napi_key = 'PRIVATE_OLLAMA_KEY'\n";
    let config = WorkspaceConfig::parse(source, path).unwrap();
    let debug = format!("{config:?}");
    for secret in [
        "PRIVATE_ACCESS_KEY",
        "PRIVATE_SECRET_KEY",
        "PRIVATE_SESSION",
        "PRIVATE_OLLAMA_KEY",
    ] {
        assert!(!debug.contains(secret), "{debug}");
    }
    assert_eq!(
        WorkspaceConfig::parse(&config.to_toml().unwrap(), path).unwrap(),
        config
    );
    let BackendConfig::Ollama(local) = &config.authoring.as_ref().unwrap().configs["local"].backend
    else {
        panic!("wrong backend");
    };
    assert_eq!(
        local.api_key_from(|_| None).unwrap().as_deref(),
        Some("PRIVATE_OLLAMA_KEY")
    );
    for invalid_source in [
        "version = 1\n[authoring.configs.local]\nbackend = 'ollama'\nmodel = 'm'\napi_key = ['PRIVATE_OLLAMA_KEY']\n",
        "version = 1\n[authoring.configs.local]\nbackend = 'ollama'\nmodel = 'm'\napi_key = 'PRIVATE_OLLAMA_KEY' unexpected\n",
        "version = 1\n[authoring.configs.cloud]\nbackend = 'bedrock'\nmodel = 'm'\ncredentials = 'PRIVATE_SECRET_KEY'\n",
        "version = 1\n[authoring.configs.local]\nbackend = 'ollama'\nmodel = 'm'\napi_key = 123456789\n",
    ] {
        for error in [
            WorkspaceConfig::parse(invalid_source, path).unwrap_err(),
            UserConfig::parse(invalid_source, path).unwrap_err(),
        ] {
            let error = error.to_string();
            assert!(error.contains("habeas.toml"), "{error}");
            assert!(!error.contains("PRIVATE_"), "{error}");
            assert!(!error.contains("123456789"), "{error}");
        }
    }
    for invalid_source in [
        source.replace(
            "model = 'm'\n[authoring.configs.cloud.credentials]",
            "model = 'm'\naws_profile = 'p'\n[authoring.configs.cloud.credentials]",
        ),
        source.replace(
            "api_key = 'PRIVATE_OLLAMA_KEY'",
            "api_key = 'PRIVATE_OLLAMA_KEY'\napi_key_env = 'KEY'",
        ),
        source.replace(
            "secret_access_key = 'PRIVATE_SECRET_KEY'",
            "secret_access_key = ''",
        ),
        source.replace("session_token = 'PRIVATE_SESSION'", "session_token = ''"),
        source.replace("api_key = 'PRIVATE_OLLAMA_KEY'", "api_key = ''"),
    ] {
        assert!(WorkspaceConfig::parse(&invalid_source, path).is_err());
    }
}

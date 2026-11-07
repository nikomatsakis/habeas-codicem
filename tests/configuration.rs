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

// {test}`requirements.authoring.configuration_precedence`
#[test]
fn configurations_without_any_selection_report_how_to_choose_one() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let config_path = root.join("config.toml");
    // Configurations exist, but no name, project entry, or default selects one.
    let unselected = UserConfig::parse(
        "version = 1\n[authoring.configs.only]\nbackend = 'ollama'\nmodel = 'm:tag'\n",
        &config_path,
    )
    .unwrap();
    let error = resolve_authoring_from(&config_path, Some(&unselected), &root, None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("no configuration selected"), "{error}");
    assert!(error.contains("editing-the-file"), "{error}");
}

// {test}`requirements.configuration.structure`
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

// {test}`requirements.authoring.configuration_precedence`
#[test]
fn unknown_selections_name_the_file_that_made_them() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let user_path = root.join("user.toml");
    let workspace_path = root.join("habeas.toml");
    let user_text = user_path.to_str().unwrap();
    let workspace_text = workspace_path.to_str().unwrap();
    fs::write(
        &workspace_path,
        "version = 1\n[authoring.configs.local]\nbackend = 'ollama'\nmodel = 'm'\n",
    )
    .unwrap();
    let mut workspace = load_workspace_config(&root).unwrap().unwrap();
    let mut user = UserConfig::parse(
        "version = 1\n[authoring]\ndefault = 'user-default'\n[authoring.configs.other]\nbackend = 'ollama'\nmodel = 'm'\n",
        &user_path,
    )
    .unwrap();
    let resolve = |user: &UserConfig,
                   workspace: Option<&habeas::config::LoadedWorkspaceConfig>,
                   explicit: Option<&str>| {
        resolve_authoring_with_workspace_from(&user_path, Some(user), workspace, &root, explicit)
            .unwrap_err()
            .to_string()
    };

    // A dangling user default is reported against the user file even though a
    // workspace file exists, and both searched files are listed.
    let error = resolve(&user, Some(&workspace), None);
    assert!(error.starts_with(&format!("{user_text}: ")), "{error}");
    assert!(error.contains("\"user-default\""), "{error}");
    assert!(error.contains(workspace_text), "{error}");
    assert!(error.contains("editing-the-file"), "{error}");

    // So is a dangling project selection, which outranks the default.
    user.authoring.as_mut().unwrap().projects.insert(
        root.clone(),
        habeas::config::ProjectSelection {
            config: "project-choice".into(),
        },
    );
    let error = resolve(&user, Some(&workspace), None);
    assert!(error.starts_with(&format!("{user_text}: ")), "{error}");
    assert!(error.contains("\"project-choice\""), "{error}");
    user.authoring.as_mut().unwrap().projects.clear();

    // A dangling workspace default is reported against the workspace file.
    workspace.config.authoring.as_mut().unwrap().default = Some("gone".into());
    let error = resolve(&user, Some(&workspace), None);
    assert!(error.starts_with(&format!("{workspace_text}: ")), "{error}");
    assert!(error.contains("\"gone\""), "{error}");
    assert!(error.contains(user_text), "{error}");

    // An unknown `--config` name was searched for in both files.
    let error = resolve(&user, Some(&workspace), Some("absent"));
    assert!(error.contains("\"absent\""), "{error}");
    assert!(
        error.contains(user_text) && error.contains(workspace_text),
        "{error}"
    );

    // With no workspace file, only the user file is named.
    let error = resolve(&user, None, Some("absent"));
    assert!(error.starts_with(&format!("{user_text}: ")), "{error}");
    assert!(!error.contains(workspace_text), "{error}");
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

// {test}`requirements.authoring.configuration_serialization`
// {test}`requirements.authoring.configuration_structure`
// {test}`requirements.configuration.serialization`
#[test]
fn user_and_workspace_authoring_tables_round_trip_every_map() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let path = root.join("config.toml");

    // A user section with a project selection keeps the canonical key.
    let source = format!(
        "version = 1\n[authoring]\n[authoring.configs.local]\nbackend = 'ollama'\nmodel = 'm:tag'\n[authoring.projects.{:?}]\nconfig = 'local'\n",
        root.display().to_string()
    );
    let user = UserConfig::parse(&source, &path).unwrap();
    let projects = &user.authoring.as_ref().unwrap().projects;
    assert_eq!(projects[&root].config, "local");
    let serialized = user.to_toml().unwrap();
    assert!(serialized.contains("[authoring.projects."), "{serialized}");
    assert_eq!(UserConfig::parse(&serialized, &path).unwrap(), user);

    // A present-but-empty `configs` is preserved and still parses.
    let empty = UserConfig::parse("version = 1\n[authoring]\nconfigs = {}\n", &path).unwrap();
    let serialized = empty.to_toml().unwrap();
    assert!(serialized.contains("configs"), "{serialized}");
    let reparsed = UserConfig::parse(&serialized, &path).unwrap();
    assert_eq!(reparsed, empty);
    assert!(reparsed.authoring.unwrap().configs.is_empty());

    // A workspace table with populated configs and options round-trips, and
    // the production loader decodes it exactly as `WorkspaceConfig::parse`.
    let workspace_path = root.join("habeas.toml");
    let workspace_text = "version = 1\n[authoring]\ndefault = 'cloud'\n[authoring.configs.cloud]\nbackend = 'bedrock'\nmodel = 'arn:aws:bedrock:us-east-1:123:inference-profile/us.model-v1:0'\nregion = 'us-east-1'\n[authoring.configs.cloud.options]\ntemperature = 0.5\n[authoring.configs.cloud.options.nested]\nflag = true\n[authoring.configs.local]\nbackend = 'ollama'\nmodel = 'local:7b'\n[authoring.configs.local.options]\nnum_ctx = 2048\n";
    fs::write(&workspace_path, workspace_text).unwrap();
    let workspace = WorkspaceConfig::parse(workspace_text, &workspace_path).unwrap();
    let loaded = load_workspace_config(&root).unwrap().unwrap();
    assert_eq!(loaded.config, workspace);
    let configs = &workspace.authoring.as_ref().unwrap().configs;
    assert_eq!(configs["cloud"].backend.options().len(), 2);
    assert_eq!(configs["local"].backend.options().len(), 1);
    let serialized = workspace.to_toml().unwrap();
    assert!(serialized.contains("backend = \"bedrock\""), "{serialized}");
    assert!(
        serialized.contains("[authoring.configs.cloud.options"),
        "{serialized}"
    );
    assert_eq!(
        WorkspaceConfig::parse(&serialized, &workspace_path).unwrap(),
        workspace
    );

    // `ModelRef` keeps the exact Bedrock identifier and nothing else.
    for (model, region) in [
        (
            "arn:aws:bedrock:us-east-1:123:inference-profile/us.model-v1:0",
            "us-east-1",
        ),
        ("us.anthropic.claude-v1:0", "eu-west-1"),
    ] {
        let config = UserConfig::parse(
            &format!("version = 1\n[authoring.configs.c]\nbackend = 'bedrock'\nmodel = '{model}'\naws_profile = 'p'\nregion = '{region}'\n"),
            &path,
        )
        .unwrap();
        let backend = &config.authoring.as_ref().unwrap().configs["c"].backend;
        assert_eq!(
            backend.model_ref(),
            habeas::config::ModelRef {
                backend: BackendKind::Bedrock,
                model: model.to_owned(),
            }
        );
        let BackendConfig::Bedrock(bedrock) = backend else {
            panic!("wrong backend")
        };
        assert_eq!(bedrock.aws_profile.as_deref(), Some("p"));
        assert_eq!(bedrock.region.as_deref(), Some(region));
    }
}

// {test}`requirements.authoring.configuration_storage`
// {test}`requirements.authoring.configuration_structure`
// {test}`requirements.configuration.loading`
#[test]
fn storage_rules_reject_each_invalid_field_with_its_setting() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let path = root.join("config.toml");
    let named = |name: &str| {
        format!("version = 1\n[authoring.configs.{name}]\nbackend = 'ollama'\nmodel = 'm'\n")
    };
    let ollama = |extra: &str| format!("{}{extra}", named("local"));
    let project = |key: &str, config: &str| {
        format!(
            "version = 1\n[authoring]\nconfigs = {{}}\n[authoring.projects.{key:?}]\nconfig = '{config}'\n"
        )
    };
    let root_text = root.display().to_string();
    let base = root.file_name().unwrap().to_string_lossy().into_owned();
    let mut cases = vec![
        // Configuration names.
        (named("\"\""), "authoring.configs."),
        (named("\"a.b\""), "authoring.configs.a.b"),
        (named("\"a b\""), "authoring.configs.a b"),
        (named("\"é\""), "authoring.configs.é"),
        (named("\"-x\""), "authoring.configs.-x"),
        (named("_x"), "authoring.configs._x"),
        (
            "version = 1\n[authoring]\nconfigs = {}\ndefault = 'a.b'\n".to_owned(),
            "authoring.default",
        ),
        (project(&root_text, "bad name"), ".config"),
        // Project-selection keys must be canonical absolute paths.
        (project("relative", "local"), "authoring.projects.relative"),
        (
            project(&format!("{root_text}/../{base}"), "local"),
            "authoring.projects.",
        ),
        (
            project(&format!("{root_text}/./"), "local"),
            "authoring.projects.",
        ),
        (
            project(&format!("{root_text}/"), "local"),
            "authoring.projects.",
        ),
        // Model IDs.
        (
            "version = 1\n[authoring.configs.c]\nbackend = 'bedrock'\nmodel = ''\n".to_owned(),
            "authoring.configs.c.model",
        ),
        // `api_key_env` names.
        (
            ollama("api_key_env = ''\n"),
            "authoring.configs.local.api_key_env",
        ),
        (
            ollama("api_key_env = 'A=B'\n"),
            "authoring.configs.local.api_key_env",
        ),
        (
            ollama("api_key_env = \"A\\u0000B\"\n"),
            "authoring.configs.local.api_key_env",
        ),
        // Non-finite floats, including nested ones.
        (
            ollama("[authoring.configs.local.options]\nvalue = nan\n"),
            "authoring.configs.local.options.value",
        ),
        (
            ollama("[authoring.configs.local.options]\nvalue = inf\n"),
            "authoring.configs.local.options.value",
        ),
        (
            ollama("[authoring.configs.local.options]\nvalue = [1.0, -inf]\n"),
            "authoring.configs.local.options.value",
        ),
        (
            ollama("[authoring.configs.local.options.value]\ninner = nan\n"),
            "authoring.configs.local.options.value",
        ),
    ];
    #[cfg(unix)]
    {
        let link = root.join("link");
        std::os::unix::fs::symlink(&root, &link).unwrap();
        cases.push((
            project(&link.display().to_string(), "local"),
            "authoring.projects.",
        ));
    }
    // Options may not replace the model, prompt, or tools.
    for key in [
        "model",
        "modelId",
        "model_id",
        "prompt",
        "messages",
        "system",
        "tools",
        "tool_choice",
        "toolChoice",
        "toolConfig",
    ] {
        for backend in ["ollama", "bedrock"] {
            cases.push((
                format!("version = 1\n[authoring.configs.c]\nbackend = '{backend}'\nmodel = 'm'\n[authoring.configs.c.options]\n{key} = 'x'\n"),
                "authoring.configs.c.options.",
            ));
        }
    }
    for (source, setting) in &cases {
        let error = UserConfig::parse(source, &path).unwrap_err().to_string();
        assert!(error.contains(path.to_str().unwrap()), "{source}: {error}");
        assert!(error.contains(setting), "{source}: {error}");
    }

    // Workspace files apply the same name and field rules.
    let workspace_path = root.join("habeas.toml");
    for (source, setting) in [
        (
            "version = 1\n[authoring]\ndefault = '_bad'\n".to_owned(),
            "authoring.default",
        ),
        (named("\"a.b\""), "authoring.configs.a.b"),
        (
            ollama("api_key_env = ''\n"),
            "authoring.configs.local.api_key_env",
        ),
    ] {
        let error = WorkspaceConfig::parse(&source, &workspace_path)
            .unwrap_err()
            .to_string();
        assert!(error.contains("habeas.toml"), "{source}: {error}");
        assert!(error.contains(setting), "{source}: {error}");
    }

    // Accepted values: case-sensitive distinct names, opaque model IDs with
    // tags, a canonical existing root, and a canonical root that does not exist.
    let missing = root.join("not-yet-created");
    let accepted = UserConfig::parse(
        &format!(
            "version = 1\n[authoring]\n[authoring.configs.Local]\nbackend = 'ollama'\nmodel = 'name:tag@sha256:abc'\napi_key_env = 'MODEL_KEY_1'\n[authoring.configs.local]\nbackend = 'ollama'\nmodel = 'm'\n[authoring.configs.A-1_b]\nbackend = 'bedrock'\nmodel = 'arn:aws:bedrock:us-east-1:1:x/y'\n[authoring.projects.{root_text:?}]\nconfig = 'Local'\n[authoring.projects.{:?}]\nconfig = 'A-1_b'\n",
            missing.display().to_string()
        ),
        &path,
    )
    .unwrap();
    let authoring = accepted.authoring.unwrap();
    assert_eq!(authoring.configs.len(), 3);
    assert_eq!(
        authoring.configs["Local"].backend.model_ref().model,
        "name:tag@sha256:abc"
    );
    assert_eq!(authoring.projects.len(), 2);
}

// {test}`requirements.authoring.concurrency`
// {test}`requirements.authoring.configuration_precedence`
#[test]
fn workspace_configurations_accept_validate_and_override_concurrency() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let user_path = root.join("user.toml");
    let user = UserConfig::parse(
        "version = 1\n[authoring.configs.shared]\nbackend = 'bedrock'\nmodel = 'user'\nconcurrency = 5\n",
        &user_path,
    )
    .unwrap();
    let path = root.join("habeas.toml");
    for (workspace, expected) in [("concurrency = 3\n", Some(3)), ("", None)] {
        for backend in ["bedrock", "ollama"] {
            fs::write(
                &path,
                format!("version = 1\n[authoring.configs.shared]\nbackend = '{backend}'\nmodel = 'local'\n{workspace}"),
            )
            .unwrap();
            let loaded = load_workspace_config(&root).unwrap().unwrap();
            let resolved = resolve_authoring_with_workspace_from(
                &user_path,
                Some(&user),
                Some(&loaded),
                &root,
                Some("shared"),
            )
            .unwrap();
            // The workspace config replaces the user config as a whole,
            // including its concurrency.
            assert_eq!(resolved.config_path, path);
            assert_eq!(resolved.concurrency.map(|value| value.get()), expected);
            let default = if backend == "bedrock" {
                std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(1)
            } else {
                1
            };
            assert_eq!(
                resolved.effective_concurrency(),
                expected.unwrap_or(default)
            );
            let serialized = loaded.config.to_toml().unwrap();
            assert_eq!(serialized.contains("concurrency = 3"), expected.is_some());
        }
    }
    for invalid in ["0", "-1", "1.5", "'2'"] {
        let text = format!(
            "version = 1\n[authoring.configs.shared]\nbackend = 'ollama'\nmodel = 'm'\nconcurrency = {invalid}\n"
        );
        let error = WorkspaceConfig::parse(&text, &path)
            .unwrap_err()
            .to_string();
        assert!(error.contains("concurrency"), "{error}");
        fs::write(&path, text).unwrap();
        let error = load_workspace_config(&root).unwrap_err().to_string();
        assert!(error.contains(path.to_str().unwrap()), "{error}");
        assert!(error.contains("concurrency"), "{error}");
    }
}

// {test}`requirements.configuration.workspace_structure`
// {test}`requirements.configuration.loading`
#[test]
fn workspace_diagnostics_identify_the_file_and_setting() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("habeas.toml");
    let file = path.to_str().unwrap();
    for (source, setting) in [
        ("version = '1'", "version"),
        ("version = 2", "version"),
        ("version = 1\nunknown = true", "unknown"),
        (
            "version = 1\n[authoring]\ndefault = 123",
            "authoring.default",
        ),
        ("version = 1\n[authoring]\nbogus = 1", "authoring.bogus"),
        (
            "version = 1\n[authoring.configs.cloud]\nbackend = 'bedrock'\nmodel = 3",
            "authoring.configs.cloud",
        ),
        (
            "version = 1\n[evidence]\ndisallow = [42]",
            "evidence.disallow",
        ),
        ("version = 1\n[evidence]\nunknown = []", "evidence.unknown"),
        ("version = 1\n[evidence]\nallow = ['']", "evidence.allow[0]"),
    ] {
        fs::write(&path, source).unwrap();
        let errors = [
            load_workspace_config(temp.path()).map(|_| ()),
            WorkspaceConfig::parse(source, &path).map(|_| ()),
        ];
        for error in errors {
            let error = error.unwrap_err().to_string();
            assert!(error.starts_with(&format!("{file}: {setting}")), "{error}");
        }
    }
    // The evidence loader decodes only the `[evidence]` table it uses.
    for (source, setting) in [
        (
            "version = 1\n[evidence]\ndisallow = [42]",
            "evidence.disallow",
        ),
        ("version = 1\n[evidence]\nunknown = []", "evidence.unknown"),
    ] {
        fs::write(&path, source).unwrap();
        let error = load_workspace_evidence(temp.path())
            .unwrap_err()
            .to_string();
        assert!(error.starts_with(&format!("{file}: {setting}")), "{error}");
    }
    fs::write(&path, b"version = 1\n# \xff\n").unwrap();
    let error = load_workspace_config(temp.path()).unwrap_err().to_string();
    assert!(error.contains(file), "{error}");
    fs::write(&path, "version = 1\n[evidence\n").unwrap();
    let error = load_workspace_config(temp.path()).unwrap_err().to_string();
    assert!(error.starts_with(file), "{error}");
    assert!(
        error.contains("invalid TOML syntax at line 2, column"),
        "{error}"
    );
    let error = UserConfig::parse("version = 1\n[authoring]\ndefault = 123", &path)
        .unwrap_err()
        .to_string();
    assert!(
        error.starts_with(&format!("{file}: authoring.default")),
        "{error}"
    );
}

// {test}`requirements.configuration.workspace_location`
// {test}`requirements.configuration.loading`
// {test}`requirements.configuration.structure`
#[test]
fn unreadable_configuration_files_are_reported_instead_of_skipped() {
    let temp = tempfile::tempdir().unwrap();
    let outer = temp.path().join("habeas.toml");
    fs::write(&outer, "version = 1\n").unwrap();
    let project = temp.path().join("project");
    fs::create_dir_all(project.join("habeas.toml")).unwrap();
    for error in [
        load_workspace_config(&project).map(|_| ()),
        load_workspace_evidence(&project).map(|_| ()),
    ] {
        let error = error.unwrap_err().to_string();
        assert!(
            error.contains(project.join("habeas.toml").to_str().unwrap()),
            "{error}"
        );
    }
    let user = temp.path().join("config.toml");
    fs::create_dir(&user).unwrap();
    let error = load_user_config(&user).unwrap_err().to_string();
    assert!(error.contains(user.to_str().unwrap()), "{error}");
    let binary = temp.path().join("binary.toml");
    fs::write(&binary, b"version = 1\n# \xff\n").unwrap();
    let error = load_user_config(&binary).unwrap_err().to_string();
    assert!(error.contains(binary.to_str().unwrap()), "{error}");

    // Loading an absent workspace file leaves the directory unchanged.
    let empty = tempfile::tempdir().unwrap();
    assert!(load_workspace_config(empty.path()).unwrap().is_none());
    assert!(load_workspace_evidence(empty.path()).unwrap().is_none());
    assert!(!empty.path().join("habeas.toml").exists());
    assert_eq!(fs::read_dir(empty.path()).unwrap().count(), 0);
}

// {test}`requirements.configuration.feature_sections`
// {test}`requirements.configuration.evidence_paths`
// {test}`requirements.configuration.structure`
#[test]
fn feature_sections_are_scoped_and_absent_sections_use_defaults() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("habeas.toml");
    // An absent `[evidence]` table, or omitted arrays, mean empty lists.
    for source in ["version = 1\n", "version = 1\n[evidence]\n"] {
        fs::write(&path, source).unwrap();
        let policy = load_workspace_evidence(temp.path()).unwrap().unwrap();
        assert!(policy.evidence.allow.is_empty() && policy.evidence.disallow.is_empty());
    }
    let config = WorkspaceConfig::parse("version = 1\n", &path).unwrap();
    assert!(config.authoring.is_none() && config.evidence.is_none());
    // Feature settings must stay within their sections.
    for source in [
        "version = 1\nallow = ['generated']\n",
        "version = 1\ndefault = 'cloud'\n",
        "version = 1\n[authoring]\nallow = ['generated']\n",
        "version = 1\n[evidence]\ndefault = 'cloud'\n",
    ] {
        assert!(WorkspaceConfig::parse(source, &path).is_err(), "{source}");
    }
    assert!(UserConfig::parse("version = 1\n[evidence]\nallow = []\n", &path).is_err());
    // The evidence feature does not reinterpret or validate authoring settings.
    fs::write(
        &path,
        "version = 1\n[authoring]\ndefault = 'missing'\nunknown = 1\n[evidence]\nallow = ['generated']\n",
    )
    .unwrap();
    let policy = load_workspace_evidence(temp.path()).unwrap().unwrap();
    assert_eq!(policy.evidence.allow, vec![Path::new("generated")]);
    // Authoring settings stay in `[authoring]`; its provider-owned options
    // cannot replace harness settings.
    for source in [
        "version = 1\n[authoring.configs.c]\nbackend = 'ollama'\nmodel = 'm'\nallow = []\n",
        "version = 1\n[authoring.configs.c]\nbackend = 'ollama'\nmodel = 'm'\n[authoring.configs.c.options]\nmodel = 'x'\n",
    ] {
        assert!(UserConfig::parse(source, &path).is_err(), "{source}");
        assert!(WorkspaceConfig::parse(source, &path).is_err(), "{source}");
    }
    // An absent authoring section configures nothing.
    let user = UserConfig::parse("version = 1\n", &path).unwrap();
    assert!(user.authoring.is_none());
}

// {test}`requirements.configuration.documentation`
#[test]
fn documented_configuration_examples_are_complete_valid_files() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut checked = 0;
    let mut literal_credential_examples = 0;
    for document in [
        "md/reference.md",
        "md/requirements/configuration.md",
        "md/requirements/authoring.md",
    ] {
        let text = fs::read_to_string(root.join(document)).unwrap();
        let blocks = text.split("```toml\n").skip(1);
        for block in blocks {
            let source = block.split("\n```").next().unwrap();
            let path = Path::new(document);
            // Workspace files are the only ones with an `[evidence]` table.
            let parsed = if source.contains("[evidence]") {
                WorkspaceConfig::parse(source, path).map(|_| ())
            } else {
                UserConfig::parse(source, path).map(|_| ())
            };
            parsed.unwrap_or_else(|error| panic!("{document}: {error}\n{source}"));
            assert_eq!(source.lines().next(), Some("version = 1"), "{document}");
            // Primary examples name credential sources. The one literal-credential
            // example, in the authoring chapter, uses only uppercase placeholders.
            let secrets: Vec<_> = source
                .lines()
                .filter(|line| {
                    [
                        "api_key =",
                        "access_key_id =",
                        "secret_access_key =",
                        "session_token =",
                    ]
                    .iter()
                    .any(|key| line.starts_with(key))
                })
                .collect();
            let literal = source.contains(".credentials]")
                || source.lines().any(|line| line.starts_with("credentials"));
            if literal || !secrets.is_empty() {
                assert_eq!(document, "md/requirements/authoring.md", "{source}");
                literal_credential_examples += 1;
            }
            for line in secrets {
                let value = line.split_once('=').unwrap().1.trim().trim_matches('"');
                assert!(
                    !value.is_empty()
                        && value
                            .chars()
                            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'),
                    "{document}: {line}"
                );
            }
            checked += 1;
        }
    }
    assert_eq!(literal_credential_examples, 1);
    assert!(checked >= 14, "{checked}");
}

// {test}`requirements.configuration.loading`
#[test]
fn commands_without_evidence_inputs_ignore_an_invalid_workspace_file() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path();
    fs::write(
        project.join("habeas.toml"),
        "version = 1\n[evidence]\nallow = [1]\n",
    )
    .unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_habeas"))
            .current_dir(project)
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
    };
    let hook = run(&["hook", "session-start"]);
    assert!(hook.status.success(), "{hook:?}");
    let setup = run(&["setup"]);
    let stderr = String::from_utf8_lossy(&setup.stderr);
    assert!(stderr.contains("cannot find a project"), "{stderr}");
    assert!(!stderr.contains("habeas.toml"), "{stderr}");
    assert!(run(&["mdbook", "supports", "html"]).status.success());
    // Commands that resolve evidence inputs still report the invalid file.
    // (`status` and `info` report it as a validation finding instead.)
    let add_input = run(&[
        "add-input",
        "--for-requirement",
        "requirements.x",
        "--requirements",
        "requirements.y",
    ]);
    let stderr = String::from_utf8_lossy(&add_input.stderr);
    assert!(!add_input.status.success());
    assert!(stderr.contains("evidence.allow"), "{stderr}");
}

// {test}`requirements.configuration.documentation`
#[test]
fn documented_configuration_examples_name_the_file_to_edit() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut checked = 0;
    for document in [
        "md/reference.md",
        "md/requirements/configuration.md",
        "md/requirements/authoring.md",
    ] {
        let text = fs::read_to_string(root.join(document)).unwrap();
        let mut pieces = text.split("```toml\n");
        let mut before = pieces.next().unwrap();
        for block in pieces {
            let (source, after) = block.split_once("\n```").unwrap();
            // The prose paragraph introducing the example, ignoring reference
            // role lines between it and the fence.
            let intro = before
                .trim_end()
                .rsplit("\n\n")
                .find(|paragraph| {
                    !paragraph
                        .lines()
                        .all(|line| line.starts_with('{') || line.trim().is_empty())
                })
                .unwrap_or_default();
            let user = intro.contains("config.toml`");
            let workspace = intro.contains("habeas.toml`");
            assert!(
                user || workspace,
                "{document}: example does not name its file:\n{intro}\n{source}"
            );
            let path = Path::new(document);
            if user {
                UserConfig::parse(source, path)
                    .unwrap_or_else(|error| panic!("{document}: user file: {error}\n{source}"));
            }
            if workspace {
                WorkspaceConfig::parse(source, path).unwrap_or_else(|error| {
                    panic!("{document}: workspace file: {error}\n{source}")
                });
            }
            checked += 1;
            before = after;
        }
    }
    assert!(checked >= 8, "{checked}");
}

// {test}`requirements.configuration.user_location`
// {test}`requirements.configuration.state_location`
#[test]
fn base_directories_must_be_absolute_and_nonempty() {
    use std::ffi::OsString;
    let only = |name: &'static str, value: &'static str| {
        move |key: &str| (key == name).then(|| OsString::from(value))
    };
    for (platform, variable) in [
        (ConfigPlatform::Unix, "HOME"),
        (ConfigPlatform::Windows, "APPDATA"),
    ] {
        for value in ["", "relative/home"] {
            let error = user_config_path_from(platform, only(variable, value)).unwrap_err();
            assert!(error.to_string().contains(variable), "{error}");
        }
    }
    for (platform, variable) in [
        (ConfigPlatform::Unix, "HOME"),
        (ConfigPlatform::Windows, "LOCALAPPDATA"),
    ] {
        for value in ["", "relative/home"] {
            let error = state_root_from(platform, only(variable, value)).unwrap_err();
            assert!(error.to_string().contains(variable), "{error}");
        }
    }
    // A relative XDG value falls back to an absolute HOME.
    let lookup = |key: &str| match key {
        "XDG_CONFIG_HOME" | "XDG_STATE_HOME" => Some(OsString::from("relative")),
        "HOME" => Some(OsString::from("/home/user")),
        _ => None,
    };
    assert_eq!(
        user_config_path_from(ConfigPlatform::Unix, lookup).unwrap(),
        std::path::PathBuf::from("/home/user/.config/habeas/config.toml")
    );
    assert_eq!(
        state_root_from(ConfigPlatform::Unix, lookup).unwrap(),
        std::path::PathBuf::from("/home/user/.local/state/habeas")
    );
}

//! Saved configuration and resolution for evidence authoring.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

const CONFIG_EXAMPLES: &str = "see md/requirements/authoring.md#editing-the-file";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserConfig {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authoring: Option<UserAuthoringConfig>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserAuthoringConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    pub configs: BTreeMap<String, AuthoringConfig>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub projects: BTreeMap<PathBuf, ProjectSelection>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectSelection {
    pub config: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AuthoringConfig {
    #[serde(flatten)]
    pub backend: BackendConfig,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "backend", rename_all = "lowercase")]
pub enum BackendConfig {
    Bedrock(BedrockConfig),
    Ollama(OllamaConfig),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BedrockConfig {
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aws_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, OptionValue>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OllamaConfig {
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, OptionValue>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OptionValue {
    String(String),
    Boolean(bool),
    Integer(i64),
    Float(f64),
    Array(Vec<OptionValue>),
    Map(BTreeMap<String, OptionValue>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    Bedrock,
    Ollama,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRef {
    pub backend: BackendKind,
    pub model: String,
}

/// The selected saved values together with their source, without resolved secrets.
#[derive(Clone, Debug)]
pub struct ResolvedAuthoringConfig {
    pub config_path: PathBuf,
    pub project_root: PathBuf,
    pub name: String,
    pub backend: BackendConfig,
}

#[derive(Clone, Copy, Debug)]
pub enum ConfigPlatform {
    Unix,
    Windows,
}

impl ConfigPlatform {
    // {impl}`requirements.configuration.user_location`
    // {impl}`requirements.configuration.state_location`
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unix
        }
    }
}

impl UserConfig {
    // {impl}`requirements.configuration.structure`
    // {impl}`requirements.configuration.loading`
    // {impl}`requirements.authoring.configuration_storage`
    pub fn parse(text: &str, path: &Path) -> io::Result<Self> {
        let config: Self = toml::from_str(text).map_err(|error| invalid(path, "", error))?;
        config.validate(path)?;
        Ok(config)
    }

    // {impl}`requirements.configuration.serialization`
    // {impl}`requirements.authoring.configuration_serialization`
    pub fn to_toml(&self) -> io::Result<String> {
        self.validate(Path::new("user configuration"))?;
        toml::to_string_pretty(self)
            .map_err(|error| invalid(Path::new("user configuration"), "", error))
    }

    // {impl}`requirements.configuration.structure`
    // {impl}`requirements.configuration.loading`
    // {impl}`requirements.authoring.configuration_storage`
    pub fn validate(&self, path: &Path) -> io::Result<()> {
        if self.version != 1 {
            return Err(invalid(path, "version", "expected version = 1"));
        }
        if let Some(authoring) = &self.authoring {
            authoring.validate(path)?;
        }
        Ok(())
    }
}

impl ProjectConfig {
    // {impl}`requirements.configuration.structure`
    // {impl}`requirements.configuration.loading`
    pub fn from_book_toml(text: &str, path: &Path) -> io::Result<Option<Self>> {
        let book: toml::Table = toml::from_str(text).map_err(|error| invalid(path, "", error))?;
        book.get("habeas")
            .map(|value| {
                value
                    .clone()
                    .try_into()
                    .map_err(|error| invalid(path, "habeas", error))
            })
            .transpose()
    }
}

impl UserAuthoringConfig {
    // {impl}`requirements.authoring.configuration_storage`
    // {impl}`requirements.configuration.loading`
    pub fn validate(&self, path: &Path) -> io::Result<()> {
        if let Some(name) = &self.default {
            validate_name(path, "authoring.default", name)?;
        }
        for (name, config) in &self.configs {
            let setting = format!("authoring.configs.{name}");
            validate_name(path, &setting, name)?;
            config.backend.validate(path, &setting)?;
        }
        for (root, selection) in &self.projects {
            let setting = format!("authoring.projects.{}", root.display());
            if !root.is_absolute()
                || root
                    .components()
                    .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
                || fs::canonicalize(root).is_ok_and(|canonical| canonical != *root)
            {
                return Err(invalid(
                    path,
                    &setting,
                    "expected a canonical absolute project path",
                ));
            }
            validate_name(path, &format!("{setting}.config"), &selection.config)?;
        }
        Ok(())
    }
}

impl BackendConfig {
    // {impl}`requirements.authoring.configuration_structure`
    pub fn model_ref(&self) -> ModelRef {
        match self {
            Self::Bedrock(config) => ModelRef {
                backend: BackendKind::Bedrock,
                model: config.model.clone(),
            },
            Self::Ollama(config) => ModelRef {
                backend: BackendKind::Ollama,
                model: config.model.clone(),
            },
        }
    }

    // {impl}`requirements.authoring.configuration_storage`
    pub fn options(&self) -> &BTreeMap<String, OptionValue> {
        match self {
            Self::Bedrock(config) => &config.options,
            Self::Ollama(config) => &config.options,
        }
    }

    // {impl}`requirements.authoring.configuration_storage`
    // {impl}`requirements.authoring.rig_configuration_mapping`
    pub fn options_json(&self) -> io::Result<serde_json::Value> {
        for value in self.options().values() {
            value.validate()?;
        }
        serde_json::to_value(self.options()).map_err(io::Error::other)
    }

    // {impl}`requirements.authoring.configuration_storage`
    // {impl}`requirements.configuration.loading`
    fn validate(&self, path: &Path, setting: &str) -> io::Result<()> {
        if self.model_ref().model.is_empty() {
            return Err(invalid(
                path,
                &format!("{setting}.model"),
                "expected a nonempty model ID",
            ));
        }
        if let Self::Ollama(config) = self
            && let Some(name) = &config.api_key_env
            && (name.is_empty() || name.contains(['=', '\0']))
        {
            return Err(invalid(
                path,
                &format!("{setting}.api_key_env"),
                "expected a nonempty environment variable name without '=' or NUL",
            ));
        }
        for (key, value) in self.options() {
            let field = format!("{setting}.options.{key}");
            if matches!(
                key.as_str(),
                "model"
                    | "modelId"
                    | "model_id"
                    | "prompt"
                    | "messages"
                    | "system"
                    | "tools"
                    | "tool_choice"
                    | "toolChoice"
                    | "toolConfig"
            ) {
                return Err(invalid(
                    path,
                    &field,
                    "options cannot replace the model, prompt, or tools",
                ));
            }
            value
                .validate()
                .map_err(|error| invalid(path, &field, error))?;
        }
        Ok(())
    }
}

impl OptionValue {
    // {impl}`requirements.authoring.configuration_structure`
    // {impl}`requirements.authoring.configuration_storage`
    fn validate(&self) -> io::Result<()> {
        match self {
            Self::Float(value) if !value.is_finite() => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "inference option floats must be finite",
            )),
            Self::Array(values) => values.iter().try_for_each(Self::validate),
            Self::Map(values) => values.values().try_for_each(Self::validate),
            _ => Ok(()),
        }
    }
}

impl OllamaConfig {
    // {impl}`requirements.authoring.configuration_precedence`
    pub fn endpoint(&self) -> &str {
        self.endpoint.as_deref().unwrap_or("http://localhost:11434")
    }

    // {impl}`requirements.authoring.configuration_precedence`
    // {impl}`requirements.configuration.serialization`
    pub fn api_key(&self) -> io::Result<Option<String>> {
        self.api_key_from(|name| std::env::var_os(name))
    }

    // {impl}`requirements.authoring.configuration_precedence`
    // {impl}`requirements.configuration.serialization`
    pub fn api_key_from(
        &self,
        lookup: impl Fn(&str) -> Option<OsString>,
    ) -> io::Result<Option<String>> {
        self.api_key_env.as_ref().map(|name| {
            lookup(name)
                .filter(|value| !value.is_empty())
                .and_then(|value| value.into_string().ok())
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput,
                    format!("authoring api_key_env {name:?} must name a set, nonempty UTF-8 environment variable")))
        }).transpose()
    }
}

impl ResolvedAuthoringConfig {
    // {impl}`requirements.authoring.configuration_structure`
    pub fn model_ref(&self) -> ModelRef {
        self.backend.model_ref()
    }
}

// {impl}`requirements.configuration.user_location`
pub fn user_config_path() -> io::Result<PathBuf> {
    user_config_path_from(ConfigPlatform::current(), |name| std::env::var_os(name))
}

// {impl}`requirements.configuration.user_location`
pub fn user_config_path_from(
    platform: ConfigPlatform,
    lookup: impl Fn(&str) -> Option<OsString>,
) -> io::Result<PathBuf> {
    let base = match platform {
        ConfigPlatform::Windows => required_directory(&lookup, "APPDATA")?,
        ConfigPlatform::Unix => match absolute_directory(&lookup, "XDG_CONFIG_HOME") {
            Some(path) => path,
            None => required_directory(&lookup, "HOME")?.join(".config"),
        },
    };
    Ok(base.join("habeas/config.toml"))
}

// {impl}`requirements.configuration.state_location`
pub fn state_root() -> io::Result<PathBuf> {
    state_root_from(ConfigPlatform::current(), |name| std::env::var_os(name))
}

// {impl}`requirements.configuration.state_location`
pub fn state_root_from(
    platform: ConfigPlatform,
    lookup: impl Fn(&str) -> Option<OsString>,
) -> io::Result<PathBuf> {
    match platform {
        ConfigPlatform::Windows => {
            Ok(required_directory(&lookup, "LOCALAPPDATA")?.join("habeas/state"))
        }
        ConfigPlatform::Unix => Ok(match absolute_directory(&lookup, "XDG_STATE_HOME") {
            Some(path) => path.join("habeas"),
            None => required_directory(&lookup, "HOME")?.join(".local/state/habeas"),
        }),
    }
}

// {impl}`requirements.configuration.user_location`
// {impl}`requirements.configuration.state_location`
fn absolute_directory(lookup: &impl Fn(&str) -> Option<OsString>, name: &str) -> Option<PathBuf> {
    lookup(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

// {impl}`requirements.configuration.user_location`
// {impl}`requirements.configuration.state_location`
fn required_directory(
    lookup: &impl Fn(&str) -> Option<OsString>,
    name: &str,
) -> io::Result<PathBuf> {
    absolute_directory(lookup, name).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("cannot determine Habeas directory: {name} must be an absolute, nonempty path"),
        )
    })
}

// {impl}`requirements.configuration.loading`
// {impl}`requirements.configuration.serialization`
pub fn load_user_config(path: &Path) -> io::Result<Option<UserConfig>> {
    match fs::read_to_string(path) {
        Ok(text) => UserConfig::parse(&text, path).map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io::Error::new(
            error.kind(),
            format!("{}: {error}", path.display()),
        )),
    }
}

// {impl}`requirements.authoring.configuration`
// {impl}`requirements.authoring.configuration_precedence`
// {impl}`requirements.authoring.rig_configuration_mapping`
// {impl}`requirements.configuration.structure`
// {impl}`requirements.configuration.loading`
pub fn resolve_authoring(
    root: &Path,
    explicit: Option<&str>,
) -> io::Result<ResolvedAuthoringConfig> {
    let project_path = root.join("book.toml");
    let project_text = fs::read_to_string(&project_path).map_err(|error| {
        io::Error::new(error.kind(), format!("{}: {error}", project_path.display()))
    })?;
    ProjectConfig::from_book_toml(&project_text, &project_path)?;
    let path = user_config_path()?;
    let config = load_user_config(&path)?;
    resolve_authoring_from(&path, config.as_ref(), root, explicit)
}

// {impl}`requirements.authoring.configuration_precedence`
// {impl}`requirements.authoring.configuration_storage`
// {impl}`requirements.configuration.serialization`
pub fn resolve_authoring_from(
    path: &Path,
    user: Option<&UserConfig>,
    root: &Path,
    explicit: Option<&str>,
) -> io::Result<ResolvedAuthoringConfig> {
    let project_root = fs::canonicalize(root).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("cannot resolve project root {}: {error}", root.display()),
        )
    })?;
    if let Some(user) = user {
        user.validate(path)?;
    }
    let authoring = user
        .and_then(|config| config.authoring.as_ref())
        .ok_or_else(|| {
            invalid(
                path,
                "authoring",
                format!("no authoring configuration; {CONFIG_EXAMPLES}"),
            )
        })?;
    let selected = explicit
        .or_else(|| {
            authoring
                .projects
                .get(&project_root)
                .map(|selection| selection.config.as_str())
        })
        .or(authoring.default.as_deref())
        .ok_or_else(|| {
            invalid(
                path,
                "authoring",
                format!("no configuration selected; {CONFIG_EXAMPLES}"),
            )
        })?;
    let backend = authoring
        .configs
        .get(selected)
        .ok_or_else(|| {
            invalid(
                path,
                "authoring",
                format!("unknown configuration {selected:?}; {CONFIG_EXAMPLES}"),
            )
        })?
        .backend
        .clone();
    Ok(ResolvedAuthoringConfig {
        config_path: path.to_owned(),
        project_root,
        name: selected.to_owned(),
        backend,
    })
}

// {impl}`requirements.authoring.configuration_storage`
fn validate_name(path: &Path, setting: &str, name: &str) -> io::Result<()> {
    if name
        .as_bytes()
        .first()
        .is_none_or(|first| !first.is_ascii_alphanumeric())
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(invalid(
            path,
            setting,
            "configuration names must match [A-Za-z0-9][A-Za-z0-9_-]*",
        ));
    }
    Ok(())
}

// {impl}`requirements.configuration.loading`
fn invalid(path: &Path, setting: &str, message: impl std::fmt::Display) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("{}: {setting}: {message}", path.display()),
    )
}

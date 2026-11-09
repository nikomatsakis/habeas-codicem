//! Saved configuration and resolution for evidence authoring.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::num::NonZeroUsize;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

const CONFIG_EXAMPLES: &str = "see md/requirements/authoring.md#editing-the-file";

// {impl}`requirements.authoring.configuration_serialization`
// {impl}`requirements.configuration.structure`
// {impl}`requirements.configuration.feature_sections`
// {impl}`requirements.configuration.serialization`
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserConfig {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authoring: Option<UserAuthoringConfig>,
}

// {impl}`requirements.configuration.workspace_structure`
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {}

// {impl}`requirements.authoring.configuration_serialization`
// {impl}`requirements.configuration.workspace_structure`
// {impl}`requirements.configuration.feature_sections`
// {impl}`requirements.configuration.serialization`
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceConfig {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authoring: Option<WorkspaceAuthoringConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<EvidenceConfig>,
}

// {impl}`requirements.authoring.configuration_structure`
// {impl}`requirements.authoring.configuration_serialization`
// {impl}`requirements.configuration.feature_sections`
// {impl}`requirements.configuration.serialization`
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceAuthoringConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub configs: BTreeMap<String, AuthoringConfig>,
}

// {impl}`requirements.configuration.serialization`
// {impl}`requirements.configuration.evidence_paths`
// {impl}`requirements.configuration.feature_sections`
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disallow: Vec<PathBuf>,
}

// {impl}`requirements.configuration.workspace_location`
#[derive(Clone, Debug)]
pub struct LoadedWorkspaceConfig {
    pub path: PathBuf,
    pub root: PathBuf,
    pub config: WorkspaceConfig,
}

// {impl}`requirements.configuration.evidence_paths`
// {impl}`requirements.authoring.credentials`
#[derive(Clone, Debug)]
pub struct WorkspaceEvidenceConfig {
    pub path: PathBuf,
    pub root: PathBuf,
    pub evidence: EvidenceConfig,
    pub contains_credentials: bool,
}

/// A user-supplied credential value. Serialization preserves it; diagnostics do not.
// {impl}`requirements.authoring.configuration_structure`
// {impl}`requirements.authoring.configuration_serialization`
// {impl}`requirements.configuration.serialization`
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretString(String);

impl SecretString {
    // {impl}`requirements.authoring.credentials`
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    // {impl}`requirements.authoring.credentials`
    // {impl}`requirements.authoring.configuration_structure`
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<redacted>")
    }
}

// {impl}`requirements.authoring.configuration_structure`
// {impl}`requirements.authoring.configuration_serialization`
// {impl}`requirements.configuration.serialization`
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BedrockCredentials {
    pub access_key_id: SecretString,
    pub secret_access_key: SecretString,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_token: Option<SecretString>,
}

// {impl}`requirements.authoring.configuration_structure`
// {impl}`requirements.authoring.configuration_serialization`
// {impl}`requirements.configuration.feature_sections`
// {impl}`requirements.configuration.serialization`
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserAuthoringConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    pub configs: BTreeMap<String, AuthoringConfig>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub projects: BTreeMap<PathBuf, ProjectSelection>,
}

// {impl}`requirements.configuration.serialization`
// {impl}`requirements.authoring.configuration_structure`
// {impl}`requirements.authoring.configuration_serialization`
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectSelection {
    pub config: String,
}

// {impl}`requirements.authoring.configuration_structure`
// {impl}`requirements.authoring.configuration_serialization`
// {impl}`requirements.authoring.concurrency`
// {impl}`requirements.configuration.serialization`
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AuthoringConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<NonZeroUsize>,
    // {impl}`requirements.authoring.policy`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<AssessmentPolicy>,
    #[serde(flatten)]
    pub backend: BackendConfig,
}

// {impl}`requirements.configuration.serialization`
// {impl}`requirements.authoring.configuration_structure`
// {impl}`requirements.authoring.configuration_serialization`
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "backend", rename_all = "lowercase")]
pub enum BackendConfig {
    Bedrock(BedrockConfig),
    Ollama(OllamaConfig),
}

// {impl}`requirements.authoring.configuration_structure`
// {impl}`requirements.authoring.configuration_serialization`
// {impl}`requirements.authoring.rig_configuration_mapping`
// {impl}`requirements.configuration.serialization`
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BedrockConfig {
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aws_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials: Option<BedrockCredentials>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, OptionValue>,
}

// {impl}`requirements.authoring.configuration_structure`
// {impl}`requirements.authoring.configuration_serialization`
// {impl}`requirements.authoring.rig_configuration_mapping`
// {impl}`requirements.configuration.serialization`
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OllamaConfig {
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<SecretString>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, OptionValue>,
}

// {impl}`requirements.configuration.serialization`
// {impl}`requirements.authoring.configuration_structure`
// {impl}`requirements.authoring.configuration_serialization`
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

/// How each subject is assessed: `confirm` checks a `true` or FIXME result
/// with further independent conversations; `once` records a single
/// assessment.
// {impl}`requirements.configuration.serialization`
// {impl}`requirements.authoring.configuration_structure`
// {impl}`requirements.authoring.policy`
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssessmentPolicy {
    #[default]
    Confirm,
    Once,
}

impl AssessmentPolicy {
    /// The policy's name, as written in TOML and on the command line.
    // {impl}`requirements.authoring.policy`
    pub fn name(self) -> &'static str {
        match self {
            Self::Confirm => "confirm",
            Self::Once => "once",
        }
    }
}

impl std::str::FromStr for AssessmentPolicy {
    type Err = String;

    /// Parses `--policy <name>`, rejecting an unknown name.
    // {impl}`requirements.authoring.policy`
    fn from_str(name: &str) -> Result<Self, Self::Err> {
        match name {
            "confirm" => Ok(Self::Confirm),
            "once" => Ok(Self::Once),
            _ => Err(format!(
                "unknown assessment policy {name:?}; expected \"confirm\" or \"once\""
            )),
        }
    }
}

impl fmt::Display for AssessmentPolicy {
    // {impl}`requirements.authoring.policy`
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

// {impl}`requirements.authoring.configuration_structure`
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    Bedrock,
    Ollama,
}

// {impl}`requirements.authoring.configuration_structure`
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRef {
    pub backend: BackendKind,
    pub model: String,
}

// {impl}`requirements.authoring.concurrency`
// {impl}`requirements.configuration.serialization`
/// The selected saved values together with their source, without resolved secrets.
#[derive(Clone, Debug)]
pub struct ResolvedAuthoringConfig {
    pub config_path: PathBuf,
    pub project_root: PathBuf,
    pub name: String,
    pub concurrency: Option<NonZeroUsize>,
    // {impl}`requirements.authoring.policy`
    pub policy: Option<AssessmentPolicy>,
    pub backend: BackendConfig,
}

// {impl}`requirements.configuration.user_location`
// {impl}`requirements.configuration.state_location`
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
    // {impl}`requirements.authoring.configuration_serialization`
    // {impl}`requirements.authoring.concurrency`
    // {impl}`requirements.authoring.credentials`
    // {impl}`requirements.configuration.serialization`
    pub fn parse(text: &str, path: &Path) -> io::Result<Self> {
        let config: Self = decode(toml::Value::Table(parse_document(text, path)?), path, "")?;
        config.validate(path)?;
        Ok(config)
    }

    // {impl}`requirements.configuration.structure`
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

impl WorkspaceConfig {
    // {impl}`requirements.configuration.workspace_structure`
    // {impl}`requirements.configuration.loading`
    // {impl}`requirements.authoring.credentials`
    // {impl}`requirements.authoring.configuration_serialization`
    // {impl}`requirements.authoring.concurrency`
    // {impl}`requirements.configuration.serialization`
    pub fn parse(text: &str, path: &Path) -> io::Result<Self> {
        let table = parse_document(text, path)?;
        validate_workspace_common(&table, path)?;
        let config: Self = decode(toml::Value::Table(table), path, "")?;
        config.validate(path)?;
        Ok(config)
    }

    // {impl}`requirements.configuration.workspace_structure`
    // {impl}`requirements.configuration.serialization`
    // {impl}`requirements.authoring.configuration_serialization`
    pub fn to_toml(&self) -> io::Result<String> {
        self.validate(Path::new("workspace configuration"))?;
        toml::to_string_pretty(self).map_err(|_| {
            invalid(
                Path::new("workspace configuration"),
                "",
                "cannot serialize configuration",
            )
        })
    }

    // {impl}`requirements.configuration.workspace_structure`
    // {impl}`requirements.configuration.loading`
    // {impl}`requirements.authoring.configuration_storage`
    pub fn validate(&self, path: &Path) -> io::Result<()> {
        if self.version != 1 {
            return Err(invalid(path, "version", "expected version = 1"));
        }
        if let Some(authoring) = &self.authoring {
            if let Some(name) = &authoring.default {
                validate_name(path, "authoring.default", name)?;
            }
            for (name, config) in &authoring.configs {
                let setting = format!("authoring.configs.{name}");
                validate_name(path, &setting, name)?;
                config.backend.validate(path, &setting)?;
            }
        }
        if let Some(evidence) = &self.evidence {
            evidence.validate(path)?;
        }
        Ok(())
    }
}

impl EvidenceConfig {
    // {impl}`requirements.configuration.workspace_structure`
    // {impl}`requirements.configuration.evidence_paths`
    // {impl}`requirements.configuration.loading`
    pub fn validate(&self, path: &Path) -> io::Result<()> {
        for (field, paths) in [("allow", &self.allow), ("disallow", &self.disallow)] {
            for (index, entry) in paths.iter().enumerate() {
                if entry.as_os_str().is_empty() || entry.to_string_lossy().contains('\0') {
                    return Err(invalid(
                        path,
                        &format!("evidence.{field}[{index}]"),
                        "expected a nonempty path without NUL",
                    ));
                }
            }
        }
        Ok(())
    }
}

impl ProjectConfig {
    // {impl}`requirements.configuration.loading`
    // {impl}`requirements.configuration.workspace_structure`
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
                // `Path` equality ignores `.` components and trailing
                // separators, so compare the spelled-out keys instead.
                || root.components().collect::<PathBuf>().as_os_str() != root.as_os_str()
                || fs::canonicalize(root)
                    .is_ok_and(|canonical| canonical.as_os_str() != root.as_os_str())
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
    // {impl}`requirements.authoring.rig_configuration_mapping`
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
    // {impl}`requirements.configuration.feature_sections`
    // {impl}`requirements.authoring.credentials`
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
        match self {
            Self::Bedrock(config) => {
                if let Some(credentials) = &config.credentials {
                    if config.aws_profile.is_some() {
                        return Err(invalid(
                            path,
                            &format!("{setting}.credentials"),
                            "credentials and aws_profile cannot both be set",
                        ));
                    }
                    for (field, value) in [
                        ("access_key_id", Some(&credentials.access_key_id)),
                        ("secret_access_key", Some(&credentials.secret_access_key)),
                        ("session_token", credentials.session_token.as_ref()),
                    ] {
                        if value.is_some_and(|value| value.expose_secret().is_empty()) {
                            return Err(invalid(
                                path,
                                &format!("{setting}.credentials.{field}"),
                                "credential values must be nonempty",
                            ));
                        }
                    }
                }
            }
            Self::Ollama(config) => {
                if let Some(key) = &config.api_key {
                    if config.api_key_env.is_some() {
                        return Err(invalid(
                            path,
                            &format!("{setting}.api_key"),
                            "api_key and api_key_env cannot both be set",
                        ));
                    }
                    if key.expose_secret().is_empty() {
                        return Err(invalid(
                            path,
                            &format!("{setting}.api_key"),
                            "credential values must be nonempty",
                        ));
                    }
                }
            }
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
    // {impl}`requirements.authoring.rig_configuration_mapping`
    // {impl}`requirements.configuration.serialization`
    pub fn endpoint(&self) -> &str {
        self.endpoint.as_deref().unwrap_or("http://localhost:11434")
    }

    // {impl}`requirements.authoring.configuration_precedence`
    // {impl}`requirements.configuration.serialization`
    // {impl}`requirements.authoring.credentials`
    // {impl}`requirements.authoring.rig_configuration_mapping`
    pub fn api_key(&self) -> io::Result<Option<String>> {
        self.api_key_from(|name| std::env::var_os(name))
    }

    // {impl}`requirements.authoring.configuration_precedence`
    // {impl}`requirements.configuration.serialization`
    // {impl}`requirements.authoring.credentials`
    // {impl}`requirements.authoring.rig_configuration_mapping`
    pub fn api_key_from(
        &self,
        lookup: impl Fn(&str) -> Option<OsString>,
    ) -> io::Result<Option<String>> {
        if let Some(key) = &self.api_key {
            return Ok(Some(key.expose_secret().to_owned()));
        }
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

    /// The policy from `--policy` when given, otherwise from this
    /// configuration, otherwise `confirm`.
    // {impl}`requirements.authoring.policy`
    pub fn effective_policy(&self, explicit: Option<AssessmentPolicy>) -> AssessmentPolicy {
        explicit.or(self.policy).unwrap_or_default()
    }

    // {impl}`requirements.authoring.concurrency`
    pub fn effective_concurrency(&self) -> usize {
        self.concurrency
            .map(NonZeroUsize::get)
            .unwrap_or_else(|| match &self.backend {
                BackendConfig::Bedrock(_) => std::thread::available_parallelism()
                    .map(NonZeroUsize::get)
                    .unwrap_or(1),
                BackendConfig::Ollama(_) => 1,
            })
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

// {impl}`requirements.configuration.structure`
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

// {impl}`requirements.configuration.workspace_location`
// {impl}`requirements.configuration.loading`
// {impl}`requirements.configuration.workspace_structure`
// {impl}`requirements.configuration.serialization`
// {impl}`requirements.authoring.configuration_serialization`
// {impl}`requirements.authoring.concurrency`
pub fn load_workspace_config(root: &Path) -> io::Result<Option<LoadedWorkspaceConfig>> {
    let Some((path, table)) = workspace_document(root)? else {
        return Ok(None);
    };
    let config: WorkspaceConfig = decode(toml::Value::Table(table), &path, "")?;
    config.validate(&path)?;
    Ok(Some(LoadedWorkspaceConfig {
        root: path
            .parent()
            .expect("configuration has a parent")
            .to_owned(),
        path,
        config,
    }))
}

// {impl}`requirements.configuration.workspace_location`
// {impl}`requirements.configuration.workspace_structure`
// {impl}`requirements.configuration.evidence_paths`
// {impl}`requirements.configuration.loading`
// {impl}`requirements.authoring.credentials`
// {impl}`requirements.authoring.deterministic_validation`
// {impl}`requirements.configuration.feature_sections`
pub fn load_workspace_evidence(root: &Path) -> io::Result<Option<WorkspaceEvidenceConfig>> {
    let Some((path, table)) = workspace_document(root)? else {
        return Ok(None);
    };
    let evidence: EvidenceConfig = table
        .get("evidence")
        .map(|value| decode(value.clone(), &path, "evidence"))
        .transpose()?
        .unwrap_or_default();
    evidence.validate(&path)?;
    let contains_credentials = table
        .get("authoring")
        .and_then(|value| value.get("configs"))
        .and_then(toml::Value::as_table)
        .is_some_and(|configs| {
            configs.values().any(|config| {
                config.get("credentials").is_some() || config.get("api_key").is_some()
            })
        });
    Ok(Some(WorkspaceEvidenceConfig {
        root: path
            .parent()
            .expect("configuration has a parent")
            .to_owned(),
        path,
        evidence,
        contains_credentials,
    }))
}

// {impl}`requirements.configuration.workspace_location`
// {impl}`requirements.configuration.loading`
// {impl}`requirements.configuration.workspace_structure`
fn workspace_document(root: &Path) -> io::Result<Option<(PathBuf, toml::Table)>> {
    let root = fs::canonicalize(root)?;
    let start = std::env::current_dir()
        .and_then(fs::canonicalize)
        .ok()
        .filter(|cwd| cwd.starts_with(&root))
        .unwrap_or(root);
    for directory in start.ancestors() {
        let path = directory.join("habeas.toml");
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                let text = fs::read_to_string(&path).map_err(|error| {
                    io::Error::new(error.kind(), format!("{}: {error}", path.display()))
                })?;
                let table = parse_document(&text, &path)?;
                validate_workspace_common(&table, &path)?;
                return Ok(Some((path, table)));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(io::Error::new(
                    error.kind(),
                    format!("{}: {error}", path.display()),
                ));
            }
        }
    }
    Ok(None)
}

// {impl}`requirements.configuration.workspace_structure`
// {impl}`requirements.configuration.loading`
// {impl}`requirements.configuration.feature_sections`
fn validate_workspace_common(table: &toml::Table, path: &Path) -> io::Result<()> {
    if table.get("version").and_then(toml::Value::as_integer) != Some(1) {
        return Err(invalid(path, "version", "expected version = 1"));
    }
    if let Some(key) = table
        .keys()
        .find(|key| !matches!(key.as_str(), "version" | "authoring" | "evidence"))
    {
        return Err(invalid(path, key, "unknown workspace configuration field"));
    }
    Ok(())
}

// {impl}`requirements.configuration.loading`
// {impl}`requirements.authoring.credentials`
// {impl}`requirements.configuration.workspace_structure`
fn parse_document(text: &str, path: &Path) -> io::Result<toml::Table> {
    toml::from_str(text).map_err(|error: toml::de::Error| {
        let position = error
            .span()
            .map(|span| {
                let prefix = text.get(..span.start).unwrap_or(text);
                let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
                let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
                format!(" at line {line}, column {column}")
            })
            .unwrap_or_default();
        invalid(path, "", format!("invalid TOML syntax{position}"))
    })
}

// {impl}`requirements.configuration.loading`
// {impl}`requirements.authoring.credentials`
// {impl}`requirements.configuration.workspace_structure`
fn decode<T: DeserializeOwned>(value: toml::Value, path: &Path, setting: &str) -> io::Result<T> {
    let mut secrets = Vec::new();
    credential_values(&value, false, &mut secrets);
    secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
    value.try_into().map_err(|mut error: toml::de::Error| {
        error.set_input(None);
        let (mut message, setting) = decoded_setting(&error.to_string(), setting);
        for secret in secrets {
            if !secret.is_empty() {
                message = message
                    .replace(&format!("{secret:?}"), "<redacted>")
                    .replace(&secret, "<redacted>");
            }
        }
        invalid(path, &setting, message)
    })
}

/// Splits a TOML decoding message into its explanation and the full setting
/// it concerns: the decoder's trailing "in `key`" location joined to `prefix`,
/// plus the field name of an unknown-field error.
// {impl}`requirements.configuration.loading`
// {impl}`requirements.configuration.workspace_structure`
fn decoded_setting(message: &str, prefix: &str) -> (String, String) {
    let message = message.trim_end();
    let (message, location) = match message.rsplit_once("\nin `") {
        Some((message, location)) => (message, location.strip_suffix('`').unwrap_or(location)),
        None => (message, ""),
    };
    let unknown = message
        .strip_prefix("unknown field `")
        .and_then(|rest| rest.split_once('`'))
        .map_or("", |(field, _)| field);
    let setting = [prefix, location, unknown]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(".");
    (message.trim().to_owned(), setting)
}

// {impl}`requirements.authoring.credentials`
fn credential_values(value: &toml::Value, secret: bool, output: &mut Vec<String>) {
    match value {
        toml::Value::String(value) if secret => output.push(value.clone()),
        toml::Value::Integer(_)
        | toml::Value::Float(_)
        | toml::Value::Boolean(_)
        | toml::Value::Datetime(_)
            if secret =>
        {
            output.push(value.to_string())
        }
        toml::Value::Table(table) => {
            for (key, value) in table {
                credential_values(
                    value,
                    secret || matches!(key.as_str(), "credentials" | "api_key"),
                    output,
                );
            }
        }
        toml::Value::Array(values) => {
            for value in values {
                credential_values(value, secret, output);
            }
        }
        _ => {}
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
    let workspace = load_workspace_config(root)?;
    let path = user_config_path()?;
    let config = load_user_config(&path)?;
    resolve_authoring_with_workspace_from(
        &path,
        config.as_ref(),
        workspace.as_ref(),
        root,
        explicit,
    )
}

// {impl}`requirements.authoring.configuration_precedence`
// {impl}`requirements.authoring.configuration_storage`
// {impl}`requirements.configuration.serialization`
// {impl}`requirements.authoring.concurrency`
// {impl}`requirements.authoring.configuration`
// {impl}`requirements.configuration.loading`
pub fn resolve_authoring_from(
    path: &Path,
    user: Option<&UserConfig>,
    root: &Path,
    explicit: Option<&str>,
) -> io::Result<ResolvedAuthoringConfig> {
    resolve_authoring_with_workspace_from(path, user, None, root, explicit)
}

// {impl}`requirements.authoring.configuration_precedence`
// {impl}`requirements.authoring.configuration_storage`
// {impl}`requirements.configuration.workspace_structure`
// {impl}`requirements.configuration.loading`
// {impl}`requirements.authoring.configuration`
// {impl}`requirements.configuration.serialization`
// {impl}`requirements.authoring.concurrency`
pub fn resolve_authoring_with_workspace_from(
    path: &Path,
    user: Option<&UserConfig>,
    workspace: Option<&LoadedWorkspaceConfig>,
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
    if let Some(workspace) = workspace {
        workspace.config.validate(&workspace.path)?;
    }
    let authoring = user.and_then(|config| config.authoring.as_ref());
    let local = workspace.and_then(|workspace| workspace.config.authoring.as_ref());
    let workspace_path = workspace.map(|workspace| workspace.path.as_path());
    // Errors that concern both files name the workspace file, when there is
    // one, and also list every file that was searched.
    let diagnostic_path = workspace_path.unwrap_or(path);
    let searched = match workspace_path {
        Some(workspace_path) => format!(
            "searched {} and {}",
            path.display(),
            workspace_path.display()
        ),
        None => format!("searched {}", path.display()),
    };
    if authoring.is_none() && local.is_none() {
        return Err(invalid(
            diagnostic_path,
            "authoring",
            format!("no authoring configuration ({searched}); {CONFIG_EXAMPLES}"),
        ));
    }
    // Each selection records the file it came from, so an unknown name is
    // reported against the file that chose it.
    let selected = explicit
        .map(|name| (name, diagnostic_path))
        .or_else(|| {
            authoring?
                .projects
                .get(&project_root)
                .map(|selection| (selection.config.as_str(), path))
        })
        .or_else(|| {
            local
                .and_then(|local| local.default.as_deref())
                .zip(workspace_path)
        })
        .or_else(|| {
            authoring
                .and_then(|authoring| authoring.default.as_deref())
                .map(|name| (name, path))
        });
    let Some((selected, selection_path)) = selected else {
        return Err(invalid(
            diagnostic_path,
            "authoring",
            format!("no configuration selected ({searched}); {CONFIG_EXAMPLES}"),
        ));
    };
    let configured = local
        .and_then(|local| local.configs.get(selected))
        .map(|config| {
            (
                workspace_path.expect("local configuration has a file"),
                config,
            )
        })
        .or_else(|| {
            authoring
                .and_then(|authoring| authoring.configs.get(selected))
                .map(|config| (path, config))
        });
    let (source_path, config) = configured.ok_or_else(|| {
        invalid(
            selection_path,
            "authoring",
            format!("unknown configuration {selected:?} ({searched}); {CONFIG_EXAMPLES}"),
        )
    })?;
    Ok(ResolvedAuthoringConfig {
        config_path: source_path.to_owned(),
        project_root,
        name: selected.to_owned(),
        concurrency: config.concurrency,
        policy: config.policy,
        backend: config.backend.clone(),
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
// {impl}`requirements.configuration.workspace_structure`
fn invalid(path: &Path, setting: &str, message: impl std::fmt::Display) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("{}: {setting}: {message}", path.display()),
    )
}

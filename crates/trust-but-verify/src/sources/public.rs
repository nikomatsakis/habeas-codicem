mod hits;
use crate::resource::ResourceId;
use crate::source_engine::LocalSources;
use crate::source_engine::SourceAnalysis;
use crate::source_engine::SourceProvider;
use crate::source_engine::project_resource;

use serde_json::{Map, Value};
use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};

/// Redact recognized credentials in standalone text, preserving line breaks.
// {impl}`requirements.sdk.sources.redact_text`
pub fn redact_text(text: &str) -> std::borrow::Cow<'_, str> {
    crate::redaction::Redaction::of_text(text).apply(text)
}

/// The name used to look up an outline language for source inspection.
// {impl}`requirements.sdk.sources.language_id`
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct LanguageId(String);

impl LanguageId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for LanguageId {
    fn from(name: String) -> Self {
        Self(name)
    }
}

impl From<&str> for LanguageId {
    fn from(name: &str) -> Self {
        Self(name.to_owned())
    }
}

/// Selects the project files considered by listing and search operations.
// {impl}`requirements.sdk.graph.search.source_scope`
#[derive(Clone, Debug, Default)]
pub struct SourceScope {
    prefix: String,
}

impl SourceScope {
    /// Select all visible project files. This is also the default scope.
    // {impl}`requirements.sdk.graph.search.source_scope`
    pub fn all() -> Self {
        Self::default()
    }

    /// Select a project-relative file or the files beneath a directory.
    // {impl}`requirements.sdk.graph.search.source_scope`
    pub fn under(path: impl Into<String>) -> Self {
        Self {
            prefix: path.into(),
        }
    }

    pub(crate) fn path_prefix(&self) -> &str {
        &self.prefix
    }

    // {cite}`habeas:util`
    pub(crate) fn contains(&self, id: &ResourceId) -> bool {
        let mut prefix = PathBuf::new();
        for component in Path::new(&self.prefix).components() {
            match component {
                Component::Normal(part) => prefix.push(part),
                Component::CurDir => {}
                Component::ParentDir if prefix.pop() => {}
                _ => return false,
            }
        }
        Path::new(&id.location).starts_with(prefix)
    }
}

// {cite}`habeas:util`
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct SourceFile {
    pub bytes: Arc<[u8]>,
    pub language: Option<LanguageId>,
}

impl SourceFile {
    pub fn new(bytes: Arc<[u8]>) -> Self {
        Self {
            bytes,
            language: None,
        }
    }

    pub fn language(mut self, language: LanguageId) -> Self {
        self.language = Some(language);
        self
    }

    // {cite}`habeas:util`
    pub fn text(&self) -> Result<&str, SourceError> {
        std::str::from_utf8(&self.bytes).map_err(|error| SourceError::Invalid(error.to_string()))
    }
}

// {cite}`habeas:util`
#[derive(Debug)]
pub enum SourceError {
    Io(std::io::Error),
    Invalid(String),
}
impl std::fmt::Display for SourceError {
    // {cite}`habeas:util`
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Invalid(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for SourceError {}
impl From<std::io::Error> for SourceError {
    // {cite}`habeas:util`
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

// {cite}`habeas:util`
#[non_exhaustive]
pub struct TextQuery {
    pub scope: SourceScope,
    pub pattern: String,
    pub context_lines: usize,
    pub limit: Option<usize>,
}

impl TextQuery {
    pub fn new(pattern: String) -> Self {
        Self {
            scope: SourceScope::all(),
            pattern,
            context_lines: 0,
            limit: None,
        }
    }

    pub fn scope(mut self, scope: SourceScope) -> Self {
        self.scope = scope;
        self
    }

    pub fn context_lines(mut self, context_lines: usize) -> Self {
        self.context_lines = context_lines;
        self
    }

    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }
}
// {cite}`habeas:util`
#[non_exhaustive]
pub struct TextMatch {
    pub path: PathBuf,
    pub line: usize,
    pub excerpt: String,
}

impl TextMatch {
    pub fn new(path: PathBuf, line: usize, excerpt: String) -> Self {
        Self {
            path,
            line,
            excerpt,
        }
    }
}
// {cite}`habeas:util`
#[non_exhaustive]
pub struct StructuralQuery {
    pub scope: SourceScope,
    pub language: LanguageId,
    pub rule: Map<String, Value>,
    pub limit: Option<usize>,
}

impl StructuralQuery {
    pub fn new(language: LanguageId, rule: Map<String, Value>) -> Self {
        Self {
            scope: SourceScope::all(),
            language,
            rule,
            limit: None,
        }
    }

    pub fn scope(mut self, scope: SourceScope) -> Self {
        self.scope = scope;
        self
    }

    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }
}
// {cite}`habeas:util`
#[non_exhaustive]
pub struct StructuralMatch {
    pub node: crate::outline::OutlineData,
    pub excerpt: String,
}

impl StructuralMatch {
    pub fn new(node: crate::outline::OutlineData, excerpt: String) -> Self {
        Self { node, excerpt }
    }
}
// {impl}`requirements.sdk.sources.provider_results.search_completeness`
#[non_exhaustive]
pub struct SearchResults<T> {
    pub matches: Vec<T>,
    pub complete: bool,
}

impl<T> SearchResults<T> {
    pub fn new(matches: Vec<T>, complete: bool) -> Self {
        Self { matches, complete }
    }
}
pub type TextMatches = SearchResults<TextMatch>;
pub type StructuralMatches = SearchResults<StructuralMatch>;

/// Filesystem inspection rooted in a project directory.
///
/// Listing, reading, outlining, and searching share the project's `.gitignore`
/// rules. Untracked files are available unless ignored. The outline languages
/// enabled at compile time are selected by file extension.
pub struct Sources {
    pub(crate) analysis: SourceAnalysis<LocalSources>,
}

impl Sources {
    // {impl}`requirements.sdk.sources.provider`
    /// Inspect the project directory with the SDK's built-in defaults.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let languages = crate::outline_engine::languages::registry();
        Self {
            analysis: SourceAnalysis::new(LocalSources::new(root), languages),
        }
    }

    // {impl}`requirements.sdk.graph.validation-context.list_sources.fn`
    // {impl}`requirements.sdk.sources.provider.errors`
    /// List available project files within a relative path scope.
    pub async fn list(&self, scope: &SourceScope) -> Result<Vec<PathBuf>, SourceError> {
        Ok(self
            .analysis
            .provider
            .list(scope)
            .await?
            .into_iter()
            .map(|id| PathBuf::from(id.location))
            .collect())
    }

    // {impl}`requirements.sdk.sources.provider.errors`
    // {impl}`requirements.sdk.sources.read_file`
    /// Read an available project file, including a successful empty file.
    pub async fn read(&self, path: impl AsRef<Path>) -> Result<SourceFile, SourceError> {
        let id = project_resource(path.as_ref())?;
        self.analysis.provider.read(&id).await
    }

    // {impl}`requirements.sdk.graph.validation-context.outline_source.fn`
    // {impl}`requirements.sdk.sources.provider_defaults`
    /// Read and outline one file snapshot using its extension's language.
    pub async fn parse(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<crate::outline::OutlineContent, SourceError> {
        let path = path.as_ref();
        let file = self.read(path).await?;
        crate::outline::OutlineContent::parse_source(path, Arc::from(file.text()?))
            .map_err(|error| SourceError::Invalid(error.to_string()))
    }

    // {impl}`requirements.sdk.graph.search.search_text.fn`
    /// Search the available files with a regular expression.
    pub async fn search_text(&self, query: &TextQuery) -> Result<TextMatches, SourceError> {
        self.analysis.provider.search_text(query).await
    }

    // {impl}`requirements.sdk.graph.search.search_structure.fn`
    /// Search the available files with the selected language's syntax rules.
    pub async fn search_structure(
        &self,
        query: &StructuralQuery,
    ) -> Result<StructuralMatches, SourceError> {
        self.analysis.search_structure(query).await
    }
}

/// Project files and the domain mapping used by agent inspection tools.
// {impl}`requirements.sdk.sources.SourceProject`
pub struct SourceProject<D: crate::domain::Domain> {
    pub(crate) sources: Arc<Sources>,
    pub(crate) domain: std::sync::Arc<D>,
}

impl<D: crate::domain::Domain> SourceProject<D> {
    pub fn new(domain: D, sources: Sources) -> Self {
        Self {
            domain: Arc::new(domain),
            sources: Arc::new(sources),
        }
    }
}

impl From<SourceError> for std::io::Error {
    fn from(error: SourceError) -> Self {
        match error {
            SourceError::Io(e) => e,
            other => Self::other(other),
        }
    }
}

/// One occurrence in a retained project source snapshot.
#[derive(Debug)]
#[non_exhaustive]
pub struct SearchHit<N> {
    pub node: N,
    pub span: crate::outline::Span,
    pub text: String,
    pub(crate) item: crate::outline::OutlineContent,
}

impl<N> SearchHit<N> {
    /// Capture the matched bytes at a file-relative span in the retained outline.
    pub fn new(
        node: N,
        span: crate::outline::Span,
        item: crate::outline::OutlineContent,
    ) -> Result<Self, SourceError> {
        let text = item
            .source_span(span)
            .ok_or_else(|| SourceError::Invalid("invalid match span".into()))?
            .to_owned();
        Ok(Self {
            node,
            span,
            text,
            item,
        })
    }
}

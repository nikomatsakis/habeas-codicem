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

// {impl}`requirements.sdk.sources.redact_text`
/// Redact recognized credentials in standalone text, preserving line breaks.
pub fn redact_text(text: &str) -> std::borrow::Cow<'_, str> {
    crate::redaction::Redaction::of_text(text).apply(text)
}

// {impl}`requirements.sdk.sources.language_id`
/// The name used to look up an outline language for source inspection.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct LanguageId(String);

// {impl}`requirements.sdk.sources.language_id`
impl LanguageId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// {impl}`requirements.sdk.sources.language_id`
impl From<String> for LanguageId {
    fn from(name: String) -> Self {
        Self(name)
    }
}

// {impl}`requirements.sdk.sources.language_id`
impl From<&str> for LanguageId {
    fn from(name: &str) -> Self {
        Self(name.to_owned())
    }
}

// {impl}`requirements.sdk.graph.search.source_scope`
/// Selects the project files considered by listing and search operations.
#[derive(Clone, Debug, Default)]
pub struct SourceScope {
    prefix: String,
}

impl SourceScope {
    // {impl}`requirements.sdk.graph.search.source_scope`
    /// Select all visible project files. This is also the default scope.
    pub fn all() -> Self {
        Self::default()
    }

    // {impl}`requirements.sdk.graph.search.source_scope`
    /// Select a project-relative file or the files beneath a directory.
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

// {impl}`requirements.sdk.public_api`
// {impl}`requirements.sdk.sources.read_file`
// {cite}`habeas:util`
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct SourceFile {
    pub bytes: Arc<[u8]>,
    pub language: Option<LanguageId>,
}

// {impl}`requirements.sdk.public_api`
impl SourceFile {
    // {impl}`requirements.sdk.sources.read_file`
    pub fn new(bytes: Arc<[u8]>) -> Self {
        Self {
            bytes,
            language: None,
        }
    }

    // {impl}`requirements.sdk.sources.read_file`
    pub fn language(mut self, language: LanguageId) -> Self {
        self.language = Some(language);
        self
    }

    // {impl}`requirements.sdk.sources.read_file`
    // {cite}`habeas:util`
    pub fn text(&self) -> Result<&str, SourceError> {
        std::str::from_utf8(&self.bytes).map_err(|error| SourceError::Invalid(error.to_string()))
    }
}

// {impl}`requirements.sdk.sources.SourceError`
// {cite}`habeas:util`
#[derive(Debug)]
pub enum SourceError {
    Io(std::io::Error),
    Invalid(String),
}
// {impl}`requirements.sdk.sources.SourceError`
impl std::fmt::Display for SourceError {
    // {cite}`habeas:util`
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Invalid(error) => error.fmt(f),
        }
    }
}
// {impl}`requirements.sdk.sources.SourceError`
impl std::error::Error for SourceError {}
// {impl}`requirements.sdk.sources.SourceError`
impl From<std::io::Error> for SourceError {
    // {cite}`habeas:util`
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

// {impl}`requirements.sdk.graph.search.TextQuery`
// {impl}`requirements.sdk.public_api`
// {cite}`habeas:util`
#[non_exhaustive]
pub struct TextQuery {
    pub scope: SourceScope,
    pub pattern: String,
    pub context_lines: usize,
    pub limit: Option<usize>,
}

// {impl}`requirements.sdk.public_api`
impl TextQuery {
    // {impl}`requirements.sdk.graph.search.TextQuery`
    pub fn new(pattern: String) -> Self {
        Self {
            scope: SourceScope::all(),
            pattern,
            context_lines: 0,
            limit: None,
        }
    }

    // {impl}`requirements.sdk.graph.search.TextQuery`
    pub fn scope(mut self, scope: SourceScope) -> Self {
        self.scope = scope;
        self
    }

    // {impl}`requirements.sdk.graph.search.TextQuery`
    pub fn context_lines(mut self, context_lines: usize) -> Self {
        self.context_lines = context_lines;
        self
    }

    // {impl}`requirements.sdk.graph.search.TextQuery`
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }
}
// {impl}`requirements.sdk.graph.search.TextMatch`
// {impl}`requirements.sdk.public_api`
// {impl}`requirements.sdk.sources.source_addresses`
// {cite}`habeas:util`
#[non_exhaustive]
pub struct TextMatch {
    pub path: PathBuf,
    pub line: usize,
    pub excerpt: String,
}

// {impl}`requirements.sdk.public_api`
// {impl}`requirements.sdk.graph.search.TextMatch`
impl TextMatch {
    pub fn new(path: PathBuf, line: usize, excerpt: String) -> Self {
        Self {
            path,
            line,
            excerpt,
        }
    }
}
/// A reusable structural search rule for source text in a bundled language.
pub struct StructuralRule {
    inner: crate::structural::StructuralRule,
}

// {impl}`requirements.sdk.graph.search.StructuralRule`
impl StructuralRule {
    /// Names accepted by the structural rule compiler.
    pub fn languages() -> Vec<&'static str> {
        crate::structural::QueryLanguage::names()
    }

    /// Compile an ast-grep rule core before searching any source.
    pub fn compile(language: &str, rule: &Map<String, Value>) -> Result<Self, SourceError> {
        let inner = crate::structural::StructuralRule::compile(language, rule)
            .map_err(SourceError::Invalid)?;
        Ok(Self { inner })
    }

    /// Filename extensions of the rule's language, without the leading dot.
    pub fn extensions(&self) -> &[&str] {
        self.inner.language().extensions()
    }

    /// Distinct half-open byte spans in the supplied text, in source order.
    pub fn matches(&self, source: &str) -> Vec<crate::outline::Span> {
        self.inner
            .matches(source)
            .into_iter()
            .map(crate::outline::Span::from_range)
            .collect()
    }
}

// {impl}`requirements.sdk.graph.search.StructuralQuery`
// {impl}`requirements.sdk.public_api`
// {impl}`requirements.sdk.sources.language_id`
// {cite}`habeas:util`
#[non_exhaustive]
pub struct StructuralQuery {
    pub scope: SourceScope,
    pub language: LanguageId,
    pub rule: Map<String, Value>,
    pub limit: Option<usize>,
}

// {impl}`requirements.sdk.public_api`
impl StructuralQuery {
    // {impl}`requirements.sdk.graph.search.StructuralQuery`
    pub fn new(language: LanguageId, rule: Map<String, Value>) -> Self {
        Self {
            scope: SourceScope::all(),
            language,
            rule,
            limit: None,
        }
    }

    // {impl}`requirements.sdk.graph.search.StructuralQuery`
    pub fn scope(mut self, scope: SourceScope) -> Self {
        self.scope = scope;
        self
    }

    // {impl}`requirements.sdk.graph.search.StructuralQuery`
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }
}
// {impl}`requirements.sdk.sources.source_addresses`
// {impl}`requirements.sdk.graph.search.StructuralMatch`
// {impl}`requirements.sdk.public_api`
// {cite}`habeas:util`
#[non_exhaustive]
pub struct StructuralMatch {
    pub node: crate::outline::OutlineData,
    pub excerpt: String,
}

// {impl}`requirements.sdk.public_api`
impl StructuralMatch {
    // {impl}`requirements.sdk.graph.search.StructuralMatch`
    pub fn new(node: crate::outline::OutlineData, excerpt: String) -> Self {
        Self { node, excerpt }
    }
}
// {impl}`requirements.sdk.graph.search.SearchResults`
// {impl}`requirements.sdk.public_api`
/// Construct a result with [`SearchResults::new`].
///
/// ```compile_fail,E0639
/// use trust_but_verify::sources::SearchResults;
/// let _ = SearchResults { matches: vec![1], complete: true };
/// ```
// {test}`requirements.sdk.graph.search.SearchResults`
#[non_exhaustive]
pub struct SearchResults<T> {
    pub matches: Vec<T>,
    pub complete: bool,
}

// {impl}`requirements.sdk.public_api`
impl<T> SearchResults<T> {
    // {impl}`requirements.sdk.graph.search.SearchResults`
    pub fn new(matches: Vec<T>, complete: bool) -> Self {
        Self { matches, complete }
    }
}
// {impl}`requirements.sdk.graph.search.TextMatches`
pub type TextMatches = SearchResults<TextMatch>;
// {impl}`requirements.sdk.graph.search.StructuralMatches`
pub type StructuralMatches = SearchResults<StructuralMatch>;

// {impl}`requirements.sdk.sources.Sources`
// {impl}`requirements.sdk.sources.provider`
/// Filesystem inspection rooted in a project directory.
///
/// Listing, reading, outlining, and searching share the project's `.gitignore`
/// rules. Untracked files are available unless ignored. The outline languages
/// enabled at compile time are selected by file extension.
pub struct Sources {
    pub(crate) analysis: SourceAnalysis<LocalSources>,
}

// {impl}`requirements.sdk.sources.provider`
impl Sources {
    // {impl}`requirements.sdk.sources.Sources`
    // {impl}`requirements.sdk.sources.provider`
    /// Inspect the project directory with the SDK's built-in defaults.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let languages = crate::outline_engine::languages::registry();
        Self {
            analysis: SourceAnalysis::new(LocalSources::new(root), languages),
        }
    }

    // {impl}`requirements.sdk.sources.Sources`
    // {impl}`requirements.sdk.sources.source_addresses`
    // {impl}`requirements.sdk.graph.validation-context.list_sources.fn`
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

    // {impl}`requirements.sdk.sources.Sources`
    // {impl}`requirements.sdk.sources.read_file`
    // {impl}`requirements.sdk.sources.source_addresses`
    /// Read an available project file, including a successful empty file.
    pub async fn read(&self, path: impl AsRef<Path>) -> Result<SourceFile, SourceError> {
        let id = project_resource(path.as_ref())?;
        self.analysis.provider.read(&id).await
    }

    // {impl}`requirements.sdk.sources.Sources`
    // {impl}`requirements.sdk.graph.validation-context.outline_source.fn`
    // {impl}`requirements.sdk.sources.source_addresses`
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

    // {impl}`requirements.sdk.sources.Sources`
    // {impl}`requirements.sdk.graph.search.search_text.fn`
    /// Search the available files with a regular expression.
    pub async fn search_text(&self, query: &TextQuery) -> Result<TextMatches, SourceError> {
        self.analysis.provider.search_text(query).await
    }

    // {impl}`requirements.sdk.sources.Sources`
    // {impl}`requirements.sdk.graph.search.search_structure.fn`
    /// Search the available files with the selected language's syntax rules.
    pub async fn search_structure(
        &self,
        query: &StructuralQuery,
    ) -> Result<StructuralMatches, SourceError> {
        self.analysis.search_structure(query).await
    }
}

// {impl}`requirements.sdk.sources.SourceProject`
/// Project files and the domain mapping used by agent inspection tools.
pub struct SourceProject<D: crate::domain::Domain> {
    pub(crate) sources: Arc<Sources>,
    pub(crate) domain: std::sync::Arc<D>,
}

impl<D: crate::domain::Domain> SourceProject<D> {
    // {impl}`requirements.sdk.sources.SourceProject`
    pub fn new(domain: D, sources: Sources) -> Self {
        Self {
            domain: Arc::new(domain),
            sources: Arc::new(sources),
        }
    }
}

// {impl}`requirements.sdk.sources.SourceError`
impl From<SourceError> for std::io::Error {
    fn from(error: SourceError) -> Self {
        match error {
            SourceError::Io(e) => e,
            other => Self::other(other),
        }
    }
}

// {impl}`requirements.sdk.graph.search.occurrence`
// {impl}`requirements.sdk.public_api`
/// One occurrence in a retained project source snapshot.
#[derive(Debug)]
#[non_exhaustive]
pub struct SearchHit<N> {
    pub node: N,
    pub span: crate::outline::Span,
    pub text: String,
    pub(crate) item: crate::outline::OutlineContent,
}

// {impl}`requirements.sdk.public_api`
impl<N> SearchHit<N> {
    // {impl}`requirements.sdk.graph.search.occurrence`
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

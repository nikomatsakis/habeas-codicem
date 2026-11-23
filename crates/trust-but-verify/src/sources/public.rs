#[cfg(not(feature = "default-languages"))]
use crate::outline_engine::language::Registry;
use crate::resource::ResourceId;
use crate::source_engine::LocalSources;
use crate::source_engine::SourceAnalysis;
use crate::source_engine::SourceProvider;
use crate::source_engine::project_path_visible;
use crate::source_engine::project_resource;
use serde_json::{Map, Value};
use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};

pub type LanguageId = String;

// {cite}`habeas:util`
#[derive(Clone, Debug, Default)]
pub struct SourceScope {
    pub prefix: String,
}

impl SourceScope {
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
pub struct SourceFile {
    pub bytes: Arc<[u8]>,
    pub language: Option<LanguageId>,
}
impl SourceFile {
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
pub struct TextQuery {
    pub scope: SourceScope,
    pub pattern: String,
    pub context_lines: usize,
    pub limit: Option<usize>,
}
// {cite}`habeas:util`
pub struct TextMatch {
    pub path: PathBuf,
    pub line: usize,
    pub excerpt: String,
}
// {cite}`habeas:util`
pub struct StructuralQuery {
    pub scope: SourceScope,
    pub language: LanguageId,
    pub rule: Map<String, Value>,
    pub limit: Option<usize>,
}
// {cite}`habeas:util`
pub struct StructuralMatch {
    pub node: crate::outline::OutlineId,
    pub excerpt: String,
}
// {impl}`requirements.sdk.sources.provider_results.search_completeness`
pub struct SearchResults<T> {
    pub matches: Vec<T>,
    pub complete: bool,
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
    pub(crate) fn check_path(&self, path: &Path) -> Result<(), SourceError> {
        let root = self.analysis.provider.project_directory()?;
        let requested = path
            .to_str()
            .ok_or_else(|| SourceError::Invalid("source path is not UTF-8".into()))?;
        let target = self.analysis.provider.path(requested)?;
        let original = root.join(path);
        if !project_path_visible(&root, &original)? || !project_path_visible(&root, &target)? {
            return Err(SourceError::Invalid(
                "source path is excluded from the project".into(),
            ));
        }
        Ok(())
    }
    // {impl}`requirements.sdk.sources.provider`
    /// Inspect the project directory with the SDK's built-in defaults.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        #[cfg(feature = "default-languages")]
        let languages = crate::outline_engine::languages::registry();
        #[cfg(not(feature = "default-languages"))]
        let languages = Registry::new([]);
        Self {
            analysis: SourceAnalysis::new(LocalSources::new(root), languages),
        }
    }

    // {impl}`requirements.sdk.graph.context-tools.list_sources.fn`
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
    /// Read an available project file, including a successful empty file.
    pub async fn read(&self, path: impl AsRef<Path>) -> Result<SourceFile, SourceError> {
        let id = project_resource(path.as_ref())?;
        self.analysis.provider.read(&id).await
    }

    // {impl}`requirements.sdk.graph.context-tools.outline_source.fn`
    // {impl}`requirements.sdk.sources.provider_defaults`
    /// Read and outline one file snapshot using its extension's language.
    pub async fn parse(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<crate::outline::OutlineContent, SourceError> {
        let path = path.as_ref();
        let file = self.read(path).await?;
        crate::outline::OutlineContent::from_source(path, Arc::from(file.text()?))
            .map_err(|error| SourceError::Invalid(error.to_string()))
    }

    // {impl}`requirements.sdk.graph.context-tools.search_text.fn`
    /// Search the available files with a regular expression.
    pub async fn search_text(&self, query: &TextQuery) -> Result<TextMatches, SourceError> {
        self.analysis.provider.search_text(query).await
    }

    // {impl}`requirements.sdk.graph.context-tools.search_structure.fn`
    /// Search the available files with the selected language's syntax rules.
    pub async fn search_structure(
        &self,
        query: &StructuralQuery,
    ) -> Result<StructuralMatches, SourceError> {
        self.analysis.search_structure(query).await
    }
}

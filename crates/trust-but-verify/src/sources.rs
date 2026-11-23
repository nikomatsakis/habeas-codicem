//! Built-in project inspection and its internal source adapters.

use crate::outline::language::Registry;
use crate::{
    outline::{self, Outline},
    resource::{OutlineId, ResourceId},
    search::RegexSearch,
    structural::{StructuralRule, innermost_node},
};
use serde_json::{Map, Value};
use std::{
    future::Future,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
};

pub type LanguageId = String;

// {cite}`habeas:util`
#[derive(Clone, Debug, Default)]
pub struct SourceScope {
    pub prefix: String,
}

impl SourceScope {
    // {cite}`habeas:util`
    pub fn contains(&self, id: &ResourceId) -> bool {
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

// {impl}`requirements.sdk.sources.provider_results.parse_snapshot`
pub struct ParsedSource {
    pub resource: ResourceId,
    pub source: SourceFile,
    pub outline: Outline,
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
    pub resource: ResourceId,
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
    pub node: OutlineId,
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
    // {impl}`requirements.sdk.sources.provider`
    /// Inspect the project directory with the SDK's built-in defaults.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        #[cfg(feature = "default-languages")]
        let languages = crate::outline::languages::registry();
        #[cfg(not(feature = "default-languages"))]
        let languages = Registry::new([]);
        Self {
            analysis: SourceAnalysis::new(LocalSources::new(root), languages),
        }
    }

    // {impl}`requirements.sdk.graph.context-tools.list_sources.fn`
    // {impl}`requirements.sdk.sources.provider.errors`
    /// List available project files within a relative path scope.
    pub async fn list(&self, scope: &SourceScope) -> Result<Vec<ResourceId>, SourceError> {
        self.analysis.provider.list(scope).await
    }

    // {impl}`requirements.sdk.sources.provider.errors`
    /// Read an available project file, including a successful empty file.
    pub async fn read(&self, id: &ResourceId) -> Result<SourceFile, SourceError> {
        self.analysis.provider.read(id).await
    }

    // {impl}`requirements.sdk.graph.context-tools.outline_source.fn`
    // {impl}`requirements.sdk.sources.provider_defaults`
    /// Read and outline one file snapshot using its extension's language.
    pub async fn parse(&self, id: &ResourceId) -> Result<ParsedSource, SourceError> {
        self.analysis.parse(id).await
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

// {impl}`requirements.sdk.sources.provider`
// {impl}`requirements.sdk.sources.provider.errors`
// {impl}`requirements.sdk.sources.provider_defaults`
pub trait SourceProvider: Send + Sync {
    type Error: From<SourceError> + Send;

    // {impl}`requirements.sdk.graph.context-tools.list_sources.fn`
    fn list(
        &self,
        scope: &SourceScope,
    ) -> impl Future<Output = Result<Vec<ResourceId>, Self::Error>> + Send;
    fn read(&self, id: &ResourceId)
    -> impl Future<Output = Result<SourceFile, Self::Error>> + Send;

    // {cite}`habeas:util`
    fn parse(
        &self,
        id: &ResourceId,
        languages: &Registry,
    ) -> impl Future<Output = Result<ParsedSource, Self::Error>> + Send {
        defaults::parse(self, id, languages)
    }
    // {impl}`requirements.sdk.graph.context-tools.search_text.fn`
    fn search_text(
        &self,
        query: &TextQuery,
    ) -> impl Future<Output = Result<TextMatches, Self::Error>> + Send {
        defaults::search_text(self, query)
    }
    // {cite}`habeas:util`
    fn search_structure(
        &self,
        query: &StructuralQuery,
        languages: &Registry,
    ) -> impl Future<Output = Result<StructuralMatches, Self::Error>> + Send {
        defaults::search_structure(self, query, languages)
    }
}

/// Interpret bytes from one observation without refetching the resource.
pub fn parse_snapshot(
    id: &ResourceId,
    source: SourceFile,
    languages: &Registry,
) -> Result<ParsedSource, SourceError> {
    let text = source.text()?;
    let provider = match &source.language {
        Some(language) => Some(languages.named(language).ok_or_else(|| {
            SourceError::Invalid(format!("unknown source language {language:?}"))
        })?),
        None => languages.for_path(Path::new(&id.location)),
    };
    let outline = match provider {
        Some(provider) => outline::parse_provider(provider, text),
        None => outline::parse_with(&outline::plain::PROFILE, text),
    }
    .map_err(|error| SourceError::Invalid(error.to_string()))?;
    Ok(ParsedSource {
        resource: id.clone(),
        source,
        outline,
    })
}

pub mod defaults {
    use super::*;

    // {impl}`requirements.sdk.sources.provider_defaults.composition`
    pub async fn parse<P: SourceProvider + ?Sized>(
        provider: &P,
        id: &ResourceId,
        languages: &Registry,
    ) -> Result<ParsedSource, P::Error> {
        parse_snapshot(id, provider.read(id).await?, languages).map_err(Into::into)
    }

    pub async fn search_text<P: SourceProvider + ?Sized>(
        provider: &P,
        query: &TextQuery,
    ) -> Result<TextMatches, P::Error> {
        let search = RegexSearch::compile(&query.pattern).map_err(SourceError::from)?;
        let mut matches = Vec::new();
        for id in provider.list(&query.scope).await? {
            if !query.scope.contains(&id) {
                continue;
            }
            let file = provider.read(&id).await?;
            let found = search
                .search_bytes(&file.bytes)
                .map_err(SourceError::from)?;
            let text = String::from_utf8_lossy(&file.bytes);
            let lines: Vec<_> = text.lines().collect();
            for line in found {
                if query.limit.is_some_and(|limit| matches.len() == limit) {
                    return Ok(SearchResults {
                        matches,
                        complete: false,
                    });
                }
                let start = (line - 1).saturating_sub(query.context_lines);
                let end = line.saturating_add(query.context_lines).min(lines.len());
                matches.push(TextMatch {
                    resource: id.clone(),
                    line,
                    excerpt: lines[start..end].join("\n"),
                });
            }
        }
        Ok(SearchResults {
            matches,
            complete: true,
        })
    }

    // {cite}`habeas:util`
    pub async fn search_structure<P: SourceProvider + ?Sized>(
        provider: &P,
        query: &StructuralQuery,
        languages: &Registry,
    ) -> Result<StructuralMatches, P::Error> {
        let language = languages.named(&query.language).ok_or_else(|| {
            SourceError::Invalid(format!("unknown source language {:?}", query.language))
        })?;
        let rule =
            StructuralRule::compile_with(language, &query.rule).map_err(SourceError::Invalid)?;
        let mut matches = Vec::new();
        for id in provider.list(&query.scope).await? {
            if !query.scope.contains(&id) {
                continue;
            }
            // The provider's override supplies the parse and its source snapshot.
            let parsed = provider.parse(&id, languages).await?;
            if parsed.resource != id {
                return Err(
                    SourceError::Invalid("parse returned a different resource".into()).into(),
                );
            }
            if parsed.outline.profile.name != language.profile().name {
                continue;
            }
            let text = parsed.source.text()?;
            for range in rule.matches(text) {
                if query.limit.is_some_and(|limit| matches.len() == limit) {
                    return Ok(SearchResults {
                        matches,
                        complete: false,
                    });
                }
                let node = &parsed.outline.nodes[innermost_node(&parsed.outline, text, &range)];
                matches.push(StructuralMatch {
                    node: OutlineId {
                        resource: id.clone(),
                        selector: node.selector.clone(),
                    },
                    excerpt: text[range].to_owned(),
                });
            }
        }
        Ok(SearchResults {
            matches,
            complete: true,
        })
    }
}

// {impl}`requirements.sdk.sources.local_provider`
pub struct LocalSources {
    root: PathBuf,
}
impl LocalSources {
    // {cite}`habeas:util`
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn project_directory(&self) -> Result<PathBuf, SourceError> {
        let root = self.root.canonicalize()?;
        if !root.is_dir() {
            return Err(SourceError::Invalid(
                "project root must be a directory".into(),
            ));
        }
        Ok(root)
    }

    fn path(&self, location: &str) -> Result<PathBuf, SourceError> {
        let root = self.project_directory()?;
        let mut relative = PathBuf::new();
        for component in Path::new(location).components() {
            match component {
                Component::Normal(part) => relative.push(part),
                Component::CurDir => {}
                Component::ParentDir if relative.pop() => {}
                _ => {
                    return Err(SourceError::Invalid(
                        "source path must be relative to the project directory".into(),
                    ));
                }
            }
        }
        let path = root.join(relative);
        if !path.canonicalize()?.starts_with(&root) {
            return Err(SourceError::Invalid(
                "source is outside the selected root".into(),
            ));
        }
        Ok(path)
    }

    pub fn list_sync(&self, scope: &SourceScope) -> Result<Vec<ResourceId>, SourceError> {
        let root = self.project_directory()?;
        let selected = self.path(&scope.prefix)?;
        let mut resources = Vec::new();
        let filter_root = root.clone();
        let filter_scope = selected.clone();
        let visibility_error = Arc::new(Mutex::new(None));
        let filter_error = visibility_error.clone();
        let mut walker = ignore::WalkBuilder::new(&root);
        walker
            .hidden(false)
            .parents(false)
            .ignore(false)
            .git_global(false)
            .git_exclude(false)
            .require_git(false)
            .follow_links(true)
            .filter_entry(move |entry| {
                let path = entry.path();
                if !filter_scope.starts_with(path) && !path.starts_with(&filter_scope) {
                    return false;
                }
                if git_metadata(&filter_root, path) {
                    return false;
                }
                match path.canonicalize() {
                    Ok(target) => {
                        if !target.starts_with(&filter_root) {
                            return false;
                        }
                        if target == path {
                            return true;
                        }
                        match project_path_visible(&filter_root, &target) {
                            Ok(visible) => visible,
                            Err(error) => {
                                *filter_error.lock().expect("visibility error lock") = Some(error);
                                false
                            }
                        }
                    }
                    // Let the walker report filesystem errors.
                    Err(_) => true,
                }
            });
        for entry in walker.build() {
            let entry = entry.map_err(|error| SourceError::Invalid(error.to_string()))?;
            if let Some(error) = entry.error() {
                return Err(SourceError::Invalid(error.to_string()));
            }
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            if !entry.path().starts_with(&selected) {
                continue;
            }
            let location = entry
                .path()
                .strip_prefix(&root)
                .map_err(|error| SourceError::Invalid(error.to_string()))?
                .to_str()
                .ok_or_else(|| SourceError::Invalid("source path is not UTF-8".into()))?
                .replace(std::path::MAIN_SEPARATOR, "/");
            resources.push(ResourceId {
                scheme: "repo".into(),
                location,
            });
        }
        if let Some(error) = visibility_error
            .lock()
            .expect("visibility error lock")
            .take()
        {
            return Err(error);
        }
        resources.sort_by(|a, b| a.location.cmp(&b.location));
        Ok(resources)
    }

    pub fn read_sync(&self, id: &ResourceId) -> Result<SourceFile, SourceError> {
        if id.scheme != "repo" {
            return Err(SourceError::Invalid(format!(
                "unsupported resource scheme {:?}",
                id.scheme
            )));
        }
        let path = self.path(&id.location)?;
        let root = self.project_directory()?;
        let target = path.canonicalize()?;
        if !project_path_visible(&root, &path)?
            || (target != path && !project_path_visible(&root, &target)?)
        {
            return Err(SourceError::Invalid(format!(
                "source is excluded from project inspection: {}",
                id.location
            )));
        }
        Ok(SourceFile {
            bytes: std::fs::read(path)?.into(),
            language: None,
        })
    }

    #[cfg(test)]
    pub fn parse_sync(
        &self,
        id: &ResourceId,
        languages: &Registry,
    ) -> Result<ParsedSource, SourceError> {
        parse_snapshot(id, self.read_sync(id)?, languages)
    }
}

fn git_metadata(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root).is_ok_and(|relative| {
        relative
            .components()
            .any(|part| matches!(part, Component::Normal(name) if name == ".git"))
    })
}

/// Apply each directory's rules in precedence order, stopping at ignored
/// directories so a nested exception cannot reopen an excluded parent.
fn project_path_visible(root: &Path, path: &Path) -> Result<bool, SourceError> {
    use ignore::gitignore::GitignoreBuilder;

    if git_metadata(root, path) {
        return Ok(false);
    }
    let relative = path
        .strip_prefix(root)
        .map_err(|error| SourceError::Invalid(error.to_string()))?;
    let mut directory = root.to_path_buf();
    let mut rules = Vec::new();
    for part in relative.components() {
        let ignore_file = directory.join(".gitignore");
        if ignore_file.is_file() {
            let mut builder = GitignoreBuilder::new(&directory);
            if let Some(error) = builder.add(ignore_file) {
                return Err(SourceError::Invalid(error.to_string()));
            }
            rules.push(
                builder
                    .build()
                    .map_err(|error| SourceError::Invalid(error.to_string()))?,
            );
        }
        directory.push(part);
        let matched = rules
            .iter()
            .rev()
            .map(|rule| rule.matched(&directory, directory.is_dir()))
            .find(|matched| !matched.is_none());
        if matched.is_some_and(|matched| matched.is_ignore()) {
            return Ok(false);
        }
    }
    Ok(true)
}
impl SourceProvider for LocalSources {
    type Error = SourceError;
    async fn list(&self, scope: &SourceScope) -> Result<Vec<ResourceId>, Self::Error> {
        self.list_sync(scope)
    }
    async fn read(&self, id: &ResourceId) -> Result<SourceFile, Self::Error> {
        self.read_sync(id)
    }
}

// {cite}`habeas:util`
pub struct SourceAnalysis<P> {
    pub provider: P,
    pub languages: Registry,
}
impl<P: SourceProvider> SourceAnalysis<P> {
    // {cite}`habeas:util`
    pub fn new(provider: P, languages: Registry) -> Self {
        Self {
            provider,
            languages,
        }
    }
    // {cite}`habeas:util`
    // {impl}`requirements.sdk.graph.context-tools.read_source.fn`
    // {impl}`requirements.sdk.graph.context-tools.outline_source.fn`
    pub async fn parse(&self, id: &ResourceId) -> Result<ParsedSource, P::Error> {
        self.provider.parse(id, &self.languages).await
    }
    // {cite}`habeas:util`
    // {impl}`requirements.sdk.graph.context-tools.search_structure.fn`
    pub async fn search_structure(
        &self,
        query: &StructuralQuery,
    ) -> Result<StructuralMatches, P::Error> {
        self.provider.search_structure(query, &self.languages).await
    }
}

#[cfg(test)]
mod tests;

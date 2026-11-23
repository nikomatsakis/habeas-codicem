//! Resource exploration independent of filesystem paths and graph meaning.

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
    path::{Path, PathBuf},
    sync::Arc,
};

pub type LanguageRegistry = Registry;
pub type LanguageId = String;

// {cite}`habeas:util`
#[derive(Clone, Debug, Default)]
pub struct SourceScope {
    pub prefix: String,
}

impl SourceScope {
    // {cite}`habeas:util`
    pub fn contains(&self, id: &ResourceId) -> bool {
        let prefix = self.prefix.trim_end_matches('/').trim_start_matches("./");
        prefix.is_empty()
            || prefix == "."
            || id.location == prefix
            || id.location.starts_with(&format!("{prefix}/"))
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

    fn path(&self, location: &str) -> Result<PathBuf, SourceError> {
        let root = self.root.canonicalize()?;
        let path = root.join(location).canonicalize()?;
        if !path.starts_with(&root) {
            return Err(SourceError::Invalid(
                "source is outside the selected root".into(),
            ));
        }
        Ok(path)
    }

    pub fn list_sync(&self, scope: &SourceScope) -> Result<Vec<ResourceId>, SourceError> {
        let root = self.root.canonicalize()?;
        let selected = self.path(&scope.prefix)?;
        let mut resources = Vec::new();
        for entry in ignore::WalkBuilder::new(selected).hidden(false).build() {
            let entry = entry.map_err(|error| SourceError::Invalid(error.to_string()))?;
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
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
        Ok(SourceFile {
            bytes: std::fs::read(self.path(&id.location)?)?.into(),
            language: None,
        })
    }

    pub fn parse_sync(
        &self,
        id: &ResourceId,
        languages: &Registry,
    ) -> Result<ParsedSource, SourceError> {
        parse_snapshot(id, self.read_sync(id)?, languages)
    }
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

use super::StructuralRule;
use super::{
    SearchHit, SearchResults, SourceError, SourceProject, SourceScope, StructuralQuery, TextQuery,
};
use crate::domain::Domain;
use crate::domain::NodeId;
use crate::outline::{OutlineContent, OutlineData, Span};

impl<D: Domain> SourceProject<D> {
    // {impl}`requirements.sdk.sources.source_nodes`
    pub(crate) fn intern_source(&self, address: &OutlineData) -> Result<NodeId<D>, SourceError> {
        Ok(self
            .domain
            .cache()
            .intern_source(self.domain.source_node(address)?, address))
    }

    // {impl}`requirements.sdk.sources.source_nodes`
    pub(crate) fn address(&self, node: NodeId<D>) -> Option<OutlineData> {
        let cache = self.domain.cache();
        let known = cache
            .source_addresses
            .lock()
            .expect("source address lock poisoned")
            .get(&node)
            .cloned();
        known.or_else(|| self.domain.source_address(cache.node_data(node)?.as_ref()))
    }

    // {impl}`requirements.sdk.graph.validation-context.node_reference`
    pub(crate) fn resolve_outline(
        &self,
        address: &OutlineData,
    ) -> Result<OutlineContent, SourceError> {
        let provider = &self.sources.analysis.provider;
        let full = provider.visible_path(
            address
                .path
                .to_str()
                .ok_or_else(|| SourceError::Invalid("non-UTF-8 source path".into()))?,
        )?;
        let relative = full
            .strip_prefix(provider.project_directory()?)
            .map_err(|error| SourceError::Invalid(error.to_string()))?;
        let item = OutlineContent::from_path(self.domain.cache(), &full)
            .and_then(|root| root.select(&address.selector))
            .map_err(|e| {
                SourceError::Invalid(format!(
                    "{}#{}: {}",
                    address.path.display(),
                    address.selector,
                    e.message
                ))
            })?;
        Ok(item.with_path(relative))
    }

    // {impl}`requirements.sdk.graph.search.occurrence`
    pub async fn search_text(
        &self,
        query: &TextQuery,
    ) -> Result<SearchResults<SearchHit<NodeId<D>>>, SourceError> {
        let regex = regex::RegexBuilder::new(&query.pattern)
            .multi_line(true)
            .build()
            .map_err(|e| SourceError::Invalid(e.to_string()))?;
        self.find_hits(&query.scope, query.limit, None, |text| {
            regex
                .find_iter(text)
                .take(query.limit.unwrap_or(usize::MAX).saturating_add(1))
                .map(|m| Span::from_range(m.range()))
                .collect()
        })
        .await
    }

    // {impl}`requirements.sdk.graph.search.StructuralRule`
    pub async fn search_structure(
        &self,
        query: &StructuralQuery,
    ) -> Result<SearchResults<SearchHit<NodeId<D>>>, SourceError> {
        let lang = query.language.as_str();
        let rule = StructuralRule::compile(lang, &query.rule)?;
        self.find_hits(&query.scope, query.limit, Some(lang), |text| {
            rule.matches(text)
        })
        .await
    }

    // {impl}`requirements.sdk.graph.search.enclosing_item`
    // {impl}`requirements.sdk.graph.search.domain_node`
    async fn find_hits(
        &self,
        scope: &SourceScope,
        limit: Option<usize>,
        language: Option<&str>,
        ranges: impl Fn(&str) -> Vec<Span>,
    ) -> Result<SearchResults<SearchHit<NodeId<D>>>, SourceError> {
        let mut hits = Vec::new();
        for path in self.sources.list(scope).await? {
            if let Some(lang) = language
                && self
                    .sources
                    .analysis
                    .languages
                    .for_path(&path)
                    .map(|p| p.profile().name)
                    != Some(lang)
            {
                continue;
            }
            let absolute = self.sources.analysis.provider.path(
                path.to_str()
                    .ok_or_else(|| SourceError::Invalid("non-UTF-8 source path".into()))?,
            )?;
            let Some(source) =
                OutlineContent::search_text_snapshot(self.domain.cache(), &absolute)?
            else {
                continue;
            };
            let matches = ranges(&source);
            if matches.is_empty() {
                continue;
            }
            let file = OutlineContent::from_path(self.domain.cache(), &absolute)
                .map_err(|error| SourceError::Invalid(error.to_string()))?;
            for span in matches {
                if limit == Some(hits.len()) {
                    return Ok(SearchResults::new(hits, false));
                }
                let item = file.at_offset(span.start).with_path(&path);
                let node = self.intern_source(item.id())?;
                hits.push(SearchHit::new(node, span, item)?);
            }
        }
        Ok(SearchResults::new(hits, true))
    }
}

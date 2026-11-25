use super::{
    SearchHit, SearchResults, SourceError, SourceProject, SourceScope, StructuralQuery, TextQuery,
};
use crate::domain::Domain;
use crate::domain::NodeId;
use crate::outline::{OutlineContent, OutlineData, Span};
use crate::structural::StructuralRule;

impl<D: Domain> SourceProject<D> {
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
                .map(|m| m.range())
                .collect()
        })
        .await
    }

    pub async fn search_structure(
        &self,
        query: &StructuralQuery,
    ) -> Result<SearchResults<SearchHit<NodeId<D>>>, SourceError> {
        let lang = query.language.as_str();
        let rule = StructuralRule::compile(lang, &query.rule).map_err(SourceError::Invalid)?;
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
        ranges: impl Fn(&str) -> Vec<std::ops::Range<usize>>,
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
            for range in matches {
                if limit == Some(hits.len()) {
                    return Ok(SearchResults::new(hits, false));
                }
                let item = file.at_offset(range.start);
                let id = OutlineData {
                    path: path.clone(),
                    selector: item.id().selector.clone(),
                };
                let node = self
                    .domain
                    .cache()
                    .intern_node(self.domain.source_node(&id)?);
                hits.push(SearchHit::new(
                    node,
                    Span::new(range.start, range.end),
                    item,
                )?);
            }
        }
        Ok(SearchResults::new(hits, true))
    }
}

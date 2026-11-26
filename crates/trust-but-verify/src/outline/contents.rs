//! Selectable source contents with retained text and structural fingerprints.
//!
//! [`crate::outline::OutlineContent`] loads a file or supplied snapshot, selects an item, and
//! implements [`crate::domain::NodeContents`]. [`crate::outline::OutlineLang`] supplies language
//! rules; [`crate::outline::languages`] contains the bundled languages and plain-text fallback.

#[path = "languages/mod.rs"]
pub mod languages;
#[path = "selector.rs"]
pub mod selector;
use crate::domain::{NodeContents, ValidationCache};
use crate::graph::Fingerprint;
pub(crate) use crate::outline_engine::LegacyOutline;
pub(crate) use crate::outline_engine::parse_provider;
pub(crate) use crate::outline_engine::parse_with;
pub(crate) use crate::outline_engine::plain;
use selector::parse_selector;
use std::fmt;
use std::{
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};

/// A file and a selectable item within it.
///
/// The empty selector identifies the file root. Constructing an address does
/// not read the file.
// {impl}`requirements.sdk.outlines.address_identity`
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct OutlineData {
    pub path: PathBuf,
    pub selector: String,
}

impl OutlineData {
    pub fn root(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            selector: String::new(),
        }
    }

    pub fn selector(self, selector: String) -> Result<Self, OutlineError> {
        Self::new(self.path, selector)
    }

    /// Construct an address after checking its selector syntax.
    // {impl}`requirements.sdk.outlines.address_identity`
    pub fn new(path: impl Into<PathBuf>, selector: String) -> Result<Self, OutlineError> {
        parse_selector(&selector).map_err(selection_error)?;
        Ok(Self {
            path: path.into(),
            selector,
        })
    }
}

/// UTF-8 byte boundaries in the original file snapshot, with an exclusive end.
// {impl}`requirements.sdk.outlines.Span`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub(crate) fn from_range(range: Range<usize>) -> Self {
        Self {
            start: range.start,
            end: range.end,
        }
    }

    fn range(self) -> Range<usize> {
        self.start..self.end
    }
}

// {impl}`requirements.sdk.outlines.Span`
impl std::ops::Index<Span> for str {
    type Output = str;
    fn index(&self, span: Span) -> &str {
        &self[span.range()]
    }
}

/// Selectable items in a single source snapshot, in source preorder.
// {impl}`requirements.sdk.outlines.items`
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Outline {
    pub items: Vec<OutlineItem>,
    /// The first syntax error reported while recovering this outline.
    pub diagnostic: Option<OutlineError>,
}

impl Outline {
    pub fn new(items: Vec<OutlineItem>) -> Self {
        Self {
            items,
            diagnostic: None,
        }
    }
}

/// A selectable item and its structural digest.
///
/// `span` contains byte offsets into the retained source, including every
/// attachment owned by the item. Its end is exclusive.
// {impl}`requirements.sdk.outlines.items`
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct OutlineItem {
    pub selector: String,
    pub kind: String,
    pub span: Span,
    /// Source to highlight for the selector name.
    pub name_span: Option<Span>,
    pub header_span: Option<Span>,
    pub trailer_span: Option<Span>,
    pub structural_hash: String,
    pub attachments: Vec<OutlineAttachment>,
}

impl OutlineItem {
    // {impl}`requirements.sdk.outlines.attachments.empty`
    pub fn new(selector: String, kind: String, span: Span, structural_hash: String) -> Self {
        Self {
            selector,
            kind,
            span,
            name_span: None,
            header_span: None,
            trailer_span: None,
            structural_hash,
            attachments: Vec::new(),
        }
    }

    /// Set the source highlight for a named item.
    // {impl}`requirements.sdk.outlines.name_span`
    pub fn with_name_span(mut self, span: Span) -> Self {
        self.name_span = Some(span);
        self
    }

    /// Retain the declaration header when formatting an excerpt.
    pub fn with_header_span(mut self, span: Span) -> Self {
        self.header_span = Some(span);
        self
    }

    /// Retain the declaration trailer when formatting an excerpt.
    pub fn with_trailer_span(mut self, span: Span) -> Self {
        self.trailer_span = Some(span);
        self
    }

    /// Set the language-defined attachments in source order.
    pub fn with_attachments(mut self, attachments: Vec<OutlineAttachment>) -> Self {
        self.attachments = attachments;
        self
    }
}

/// A language-defined construct owned by an outline item.
// {impl}`requirements.sdk.outlines.OutlineAttachment`
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct OutlineAttachment {
    pub kind: AttachmentKind,
    pub span: Span,
}

impl OutlineAttachment {
    pub fn new(kind: AttachmentKind, span: Span) -> Self {
        Self { kind, span }
    }
}

/// How the language classifies the attached source.
// {impl}`requirements.sdk.outlines.AttachmentKind`
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AttachmentKind {
    Comment,
    Annotation,
    Other(String),
}

/// Language rules for producing selectable source items.
///
/// Clones share prepared configuration; implementations should not rebuild
/// parsing rules or copy configuration in proportion to its size.
// {impl}`requirements.sdk.outlines.language_provider`
pub trait OutlineLang: Clone + Send + Sync {
    fn name(&self) -> &str;
    fn extensions(&self) -> &[&str];
    fn outline(&self, source: &str) -> Result<Outline, OutlineError>;

    /// The structural-hash rules and version used by this language.
    // {impl}`requirements.sdk.outlines.language_provider.fingerprint_scheme`
    fn fingerprint_scheme(&self) -> &str;

    /// Return source byte ranges matched by an optional structural rule.
    ///
    /// Outline-only languages report an unsupported-language error.
    // {impl}`requirements.sdk.outlines.language_provider.structural_search`
    fn search_structure(
        &self,
        _source: &str,
        _rule: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Vec<Span>, OutlineError> {
        Err(unsupported_search(self.name()))
    }
}

struct Snapshot {
    source: Arc<str>,
    outline: Outline,
    scheme: String,
    redaction: crate::redaction::Redaction,
}

/// Owned contents for one selected source item.
///
/// Selections share the original source and outline, so later file changes do
/// not alter their text or fingerprints.
// {impl}`requirements.sdk.outlines.snapshot`
#[derive(Clone)]
pub struct OutlineContent {
    id: OutlineData,
    snapshot: Arc<Snapshot>,
    selected: usize,
}

impl std::fmt::Debug for OutlineContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutlineContent")
            .field("id", &self.id)
            .field("scheme", &self.snapshot.scheme)
            .finish()
    }
}

impl OutlineContent {
    /// Read and outline a file, selecting its root.
    // {impl}`requirements.sdk.outlines.parser`
    // {impl}`requirements.sdk.outlines.outline_cache`
    // {impl}`requirements.sdk.outlines.outline_cache.retry`
    // {impl}`requirements.sdk.outlines.outline_cache.fresh`
    // {impl}`requirements.sdk.outlines.outline_file`
    // {impl}`requirements.sdk.outlines.outline_file.lifetime`
    pub fn from_path(
        cache: &ValidationCache<impl crate::domain::Domain>,
        path: impl AsRef<Path>,
    ) -> Result<Self, OutlineError> {
        let path = path.as_ref();
        let mut outlines = cache.outlines.lock().expect("outline cache lock poisoned");
        if let Some(outline) = outlines.get(path).and_then(|file| file.outline.as_ref()) {
            return Ok(outline.clone());
        }
        let source = match outlines.get(path) {
            Some(file) => file.source.clone(),
            None => std::fs::read_to_string(path).map_err(read_error)?.into(),
        };
        let outline = Self::parse_source(path, source.clone())?;
        outlines.insert(
            path.to_owned(),
            crate::domain::CachedFile {
                source,
                outline: Some(outline.clone()),
            },
        );
        Ok(outline)
    }

    pub(crate) fn search_text_snapshot(
        cache: &ValidationCache<impl crate::domain::Domain>,
        path: &Path,
    ) -> Result<Option<Arc<str>>, crate::sources::SourceError> {
        let mut files = cache.outlines.lock().expect("outline cache lock poisoned");
        if let Some(file) = files.get(path) {
            return Ok((!file.source.contains('\0')).then(|| file.source.clone()));
        }
        let text = match std::fs::read_to_string(path) {
            Ok(text) if !text.contains('\0') => Arc::<str>::from(text),
            Ok(_) => return Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        files.insert(
            path.to_owned(),
            crate::domain::CachedFile {
                source: text.clone(),
                outline: None,
            },
        );
        Ok(Some(text))
    }

    /// Read the addressed file, then select its item.
    // {impl}`requirements.sdk.outlines.parser`
    pub fn from_id(
        cache: &ValidationCache<impl crate::domain::Domain>,
        id: &OutlineData,
    ) -> Result<Self, OutlineError> {
        Self::from_path(cache, &id.path)?.select(&id.selector)
    }

    /// Outline supplied text without reading the named file.
    // {impl}`requirements.sdk.outlines.parser.supplied_source`
    // {impl}`requirements.sdk.outlines.defaults`
    pub fn from_source(
        cache: &ValidationCache<impl crate::domain::Domain>,
        filename: impl AsRef<Path>,
        source: Arc<str>,
    ) -> Result<Self, OutlineError> {
        let path = filename.as_ref();
        if let Some(outline) = cache
            .outlines
            .lock()
            .expect("outline cache lock poisoned")
            .get(path)
            .filter(|file| file.source == source)
            .and_then(|file| file.outline.clone())
        {
            return Ok(outline);
        }
        Self::parse_source(path, source)
    }

    pub(crate) fn parse_source(
        filename: impl AsRef<Path>,
        source: Arc<str>,
    ) -> Result<Self, OutlineError> {
        let filename = filename.as_ref();
        if let Some(profile) = languages::registry().for_path(filename) {
            return Self::prepare(
                filename,
                source.clone(),
                Outline::from_legacy(parse_provider(profile, &source)?),
                &format!("{}-structural-v1", profile.profile().name),
            );
        }
        Self::parse_with(&languages::PlainText, filename, source)
    }

    /// Read a file using explicit language rules.
    // {impl}`requirements.sdk.outlines.additional_languages`
    pub fn from_path_with<L: OutlineLang>(
        cache: &ValidationCache<impl crate::domain::Domain>,
        language: &L,
        path: impl AsRef<Path>,
    ) -> Result<Self, OutlineError> {
        let path = path.as_ref();
        let cache = &cache.outlines;
        let cached = cache
            .lock()
            .expect("outline cache lock poisoned")
            .get(path)
            .map(|file| file.source.clone());
        if let Some(source) = cached {
            return Self::parse_with(language, path, source);
        }
        let source: Arc<str> = std::fs::read_to_string(path).map_err(read_error)?.into();
        // Custom languages can request another outline. Parse outside the lock.
        let parsed = Self::parse_with(language, path, source.clone())?;
        let retained = cache
            .lock()
            .expect("outline cache lock poisoned")
            .entry(path.to_owned())
            .or_insert_with(|| crate::domain::CachedFile {
                source: source.clone(),
                outline: None,
            })
            .source
            .clone();
        if retained == source {
            Ok(parsed)
        } else {
            Self::parse_with(language, path, retained)
        }
    }

    /// Outline supplied text using explicit language rules.
    // {impl}`requirements.sdk.outlines.additional_languages`
    pub fn from_source_with<L: OutlineLang>(
        cache: &ValidationCache<impl crate::domain::Domain>,
        language: &L,
        filename: impl AsRef<Path>,
        source: Arc<str>,
    ) -> Result<Self, OutlineError> {
        let path = filename.as_ref();
        let source = cache
            .outlines
            .lock()
            .expect("outline cache lock poisoned")
            .get(path)
            .filter(|file| file.source == source)
            .map(|file| file.source.clone())
            .unwrap_or(source);
        Self::parse_with(language, path, source)
    }

    fn parse_with<L: OutlineLang>(
        language: &L,
        filename: impl AsRef<Path>,
        source: Arc<str>,
    ) -> Result<Self, OutlineError> {
        let outline = language.outline(&source)?;
        Self::prepare(
            filename.as_ref(),
            source,
            outline,
            language.fingerprint_scheme(),
        )
    }

    fn prepare(
        filename: &Path,
        source: Arc<str>,
        outline: Outline,
        scheme: &str,
    ) -> Result<Self, OutlineError> {
        validate_outline(&outline, &source)?;
        let redaction = crate::redaction::Redaction::of_text(&source);
        Ok(Self {
            id: OutlineData {
                path: filename.to_path_buf(),
                selector: String::new(),
            },
            snapshot: Arc::new(Snapshot {
                source,
                outline,
                scheme: scheme.to_owned(),
                redaction,
            }),
            selected: 0,
        })
    }

    /// Select another item from this retained file snapshot.
    // {impl}`requirements.sdk.outlines.selection`
    // {impl}`requirements.sdk.outlines.errors`
    pub fn select(&self, selector: &str) -> Result<Self, OutlineError> {
        let selector = parse_selector(selector)
            .map_err(selection_error)?
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("/");
        let matches = self
            .snapshot
            .outline
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.selector == selector)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let selected = *matches
            .first()
            .ok_or_else(|| selection_error(format!("no outline item matches {selector:?}")))?;
        if matches.len() > 1
            && matches.iter().any(|index| {
                let item = &self.snapshot.outline.items[*index];
                item.kind != "line"
                    || item.structural_hash != self.snapshot.outline.items[selected].structural_hash
            })
        {
            return Err(selection_error(format!(
                "ambiguous outline item {selector:?}"
            )));
        }
        Ok(Self {
            id: OutlineData {
                path: self.id.path.clone(),
                selector,
            },
            snapshot: self.snapshot.clone(),
            selected,
        })
    }

    /// Address of the selected item.
    pub fn id(&self) -> &OutlineData {
        &self.id
    }

    /// Selected source text, including attached source.
    pub fn text(&self) -> &str {
        &self.snapshot.source[self.item().span]
    }

    /// Outline of the whole retained file, including when an item is selected.
    // {impl}`requirements.sdk.outlines.items`
    pub fn outline(&self) -> &Outline {
        &self.snapshot.outline
    }

    /// Retained structural digest of the selected item.
    pub fn structural_hash(&self) -> &str {
        &self.item().structural_hash
    }

    /// Retained structural-hash rules and their version.
    // {impl}`requirements.sdk.outlines.language_provider.fingerprint_scheme`
    pub fn fingerprint_scheme(&self) -> &str {
        &self.snapshot.scheme
    }

    pub(crate) fn at_offset(&self, offset: usize) -> Self {
        let selected = self
            .outline()
            .items
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, item)| item.span.start <= offset && offset < item.span.end)
            .min_by_key(|(_, item)| item.span.end - item.span.start)
            .map_or(0, |(i, _)| i);
        let mut content = self.clone();
        content.selected = selected;
        content.id.selector = content.item().selector.clone();
        content
    }

    pub(crate) fn source_span(&self, span: Span) -> Option<&str> {
        self.snapshot.source.get(span.range())
    }

    pub(crate) fn redacted_span(&self, span: Span) -> std::borrow::Cow<'_, str> {
        self.snapshot
            .redaction
            .slice(&self.snapshot.source, span.range())
    }

    pub(crate) fn item(&self) -> &OutlineItem {
        &self.snapshot.outline.items[self.selected]
    }
}

// {impl}`requirements.sdk.outlines.node_contents`
impl NodeContents for OutlineContent {
    fn fingerprint(&self) -> Fingerprint {
        Fingerprint {
            scheme: self.snapshot.scheme.clone(),
            value: self.structural_hash().to_owned(),
        }
    }

    // {impl}`requirements.sdk.sources.redaction`
    fn format(&self) -> String {
        self.snapshot
            .redaction
            .slice(&self.snapshot.source, self.item().span.range())
            .into_owned()
    }
}

impl Outline {
    // {impl}`requirements.sdk.outlines.attachments`
    // {impl}`requirements.sdk.outlines.name_span`
    // {impl}`requirements.sdk.outlines.attachments.order`
    // {impl}`requirements.sdk.outlines.attachments.comments`
    // {impl}`requirements.sdk.outlines.attachments.item_span`
    pub(crate) fn from_legacy(outline: LegacyOutline) -> Self {
        let mut items: Vec<_> = outline
            .nodes
            .into_iter()
            .map(|node| {
                let mut item = OutlineItem::new(
                    node.selector,
                    node.kind.to_owned(),
                    Span::new(node.source_start, node.byte_end),
                    node.structural_hash,
                );
                item.name_span = node.name_span;
                item.header_span = node.header_span;
                item.trailer_span = node.trailer_span;
                item
            })
            .collect();
        for comment in outline.comments {
            items[comment.owner].attachments.push(OutlineAttachment {
                kind: AttachmentKind::Comment,
                span: Span {
                    start: comment.byte_start,
                    end: comment.byte_end,
                },
            });
        }
        for (owner, span) in outline.annotations {
            items[owner].attachments.push(OutlineAttachment {
                kind: AttachmentKind::Annotation,
                span: Span::from_range(span),
            });
        }
        for item in &mut items {
            for attachment in &item.attachments {
                item.span.start = item.span.start.min(attachment.span.start);
                item.span.end = item.span.end.max(attachment.span.end);
            }
            item.attachments
                .sort_by_key(|attachment| attachment.span.start);
        }
        Self {
            items,
            diagnostic: outline.diagnostic,
        }
    }
}

// {impl}`requirements.sdk.outlines.attachments.span`
// {impl}`requirements.sdk.outlines.attachments.item_span`
// {impl}`requirements.sdk.outlines.name_span`
fn validate_outline(outline: &Outline, source: &str) -> Result<(), OutlineError> {
    if outline
        .items
        .first()
        .is_none_or(|root| !root.selector.is_empty() || root.span.range() != (0..source.len()))
    {
        return Err(selection_error(
            "outline must begin with its complete file root",
        ));
    }
    for item in &outline.items {
        if source.get(item.span.range()).is_none() {
            return Err(selection_error("outline item has an invalid source span"));
        }
        if item.attachments.iter().any(|attachment| {
            attachment.span.start < item.span.start
                || attachment.span.end > item.span.end
                || source.get(attachment.span.range()).is_none()
        }) {
            return Err(selection_error("attachment has an invalid source span"));
        }
        if !item
            .attachments
            .is_sorted_by_key(|attachment| attachment.span.start)
        {
            return Err(selection_error("attachments must be in source order"));
        }
        let selector = parse_selector(&item.selector).map_err(selection_error)?;
        let named = selector
            .last()
            .is_some_and(|segment| matches!(segment.label, selector::Label::Named(_)));
        if named != item.name_span.is_some() {
            return Err(selection_error(
                "name span must be present exactly for named outline items",
            ));
        }
        for span in [item.header_span, item.trailer_span].into_iter().flatten() {
            if span.start < item.span.start
                || span.end > item.span.end
                || source.get(span.range()).is_none()
            {
                return Err(selection_error("invalid declaration span"));
            }
        }
        if let Some(name) = item.name_span
            && (name.start < item.span.start
                || name.end > item.span.end
                || source.get(name.range()).is_none())
        {
            return Err(selection_error("outline item has an invalid name span"));
        }
    }
    Ok(())
}

fn selection_error(message: impl std::fmt::Display) -> OutlineError {
    OutlineError {
        line: 1,
        message: message.to_string(),
    }
}

fn read_error(error: std::io::Error) -> OutlineError {
    selection_error(format!("cannot read outline source: {error}"))
}

pub(super) fn unsupported_search(language: &str) -> OutlineError {
    selection_error(format!(
        "structural search is unsupported for outline language {language:?}"
    ))
}

/// A source that its grammar cannot parse.
#[derive(Clone, Debug, PartialEq, Eq)]
// {cite}`habeas:util`
#[non_exhaustive]
pub struct OutlineError {
    pub line: usize,
    pub message: String,
}

impl OutlineError {
    pub fn new(line: usize, message: String) -> Self {
        Self { line, message }
    }
}

impl fmt::Display for OutlineError {
    // {cite}`habeas:util`
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for OutlineError {}

#[cfg(test)]
mod tests {
    use super::*;

    // {test}`requirements.sdk.outlines.name_span`
    #[test]
    fn provider_name_spans_match_selector_kind_and_item_boundaries() {
        let source = "_é_";
        let root = OutlineItem::new(String::new(), String::new(), Span::new(0, 4), "root".into());
        for (selector, name, valid) in [
            ("part[name]", Some(Span::new(1, 3)), true),
            ("part[name]", None, false),
            ("part[name]", Some(Span::new(1, 2)), false),
            ("part[name]", Some(Span::new(3, 1)), false),
            ("part[name]", Some(Span::new(0, 3)), false),
            ("part[name]", Some(Span::new(1, 4)), false),
            ("part@0123456789ab", None, true),
            ("part@0123456789ab", Some(Span::new(1, 3)), false),
        ] {
            let mut item = OutlineItem::new(
                selector.into(),
                "part".into(),
                Span::new(1, 3),
                "item".into(),
            );
            item.name_span = name;
            let outline = Outline::new(vec![root.clone(), item]);
            assert_eq!(
                validate_outline(&outline, source).is_ok(),
                valid,
                "{selector} {name:?}"
            );
        }
    }
}

//! Selectable source contents with retained text and structural fingerprints.
//!
//! [`crate::outline::OutlineContent`] loads a file or supplied snapshot, selects an item, and
//! implements [`crate::domain::NodeContents`]. [`crate::outline::OutlineLang`] supplies language
//! rules; [`crate::outline::languages`] contains the bundled languages and plain-text fallback.

// {impl}`requirements.sdk.public-api.exports`
#[path = "languages/mod.rs"]
pub mod languages;
// {impl}`requirements.sdk.public-api.exports`
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

// {impl}`requirements.sdk.outlines.address_identity`
// {impl}`requirements.sdk.public_api`
/// A file and a selectable item within it.
///
/// The empty selector identifies the file root. Constructing an address does
/// not read the file.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct OutlineData {
    pub path: PathBuf,
    pub selector: String,
}

// {impl}`requirements.sdk.public_api`
impl OutlineData {
    // {impl}`requirements.sdk.outlines.address_identity`
    pub fn root(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            selector: String::new(),
        }
    }

    // {impl}`requirements.sdk.outlines.address_identity`
    pub fn selector(self, selector: String) -> Result<Self, OutlineError> {
        Self::new(self.path, selector)
    }

    /// Construct an address after checking its selector syntax.
    pub fn new(path: impl Into<PathBuf>, selector: String) -> Result<Self, OutlineError> {
        parse_selector(&selector).map_err(selection_error)?;
        Ok(Self {
            path: path.into(),
            selector,
        })
    }
}

// {impl}`requirements.sdk.outlines.Span`
// {impl}`requirements.sdk.public_api`
/// UTF-8 byte boundaries in the original file snapshot, with an exclusive end.
///
/// ```compile_fail,E0639
/// use trust_but_verify::outline::Span;
/// let _ = Span { start: 0, end: 1 };
/// ```
// {test}`requirements.sdk.outlines.Span`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

// {impl}`requirements.sdk.public_api`
impl Span {
    // {impl}`requirements.sdk.outlines.Span`
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

// {impl}`requirements.sdk.outlines.Outline`
// {impl}`requirements.sdk.public_api`
/// Selectable items in a single source snapshot, in source preorder.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Outline {
    pub items: Vec<OutlineItem>,
    /// The first syntax error reported while recovering this outline.
    pub diagnostic: Option<OutlineError>,
}

// {impl}`requirements.sdk.public_api`
impl Outline {
    // {impl}`requirements.sdk.outlines.Outline`
    pub fn new(items: Vec<OutlineItem>) -> Self {
        Self {
            items,
            diagnostic: None,
        }
    }

    // {impl}`requirements.sdk.outlines.Outline`
    pub fn with_diagnostic(mut self, diagnostic: OutlineError) -> Self {
        self.diagnostic = Some(diagnostic);
        self
    }
}

// {impl}`requirements.sdk.graph.search.declaration_spans`
// {impl}`requirements.sdk.outlines.OutlineItem`
// {impl}`requirements.sdk.outlines.attachments`
// {impl}`requirements.sdk.public_api`
/// A selectable item and its structural digest.
///
/// `span` contains byte offsets into the retained source, including every
/// attachment owned by the item. Its end is exclusive.
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
    own_hash: Option<String>,
    pub attachments: Vec<OutlineAttachment>,
}

// {impl}`requirements.sdk.public_api`
// {impl}`requirements.sdk.outlines.OutlineItem`
impl OutlineItem {
    // {impl}`requirements.sdk.outlines.name_span`
    pub fn new(selector: String, kind: String, span: Span, structural_hash: String) -> Self {
        Self {
            selector,
            kind,
            span,
            name_span: None,
            header_span: None,
            trailer_span: None,
            structural_hash,
            own_hash: None,
            attachments: Vec::new(),
        }
    }

    /// Set the fingerprint digest for this item with its outline descendants excluded.
    pub fn with_own_hash(mut self, hash: String) -> Self {
        self.own_hash = Some(hash);
        self
    }

    // {impl}`requirements.sdk.outlines.name_span`
    /// Set the source highlight for a named item.
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

// {impl}`requirements.sdk.outlines.OutlineAttachment`
// {impl}`requirements.sdk.public_api`
/// A language-defined construct owned by an outline item.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct OutlineAttachment {
    pub kind: AttachmentKind,
    pub span: Span,
}

// {impl}`requirements.sdk.public_api`
impl OutlineAttachment {
    // {impl}`requirements.sdk.outlines.OutlineAttachment`
    pub fn new(kind: AttachmentKind, span: Span) -> Self {
        Self { kind, span }
    }
}

// {impl}`requirements.sdk.outlines.AttachmentKind`
/// How the language classifies the attached source.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AttachmentKind {
    Comment,
    Annotation,
    Other(String),
}

// {impl}`requirements.sdk.outlines.language_provider`
/// Language rules for producing selectable source items.
///
/// Clones share prepared configuration; implementations should not rebuild
/// parsing rules or copy configuration in proportion to its size.
pub trait OutlineLang: Clone + Send + Sync {
    fn name(&self) -> &str;
    fn extensions(&self) -> &[&str];
    fn outline(&self, source: &str) -> Result<Outline, OutlineError>;

    // {impl}`requirements.sdk.outlines.language_provider.fingerprint_scheme`
    /// The structural-hash rules and version used by this language.
    fn fingerprint_scheme(&self) -> &str;

    // {impl}`requirements.sdk.outlines.language_provider.structural_search`
    /// Return source byte ranges matched by an optional structural rule.
    ///
    /// Outline-only languages report an unsupported-language error.
    fn search_structure(
        &self,
        _source: &str,
        _rule: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Vec<Span>, OutlineError> {
        Err(unsupported_search(self.name()))
    }
}

// {impl}`requirements.sdk.outlines.language_provider.fingerprint_scheme`
struct Snapshot {
    source: Arc<str>,
    outline: Outline,
    children: Vec<Vec<usize>>,
    language: String,
    scheme: String,
    redaction: crate::redaction::Redaction,
}

// {impl}`requirements.sdk.outlines.snapshot`
// {impl}`requirements.sdk.outlines.outline_file.lifetime`
// {impl}`requirements.sdk.outlines.parser`
/// Owned contents for one selected source item.
///
/// Selections share the original source and outline, so later file changes do
/// not alter their text or fingerprints.
#[derive(Clone)]
pub struct OutlineContent {
    id: OutlineData,
    snapshot: Arc<Snapshot>,
    selected: usize,
}

// {impl}`requirements.sdk.outlines.parser`
impl std::fmt::Debug for OutlineContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutlineContent")
            .field("id", &self.id)
            .field("scheme", &self.snapshot.scheme)
            .finish()
    }
}

// {impl}`requirements.sdk.outlines.parser`
impl OutlineContent {
    // {impl}`requirements.sdk.outlines.outline_file`
    /// Read and outline a file, selecting its root.
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
    pub fn from_id(
        cache: &ValidationCache<impl crate::domain::Domain>,
        id: &OutlineData,
    ) -> Result<Self, OutlineError> {
        Self::from_path(cache, &id.path)?.select(&id.selector)
    }

    // {impl}`requirements.sdk.outlines.parser.supplied_source`
    /// Outline supplied text without reading the named file.
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

    // {impl}`requirements.sdk.sources.plain-text.language`
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
                profile.profile().name,
                profile.profile().fingerprint_scheme,
            );
        }
        Self::parse_with(&languages::PlainText, filename, source)
    }

    // {impl}`requirements.sdk.outlines.additional_languages`
    /// Read a file using explicit language rules.
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

    // {impl}`requirements.sdk.outlines.additional_languages`
    /// Outline supplied text using explicit language rules.
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
            language.name(),
            language.fingerprint_scheme(),
        )
    }

    // {impl}`requirements.sdk.outlines.snapshot`
    fn prepare(
        filename: &Path,
        source: Arc<str>,
        outline: Outline,
        language: &str,
        scheme: &str,
    ) -> Result<Self, OutlineError> {
        validate_outline(&outline, &source)?;
        let redaction = crate::redaction::Redaction::of_text(&source);
        let mut children = vec![Vec::new(); outline.items.len()];
        let mut ancestors = vec![0usize];
        for (index, item) in outline.items.iter().enumerate().skip(1) {
            while ancestors.len() > 1
                && !item
                    .selector
                    .strip_prefix(&outline.items[*ancestors.last().unwrap()].selector)
                    .is_some_and(|suffix| suffix.starts_with('/'))
            {
                ancestors.pop();
            }
            let parent = *ancestors.last().unwrap();
            children[parent].push(index);
            ancestors.push(index);
        }
        Ok(Self {
            id: OutlineData {
                path: filename.to_path_buf(),
                selector: String::new(),
            },
            snapshot: Arc::new(Snapshot {
                source,
                outline,
                children,
                language: language.to_owned(),
                scheme: scheme.to_owned(),
                redaction,
            }),
            selected: 0,
        })
    }

    // {impl}`requirements.sdk.outlines.selection`
    /// Select another item from this retained file snapshot.
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

    // {impl}`requirements.sdk.outlines.selection`
    /// Address of the selected item.
    pub fn id(&self) -> &OutlineData {
        &self.id
    }

    // {impl}`requirements.sdk.outlines.selection`
    /// Selected source text, including attached source.
    pub fn text(&self) -> &str {
        &self.snapshot.source[self.item().span]
    }

    // {impl}`requirements.sdk.outlines.items`
    /// Outline of the whole retained file, including when an item is selected.
    pub fn outline(&self) -> &Outline {
        &self.snapshot.outline
    }

    /// Name of the language that produced this file snapshot.
    pub fn language(&self) -> &str {
        &self.snapshot.language
    }

    /// Retained structural digest of the selected item.
    pub fn structural_hash(&self) -> &str {
        &self.item().structural_hash
    }

    // {impl}`requirements.sdk.outlines.language_provider.fingerprint_scheme`
    /// Retained structural-hash rules and their version.
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

    /// Original text at file-relative UTF-8 byte boundaries in the retained snapshot.
    pub fn source_span(&self, span: Span) -> Option<&str> {
        self.snapshot.source.get(span.range())
    }

    /// Text at the same file-relative boundaries, with whole-file credential redaction.
    pub fn source_span_redacted(&self, span: Span) -> Option<std::borrow::Cow<'_, str>> {
        self.source_span(span)?;
        Some(
            self.snapshot
                .redaction
                .slice(&self.snapshot.source, span.range()),
        )
    }

    pub(crate) fn with_path(mut self, path: &Path) -> Self {
        self.id.path = path.to_owned();
        self
    }

    pub(crate) fn children(&self) -> impl Iterator<Item = &OutlineItem> {
        self.snapshot.children[self.selected]
            .iter()
            .map(|&index| &self.snapshot.outline.items[index])
    }

    /// Nodes whose own source contributed bytes to a displayed search excerpt.
    pub(crate) fn excerpt_items(&self, spans: &[Span]) -> Vec<Self> {
        let intersects = |start, end| {
            spans
                .iter()
                .any(|s| start < end && s.start < end && start < s.end)
        };
        let bounds = self.item().span;
        let root = if spans
            .iter()
            .all(|s| bounds.start <= s.start && s.end <= bounds.end)
        {
            self.selected
        } else {
            0
        };
        let mut pending = vec![root];
        let mut items = Vec::new();
        while let Some(index) = pending.pop() {
            let item = &self.snapshot.outline.items[index];
            if !intersects(item.span.start, item.span.end) {
                continue;
            }
            let mut cursor = item.span.start;
            let mut visible = false;
            for &child in &self.snapshot.children[index] {
                let child_span = self.snapshot.outline.items[child].span;
                visible |= intersects(cursor, child_span.start);
                cursor = cursor.max(child_span.end);
                if intersects(child_span.start, child_span.end) {
                    pending.push(child);
                }
            }
            if index != self.selected && (visible || intersects(cursor, item.span.end)) {
                let mut content = self.clone();
                content.selected = index;
                content.id.selector = item.selector.clone();
                items.push(content);
            }
        }
        items
    }

    pub(crate) fn item(&self) -> &OutlineItem {
        &self.snapshot.outline.items[self.selected]
    }
}

// {impl}`requirements.sdk.outlines.node_contents`
// {impl}`requirements.sdk.outlines.parser`
impl NodeContents for OutlineContent {
    fn fingerprint(&self) -> Fingerprint {
        match &self.item().own_hash {
            Some(digest) => Fingerprint::new(
                format!("{}:own-v1", self.fingerprint_scheme()),
                digest.clone(),
            ),
            None => Fingerprint::new(
                self.fingerprint_scheme().into(),
                self.structural_hash().into(),
            ),
        }
    }

    fn format(&self) -> String {
        let bounds = self.item().span;
        let mut cursor = bounds.start;
        let mut text = String::new();
        for child in self.children() {
            let start = child.span.start.clamp(cursor, bounds.end);
            if start > cursor {
                text.push_str(
                    &self
                        .snapshot
                        .redaction
                        .slice(&self.snapshot.source, cursor..start),
                );
            }
            let end = child.span.end.clamp(start, bounds.end);
            if end > start {
                text.push('…');
            }
            cursor = end;
        }
        text.push_str(
            &self
                .snapshot
                .redaction
                .slice(&self.snapshot.source, cursor..bounds.end),
        );
        text
    }
}

impl Outline {
    // {impl}`requirements.sdk.outlines.attachments.comments`
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
                item.own_hash = Some(node.identity_hash);
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

// {impl}`requirements.sdk.outlines.OutlineError`
// {impl}`requirements.sdk.public_api`
/// A source that its grammar cannot parse.
///
/// ```compile_fail,E0639
/// use trust_but_verify::outline::OutlineError;
/// let _ = OutlineError { line: 1, message: String::new() };
/// ```
// {test}`requirements.sdk.outlines.OutlineError`
#[derive(Clone, Debug, PartialEq, Eq)]
// {cite}`habeas:util`
#[non_exhaustive]
pub struct OutlineError {
    pub line: usize,
    pub message: String,
}

// {impl}`requirements.sdk.public_api`
impl OutlineError {
    // {impl}`requirements.sdk.outlines.OutlineError`
    pub fn new(line: usize, message: String) -> Self {
        Self { line, message }
    }
}

// {impl}`requirements.sdk.outlines.OutlineError`
impl fmt::Display for OutlineError {
    // {cite}`habeas:util`
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "line {}: {}", self.line, self.message)
    }
}

// {impl}`requirements.sdk.outlines.OutlineError`
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

use super::{OutlineError, parse_selector};
use crate::{Fingerprint, NodeContents};
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
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OutlineId {
    pub path: PathBuf,
    pub selector: String,
}

impl OutlineId {
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

/// Selectable items in a single source snapshot, in source preorder.
// {impl}`requirements.sdk.outlines.items`
#[derive(Clone, Debug)]
pub struct Outline {
    pub items: Vec<OutlineItem>,
}

/// A selectable item and its structural digest.
///
/// `span` contains byte offsets into the retained source, including attached
/// source. Its end is exclusive.
// {impl}`requirements.sdk.outlines.items`
#[derive(Clone, Debug)]
pub struct OutlineItem {
    pub selector: String,
    pub kind: String,
    pub span: Range<usize>,
    pub structural_hash: String,
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
    ) -> Result<Vec<Range<usize>>, OutlineError> {
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
    id: OutlineId,
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
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, OutlineError> {
        let path = path.as_ref();
        let source = std::fs::read_to_string(path).map_err(read_error)?;
        Self::from_source(path, source.into())
    }

    /// Read the addressed file, then select its item.
    // {impl}`requirements.sdk.outlines.parser`
    pub fn from_id(id: &OutlineId) -> Result<Self, OutlineError> {
        Self::from_path(&id.path)?.select(&id.selector)
    }

    /// Outline supplied text without reading the named file.
    // {impl}`requirements.sdk.outlines.parser.supplied_source`
    // {impl}`requirements.sdk.outlines.defaults`
    pub fn from_source(filename: impl AsRef<Path>, source: Arc<str>) -> Result<Self, OutlineError> {
        let filename = filename.as_ref();
        #[cfg(feature = "default-languages")]
        {
            if let Some(profile) = super::languages::registry().for_path(filename) {
                return Self::prepare(
                    filename,
                    source.clone(),
                    Outline::from_legacy(&super::parse_provider(profile, &source)?),
                    &format!("{}-structural-v1", profile.profile().name),
                );
            }
        }
        Self::from_source_with(&super::languages::PlainText, filename, source)
    }

    /// Read a file using explicit language rules.
    // {impl}`requirements.sdk.outlines.additional_languages`
    pub fn from_path_with<L: OutlineLang>(
        language: &L,
        path: impl AsRef<Path>,
    ) -> Result<Self, OutlineError> {
        let path = path.as_ref();
        let source = std::fs::read_to_string(path).map_err(read_error)?;
        Self::from_source_with(language, path, source.into())
    }

    /// Outline supplied text using explicit language rules.
    // {impl}`requirements.sdk.outlines.additional_languages`
    pub fn from_source_with<L: OutlineLang>(
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
            id: OutlineId {
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
            id: OutlineId {
                path: self.id.path.clone(),
                selector,
            },
            snapshot: self.snapshot.clone(),
            selected,
        })
    }

    /// Address of the selected item.
    pub fn id(&self) -> &OutlineId {
        &self.id
    }

    /// Selected source text, including attached source.
    pub fn text(&self) -> &str {
        &self.snapshot.source[self.item().span.clone()]
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

    fn item(&self) -> &OutlineItem {
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
            .slice(&self.snapshot.source, self.item().span.clone())
            .into_owned()
    }
}

impl Outline {
    pub(crate) fn from_legacy(outline: &super::LegacyOutline) -> Self {
        Self {
            items: outline
                .nodes
                .iter()
                .map(|node| OutlineItem {
                    selector: node.selector.clone(),
                    kind: node.kind.to_owned(),
                    span: node.source_start..node.byte_end,
                    structural_hash: node.structural_hash.clone(),
                })
                .collect(),
        }
    }
}

fn validate_outline(outline: &Outline, source: &str) -> Result<(), OutlineError> {
    if outline
        .items
        .first()
        .is_none_or(|root| !root.selector.is_empty() || root.span != (0..source.len()))
    {
        return Err(selection_error(
            "outline must begin with its complete file root",
        ));
    }
    for item in &outline.items {
        if source.get(item.span.clone()).is_none() {
            return Err(selection_error("outline item has an invalid source span"));
        }
        parse_selector(&item.selector).map_err(selection_error)?;
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

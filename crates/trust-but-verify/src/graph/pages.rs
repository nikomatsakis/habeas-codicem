use super::Fingerprint;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_CONTEXT: AtomicU64 = AtomicU64::new(1);
const PAGE_CHARACTERS: usize = 8_192;

/// An opaque continuation for one inspection context's retained result.
// {impl}`requirements.sdk.graph.context-tools.pages.behavior`
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PageId {
    context: u64,
    index: usize,
}

/// The first part of a retained result and its later continuations.
///
/// The page IDs are ordered after `text`; reading them reconstructs the result.
#[derive(Clone, Debug)]
pub struct PagedText {
    pub text: String,
    pub pages: Vec<PageId>,
}

/// A continuation that is unavailable in this inspection context.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PageError {
    UnknownPage,
    WrongContext,
}

impl std::fmt::Display for PageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownPage => f.write_str("unknown page ID"),
            Self::WrongContext => f.write_str("page ID belongs to another inspection context"),
        }
    }
}

impl std::error::Error for PageError {}

pub(super) struct Page<N> {
    pub text: String,
    pub node: N,
    pub fingerprint: Fingerprint,
}

pub(super) struct Pages<N> {
    context: u64,
    entries: Vec<Page<N>>,
}

impl<N: Clone> Pages<N> {
    pub fn new() -> Self {
        Self {
            context: NEXT_CONTEXT.fetch_add(1, Ordering::Relaxed),
            entries: Vec::new(),
        }
    }

    pub fn prepare(&mut self, text: String, node: N, fingerprint: Fingerprint) -> PagedText {
        let characters = text.chars().collect::<Vec<_>>();
        let mut chunks = characters.chunks(PAGE_CHARACTERS);
        let text = chunks
            .next()
            .map_or_else(String::new, |chunk| chunk.iter().collect());
        let mut pages = Vec::new();
        for chunk in chunks {
            pages.push(PageId {
                context: self.context,
                index: self.entries.len(),
            });
            self.entries.push(Page {
                text: chunk.iter().collect(),
                node: node.clone(),
                fingerprint: fingerprint.clone(),
            });
        }
        PagedText { text, pages }
    }

    pub fn read(&self, page: &PageId) -> Result<Page<N>, PageError> {
        if page.context != self.context {
            return Err(PageError::WrongContext);
        }
        let page = self.entries.get(page.index).ok_or(PageError::UnknownPage)?;
        Ok(Page {
            text: page.text.clone(),
            node: page.node.clone(),
            fingerprint: page.fingerprint.clone(),
        })
    }
}

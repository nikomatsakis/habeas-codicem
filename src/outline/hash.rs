//! Structural and identity hashes over an unambiguous token encoding.

use sha2::{Digest, Sha256};

/// Feeds a length-prefixed, tagged encoding of grammar node kinds, node ends,
/// and the words of text pieces into SHA-256. Whitespace is never encoded.
// {impl}`requirements.references.structural_hash`
pub(crate) struct Encoder {
    hasher: Sha256,
    /// Group of the prose run currently open, if any: consecutive prose words
    /// of one group form a single run, however they were split into tokens.
    prose: Option<&'static str>,
}

impl Encoder {
    // {impl}`requirements.references.structural_hash`
    pub(crate) fn new() -> Self {
        Self {
            hasher: Sha256::new(),
            prose: None,
        }
    }

    /// Ends the open prose run, if any.
    // {impl}`requirements.references.structural_hash`
    fn close_prose(&mut self) {
        if self.prose.take().is_some() {
            self.hasher.update(b"E");
        }
    }

    // {impl}`requirements.references.structural_hash`
    fn field(&mut self, tag: u8, bytes: &[u8]) {
        self.hasher.update([tag]);
        self.hasher.update((bytes.len() as u64).to_le_bytes());
        self.hasher.update(bytes);
    }

    /// Starts a named grammar node.
    // {impl}`requirements.references.structural_hash`
    pub(crate) fn kind(&mut self, kind: &str) {
        self.close_prose();
        self.field(b'K', kind.as_bytes());
    }

    /// Ends a named grammar node, so nesting is unambiguous.
    // {impl}`requirements.references.structural_hash`
    pub(crate) fn end(&mut self) {
        self.close_prose();
        self.hasher.update(b"E");
    }

    /// Adds one text piece, a leaf token's text or text no child token
    /// covers, as its ordered list of whitespace-separated words, each
    /// encoded separately. A piece that is only whitespace adds nothing.
    // {impl}`requirements.references.structural_hash`
    pub(crate) fn words(&mut self, text: &str) {
        let mut words = text.split_whitespace().peekable();
        if words.peek().is_some() {
            self.close_prose();
        }
        for word in words {
            self.field(b'T', word.as_bytes());
        }
    }

    /// Adds the words of one piece of hashed comment prose to the run of
    /// `group`, opening that run unless it is already open. Each word is
    /// encoded separately and whitespace never is, so splitting the same
    /// words across more or fewer comments does not change the digest.
    // {impl}`requirements.references.structural_hash`
    pub(crate) fn prose(&mut self, group: &'static str, text: &str) {
        if self.prose != Some(group) {
            self.close_prose();
            self.field(b'P', group.as_bytes());
            self.prose = Some(group);
        }
        for word in text.split_whitespace() {
            self.field(b'W', word.as_bytes());
        }
    }

    /// Returns the digest as lowercase hex.
    // {impl}`requirements.references.structural_hash`
    pub(crate) fn finish(mut self) -> String {
        self.close_prose();
        hex(&self.hasher.finalize())
    }
}

/// The hash of a `line` node: the word rule over the line's text.
// {impl}`requirements.references.structural_hash`
pub(crate) fn line(text: &str) -> String {
    let mut encoder = Encoder::new();
    encoder.words(text);
    encoder.finish()
}

// {impl}`requirements.references.structural_hash`
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

//! Structural and identity hashes over an unambiguous token encoding.

use sha2::{Digest, Sha256};

/// Feeds a length-prefixed, tagged encoding of grammar node kinds, leaf token
/// text, and node ends into SHA-256.
// {impl}`requirements.references.structural_hash`
pub(crate) struct Encoder {
    hasher: Sha256,
}

impl Encoder {
    // {impl}`requirements.references.structural_hash`
    pub(crate) fn new() -> Self {
        Self {
            hasher: Sha256::new(),
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
        self.field(b'K', kind.as_bytes());
    }

    /// Ends a named grammar node, so nesting is unambiguous.
    // {impl}`requirements.references.structural_hash`
    pub(crate) fn end(&mut self) {
        self.hasher.update(b"E");
    }

    /// Adds one leaf token's text.
    // {impl}`requirements.references.structural_hash`
    pub(crate) fn text(&mut self, text: &str) {
        self.field(b'T', text.as_bytes());
    }

    /// Returns the digest as lowercase hex.
    // {impl}`requirements.references.structural_hash`
    pub(crate) fn finish(self) -> String {
        hex(&self.hasher.finalize())
    }
}

/// The SHA-256 digest of text, as lowercase hex.
// {impl}`requirements.references.structural_hash`
pub(crate) fn digest(text: &str) -> String {
    hex(&Sha256::digest(text.as_bytes()))
}

// {impl}`requirements.references.structural_hash`
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

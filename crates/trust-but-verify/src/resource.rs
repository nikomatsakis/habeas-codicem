//! Language-independent logical addresses. Resolution belongs to the consumer.
//!
//! :::{cite} habeas:coverage
//! This private compatibility module only declares the legacy resource and
//! selector address shapes; it has no handwritten executable code. Its derived
//! serialization and identity operations do not emit source-line measurements.
//! The provider tests in `sources/tests.rs` exercise resource identities.
//! `tests/outline_content.rs` covers the public path-based item addresses.
//! :::

/// A resource namespace and an opaque location within it. For example, `repo`
/// plus `src/lib.rs`, `https` plus `//example.org/spec`, or `generated` plus a
/// generator-specific key. This is a logical identity, not a content version.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ResourceId {
    pub scheme: String,
    pub location: String,
}

/// The shared selector syntax works independently of the resource's origin or
/// language. Empty `selector` identifies the resource root.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg(test)]
pub struct OutlineId {
    pub resource: ResourceId,
    pub selector: String,
}

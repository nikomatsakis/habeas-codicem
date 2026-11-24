//! Language-independent logical addresses. Resolution belongs to the consumer.
//!
//! :::{cite} habeas:coverage
//! This private compatibility module only declares the legacy resource and
//! selector address shapes; it has no handwritten executable code. Its derived
//! serialization and identity operations do not emit source-line measurements.
//! The source-tool argument and delivery regressions in `agent/tests.rs`
//! exercise these addresses through the legacy adapter. Supported path-based
//! address construction is tested separately in `tests/outline_content.rs`.
//! :::

/// A resource namespace and an opaque location within it. For example, `repo`
/// plus `src/lib.rs`, `https` plus `//example.org/spec`, or `generated` plus a
/// generator-specific key. This is a logical identity, not a content version.
// {impl}`requirements.sdk.sources.source_addresses`
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ResourceId {
    pub scheme: String,
    pub location: String,
}

/// The shared selector syntax works independently of the resource's origin or
/// language. Empty `selector` identifies the resource root.
// {impl}`requirements.sdk.sources.source_addresses`
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg(test)]
pub struct OutlineId {
    pub resource: ResourceId,
    pub selector: String,
}

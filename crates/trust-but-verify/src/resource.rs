//! Language-independent logical addresses. Resolution belongs to the consumer.

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
pub struct OutlineId {
    pub resource: ResourceId,
    pub selector: String,
}

impl OutlineId {
    // {cite}`habeas:util`
    pub fn new(
        resource: ResourceId,
        selector: String,
    ) -> Result<Self, crate::outline::SelectorError> {
        crate::outline::parse_selector(&selector)?;
        Ok(Self { resource, selector })
    }
}

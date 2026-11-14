//! Language-independent logical addresses. Resolution belongs to the consumer.

/// A resource namespace and an opaque location within it. For example, `repo`
/// plus `src/lib.rs`, `https` plus `//example.org/spec`, or `generated` plus a
/// generator-specific key. This is a logical identity, not a content version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceId {
    pub scheme: String,
    pub location: String,
}

/// The shared selector syntax works independently of the resource's origin or
/// language. Empty `selector` identifies the resource root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutlineId {
    pub resource: ResourceId,
    pub selector: String,
}

impl OutlineId {
    pub fn new(
        resource: ResourceId,
        selector: String,
    ) -> Result<Self, crate::outline::SelectorError> {
        crate::outline::parse_selector(&selector)?;
        Ok(Self { resource, selector })
    }
}

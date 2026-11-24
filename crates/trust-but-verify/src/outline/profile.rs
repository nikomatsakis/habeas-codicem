//! Shared profile vocabulary and optional default-language conveniences.
pub(crate) use crate::outline_engine::language::*;

#[cfg(feature = "default-languages")]
pub(crate) use crate::outline_engine::languages::profiles;

#[cfg(feature = "default-languages")]
// {cite}`habeas:util`
pub fn profile_for_path(path: &std::path::Path) -> &'static Profile {
    crate::outline_engine::languages::registry()
        .for_path(path)
        .map(|provider| provider.profile())
        .unwrap_or(&super::plain::PROFILE)
}

#[cfg(feature = "default-languages")]
// {cite}`habeas:util`
pub fn is_markdown_path(path: &std::path::Path) -> bool {
    profile_for_path(path).name == "markdown"
}

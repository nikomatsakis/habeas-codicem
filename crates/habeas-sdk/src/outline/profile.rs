//! Shared profile vocabulary and optional default-language conveniences.
pub use habeas_language::*;

#[cfg(feature = "default-languages")]
pub use habeas_languages::profiles;

#[cfg(feature = "default-languages")]
pub fn profile_for_path(path: &std::path::Path) -> &'static Profile {
    habeas_languages::registry()
        .for_path(path)
        .map(|provider| provider.profile())
        .unwrap_or(&super::plain::PROFILE)
}

#[cfg(feature = "default-languages")]
pub fn is_markdown_path(path: &std::path::Path) -> bool {
    profile_for_path(path).name == "markdown"
}

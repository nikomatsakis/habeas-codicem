//! Shared profile vocabulary and bundled language lookup.
pub(crate) use crate::outline_engine::language::*;

pub(crate) use crate::outline_engine::languages::profiles;

// {cite}`habeas:util`
pub fn profile_for_path(path: &std::path::Path) -> &'static Profile {
    crate::outline_engine::languages::registry()
        .for_path(path)
        .map(|provider| provider.profile())
        .unwrap_or(&super::plain::PROFILE)
}

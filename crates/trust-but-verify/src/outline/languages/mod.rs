//! The default language collection. Applications may register their own set.

pub mod css;
pub mod markdown;
pub mod python;
pub mod rust;
pub mod toml;
pub mod typescript;
pub mod yaml;

// {cite}`habeas:util`
pub fn registry() -> crate::outline::language::Registry {
    crate::outline::language::Registry::new([
        &rust::Provider as &dyn crate::outline::language::LanguageProvider,
        &python::Provider,
        &typescript::Provider,
        &typescript::TsxProvider,
        &css::Provider,
        &markdown::Provider,
        &toml::Provider,
        &yaml::Provider,
    ])
}

// {cite}`habeas:util`
pub fn profiles() -> [&'static crate::outline::language::Profile; 8] {
    [
        &rust::PROFILE,
        &python::PROFILE,
        &typescript::PROFILE,
        &typescript::TSX_PROFILE,
        &css::PROFILE,
        &markdown::PROFILE,
        &toml::PROFILE,
        &yaml::PROFILE,
    ]
}

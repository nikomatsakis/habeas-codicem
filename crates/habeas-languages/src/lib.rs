//! The default language collection. Applications may register their own set.

pub use habeas_language_css as css;
pub use habeas_language_markdown as markdown;
pub use habeas_language_python as python;
pub use habeas_language_rust as rust;
pub use habeas_language_toml as toml;
pub use habeas_language_typescript as typescript;
pub use habeas_language_yaml as yaml;

pub fn registry() -> habeas_language::Registry {
    habeas_language::Registry::new([
        &rust::Provider as &dyn habeas_language::LanguageProvider,
        &python::Provider,
        &typescript::Provider,
        &typescript::TsxProvider,
        &css::Provider,
        &markdown::Provider,
        &toml::Provider,
        &yaml::Provider,
    ])
}

pub fn profiles() -> [&'static habeas_language::Profile; 8] {
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

//! Bundled, cheaply cloneable outline languages.
//!
//! The filename-based constructors select these languages by extension.

pub(crate) mod css;
pub(crate) mod markdown;
pub(crate) mod python;
pub(crate) mod rust;
pub(crate) mod toml;
pub(crate) mod typescript;
pub(crate) mod yaml;

// {cite}`habeas:util`
pub(crate) fn registry() -> crate::outline_engine::language::Registry {
    crate::outline_engine::language::Registry::new([
        &rust::Provider as &dyn crate::outline_engine::language::LanguageProvider,
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
pub(crate) fn profiles() -> [&'static crate::outline_engine::language::Profile; 8] {
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

macro_rules! language {
    ($name:ident, $profile:path, $scheme:literal) => {
        /// Bundled, cheaply cloneable outline language.
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;

        impl super::OutlineLang for $name {
            fn name(&self) -> &str {
                $profile.name
            }

            fn extensions(&self) -> &[&str] {
                $profile.extensions
            }

            fn outline(&self, source: &str) -> Result<super::Outline, super::OutlineError> {
                super::parse_with(&$profile, source).map(super::Outline::from_legacy)
            }

            fn fingerprint_scheme(&self) -> &str {
                $scheme
            }

            fn search_structure(
                &self,
                source: &str,
                rule: &serde_json::Map<String, serde_json::Value>,
            ) -> Result<Vec<std::ops::Range<usize>>, super::OutlineError> {
                if $profile.language.is_some() {
                    super::parse_with(&$profile, source)?;
                    let compiled = crate::structural::StructuralRule::compile($profile.name, rule)
                        .map_err(|message| super::OutlineError { line: 1, message })?;
                    return Ok(compiled.matches(source));
                }
                Err(super::unsupported_search(self.name()))
            }
        }
    };
}

// {impl}`requirements.sdk.sources.plain-text.language`
language!(PlainText, super::plain::PROFILE, "plain-text-structural-v1");
// {impl}`requirements.sdk.sources.rust.language`
language!(Rust, rust::PROFILE, "rust-structural-v1");
// {impl}`requirements.sdk.sources.python.language`
language!(Python, python::PROFILE, "python-structural-v1");
// {impl}`requirements.sdk.sources.typescript.language`
language!(TypeScript, typescript::PROFILE, "typescript-structural-v1");
// {impl}`requirements.sdk.sources.typescript.language`
language!(Tsx, typescript::TSX_PROFILE, "tsx-structural-v1");
// {impl}`requirements.sdk.sources.markdown.language`
language!(Markdown, markdown::PROFILE, "markdown-structural-v1");
// {impl}`requirements.sdk.sources.css.language`
language!(Css, css::PROFILE, "css-structural-v1");
// {impl}`requirements.sdk.sources.toml.language`
language!(Toml, toml::PROFILE, "toml-structural-v1");
// {impl}`requirements.sdk.sources.yaml.language`
language!(Yaml, yaml::PROFILE, "yaml-structural-v1");

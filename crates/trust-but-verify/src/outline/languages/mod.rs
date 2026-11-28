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

// {impl}`requirements.sdk.outlines.name_for_path`
/// Name of the bundled outline language selected by the filename alone.
pub fn name_for_path(path: &std::path::Path) -> &'static str {
    crate::outline_engine::profile::profile_for_path(path).name
}

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
    ($name:ident, $profile:path) => {
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
                $profile.fingerprint_scheme
            }

            fn search_structure(
                &self,
                source: &str,
                rule: &serde_json::Map<String, serde_json::Value>,
            ) -> Result<Vec<super::Span>, super::OutlineError> {
                if $profile.language.is_some() {
                    if let Some(error) = super::parse_with(&$profile, source)?.diagnostic {
                        return Err(error);
                    }
                    let compiled = crate::structural::StructuralRule::compile($profile.name, rule)
                        .map_err(|message| super::OutlineError { line: 1, message })?;
                    return Ok(compiled
                        .matches(source)
                        .into_iter()
                        .map(super::Span::from_range)
                        .collect());
                }
                Err(super::unsupported_search(self.name()))
            }
        }
    };
}

// {impl}`requirements.sdk.sources.plain-text.PlainText`
// {impl}`requirements.sdk.sources.plain-text.language`
language!(PlainText, super::plain::PROFILE);
// {impl}`requirements.sdk.sources.rust.Rust`
// {impl}`requirements.sdk.sources.rust.language`
language!(Rust, rust::PROFILE);
// {impl}`requirements.sdk.sources.python.Python`
// {impl}`requirements.sdk.sources.python.language`
language!(Python, python::PROFILE);
// {impl}`requirements.sdk.sources.typescript.TypeScript`
// {impl}`requirements.sdk.sources.typescript.language`
language!(TypeScript, typescript::PROFILE);
// {impl}`requirements.sdk.sources.typescript.Tsx`
// {impl}`requirements.sdk.sources.typescript.language`
language!(Tsx, typescript::TSX_PROFILE);
// {impl}`requirements.sdk.sources.markdown.Markdown`
// {impl}`requirements.sdk.sources.markdown.language`
language!(Markdown, markdown::PROFILE);
// {impl}`requirements.sdk.sources.css.Css`
// {impl}`requirements.sdk.sources.css.language`
language!(Css, css::PROFILE);
// {impl}`requirements.sdk.sources.toml.Toml`
// {impl}`requirements.sdk.sources.toml.language`
language!(Toml, toml::PROFILE);
// {impl}`requirements.sdk.sources.yaml.Yaml`
// {impl}`requirements.sdk.sources.yaml.language`
language!(Yaml, yaml::PROFILE);

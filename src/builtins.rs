//! Built-in definitions named `habeas:<name>`, available in every project.

use std::sync::OnceLock;

use crate::ReferenceKind;

/// A built-in definition: fixed text, the roles that may reference it, and
/// whether a reference must be fenced with a nonempty justification.
// {impl}`requirements.references.builtin_definitions`
pub(crate) struct Builtin {
    pub(crate) name: &'static str,
    pub(crate) roles: &'static [ReferenceKind],
    pub(crate) requires_justification: bool,
}

/// Every built-in definition.
// {impl}`requirements.references.builtin_definitions`
// {impl}`requirements.references.no_requirement`
pub(crate) const BUILTINS: &[Builtin] = &[Builtin {
    name: "habeas:none",
    roles: &[ReferenceKind::Cite],
    requires_justification: true,
}];

/// Whether a name has the reserved built-in form `habeas:<name>`.
// {impl}`requirements.references.builtin_definitions`
pub(crate) fn is_builtin_name(name: &str) -> bool {
    name.strip_prefix("habeas:").is_some_and(crate::valid_name)
}

/// The built-in definition with a name, if any.
// {impl}`requirements.references.builtin_definitions`
pub(crate) fn builtin(name: &str) -> Option<&'static Builtin> {
    BUILTINS.iter().find(|builtin| builtin.name == name)
}

impl Builtin {
    /// The built-in's fixed text, as quoted in its requirement.
    // {impl}`requirements.references.builtin_definitions`
    // {impl}`requirements.references.no_requirement`
    pub(crate) fn text(&self) -> &'static str {
        static NONE: OnceLock<String> = OnceLock::new();
        match self.name {
            "habeas:none" => NONE.get_or_init(|| {
                // Keep the approved requirement text as the sole source.
                let chapter = include_str!("../md/requirements/references.md");
                quoted_text(chapter, ":::{req} no_requirement")
            }),
            _ => unreachable!("every built-in has text"),
        }
    }

    /// Checks a reference to this built-in, returning the problem when its
    /// role or form is not permitted. `fenced` is the reference's body when
    /// written in fenced form.
    // {impl}`requirements.references.builtin_definitions`
    // {impl}`requirements.references.no_requirement`
    pub(crate) fn check(&self, kind: ReferenceKind, fenced: Option<&str>) -> Result<(), String> {
        if !self.roles.contains(&kind) {
            return Err(format!(
                "built-in {} may only be referenced with {}",
                self.name,
                self.roles
                    .iter()
                    .map(|role| format!("{{{}}}", role.label()))
                    .collect::<Vec<_>>()
                    .join(" or ")
            ));
        }
        if self.requires_justification {
            match fenced {
                None => {
                    return Err(format!(
                        "built-in {} must be cited in fenced form with a body justifying why the item exists",
                        self.name
                    ));
                }
                Some(body) if crate::fenced::justification(body).is_empty() => {
                    return Err(format!(
                        "fenced citation of built-in {} has an empty justification",
                        self.name
                    ));
                }
                Some(_) => {}
            }
        }
        Ok(())
    }
}

/// The first block quote after `marker` in a chapter, without its `> `
/// prefixes, joined as one paragraph.
// {impl}`requirements.references.builtin_definitions`
fn quoted_text(chapter: &str, marker: &str) -> String {
    let start = chapter.find(marker).expect("approved built-in text");
    chapter[start..]
        .lines()
        .skip_while(|line| !line.starts_with('>'))
        .take_while(|line| line.starts_with('>'))
        .map(|line| line.trim_start_matches('>').trim())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    // {test}`requirements.references.no_requirement`
    // {test}`requirements.references.builtin_definitions`
    #[test]
    fn habeas_none_has_its_approved_text_and_only_permits_fenced_citations() {
        let none = builtin("habeas:none").unwrap();
        assert!(
            none.text()
                .starts_with("Cite this only when no requirement")
        );
        assert!(none.text().ends_with("that no requirement specifies."));
        assert!(
            none.check(ReferenceKind::Cite, Some("Glue for the CLI."))
                .is_ok()
        );
        assert!(
            none.check(ReferenceKind::Impl, Some("x"))
                .unwrap_err()
                .contains("only be referenced with {cite}")
        );
        assert!(
            none.check(ReferenceKind::Cite, None)
                .unwrap_err()
                .contains("fenced form")
        );
        let empty = concat!(" {", "cite}`requirements.ready` ");
        assert!(
            none.check(ReferenceKind::Cite, Some(empty))
                .unwrap_err()
                .contains("empty justification")
        );
        assert!(builtin("habeas:other").is_none());
        assert!(is_builtin_name("habeas:other"));
        assert!(!is_builtin_name("habeas:"));
        assert!(!is_builtin_name("other:none"));
    }
}

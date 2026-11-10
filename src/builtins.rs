//! Built-in definitions named `habeas:<name>`, available in every project.

use std::sync::OnceLock;

use crate::ReferenceKind;

/// A built-in definition: fixed text, the roles that may reference it, and
/// whether a reference must be fenced with a nonempty justification.
// {cite}`habeas:util`
pub(crate) struct Builtin {
    pub(crate) name: &'static str,
    pub(crate) roles: &'static [ReferenceKind],
    pub(crate) requires_justification: bool,
}

/// Every built-in definition.
// {impl}`requirements.references.util`
pub(crate) const BUILTINS: &[Builtin] = &[
    Builtin {
        name: "habeas:none",
        roles: &[ReferenceKind::Cite],
        requires_justification: true,
    },
    Builtin {
        name: "habeas:util",
        roles: &[ReferenceKind::Cite],
        requires_justification: false,
    },
];

/// Whether a name has the reserved built-in form `habeas:<name>`.
// {cite}`habeas:util`
pub(crate) fn is_builtin_name(name: &str) -> bool {
    name.strip_prefix("habeas:").is_some_and(crate::valid_name)
}

/// The built-in definition with a name, if any.
// {cite}`habeas:util`
pub(crate) fn builtin(name: &str) -> Option<&'static Builtin> {
    BUILTINS.iter().find(|builtin| builtin.name == name)
}

impl Builtin {
    /// The built-in's fixed text, as quoted in its requirement.
    // {impl}`requirements.references.no_requirement`
    // {impl}`requirements.references.util`
    pub(crate) fn text(&self) -> &'static str {
        static NONE: OnceLock<String> = OnceLock::new();
        static UTIL: OnceLock<String> = OnceLock::new();
        // Keep the approved requirement text as the sole source.
        let chapter = include_str!("../md/requirements/references.md");
        match self.name {
            "habeas:none" => NONE.get_or_init(|| quoted_text(chapter, ":::{req} no_requirement")),
            "habeas:util" => UTIL.get_or_init(|| quoted_text(chapter, ":::{req} util")),
            _ => unreachable!("every built-in has text"),
        }
    }

    /// Checks a reference to this built-in, returning the problem when its
    /// role or form is not permitted. `fenced` is the reference's body when
    /// written in fenced form.
    // {cite}`habeas:util`
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
// {cite}`habeas:util`
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
        // The reference documentation quotes the same text.
        let documented = quoted_text(
            include_str!("../md/reference.md"),
            "The built-in `habeas:none` has this text:",
        );
        assert_eq!(documented, none.text());
        assert!(builtin("habeas:other").is_none());
        assert!(is_builtin_name("habeas:util"));
        assert!(is_builtin_name("habeas:other"));
        assert!(!is_builtin_name("habeas:"));
        assert!(!is_builtin_name("other:none"));
    }

    // {test}`requirements.references.util`
    // {test}`requirements.references.builtin_definitions`
    #[test]
    fn habeas_util_has_its_approved_text_and_permits_inline_or_fenced_citations() {
        let util = builtin("habeas:util").unwrap();
        assert!(
            util.text()
                .starts_with("Cite this on an item that serves the code that calls it")
        );
        assert!(
            util.text()
                .ends_with("judges what it does as part of that root.")
        );
        assert!(util.check(ReferenceKind::Cite, None).is_ok());
        assert!(util.check(ReferenceKind::Cite, Some("")).is_ok());
        assert!(
            util.check(ReferenceKind::Cite, Some("Formats a line for its caller."))
                .is_ok()
        );
        for role in [ReferenceKind::Impl, ReferenceKind::Test] {
            assert!(
                util.check(role, None)
                    .unwrap_err()
                    .contains("built-in habeas:util may only be referenced with {cite}")
            );
        }
        // The reference documentation quotes the same text.
        let documented = quoted_text(
            include_str!("../md/reference.md"),
            "The built-in `habeas:util` has this text:",
        );
        assert_eq!(documented, util.text());
    }
}

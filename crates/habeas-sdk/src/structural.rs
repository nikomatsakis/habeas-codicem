//! Structural search: ast-grep rules matched against the pinned outline grammars.
//!
//! Consumers use the same engine for interactive search and deterministic
//! checks, with match ranges attached to their owning outline nodes.

use std::borrow::Cow;
use std::ops::Range;
use std::path::Path;

use ast_grep_config::{DeserializeEnv, RuleCore, SerializableRuleCore};
use ast_grep_core::matcher::PatternBuilder;
use ast_grep_core::tree_sitter::{LanguageExt, StrDoc, TSLanguage};
use ast_grep_core::{Language, Pattern, PatternError};
use serde_json::{Map, Value};

use crate::outline::{Outline, Profile};

// {cite}`habeas:util`
/// An outline profile with a grammar, as an ast-grep language.
#[derive(Clone, Copy, Debug)]
pub struct QueryLanguage {
    pub profile: &'static Profile,
    grammar: fn() -> tree_sitter::Language,
}

impl QueryLanguage {
    // {impl}`requirements.references.distributed`
    /// The language named by an outline profile with a grammar.
    pub fn named(name: &str) -> Result<Self, String> {
        crate::outline::profile::profiles()
            .into_iter()
            .find(|profile| profile.name == name)
            .and_then(|profile| profile.language.map(|grammar| Self { profile, grammar }))
            .ok_or_else(|| {
                format!(
                    "unknown structural search language {name:?}; expected one of {}",
                    Self::names().join(", ")
                )
            })
    }

    // {cite}`habeas:util`
    /// The names of the outline profiles with a grammar.
    pub fn names() -> Vec<&'static str> {
        crate::outline::profile::profiles()
            .into_iter()
            .filter(|profile| profile.language.is_some())
            .map(|profile| profile.name)
            .collect()
    }

    // {impl}`requirements.references.distributed`
    /// Whether a file is a source of this language.
    pub fn covers(&self, path: &Path) -> bool {
        std::ptr::eq(crate::outline::profile_for_path(path), self.profile)
    }
}

impl Language for QueryLanguage {
    // {impl}`requirements.authoring.structural_search`
    fn kind_to_id(&self, kind: &str) -> u16 {
        self.get_ts_language().id_for_node_kind(kind, true)
    }

    // {impl}`requirements.authoring.structural_search`
    fn field_to_id(&self, field: &str) -> Option<u16> {
        self.get_ts_language()
            .field_id_for_name(field)
            .map(|field| field.get())
    }

    // {cite}`habeas:util`
    /// Grammars where `$` cannot start an identifier parse metavariables
    /// under another character, as ast-grep's own language table does.
    fn expando_char(&self) -> char {
        match self.profile.name {
            "rust" | "python" => 'µ',
            "css" | "toml" => '_',
            _ => '$',
        }
    }

    // {impl}`requirements.authoring.structural_search`
    fn pre_process_pattern<'q>(&self, query: &'q str) -> Cow<'q, str> {
        let expando = self.expando_char();
        if expando == '$' {
            return Cow::Borrowed(query);
        }
        // Replace the `$` of `$A`, `$$A`, `$$$A`, `$_`, and `$$$`, keeping
        // other dollars as written.
        let mut processed = String::with_capacity(query.len());
        let mut dollars = 0;
        for character in query.chars() {
            if character == '$' {
                dollars += 1;
                continue;
            }
            let replace = matches!(character, 'A'..='Z' | '_') || dollars == 3;
            let sigil = if replace { expando } else { '$' };
            processed.extend(std::iter::repeat_n(sigil, dollars));
            dollars = 0;
            processed.push(character);
        }
        let sigil = if dollars == 3 { expando } else { '$' };
        processed.extend(std::iter::repeat_n(sigil, dollars));
        Cow::Owned(processed)
    }

    // {impl}`requirements.authoring.structural_search`
    fn build_pattern(&self, builder: &PatternBuilder) -> Result<Pattern, PatternError> {
        builder.build(|source| StrDoc::try_new(source, *self))
    }
}

impl LanguageExt for QueryLanguage {
    // {cite}`habeas:util`
    fn get_ts_language(&self) -> TSLanguage {
        (self.grammar)()
    }
}

// {cite}`habeas:util`
/// A compiled ast-grep rule core for one language.
pub struct StructuralRule {
    language: QueryLanguage,
    core: RuleCore,
}

impl StructuralRule {
    /// Compiles a rule core: an object holding `rule` and optional `utils`
    /// and `constraints`, in the syntax of the pinned `ast-grep-config`.
    pub fn compile(language: &str, rule: &Map<String, Value>) -> Result<Self, String> {
        let language = QueryLanguage::named(language)?;
        for key in rule.keys() {
            if !["rule", "utils", "constraints"].contains(&key.as_str()) {
                return Err(format!(
                    "invalid ast-grep rule: unexpected key {key:?}; a rule object holds only rule, utils, and constraints"
                ));
            }
        }
        if !rule.contains_key("rule") {
            return Err("invalid ast-grep rule: missing key \"rule\"".to_owned());
        }
        let serialized: SerializableRuleCore = serde_json::from_value(Value::Object(rule.clone()))
            .map_err(|error| format!("invalid ast-grep rule: {error}"))?;
        let core = serialized
            .get_matcher(DeserializeEnv::new(language))
            .map_err(|error| format!("invalid ast-grep rule: {}", error_chain(&error)))?;
        Ok(Self { language, core })
    }

    pub fn language(&self) -> QueryLanguage {
        self.language
    }

    /// The byte ranges of every match in a source, in source order.
    pub fn matches(&self, source: &str) -> Vec<Range<usize>> {
        let root = self.language.ast_grep(source);
        let mut ranges = root
            .root()
            .find_all(&self.core)
            .map(|found| found.range())
            .collect::<Vec<_>>();
        ranges.sort_by_key(|range| (range.start, range.end));
        ranges.dedup();
        ranges
    }
}

// {cite}`habeas:util`
/// An error with each of its sources, since ast-grep's top-level messages
/// name only the part of the rule that failed.
fn error_chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

/// The innermost outline node containing a byte range of the source the
/// outline was parsed from. A match within a comment belongs to the
/// comment's owner; a node contains the lines attached before it.
// {impl}`requirements.references.outline_comments`
pub fn innermost_node(outline: &Outline, source: &str, range: &Range<usize>) -> usize {
    if let Some(comment) = outline
        .comments
        .iter()
        .find(|comment| comment.byte_start <= range.start && range.end <= comment.byte_end)
    {
        return comment.owner;
    }
    let line_starts = std::iter::once(0)
        .chain(source.match_indices('\n').map(|(index, _)| index + 1))
        .collect::<Vec<_>>();
    let line_of = |byte: usize| line_starts.partition_point(|start| *start <= byte);
    let contains = |index: usize| {
        let node = &outline.nodes[index];
        let start = if node.line_start < line_of(node.byte_start) {
            line_starts[node.line_start - 1]
        } else {
            node.byte_start
        };
        start <= range.start && range.end <= node.byte_end
    };
    let mut current = 0;
    while let Some(child) = outline.nodes[current]
        .children
        .iter()
        .copied()
        .find(|child| contains(*child))
    {
        current = child;
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // {test}`requirements.authoring.structural_search`
    // {test}`requirements.references.distributed`
    #[test]
    fn rules_compile_for_every_grammar_and_report_invalid_input() {
        for (language, rule, source, expected) in [
            (
                "rust",
                json!({"pattern": "foo($A)"}),
                "fn f() { foo(1); }\n",
                "foo(1)",
            ),
            (
                "python",
                json!({"pattern": "foo($A)"}),
                "foo(1)\n",
                "foo(1)",
            ),
            (
                "typescript",
                json!({"pattern": "foo($A)"}),
                "foo(1);\n",
                "foo(1)",
            ),
            (
                "tsx",
                json!({"pattern": "<a>{$X}</a>"}),
                "const x = <a>{y}</a>;\n",
                "<a>{y}</a>",
            ),
            (
                "css",
                json!({"kind": "class_selector"}),
                ".a { color: red; }\n",
                ".a",
            ),
            (
                "markdown",
                json!({"kind": "atx_heading"}),
                "# Title\n\nText.\n",
                "# Title\n",
            ),
            (
                "toml",
                json!({"pattern": "name = $V"}),
                "name = 'x'\n",
                "name = 'x'",
            ),
            (
                "yaml",
                json!({"kind": "block_mapping_pair"}),
                "key: value\n",
                "key: value",
            ),
        ] {
            let rule =
                StructuralRule::compile(language, json!({"rule": rule}).as_object().unwrap())
                    .unwrap_or_else(|error| panic!("{language}: {error}"));
            let found = rule
                .matches(source)
                .into_iter()
                .map(|range| &source[range])
                .collect::<Vec<_>>();
            assert_eq!(found, [expected], "{language}");
        }
        for (language, rule, message) in [
            (
                "cobol",
                json!({"rule": {"kind": "x"}}),
                "unknown structural search language",
            ),
            (
                "plain-text",
                json!({"rule": {"kind": "x"}}),
                "unknown structural search language",
            ),
            ("rust", json!({}), "missing key \"rule\""),
            (
                "rust",
                json!({"rule": {"kind": "x"}, "fix": "y"}),
                "unexpected key \"fix\"",
            ),
            (
                "rust",
                json!({"rule": {"kind": "no_such_kind"}}),
                "invalid ast-grep rule",
            ),
            (
                "rust",
                json!({"rule": {"bogus": 1}}),
                "invalid ast-grep rule",
            ),
        ] {
            let error = StructuralRule::compile(language, rule.as_object().unwrap())
                .err()
                .unwrap();
            assert!(error.contains(message), "{rule}: {error}");
        }
    }

    // {test}`requirements.authoring.structural_search`
    // {test}`requirements.references.distributed`
    // {test}`requirements.references.outline_comments`
    #[test]
    fn matches_belong_to_the_innermost_node_or_comment_owner() {
        let source = "mod outer {\n    /// Uses foo.\n    #[inline]\n    fn inner() {\n        foo(1);\n    }\n}\n";
        let outline = crate::outline::parse(Path::new("lib.rs"), source).unwrap();
        let item = |text: &str| {
            let start = source.find(text).unwrap();
            let index = innermost_node(&outline, source, &(start..start + text.len()));
            outline.nodes[index].selector.clone()
        };
        assert_eq!(item("foo(1)"), "mod[outer]/fn[inner]");
        assert_eq!(item("/// Uses foo.\n"), "mod[outer]/fn[inner]");
        assert_eq!(item("#[inline]"), "mod[outer]/fn[inner]");
        assert_eq!(item("mod outer"), "mod[outer]");
        assert_eq!(item(source), "");
    }
}

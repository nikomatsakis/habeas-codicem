//! Structural search: ast-grep rules matched against Habeas's pinned outline
//! grammars, and the named queries distributed requirements declare.
//!
//! The `ast_grep` authoring tool and distributed-requirement validation share
//! this engine, so a declared query finds the same sites wherever it runs.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use ast_grep_config::{DeserializeEnv, RuleCore, SerializableRuleCore};
use ast_grep_core::matcher::PatternBuilder;
use ast_grep_core::tree_sitter::{LanguageExt, StrDoc, TSLanguage};
use ast_grep_core::{Language, Pattern, PatternError};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::evidence::Declaration;
use crate::inputs::OutlineCache;
use crate::outline::{Outline, Profile};

/// An outline profile with a grammar, as an ast-grep language.
// {impl}`requirements.authoring.structural_search`
// {impl}`requirements.references.distributed`
#[derive(Clone, Copy, Debug)]
pub(crate) struct QueryLanguage {
    profile: &'static Profile,
    grammar: fn() -> tree_sitter::Language,
}

impl QueryLanguage {
    /// The language named by an outline profile with a grammar.
    // {impl}`requirements.authoring.structural_search`
    // {impl}`requirements.references.distributed`
    pub(crate) fn named(name: &str) -> Result<Self, String> {
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

    /// The names of the outline profiles with a grammar.
    // {impl}`requirements.authoring.structural_search`
    // {impl}`requirements.references.distributed`
    pub(crate) fn names() -> Vec<&'static str> {
        crate::outline::profile::profiles()
            .into_iter()
            .filter(|profile| profile.language.is_some())
            .map(|profile| profile.name)
            .collect()
    }

    /// Whether a file is a source of this language.
    // {impl}`requirements.authoring.structural_search`
    // {impl}`requirements.references.distributed`
    pub(crate) fn covers(&self, path: &Path) -> bool {
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

    /// Grammars where `$` cannot start an identifier parse metavariables
    /// under another character, as ast-grep's own language table does.
    // {impl}`requirements.authoring.structural_search`
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
    // {impl}`requirements.authoring.structural_search`
    fn get_ts_language(&self) -> TSLanguage {
        (self.grammar)()
    }
}

/// A compiled ast-grep rule core for one language.
// {impl}`requirements.authoring.structural_search`
// {impl}`requirements.references.distributed`
pub(crate) struct StructuralRule {
    language: QueryLanguage,
    core: RuleCore,
}

impl StructuralRule {
    /// Compiles a rule core: an object holding `rule` and optional `utils`
    /// and `constraints`, in the syntax of the pinned `ast-grep-config`.
    // {impl}`requirements.authoring.structural_search`
    // {impl}`requirements.references.distributed`
    pub(crate) fn compile(language: &str, rule: &Map<String, Value>) -> Result<Self, String> {
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

    // {impl}`requirements.authoring.structural_search`
    // {impl}`requirements.references.distributed`
    pub(crate) fn language(&self) -> QueryLanguage {
        self.language
    }

    /// The byte ranges of every match in a source, in source order.
    // {impl}`requirements.authoring.structural_search`
    // {impl}`requirements.references.distributed`
    pub(crate) fn matches(&self, source: &str) -> Vec<Range<usize>> {
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

/// An error with each of its sources, since ast-grep's top-level messages
/// name only the part of the rule that failed.
// {impl}`requirements.authoring.structural_search`
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
// {impl}`requirements.authoring.structural_search`
// {impl}`requirements.references.distributed`
// {impl}`requirements.references.outline_comments`
pub(crate) fn innermost_node(outline: &Outline, source: &str, range: &Range<usize>) -> usize {
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

/// Which directive declares a query.
// {impl}`requirements.references.distributed`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QueryKind {
    Sites,
    Violations,
}

/// A query body: one `yaml` block of a `{sites}` or `{violations}`
/// directive.
// {impl}`requirements.references.distributed`
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QueryBody {
    pub(crate) language: String,
    #[serde(default)]
    pub(crate) paths: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) exclude: Vec<String>,
    pub(crate) rule: Value,
    #[serde(default)]
    pub(crate) utils: Option<Value>,
    #[serde(default)]
    pub(crate) constraints: Option<Value>,
    #[serde(default)]
    pub(crate) within: Option<String>,
    // Validation checks the snippets; searches only require them.
    #[serde(rename = "match")]
    #[allow(dead_code)]
    pub(crate) matches: Vec<String>,
    #[allow(dead_code)]
    pub(crate) no_match: Vec<String>,
}

impl QueryBody {
    /// The body's rule core: its `rule`, `utils`, and `constraints`.
    // {impl}`requirements.references.distributed`
    pub(crate) fn rule_core(&self) -> Map<String, Value> {
        let mut core = Map::new();
        core.insert("rule".to_owned(), self.rule.clone());
        if let Some(utils) = &self.utils {
            core.insert("utils".to_owned(), utils.clone());
        }
        if let Some(constraints) = &self.constraints {
            core.insert("constraints".to_owned(), constraints.clone());
        }
        core
    }

    /// Whether a project-relative source is searched under `paths` and
    /// `exclude`.
    // {impl}`requirements.references.distributed`
    pub(crate) fn searches(&self, file: &Path) -> bool {
        let under = |entry: &String| file.starts_with(Path::new(entry.trim_end_matches('/')));
        self.paths
            .as_ref()
            .is_none_or(|paths| paths.iter().any(under))
            && !self.exclude.iter().any(under)
    }
}

/// A query a distributed requirement declares.
// {impl}`requirements.references.distributed`
#[derive(Debug)]
pub(crate) struct DeclaredQuery {
    pub(crate) name: String,
    pub(crate) kind: QueryKind,
    pub(crate) body: QueryBody,
}

/// The queries a requirement declaration contains, in declaration order.
// {impl}`requirements.references.distributed`
pub(crate) fn declared_queries(declaration: &Declaration) -> Result<Vec<DeclaredQuery>, String> {
    let lines = &declaration.raw_lines;
    let mut queries = Vec::new();
    let mut index = 1;
    while index < lines.len() {
        let Some((fence, kind, name)) = query_fence(&lines[index]) else {
            index += 1;
            continue;
        };
        let line = declaration.line_start + index;
        let close = lines[index + 1..]
            .iter()
            .position(|candidate| candidate.trim_end() == fence)
            .map(|offset| index + 1 + offset)
            .ok_or_else(|| format!("{}:{line}: query {name} is not closed", declaration.file))?;
        let body = lines[index + 1..close]
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let yaml = yaml_block(&body).ok_or_else(|| {
            format!(
                "{}:{line}: query {name} must contain exactly one fenced yaml code block",
                declaration.file
            )
        })?;
        let body: QueryBody = serde_saphyr::from_str(&yaml)
            .map_err(|error| format!("{}:{line}: query {name}: {error}", declaration.file))?;
        queries.push(DeclaredQuery {
            name: name.to_owned(),
            kind,
            body,
        });
        index = close + 1;
    }
    Ok(queries)
}

/// A query directive's opening fence: its colons, kind, and name.
// {impl}`requirements.references.distributed`
fn query_fence(line: &str) -> Option<(&str, QueryKind, &str)> {
    let colons = line.len() - line.trim_start_matches(':').len();
    if colons < 3 {
        return None;
    }
    let (fence, rest) = line.split_at(colons);
    let (kind, name) = match rest.strip_prefix("{sites}") {
        Some(name) => (QueryKind::Sites, name),
        None => (QueryKind::Violations, rest.strip_prefix("{violations}")?),
    };
    let name = name.trim();
    (!name.is_empty() && !name.contains(char::is_whitespace)).then_some((fence, kind, name))
}

/// The text of the one fenced `yaml` code block that makes up a query body.
// {impl}`requirements.references.distributed`
fn yaml_block(lines: &[&str]) -> Option<String> {
    let content = lines
        .iter()
        .copied()
        .skip_while(|line| line.trim().is_empty())
        .collect::<Vec<_>>();
    let content = &content[..content
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .map_or(0, |last| last + 1)];
    let (open, rest) = content.split_first()?;
    let (close, body) = rest.split_last()?;
    let ticks = open.len() - open.trim_start_matches(['`', '~']).len();
    let marker = &open[..ticks];
    if ticks < 3 || open[ticks..].trim() != "yaml" || close.trim_end() != marker {
        return None;
    }
    if body.iter().any(|line| line.trim_end() == marker) {
        return None;
    }
    Some(body.iter().map(|line| format!("{line}\n")).collect())
}

/// One site of a query: a project-relative file and an outline node of it.
// {impl}`requirements.references.distributed`
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Site {
    pub(crate) file: String,
    pub(crate) item: String,
}

/// Runs a query a requirement declares, named `<query-name>`, over the
/// sources reference discovery considers, in their original text, and
/// returns the distinct sites of its matches in path and source order. A
/// violation query's matches belong to the innermost site of its `within`
/// query containing them; matches inside none of them are discarded.
// {impl}`requirements.references.distributed`
// {impl}`requirements.authoring.structural_search`
pub(crate) fn query_sites(
    root: &Path,
    files: &[PathBuf],
    outlines: &OutlineCache,
    queries: &[DeclaredQuery],
    name: &str,
) -> Result<Vec<Site>, String> {
    let query = queries
        .iter()
        .find(|query| query.name == name)
        .ok_or_else(|| format!("no query named {name:?}"))?;
    let within = match (query.kind, &query.body.within) {
        (QueryKind::Sites, None) => None,
        (QueryKind::Sites, Some(_)) => {
            return Err(format!("site query {name} must not have within"));
        }
        (QueryKind::Violations, None) => {
            return Err(format!("violation query {name} must have within"));
        }
        (QueryKind::Violations, Some(within)) => {
            let target = queries
                .iter()
                .find(|query| &query.name == within && query.kind == QueryKind::Sites)
                .ok_or_else(|| format!("{name}: within names no site query {within:?}"))?;
            let mut sites = BTreeMap::<String, Vec<String>>::new();
            for site in query_sites(root, files, outlines, queries, &target.name)? {
                sites.entry(site.file).or_default().push(site.item);
            }
            Some(sites)
        }
    };
    let rule = StructuralRule::compile(&query.body.language, &query.body.rule_core())
        .map_err(|error| format!("{name}: {error}"))?;
    let canonical = fs::canonicalize(root).unwrap_or_else(|_| root.to_owned());
    let mut sources = files
        .iter()
        .filter_map(|file| {
            let relative = file
                .strip_prefix(&canonical)
                .or_else(|_| file.strip_prefix(root))
                .unwrap_or(file);
            (rule.language().covers(file) && query.body.searches(relative))
                .then(|| (relative.to_string_lossy().into_owned(), file))
        })
        .collect::<Vec<_>>();
    sources.sort();
    let mut sites = Vec::new();
    for (relative, file) in sources {
        if within
            .as_ref()
            .is_some_and(|within| !within.contains_key(&relative))
        {
            continue;
        }
        let text =
            fs::read_to_string(file).map_err(|error| format!("cannot read {relative}: {error}"))?;
        let ranges = rule.matches(&text);
        if ranges.is_empty() {
            continue;
        }
        let parsed = outlines.parse(root, &relative);
        let parsed = parsed.as_ref().as_ref().map_err(Clone::clone)?;
        for range in ranges {
            let mut node = Some(innermost_node(&parsed.outline, &parsed.source, &range));
            if let Some(within) = &within {
                let selected = &within[&relative];
                while let Some(index) = node
                    && !selected.contains(&parsed.outline.nodes[index].selector)
                {
                    node = parsed.outline.nodes[index].parent;
                }
            }
            if let Some(index) = node {
                let site = Site {
                    file: relative.clone(),
                    item: parsed.outline.nodes[index].selector.clone(),
                };
                if !sites.contains(&site) {
                    sites.push(site);
                }
            }
        }
    }
    Ok(sites)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // {test}`requirements.authoring.structural_search`
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

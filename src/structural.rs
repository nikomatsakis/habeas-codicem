//! Distributed requirement queries built on the SDK structural search engine.
use crate::evidence::Declaration;
use crate::inputs::OutlineCache;
use crate::outline_items::OutlineItems;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
pub(crate) use trust_but_verify::structural::QueryLanguage;
pub(crate) use trust_but_verify::structural::StructuralRule;

// {impl}`requirements.references.distributed`
/// Which directive declares a query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QueryKind {
    Sites,
    Violations,
}

/// A query body: one `yaml` block of a `{sites}` or `{violations}`
/// directive.
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
    #[serde(rename = "match")]
    pub(crate) matches: Vec<String>,
    pub(crate) no_match: Vec<String>,
}

impl QueryBody {
    /// The body's rule core: its `rule`, `utils`, and `constraints`.
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
    pub(crate) fn searches(&self, file: &Path) -> bool {
        let under = |entry: &String| file.starts_with(Path::new(entry.trim_end_matches('/')));
        self.paths
            .as_ref()
            .is_none_or(|paths| paths.iter().any(under))
            && !self.exclude.iter().any(under)
    }
}

// {cite}`habeas:util`
/// A query a distributed requirement declares.
#[derive(Debug)]
pub(crate) struct DeclaredQuery {
    pub(crate) name: String,
    pub(crate) kind: QueryKind,
    pub(crate) body: QueryBody,
    /// Line of the query's opening fence in its chapter.
    pub(crate) line: usize,
}

// {cite}`habeas:util`
/// The queries a requirement declaration contains, in declaration order,
/// failing on the first malformed one.
pub(crate) fn declared_queries(declaration: &Declaration) -> Result<Vec<DeclaredQuery>, String> {
    let (queries, problems) = parse_queries(declaration);
    match problems.into_iter().next() {
        Some(problem) => Err(problem),
        None => Ok(queries),
    }
}

// {cite}`habeas:util`
/// Whether a declaration contains a query directive, well-formed or not.
pub(crate) fn has_query_directive(declaration: &Declaration) -> bool {
    declaration
        .raw_lines
        .iter()
        .skip(1)
        .any(|line| query_fence(line).is_some())
}

/// The well-formed queries a requirement declaration contains, in
/// declaration order, and a problem for each malformed one.
// {impl}`requirements.ci.errors`
pub(crate) fn parse_queries(declaration: &Declaration) -> (Vec<DeclaredQuery>, Vec<String>) {
    let lines = &declaration.raw_lines;
    let mut queries = Vec::new();
    let mut errors = Vec::new();
    let mut index = 1;
    while index < lines.len() {
        let Some((fence, kind, name)) = query_fence(&lines[index]) else {
            index += 1;
            continue;
        };
        let line = declaration.line_start + index;
        let Some(close) = lines[index + 1..]
            .iter()
            .position(|candidate| candidate.trim_end() == fence)
            .map(|offset| index + 1 + offset)
        else {
            errors.push(format!(
                "{}:{line}: query {name} is not closed",
                declaration.file
            ));
            // The rest of the declaration lies inside the unclosed directive,
            // so no later query can be found in it.
            break;
        };
        let body = lines[index + 1..close]
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        index = close + 1;
        let Some(yaml) = yaml_block(&body) else {
            errors.push(format!(
                "{}:{line}: query {name} must contain exactly one fenced yaml code block",
                declaration.file
            ));
            // Decoding the body needs its yaml block.
            continue;
        };
        match serde_saphyr::from_str::<QueryBody>(&yaml) {
            Ok(body) => queries.push(DeclaredQuery {
                name: name.to_owned(),
                kind,
                body,
                line,
            }),
            Err(error) => errors.push(format!(
                "{}:{line}: query {name}: the yaml block must be a mapping of language, paths, exclude, rule, utils, constraints, within, match, and no_match: {error}",
                declaration.file
            )),
        }
    }
    (queries, errors)
}

/// A query directive's opening fence: its colons, kind, and name.
// {cite}`habeas:util`
pub(crate) fn query_fence(line: &str) -> Option<(&str, QueryKind, &str)> {
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
// {cite}`habeas:util`
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

// {cite}`habeas:util`
/// One site of a query: a project-relative file and an outline node of it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Site {
    pub(crate) file: String,
    pub(crate) item: String,
}

impl QueryKind {
    // {impl}`requirements.references.distributed`
    /// The directive's role name, `sites` or `violations`.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Sites => "sites",
            Self::Violations => "violations",
        }
    }
}

// {cite}`habeas:util`
/// One match of a declared query in the project, with its site.
#[derive(Clone, Debug)]
pub(crate) struct QueryMatch {
    pub(crate) site: Site,
    /// Inclusive one-based lines of the match.
    pub(crate) line_start: usize,
    pub(crate) line_end: usize,
    /// The matched source text.
    pub(crate) text: String,
}

/// Runs a query a requirement declares, named `<query-name>`, over the
/// sources reference discovery considers, in their original text, and
/// returns the distinct sites of its matches in path and source order. A
/// violation query's matches belong to the innermost site of its `within`
/// query containing them; matches inside none of them are discarded.
pub(crate) fn query_sites(
    root: &Path,
    files: &[PathBuf],
    outlines: &OutlineCache,
    queries: &[DeclaredQuery],
    name: &str,
) -> Result<Vec<Site>, String> {
    let mut sites = Vec::new();
    for found in query_matches(root, files, outlines, queries, name)? {
        if !sites.contains(&found.site) {
            sites.push(found.site);
        }
    }
    Ok(sites)
}

/// Every match of a declared query, with its site, in path and source
/// order, as [`query_sites`] finds them.
pub(crate) fn query_matches(
    root: &Path,
    files: &[PathBuf],
    outlines: &OutlineCache,
    queries: &[DeclaredQuery],
    name: &str,
) -> Result<Vec<QueryMatch>, String> {
    let query = queries
        .iter()
        .find(|query| query.name == name)
        .ok_or_else(|| format!("no query named {name:?}"))?;
    let within = match within_query(queries, query)? {
        None => None,
        Some(target) => {
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
    let mut found = Vec::new();
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
        let within_nodes = within.as_ref().map(|within| {
            let selected = &within[&relative];
            (0..parsed.outline.nodes.len())
                .filter(|index| selected.contains(&parsed.outline.nodes[*index].selector))
                .collect::<Vec<_>>()
        });
        let line_of = |byte: usize| text[..byte].matches('\n').count() + 1;
        for range in ranges {
            let Some(index) =
                owning_site(&parsed.outline.ownership, &range, within_nodes.as_deref())
            else {
                continue;
            };
            found.push(QueryMatch {
                site: Site {
                    file: relative.clone(),
                    item: parsed.outline.nodes[index].selector.clone(),
                },
                line_start: line_of(range.start),
                line_end: line_of(range.end.saturating_sub(1).max(range.start)),
                text: text[range].to_owned(),
            });
        }
    }
    Ok(found)
}

/// The site query a query's matches must lie within: none for a site
/// query, and the `within` target, which must be a site query of the same
/// requirement, for a violation query.
pub(crate) fn within_query<'a>(
    queries: &'a [DeclaredQuery],
    query: &DeclaredQuery,
) -> Result<Option<&'a DeclaredQuery>, String> {
    let name = &query.name;
    match (query.kind, &query.body.within) {
        (QueryKind::Sites, None) => Ok(None),
        (QueryKind::Sites, Some(_)) => Err(format!("site query {name} must not have within")),
        (QueryKind::Violations, None) => Err(format!("violation query {name} must have within")),
        (QueryKind::Violations, Some(within)) => queries
            .iter()
            .find(|query| &query.name == within && query.kind == QueryKind::Sites)
            .map(Some)
            .ok_or_else(|| format!("{name}: within names no site query {within:?}")),
    }
}

/// The site a match belongs to: the innermost outline node containing it,
/// or, with `within`, the innermost of those nodes containing it.
fn owning_site(
    outline: &OutlineItems,
    range: &Range<usize>,
    within: Option<&[usize]>,
) -> Option<usize> {
    let mut node = Some(outline.owner(range));
    if let Some(within) = within {
        while let Some(index) = node
            && !within.contains(&index)
        {
            node = outline.parent(index);
        }
    }
    node
}

/// How many matches a query has in one snippet of its language, counting a
/// violation query's matches only inside a site of its `within` query in
/// the same snippet. A snippet that does not parse is an error.
pub(crate) fn snippet_matches(
    cache: &crate::inputs::SourceCache,
    language: &str,
    rule: &StructuralRule,
    site_rule: Option<&StructuralRule>,
    snippet: &str,
) -> Result<usize, String> {
    let filename = format!("snippet.{}", rule.language().profile.extensions[0]);
    let outline = crate::outline_items::for_file(cache, Path::new(&filename), snippet)
        .map_err(|error| format!("does not parse as {language}: {error}"))?;
    if let Some(error) = &outline.diagnostic {
        return Err(format!("does not parse as {language}: {error}"));
    }
    let within = site_rule.map(|site_rule| {
        site_rule
            .matches(snippet)
            .iter()
            .map(|range| outline.owner(range))
            .collect::<Vec<_>>()
    });
    Ok(rule
        .matches(snippet)
        .iter()
        .filter(|range| owning_site(&outline, range, within.as_deref()).is_some())
        .count())
}

#[cfg(test)]
mod tests {
    use super::*;

    // {test}`requirements.references.distributed`
    #[test]
    fn query_bodies_require_exactly_one_yaml_fence_and_no_other_content() {
        let yaml = "language: rust\nrule:\n  kind: function_item\nmatch: ['fn f() {}']\nno_match: ['struct S;']";
        let declaration = |body: &str| {
            let text = format!(":::{{req}} example\n:::{{sites}} functions\n{body}\n:::\n:::");
            Declaration {
                kind: crate::DefinitionKind::Requirement,
                source: "md/requirements.md:1".to_owned(),
                file: "md/requirements.md".to_owned(),
                line_start: 1,
                line_end: text.lines().count(),
                raw_lines: text.lines().map(str::to_owned).collect(),
                anchors: vec![],
                text,
            }
        };
        for field in ["language", "rule", "match", "no_match"] {
            let lines = [
                "language: rust",
                "rule: {kind: function_item}",
                "match: ['fn f() {}']",
                "no_match: ['struct S;']",
            ];
            let incomplete = lines
                .into_iter()
                .filter(|line| line.split_once(':').unwrap().0 != field)
                .collect::<Vec<_>>()
                .join("\n");
            let (queries, errors) =
                parse_queries(&declaration(&format!("```yaml\n{incomplete}\n```")));
            assert!(queries.is_empty());
            assert_eq!(errors.len(), 1);
            assert!(errors[0].contains(field), "{errors:?}");
        }
        for yaml in ["42", "[one, two]", "plain scalar"] {
            let body = format!("```yaml\n{yaml}\n```");
            let (queries, errors) = parse_queries(&declaration(&body));
            assert!(queries.is_empty(), "accepted non-mapping {body:?}");
            assert_eq!(errors.len(), 1, "{errors:?}");
            assert!(
                errors[0].contains("yaml block must be a mapping"),
                "{errors:?}"
            );
        }
        for body in [
            format!("```yaml\n{yaml}\n```"),
            format!("\n\n~~~yaml\n{yaml}\n~~~\n\n"),
        ] {
            let (queries, errors) = parse_queries(&declaration(&body));
            assert!(errors.is_empty(), "{body}: {errors:?}");
            assert_eq!(queries.len(), 1);
            assert_eq!(queries[0].name, "functions");
            assert_eq!(queries[0].body.language, "rust");
        }
        for body in [
            format!("```json\n{yaml}\n```"),
            format!("```text\n{yaml}\n```"),
            format!("```yml\n{yaml}\n```"),
            format!("```yaml\n{yaml}\n```\n```yaml\n{yaml}\n```"),
            format!("prose\n```yaml\n{yaml}\n```"),
            format!("```yaml\n{yaml}\n```\nprose"),
            format!("```yaml\n{yaml}\n~~~"),
            format!("``yaml\n{yaml}\n``"),
            yaml.to_owned(),
        ] {
            let (queries, errors) = parse_queries(&declaration(&body));
            assert!(queries.is_empty(), "accepted {body:?}");
            assert_eq!(errors.len(), 1, "{body}: {errors:?}");
            assert!(
                errors[0]
                    .contains("query functions must contain exactly one fenced yaml code block"),
                "{body}: {errors:?}"
            );
        }
    }
}

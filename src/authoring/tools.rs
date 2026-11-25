use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use grep_matcher::Matcher;
use serde_json::{Map, Value, json};

use super::inputs::AssessmentSources;
use super::pages::{Listing, ResultPages};
use crate::inputs::InputSelector;
use crate::structural::StructuralRule;

// {impl}`requirements.authoring.delivered_inputs`
pub(crate) struct ToolDelivery {
    pub(crate) result: Value,
    pub(crate) inputs: Vec<Value>,
}

// {impl}`requirements.authoring.file_discovery`
// {impl}`requirements.authoring.structural_search`
// {impl}`requirements.configuration.evidence_paths`
// {impl}`requirements.authoring.generated_inputs`
// {impl}`requirements.authoring.outline_tool`
// {impl}`requirements.authoring.reference_index`
// {impl}`requirements.authoring.github_reads`
// {impl}`requirements.authoring.finish_search`
// {impl}`requirements.authoring.finish_search.description`
// {impl}`requirements.authoring.tool_descriptions.common_case`
// {impl}`requirements.authoring.tool_descriptions.special_cases`
pub(crate) fn definitions() -> Vec<Value> {
    // Providers such as Anthropic require a plain object at the schema root.
    // Describe the alternative forms here and enforce them in read().
    let definitions = vec![
        json!({
            "name": "habeas",
            "description": "Read project source as evidence. Supply exactly one request form: file + line_start (optional line_end), file + item, requirements, or definition. Do not combine forms. Paths are project-relative. A line range is one-based and inclusive (omitted line_end reads through EOF) and returns {inputs, definitions}: the innermost outline nodes covering those lines, each with its item selector, kind, and source text, e.g. {\"file\":\"src/lib.rs\",\"line_start\":10,\"line_end\":20}. Line numbers only locate sources: results never give line numbers, and evidence records each node's selector and structural hash. Cite code by file and selector. The item form reads one outline node and returns {input, definitions}, e.g. {\"file\":\"src/lib.rs\",\"item\":\"impl[Thing]/fn[run]\"}; use the outline tool to list a file's selectors. Selectors are /-separated paths from the file's root: kind[name] for named nodes (Rust trait impls use impl[Thing/Trait]), kind@hash for anonymous nodes such as use declarations and plain-text lines (line@hash), and #N (one-based) for otherwise identical siblings; the empty item selects the whole file. Characters %, [, ], /, #, @ and whitespace in names are percent-encoded UTF-8 bytes. The requirements form takes a definition selection: a full requirement or example name, or a glob matching complete names where * matches any characters including dots, e.g. {\"requirements\":\"requirements.cli.*\"}. Definition reads use the full terminology name, e.g. {\"definition\":\"glossary.command\"}. The requirements and definition forms return {input, definitions}: the selected input and the terminology it needs.",
            "parameters": {"type":"object", "properties": {
                "file":{"type":"string"},
                "line_start":{"type":"integer","minimum":1},
                "line_end":{"type":"integer","minimum":1},
                "item":{"type":"string"},
                "requirements":{"type":"string"},
                "definition":{"type":"string"}
            }, "additionalProperties":false}
        }),
        json!({
            "name": "list_files", "description": "List the immediate files and directories of a project-relative directory (default .). The result is an array of {path, kind}, where kind is file or directory; returned paths can be listed or read in another call. Use list_files for directory entries and outline for selectable nodes in one file. Directory listings are not evidence inputs.",
            "parameters":{"type":"object","properties":{"path":{"type":"string","default":"."}},"additionalProperties":false}
        }),
        json!({
            "name":"outline", "description":"List a file's outline; file is a project-relative path. The result contains its selectable nodes nested by ancestry, in source order, each with its item selector (for habeas {file, item} reads), kind, and name (null when anonymous), and no line numbers. Rust, Python, TypeScript, CSS, Markdown, TOML, and YAML files have language outlines; other text files list one line node per nonblank line. Results contain no source text and are not evidence inputs; read the nodes you rely on with habeas.",
            "parameters":{"type":"object","properties":{"file":{"type":"string"}},"required":["file"],"additionalProperties":false}
        }),
        json!({
            "name":"ripgrep", "description":"Search a project-relative file or directory (default .) using ripgrep regex syntax and case-sensitive matching. Use ripgrep for text patterns, ast_grep for syntax-tree rules, and references for requirement-reference sites. Account for snippets you rely on with finish_search. The result is {search_id, matches: [{tag, file, item, kind, lines: [{text, matched}]}]}: one entry for each innermost outline node containing a match (a match in a comment belongs to the node the comment belongs to), with the node's item selector and kind, and its matching lines (matched true) plus context lines before and after each match (default 2; zero means matches only), in source order and without line numbers. Windows stop at item boundaries, overlapping windows are merged within each item, and a line with text \"…\" and matched false separates windows within one node. Results carry no definitions. Before submitting, call finish_search with the result UUID and the tags whose snippets you used to justify your conclusion without an additional source read. Entries whose source you also read may be omitted; including them is harmless. Use an empty used list if no entries qualify. Selected items become evidence dependencies without another source read. No matches returns an empty matches array. With referencing {selection, roles, scope}, search only sources within path that carry a reference in one of roles (impl, test, cite, or fixme; default [impl, test, cite]) to a requirement or example the selection matches (a full name, or a glob where * matches any characters including dots), including :impl: and :test: declaration anchors: scope node searches only the outline nodes that contain or are named by those references, and scope file (the default) their whole files, e.g. {\"pattern\":\"errors\",\"referencing\":{\"selection\":\"requirements.ci.*\",\"roles\":[\"impl\"],\"scope\":\"node\"}}.",
            "parameters":{"type":"object","properties":{"pattern":{"type":"string"},"path":{"type":"string","default":"."},"context":{"type":"integer","minimum":0,"default":2},"referencing":reference_scope_schema()},"required":["pattern"],"additionalProperties":false}
        }),
        json!({
            "name":"ast_grep", "description":"Search syntax trees in process with an ast-grep rule. Use ripgrep for text patterns, ast_grep for syntax-tree rules, and references for requirement-reference sites. Supply exactly one form: {language, rule, path (default .), referencing (optional)} or {query}. language is one of rust, python, typescript, tsx, css, markdown, toml, or yaml, matched with Habeas's pinned grammars; only files of that language are searched. rule is an object holding the keys of an ast-grep rule file's rule core, written as JSON: rule (required), and optional utils (named rules) and constraints (metavariable name to rule). A rule object combines atomic rules: pattern (code with metavariables, where $A matches one node, $$$A any number of nodes, and $_ one node without capturing), kind (a grammar node kind such as function_item), regex (a Rust regex the node's whole text must match), and nthChild; relational rules: inside, has, precedes, and follows, each taking a rule plus optional stopBy (neighbor, the default, end, or a rule) and field; and composite rules: all, any, not, and matches (a utils name). Patterns parse as standalone code, so a fragment that does not parse on its own, such as a type or parameter, needs the context and selector form: pattern: {context: \"fn f(x: &mut Vec<String>) {}\", selector: reference_type} parses context and matches the node of kind selector within it. Example: {\"language\":\"rust\",\"rule\":{\"rule\":{\"pattern\":\"$X.unwrap()\",\"inside\":{\"kind\":\"function_item\",\"stopBy\":\"end\"}}}}. path selects a project-relative file or directory (default .); referencing works as for ripgrep, and results are ordered by path and then line. The query form, e.g. {\"query\":\"requirements.ci.errors/error_accumulators\"}, runs a query a distributed requirement declares, named <full-name>/<query-name>, exactly as validation runs it, including its paths, exclude, and within, and returns each match's site. The result has ripgrep's form, {search_id, matches: [{tag, file, item, kind, lines: [{text, matched}]}]}: one entry for each innermost outline node containing a match (a match in a comment belongs to the node the comment belongs to; a query match belongs to its site), with the node's item selector and kind, and the matched lines, without line numbers. Results carry no definitions. Before submitting, call finish_search with the result UUID and the tags whose snippets you used to justify your conclusion without an additional source read. Entries whose source you also read may be omitted; including them is harmless. Use an empty used list if no entries qualify. Selected items become evidence dependencies without another source read. No matches returns an empty matches array.",
            "parameters":{"type":"object","properties":{"language":{"type":"string","enum":crate::structural::QueryLanguage::names()},"rule":{"type":"object"},"path":{"type":"string","default":"."},"referencing":reference_scope_schema(),"query":{"type":"string"}},"additionalProperties":false}
        }),
        json!({
            "name":"references", "description":"Locate definitions and reference sites without reading their text. Without selection, return every requirement and named example name with its kind and empty sites. With selection (a full name or a glob where * matches any characters including dots), return the selected names/kinds plus reference sites: definition name, role, file, and item selector, without line numbers. roles filters sites and anchors by impl, cite, or test (default all three); {impl} marks implementations, {cite} related sources, and {test} tests. For example {\"selection\":\"requirements.cli.status\",\"roles\":[\"impl\"]} locates only implementations. Repeated roles have no effect; [] returns names/kinds only. An :impl: or :test: anchor is reported with item null for a whole file or its selector for a node. Results contain no requirement text, example text, commentary, or source text and are not evidence inputs. Request needed source with habeas to read and record it.",
            "parameters":{"type":"object","properties":{
                "selection":{"type":"string"},
                "roles":{"type":"array","items":{"type":"string","enum":["impl","cite","test"]},"default":["impl","cite","test"]}
            },"additionalProperties":false}
        }),
        crate::github::tool_definition(),
        json!({
            "name":"next_page", "description":"Return the following page of a paged result of habeas (line-range form), list_files, outline, ripgrep, ast_grep, or references, in the same shape as its first page. token is the page's next value, e.g. {\"token\":\"abc123\"}. Tokens are opaque.",
            "parameters":{"type":"object","properties":{"token":{"type":"string"}},"required":["token"],"additionalProperties":false}
        }),
        json!({
            "name":"finish_search",
            "description":"Close a delivered search and record dependencies for snippets you used to justify your conclusion without an additional source read. search_id is the result's UUID and used is an array of its short entry tags, e.g. {\"search_id\":\"11111111-1111-4111-8111-111111111111\",\"used\":[\"1\"]}. Entries whose source you also read may be omitted; including them is harmless. Use used:[] when no entries qualify, including an empty search. Every delivered search must be finished before submission. The result is {search_id, finished:true}; no additional source is delivered.",
            "parameters":{"type":"object","properties":{
                "search_id":{"type":"string","format":"uuid"},
                "used":{"type":"array","items":{"type":"string"}}
            },"required":["search_id","used"],"additionalProperties":false}
        }),
    ];
    definitions
}

/// The parameter schema of a `referencing` argument.
// {cite}`habeas:util`
fn reference_scope_schema() -> Value {
    json!({"type":"object","properties":{
        "selection":{"type":"string"},
        "roles":{"type":"array","items":{"type":"string","enum":["impl","test","cite","fixme"]},"default":["impl","test","cite"]},
        "scope":{"type":"string","enum":["node","file"],"default":"file"}
    },"required":["selection"],"additionalProperties":false})
}

// {impl}`requirements.authoring.file_discovery`
// {impl}`requirements.authoring.structural_search`
// {impl}`requirements.authoring.outline_tool`
// {impl}`requirements.authoring.reference_index`
// {impl}`requirements.authoring.github_reads`
pub(crate) fn dispatch_paged(
    sources: &AssessmentSources,
    pages: &mut ResultPages,
    name: &str,
    arguments: &Value,
) -> io::Result<ToolDelivery> {
    let arguments = arguments
        .as_object()
        .ok_or_else(|| io::Error::other("tool arguments must be an object"))?;
    let mut deliver = |listing: io::Result<Listing>| pages.deliver(sources, listing?);
    match name {
        "habeas" => read(sources, arguments).and_then(|read| match read {
            Ok(delivery) => Ok(delivery),
            Err(listing) => deliver(Ok(listing)),
        }),
        "list_files" => {
            deliver(list_files(sources, arguments).and_then(|listing| {
                with_not_listed(sources, arguments, NotListed::Children, listing)
            }))
        }
        "outline" => deliver(outline(sources, arguments)),
        "ripgrep" => deliver(search(sources, arguments).and_then(|listing| {
            let pattern = regex::Regex::new(string(arguments, "pattern")?).ok();
            with_not_listed(sources, arguments, NotListed::Text(pattern), listing)
        })),
        "ast_grep" => deliver(structural(sources, arguments).and_then(|listing| {
            let rule = arguments
                .get("language")
                .and_then(Value::as_str)
                .zip(arguments.get("rule").and_then(Value::as_object))
                .and_then(|(language, rule)| StructuralRule::compile(language, rule).ok());
            with_not_listed(sources, arguments, NotListed::Rule(rule), listing)
        })),
        "references" => deliver(references(sources, arguments)),
        "next_page" => check_keys(arguments, &["token"])
            .and_then(|()| pages.next_page(string(arguments, "token")?)),
        "finish_search" => pages.finish_search(&Value::Object(arguments.clone())),
        // GitHub reads are not evidence inputs.
        "github" => crate::github::tool(&sources.github, &Value::Object(arguments.clone()))
            .map(|result| ToolDelivery {
                result,
                inputs: Vec::new(),
            })
            .map_err(io::Error::other),
        _ => Err(io::Error::other(format!("unknown read tool {name:?}"))),
    }
    .map(|delivery| with_redaction_note(sources, delivery))
    .map_err(super::access::recoverable_os_denial)
    .map_err(super::access::recoverable_outside_request)
}

// {cite}`habeas:util`
/// Which untracked files a result names under `not_listed`.
enum NotListed {
    /// The requested directory's entries that would otherwise be listed.
    Children,
    /// Files under the path with a line matching the pattern.
    Text(Option<regex::Regex>),
    /// Files under the path, of the rule's language, with a match.
    Rule(Option<StructuralRule>),
}

/// Adds `not_listed` to a listing or search result: each untracked file
/// under the requested path that the listing would include, or that has a
/// match, by its project-relative path and without its contents. Searches
/// scoped by `referencing` only consider files reference discovery reads,
/// which excludes untracked ones.
// {impl}`requirements.authoring.not_listed`
fn with_not_listed(
    sources: &AssessmentSources,
    arguments: &Map<String, Value>,
    kind: NotListed,
    listing: Listing,
) -> io::Result<Listing> {
    // This is the first page; later pages come from next_page.
    if arguments.contains_key("referencing") {
        return Ok(listing);
    }
    let requested_path = project_path(sources, optional_path(arguments)?)?;
    let requested = relative_path(&sources.root, &requested_path)?;
    let requested = requested.trim_end_matches('/');
    let untracked = super::access::load_discovery_sources(&sources.root)?.untracked()?;
    let mut named = BTreeSet::new();
    for file in untracked {
        let Ok(relative) = file.strip_prefix(&sources.root) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        let inside = if requested.is_empty() {
            Some(relative.as_str())
        } else {
            relative
                .strip_prefix(requested)
                .and_then(|rest| rest.strip_prefix('/'))
                .or_else(|| (relative == requested).then_some(""))
        };
        let Some(inside) = inside else {
            continue;
        };
        let name = match &kind {
            NotListed::Children => {
                let child = inside.split('/').next().unwrap_or_default();
                if child.is_empty() {
                    continue;
                }
                if requested.is_empty() {
                    child.to_owned()
                } else {
                    format!("{requested}/{child}")
                }
            }
            NotListed::Text(pattern) => {
                let Some(pattern) = pattern else { continue };
                let Ok(text) = fs::read_to_string(&file) else {
                    continue;
                };
                if !text.lines().any(|line| pattern.is_match(line)) {
                    continue;
                }
                relative.clone()
            }
            NotListed::Rule(rule) => {
                let Some(rule) = rule else { continue };
                if !rule.language().covers(&file) {
                    continue;
                }
                let Ok(text) = fs::read_to_string(&file) else {
                    continue;
                };
                if rule.matches(&text).is_empty() {
                    continue;
                }
                relative.clone()
            }
        };
        named.insert(name);
    }
    // A directory listing already shows a child it lists.
    let listed: BTreeSet<&str> = listing
        .values()
        .filter_map(|entry| entry.get("path").and_then(Value::as_str))
        .collect();
    let not_listed: Vec<Value> = named
        .iter()
        .filter(|name| !listed.contains(name.as_str()))
        .map(|path| json!({"path": path, "reason": "untracked"}))
        .collect();
    if not_listed.is_empty() {
        return Ok(listing);
    }
    // Start on the first page; large arrays continue through ordinary paging.
    Ok(listing.with_not_listed(not_listed))
}

/// What a result containing redacted text tells the model, since tool
/// descriptions do not explain redaction.
pub(crate) const REDACTION_NOTE: &str = "[REDACTED:<kind>] replaces credential-shaped text in this result; the original text is not available.";

/// Adds the redaction note to a result that contains redacted text, and only
/// to such a result. A line showing the marker is redacted unless it appears
/// verbatim in its file's original source, where the marker is literal text.
// {impl}`requirements.authoring.redaction_notice`
// {impl}`requirements.authoring.tool_descriptions.special_cases`
fn with_redaction_note(sources: &AssessmentSources, mut delivery: ToolDelivery) -> ToolDelivery {
    if needs_redaction_note(sources, &delivery.result)
        && let Some(object) = delivery.result.as_object_mut()
    {
        object.insert("note".to_owned(), json!(REDACTION_NOTE));
    }
    delivery
}

/// Whether a result contains text the harness redacted.
// {impl}`requirements.authoring.redaction_notice`
pub(crate) fn needs_redaction_note(sources: &AssessmentSources, result: &Value) -> bool {
    if !result.to_string().contains("[REDACTED:") {
        return false;
    }
    let mut redacted = false;
    // Requirement and definition text carries no file; its lines come from
    // the book's raw declarations.
    let declarations = std::cell::OnceCell::new();
    marked_lines(result, None, &mut |file, line| {
        let literal = match file {
            Some(file) => {
                let parsed = sources.outlines.parse(&sources.root, file);
                parsed
                    .as_ref()
                    .as_ref()
                    .is_ok_and(|parsed| parsed.source.lines().any(|source| source == line))
            }
            None => declarations
                .get_or_init(|| {
                    crate::evidence::load_definitions(&sources.root).unwrap_or_default()
                })
                .values()
                .any(|declaration| declaration.raw_lines.iter().any(|raw| raw == line)),
        };
        redacted |= !literal;
    });
    redacted
}

/// Calls `visit` with each string in `value` showing the redaction marker,
/// and the file of the nearest enclosing entry that names one.
fn marked_lines<'a>(
    value: &'a Value,
    file: Option<&'a str>,
    visit: &mut dyn FnMut(Option<&'a str>, &'a str),
) {
    match value {
        Value::String(text) if text.contains("[REDACTED:") => visit(file, text),
        Value::Array(values) => {
            for value in values {
                marked_lines(value, file, visit);
            }
        }
        Value::Object(object) => {
            let file = object.get("file").and_then(Value::as_str).or(file);
            for value in object.values() {
                marked_lines(value, file, visit);
            }
        }
        _ => {}
    }
}

/// Dispatches one tool call in an assessment of its own, whose pages are
/// dropped once the call returns.
#[cfg(test)]
// {cite}`habeas:util`
pub(crate) fn dispatch(
    sources: &AssessmentSources,
    name: &str,
    arguments: &Value,
) -> io::Result<ToolDelivery> {
    dispatch_paged(sources, &mut ResultPages::default(), name, arguments)
}

/// A read's delivery, or, for the line-range form, its result to page.
// {impl}`requirements.authoring.delivered_inputs`
fn read(
    sources: &AssessmentSources,
    args: &Map<String, Value>,
) -> io::Result<Result<ToolDelivery, Listing>> {
    let selector = if args.contains_key("item") {
        check_keys(args, &["file", "item"])?;
        InputSelector::Outline {
            file: string(args, "file")?.to_owned(),
            item: string(args, "item")?.to_owned(),
        }
    } else if args.contains_key("file") {
        check_keys(args, &["file", "line_start", "line_end"])?;
        let file = string(args, "file")?;
        let line_start = integer(args, "line_start")?;
        let line_end = if args.contains_key("line_end") {
            Some(integer(args, "line_end")?)
        } else {
            None
        };
        let inputs = sources.read_lines(file, line_start, line_end, false)?;
        return Ok(Err(Listing::inputs(inputs)));
    } else if args.contains_key("requirements") {
        check_keys(args, &["requirements"])?;
        InputSelector::Requirements {
            requirements: string(args, "requirements")?.to_owned(),
        }
    } else {
        check_keys(args, &["definition"])?;
        InputSelector::Definition {
            definition: string(args, "definition")?.to_owned(),
        }
    };
    let input = sources.read_selector(&selector)?;
    let definitions = sources.additional_definitions(std::slice::from_ref(&input))?;
    let result = json!({ "input": input, "definitions": definitions });
    let mut inputs = vec![input];
    inputs.extend(definitions);
    Ok(Ok(ToolDelivery { result, inputs }))
}

// {impl}`requirements.authoring.outline_tool`
// {impl}`requirements.references.outlines`
fn outline(sources: &AssessmentSources, args: &Map<String, Value>) -> io::Result<Listing> {
    check_keys(args, &["file"])?;
    let file = string(args, "file")?;
    sources.access.file(&sources.root, Path::new(file))?;
    let snapshot = sources
        .outlines
        .snapshot(&sources.root, file)
        .map_err(io::Error::other)?;
    let outline = snapshot.outline();
    let items = crate::outline_items::OutlineItems::new(outline, snapshot.text());
    // Configuration redaction can give anonymous items different evidence addresses.
    let config = outline.items.iter().any(|i| i.selector.contains('@'))
        && crate::redaction::is_configuration_file_for(
            &snapshot.id().path,
            crate::config::user_config_path().ok().as_deref(),
        );
    let legacy = config.then(|| sources.outlines.parse(&sources.root, file));
    let evidence = legacy
        .as_deref()
        .map(Result::as_ref)
        .transpose()
        .map_err(|e| io::Error::other(e.clone()))?;
    Ok(Listing::array(outline_entries(
        outline, &items, evidence, 0,
    )))
}

/// Render names from the selector, including normalized and multipart names.
// {impl}`requirements.authoring.outline_tool`
// {impl}`requirements.authoring.no_line_numbers`
fn outline_entries(
    outline: &trust_but_verify::outline::Outline,
    items: &crate::outline_items::OutlineItems,
    evidence: Option<&crate::inputs::ParsedSource>,
    parent: usize,
) -> Vec<Value> {
    let mut entries = Vec::new();
    for &child in items.children(parent) {
        let item = &outline.items[child];
        let selector = evidence.map_or(&item.selector, |p| &p.outline.nodes[child].selector);
        let name = crate::outline_items::display_name(selector);
        entries.push(json!({
            "item": selector, "kind": item.kind, "name": name,
            "children": outline_entries(outline, items, evidence, child),
        }));
    }
    entries
}

// {impl}`requirements.authoring.file_discovery`
// {impl}`requirements.configuration.evidence_paths`
// {impl}`requirements.authoring.generated_inputs`
fn list_files(sources: &AssessmentSources, args: &Map<String, Value>) -> io::Result<Listing> {
    check_keys(args, &["path"])?;
    let path = optional_path(args)?;
    let directory = project_path(sources, path)?;
    let mut entries = Vec::new();
    for entry in fs::read_dir(&directory)
        .map_err(|error| io::Error::new(error.kind(), format!("cannot list {path}: {error}")))?
    {
        let entry = entry.map_err(|error| {
            io::Error::new(error.kind(), format!("cannot list {path}: {error}"))
        })?;
        let relative = relative_path(&sources.root, &entry.path())?;
        if sources
            .access
            .path(&sources.root, Path::new(&relative))
            .is_err()
        {
            continue;
        }
        let metadata = fs::metadata(entry.path()).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("cannot inspect {}: {error}", entry.path().display()),
            )
        })?;
        let kind = if metadata.is_dir() {
            "directory"
        } else if metadata.is_file() {
            "file"
        } else {
            continue;
        };
        entries.push((relative, kind));
    }
    if directory == sources.root {
        for extra in sources.access.extra_roots() {
            if sources.access.path(&sources.root, extra).is_err() {
                continue;
            }
            let kind = if extra.is_dir() {
                "directory"
            } else if extra.is_file() {
                "file"
            } else {
                continue;
            };
            entries.push((relative_path(&sources.root, extra)?, kind));
        }
    }
    entries.sort();
    entries.dedup();
    Ok(Listing::array(
        entries
            .into_iter()
            .map(|(path, kind)| json!({"path":path,"kind":kind}))
            .collect(),
    ))
}

/// The entry arrays of a `references` result, in order.
// {impl}`requirements.authoring.reference_index`
// {impl}`requirements.authoring.result_pages`
const REFERENCE_ARRAYS: &[&str] = &["definitions", "sites"];

// {impl}`requirements.references.definition_globs`
// {impl}`requirements.references.anchors`
// {impl}`requirements.authoring.no_line_numbers`
// {impl}`requirements.references.authoring_inputs`
fn references(sources: &AssessmentSources, args: &Map<String, Value>) -> io::Result<Listing> {
    check_keys(args, &["selection", "roles"])?;
    let roles = match args.get("roles") {
        None => vec![
            crate::ReferenceKind::Impl,
            crate::ReferenceKind::Cite,
            crate::ReferenceKind::Test,
        ],
        Some(Value::Array(roles)) => roles
            .iter()
            .map(|role| match role.as_str() {
                Some("impl") => Ok(crate::ReferenceKind::Impl),
                Some("cite") => Ok(crate::ReferenceKind::Cite),
                Some("test") => Ok(crate::ReferenceKind::Test),
                _ => Err(io::Error::other(format!(
                    "invalid reference role {role}; expected impl, cite, or test"
                ))),
            })
            .collect::<io::Result<Vec<_>>>()?,
        Some(_) => {
            return Err(io::Error::other(
                "roles must be an array of impl, cite, or test",
            ));
        }
    };
    let selection = if args.contains_key("selection") {
        let selection = string(args, "selection")?;
        crate::definition_glob::validate(selection).map_err(io::Error::other)?;
        Some(selection)
    } else {
        None
    };
    let definitions = crate::evidence::load_definitions(&sources.root)?;
    let selected: Vec<_> = definitions
        .iter()
        .filter(|(name, definition)| {
            definition.kind != crate::DefinitionKind::Term
                && selection
                    .is_none_or(|selection| crate::definition_glob::matches(selection, name))
        })
        .map(|(name, definition)| json!({"name": name, "kind": definition.kind.noun()}))
        .collect();
    let Some(selection) = selection else {
        return Ok(Listing::object(
            REFERENCE_ARRAYS,
            vec![selected, Vec::new()],
        ));
    };
    if selected.is_empty() {
        return Err(io::Error::other(format!(
            "selection {selection:?} matches no requirement or named example"
        )));
    }
    let mut errors = Vec::new();
    let mut found = crate::evidence::collect_references(&sources.root, &mut errors);
    // A considered source that cannot be read would silently drop its sites.
    if !errors.is_empty() {
        return Err(io::Error::other(format!(
            "cannot list reference sites because reference discovery failed; fix or restore the named source before retrying:\n{}",
            errors.join("\n")
        )));
    }
    found.retain(|reference| {
        // Only `{impl}`, `{cite}`, and `{test}` references are sites; a
        // `{fixme}` acknowledges a gap rather than locating the definition.
        roles.contains(&reference.kind)
            // A fence that never closes is a validation error, not a site.
            && reference.closed
            && crate::definition_glob::matches(selection, &reference.name)
            && definitions
                .get(&reference.name)
                .is_some_and(|definition| definition.kind != crate::DefinitionKind::Term)
    });
    let mut sites = Vec::new();
    for reference in &found {
        let file = relative_path(&sources.root, &reference.file)?;
        let parsed = sources.outlines.parse(&sources.root, &file);
        let item = parsed.as_ref().as_ref().ok().map(|parsed| {
            let index = parsed.reference_owner_at_line(reference.line);
            parsed.outline.nodes[index].selector.clone()
        });
        sites.push((
            reference.name.clone(),
            file,
            Some(reference.line),
            json!({
                "name": reference.name,
                "role": reference.kind.label(),
                "file": relative_path(&sources.root, &reference.file)?,
                "item": item,
            }),
        ));
    }
    // {impl}`requirements.references.anchors`
    for (name, definition) in &definitions {
        if definition.kind == crate::DefinitionKind::Term
            || !crate::definition_glob::matches(selection, name)
        {
            continue;
        }
        for anchor in &definition.anchors {
            if !roles.contains(&anchor.kind) {
                continue;
            }
            // Discovery omits excluded sources rather than aborting.
            let Ok(path) = sources.access.file(&sources.root, Path::new(&anchor.file)) else {
                continue;
            };
            let file = relative_path(&sources.root, &path)?;
            sites.push((
                name.clone(),
                file.clone(),
                None,
                json!({
                    "name": name,
                    "role": anchor.kind.label(),
                    "file": file,
                    "item": (!anchor.item.is_empty()).then_some(&anchor.item),
                }),
            ));
        }
    }
    sites.sort_by(|a, b| (&a.0, &a.1, a.2).cmp(&(&b.0, &b.1, b.2)));
    Ok(Listing::object(
        REFERENCE_ARRAYS,
        vec![selected, sites.into_iter().map(|site| site.3).collect()],
    ))
}

// {impl}`requirements.configuration.evidence_paths`
// {impl}`requirements.authoring.generated_inputs`
fn search(sources: &AssessmentSources, args: &Map<String, Value>) -> io::Result<Listing> {
    check_keys(args, &["pattern", "path", "context", "referencing"])?;
    let pattern = string(args, "pattern")?;
    let path = optional_path(args)?;
    let context = if args.contains_key("context") {
        integer(args, "context")?
    } else {
        2
    };
    let regions = referencing(sources, args)?;
    let matcher = grep_regex::RegexMatcherBuilder::new()
        .case_insensitive(false)
        .build(pattern)
        .map_err(|error| io::Error::other(format!("invalid ripgrep pattern: {error}")))?;
    let matches = walk_sources(
        sources,
        path,
        &|file, _| regions.as_ref().is_none_or(|regions| regions.has(file)),
        &|file, path| {
            let mut searcher = grep_searcher::SearcherBuilder::new()
                .line_number(true)
                .binary_detection(grep_searcher::BinaryDetection::quit(0))
                .build();
            let mut lines = Vec::new();
            // Do not parse binary or unrelated sources merely to discover
            // whether they match. Once a file qualifies, search the cached
            // snapshot again so match ranges, excerpts, and hashes agree.
            searcher.search_path(
                &matcher,
                path,
                grep_searcher::sinks::Lossy(|number, _| {
                    lines.push(number as usize);
                    Ok(true)
                }),
            )?;
            if lines.is_empty() {
                return Ok(None);
            }
            let parsed = sources.parse_file(file)?;
            let parsed = parsed
                .as_ref()
                .as_ref()
                .map_err(|error| io::Error::other(error.clone()))?;
            lines.clear();
            searcher
                .search_slice(
                    &matcher,
                    parsed.source.as_bytes(),
                    grep_searcher::sinks::Lossy(|number, _| {
                        lines.push(number as usize);
                        Ok(true)
                    }),
                )
                .map_err(|error| {
                    io::Error::new(
                        error.kind(),
                        format!("ripgrep failed to search {}: {error}", path.display()),
                    )
                })?;
            Ok((!lines.is_empty()).then_some(lines))
        },
    )?;
    let mut entries = Vec::new();
    for (file, mut lines) in matches {
        let parsed = sources.parse_file(&file)?;
        let parsed = parsed
            .as_ref()
            .as_ref()
            .map_err(|error| io::Error::other(error.clone()))?;
        let count = parsed.source.lines().count();
        let source_lines = parsed.source.split_inclusive('\n').collect::<Vec<_>>();
        let mut offset = 0;
        let line_offsets = source_lines
            .iter()
            .map(|line| {
                let start = offset;
                offset += line.len();
                start
            })
            .collect::<Vec<_>>();
        lines.sort_unstable();
        lines.dedup();
        let mut nodes = SearchNodes::default();
        for line in lines {
            if line == 0 || line > count {
                return Err(io::Error::other(format!(
                    "{file}: ripgrep returned an out-of-range match"
                )));
            }
            // With node-scoped `referencing`, only lines within a referencing
            // node are searched, and windows stop at its boundaries.
            let (first, last) = match regions
                .as_ref()
                .map(|regions| regions.region(&file, line, line))
            {
                None | Some(Some(None)) => (1, count),
                Some(Some(Some(region))) => region,
                Some(None) => continue,
            };
            let start = line.saturating_sub(context).max(first);
            let end = line.saturating_add(context).min(last);
            matcher
                .find_iter(
                    source_lines[line - 1].trim_end_matches('\n').as_bytes(),
                    |found| {
                        let range = (line_offsets[line - 1] + found.start())
                            ..(line_offsets[line - 1] + found.end());
                        let index = parsed.outline.ownership.owner(&range);
                        nodes.add(index, line..=line, start..=end);
                        true
                    },
                )
                .map_err(io::Error::other)?;
        }
        entries.extend(nodes.entries(&file, parsed));
    }
    Ok(Listing::object(&["matches"], vec![entries]))
}

/// The line separating windows within one search entry.
const ELISION: &str = "…";

// {cite}`habeas:util`
/// The entries of one file's search result being collected: each innermost
/// outline node containing a match, with its matching lines and the context
/// lines around them.
#[derive(Default)]
struct SearchNodes(BTreeMap<usize, (BTreeSet<usize>, BTreeSet<usize>)>);

impl SearchNodes {
    // {impl}`requirements.authoring.text_search`
    /// Adds a match on `lines` in a node, with `context` lines around it.
    fn add(
        &mut self,
        index: usize,
        lines: std::ops::RangeInclusive<usize>,
        context: std::ops::RangeInclusive<usize>,
    ) {
        let (matched, around) = self.0.entry(index).or_default();
        matched.extend(lines);
        around.extend(context);
    }

    /// The entries in source order: each node's file, selector, and kind,
    /// and its lines' redacted text, each marked as matching or not, with an
    /// elision line between separate windows and no line numbers. Context
    /// is clipped to each item, including boundaries within a physical line.
    // {impl}`requirements.authoring.redaction`
    // {impl}`requirements.references.outline_selectors`
    // {impl}`requirements.authoring.text_search`
    // {impl}`requirements.authoring.no_line_numbers`
    // {impl}`requirements.authoring.search_accounting.excerpts`
    fn entries(self, file: &str, parsed: &crate::inputs::ParsedSource) -> Vec<Value> {
        let redaction = crate::redaction::Redaction::of_file(&parsed.path, &parsed.source);
        let mut offset = 0;
        let text = parsed
            .source
            .split_inclusive('\n')
            .map(|line| {
                let start = offset;
                offset += line.len();
                let body = line.trim_end_matches('\n').trim_end_matches('\r');
                (start, start + body.len())
            })
            .collect::<Vec<_>>();
        let mut nodes = self.0.into_iter().collect::<Vec<_>>();
        nodes.sort_by_key(|(index, _)| {
            let node = &parsed.outline.nodes[*index];
            (node.line_start, node.byte_start, *index)
        });
        nodes
            .into_iter()
            .map(|(index, (matched, around))| {
                let node = &parsed.outline.nodes[index];
                let mut first = node.source_start;
                let mut last = node.byte_end;
                for comment in parsed
                    .outline
                    .ownership
                    .comments
                    .iter()
                    .filter(|comment| comment.owner == index)
                {
                    first = first.min(comment.span.start);
                    last = last.max(comment.span.end);
                }
                let lines = matched.union(&around);
                let mut shown_lines = Vec::new();
                let mut previous = None;
                for line in lines {
                    let Some(&(start, end)) = line.checked_sub(1).and_then(|line| text.get(line))
                    else {
                        continue;
                    };
                    let (start, end) = (start.max(first), end.min(last));
                    if start > end || end < first || start > last {
                        continue;
                    }
                    if previous.is_some_and(|previous: usize| previous + 1 < *line) {
                        shown_lines.push(json!({"text": ELISION, "matched": false}));
                    }
                    previous = Some(*line);
                    shown_lines.push(json!({
                        "text": redaction.slice(&parsed.source, start..end),
                        "matched": matched.contains(line),
                    }));
                }
                json!({
                    "file": file,
                    "item": node.selector,
                    "kind": if index == 0 { "file" } else { node.kind.as_str() },
                    "lines": shown_lines,
                })
            })
            .collect()
    }
}

/// Walks the permitted files under a requested path, and under the explicitly
/// allowed and generated-output roots it contains or lies in, visiting each
/// file `include` accepts and collecting the results `visit` returns by
/// project-relative path. Source-access policy is applied while walking, so
/// denied files are never opened.
// {impl}`requirements.configuration.evidence_paths`
// {impl}`requirements.authoring.generated_inputs`
fn walk_sources<T: Send>(
    sources: &AssessmentSources,
    path: &str,
    include: &(dyn Fn(&str, &Path) -> bool + Sync),
    visit: &(dyn Fn(&str, &Path) -> io::Result<Option<T>> + Sync),
) -> io::Result<BTreeMap<String, T>> {
    let requested = project_path(sources, path)?;
    let mut searches = vec![(requested.clone(), false)];
    for extra in sources.access.extra_roots() {
        let Ok(extra) = sources.access.path(&sources.root, extra) else {
            continue;
        };
        let candidate = if requested.starts_with(&extra) {
            requested.clone()
        } else if extra.starts_with(&requested) || requested == sources.root {
            extra.clone()
        } else {
            continue;
        };
        if sources.access.path(&sources.root, &candidate).is_ok() {
            searches.push((candidate, true));
        }
    }
    searches.sort();
    searches.dedup();
    let found = Mutex::new(BTreeMap::<String, T>::new());
    let failure = Mutex::new(None::<io::Error>);
    for (path, include_ignored) in searches {
        let mut walker = ignore::WalkBuilder::new(&path);
        walker
            .hidden(!include_ignored)
            .ignore(!include_ignored)
            .git_ignore(!include_ignored)
            .git_global(!include_ignored)
            .git_exclude(!include_ignored)
            .parents(!include_ignored)
            .add_custom_ignore_filename(".rgignore");
        walker.build_parallel().run(|| {
            let found = &found;
            let failure = &failure;
            Box::new(move |entry| {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        *failure.lock().expect("search failure lock") = Some(io::Error::other(
                            format!("cannot traverse source files: {error}"),
                        ));
                        return ignore::WalkState::Quit;
                    }
                };
                if entry.file_type().is_some_and(|kind| kind.is_dir())
                    && sources.access.path(&sources.root, entry.path()).is_err()
                {
                    return ignore::WalkState::Skip;
                }
                if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                    return ignore::WalkState::Continue;
                }
                // Apply source-access policy while walking: denied files are
                // never opened.
                if sources.access.file(&sources.root, entry.path()).is_err() {
                    return ignore::WalkState::Continue;
                }
                let visited = relative_path(&sources.root, entry.path()).and_then(|file| {
                    if include(&file, entry.path()) {
                        Ok(visit(&file, entry.path())?.map(|result| (file, result)))
                    } else {
                        Ok(None)
                    }
                });
                match visited {
                    Ok(Some((file, result))) => {
                        found
                            .lock()
                            .expect("search results lock")
                            .insert(file, result);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        *failure.lock().expect("search failure lock") = Some(error);
                        return ignore::WalkState::Quit;
                    }
                }
                ignore::WalkState::Continue
            })
        });
    }
    if let Some(error) = failure.into_inner().expect("search failure lock") {
        return Err(error);
    }
    // Parallel walks finish in any order; the map makes results independent
    // of scheduling.
    Ok(found.into_inner().expect("search results lock"))
}

// {cite}`habeas:util`
/// The sources a `referencing` scope restricts a search to, by
/// project-relative file: the inclusive line ranges of each referencing
/// outline node, or `None` for the whole file.
struct ReferenceRegions(BTreeMap<String, Option<Vec<(usize, usize)>>>);

impl ReferenceRegions {
    // {impl}`requirements.authoring.text_search`
    /// Whether a file carries a selected reference.
    fn has(&self, file: &str) -> bool {
        self.0.contains_key(file)
    }

    // {impl}`requirements.authoring.text_search`
    /// The searched region containing an inclusive line range: `None` when
    /// none does, `Some(None)` for a whole file, and otherwise the smallest
    /// referencing node's lines.
    fn region(&self, file: &str, first: usize, last: usize) -> Option<Option<(usize, usize)>> {
        match self.0.get(file)? {
            None => Some(None),
            Some(regions) => regions
                .iter()
                .filter(|(start, end)| *start <= first && last <= *end)
                .min_by_key(|(start, end)| end - start)
                .map(|region| Some(*region)),
        }
    }

    // {impl}`requirements.authoring.text_search`
    /// Adds a referencing node's lines, or a whole file.
    fn add(&mut self, file: String, lines: Option<(usize, usize)>) {
        let entry = self.0.entry(file).or_insert_with(|| Some(Vec::new()));
        match (entry, lines) {
            (Some(regions), Some(lines)) => regions.push(lines),
            (entry, None) => *entry = None,
            (None, Some(_)) => {}
        }
    }
}

/// The regions an optional `referencing` argument selects: the sources that
/// carry a reference in one of its roles to a requirement or example its
/// selection matches, including declaration anchors, as referencing nodes or
/// whole files by its scope.
// {impl}`requirements.references.definition_globs`
// {impl}`requirements.references.anchors`
fn referencing(
    sources: &AssessmentSources,
    args: &Map<String, Value>,
) -> io::Result<Option<ReferenceRegions>> {
    let Some(scope) = args.get("referencing") else {
        return Ok(None);
    };
    let scope = scope
        .as_object()
        .ok_or_else(|| io::Error::other("referencing must be an object"))?;
    for key in scope.keys() {
        if !["selection", "roles", "scope"].contains(&key.as_str()) {
            return Err(io::Error::other(format!(
                "unexpected referencing argument {key:?}"
            )));
        }
    }
    let selection = scope
        .get("selection")
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("referencing.selection must be a string"))?;
    crate::definition_glob::validate(selection).map_err(io::Error::other)?;
    let roles = match scope.get("roles") {
        None => vec![
            crate::ReferenceKind::Impl,
            crate::ReferenceKind::Test,
            crate::ReferenceKind::Cite,
        ],
        Some(Value::Array(roles)) => roles
            .iter()
            .map(|role| match role.as_str() {
                Some("impl") => Ok(crate::ReferenceKind::Impl),
                Some("test") => Ok(crate::ReferenceKind::Test),
                Some("cite") => Ok(crate::ReferenceKind::Cite),
                Some("fixme") => Ok(crate::ReferenceKind::Fixme),
                _ => Err(io::Error::other(format!(
                    "unknown referencing role {role}; expected impl, test, cite, or fixme"
                ))),
            })
            .collect::<io::Result<_>>()?,
        Some(_) => {
            return Err(io::Error::other(
                "referencing.roles must be an array of roles",
            ));
        }
    };
    let whole_files = match scope.get("scope") {
        None => true,
        Some(value) => match value.as_str() {
            Some("file") => true,
            Some("node") => false,
            _ => {
                return Err(io::Error::other(format!(
                    "unknown referencing scope {value}; expected \"node\" or \"file\""
                )));
            }
        },
    };
    let definitions = crate::evidence::load_definitions(&sources.root)?;
    let selected = |name: &str| {
        crate::definition_glob::matches(selection, name)
            && definitions
                .get(name)
                .is_some_and(|definition| definition.kind != crate::DefinitionKind::Term)
    };
    if !definitions.keys().any(|name| selected(name)) {
        return Err(io::Error::other(format!(
            "referencing selection {selection:?} matches no requirement or named example"
        )));
    }
    let mut errors = Vec::new();
    let found = crate::evidence::collect_references(&sources.root, &mut errors);
    // A considered source that cannot be read would silently drop its sources.
    if !errors.is_empty() {
        return Err(io::Error::other(format!(
            "cannot restrict the search to referencing sources because reference discovery failed; fix or restore the named source before retrying:\n{}",
            errors.join("\n")
        )));
    }
    let mut regions = ReferenceRegions(BTreeMap::new());
    for reference in found {
        if !reference.closed
            || !reference.is_fixme_edge()
            || !roles.contains(&reference.kind)
            || !selected(&reference.name)
        {
            continue;
        }
        let file = relative_path(&sources.root, &reference.file)?;
        let lines = if whole_files {
            None
        } else {
            let parsed = sources.outlines.parse(&sources.root, &file);
            match parsed.as_ref() {
                Ok(parsed) => {
                    let node =
                        &parsed.outline.nodes[parsed.reference_owner_at_line(reference.line)];
                    Some((node.line_start, node.line_end))
                }
                Err(_) => None,
            }
        };
        regions.add(file, lines);
    }
    for (name, definition) in &definitions {
        if !selected(name) {
            continue;
        }
        for anchor in &definition.anchors {
            if !roles.contains(&anchor.kind) {
                continue;
            }
            // Discovery omits excluded sources rather than aborting.
            let Ok(path) = sources.access.file(&sources.root, Path::new(&anchor.file)) else {
                continue;
            };
            let file = relative_path(&sources.root, &path)?;
            let lines = if whole_files || anchor.item.is_empty() {
                None
            } else {
                let parsed = sources.outlines.parse(&sources.root, &file);
                match parsed.as_ref() {
                    Ok(parsed) => match parsed.outline.resolve(&anchor.item) {
                        Ok(index) => {
                            let node = &parsed.outline.nodes[index];
                            Some((node.line_start, node.line_end))
                        }
                        // An anchor that no longer resolves names no node.
                        Err(_) => continue,
                    },
                    Err(_) => None,
                }
            };
            regions.add(file, lines);
        }
    }
    Ok(Some(regions))
}

// {impl}`requirements.configuration.evidence_paths`
// {impl}`requirements.authoring.generated_inputs`
fn structural(sources: &AssessmentSources, args: &Map<String, Value>) -> io::Result<Listing> {
    if args.contains_key("query") {
        check_keys(args, &["query"])?;
        return declared_query(sources, string(args, "query")?);
    }
    check_keys(args, &["language", "rule", "path", "referencing"])?;
    let language = string(args, "language")?;
    let rule = args
        .get("rule")
        .and_then(Value::as_object)
        .ok_or_else(|| io::Error::other("rule must be an object"))?;
    let rule = StructuralRule::compile(language, rule).map_err(io::Error::other)?;
    let path = optional_path(args)?;
    let regions = referencing(sources, args)?;
    let matches = walk_sources(
        sources,
        path,
        &|file, path| {
            rule.language().covers(path) && regions.as_ref().is_none_or(|regions| regions.has(file))
        },
        &|_, path| {
            let text = fs::read_to_string(path).map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("ast-grep failed to search {}: {error}", path.display()),
                )
            })?;
            let ranges = rule.matches(&text);
            Ok((!ranges.is_empty()).then_some((text, ranges)))
        },
    )?;
    let mut entries = Vec::new();
    for (file, (text, ranges)) in matches {
        let parsed = sources.parse_file(&file)?;
        let parsed = parsed
            .as_ref()
            .as_ref()
            .map_err(|error| io::Error::other(error.clone()))?;
        if parsed.source != text {
            return Err(io::Error::other(format!(
                "{file} changed during the search; retry it"
            )));
        }
        let line_of = |byte: usize| text[..byte].matches('\n').count() + 1;
        let mut nodes = SearchNodes::default();
        for range in ranges {
            let (first, last) = (
                line_of(range.start),
                line_of(range.end.saturating_sub(1).max(range.start)),
            );
            if regions
                .as_ref()
                .is_some_and(|regions| regions.region(&file, first, last).is_none())
            {
                continue;
            }
            let index = parsed.outline.ownership.owner(&range);
            nodes.add(index, first..=last, first..=last);
        }
        entries.extend(nodes.entries(&file, parsed));
    }
    Ok(Listing::object(&["matches"], vec![entries]))
}

/// Runs a query a distributed requirement declares, named
/// `<full-name>/<query-name>`, as validation runs it, returning each match's
/// site as a search entry with the matched lines.
// {impl}`requirements.references.distributed`
fn declared_query(sources: &AssessmentSources, query: &str) -> io::Result<Listing> {
    let (requirement, name) = query.rsplit_once('/').ok_or_else(|| {
        io::Error::other(format!(
            "query {query:?} must have the form <full-name>/<query-name>"
        ))
    })?;
    let definitions = crate::evidence::load_definitions(&sources.root)?;
    let declaration = definitions
        .get(requirement)
        .filter(|definition| definition.kind == crate::DefinitionKind::Requirement)
        .ok_or_else(|| {
            io::Error::other(format!("query {query:?} names no declared requirement"))
        })?;
    let queries = crate::structural::declared_queries(declaration).map_err(io::Error::other)?;
    if !queries.iter().any(|declared| declared.name == name) {
        return Err(io::Error::other(format!(
            "requirement {requirement} declares no query named {name:?}"
        )));
    }
    let mut errors = Vec::new();
    let files = crate::evidence::discovered_files(&sources.root, &mut errors);
    if !errors.is_empty() {
        return Err(io::Error::other(format!(
            "cannot run query {query} because reference discovery failed:\n{}",
            errors.join("\n")
        )));
    }
    let found =
        crate::structural::query_matches(&sources.root, &files, &sources.outlines, &queries, name)
            .map_err(|error| io::Error::other(format!("query {query}: {error}")))?;
    // Matches arrive in path and source order; each site is one entry.
    let mut by_file = BTreeMap::<String, BTreeMap<String, SearchMatchLines>>::new();
    for found in found {
        by_file
            .entry(found.site.file)
            .or_default()
            .entry(found.site.item)
            .or_default()
            .push(found.line_start..=found.line_end);
    }
    let mut entries = Vec::new();
    for (file, items) in by_file {
        let parsed = sources.parse_file(&file)?;
        let parsed = parsed
            .as_ref()
            .as_ref()
            .map_err(|error| io::Error::other(error.clone()))?;
        let mut nodes = SearchNodes::default();
        for (item, lines) in items {
            let index = parsed.resolve(&file, &item).map_err(io::Error::other)?;
            for lines in lines {
                nodes.add(index, lines.clone(), lines);
            }
        }
        entries.extend(nodes.entries(&file, parsed));
    }
    Ok(Listing::object(&["matches"], vec![entries]))
}

/// The line ranges of a declared query's matches in one site.
type SearchMatchLines = Vec<std::ops::RangeInclusive<usize>>;

// {impl}`requirements.authoring.file_discovery`
fn check_keys(args: &Map<String, Value>, allowed: &[&str]) -> io::Result<()> {
    for key in args.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(io::Error::other(format!(
                "unexpected tool argument {key:?}"
            )));
        }
    }
    Ok(())
}

// {cite}`habeas:util`
fn string<'a>(args: &'a Map<String, Value>, key: &str) -> io::Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other(format!("{key} must be a string")))
}

// {cite}`habeas:util`
fn integer(args: &Map<String, Value>, key: &str) -> io::Result<usize> {
    args.get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| io::Error::other(format!("{key} must be a nonnegative integer")))
}

// {impl}`requirements.authoring.file_discovery`
fn optional_path(args: &Map<String, Value>) -> io::Result<&str> {
    if args.contains_key("path") {
        string(args, "path")
    } else {
        Ok(".")
    }
}

// {impl}`requirements.authoring.file_discovery`
// {impl}`requirements.ci.input_containment`
fn project_path(sources: &AssessmentSources, relative: &str) -> io::Result<PathBuf> {
    sources.access.path(&sources.root, Path::new(relative))
}

// {impl}`requirements.authoring.file_discovery`
fn relative_path(root: &Path, path: &Path) -> io::Result<String> {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .filter(|part| *part != Component::CurDir)
        .collect::<PathBuf>()
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| io::Error::other("path is not UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoring::access::tests::git;
    use crate::authoring::inputs::tests::{project, role};

    // {test}`requirements.authoring.github_reads`
    #[test]
    fn a_successful_github_read_records_no_evidence_inputs() {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 0 && line != "\r\n" {
                line.clear();
            }
            let body =
                json!({"state":"open","title":"Queue capacity","body":"Details."}).to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        let root = project();
        let mut sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        sources.github = crate::github::GithubIssues::with_api(endpoint, None, None);
        let before = sources.recorded_inputs().to_vec();
        let delivery = dispatch(&sources, "github", &json!({"issue": "example/queue#7"})).unwrap();
        server.join().unwrap();
        assert_eq!(delivery.result["reference"], "example/queue#7");
        assert_eq!(delivery.result["state"], "open");
        assert_eq!(delivery.result["body"], "Details.");
        assert!(delivery.inputs.is_empty());
        sources.record_inputs(&delivery.inputs);
        assert_eq!(sources.recorded_inputs(), before.as_slice());
    }

    // {test}`requirements.authoring.additional_read`
    // {test}`requirements.authoring.github_reads`
    // {test}`requirements.authoring.file_discovery`
    // {test}`requirements.authoring.read_denial`
    #[test]
    fn additional_read_flow_searches_outlines_and_reads_a_rust_file() {
        let root = project();
        fs::write(
            root.path().join("md/terms.md"),
            ":::{definition} value\n:phrase: value\nA useful value.\n:::\n",
        )
        .unwrap();
        fs::write(
            root.path().join("src/lib.rs"),
            format!(
                "// {}\nfn work() {{}}\n\nfn other() {{}}\n",
                role("term", "terms.value")
            ),
        )
        .unwrap();
        let mut sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        for arguments in [json!({}), json!({"issue": 7})] {
            let error = dispatch(&sources, "github", &arguments).err().unwrap();
            assert!(
                error
                    .to_string()
                    .contains("github expects exactly an issue field"),
                "{error}"
            );
        }
        // GitHub reads return tool errors here, since unit tests never contact
        // GitHub, and are never evidence inputs.
        for (issue, message) in [
            ("example/app#1", "cannot read GitHub issue example/app#1"),
            ("example/app", "is not an issue reference"),
        ] {
            let error = dispatch(&sources, "github", &json!({ "issue": issue }))
                .err()
                .unwrap();
            assert!(error.to_string().contains(message), "{error}");
        }
        let directories = dispatch(&sources, "list_files", &json!({})).unwrap();
        let directory = directories
            .result
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["path"] == "src")
            .unwrap();
        assert_eq!(directory["kind"], "directory");
        assert!(directories.inputs.is_empty());
        // The listing finds the directory's file and records nothing.
        let listing = dispatch(&sources, "list_files", &json!({"path":directory["path"]})).unwrap();
        assert_eq!(listing.result, json!([{"path":"src/lib.rs","kind":"file"}]));
        assert!(listing.inputs.is_empty());
        // Searching the Rust file returns the innermost item containing the match.
        let search = dispatch(
            &sources,
            "ripgrep",
            &json!({"path":"src/lib.rs","pattern":"work","context":0}),
        )
        .unwrap();
        // It only locates the item: nothing is recorded and no definitions
        // accompany it.
        assert!(search.inputs.is_empty());
        assert_eq!(
            search.result["matches"],
            json!([{
                "file": "src/lib.rs", "item": "fn[work]", "kind": "fn", "tag":"1",
                "lines": [{"text": "fn work() {}", "matched": true}],
            }])
        );
        // Outlining it lists its items without content and records nothing.
        let outline = dispatch(&sources, "outline", &json!({"file":"src/lib.rs"})).unwrap();
        assert!(outline.inputs.is_empty());
        let items = outline
            .result
            .get("entries")
            .unwrap_or(&outline.result)
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["item"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(items, ["fn[work]", "fn[other]"]);
        assert!(!outline.result.to_string().contains("fn other()"));
        // Reading one item by selector returns it with its terminology.
        let item = dispatch(
            &sources,
            "habeas",
            &json!({"file":"src/lib.rs","item":"fn[work]"}),
        )
        .unwrap();
        assert_eq!(item.result["input"]["item"], "fn[work]");
        assert!(
            item.result["input"]["text"]
                .as_array()
                .unwrap()
                .iter()
                .any(|line| line == "fn work() {}")
        );
        assert_eq!(item.result["definitions"].as_array().unwrap().len(), 1);
        assert_eq!(item.result["definitions"][0]["definition"], "terms.value");
        assert!(
            item.inputs
                .iter()
                .any(|input| input["definition"] == "terms.value")
        );
        assert_eq!(item.inputs.len(), 2);
        sources.record_inputs(&item.inputs);
        assert!(sources.recorded_inputs().contains(
            &crate::inputs::outline_input(root.path(), "src/lib.rs", "fn[work]").unwrap()
        ));
        assert_eq!(sources.recorded_inputs().len(), 2);
        // A line range without line_end reads through EOF.
        let eof = dispatch(
            &sources,
            "habeas",
            &json!({"file":"src/lib.rs","line_start":2}),
        )
        .unwrap();
        let covered = eof.result["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|input| input["item"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(covered, ["fn[work]", "fn[other]"]);
        sources.record_inputs(&eof.inputs);
        let recorded = sources.recorded_inputs();
        for item in ["fn[work]", "fn[other]"] {
            assert!(
                recorded.contains(
                    &crate::inputs::outline_input(root.path(), "src/lib.rs", item).unwrap()
                ),
                "{item} recorded: {recorded:?}"
            );
        }
        assert!(
            recorded
                .iter()
                .any(|input| input["definition"] == "terms.value"),
            "terminology recorded: {recorded:?}"
        );
    }

    // {test}`requirements.authoring.additional_read`
    // {test}`requirements.authoring.file_discovery`
    #[test]
    fn listed_git_directories_need_an_eligible_descendant_at_any_depth() {
        let root = project();
        fs::create_dir_all(root.path().join("a/b")).unwrap();
        fs::create_dir(root.path().join("blocked-only")).unwrap();
        fs::write(root.path().join("a/b/file.txt"), "eligible\n").unwrap();
        fs::write(root.path().join("blocked-only/hidden.txt"), "blocked\n").unwrap();
        fs::write(
            root.path().join("habeas.toml"),
            "version = 1\n[evidence]\ndisallow = ['blocked-only/hidden.txt']\n",
        )
        .unwrap();
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["add", "."]);
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let root_listing = dispatch(&sources, "list_files", &json!({})).unwrap();
        let entries = root_listing.result.as_array().unwrap();
        assert!(
            entries
                .iter()
                .any(|e| e["path"] == "a" && e["kind"] == "directory"),
            "{entries:?}"
        );
        assert!(
            !entries.iter().any(|e| e["path"] == "blocked-only"),
            "{entries:?}"
        );
        let nested = dispatch(&sources, "list_files", &json!({"path":"a"})).unwrap();
        assert_eq!(nested.result, json!([{"path":"a/b","kind":"directory"}]));
        assert!(root_listing.inputs.is_empty());
        assert!(nested.inputs.is_empty());
    }

    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.file_discovery`
    // {test}`requirements.authoring.delivered_inputs`
    // {test}`requirements.definitions.authoring_inputs`
    // {test}`requirements.authoring.no_line_numbers`
    #[test]
    fn reading_records_only_delivered_sources_and_their_terms() {
        let root = project();
        fs::write(
            root.path().join("md/terms.md"),
            ":::{definition} value\n:phrase: value\nA useful value.\n:::\n",
        )
        .unwrap();
        fs::write(
            root.path().join("src/lib.rs"),
            format!("// {}\nfn work() {{}}\n", role("term", "terms.value")),
        )
        .unwrap();
        let mut sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let listing = dispatch(&sources, "list_files", &json!({"path":"src"})).unwrap();
        assert_eq!(listing.result, json!([{"path":"src/lib.rs","kind":"file"}]));
        assert!(listing.inputs.is_empty());
        let read = dispatch(
            &sources,
            "habeas",
            &json!({"file":"src/lib.rs","item":"fn[work]"}),
        )
        .unwrap();
        assert_eq!(read.inputs.len(), 2);
        assert_eq!(read.result["input"]["item"], "fn[work]");
        assert_eq!(read.result["input"]["kind"], "fn");
        // Results give the selector, never line numbers.
        assert!(read.result["input"].get("line_start").is_none());
        assert!(read.result["input"].get("line_end").is_none());
        assert_eq!(read.result["input"]["text"][1], "fn work() {}");
        assert_eq!(read.result["definitions"][0]["definition"], "terms.value");
        assert!(sources.recorded_inputs().is_empty());
        sources.record_inputs(&read.inputs);
        sources.record_inputs(&read.inputs);
        assert_eq!(sources.recorded_inputs().len(), 2);
        // Recorded outline inputs carry only the file, selector, and hash.
        assert_eq!(
            sources.recorded_inputs()[0],
            crate::inputs::outline_input(root.path(), "src/lib.rs", "fn[work]").unwrap()
        );
        let eof = dispatch(
            &sources,
            "habeas",
            &json!({"file":"src/lib.rs","line_start":2}),
        )
        .unwrap();
        assert_eq!(eof.result["inputs"].as_array().unwrap().len(), 1);
        assert_eq!(eof.result["inputs"][0]["item"], "fn[work]");
        assert!(eof.result["inputs"][0].get("line_end").is_none());
        // A line-range read supplies the terminology of what it delivers.
        let fresh = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let lines = dispatch(
            &fresh,
            "habeas",
            &json!({"file":"src/lib.rs","line_start":1}),
        )
        .unwrap();
        assert_eq!(lines.result["definitions"][0]["definition"], "terms.value");
        assert_eq!(lines.result["definitions"][0]["text"][1], ":phrase: value");
        assert_eq!(lines.inputs.len(), 2);
        assert!(
            dispatch(
                &sources,
                "habeas",
                &json!({"file":"src/lib.rs","line_start":3})
            )
            .is_err()
        );
        assert!(
            dispatch(
                &sources,
                "habeas",
                &json!({"file":"src/lib.rs","item":"fn[missing]"})
            )
            .is_err()
        );
        assert!(
            dispatch(
                &sources,
                "habeas",
                &json!({"file":"src/lib.rs","line_start":0})
            )
            .is_err()
        );
        // The provider-facing schema leaves selector fields optional; the
        // harness still rejects incomplete, mixed, and unknown request forms.
        for arguments in [
            json!({}),
            json!({"file":"src/lib.rs"}),
            json!({"line_start":1}),
            json!({"rust_file":"src/lib.rs","rust_item":"fn[work]"}),
            json!({"item":"fn[work]"}),
            json!({"file":"src/lib.rs","line_start":1,"definition":"terms.value"}),
            json!({"file":"src/lib.rs","item":"fn[work]","line_start":1}),
            json!({"file":"src/lib.rs","item":"fn[work]","requirements":"requirements.*"}),
            json!({"requirements":"requirements.*","definition":"terms.value"}),
            json!({"definition":"terms.value","extra":true}),
        ] {
            assert!(
                dispatch(&sources, "habeas", &arguments).is_err(),
                "unexpectedly accepted {arguments}"
            );
        }
        assert!(
            dispatch(
                &sources,
                "habeas",
                &json!({"requirements":"requirements.*"})
            )
            .is_ok()
        );
        assert!(dispatch(&sources, "habeas", &json!({"definition":"terms.value"})).is_ok());
    }

    // {test}`requirements.definitions.selection`
    // {test}`requirements.references.definition_globs`
    // {test}`requirements.authoring.source_tools`
    #[test]
    fn requirements_reads_select_requirements_and_examples_but_not_definitions() {
        let root = project();
        fs::write(
            root.path().join("md/requirements.md"),
            format!(
                ":::{{req}} ready\nThe project MUST be ready.\n:::\n\n\
                 :::{{example}} case\nA ready case. {}\n:::\n\n\
                 :::{{definition}} unused\n:phrase: unused\n\nNot referenced.\n:::\n",
                role("cite", "requirements.ready")
            ),
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let delivery = dispatch(
            &sources,
            "habeas",
            &json!({"requirements": "requirements.*"}),
        )
        .unwrap();
        assert_eq!(
            delivery.result["input"]["matches"],
            json!(["requirements.case", "requirements.ready"]),
            "{}",
            delivery.result
        );
        assert_eq!(delivery.result["definitions"], json!([]));
        let error = dispatch(
            &sources,
            "habeas",
            &json!({"requirements": "requirements.unused"}),
        )
        .err()
        .unwrap();
        assert!(
            error
                .to_string()
                .contains("matches no requirement or example"),
            "{error}"
        );
    }

    // {test}`requirements.authoring.redaction`
    // {test}`requirements.authoring.credentials`
    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.text_search`
    // {test}`requirements.authoring.mandatory_inputs`
    // {test}`requirements.authoring.citation_prompt`
    // {test}`requirements.authoring.fixme_inputs`
    #[test]
    fn model_inputs_reads_searches_and_citation_prompts_are_redacted() {
        let root = project();
        let token = crate::redaction::tests::github_token();
        fs::write(
            root.path().join("habeas.toml"),
            "version = 1\n[authoring.configs.local]\nbackend = 'ollama'\nmodel = 'm'\napi_key = 'plain ollama key'\n",
        )
        .unwrap();
        fs::write(
            root.path().join("src/lib.rs"),
            format!(
                "// {}\nfn client() -> &'static str {{\n    \"{token}\"\n}}\n\n// :::{} requirements.ready\n// Defaults to {token}.\n// :::\nfn helper() {{}}\n\n// :::{{fixme}} requirements.ready\n// Missing check for {token}.\n// :::\nfn gap() {{}}\n",
                role("test", "requirements.ready"),
                "{cite}"
            ),
        )
        .unwrap();
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["add", "."]);
        let leaks = |value: &Value| {
            let text = value.to_string();
            text.contains(&token) || text.contains("plain ollama key")
        };
        // Mandatory inputs, and so the initial and verification prompts.
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let mandatory = Value::Array(sources.mandatory_inputs().to_vec());
        assert!(!leaks(&mandatory), "{mandatory:#}");
        assert!(mandatory.to_string().contains("[REDACTED:GitHub Token]"));
        let prompt = super::super::initial_prompt(&sources).unwrap().joined();
        assert!(!prompt.contains(&token), "{prompt}");
        assert!(
            prompt.contains("Missing check for [REDACTED:GitHub Token]."),
            "{prompt}"
        );
        assert!(!prompt.contains("fn gap()"), "{prompt}");
        // A habeas read of the configuration file and a line range.
        let read = dispatch(&sources, "habeas", &json!({"file":"habeas.toml","item":""})).unwrap();
        assert!(!leaks(&read.result), "{:#}", read.result);
        assert!(
            read.result["input"]["text"]
                .as_array()
                .unwrap()
                .contains(&json!("api_key = '[REDACTED:api_key]'"))
        );
        let lines = dispatch(
            &sources,
            "habeas",
            &json!({"file":"src/lib.rs","line_start":1}),
        )
        .unwrap();
        assert!(!leaks(&lines.result), "{:#}", lines.result);
        // Search results, even for a pattern matching the credential itself.
        for pattern in ["ollama", "gh[p]_", "Defaults"] {
            let search = dispatch(&sources, "ripgrep", &json!({"pattern": pattern})).unwrap();
            assert!(
                !search.result["matches"].as_array().unwrap().is_empty(),
                "{pattern}"
            );
            assert!(!leaks(&search.result), "{pattern}: {:#}", search.result);
        }
        // A citation review's inputs and its commentary in the prompt.
        let citation = AssessmentSources::prepare_citation(
            crate::sdk::HabeasDomain::discover(root.path()).unwrap(),
            "src/lib.rs",
            "fn[helper]",
        )
        .unwrap();
        assert!(!leaks(&Value::Array(citation.mandatory_inputs().to_vec())));
        let prompt = super::super::initial_prompt(&citation).unwrap().joined();
        assert!(!prompt.contains(&token), "{prompt}");
        assert!(
            prompt.contains("Defaults to [REDACTED:GitHub Token]."),
            "{prompt}"
        );
    }

    // {test}`requirements.authoring.redaction`
    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.delivered_inputs`
    // {test}`requirements.definitions.authoring_inputs`
    #[test]
    fn declaration_text_reaches_the_model_redacted() {
        let root = project();
        let token = crate::redaction::tests::github_token();
        fs::write(
            root.path().join("md/terms.md"),
            format!(":::{{definition}} value\n:phrase: value\nA value such as {token}, with a {}.\n:::\n\n:::{{definition}} base\n:phrase: base\nA foundation.\n:::\n", role("term", "terms.base")),
        )
        .unwrap();
        fs::write(
            root.path().join("md/requirements.md"),
            format!(
                ":::{{req}} ready\nThe project MUST accept {token} as a {}.\n{}\n:::\n\n:::{{req}} other\nOther MUST reject\n{token}\non its own line.\n:::\n",
                role("term", "terms.value"),
                role("cite", "requirements.other"),
            ),
        )
        .unwrap();
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["add", "."]);
        // The subject, a cited requirement, and a term in the mandatory
        // inputs and so the prompt, with their line structure kept.
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let mandatory = Value::Array(sources.mandatory_inputs().to_vec());
        let text = mandatory.to_string();
        assert!(!text.contains(&token), "{mandatory:#}");
        for expected in [
            "The project MUST accept [REDACTED:GitHub Token] as a",
            "A value such as [REDACTED:GitHub Token], with a",
            "\"Other MUST reject\",\"[REDACTED:GitHub Token]\",\"on its own line.\"",
        ] {
            assert!(
                text.contains(expected),
                "missing {expected:?}: {mandatory:#}"
            );
        }
        let prompt = super::super::initial_prompt(&sources).unwrap().joined();
        assert!(!prompt.contains(&token), "{prompt}");
        // Requirements and definition reads.
        for arguments in [
            json!({"requirements":"requirements.*"}),
            json!({"definition":"terms.value"}),
        ] {
            let read = dispatch(&sources, "habeas", &arguments).unwrap();
            let text = read.result.to_string();
            assert!(!text.contains(&token), "{arguments}: {:#}", read.result);
            assert!(
                text.contains("[REDACTED:GitHub Token]"),
                "{arguments}: {:#}",
                read.result
            );
            let definition = arguments.get("definition").is_some();
            assert_eq!(read.result["input"].get("definition").is_some(), definition);
            assert_eq!(
                read.result["input"].get("requirements").is_some(),
                !definition
            );
            assert!(read.result.get("inputs").is_none());
            let expected_terms = if definition {
                vec!["terms.base"]
            } else {
                vec!["terms.base", "terms.value"]
            };
            let terms: Vec<_> = read.result["definitions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|input| input["definition"].as_str().unwrap())
                .collect();
            assert_eq!(terms, expected_terms);
            if definition {
                assert_eq!(read.result["input"]["definition"], "terms.value");
                assert_eq!(
                    read.result["input"]["text"],
                    json!([
                        ":::{definition} value",
                        ":phrase: value",
                        "A value such as [REDACTED:GitHub Token], with a {term}`terms.base`.",
                        ":::"
                    ])
                );
            } else {
                assert_eq!(read.result["input"]["requirements"], "requirements.*");
                assert_eq!(
                    read.result["input"]["matches"],
                    json!(["requirements.other", "requirements.ready"])
                );
                assert_eq!(
                    read.result["input"]["text"],
                    json!([
                        ":::{req} other",
                        "Other MUST reject",
                        "[REDACTED:GitHub Token]",
                        "on its own line.",
                        ":::",
                        "",
                        ":::{req} ready",
                        "The project MUST accept [REDACTED:GitHub Token] as a {term}`terms.value`.",
                        "{cite}`requirements.other`",
                        ":::"
                    ])
                );
            }
            let mut fresh = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
            assert!(fresh.recorded_inputs().is_empty());
            fresh.record_inputs(&read.inputs);
            fresh.record_inputs(&read.inputs);
            let mut expected = vec![read.result["input"].clone()];
            expected.extend(
                read.result["definitions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .cloned(),
            );
            assert_eq!(fresh.recorded_inputs(), expected);
            assert_eq!(read.inputs, expected);
        }
    }

    // {test}`requirements.authoring.text_search`
    // {test}`requirements.authoring.additional_read`
    // {test}`requirements.authoring.file_discovery`
    // {test}`requirements.authoring.no_line_numbers`
    #[test]
    fn search_merges_context_windows_and_respects_ignore_rules() {
        let root = project();
        git(root.path(), &["init", "--quiet"]);
        fs::write(root.path().join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(root.path().join("ignored.txt"), "needle\n").unwrap();
        fs::write(root.path().join(".hidden.txt"), "needle\n").unwrap();
        let lines = (1..=20)
            .map(|number| {
                if number == 2 || number == 6 {
                    "needle".to_owned()
                } else {
                    format!("line {number}")
                }
            })
            .collect::<Vec<_>>();
        fs::write(root.path().join("sample.txt"), lines.join("\n")).unwrap();
        git(root.path(), &["add", "--force", "."]);
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let search = dispatch(&sources, "ripgrep", &json!({"pattern":"needle"})).unwrap();
        // Each plain-text line is an item. Even a large context window must
        // not include neighboring items.
        assert!(search.inputs.is_empty());
        let matches = search.result["matches"].as_array().unwrap();
        assert_eq!(matches.len(), 2, "{matches:#?}");
        let shown = matches
            .iter()
            .map(|entry| {
                assert_eq!(entry["file"], "sample.txt");
                assert_eq!(entry["kind"], "line");
                assert!(entry.get("line_start").is_none());
                entry["lines"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|line| {
                        assert!(line.get("line").is_none());
                        (
                            line["text"].as_str().unwrap(),
                            line["matched"].as_bool().unwrap(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(shown, [vec![("needle", true)], vec![("needle", true)]]);
        let exact = dispatch(
            &sources,
            "ripgrep",
            &json!({"pattern":"needle","path":"sample.txt","context":0}),
        )
        .unwrap();
        assert_eq!(
            exact.result["matches"][0]["lines"],
            json!([{"text": "needle", "matched": true}])
        );
        assert_eq!(
            exact.result["matches"][1]["lines"],
            json!([{"text": "needle", "matched": true}])
        );
        let absent = dispatch(&sources, "ripgrep", &json!({"pattern":"NEEDLE"})).unwrap();
        assert_eq!(absent.result["matches"], json!([]));
        assert!(dispatch(&sources, "ripgrep", &json!({"pattern":"["})).is_err());
        assert!(
            dispatch(
                &sources,
                "ripgrep",
                &json!({"pattern":"needle","context":-1})
            )
            .is_err()
        );
        assert!(dispatch(&sources, "list_files", &json!({"path":"../"})).is_err());
    }

    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.file_discovery`
    // {test}`requirements.authoring.text_search`
    // {test}`requirements.authoring.structural_search`
    // {test}`requirements.authoring.read_denial`
    // {test}`requirements.authoring.tool_descriptions.special_cases`
    #[test]
    fn generated_evidence_is_excluded_from_reads_search_and_discovery() {
        let root = project();
        fs::create_dir(root.path().join("evidence")).unwrap();
        fs::create_dir(root.path().join("src/evidence")).unwrap();
        fs::write(
            root.path().join("evidence/old.json"),
            "needle in a previous judgment\n",
        )
        .unwrap();
        fs::write(
            root.path().join("evidence/generated.rs"),
            "fn generated() {}\n",
        )
        .unwrap();
        fs::write(root.path().join("source.txt"), "needle in source\n").unwrap();
        fs::write(
            root.path().join("src/evidence/source.txt"),
            "needle in nested source\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                root.path().join("evidence"),
                root.path().join("old-output"),
            )
            .unwrap();
            std::os::unix::fs::symlink(
                root.path().join("evidence/old.json"),
                root.path().join("old.json"),
            )
            .unwrap();
        }
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["add", "."]);
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let search = dispatch(
            &sources,
            "ripgrep",
            &json!({"pattern":"needle", "context":0}),
        )
        .unwrap();
        let files = matched(&search)
            .iter()
            .map(|input| input["file"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(files, vec!["source.txt", "src/evidence/source.txt"]);
        let structural = dispatch(
            &sources,
            "ast_grep",
            &json!({"language":"rust","rule":{"rule":{"kind":"function_item"}}}),
        )
        .unwrap();
        assert!(matched(&structural).is_empty(), "{:?}", structural.result);
        assert!(
            dispatch(
                &sources,
                "ast_grep",
                &json!({"language":"rust","rule":{"rule":{"kind":"function_item"}},"path":"evidence"}),
            )
            .err()
            .unwrap()
            .to_string()
            .contains("generated evidence")
        );
        let listing = dispatch(&sources, "list_files", &json!({})).unwrap();
        assert!(listing.result.as_array().unwrap().iter().all(|entry| {
            !["evidence", "old-output", "old.json"].contains(&entry["path"].as_str().unwrap())
        }));
        for path in ["evidence/old.json", "./evidence/old.json"] {
            let error = dispatch(&sources, "habeas", &json!({"file":path, "line_start":1}))
                .err()
                .unwrap();
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
            assert!(error.to_string().contains("evidence/old.json"), "{error}");
            assert!(error.to_string().contains("generated evidence"), "{error}");
            assert!(sources.recorded_inputs().is_empty());
        }
        for (name, arguments) in [
            (
                "habeas",
                json!({"file":"evidence/generated.rs", "item":"fn[generated]"}),
            ),
            ("list_files", json!({"path":"evidence"})),
            ("ripgrep", json!({"path":"evidence", "pattern":"needle"})),
            (
                "ast_grep",
                json!({"path":"evidence", "language":"rust","rule":{"rule":{"kind":"function_item"}}}),
            ),
        ] {
            let error = dispatch(&sources, name, &arguments).err().unwrap();
            let path = arguments["file"]
                .as_str()
                .or_else(|| arguments["path"].as_str())
                .unwrap();
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
            assert!(error.to_string().contains(path), "{error}");
            assert!(error.to_string().contains("generated evidence"), "{error}");
            assert!(sources.recorded_inputs().is_empty());
        }
        #[cfg(unix)]
        {
            for path in ["old.json", "old-output/old.json"] {
                assert!(
                    dispatch(&sources, "habeas", &json!({"file":path, "line_start":1}))
                        .err()
                        .unwrap()
                        .to_string()
                        .contains("generated evidence")
                );
                assert!(
                    dispatch(
                        &sources,
                        "ripgrep",
                        &json!({"path":path, "pattern":"needle"})
                    )
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("generated evidence")
                );
            }
            assert!(
                dispatch(&sources, "list_files", &json!({"path":"old-output"}))
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("generated evidence")
            );
        }
    }

    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.file_discovery`
    // {test}`requirements.authoring.text_search`
    #[cfg(unix)]
    #[test]
    fn outside_symlinks_are_excluded_from_reads_search_and_discovery() {
        let root = project();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.rs"), "fn outside_needle() {}\n").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("outside")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("secret.rs"),
            root.path().join("outside.rs"),
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let listing = dispatch(&sources, "list_files", &json!({})).unwrap();
        assert!(listing.result.as_array().unwrap().iter().all(|entry| {
            !["outside", "outside.rs"].contains(&entry["path"].as_str().unwrap())
        }));
        let search = dispatch(&sources, "ripgrep", &json!({"pattern":"outside_needle"})).unwrap();
        assert!(matched(&search).is_empty());
        for path in ["outside/secret.rs", "outside.rs"] {
            for arguments in [
                json!({"file":path, "line_start":1}),
                json!({"file":path, "line_start":1, "line_end":1}),
                json!({"file":path, "item":"fn[outside_needle]"}),
            ] {
                assert!(dispatch(&sources, "habeas", &arguments).is_err());
            }
            assert!(
                dispatch(
                    &sources,
                    "ripgrep",
                    &json!({"path":path, "pattern":"needle"})
                )
                .is_err()
            );
        }
        assert!(dispatch(&sources, "list_files", &json!({"path":"outside"})).is_err());
    }

    // {test}`requirements.authoring.read_denial`
    #[cfg(unix)]
    #[test]
    // {test}`requirements.authoring.text_search`
    // {test}`requirements.authoring.structural_search`
    fn operating_system_permission_failures_are_ordinary_read_failures() {
        use std::os::unix::fs::PermissionsExt;
        let root = project();
        fs::create_dir(root.path().join("locked")).unwrap();
        fs::write(root.path().join("locked.rs"), "fn locked() {}\n").unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        for path in ["locked", "locked.rs"] {
            fs::set_permissions(root.path().join(path), fs::Permissions::from_mode(0o000)).unwrap();
        }
        let unreadable = fs::read(root.path().join("locked.rs")).is_err();
        let results = [
            dispatch(
                &sources,
                "ripgrep",
                &json!({"path":"locked","pattern":"anything"}),
            ),
            dispatch(
                &sources,
                "ast_grep",
                &json!({"path":"locked","language":"rust","rule":{"rule":{"kind":"function_item"}}}),
            ),
            dispatch(&sources, "list_files", &json!({"path": "locked"})),
            dispatch(
                &sources,
                "habeas",
                &json!({"file": "locked.rs", "line_start": 1}),
            ),
        ];
        for path in ["locked", "locked.rs"] {
            fs::set_permissions(root.path().join(path), fs::Permissions::from_mode(0o755)).unwrap();
        }
        if !unreadable {
            // Permission bits do not restrict this user, e.g. root.
            return;
        }
        for result in results {
            let error = result.err().expect("the read fails");
            assert_ne!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
        }
    }

    // {test}`requirements.authoring.read_denial`
    // {test}`requirements.authoring.read_denial.untracked`
    // {test}`requirements.authoring.tool_descriptions.special_cases`
    #[test]
    fn untracked_diagnostics_name_git_add_and_an_allow_rule() {
        let root = project();
        fs::write(root.path().join("tracked.txt"), "tracked\n").unwrap();
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["add", "."]);
        fs::create_dir(root.path().join("untracked-dir")).unwrap();
        fs::write(root.path().join("untracked-dir/file.rs"), "fn f() {}\n").unwrap();
        fs::write(root.path().join("untracked.rs"), "fn g() {}\n").unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        for (name, arguments) in [
            ("habeas", json!({"file": "untracked.rs", "line_start": 1})),
            ("list_files", json!({"path": "untracked-dir"})),
        ] {
            let error = dispatch(&sources, name, &arguments).err().unwrap();
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
            let message = error.to_string();
            assert!(message.contains("git add"), "{message}");
            assert!(message.contains("habeas.toml allow rule"), "{message}");
            let path = arguments["file"]
                .as_str()
                .or_else(|| arguments["path"].as_str())
                .unwrap();
            assert!(message.contains(path), "{message}");
        }
        // The harness neither stages nor permits the files itself.
        let status = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(root.path())
            .output()
            .unwrap();
        let status = String::from_utf8(status.stdout).unwrap();
        assert!(status.contains("?? untracked-dir/"), "{status}");
        assert!(status.contains("?? untracked.rs"), "{status}");
        assert!(!root.path().join("habeas.toml").exists());
    }

    // {test}`requirements.authoring.read_denial`
    #[cfg(unix)]
    #[test]
    fn requests_through_symlinks_leaving_the_project_are_denials() {
        let root = project();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.rs"), "fn secret() {}\n").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("outside")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("secret.rs"),
            root.path().join("outside.rs"),
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        for (name, arguments) in [
            ("habeas", json!({"file": "outside.rs", "line_start": 1})),
            (
                "habeas",
                json!({"file": "outside.rs", "item": "fn[secret]"}),
            ),
            ("outline", json!({"file": "outside.rs"})),
            (
                "ripgrep",
                json!({"path": "outside.rs", "pattern": "secret"}),
            ),
            ("list_files", json!({"path": "outside"})),
        ] {
            let error = dispatch(&sources, name, &arguments).err().unwrap();
            assert_eq!(
                error.kind(),
                io::ErrorKind::PermissionDenied,
                "{name} {arguments}: {error}"
            );
            assert!(
                error.to_string().contains("outside the project"),
                "{name} {arguments}: {error}"
            );
        }
    }

    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.file_discovery`
    // {test}`requirements.authoring.text_search`
    // {test}`requirements.authoring.read_denial`
    #[test]
    fn git_tools_allow_staged_and_dirty_sources_but_hide_untracked_files() {
        let root = project();
        fs::create_dir(root.path().join("tracked-dir")).unwrap();
        fs::write(root.path().join("tracked-dir/file.txt"), "original\n").unwrap();
        fs::write(root.path().join("staged.txt"), "needle staged\n").unwrap();
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["add", "."]);
        fs::write(root.path().join("tracked-dir/file.txt"), "needle dirty\n").unwrap();
        fs::create_dir(root.path().join("untracked-dir")).unwrap();
        fs::write(
            root.path().join("untracked-dir/file.txt"),
            "needle hidden\n",
        )
        .unwrap();
        fs::write(root.path().join("untracked.rs"), "fn needle() {}\n").unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let listing = dispatch(&sources, "list_files", &json!({})).unwrap();
        assert_eq!(
            listing
                .result
                .get("entries")
                .unwrap_or(&listing.result)
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| entry["path"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["Cargo.toml", "book.toml", "md", "staged.txt", "tracked-dir"]
        );
        for (path, text) in [
            ("staged.txt", "needle staged"),
            ("tracked-dir/file.txt", "needle dirty"),
        ] {
            let read = dispatch(&sources, "habeas", &json!({"file":path, "line_start":1})).unwrap();
            assert_eq!(read.result["inputs"][0]["text"], json!([text]));
        }
        let search = dispatch(&sources, "ripgrep", &json!({"pattern":"needle"})).unwrap();
        assert_eq!(
            matched(&search)
                .iter()
                .map(|input| input["file"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["staged.txt", "tracked-dir/file.txt"]
        );
        for arguments in [
            json!({"file":"untracked.rs", "line_start":1}),
            json!({"file":"untracked.rs", "line_start":1, "line_end":1}),
            json!({"file":"untracked.rs", "item":"fn[needle]"}),
        ] {
            let error = dispatch(&sources, "habeas", &arguments).err().unwrap();
            assert!(error.to_string().contains("git add"), "{error}");
        }
        let error = dispatch(&sources, "outline", &json!({"file":"untracked.rs"}))
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
        assert!(error.to_string().contains("git add"), "{error}");
        assert!(dispatch(&sources, "list_files", &json!({"path":"untracked-dir"})).is_err());
        assert!(
            dispatch(
                &sources,
                "ripgrep",
                &json!({"path":"untracked.rs", "pattern":"needle"})
            )
            .is_err()
        );
    }

    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.file_discovery`
    // {test}`requirements.authoring.text_search`
    // {test}`requirements.authoring.read_denial`
    #[cfg(unix)]
    #[test]
    fn git_symlink_reads_require_both_the_alias_and_target_to_be_tracked() {
        let root = project();
        fs::write(root.path().join("tracked.txt"), "needle allowed\n").unwrap();
        fs::write(root.path().join("untracked.txt"), "needle hidden\n").unwrap();
        for (alias, target) in [
            ("tracked-alias.txt", "tracked.txt"),
            ("untracked-target.txt", "untracked.txt"),
            ("untracked-alias.txt", "tracked.txt"),
        ] {
            std::os::unix::fs::symlink(target, root.path().join(alias)).unwrap();
        }
        git(root.path(), &["init", "--quiet"]);
        git(
            root.path(),
            &[
                "add",
                "Cargo.toml",
                "book.toml",
                "md",
                "tracked.txt",
                "tracked-alias.txt",
                "untracked-target.txt",
            ],
        );
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let allowed = dispatch(
            &sources,
            "habeas",
            &json!({"file":"tracked-alias.txt", "line_start":1}),
        )
        .unwrap();
        assert_eq!(
            allowed.result["inputs"][0]["text"],
            json!(["needle allowed"])
        );
        for alias in ["untracked-target.txt", "untracked-alias.txt"] {
            assert!(
                dispatch(&sources, "habeas", &json!({"file":alias, "line_start":1}))
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("git add")
            );
            assert!(
                dispatch(
                    &sources,
                    "ripgrep",
                    &json!({"path":alias, "pattern":"needle"})
                )
                .is_err()
            );
        }
        let listing = dispatch(&sources, "list_files", &json!({})).unwrap();
        let paths = listing
            .result
            .get("entries")
            .unwrap_or(&listing.result)
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["path"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(paths.contains(&"tracked-alias.txt"));
        assert!(!paths.contains(&"untracked-target.txt"));
        assert!(!paths.contains(&"untracked-alias.txt"));
        assert!(!paths.contains(&"untracked.txt"));
    }

    // {test}`requirements.configuration.evidence_paths`
    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.file_discovery`
    // {test}`requirements.authoring.text_search`
    // {test}`requirements.authoring.read_denial`
    // {test}`requirements.authoring.structural_search`
    #[test]
    fn allowed_ignored_and_external_sources_are_discoverable_but_disallowed_descendants_are_not() {
        let root = project();
        let external = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("generated/private")).unwrap();
        fs::write(
            root.path().join("generated/visible.txt"),
            "needle allowed\n",
        )
        .unwrap();
        fs::write(
            root.path().join("generated/private/hidden.txt"),
            "needle denied\n",
        )
        .unwrap();
        fs::write(external.path().join("outside.txt"), "needle external\n").unwrap();
        fs::write(external.path().join("outside.rs"), "fn external() {}\n").unwrap();
        fs::write(
            root.path().join("generated/visible.rs"),
            "fn visible() {}\n",
        )
        .unwrap();
        fs::write(
            root.path().join("generated/private/denied.rs"),
            "fn denied() {}\n",
        )
        .unwrap();
        fs::write(root.path().join(".hidden.rs"), "fn hidden() {}\n").unwrap();
        fs::write(root.path().join(".gitignore"), "generated/\n").unwrap();
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["add", "."]);
        let external_toml = toml::Value::String(external.path().display().to_string());
        fs::write(root.path().join("habeas.toml"), format!(
            "version = 1\n[evidence]\nallow = ['generated', '.hidden.rs', {external_toml}]\ndisallow = ['generated/private']\n"
        )).unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let listing = dispatch(&sources, "list_files", &json!({})).unwrap();
        let paths = listing
            .result
            .get("entries")
            .unwrap_or(&listing.result)
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["path"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(paths.contains(&"generated"));
        assert!(paths.contains(&external.path().to_str().unwrap()));
        let generated = dispatch(&sources, "list_files", &json!({"path":"generated"})).unwrap();
        assert_eq!(
            generated.result,
            json!([
                {"path":"generated/visible.rs", "kind":"file"},
                {"path":"generated/visible.txt", "kind":"file"}
            ])
        );
        assert!(generated.inputs.is_empty());
        let external_listing =
            dispatch(&sources, "list_files", &json!({"path":external.path()})).unwrap();
        assert_eq!(
            external_listing.result,
            json!([
                {"path":external.path().join("outside.rs"),"kind":"file"},
                {"path":external.path().join("outside.txt"),"kind":"file"}
            ])
        );
        assert!(external_listing.inputs.is_empty());

        let structural = dispatch(
            &sources,
            "ast_grep",
            &json!({"language":"rust","rule":{"rule":{"kind":"function_item"}}}),
        )
        .unwrap();
        let structural_files = matched(&structural)
            .iter()
            .map(|entry| entry["file"].as_str().unwrap())
            .collect::<Vec<_>>();
        for expected in [
            ".hidden.rs",
            "generated/visible.rs",
            external.path().join("outside.rs").to_str().unwrap(),
        ] {
            assert!(structural_files.contains(&expected), "{structural_files:?}");
        }
        assert!(!structural_files.contains(&"generated/private/denied.rs"));
        assert!(structural.inputs.is_empty());

        let search = dispatch(
            &sources,
            "ripgrep",
            &json!({"pattern":"needle", "context":0}),
        )
        .unwrap();
        assert_eq!(matched(&search).len(), 2);
        assert!(
            matched(&search)
                .iter()
                .any(|input| input["file"] == "generated/visible.txt")
        );
        assert!(matched(&search).iter().any(|input| input["file"] == external.path().join("outside.txt").to_str().unwrap()));
        let read = dispatch(
            &sources,
            "habeas",
            &json!({"file":external.path().join("outside.txt"), "line_start":1}),
        )
        .unwrap();
        assert_eq!(read.result["inputs"][0]["text"], json!(["needle external"]));
        for (name, args) in [
            (
                "habeas",
                json!({"file":"generated/private/hidden.txt", "line_start":1}),
            ),
            ("list_files", json!({"path":"generated/private"})),
            (
                "ripgrep",
                json!({"path":"generated/private", "pattern":"needle"}),
            ),
            (
                "ast_grep",
                json!({"path":"generated/private","language":"rust","rule":{"rule":{"kind":"function_item"}}}),
            ),
        ] {
            assert_eq!(
                dispatch(&sources, name, &args).err().unwrap().kind(),
                io::ErrorKind::PermissionDenied
            );
        }
    }

    // {test}`requirements.authoring.generated_inputs`
    // {test}`requirements.authoring.file_discovery`
    // {test}`requirements.authoring.text_search`
    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.configuration.evidence_paths`
    // {test}`requirements.authoring.read_denial`
    // {test}`requirements.authoring.read_denial.build_output`
    // {test}`requirements.authoring.structural_search`
    #[test]
    fn cargo_output_is_discovered_and_read_without_building_or_allowing_other_target_files() {
        let root = project();
        fs::write(root.path().join("src/lib.rs"), "pub fn ready() {}\n").unwrap();
        fs::write(
            root.path().join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = 'fixture'\nversion = '0.1.0'\n",
        )
        .unwrap();
        fs::write(root.path().join(".gitignore"), "target/\n").unwrap();
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["add", "."]);
        let unbuilt = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        assert!(
            dispatch(
                &unbuilt,
                "habeas",
                &json!({"file":"src/lib.rs", "line_start":1})
            )
            .is_ok()
        );
        assert!(!root.path().join("target").exists());
        let out = root.path().join("target/debug/build/fixture-hash/out");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("generated.rs"), "pub fn generated_needle() {}\n").unwrap();
        fs::write(
            out.parent().unwrap().join("root-output"),
            out.to_str().unwrap(),
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let listing = dispatch(&sources, "list_files", &json!({})).unwrap();
        assert!(
            listing
                .result
                .get("entries")
                .unwrap_or(&listing.result)
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["path"] == "target/debug/build/fixture-hash/out")
        );
        let search = dispatch(&sources, "ripgrep", &json!({"pattern":"generated_needle"})).unwrap();
        let generated = dispatch(
            &sources,
            "list_files",
            &json!({"path":"target/debug/build/fixture-hash/out"}),
        )
        .unwrap();
        assert_eq!(
            generated.result,
            json!([{"path":"target/debug/build/fixture-hash/out/generated.rs","kind":"file"}])
        );
        assert!(generated.inputs.is_empty());
        assert_eq!(matched(&search).len(), 1);
        let structural_arguments =
            json!({"language":"rust","rule":{"rule":{"pattern":"pub fn generated_needle() {}"}}});
        let structural = dispatch(&sources, "ast_grep", &structural_arguments).unwrap();
        assert_eq!(
            found(&structural),
            ["target/debug/build/fixture-hash/out/generated.rs#fn[generated_needle]"]
        );
        assert!(structural.inputs.is_empty());
        let read = dispatch(&sources, "habeas", &json!({"file":"target/debug/build/fixture-hash/out/generated.rs", "item":"fn[generated_needle]"})).unwrap();
        assert_eq!(
            read.result["input"]["text"],
            json!(["pub fn generated_needle() {}"])
        );
        // Other build output under target is an ordinary read failure.
        let error = dispatch(
            &sources,
            "habeas",
            &json!({"file":"target/debug/build/fixture-hash/root-output", "line_start":1}),
        )
        .err()
        .unwrap();
        assert_ne!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
        assert!(error.to_string().contains("holds build output"), "{error}");
        fs::write(
            root.path().join("habeas.toml"),
            "version = 1\n[evidence]\ndisallow = ['target/debug/build/fixture-hash/out']\n",
        )
        .unwrap();
        let denied = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let listing = dispatch(&denied, "list_files", &json!({})).unwrap();
        assert!(
            !listing
                .result
                .get("entries")
                .unwrap_or(&listing.result)
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["path"] == "target/debug/build/fixture-hash/out")
        );
        assert!(
            matched(&dispatch(&denied, "ripgrep", &json!({"pattern":"generated_needle"})).unwrap())
                .is_empty()
        );
        assert!(matched(&dispatch(&denied, "ast_grep", &structural_arguments).unwrap()).is_empty());
        for (name, arguments) in [
            (
                "habeas",
                json!({"file":"target/debug/build/fixture-hash/out/generated.rs", "line_start":1}),
            ),
            (
                "list_files",
                json!({"path":"target/debug/build/fixture-hash/out"}),
            ),
            (
                "ripgrep",
                json!({"path":"target/debug/build/fixture-hash/out", "pattern":"generated_needle"}),
            ),
            (
                "ast_grep",
                json!({"path":"target/debug/build/fixture-hash/out","language":"rust","rule":{"rule":{"kind":"function_item"}}}),
            ),
        ] {
            let error = dispatch(&denied, name, &arguments).err().unwrap();
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
            assert!(error.to_string().contains("disallowed"), "{error}");
            let path = arguments["file"]
                .as_str()
                .or_else(|| arguments["path"].as_str())
                .unwrap();
            assert!(error.to_string().contains(path), "{error}");
        }
    }

    // {test}`requirements.authoring.text_search`
    // {test}`requirements.configuration.evidence_paths`
    // {test}`requirements.ci.input_containment`
    #[cfg(unix)]
    #[test]
    fn search_uses_the_validated_target_when_symlinks_precede_parent_components() {
        let root = project();
        fs::create_dir_all(root.path().join("actual/nested")).unwrap();
        fs::write(root.path().join("actual/needle.txt"), "actual needle\n").unwrap();
        fs::write(root.path().join("needle.txt"), "wrong needle\n").unwrap();
        std::os::unix::fs::symlink(root.path().join("actual/nested"), root.path().join("alias"))
            .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let result = dispatch(
            &sources,
            "ripgrep",
            &json!({"path":"alias/../needle.txt", "pattern":"needle", "context":0}),
        )
        .unwrap();
        assert_eq!(matched(&result).len(), 1);
        assert_eq!(matched(&result)[0]["file"], "actual/needle.txt");
        assert_eq!(
            matched(&result)[0]["lines"],
            json!([{"text": "actual needle", "matched": true}])
        );
    }

    // {test}`requirements.authoring.reference_index`
    // {test}`requirements.references.definition_globs`
    // {test}`requirements.authoring.no_line_numbers`
    // {test}`requirements.authoring.read_denial`
    #[test]
    // {test}`requirements.references.authoring_inputs`
    // {test}`requirements.authoring.read_denial`
    fn references_list_names_and_flattened_sites_without_recording_inputs() {
        let root = project();
        fs::write(
            root.path().join("md/requirements.md"),
            format!(
                ":::{{req}} ready\nThe project MUST be ready.\n:::\n\n:::{{req}} other\nOther MUST hold.\n:::\n\n:::{{example}} case\nA ready case. {}\n:::\n\n:::{{definition}} word\n:phrase: word\n:impl: src/lib.rs#fn[work]\nA vocabulary term.\n:::\n",
                role("cite", "requirements.ready")
            ),
        )
        .unwrap();
        fs::write(
            root.path().join("src/lib.rs"),
            format!(
                "// {}\nfn zeta() {{}}\n\n// {}\nfn work() {{\n    // {}\n    // {}\n}}\n",
                role("test", "requirements.ready"),
                role("impl", "requirements.ready"),
                role("cite", "requirements.other"),
                role("term", "requirements.word"),
            ),
        )
        .unwrap();
        fs::create_dir_all(root.path().join("private")).unwrap();
        fs::write(
            root.path().join("private/notes.md"),
            format!("{}\n", role("cite", "requirements.ready")),
        )
        .unwrap();
        fs::write(
            root.path().join("habeas.toml"),
            "version = 1\n\n[evidence]\ndisallow = [\"private\"]\n",
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();

        let all = dispatch(&sources, "references", &json!({})).unwrap();
        assert_eq!(
            all.result,
            json!({"definitions": [
                {"name": "requirements.case", "kind": "example"},
                {"name": "requirements.other", "kind": "requirement"},
                {"name": "requirements.ready", "kind": "requirement"},
            ], "sites": []})
        );
        assert!(all.inputs.is_empty());

        let ready = dispatch(
            &sources,
            "references",
            &json!({"selection": "requirements.ready"}),
        )
        .unwrap();
        let mut result = ready.result.clone();
        let markdown = result["sites"][0]["item"].take();
        assert!(
            markdown.as_str().unwrap().starts_with("section@"),
            "{markdown}"
        );
        assert_eq!(
            result,
            json!({"definitions": [{"name": "requirements.ready", "kind": "requirement"}], "sites": [
                {"name": "requirements.ready", "role": "cite", "file": "md/requirements.md", "item": null},
                {"name": "requirements.ready", "role": "test", "file": "src/lib.rs", "item": "fn[zeta]"},
                {"name": "requirements.ready", "role": "impl", "file": "src/lib.rs", "item": "fn[work]"},
            ]})
        );
        assert!(ready.inputs.is_empty());
        let implementations = dispatch(
            &sources,
            "references",
            &json!({"selection":"requirements.ready", "roles":["impl", "impl"]}),
        )
        .unwrap();
        assert_eq!(
            implementations.result,
            json!({"definitions":[{"name":"requirements.ready","kind":"requirement"}],
                "sites":[{"name":"requirements.ready","role":"impl","file":"src/lib.rs","item":"fn[work]"}]})
        );
        assert!(implementations.inputs.is_empty());
        let names = dispatch(
            &sources,
            "references",
            &json!({"selection":"requirements.ready", "roles":[]}),
        )
        .unwrap();
        assert_eq!(names.result["definitions"], ready.result["definitions"]);
        assert_eq!(names.result["sites"], json!([]));
        assert!(names.inputs.is_empty());
        assert_eq!(
            dispatch(&sources, "references", &json!({"roles":["impl"]}))
                .unwrap()
                .result,
            all.result
        );
        for roles in [json!(null), json!("impl"), json!(["fixme"]), json!([1])] {
            let error = dispatch(&sources, "references", &json!({"roles":roles}))
                .err()
                .unwrap();
            assert!(error.to_string().contains("role"), "{roles}: {error}");
        }

        let flattened = dispatch(
            &sources,
            "references",
            &json!({"selection": "requirements.*"}),
        )
        .unwrap();
        let sites = flattened.result["sites"].as_array().unwrap();
        assert_eq!(flattened.result["definitions"], all.result["definitions"]);
        assert_eq!(sites.len(), 4);
        assert_eq!(sites[0]["name"], "requirements.other");
        assert_eq!(sites[0]["item"], "fn[work]");
        assert!(flattened.inputs.is_empty());

        // A fenced reference in a comment run is a site of the node the run
        // belongs to; a `{fixme}` is not a site, nor is a fence that never
        // closes.
        fs::write(
            root.path().join("src/fenced.rs"),
            format!(
                "// :::{} requirements.other\n// Commentary.\n// :::\nfn fenced() {{}}\n\n// :::{} requirements.other\n// Tracked by example/queue#1.\n// :::\nfn gap() {{}}\n\n// :::{} requirements.other\n// never closed\n\nfn open() {{}}\n",
                "{impl}", "{fixme}", "{cite}"
            ),
        )
        .unwrap();
        let other = dispatch(
            &sources,
            "references",
            &json!({"selection": "requirements.other"}),
        )
        .unwrap();
        assert_eq!(
            other.result["sites"],
            json!([
                {"name": "requirements.other", "role": "impl", "file": "src/fenced.rs", "item": "fn[fenced]"},
                {"name": "requirements.other", "role": "cite", "file": "src/lib.rs", "item": "fn[work]"},
            ])
        );
        fs::remove_file(root.path().join("src/fenced.rs")).unwrap();

        for selection in ["requirements.missing", "requirements.word", "bad name", ""] {
            assert!(
                dispatch(&sources, "references", &json!({"selection": selection})).is_err(),
                "accepted {selection:?}"
            );
        }
        let error = dispatch(&sources, "references", &json!({"selection": 1}))
            .err()
            .unwrap();
        assert!(
            error.to_string().contains("selection must be a string"),
            "{error}"
        );
        assert!(
            dispatch(
                &sources,
                "references",
                &json!({"name": "requirements.ready"})
            )
            .is_err()
        );
    }

    // {test}`requirements.authoring.text_search`
    #[test]
    fn parallel_search_orders_results_by_path_and_line() {
        let root = project();
        for index in (0..40).rev() {
            fs::write(
                root.path().join(format!("src/file{index:02}.rs")),
                "// needle\nfn a() {\n    // needle\n}\n",
            )
            .unwrap();
        }
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let first = dispatch(
            &sources,
            "ripgrep",
            &json!({"pattern": "needle", "path": "src", "context": 0}),
        )
        .unwrap();
        let files: Vec<_> = first.result["matches"]
            .as_array()
            .unwrap()
            .iter()
            .map(|input| {
                (
                    input["file"].as_str().unwrap().to_owned(),
                    input["item"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        let mut sorted = files.clone();
        sorted.sort();
        assert_eq!(files, sorted);
        // Both comment matches in each file belong to its one function.
        assert_eq!(files.len(), 40);
        assert!(
            first.result["matches"]
                .as_array()
                .unwrap()
                .iter()
                .all(|input| input["item"] == "fn[a]" && input.get("line_end").is_none())
        );
        for _ in 0..5 {
            let again = dispatch(
                &sources,
                "ripgrep",
                &json!({"pattern": "needle", "path": "src", "context": 0}),
            )
            .unwrap();
            assert_eq!(again.result["matches"], first.result["matches"]);
            assert_ne!(again.result["search_id"], first.result["search_id"]);
        }
        assert!(dispatch(&sources, "ripgrep", &json!({"pattern": "("})).is_err());
    }

    // {test}`requirements.authoring.reference_index`
    // {test}`requirements.references.anchors`
    #[test]
    fn references_report_anchors_as_file_or_node_sites() {
        let root = project();
        fs::create_dir_all(root.path().join("skills")).unwrap();
        fs::write(root.path().join("skills/ready.md"), "# Skill\n").unwrap();
        fs::create_dir_all(root.path().join("private")).unwrap();
        fs::write(root.path().join("private/notes.md"), "hidden\n").unwrap();
        fs::write(
            root.path().join("md/requirements.md"),
            format!(
                ":::{{req}} ready\n:impl: skills/ready.md#section[Skill]\nThe project MUST be ready.\n:::\n\n:::{{example}} case\n:test: skills/ready.md\n:test: private/notes.md\nA ready case. {}\n:::\n",
                role("cite", "requirements.ready")
            ),
        )
        .unwrap();
        fs::write(
            root.path().join("habeas.toml"),
            "version = 1\n\n[evidence]\ndisallow = [\"private\"]\n",
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let result = dispatch(
            &sources,
            "references",
            &json!({"selection": "requirements.*"}),
        )
        .unwrap();
        let mut sites = result.result["sites"].clone();
        assert!(
            sites[1]["item"]
                .take()
                .as_str()
                .unwrap()
                .starts_with("section@")
        );
        assert_eq!(
            sites,
            json!([
                {"name": "requirements.case", "role": "test", "file": "skills/ready.md", "item": null},
                {"name": "requirements.ready", "role": "cite", "file": "md/requirements.md", "item": null},
                {"name": "requirements.ready", "role": "impl", "file": "skills/ready.md", "item": "section[Skill]"},
            ])
        );
        assert!(result.inputs.is_empty());
        let filtered = dispatch(
            &sources,
            "references",
            &json!({"selection":"requirements.*", "roles":["impl"]}),
        )
        .unwrap();
        assert_eq!(
            filtered.result["sites"],
            json!([{"name":"requirements.ready","role":"impl","file":"skills/ready.md","item":"section[Skill]"}])
        );
        assert!(filtered.inputs.is_empty());
    }

    // {test}`requirements.authoring.reference_index`
    // {test}`requirements.references.discovery`
    #[test]
    fn references_report_only_sites_in_considered_sources() {
        let root = project();
        let reference = format!(
            "// {}\nfn site() {{}}\n",
            role("impl", "requirements.ready")
        );
        for file in ["src/tracked.rs", "src/untracked.rs", "private/tracked.rs"] {
            let path = root.path().join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, &reference).unwrap();
        }
        fs::write(
            root.path().join("habeas.toml"),
            "version = 1\n\n[evidence]\ndisallow = [\"private\"]\n",
        )
        .unwrap();
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["add", "."]);
        git(
            root.path(),
            &["rm", "--cached", "--quiet", "src/untracked.rs"],
        );
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let result = dispatch(
            &sources,
            "references",
            &json!({"selection": "requirements.ready"}),
        )
        .unwrap();
        let files: Vec<_> = result.result["sites"]
            .as_array()
            .unwrap()
            .iter()
            .map(|site| site["file"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(files, ["src/tracked.rs"]);
    }

    // {test}`requirements.authoring.reference_index`
    #[cfg(unix)]
    #[test]
    fn references_name_a_considered_source_that_cannot_be_read() {
        use std::os::unix::fs::PermissionsExt;
        let root = project();
        let locked = root.path().join("src/locked.rs");
        fs::write(&locked, "fn locked() {}\n").unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read(&locked).is_ok() {
            // Permissions do not restrict this user, such as root.
            return;
        }
        let error = dispatch(
            &sources,
            "references",
            &json!({"selection": "requirements.ready"}),
        )
        .err()
        .unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();
        let message = error.to_string();
        assert!(message.contains("src/locked.rs"), "{message}");
        assert!(
            message.contains("cannot read reference source"),
            "{message}"
        );
        assert!(message.contains("reference discovery failed"), "{message}");
    }

    // {test}`requirements.authoring.read_denial`
    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.file_discovery`
    // {test}`requirements.authoring.text_search`
    // {test}`requirements.authoring.outline_tool`
    // {test}`requirements.authoring.tool_descriptions.special_cases`
    #[test]
    fn unpermitted_requests_outside_the_project_are_recoverable_tool_errors() {
        let root = project();
        let external = tempfile::tempdir().unwrap();
        fs::write(external.path().join("lib.rs"), "fn external() {}\n").unwrap();
        let file = external.path().join("lib.rs");
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let requests = [
            ("habeas", json!({"file": file, "line_start": 1})),
            ("habeas", json!({"file": file, "item": "fn[external]"})),
            ("list_files", json!({"path": external.path()})),
            ("outline", json!({"file": file})),
            (
                "ripgrep",
                json!({"path": external.path(), "pattern": "external"}),
            ),
        ];
        for (name, arguments) in &requests {
            let error = dispatch(&sources, name, arguments).err().unwrap();
            assert_ne!(
                error.kind(),
                io::ErrorKind::PermissionDenied,
                "{name}: {error}"
            );
            assert!(error.to_string().contains("outside the project"), "{error}");
        }
        assert!(sources.recorded_inputs().is_empty());

        // A matching disallow still aborts, as do in-project denials.
        fs::write(
            root.path().join("habeas.toml"),
            format!(
                "version = 1\n[evidence]\ndisallow = [{:?}]\n",
                external.path().to_str().unwrap()
            ),
        )
        .unwrap();
        fs::create_dir_all(root.path().join("evidence")).unwrap();
        fs::write(root.path().join("evidence/old.json"), "{}\n").unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        for (name, arguments) in &requests[..1] {
            let error = dispatch(&sources, name, arguments).err().unwrap();
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
        }
        let error = dispatch(
            &sources,
            "habeas",
            &json!({"file": "evidence/old.json", "item": ""}),
        )
        .err()
        .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
    }

    // {test}`requirements.authoring.read_denial`
    // {test}`requirements.authoring.read_denial.build_output`
    // {test}`requirements.authoring.tool_descriptions.special_cases`
    #[test]
    fn requests_for_book_or_target_output_are_ordinary_read_failures() {
        let root = project();
        for (book, built) in [("", "book"), ("[build]\nbuild-dir = 'site'\n", "site")] {
            fs::write(
                root.path().join("book.toml"),
                format!("[book]\nsrc = 'md'\n{book}"),
            )
            .unwrap();
            for directory in [built, "target"] {
                fs::create_dir_all(root.path().join(directory).join("nested")).unwrap();
                fs::write(
                    root.path().join(directory).join("nested/page.html"),
                    "fn built_needle() {}\n",
                )
                .unwrap();
            }
            let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
            for directory in [built, "target"] {
                let file = format!("{directory}/nested/page.html");
                for (name, arguments) in [
                    ("habeas", json!({"file": file, "line_start": 1})),
                    ("habeas", json!({"file": file, "item": ""})),
                    ("outline", json!({"file": file})),
                    ("list_files", json!({"path": directory})),
                    ("ripgrep", json!({"pattern": "needle", "path": directory})),
                ] {
                    let error = dispatch(&sources, name, &arguments).err().unwrap();
                    assert_ne!(
                        error.kind(),
                        io::ErrorKind::PermissionDenied,
                        "{name} {arguments}: {error}"
                    );
                    assert!(
                        error.to_string().contains("holds build output")
                            && error.to_string().contains(
                                "the sources it is built from are elsewhere in the project"
                            ),
                        "{name} {arguments}: {error}"
                    );
                }
            }
        }
        // With a build-dir set, `book` is an ordinary project directory.
        fs::write(root.path().join("book/source.txt"), "plain\n").unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        assert!(
            dispatch(
                &sources,
                "habeas",
                &json!({"file":"book/source.txt","line_start":1})
            )
            .is_ok()
        );
    }

    // {test}`requirements.authoring.read_denial`
    // {test}`requirements.authoring.mandatory_inputs`
    #[test]
    fn mandatory_inputs_outside_the_project_still_abort() {
        let root = project();
        let external = tempfile::tempdir().unwrap();
        fs::write(external.path().join("skill.md"), "# Skill\n").unwrap();
        fs::write(
            root.path().join("md/requirements.md"),
            format!(
                ":::{{req}} ready\n:impl: {}\nThe project MUST be ready.\n:::\n",
                external.path().join("skill.md").display()
            ),
        )
        .unwrap();
        let error = AssessmentSources::prepare(root.path(), "requirements.ready")
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
        assert!(error.to_string().contains("outside the project"), "{error}");
    }

    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.tool_descriptions.common_case`
    #[test]
    fn habeas_tool_describes_its_argument_conventions_with_an_example_per_form() {
        let definitions = definitions();
        let habeas = definitions
            .iter()
            .find(|definition| definition["name"] == "habeas")
            .expect("habeas tool is advertised");
        let description = habeas["description"].as_str().unwrap();
        for phrase in [
            // Request forms and line conventions.
            "Read project source as evidence",
            "Paths are project-relative",
            "use the outline tool to list a file's selectors",
            "exactly one request form",
            "one-based and inclusive",
            "omitted line_end reads through EOF",
            "{inputs, definitions}",
            "{input, definitions}",
            // Outline selector syntax.
            "kind[name]",
            "impl[Thing/Trait]",
            "kind@hash",
            "#N",
            "percent-encoded",
            // Definition selections and full definition names.
            "definition selection",
            "* matches any characters including dots",
            "full terminology name",
            // A short request example for each form.
            r#"{"file":"src/lib.rs","line_start":10,"line_end":20}"#,
            r#"{"file":"src/lib.rs","item":"impl[Thing]/fn[run]"}"#,
            r#"{"requirements":"requirements.cli.*"}"#,
            r#"{"definition":"glossary.command"}"#,
        ] {
            assert!(
                description.contains(phrase),
                "missing {phrase:?}: {description}"
            );
        }
        assert_eq!(
            habeas["parameters"]["properties"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            [
                "file",
                "line_start",
                "line_end",
                "item",
                "requirements",
                "definition"
            ]
        );
    }

    // {test}`requirements.authoring.outline_tool`
    // {test}`requirements.authoring.tool_descriptions.common_case`
    #[test]
    fn outline_tool_is_advertised_with_a_required_file_argument() {
        let definitions = definitions();
        let outline = definitions
            .iter()
            .find(|definition| definition["name"] == "outline")
            .expect("outline tool is advertised");
        assert_eq!(
            outline["parameters"],
            json!({
                "type": "object",
                "properties": {"file": {"type": "string"}},
                "required": ["file"],
                "additionalProperties": false
            })
        );
        let description = outline["description"].as_str().unwrap();
        for phrase in [
            "item selector",
            "List a file's outline",
            "file is a project-relative path",
            "kind",
            "name",
            "no line numbers",
            "no source text",
            "for habeas {file, item} reads",
            "read the nodes you rely on with habeas",
        ] {
            assert!(description.contains(phrase), "{description}");
        }
    }

    // {test}`requirements.authoring.reference_index`
    // {test}`requirements.authoring.tool_descriptions.common_case`
    #[test]
    fn references_tool_is_advertised_with_an_optional_selection() {
        let definitions = definitions();
        let references = definitions
            .iter()
            .find(|definition| definition["name"] == "references")
            .expect("references tool is advertised");
        assert_eq!(
            references["parameters"],
            json!({
                "type": "object",
                "properties": {
                    "selection": {"type": "string"},
                    "roles": {"type":"array","items":{"type":"string","enum":["impl","cite","test"]},"default":["impl","cite","test"]}
                },
                "additionalProperties": false
            })
        );
        let description = references["description"].as_str().unwrap();
        for phrase in [
            "{impl}",
            "{cite}",
            "{test}",
            "glob",
            ":impl:",
            "null",
            "Without selection, return every requirement and named example name with its kind and empty sites",
            "a full name or a glob where * matches any characters including dots",
            "roles filters sites and anchors by impl, cite, or test (default all three)",
            "Locate definitions and reference sites without reading their text",
            "[] returns names/kinds only",
            "{impl} marks implementations, {cite} related sources, and {test} tests",
            r#"{"selection":"requirements.cli.status","roles":["impl"]}"#,
            "Request needed source with habeas",
        ] {
            assert!(description.contains(phrase), "{phrase}: {description}");
        }
    }

    // {test}`requirements.authoring.outline_tool`
    // {test}`requirements.references.outlines`
    // {test}`requirements.authoring.no_line_numbers`
    // {test}`requirements.authoring.read_denial`
    #[test]
    fn outline_lists_nested_nodes_without_text_or_inputs() {
        let root = project();
        fs::write(
            root.path().join("src/lib.rs"),
            "/// Docs.\nstruct Thing;\n\nimpl Thing {\n    fn run(&self) {}\n}\n",
        )
        .unwrap();
        fs::write(
            root.path().join("src/trait_impl.rs"),
            "impl Trait for Thing { fn run(&self) {} }\n",
        )
        .unwrap();
        fs::write(root.path().join("notes.txt"), "alpha\n\nbeta\n").unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let outline = dispatch(&sources, "outline", &json!({"file":"src/lib.rs"})).unwrap();
        assert!(outline.inputs.is_empty());
        assert_eq!(
            outline.result,
            json!([
                {"item": "struct[Thing]", "kind": "struct", "name": "Thing", "children": []},
                {"item": "impl[Thing]", "kind": "impl", "name": "Thing", "children": [
                    {"item": "impl[Thing]/fn[run]", "kind": "fn", "name": "run", "children": []},
                ]},
            ])
        );
        assert!(!outline.result.to_string().contains("Docs"));
        let implementation =
            dispatch(&sources, "outline", &json!({"file":"src/trait_impl.rs"})).unwrap();
        assert_eq!(
            implementation.result,
            json!([{
                "item": "impl[Thing/Trait]", "kind": "impl", "name": "Thing/Trait", "children": [
                    {"item": "impl[Thing/Trait]/fn[run]", "kind": "fn", "name": "run", "children": []}
                ]
            }])
        );
        let plain = dispatch(&sources, "outline", &json!({"file":"notes.txt"})).unwrap();
        let entries = plain.result.as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|entry| entry["kind"] == "line"
            && entry["name"].is_null()
            && entry["item"].as_str().unwrap().starts_with("line@")));
        assert!(entries[1].get("line_start").is_none());
        for (arguments, message) in [
            (json!({"file":"missing.rs"}), "missing.rs"),
            (json!({}), "file"),
            (json!({"file":1}), "file"),
            (json!({"file":"src/lib.rs", "extra":true}), "extra"),
        ] {
            let error = dispatch(&sources, "outline", &arguments).err().unwrap();
            assert!(error.to_string().contains(message), "{arguments}: {error}");
        }
        let error = dispatch(&sources, "outline", &json!({})).err().unwrap();
        assert!(
            error.to_string().contains("file must be a string"),
            "{error}"
        );

        fs::write(
            root.path().join("habeas.toml"),
            "version = 1\n[evidence]\ndisallow = ['notes.txt']\n",
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let error = dispatch(&sources, "outline", &json!({"file":"notes.txt"}))
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
        assert!(error.to_string().contains("notes.txt"), "{error}");
        assert!(error.to_string().contains("disallowed"), "{error}");
    }

    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.delivered_inputs`
    // {test}`requirements.ci.outline_inputs`
    // {test}`requirements.authoring.no_line_numbers`
    #[test]
    fn line_range_reads_record_the_innermost_covering_nodes() {
        let root = project();
        let source = "use std::fmt;\n\nfn first() {}\n\nmod inner {\n    fn second() {\n        let x = 1;\n    }\n}\n";
        fs::write(root.path().join("src/lib.rs"), source).unwrap();
        let mut sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        // Blank lines between nodes do not select their common ancestor.
        let read = dispatch(
            &sources,
            "habeas",
            &json!({"file":"src/lib.rs","line_start":3,"line_end":7}),
        )
        .unwrap();
        let items = read.result["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|input| {
                assert!(input.get("line_start").is_none());
                input["item"].as_str().unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            items,
            ["fn[first]", "mod[inner]"],
            "line 5 is mod inner's own line, so the module is selected"
        );
        let nested = dispatch(
            &sources,
            "habeas",
            &json!({"file":"src/lib.rs","line_start":7,"line_end":7}),
        )
        .unwrap();
        assert_eq!(nested.result["inputs"][0]["item"], "mod[inner]/fn[second]");
        assert_eq!(
            nested.result["inputs"][0]["text"],
            json!(["    fn second() {", "        let x = 1;", "    }"])
        );
        sources.record_inputs(&nested.inputs);
        assert_eq!(
            sources.recorded_inputs(),
            [
                crate::inputs::outline_input(root.path(), "src/lib.rs", "mod[inner]/fn[second]")
                    .unwrap()
            ]
        );
        assert!(sources.recorded_inputs()[0].get("line_start").is_none());
        assert!(sources.recorded_inputs()[0].get("text").is_none());
        let whole = dispatch(
            &sources,
            "habeas",
            &json!({"file":"src/lib.rs","line_start":1}),
        )
        .unwrap();
        assert!(
            whole.result["inputs"][0]["item"]
                .as_str()
                .unwrap()
                .starts_with("use@")
        );
        assert_eq!(whole.result["inputs"].as_array().unwrap().len(), 3);
        fs::write(root.path().join("notes.txt"), "alpha\nbeta\ngamma\ndelta\n").unwrap();
        let plain = dispatch(
            &sources,
            "habeas",
            &json!({"file":"notes.txt","line_start":2,"line_end":3}),
        )
        .unwrap();
        let inputs = plain.result["inputs"].as_array().unwrap();
        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0]["text"], json!(["beta"]));
        assert_eq!(inputs[1]["text"], json!(["gamma"]));
        assert_ne!(inputs[0]["item"], inputs[1]["item"]);
        for input in inputs {
            assert!(input["item"].as_str().unwrap().starts_with("line@"));
            assert!(input.get("line_start").is_none());
            assert!(input.get("line_end").is_none());
        }
    }

    // {test}`requirements.authoring.text_search`
    #[test]
    fn context_windows_include_exactly_the_requested_lines_on_each_side() {
        let root = project();
        let source = "fn long() {\n    before_far();\n    before_three();\n    before_two();\n    before_one();\n    needle();\n    after_one();\n    after_two();\n    after_three();\n    after_far();\n}\n";
        fs::write(root.path().join("src/lib.rs"), source).unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        for context in [None, Some(0), Some(1), Some(2), Some(3)] {
            let mut arguments = json!({"pattern":"needle","path":"src/lib.rs"});
            if let Some(context) = context {
                arguments["context"] = json!(context);
            }
            let delivery = dispatch(&sources, "ripgrep", &arguments).unwrap();
            let count = context.unwrap_or(2);
            let expected = source
                .lines()
                .enumerate()
                .skip(5 - count)
                .take(2 * count + 1)
                .map(|(index, text)| json!({"text":text, "matched":index == 5}))
                .collect::<Vec<_>>();
            assert_eq!(
                delivery.result["matches"][0]["lines"],
                json!(expected),
                "{context:?}"
            );
        }
    }

    // {test}`requirements.authoring.text_search`
    // {test}`requirements.authoring.no_line_numbers`
    #[test]
    fn separate_windows_in_one_node_are_divided_by_an_elision_line() {
        let root = project();
        fs::write(
            root.path().join("src/lib.rs"),
            "fn long() {\n    needle(1);\n    a();\n    b();\n    c();\n    needle(2);\n    d();\n}\n",
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let search = dispatch(
            &sources,
            "ripgrep",
            &json!({"pattern":"needle","path":"src/lib.rs","context":0}),
        )
        .unwrap();
        assert_eq!(
            matched(&search),
            &vec![
                json!({"file": "src/lib.rs", "item": "fn[long]", "kind": "fn", "tag":"1", "lines": [
                    {"text": "    needle(1);", "matched": true},
                    {"text": ELISION, "matched": false},
                    {"text": "    needle(2);", "matched": true},
                ]})
            ]
        );
        // Adjacent windows are one run, with no elision.
        let joined = dispatch(
            &sources,
            "ripgrep",
            &json!({"pattern":"needle","path":"src/lib.rs","context":2}),
        )
        .unwrap();
        let lines = matched(&joined)[0]["lines"].as_array().unwrap().clone();
        assert!(
            lines.iter().all(|line| line["text"] != ELISION),
            "{lines:?}"
        );
        assert_eq!(lines.len(), 8);
        let defaults = dispatch(
            &sources,
            "ripgrep",
            &json!({"pattern":"needle","path":"src/lib.rs"}),
        )
        .unwrap();
        assert_eq!(
            defaults.result["matches"][0]["lines"],
            joined.result["matches"][0]["lines"]
        );
    }

    // {test}`requirements.authoring.search_accounting.excerpts`
    // {test}`requirements.authoring.text_search`
    // {test}`requirements.authoring.structural_search`
    // {test}`requirements.authoring.redaction`
    #[test]
    fn search_excerpts_stop_at_byte_boundaries_including_attached_comments_and_attributes() {
        let root = project();
        let token = format!("ghp_{}", "S".repeat(36));
        fs::write(root.path().join("src/lib.rs"), format!(
            "fn before() {{ unwanted_before(); }}\n\
             /// needle docs\n\
             #[inline]\n\
             fn target() {{\n    let café = \"{token}\";\n    needle();\n}}\n\
             fn after() {{ unwanted_after(); }}\n\
             fn left() {{ unwanted_left(); }} /* needle attached */ #[inline] fn right() {{ needle(); }} fn last() {{ unwanted_last(); }}\n"
        )).unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        for (tool, args) in [
            (
                "ripgrep",
                json!({"pattern":"needle","path":"src/lib.rs","context":99}),
            ),
            (
                "ast_grep",
                json!({"language":"rust","path":"src/lib.rs",
                "rule":{"rule":{"any":[{"pattern":"needle()"},{"kind":"block_comment","regex":"needle"}]}}}),
            ),
        ] {
            let result = dispatch(&sources, tool, &args).unwrap().result;
            let matches = result["matches"].as_array().unwrap();
            assert_eq!(matches.len(), 3, "{tool}: {matches:?}");
            assert_eq!(matches[0]["item"], "fn[target]");
            assert_eq!(matches[1]["item"], "fn[left]");
            assert_eq!(matches[2]["item"], "fn[right]");
            assert!(!matches[1]["lines"].to_string().contains("fn right"));
            assert!(!matches[2]["lines"].to_string().contains("fn left"));
            assert!(!matches[2]["lines"].to_string().contains("needle attached"));
            let text = result.to_string();
            for unwanted in [
                "unwanted_before",
                "unwanted_after",
                "unwanted_last",
                "fn last",
            ] {
                assert!(!text.contains(unwanted), "{tool}: {text}");
            }
            assert!(!text.contains(&token), "{text}");
            if tool == "ripgrep" {
                let lines = &matches[0]["lines"];
                assert!(lines.to_string().contains("needle docs"));
                assert!(lines.to_string().contains("#[inline]"));
                assert!(lines.to_string().contains("café"));
                assert!(lines.to_string().contains("[REDACTED:"));
            }
            assert!(matches[1]["lines"].to_string().contains("needle attached"));
            assert!(
                matches[2]["lines"]
                    .to_string()
                    .contains("#[inline] fn right")
            );
        }
        let only = dispatch(
            &sources,
            "ripgrep",
            &json!({"pattern":"café","path":"src/lib.rs","context":0}),
        )
        .unwrap();
        assert_eq!(only.result["matches"].as_array().unwrap().len(), 1);
        assert_eq!(only.result["matches"][0]["item"], "fn[target]");
        let attributes = dispatch(
            &sources,
            "ripgrep",
            &json!({
                "pattern":"inline","path":"src/lib.rs","context":0
            }),
        )
        .unwrap();
        assert_eq!(
            found(&attributes),
            ["src/lib.rs#fn[target]", "src/lib.rs#fn[right]"]
        );
    }

    // {test}`requirements.authoring.text_search`
    // {test}`requirements.authoring.search_accounting.excerpts`
    #[test]
    fn exact_match_ranges_preserve_regex_anchors_and_skip_unmatched_or_binary_sources() {
        let root = project();
        fs::write(
            root.path().join("src/lib.rs"),
            "fn exact() { needle(); }\nfn last() {}\n",
        )
        .unwrap();
        fs::write(root.path().join("src/broken.rs"), "fn incomplete( {\n").unwrap();
        fs::write(root.path().join("image.bin"), b"\0needle\xff").unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        for pattern in [
            r"^fn exact",
            r"needle\(\); \}$",
            r"(?i)EXACT",
            r"^fn exact.*$",
        ] {
            let result = dispatch(
                &sources,
                "ripgrep",
                &json!({"pattern":pattern,"context":10}),
            )
            .unwrap();
            assert_eq!(
                found(&result),
                ["src/lib.rs#fn[exact]"],
                "{pattern}: {}",
                result.result
            );
            assert_eq!(
                result.result["matches"][0]["lines"],
                json!([
                    {"text":"fn exact() { needle(); }","matched":true}
                ])
            );
        }
    }

    // {test}`requirements.authoring.finish_search`
    // {test}`requirements.authoring.finish_search.description`
    // {test}`requirements.authoring.tool_descriptions.common_case`
    #[test]
    fn finish_search_advertises_the_accounting_schema_and_selection_rule() {
        let tools = definitions();
        let finish = tools
            .iter()
            .find(|tool| tool["name"] == "finish_search")
            .unwrap();
        assert_eq!(
            finish["parameters"],
            json!({"type":"object","properties":{
            "search_id":{"type":"string","format":"uuid"},
            "used":{"type":"array","items":{"type":"string"}}
        },"required":["search_id","used"],"additionalProperties":false})
        );
        let description = finish["description"].as_str().unwrap();
        for phrase in [
            "snippets you used to justify your conclusion without an additional source read",
            "Entries whose source you also read may be omitted; including them is harmless",
            "Use used:[] when no entries qualify, including an empty search",
            "Every delivered search must be finished before submission",
            "{search_id, finished:true}",
            "no additional source is delivered",
            "search_id is the result's UUID and used is an array of its short entry tags",
            r#"{"search_id":"11111111-1111-4111-8111-111111111111","used":["1"]}"#,
        ] {
            assert!(description.contains(phrase), "{phrase}: {description}");
        }
        for name in ["ripgrep", "ast_grep"] {
            let description =
                tools.iter().find(|tool| tool["name"] == name).unwrap()["description"]
                    .as_str()
                    .unwrap();
            for phrase in [
                "tags whose snippets you used to justify your conclusion without an additional source read",
                "Entries whose source you also read may be omitted; including them is harmless",
                "Use an empty used list if no entries qualify",
                "Before submitting, call finish_search",
                "Use ripgrep for text patterns, ast_grep for syntax-tree rules, and references for requirement-reference sites",
            ] {
                assert!(
                    description.contains(phrase),
                    "{name}: {phrase}: {description}"
                );
            }
        }
    }

    // {test}`requirements.authoring.text_search`
    // {test}`requirements.references.outline_comments`
    #[test]
    fn search_returns_innermost_nodes_and_comment_owners() {
        let root = project();
        fs::write(
            root.path().join("src/lib.rs"),
            "fn outer() {\n    fn inner() {\n        needle();\n    }\n}\n\n// needle in a comment\nfn after() {}\n",
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let search = dispatch(
            &sources,
            "ripgrep",
            &json!({"pattern":"needle","path":"src/lib.rs","context":0}),
        )
        .unwrap();
        let items = matched(&search)
            .iter()
            .map(|input| input["item"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(items, ["fn[outer]/fn[inner]", "fn[after]"]);
        // A comment match belongs to its owner, and shows the matching line.
        assert_eq!(
            matched(&search)[1],
            json!({"file": "src/lib.rs", "item": "fn[after]", "kind": "fn", "tag":"2",
                "lines": [{"text": "// needle in a comment", "matched": true}]})
        );
        assert_eq!(
            matched(&search)[0]["lines"],
            json!([{"text": "        needle();", "matched": true}])
        );
        // Entries carry no hash: they are not evidence inputs.
        assert!(matched(&search)[0].get("hash").is_none());
    }

    // {test}`requirements.authoring.text_search`
    // {test}`requirements.authoring.tool_descriptions.common_case`
    #[test]
    fn ripgrep_tool_is_advertised_with_its_interface_and_conventions() {
        let definitions = definitions();
        let ripgrep = definitions
            .iter()
            .find(|definition| definition["name"] == "ripgrep")
            .expect("ripgrep tool is advertised");
        assert_eq!(
            ripgrep["parameters"],
            json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string"},
                    "path": {"type": "string", "default": "."},
                    "context": {"type": "integer", "minimum": 0, "default": 2},
                    "referencing": {
                        "type": "object",
                        "properties": {
                            "selection": {"type": "string"},
                            "roles": {
                                "type": "array",
                                "items": {"type": "string", "enum": ["impl", "test", "cite", "fixme"]},
                                "default": ["impl", "test", "cite"]
                            },
                            "scope": {"type": "string", "enum": ["node", "file"], "default": "file"}
                        },
                        "required": ["selection"],
                        "additionalProperties": false
                    }
                },
                "required": ["pattern"],
                "additionalProperties": false
            })
        );
        let description = ripgrep["description"].as_str().unwrap();
        for phrase in [
            "ripgrep regex syntax",
            "case-sensitive",
            "default 2",
            "zero means matches only",
            "overlapping windows are merged",
            "Windows stop at item boundaries",
            "Before submitting, call finish_search",
            "{search_id, matches: [{tag, file, item, kind, lines: [{text, matched}]}]}",
            "without line numbers",
            "separates windows within one node",
            "one entry for each innermost outline node containing a match",
            "a match in a comment belongs to the node the comment belongs to",
            "Results carry no definitions",
            "Selected items become evidence dependencies without another source read",
            "No matches returns an empty matches array",
            "With referencing {selection, roles, scope}",
            "Search a project-relative file or directory (default .)",
            "default [impl, test, cite]",
            "including :impl: and :test: declaration anchors",
            "scope node searches only the outline nodes that contain or are named by those references",
            "scope file (the default) their whole files",
            r#"{"pattern":"errors","referencing":{"selection":"requirements.ci.*","roles":["impl"],"scope":"node"}}"#,
        ] {
            assert!(description.contains(phrase), "{phrase:?} in {description}");
        }
    }

    // {test}`requirements.authoring.text_search`
    // {test}`requirements.authoring.read_denial`
    // {test}`requirements.authoring.additional_read`
    #[test]
    fn search_records_nothing_clamps_at_eof_and_reports_failures() {
        let root = project();
        fs::write(
            root.path().join("md/terms.md"),
            ":::{definition} value\n:phrase: value\nA useful value.\n:::\n",
        )
        .unwrap();
        fs::write(
            root.path().join("src/lib.rs"),
            format!(
                "fn first() {{}}\n\n// {}\nfn needle() {{}}\n",
                role("term", "terms.value")
            ),
        )
        .unwrap();
        // An allowed hidden directory is searched; an unallowed one is not.
        fs::create_dir_all(root.path().join(".generated")).unwrap();
        fs::write(root.path().join(".generated/out.txt"), "needle allowed\n").unwrap();
        fs::create_dir_all(root.path().join(".other")).unwrap();
        fs::write(root.path().join(".other/out.txt"), "needle hidden\n").unwrap();
        fs::write(
            root.path().join("habeas.toml"),
            "version = 1\n[evidence]\nallow = ['.generated']\n",
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let search = dispatch(
            &sources,
            "ripgrep",
            &json!({"pattern":"fn needle","path":"src","context":5}),
        )
        .unwrap();
        // Context includes the attached comment and stops at the item boundary.
        assert_eq!(
            matched(&search),
            &vec![
                json!({"file": "src/lib.rs", "item": "fn[needle]", "kind": "fn", "tag":"1",
                "lines": [
                    {"text": "// {term}`terms.value`", "matched": false},
                    {"text": "fn needle() {}", "matched": true},
                ]})
            ]
        );
        // The terminology the node uses is neither supplied nor recorded.
        assert!(search.result.get("definitions").is_none());
        assert!(sources.recorded_inputs().is_empty());
        let hidden = dispatch(
            &sources,
            "ripgrep",
            &json!({"pattern":"needle (allowed|hidden)","context":0}),
        )
        .unwrap();
        let files = matched(&hidden)
            .iter()
            .map(|input| input["file"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(files, [".generated/out.txt"]);
        // Invalid arguments are explanatory tool errors.
        for (arguments, message) in [
            (json!({}), "pattern must be a string"),
            (json!({"pattern": 1}), "pattern must be a string"),
            (json!({"pattern":"x","path":1}), "path must be a string"),
            (
                json!({"pattern":"x","context":1.5}),
                "context must be a nonnegative integer",
            ),
            (
                json!({"pattern":"x","extra":true}),
                "unexpected tool argument \"extra\"",
            ),
            (json!({"pattern":"("}), "invalid ripgrep pattern"),
        ] {
            let error = dispatch(&sources, "ripgrep", &arguments).err().unwrap();
            assert!(error.to_string().contains(message), "{arguments}: {error}");
        }
        // A matched file that cannot be parsed fails the search with an
        // explanatory error rather than silently dropping the match.
        fs::write(root.path().join("src/broken.rs"), "fn needle( {\n").unwrap();
        let error = dispatch(
            &sources,
            "ripgrep",
            &json!({"pattern":"fn needle","path":"src"}),
        )
        .err()
        .unwrap();
        assert!(error.to_string().contains("syntax error"), "{error}");
    }

    /// A search result's entries; searches record no inputs.
    // {cite}`habeas:util`
    fn matched(delivery: &ToolDelivery) -> &Vec<Value> {
        assert!(delivery.inputs.is_empty(), "searches record no inputs");
        delivery.result["matches"]
            .as_array()
            .expect("search matches")
    }

    /// The items of a search result's entries, as `file#item`.
    // {cite}`habeas:util`
    fn found(delivery: &ToolDelivery) -> Vec<String> {
        assert!(delivery.inputs.is_empty(), "searches record no inputs");
        delivery.result["matches"]
            .as_array()
            .unwrap()
            .iter()
            .map(|input| {
                format!(
                    "{}#{}",
                    input["file"].as_str().unwrap(),
                    input["item"].as_str().unwrap()
                )
            })
            .collect()
    }

    /// A project whose sources reference `requirements.ready` and
    /// `requirements.other` in several roles, each source calling `needle()`.
    // {cite}`habeas:util`
    fn referencing_project() -> tempfile::TempDir {
        let root = project();
        fs::write(
            root.path().join("md/requirements.md"),
            ":::{req} ready\nThe project MUST be ready.\n:::\n\n:::{req} other\n:impl: src/other.rs\nOther.\n:::\n\n:::{req} third\nThird.\n:::\n",
        )
        .unwrap();
        fs::write(
            root.path().join("src/lib.rs"),
            format!(
                "// {}\nfn ready() {{\n    needle();\n}}\n\nfn unrelated() {{\n    needle();\n}}\n",
                role("impl", "requirements.ready")
            ),
        )
        .unwrap();
        fs::write(
            root.path().join("src/cite.rs"),
            format!(
                "fn before() {{\n    needle();\n}}\n\n// {}\nfn cited() {{\n    needle();\n}}\n",
                role("cite", "requirements.ready")
            ),
        )
        .unwrap();
        fs::write(
            root.path().join("src/other.rs"),
            "fn elsewhere() {\n    needle();\n}\n",
        )
        .unwrap();
        fs::write(
            root.path().join("src/plain.rs"),
            "fn plain() {\n    needle();\n}\n",
        )
        .unwrap();
        root
    }

    // {test}`requirements.authoring.text_search`
    #[test]
    fn text_search_selects_example_test_references_and_test_anchors() {
        let root = referencing_project();
        let declarations = root.path().join("md/requirements.md");
        let mut text = fs::read_to_string(&declarations).unwrap();
        text.push_str("\n:::{example} case\n:test: src/other.rs\nA named case. {cite}`requirements.ready`\n:::\n");
        fs::write(declarations, text).unwrap();
        fs::write(
            root.path().join("src/example_test.rs"),
            format!(
                "// {}\nfn example_test() {{ needle(); }}\n",
                role("test", "requirements.case")
            ),
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let result = dispatch(&sources, "ripgrep", &json!({"pattern":"needle","context":0,"referencing":{"selection":"requirements.case","roles":["test"]}})).unwrap();
        assert_eq!(
            found(&result),
            [
                "src/example_test.rs#fn[example_test]",
                "src/other.rs#fn[elsewhere]"
            ]
        );
        let no_impl = dispatch(&sources, "ripgrep", &json!({"pattern":"needle","referencing":{"selection":"requirements.case","roles":["impl"]}})).unwrap();
        assert!(found(&no_impl).is_empty());
    }

    // {test}`requirements.authoring.text_search`
    // {test}`requirements.references.definition_globs`
    // {test}`requirements.references.anchors`
    #[test]
    fn ripgrep_referencing_searches_only_referencing_files_or_nodes() {
        let root = referencing_project();
        fs::write(
            root.path().join("src/gap.rs"),
            "// :::{fixme} requirements.ready\n// This boundary needs a test.\n// :::\nfn gap() { needle(); }\n",
        ).unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let search = |referencing: Value, context: u64| {
            dispatch(
                &sources,
                "ripgrep",
                &json!({"pattern":"needle","context":context,"referencing":referencing}),
            )
        };
        // By default, every impl, test, or cite reference selects its whole file.
        assert_eq!(
            found(&search(json!({"selection":"requirements.ready"}), 0).unwrap()),
            [
                "src/cite.rs#fn[before]",
                "src/cite.rs#fn[cited]",
                "src/lib.rs#fn[ready]",
                "src/lib.rs#fn[unrelated]"
            ]
        );
        for scope in ["file", "node"] {
            assert_eq!(
                found(
                    &search(
                        json!({"selection":"requirements.ready","roles":["fixme"],"scope":scope}),
                        0
                    )
                    .unwrap()
                ),
                ["src/gap.rs#fn[gap]"]
            );
        }
        // Roles narrow the references; a glob selects several definitions,
        // including declaration anchors.
        assert_eq!(
            found(&search(json!({"selection":"requirements.*","roles":["impl"]}), 0).unwrap()),
            [
                "src/lib.rs#fn[ready]",
                "src/lib.rs#fn[unrelated]",
                "src/other.rs#fn[elsewhere]"
            ]
        );
        // Node scope searches only the nodes the references belong to, and
        // context windows stop at their boundaries.
        let node = search(json!({"selection":"requirements.ready","scope":"node"}), 5).unwrap();
        assert_eq!(
            found(&node),
            ["src/cite.rs#fn[cited]", "src/lib.rs#fn[ready]"]
        );
        // `path` still limits the search.
        let limited = dispatch(
            &sources,
            "ripgrep",
            &json!({"pattern":"needle","path":"src/lib.rs","referencing":{"selection":"requirements.*"}}),
        )
        .unwrap();
        assert_eq!(found(&limited).len(), 2);
        // A matching definition with no references in the roles finds nothing.
        let none = search(json!({"selection":"requirements.third"}), 0).unwrap();
        assert_eq!(none.result["matches"], json!([]));
        for (referencing, message) in [
            (
                json!({"selection":"requirements.none"}),
                "matches no requirement or named example",
            ),
            (
                json!({"selection":"requirements.ready","roles":["bogus"]}),
                "unknown referencing role",
            ),
            (
                json!({"selection":"requirements.ready","roles":"impl"}),
                "roles must be an array",
            ),
            (
                json!({"selection":"requirements.ready","scope":"dir"}),
                "unknown referencing scope",
            ),
            (
                json!({"selection":"requirements.ready","extra":1}),
                "unexpected referencing argument",
            ),
            (
                json!({"roles":["impl"]}),
                "referencing.selection must be a string",
            ),
            (json!("requirements.ready"), "referencing must be an object"),
        ] {
            let error = search(referencing.clone(), 0).err().unwrap();
            assert!(
                error.to_string().contains(message),
                "{referencing}: {error}"
            );
        }
    }

    // {test}`requirements.authoring.text_search`
    // {test}`requirements.references.anchors`
    #[test]
    fn ripgrep_node_scope_searches_only_the_node_a_selector_anchor_names() {
        let root = referencing_project();
        fs::write(
            root.path().join("md/requirements.md"),
            ":::{req} ready\nThe project MUST be ready.\n:::\n\n:::{req} anchored\n:impl: src/plain.rs#fn[second]\nAnchored.\n:::\n\n:::{req} stale\n:impl: src/plain.rs#fn[gone]\nStale.\n:::\n",
        )
        .unwrap();
        fs::write(
            root.path().join("src/plain.rs"),
            "fn plain() {\n    needle();\n}\n\nfn second() {\n    needle();\n}\n",
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let search = |selection: &str, scope: &str| {
            dispatch(
                &sources,
                "ripgrep",
                &json!({"pattern":"needle","context":5,"referencing":{"selection":selection,"scope":scope}}),
            )
            .unwrap()
        };
        // Node scope searches only the node the anchor names, and the context
        // window stops at its boundary rather than reaching `plain`.
        let node = search("requirements.anchored", "node");
        assert_eq!(found(&node), ["src/plain.rs#fn[second]"]);
        // File scope searches the anchored file.
        assert_eq!(
            found(&search("requirements.anchored", "file")),
            ["src/plain.rs#fn[plain]", "src/plain.rs#fn[second]"]
        );
        // A selector that no longer resolves names no node to search.
        assert_eq!(
            found(&search("requirements.stale", "node")),
            Vec::<String>::new()
        );
    }

    // {test}`requirements.ci.schema`
    #[test]
    fn project_local_absolute_paths_are_recorded_relative_to_the_project() {
        let root = project();
        fs::write(root.path().join("src/lib.rs"), "fn work() {}\n").unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        for path in [
            root.path().join("src/lib.rs"),
            fs::canonicalize(root.path()).unwrap().join("src/./lib.rs"),
        ] {
            let file = path.to_str().unwrap();
            for arguments in [
                json!({"file": file, "item": "fn[work]"}),
                json!({"file": file, "line_start": 1}),
            ] {
                let delivery = dispatch(&sources, "habeas", &arguments).unwrap();
                let recorded = delivery
                    .inputs
                    .iter()
                    .map(crate::inputs::recorded_input)
                    .filter(|input| input.get("item").is_some())
                    .collect::<Vec<_>>();
                assert_eq!(recorded.len(), 1, "{arguments}: {recorded:?}");
                assert_eq!(recorded[0]["file"], "src/lib.rs", "{arguments}");
                assert_eq!(recorded[0]["item"], "fn[work]", "{arguments}");
            }
        }
    }

    // {test}`requirements.authoring.not_listed`
    // {test}`requirements.authoring.search_accounting.entry_tags`
    #[test]
    // {test}`requirements.authoring.read_denial`
    fn listings_and_searches_name_untracked_files_without_their_contents() {
        let root = project();
        fs::write(root.path().join("src/lib.rs"), "fn tracked() {}\n").unwrap();
        fs::write(
            root.path().join("habeas.toml"),
            "version = 1\n[evidence]\ndisallow = ['secret.rs']\n",
        )
        .unwrap();
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["add", "."]);
        fs::write(root.path().join("untracked.rs"), "fn needle() {}\n").unwrap();
        fs::create_dir(root.path().join("drafts")).unwrap();
        fs::write(root.path().join("drafts/plan.txt"), "a needle here\n").unwrap();
        fs::write(root.path().join("secret.rs"), "fn needle() {}\n").unwrap();
        fs::create_dir_all(root.path().join("evidence")).unwrap();
        fs::write(root.path().join("evidence/note.txt"), "needle\n").unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let named = |result: &Value| {
            result["not_listed"]
                .as_array()
                .map(|entries| {
                    entries
                        .iter()
                        .map(|entry| {
                            assert_eq!(entry["reason"], "untracked");
                            assert!(entry.get("tag").is_none());
                            entry["path"].as_str().unwrap().to_owned()
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };
        let listing = dispatch(&sources, "list_files", &json!({})).unwrap();
        assert_eq!(named(&listing.result), ["drafts", "untracked.rs"]);
        assert!(
            listing.result["entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["path"] == "src")
        );
        let search = dispatch(&sources, "ripgrep", &json!({"pattern": "needle"})).unwrap();
        assert_eq!(named(&search.result), ["drafts/plan.txt", "untracked.rs"]);
        let text = search.result.to_string();
        assert!(
            !text.contains("a needle here") && !text.contains("fn needle"),
            "{text}"
        );
        assert!(search.inputs.is_empty());
        let rust = dispatch(
            &sources,
            "ast_grep",
            &json!({"language": "rust", "rule": {"rule": {"pattern": "fn needle() {}"}}}),
        )
        .unwrap();
        assert_eq!(named(&rust.result), ["untracked.rs"]);
        for delivery in [&listing, &rust] {
            assert!(delivery.inputs.is_empty());
            let text = delivery.result.to_string();
            assert!(
                !text.contains("fn needle") && !text.contains("a needle here"),
                "{text}"
            );
        }

        // Nothing untracked matches: no not_listed.
        let none = dispatch(&sources, "ripgrep", &json!({"pattern": "tracked\\(\\)"})).unwrap();
        assert!(none.result.get("not_listed").is_none(), "{}", none.result);
        let inside = dispatch(&sources, "list_files", &json!({"path": "src"})).unwrap();
        assert!(
            inside.result.get("not_listed").is_none(),
            "{}",
            inside.result
        );
        fs::write(root.path().join("src/draft.rs"), "fn needle() {}\n").unwrap();
        for path in [
            "./src".to_owned(),
            "src/../src".to_owned(),
            root.path().join("src").to_string_lossy().into_owned(),
        ] {
            for (tool, arguments) in [
                ("list_files", json!({"path":path})),
                ("ripgrep", json!({"path":path,"pattern":"needle"})),
                (
                    "ast_grep",
                    json!({"path":path,"language":"rust","rule":{"rule":{"pattern":"fn needle() {}"}}}),
                ),
            ] {
                let delivery = dispatch(&sources, tool, &arguments).unwrap();
                assert_eq!(named(&delivery.result), ["src/draft.rs"], "{tool}: {path}");
                assert!(delivery.inputs.is_empty());
            }
        }
    }

    // {test}`requirements.authoring.generated_inputs`
    #[test]
    fn an_external_build_directory_lists_its_generated_output() {
        let root = project();
        let external = tempfile::tempdir().unwrap();
        fs::write(root.path().join("src/lib.rs"), "pub fn ready() {}\n").unwrap();
        fs::write(
            root.path().join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = 'fixture'\nversion = '0.1.0'\n",
        )
        .unwrap();
        fs::create_dir_all(root.path().join(".cargo")).unwrap();
        fs::write(
            root.path().join(".cargo/config.toml"),
            format!(
                "[build]\ntarget-dir = {}\n",
                toml::Value::String(external.path().display().to_string())
            ),
        )
        .unwrap();
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["add", "."]);
        let out = external.path().join("debug/build/fixture-hash/out");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("generated.rs"), "pub fn generated_needle() {}\n").unwrap();
        fs::write(
            out.parent().unwrap().join("root-output"),
            out.to_str().unwrap(),
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let listing = dispatch(&sources, "list_files", &json!({})).unwrap();
        let out = fs::canonicalize(&out).unwrap();
        let paths = listing
            .result
            .get("entries")
            .unwrap_or(&listing.result)
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["path"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert!(
            paths.iter().any(|path| std::path::Path::new(path) == out),
            "{paths:?}"
        );
        let search = dispatch(
            &sources,
            "ripgrep",
            &json!({"pattern": "generated_needle", "path": out.to_str().unwrap()}),
        )
        .unwrap();
        assert_eq!(
            search.result["matches"].as_array().unwrap().len(),
            1,
            "{}",
            search.result
        );
    }

    // {test}`requirements.authoring.file_discovery`
    // {test}`requirements.authoring.read_denial`
    #[test]
    fn failed_listings_return_explanatory_tool_errors() {
        let root = project();
        fs::write(root.path().join("src/lib.rs"), "fn work() {}\n").unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        for (arguments, message) in [
            (json!({"path": "missing"}), "cannot"),
            (json!({"path": "src/lib.rs"}), "cannot list"),
            (json!({"path": 3}), "path"),
            (json!({"path": ".", "extra": true}), "extra"),
        ] {
            let error = dispatch(&sources, "list_files", &arguments)
                .err()
                .unwrap_or_else(|| panic!("{arguments} fails"));
            assert_ne!(error.kind(), io::ErrorKind::PermissionDenied, "{arguments}");
            assert!(error.to_string().contains(message), "{arguments}: {error}");
        }
    }

    // {test}`requirements.authoring.tool_descriptions.common_case`
    // {test}`requirements.authoring.tool_descriptions.special_cases`
    #[test]
    fn descriptions_give_each_argument_and_the_result_form() {
        for definition in definitions() {
            let name = definition["name"].as_str().unwrap();
            let description = definition["description"].as_str().unwrap();
            // At least a few sentences.
            assert!(
                description.matches(". ").count() >= 2,
                "{name}: {description}"
            );
            for argument in definition["parameters"]["properties"]
                .as_object()
                .unwrap()
                .keys()
            {
                assert!(
                    description.contains(argument.as_str()),
                    "{name} omits {argument}"
                );
            }
            let result = match name {
                "habeas" => "{input, definitions}",
                "list_files" => "{path, kind}",
                "outline" => "item selector",
                "ripgrep" | "ast_grep" => {
                    "{search_id, matches: [{tag, file, item, kind, lines: [{text, matched}]}]}"
                }
                "references" => "definition name, role, file, and item",
                "next_page" => "same shape",
                "github" => "state (open or closed), title, and body",
                "finish_search" => "{search_id, finished:true}",
                _ => continue,
            };
            assert!(
                description.contains(result),
                "{name} omits its result: {description}"
            );
            let phrases: &[&str] = match name {
                "list_files" => &[
                    "immediate files and directories",
                    "Use list_files for directory entries and outline for selectable nodes in one file",
                    "project-relative",
                    "default .",
                    "listed or read",
                    "not evidence inputs",
                ],
                "next_page" => &[
                    "following page",
                    "habeas (line-range form), list_files, outline, ripgrep, ast_grep, or references",
                    "same shape",
                    "token is the page's next value",
                    "opaque",
                    r#"{"token":"abc123"}"#,
                ],
                "github" => &[
                    "<owner>/<repo>#<number>",
                    "issue or pull request",
                    "one a {fixme} option line names",
                ],
                _ => &[],
            };
            for phrase in phrases {
                assert!(
                    description.contains(phrase),
                    "{name}: missing {phrase}: {description}"
                );
            }
        }
    }

    // {test}`requirements.authoring.redaction_notice`
    // {test}`requirements.authoring.tool_descriptions.special_cases`
    #[test]
    fn only_results_with_redacted_text_carry_the_redaction_note() {
        let root = project();
        let token = crate::redaction::tests::github_token();
        fs::write(
            root.path().join("src/lib.rs"),
            format!("fn client() -> &'static str {{\n    \"{token}\"\n}}\n\nfn plain() {{}}\n"),
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let mut pages = ResultPages::default();
        let mut read = |arguments: Value| {
            dispatch_paged(&sources, &mut pages, "habeas", &arguments)
                .unwrap()
                .result
        };
        let redacted = read(json!({"file": "src/lib.rs", "item": "fn[client]"}));
        assert!(redacted.to_string().contains("[REDACTED:"), "{redacted}");
        let note = redacted["note"].as_str().unwrap();
        assert!(note.contains("[REDACTED:<kind>] replaces credential-shaped text"));
        assert!(note.contains("the original text is not available"));
        let plain = read(json!({"file": "src/lib.rs", "item": "fn[plain]"}));
        assert!(plain.get("note").is_none(), "{plain}");
        let search = dispatch_paged(
            &sources,
            &mut pages,
            "ripgrep",
            &json!({"pattern": "client", "path": "src/lib.rs"}),
        )
        .unwrap()
        .result;
        assert_eq!(search["note"], REDACTION_NOTE, "{search}");
        // Source that itself contains the marker text was not redacted.
        fs::write(
            root.path().join("src/notes.rs"),
            "// Secrets show as [REDACTED:kind] in tool output.\nfn notes() {}\n",
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let literal = dispatch_paged(
            &sources,
            &mut ResultPages::default(),
            "habeas",
            &json!({"file": "src/notes.rs", "item": "fn[notes]"}),
        )
        .unwrap()
        .result;
        assert!(literal.to_string().contains("[REDACTED:kind]"), "{literal}");
        assert!(literal.get("note").is_none(), "{literal}");
        // So is requirement text that contains it, which names no file.
        fs::write(
            root.path().join("md/requirements.md"),
            ":::{req} ready\nSecrets MUST read [REDACTED:kind].\n:::\n",
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let requirement = dispatch_paged(
            &sources,
            &mut ResultPages::default(),
            "habeas",
            &json!({"requirements": "requirements.ready"}),
        )
        .unwrap()
        .result;
        assert!(
            requirement.to_string().contains("[REDACTED:kind]"),
            "{requirement}"
        );
        assert!(requirement.get("note").is_none(), "{requirement}");
        fs::write(
            root.path().join("md/requirements.md"),
            format!(":::{{req}} ready\nThe credential {token} MUST be private.\n:::\n"),
        )
        .unwrap();
        fs::write(
            root.path().join("md/terms.md"),
            format!(":::{{definition}} secret\n:phrase: secret\nA credential {token}.\n:::\n"),
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        for args in [
            json!({"requirements":"requirements.ready"}),
            json!({"definition":"terms.secret"}),
        ] {
            let result = dispatch(&sources, "habeas", &args).unwrap().result;
            assert!(!result.to_string().contains(&token));
            assert!(result.to_string().contains("[REDACTED:"));
            assert_eq!(result["note"], REDACTION_NOTE);
        }
    }

    // {test}`requirements.authoring.result_pages`
    // {test}`requirements.authoring.tool_descriptions.special_cases`
    #[test]
    fn descriptions_leave_paging_policy_redaction_and_failures_to_results() {
        let definitions = definitions();
        let next = definitions
            .iter()
            .find(|definition| definition["name"] == "next_page")
            .unwrap();
        assert_eq!(
            next["parameters"],
            json!({"type":"object","properties":{"token":{"type":"string"}},"required":["token"],"additionalProperties":false})
        );
        for definition in &definitions {
            let name = definition["name"].as_str().unwrap();
            let description = definition["description"].as_str().unwrap().to_lowercase();
            for special in [
                "kib",
                "returned in pages",
                "redact",
                "credential",
                "denied",
                "disallow",
                "habeas.toml",
                "aborts",
                "untracked",
                "build output",
                "outside the project",
                "tool error",
                "policy",
                "ignore rules",
                "exclusion",
                "permitted",
            ] {
                assert!(
                    !description.contains(special),
                    "{name} explains {special:?}: {description}"
                );
            }
        }
    }

    // {test}`requirements.authoring.structural_search`
    // {test}`requirements.authoring.result_pages`
    // {test}`requirements.authoring.tool_descriptions.common_case`
    #[test]
    fn ast_grep_tool_is_advertised_with_both_forms_and_rule_syntax() {
        let definitions = definitions();
        let names = definitions
            .iter()
            .map(|definition| definition["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "habeas",
                "list_files",
                "outline",
                "ripgrep",
                "ast_grep",
                "references",
                "github",
                "next_page",
                "finish_search"
            ]
        );
        let ast_grep = &definitions[4];
        let ripgrep = &definitions[3];
        assert_eq!(
            ast_grep["parameters"],
            json!({
                "type": "object",
                "properties": {
                    "language": {
                        "type": "string",
                        "enum": ["rust", "python", "typescript", "tsx", "css", "markdown", "toml", "yaml"]
                    },
                    "rule": {"type": "object"},
                    "path": {"type": "string", "default": "."},
                    "referencing": ripgrep["parameters"]["properties"]["referencing"],
                    "query": {"type": "string"}
                },
                "additionalProperties": false
            })
        );
        let description = ast_grep["description"].as_str().unwrap();
        for phrase in [
            "{language, rule, path (default .), referencing (optional)} or {query}",
            "matched with Habeas's pinned grammars",
            "Search syntax trees in process with an ast-grep rule",
            "path selects a project-relative file or directory (default .)",
            "language is one of rust, python, typescript, tsx, css, markdown, toml, or yaml",
            "rule (required), and optional utils (named rules) and constraints",
            "pattern (code with metavariables",
            "kind (a grammar node kind",
            "inside, has, precedes, and follows",
            "all, any, not, and matches",
            "the context and selector form",
            "path selects a project-relative file or directory (default .); referencing works as for ripgrep, and results are ordered by path and then line",
            "named <full-name>/<query-name>, exactly as validation runs it, including its paths, exclude, and within, and returns each match's site",
            "Before submitting, call finish_search",
            "{search_id, matches: [{tag, file, item, kind, lines: [{text, matched}]}]}",
            "one entry for each innermost outline node containing a match",
            "a match in a comment belongs to the node the comment belongs to",
            "the matched lines, without line numbers",
            "Results carry no definitions",
            "Selected items become evidence dependencies without another source read",
            "No matches returns an empty matches array",
            r#"{"language":"rust","rule":{"rule":{"pattern":"$X.unwrap()","inside":{"kind":"function_item","stopBy":"end"}}}}"#,
            r#"{"query":"requirements.ci.errors/error_accumulators"}"#,
        ] {
            assert!(description.contains(phrase), "{phrase:?} in {description}");
        }
    }

    // {test}`requirements.authoring.structural_search`
    // {test}`requirements.references.outline_comments`
    // {test}`requirements.authoring.redaction`
    // {test}`requirements.authoring.read_denial`
    #[test]
    fn ast_grep_returns_innermost_nodes_of_the_language_and_reports_failures() {
        let root = project();
        fs::write(
            root.path().join("md/terms.md"),
            ":::{definition} value\n:phrase: value\nA useful value.\n:::\n",
        )
        .unwrap();
        let token = ["gh", "p_", "abcdefghijklmnopqrstuvwxyz0123456789"].concat();
        fs::write(
            root.path().join("src/lib.rs"),
            format!(
                "// {}\nfn first() {{\n    let x = value.unwrap();\n}}\n\n/// Calls unwrap in docs.\nfn second() {{\n    other(\"{token}\").unwrap();\n}}\n\nmod inner {{\n    fn third() {{\n        a.unwrap();\n        b.unwrap();\n    }}\n}}\n",
                role("term", "terms.value")
            ),
        )
        .unwrap();
        fs::write(
            root.path().join("src/a.rs"),
            "fn a() {\n    z.unwrap();\n}\n",
        )
        .unwrap();
        fs::write(root.path().join("src/tool.py"), "value.unwrap()\n").unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let rust = |rule: Value, path: &str| {
            dispatch(
                &sources,
                "ast_grep",
                &json!({"language":"rust","rule":rule,"path":path}),
            )
        };
        let search = rust(json!({"rule":{"pattern":"$X.unwrap()"}}), ".").unwrap();
        // Only Rust files are searched, ordered by path and then line, and
        // each node once.
        assert_eq!(
            found(&search),
            [
                "src/a.rs#fn[a]",
                "src/lib.rs#fn[first]",
                "src/lib.rs#fn[second]",
                "src/lib.rs#mod[inner]/fn[third]"
            ]
        );
        // Each entry shows the matched lines, redacted; no terminology is
        // supplied.
        let second = &matched(&search)[2]["lines"];
        assert_eq!(second[0]["matched"], true);
        assert!(second.to_string().contains("[REDACTED:"), "{second}");
        assert!(!second.to_string().contains(&token), "{second}");
        assert_eq!(
            matched(&search)[3]["lines"],
            json!([
                {"text": "        a.unwrap();", "matched": true},
                {"text": "        b.unwrap();", "matched": true},
            ])
        );
        assert!(search.result.get("definitions").is_none());
        // A match in a comment belongs to the comment's owner; utils and
        // constraints are accepted.
        let comment = rust(
            json!({
                "rule": {"matches": "doc"},
                "utils": {"doc": {"kind": "line_comment", "regex": "unwrap"}}
            }),
            "src/lib.rs",
        )
        .unwrap();
        assert_eq!(found(&comment), ["src/lib.rs#fn[second]"]);
        let constrained = rust(
            json!({
                "rule": {"pattern": "$X.unwrap()"},
                "constraints": {"X": {"regex": "^[ab]$"}}
            }),
            "src",
        )
        .unwrap();
        assert_eq!(found(&constrained), ["src/lib.rs#mod[inner]/fn[third]"]);
        let python = dispatch(
            &sources,
            "ast_grep",
            &json!({"language":"python","rule":{"rule":{"pattern":"$X.unwrap()"}}}),
        )
        .unwrap();
        assert_eq!(found(&python), ["src/tool.py#"]);
        let none = rust(json!({"rule":{"pattern":"missing()"}}), ".").unwrap();
        assert_eq!(none.result["matches"], json!([]));
        assert!(none.inputs.is_empty());
        for (arguments, message) in [
            (
                json!({"language":"cobol","rule":{"rule":{"kind":"x"}}}),
                "unknown structural search language",
            ),
            (
                json!({"rule":{"rule":{"kind":"x"}}}),
                "language must be a string",
            ),
            (
                json!({"language":"rust","rule":"x"}),
                "rule must be an object",
            ),
            (
                json!({"language":"rust","rule":{"rule":{"kind":"no_such_kind"}}}),
                "invalid ast-grep rule",
            ),
            (
                json!({"language":"rust","rule":{"pattern":"x"}}),
                "unexpected key \"pattern\"",
            ),
            (
                json!({"language":"rust","rule":{"rule":{"kind":"x"}},"extra":1}),
                "unexpected tool argument \"extra\"",
            ),
            (
                json!({"query":"x","language":"rust"}),
                "unexpected tool argument \"language\"",
            ),
            (
                json!({"query":"x"}),
                "must have the form <full-name>/<query-name>",
            ),
            (
                json!({"query":"requirements.none/sites"}),
                "names no declared requirement",
            ),
            (
                json!({"query":"requirements.ready/sites"}),
                "declares no query named \"sites\"",
            ),
        ] {
            let error = dispatch(&sources, "ast_grep", &arguments).err().unwrap();
            assert!(error.to_string().contains(message), "{arguments}: {error}");
        }
        // A matched file that cannot be parsed fails the search.
        fs::write(
            root.path().join("src/broken.rs"),
            "fn broken() {\n    q.unwrap();\n}\n}\n",
        )
        .unwrap();
        let error = rust(json!({"rule":{"pattern":"$X.unwrap()"}}), "src")
            .err()
            .unwrap();
        assert!(error.to_string().contains("syntax error"), "{error}");
        assert!(sources.recorded_inputs().is_empty());
    }

    // {test}`requirements.authoring.structural_search`
    // {test}`requirements.references.definition_globs`
    // {test}`requirements.references.anchors`
    #[test]
    fn ast_grep_referencing_restricts_the_search_as_ripgrep_does() {
        let root = referencing_project();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let search = |referencing: Value| {
            dispatch(
                &sources,
                "ast_grep",
                &json!({"language":"rust","rule":{"rule":{"pattern":"needle()"}},"referencing":referencing}),
            )
        };
        assert_eq!(
            found(&search(json!({"selection":"requirements.ready","roles":["cite"]})).unwrap()),
            ["src/cite.rs#fn[before]", "src/cite.rs#fn[cited]"]
        );
        assert_eq!(
            found(&search(json!({"selection":"requirements.*","scope":"node"})).unwrap()),
            [
                "src/cite.rs#fn[cited]",
                "src/lib.rs#fn[ready]",
                "src/other.rs#fn[elsewhere]"
            ]
        );
        let error = search(json!({"selection":"requirements.ready","scope":"dir"}))
            .err()
            .unwrap();
        assert!(
            error.to_string().contains("unknown referencing scope"),
            "{error}"
        );
        for (referencing, message) in [
            (json!({"scope":"node"}), "selection must be a string"),
            (
                json!({"selection":"requirements.ready","roles":["bogus"]}),
                "unknown referencing role",
            ),
        ] {
            let error = search(referencing).err().unwrap();
            assert!(error.to_string().contains(message), "{error}");
        }
    }

    // {test}`requirements.authoring.structural_search`
    // {test}`requirements.references.distributed`
    #[test]
    fn ast_grep_queries_run_declared_queries_and_return_their_sites() {
        let root = project();
        fs::write(
            root.path().join("md/requirements.md"),
            concat!(
                ":::{req} ready\nThe project MUST be ready.\n:::\n\n",
                "::::{req} errors\nAccumulating functions MUST keep going.\n\n",
                ":::{sites} accumulators\n```yaml\nlanguage: rust\npaths: [src]\nexclude: [src/skip.rs]\n",
                "rule:\n  kind: function_item\n  has:\n    stopBy: end\n    kind: identifier\n    regex: \"^errors$\"\n",
                "match: [\"fn f(errors: u8) {}\"]\nno_match: [\"fn f() {}\"]\n```\n:::\n\n",
                ":::{violations} exits\n```yaml\nlanguage: rust\nwithin: accumulators\n",
                "rule:\n  kind: return_expression\n",
                "match: [\"fn f(errors: u8) { return; }\"]\nno_match: [\"fn f() { return; }\"]\n```\n:::\n",
                "::::\n\n",
                "::::{req} broken\nBroken.\n\n:::{sites} bad\nNo code block.\n:::\n::::\n",
            ),
        )
        .unwrap();
        fs::write(
            root.path().join("md/terms.md"),
            ":::{definition} value\n:phrase: value\nA useful value.\n:::\n",
        )
        .unwrap();
        fs::write(
            root.path().join("src/lib.rs"),
            format!(
                "fn check(errors: &mut Vec<String>) {{\n    // {}\n    if errors.is_empty() {{\n        return;\n    }}\n}}\n\nfn helper() {{\n    return;\n}}\n\nmod inner {{\n    fn nested(errors: u8) {{}}\n}}\n",
                role("term", "terms.value")
            ),
        )
        .unwrap();
        fs::write(
            root.path().join("src/skip.rs"),
            "fn skipped(errors: u8) {}\n",
        )
        .unwrap();
        fs::create_dir_all(root.path().join("tests")).unwrap();
        fs::write(
            root.path().join("tests/outside.rs"),
            "fn outside(errors: u8) {}\n",
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let query = |name: &str| dispatch(&sources, "ast_grep", &json!({"query": name}));
        // Site queries honour paths and exclude.
        let sites = query("requirements.errors/accumulators").unwrap();
        assert_eq!(
            found(&sites),
            ["src/lib.rs#fn[check]", "src/lib.rs#mod[inner]/fn[nested]"]
        );
        // Each site shows its matched lines, without terminology.
        let lines = |delivery: &ToolDelivery, entry: usize| {
            matched(delivery)[entry]["lines"]
                .as_array()
                .unwrap()
                .iter()
                .map(|line| line["matched"].as_bool().unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(lines(&sites, 0), [true; 6]);
        assert!(sites.result.get("definitions").is_none());
        // Violation matches belong to the innermost containing site; matches
        // inside no site are discarded.
        let violations = query("requirements.errors/exits").unwrap();
        assert_eq!(found(&violations), ["src/lib.rs#fn[check]"]);
        assert_eq!(
            matched(&violations)[0]["lines"],
            json!([{"text": "        return;", "matched": true}])
        );
        for (name, message) in [
            (
                "requirements.errors/missing",
                "declares no query named \"missing\"",
            ),
            (
                "requirements.broken/bad",
                "exactly one fenced yaml code block",
            ),
        ] {
            let error = query(name).err().unwrap();
            assert!(error.to_string().contains(message), "{name}: {error}");
        }
    }
}

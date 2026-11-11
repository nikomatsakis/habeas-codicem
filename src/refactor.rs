//! `habeas refactor`: rewriting evidence addresses after a move, where the
//! recorded content is unchanged.
//!
//! The command runs after the user has moved files, outline nodes, or
//! definitions. It computes the repository as it will be once references to
//! renamed definitions and anchors to moved files are rewritten (the *post*
//! state), and the *mapped-back* form of that state, with every new side of
//! a mapping replaced by its old side. A record is rewritten only when its
//! mapped-back content is exactly what it recorded; it then records the post
//! state's addresses, text, hashes, and digests.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Value, json};

use crate::ReferenceKind;
use crate::evidence::{Anchor, Declaration, SourceReference, display_path};
use crate::inputs::{InputSelector, OutlineCache};

/// A failure of `habeas refactor`: an invalid mapping, reported as a usage
/// error before anything changes, or an I/O failure.
#[derive(Debug)]
pub enum RefactorError {
    Usage(String),
    Io(io::Error),
}

impl From<io::Error> for RefactorError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl std::fmt::Display for RefactorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Usage(message) => f.write_str(message),
            Self::Io(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for RefactorError {}

/// The suffix of `value` beneath `prefix`: empty when they are equal, and
/// starting with `separator` when `value` lies beneath it.
fn beneath<'a>(value: &'a str, prefix: &str, separator: char) -> Option<&'a str> {
    let rest = value.strip_prefix(prefix)?;
    (rest.is_empty() || rest.starts_with(separator)).then_some(rest)
}

/// A node selector beneath `prefix`, relative to it: `None` unless `item` is
/// `prefix` or one of its descendants. Every selector lies beneath the root's
/// empty selector.
fn beneath_node<'a>(item: &'a str, prefix: &str) -> Option<&'a str> {
    if prefix.is_empty() {
        return Some(item);
    }
    beneath(item, prefix, '/').map(|rest| rest.trim_start_matches('/'))
}

/// `relative` beneath the selector `prefix`.
fn join_node(prefix: &str, relative: &str) -> String {
    match (prefix.is_empty(), relative.is_empty()) {
        (true, _) => relative.to_owned(),
        (false, true) => prefix.to_owned(),
        (false, false) => format!("{prefix}/{relative}"),
    }
}

/// A node address, as written in an anchor or rationale: `file#selector`, or
/// `file` for the root.
fn address(file: &str, item: &str) -> String {
    if item.is_empty() {
        file.to_owned()
    } else {
        format!("{file}#{item}")
    }
}

/// The mappings of one run, by kind.
// {impl}`requirements.refactor.mappings`
#[derive(Debug, Default)]
pub(crate) struct Mappings {
    names: Vec<(String, String)>,
    paths: Vec<(String, String)>,
    nodes: Vec<((String, String), (String, String))>,
}

impl Mappings {
    /// A definition name's new name, if a mapping covers it.
    fn name(&self, name: &str) -> Option<String> {
        self.names
            .iter()
            .find_map(|(old, new)| beneath(name, old, '.').map(|rest| format!("{new}{rest}")))
    }

    /// A current definition name's old name, if a mapping covers it.
    fn name_back(&self, name: &str) -> Option<String> {
        self.names
            .iter()
            .find_map(|(old, new)| beneath(name, new, '.').map(|rest| format!("{old}{rest}")))
    }

    /// A definition selection with a mapped name prefix replaced.
    fn selection(&self, selection: &str) -> Option<String> {
        self.name(selection)
    }

    /// A file's new path under a path mapping.
    fn path(&self, file: &str) -> Option<String> {
        self.paths
            .iter()
            .find_map(|(old, new)| beneath(file, old, '/').map(|rest| format!("{new}{rest}")))
    }

    /// A current file's old path under a path mapping.
    fn path_back(&self, file: &str) -> Option<String> {
        self.paths
            .iter()
            .find_map(|(old, new)| beneath(file, new, '/').map(|rest| format!("{old}{rest}")))
    }

    /// A node's new address: under a node mapping covering it, or else its
    /// file's path mapping.
    fn node(&self, file: &str, item: &str) -> Option<(String, String)> {
        self.nodes
            .iter()
            .find_map(|((old_file, old_item), (new_file, new_item))| {
                (file == old_file)
                    .then(|| beneath_node(item, old_item))
                    .flatten()
                    .map(|rest| (new_file.clone(), join_node(new_item, rest)))
            })
            .or_else(|| self.path(file).map(|file| (file, item.to_owned())))
    }

    /// A current node's old address.
    fn node_back(&self, file: &str, item: &str) -> Option<(String, String)> {
        self.nodes
            .iter()
            .find_map(|((old_file, old_item), (new_file, new_item))| {
                (file == new_file)
                    .then(|| beneath_node(item, new_item))
                    .flatten()
                    .map(|rest| (old_file.clone(), join_node(old_item, rest)))
            })
            .or_else(|| self.path_back(file).map(|file| (file, item.to_owned())))
    }

    /// An anchor value or rationale address, `file` or `file#selector`,
    /// under the mappings.
    fn address(&self, value: &str) -> Option<String> {
        let (file, item) = value.split_once('#').unwrap_or((value, ""));
        self.node(file, item)
            .map(|(file, item)| address(&file, &item))
    }

    /// An address mapped back.
    fn address_back(&self, value: &str) -> Option<String> {
        let (file, item) = value.split_once('#').unwrap_or((value, ""));
        self.node_back(file, item)
            .map(|(file, item)| address(&file, &item))
    }

    /// The old sides of path and node mappings, as paths.
    fn old_paths(&self) -> Vec<&str> {
        self.paths
            .iter()
            .map(|(old, _)| old.as_str())
            .chain(self.nodes.iter().map(|((file, _), _)| file.as_str()))
            .collect()
    }

    /// The {term}`requirements.refactor.mapped_back` form of text: each new
    /// node address, definition name, and path replaced by its old side.
    // {impl}`requirements.refactor.mapped_back`
    fn text_back(&self, text: &str) -> String {
        let mut text = text.to_owned();
        for ((old_file, old_item), (new_file, new_item)) in &self.nodes {
            if !new_item.is_empty() {
                text = text.replace(&address(new_file, new_item), &address(old_file, old_item));
            }
        }
        for (old, new) in &self.names {
            text = replace_tokens(&text, new, old, Token::Name);
        }
        for (old, new) in &self.paths {
            text = replace_tokens(&text, new, old, Token::Path);
        }
        text
    }
}

/// The kind of token [`replace_tokens`] replaces, which decides where a
/// token may continue.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Token {
    /// A definition name, which continues past a `.` into names beneath it.
    Name,
    /// A path, which continues past a `/` into paths beneath it.
    Path,
}

/// Replaces each occurrence of `from` in `text` that is a whole token, or
/// the prefix of a token beneath it, with `to`.
fn replace_tokens(text: &str, from: &str, to: &str, token: Token) -> String {
    if from.is_empty() || from == to {
        return text.to_owned();
    }
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut previous: Option<char> = None;
    while let Some(found) = rest.find(from) {
        let before = rest[..found].chars().next_back().or(previous);
        let after = &rest[found + from.len()..];
        let mut next = after.chars();
        let continues = match (next.next(), next.next()) {
            (None, _) => false,
            (Some(c), _) if word(c) => true,
            // A name continues into a child name; a path does not continue
            // into an extension.
            (Some('.'), Some(c)) if word(c) => token == Token::Path,
            (Some('/'), Some(c)) if word(c) => token == Token::Name,
            _ => false,
        };
        let preceded = before.is_some_and(|c| word(c) || c == '.' || c == '/');
        out.push_str(&rest[..found]);
        if preceded || continues {
            out.push_str(from);
        } else {
            out.push_str(to);
        }
        previous = from.chars().next_back();
        rest = after;
    }
    out.push_str(rest);
    out
}

/// The opening fence of a fenced reference: its role and the whitespace
/// before the name.
static FENCE_ROLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{(?:impl|test|cite|fixme)\}[ \t]+").expect("fence pattern"));

/// An `:impl:` or `:test:` option line: the text before the value.
static ANCHOR_OPTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^:(?:impl|test):[ \t]+").expect("anchor pattern"));

/// A backtick-delimited code span.
static CODE_SPAN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"`([^`\n]+)`").expect("code span pattern"));

/// Rewrites references and anchors in a source's content: each reference
/// naming a mapped definition, and in Markdown each anchor option naming a
/// mapped file or node. Returns the new content and the number of edits.
// {impl}`requirements.refactor.reference_rewrite`
fn rewrite_source(mappings: &Mappings, path: &Path, content: &str) -> (String, usize) {
    let markdown = crate::outline::is_markdown_path(path);
    let mut edits = Vec::new();
    for role in crate::reference_roles(content, markdown) {
        if !matches!(
            role.kind,
            ReferenceKind::Impl
                | ReferenceKind::Test
                | ReferenceKind::Cite
                | ReferenceKind::Term
                | ReferenceKind::Fixme
        ) {
            continue;
        }
        if let Some(new) = mappings.name(role.name) {
            let start = role.end - 1 - role.name.len();
            edits.push((start..start + role.name.len(), new));
        }
    }
    let line_starts = std::iter::once(0)
        .chain(content.match_indices('\n').map(|(index, _)| index + 1))
        .collect::<Vec<_>>();
    for fence in crate::fenced::source_fences(path, content) {
        let Some(new) = mappings.name(&fence.name) else {
            continue;
        };
        let Some(&start) = line_starts.get(fence.line - 1) else {
            continue;
        };
        let end = content[start..]
            .find('\n')
            .map_or(content.len(), |end| start + end);
        let line = &content[start..end];
        if let Some(role) = FENCE_ROLE.find(line)
            && line[role.end()..].starts_with(&fence.name)
        {
            let name = start + role.end();
            edits.push((name..name + fence.name.len(), new));
        }
    }
    if markdown {
        for option in ANCHOR_OPTION.find_iter(content) {
            let end = content[option.end()..]
                .find('\n')
                .map_or(content.len(), |end| option.end() + end);
            let value = content[option.end()..end].trim_end();
            if let Some(new) = mappings.address(value) {
                edits.push((option.end()..option.end() + value.len(), new));
            }
        }
    }
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let mut rewritten = content.to_owned();
    let mut last = usize::MAX;
    let mut count = 0;
    for (range, new) in edits {
        if range.end > last {
            continue;
        }
        last = range.start;
        rewritten.replace_range(range, &new);
        count += 1;
    }
    (rewritten, count)
}

/// Rewrites a rationale's code spans that are exactly an old definition
/// name, path, or node address, and lists the other spans that mention an
/// old path.
// {impl}`requirements.refactor.rationale.names`
// {impl}`requirements.refactor.rationale.paths`
// {impl}`requirements.refactor.rationale.addresses`
// {impl}`requirements.refactor.rationale.untouched`
fn rewrite_rationale(mappings: &Mappings, rationale: &str) -> (String, Vec<String>) {
    let old_paths = mappings.old_paths();
    let mut mentions = Vec::new();
    let rewritten = CODE_SPAN.replace_all(rationale, |captures: &regex::Captures<'_>| {
        let span = &captures[1];
        let mapped = mappings.name(span).or_else(|| mappings.address(span));
        match mapped {
            Some(new) => format!("`{new}`"),
            None => {
                if old_paths.iter().any(|old| span.contains(old)) {
                    mentions.push(format!("`{span}`"));
                }
                captures[0].to_owned()
            }
        }
    });
    (rewritten.into_owned(), mentions)
}

/// The repository before and after the rewrite, and mapped back.
struct States {
    root: PathBuf,
    mappings: Mappings,
    files: Vec<PathBuf>,
    /// Rewritten source contents, by project-relative path.
    rewritten: BTreeMap<String, (String, usize)>,
    definitions: BTreeMap<String, Declaration>,
    definitions_post: BTreeMap<String, Declaration>,
    definitions_back: BTreeMap<String, Declaration>,
    references_post: Vec<SourceReference>,
    references_back: Vec<SourceReference>,
    outlines_post: OutlineCache,
    outlines_back: OutlineCache,
}

impl States {
    /// Computes the post and mapped-back states from the current repository.
    fn compute(root: &Path, mappings: Mappings) -> io::Result<Self> {
        let definitions = crate::evidence::load_definitions(root)?;
        let mut failures = Vec::new();
        let files = crate::evidence::discovered_files(root, &mut failures);
        let references = crate::evidence::collect_references_with_progress(
            root,
            &files,
            &mut failures,
            &mut |_| {},
        );
        if !failures.is_empty() {
            return Err(io::Error::other(failures.join("\n")));
        }
        let mut sources = files
            .iter()
            .map(|file| display_path(root, file))
            .collect::<BTreeSet<_>>();
        sources.extend(
            definitions
                .values()
                .map(|definition| definition.file.clone()),
        );
        let mut rewritten = BTreeMap::new();
        let mut post = HashMap::new();
        let mut back = HashMap::new();
        for file in &sources {
            let Ok(content) = fs::read_to_string(root.join(file)) else {
                continue;
            };
            let (new, edits) = rewrite_source(&mappings, Path::new(file), &content);
            let reverted = mappings.text_back(&new);
            if reverted != content {
                back.insert(file.clone(), reverted);
            }
            if edits > 0 {
                post.insert(file.clone(), new.clone());
                rewritten.insert(file.clone(), (new, edits));
            }
        }
        let rewrite = |text: &str, file: &str| rewrite_source(&mappings, Path::new(file), text).0;
        let definitions_post = definitions
            .iter()
            .map(|(name, definition)| {
                let mut definition = definition.clone();
                definition.text = rewrite(&definition.text, &definition.file);
                definition.raw_lines = rewrite(&definition.raw_lines.join("\n"), &definition.file)
                    .split('\n')
                    .map(str::to_owned)
                    .collect();
                for anchor in &mut definition.anchors {
                    if let Some(new) = mappings.address(&anchor.label()) {
                        *anchor = Anchor::parse(anchor.kind, &new);
                    }
                }
                (name.clone(), definition)
            })
            .collect::<BTreeMap<_, _>>();
        let definitions_back = definitions_post
            .iter()
            .map(|(name, definition)| {
                let old = mappings.name_back(name).unwrap_or_else(|| name.clone());
                let mut definition = definition.clone();
                definition.text = mappings.text_back(&definition.text);
                definition.raw_lines = definition
                    .raw_lines
                    .iter()
                    .map(|line| mappings.text_back(line))
                    .collect();
                if let Some(opening) = definition.raw_lines.first_mut() {
                    *opening = opening_back(opening, name, &old);
                }
                for anchor in &mut definition.anchors {
                    if let Some(old) = mappings.address_back(&anchor.label()) {
                        *anchor = Anchor::parse(anchor.kind, &old);
                    }
                }
                (old, definition)
            })
            .collect();
        let references_post = references
            .iter()
            .map(|reference| {
                let mut reference = reference.clone();
                if let Some(new) = mappings.name(&reference.name) {
                    reference.name = new;
                }
                if let Some(body) = &mut reference.commentary {
                    *body = rewrite_source(&mappings, Path::new("commentary"), body).0;
                }
                reference
            })
            .collect::<Vec<_>>();
        let references_back = references_post
            .iter()
            .map(|reference| {
                let mut reference = reference.clone();
                if let Some(old) = mappings.name_back(&reference.name) {
                    reference.name = old;
                }
                if let Some(body) = &mut reference.commentary {
                    *body = mappings.text_back(body);
                }
                reference
            })
            .collect();
        Ok(Self {
            root: root.to_owned(),
            mappings,
            files,
            rewritten,
            definitions,
            definitions_post,
            definitions_back,
            references_post,
            references_back,
            outlines_post: OutlineCache::with_overrides(post),
            outlines_back: OutlineCache::with_overrides(back),
        })
    }

    /// A node's structural hash in a state.
    fn hash(&self, outlines: &OutlineCache, file: &str, item: &str) -> Result<String, String> {
        let parsed = outlines.parse(&self.root, file);
        let parsed = parsed.as_ref().as_ref().map_err(Clone::clone)?;
        let index = parsed.resolve(file, item)?;
        Ok(parsed.outline.nodes[index].structural_hash.clone())
    }

    /// The post state's reference-set digest of a current definition, and
    /// the mapped-back digest of its old name.
    // {impl}`requirements.refactor.reference_digest`
    fn digests(&self, new: &str, old: &str) -> io::Result<Option<(String, String)>> {
        let (Some(post), Some(back)) = (
            self.definitions_post.get(new),
            self.definitions_back.get(old),
        ) else {
            return Ok(None);
        };
        let sites = crate::evidence::definition_sites(
            &self.root,
            new,
            post,
            &self.files,
            &self.outlines_post,
        )?;
        let current = crate::evidence::reference_set_digest(
            &self.root,
            &self.outlines_post,
            new,
            post,
            &self.references_post,
            &sites,
        );
        let mut entries = crate::evidence::reference_set_entries(
            &self.root,
            &self.outlines_back,
            old,
            back,
            &self.references_back,
            &sites,
        );
        for entry in &mut entries {
            if entry.origin != "anchor"
                && let Some((file, item)) = self.mappings.node_back(&entry.file, &entry.item)
            {
                entry.file = file;
                entry.item = item;
            }
        }
        Ok(Some((current, crate::evidence::digest_entries(entries))))
    }
}

/// A declaration's opening fence with its short name mapped back from the
/// current full name `new` to `old`.
fn opening_back(opening: &str, new: &str, old: &str) -> String {
    let Some((fence, short)) = opening.rsplit_once(' ') else {
        return opening.to_owned();
    };
    if new == old || !new.ends_with(short) {
        return opening.to_owned();
    }
    let segments = short.split('.').count();
    let parts = old.split('.').collect::<Vec<_>>();
    let old_short = parts[parts.len().saturating_sub(segments)..].join(".");
    format!("{fence} {old_short}")
}

/// What `habeas refactor` does with one record.
enum Outcome {
    /// No mapping affects it.
    Unaffected,
    /// It is rewritten, and possibly moved.
    Changed {
        path: String,
        value: Value,
        mentions: Vec<String>,
    },
    /// A mapping affects it but a check failed.
    Unchanged(String),
}

/// The kind of an evidence record, from its path.
enum RecordKind {
    Definition(String),
    Citation,
    Site,
}

fn record_kind(path: &str) -> Option<RecordKind> {
    let rest = path.strip_prefix("evidence/")?;
    if rest.starts_with(&format!("{}/", crate::citations::CITATIONS_DIR)) {
        Some(RecordKind::Citation)
    } else if rest.starts_with(&format!("{}/", crate::sites::SITES_DIR)) {
        Some(RecordKind::Site)
    } else if !rest.contains('/') {
        rest.strip_suffix(".json")
            .map(|name| RecordKind::Definition(name.to_owned()))
    } else {
        None
    }
}

/// Rewrites one input, or returns `None` when no mapping affects it.
// {impl}`requirements.refactor.outline_inputs`
// {impl}`requirements.refactor.definition_inputs`
fn rewrite_input(states: &States, input: &Value) -> Option<Result<Value, String>> {
    let mappings = &states.mappings;
    let root = &states.root;
    match InputSelector::from_input(input)? {
        InputSelector::Outline { file, item } => {
            let recorded = input.get("hash").and_then(Value::as_str)?;
            let mapped = mappings.node(&file, &item);
            let (new_file, new_item) = mapped.clone().unwrap_or((file.clone(), item.clone()));
            if mapped.is_none() && !states.rewritten.contains_key(&new_file) {
                return None;
            }
            let back = states.hash(&states.outlines_back, &new_file, &new_item);
            let current = states.hash(&states.outlines_post, &new_file, &new_item);
            Some(match (back, current) {
                (Ok(back), Ok(current)) if back == recorded => {
                    Ok(json!({"file": new_file, "item": new_item, "hash": current}))
                }
                (Err(error), _) | (_, Err(error)) => Err(error),
                _ => Err(format!(
                    "{} no longer has the recorded hash",
                    address(&new_file, &new_item)
                )),
            })
        }
        selector @ (InputSelector::Requirements { .. } | InputSelector::Definition { .. }) => {
            let (new_selector, names) = match &selector {
                InputSelector::Requirements { requirements } => (
                    InputSelector::Requirements {
                        requirements: mappings
                            .selection(requirements)
                            .unwrap_or_else(|| requirements.clone()),
                    },
                    input
                        .get("matches")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect::<Vec<_>>(),
                ),
                InputSelector::Definition { definition } => (
                    InputSelector::Definition {
                        definition: mappings
                            .name(definition)
                            .unwrap_or_else(|| definition.clone()),
                    },
                    vec![definition.clone()],
                ),
                InputSelector::Outline { .. } => unreachable!(),
            };
            let affected = new_selector != selector
                || names.iter().any(|name| {
                    let new = mappings.name(name).unwrap_or_else(|| name.clone());
                    mappings.name(name).is_some()
                        || states.definitions.get(name).map(|d| &d.raw_lines)
                            != states.definitions_post.get(&new).map(|d| &d.raw_lines)
                });
            if !affected {
                return None;
            }
            let back = selector
                .resolve(root, &states.definitions_back, &states.outlines_back)
                .map(|resolved| selector.to_input(&resolved));
            let current = new_selector
                .resolve(root, &states.definitions_post, &states.outlines_post)
                .map(|resolved| new_selector.to_input(&resolved));
            Some(match (back, current) {
                (Ok(back), Ok(current))
                    if &back == input && {
                        let mut mapped = names
                            .iter()
                            .map(|name| mappings.name(name).unwrap_or_else(|| name.clone()))
                            .collect::<Vec<_>>();
                        mapped.sort();
                        current
                            .get("matches")
                            .is_none_or(|matches| *matches == json!(mapped))
                    } =>
                {
                    Ok(current)
                }
                (_, Err(error)) => Err(error),
                _ => Err(format!(
                    "{} no longer selects what it recorded",
                    match &new_selector {
                        InputSelector::Requirements { requirements } => requirements,
                        InputSelector::Definition { definition } => definition,
                        InputSelector::Outline { .. } => unreachable!(),
                    }
                )),
            })
        }
    }
}

/// Decides one record under the mappings.
// {impl}`requirements.refactor.whole_record`
// {impl}`requirements.refactor.subject_text`
// {impl}`requirements.refactor.record_fields`
fn rewrite_record(states: &States, path: &str, record: &Value) -> io::Result<Outcome> {
    let mappings = &states.mappings;
    let Some(kind) = record_kind(path) else {
        return Ok(Outcome::Unaffected);
    };
    let mut value = record.clone();
    let mut changed = false;
    let mut failures = Vec::new();
    let new_path = match &kind {
        RecordKind::Definition(name) => {
            let new = mappings.name(name).unwrap_or_else(|| name.clone());
            let field = if record.get("example").is_some() {
                "example"
            } else {
                "requirement"
            };
            let body = |definitions: &BTreeMap<String, Declaration>, name: &str| {
                definitions
                    .get(name)
                    .map(|definition| definition.text.clone())
            };
            if new != *name
                || body(&states.definitions, name) != body(&states.definitions_post, &new)
            {
                match (
                    body(&states.definitions_back, name),
                    body(&states.definitions_post, &new),
                ) {
                    (Some(back), Some(current))
                        if Some(back.as_str()) == record[field].as_str() =>
                    {
                        value[field] = Value::String(current);
                        changed = true;
                    }
                    (_, None) => failures.push(format!("{new}: no current definition")),
                    _ => failures.push(format!("{field} text of {new} changed")),
                }
            }
            if let Some(recorded) = record.get("references").and_then(Value::as_str)
                && let Some((current, back)) = states.digests(&new, name)?
                && current != recorded
                && (new != *name || current != back)
            {
                if back == recorded {
                    value["references"] = Value::String(current);
                    changed = true;
                } else {
                    failures.push(format!("the reference set of {new} changed"));
                }
            }
            if new != *name {
                changed = true;
            }
            format!("evidence/{new}.json")
        }
        RecordKind::Citation | RecordKind::Site => {
            let file = record
                .get("file")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let item = record
                .get("item")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let (new_file, new_item) = mappings
                .node(file, item)
                .unwrap_or((file.to_owned(), item.to_owned()));
            if (new_file.as_str(), new_item.as_str()) != (file, item) {
                value["file"] = json!(new_file);
                value["item"] = json!(new_item);
                changed = true;
            }
            match &kind {
                RecordKind::Citation => {
                    if let Some(citations) =
                        value.get_mut("citations").and_then(Value::as_array_mut)
                    {
                        for citation in citations {
                            let name = citation["name"].as_str().unwrap_or_default();
                            if let Some(new) = mappings.name(name) {
                                citation["name"] = json!(new);
                                changed = true;
                            }
                            let commentary = citation["commentary"]
                                .as_str()
                                .unwrap_or_default()
                                .to_owned();
                            let (new, edits) =
                                rewrite_source(mappings, Path::new("commentary"), &commentary);
                            if edits > 0 {
                                citation["commentary"] = json!(new);
                                changed = true;
                            }
                        }
                    }
                    if changed && !same_citations(states, &new_file, &new_item, &value) {
                        failures.push(format!(
                            "the citations of {} differ from the recorded ones",
                            address(&new_file, &new_item)
                        ));
                    }
                    crate::citations::review_path(&new_file, &new_item)
                }
                _ => {
                    let requirement = record
                        .get("requirement")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let new = mappings
                        .name(requirement)
                        .unwrap_or_else(|| requirement.to_owned());
                    if new != requirement {
                        value["requirement"] = json!(new);
                        changed = true;
                    }
                    crate::sites::review_path(&new, &new_file, &new_item)
                }
            }
        }
    };
    if let Some(inputs) = value.get_mut("inputs").and_then(Value::as_array_mut) {
        for (index, input) in inputs.iter_mut().enumerate() {
            match rewrite_input(states, input) {
                None => {}
                Some(Ok(new)) => {
                    if new != *input {
                        *input = new;
                        changed = true;
                    }
                }
                Some(Err(error)) => {
                    failures.push(format!("inputs[{index}]: {error}"));
                    changed = true;
                }
            }
        }
    }
    if !changed {
        return Ok(Outcome::Unaffected);
    }
    if !failures.is_empty() {
        return Ok(Outcome::Unchanged(failures.join("; ")));
    }
    let mut mentions = Vec::new();
    if let Some(rationale) = value.get("rationale").and_then(Value::as_str) {
        let (rewritten, found) = rewrite_rationale(mappings, rationale);
        value["rationale"] = Value::String(rewritten);
        mentions = found;
    }
    Ok(Outcome::Changed {
        path: new_path,
        value,
        mentions,
    })
}

/// Whether a citation review's citations, as rewritten, are the post
/// state's citations of its item.
fn same_citations(states: &States, file: &str, item: &str, value: &Value) -> bool {
    let items = crate::citations::citation_items(
        &states.root,
        &states.definitions_post,
        &states.references_post,
        &states.outlines_post,
    );
    let Some(current) = items.get(&(file.to_owned(), item.to_owned())) else {
        return false;
    };
    let recorded = value["citations"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|citation| crate::citations::Citation {
            role: match citation["role"].as_str() {
                Some("cite") => "cite",
                Some("impl") => "impl",
                Some("test") => "test",
                Some("fixme") => "fixme",
                _ => "",
            },
            name: citation["name"].as_str().unwrap_or_default().to_owned(),
            commentary: citation["commentary"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        })
        .collect::<Vec<_>>();
    crate::citations::same_citations(&recorded, &current.citations)
}

/// Parses and checks the mappings against the repository.
// {impl}`requirements.refactor.mappings`
// {impl}`requirements.refactor.invalid_mappings`
fn parse_mappings(
    root: &Path,
    values: &[String],
    records: &[(String, Value)],
) -> Result<Mappings, RefactorError> {
    let usage = |message: String| Err(RefactorError::Usage(message));
    let definitions = crate::evidence::load_definitions(root)?;
    let mut names = definitions.keys().cloned().collect::<BTreeSet<_>>();
    let mut files = BTreeSet::new();
    for (path, record) in records {
        if let Some(RecordKind::Definition(name)) = record_kind(path) {
            names.insert(name);
        }
        for key in ["requirement"] {
            if let Some(name) = record.get(key).and_then(Value::as_str)
                && matches!(record_kind(path), Some(RecordKind::Site))
            {
                names.insert(name.to_owned());
            }
        }
        if let Some(file) = record.get("file").and_then(Value::as_str) {
            files.insert(file.to_owned());
        }
        for input in record
            .get("inputs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(file) = input.get("file").and_then(Value::as_str) {
                files.insert(file.to_owned());
            }
            for name in input
                .get("matches")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .chain(input.get("definition").and_then(Value::as_str))
            {
                names.insert(name.to_owned());
            }
        }
    }
    let mut mappings = Mappings::default();
    let outlines = OutlineCache::default();
    for value in values {
        let Some((old, new)) = value
            .split_once('=')
            .filter(|(old, new)| !old.is_empty() && !new.is_empty() && old != new)
        else {
            return usage(format!(
                "invalid mapping {value:?}: expected <old>=<new> with different, nonempty sides"
            ));
        };
        if let Some((old_file, old_item)) = old.split_once('#') {
            let (new_file, new_item) = new.split_once('#').unwrap_or((new, ""));
            if old_file.is_empty() || new_file.is_empty() {
                return usage(format!("invalid mapping {value:?}: a node needs a file"));
            }
            let parsed = outlines.parse(root, new_file);
            if parsed
                .as_ref()
                .as_ref()
                .map_err(Clone::clone)
                .and_then(|parsed| parsed.resolve(new_file, new_item))
                .is_err()
            {
                return usage(format!("mapping {value:?}: {new} does not exist"));
            }
            mappings.nodes.push((
                (old_file.to_owned(), old_item.to_owned()),
                (new_file.to_owned(), new_item.to_owned()),
            ));
            continue;
        }
        let name_like = names.iter().any(|name| beneath(name, old, '.').is_some());
        let path_like = old.contains('/')
            || files.iter().any(|file| beneath(file, old, '/').is_some())
            || root.join(old).exists();
        match (name_like, path_like) {
            (true, true) => {
                return usage(format!(
                    "mapping {value:?} is ambiguous: {old} names both a definition and a path"
                ));
            }
            (true, false) => {
                if !definitions
                    .keys()
                    .any(|name| beneath(name, new, '.').is_some())
                {
                    return usage(format!(
                        "mapping {value:?}: no definition named {new} exists"
                    ));
                }
                mappings.names.push((old.to_owned(), new.to_owned()));
            }
            (false, true) => {
                if !root.join(new).exists() {
                    return usage(format!("mapping {value:?}: {new} does not exist"));
                }
                mappings.paths.push((old.to_owned(), new.to_owned()));
            }
            (false, false) => {
                return usage(format!(
                    "mapping {value:?}: {old} names no recorded definition, file, or node"
                ));
            }
        }
    }
    let overlap = |a: &str, b: &str, separator| {
        beneath(a, b, separator).is_some() || beneath(b, a, separator).is_some()
    };
    let paths = mappings
        .paths
        .iter()
        .map(|(old, _)| (old.as_str(), None))
        .chain(
            mappings
                .nodes
                .iter()
                .map(|((file, item), _)| (file.as_str(), Some(item.as_str()))),
        )
        .collect::<Vec<_>>();
    for (index, (a, a_item)) in paths.iter().enumerate() {
        for (b, b_item) in &paths[index + 1..] {
            let clash = match (a_item, b_item) {
                (Some(a_item), Some(b_item)) => {
                    a == b
                        && (beneath_node(a_item, b_item).is_some()
                            || beneath_node(b_item, a_item).is_some())
                }
                _ => overlap(a, b, '/'),
            };
            if clash {
                return usage(format!("mappings of {a} and {b} overlap"));
            }
        }
    }
    for (index, (a, _)) in mappings.names.iter().enumerate() {
        for (b, _) in &mappings.names[index + 1..] {
            if overlap(a, b, '.') {
                return usage(format!("mappings of {a} and {b} overlap"));
            }
        }
    }
    Ok(mappings)
}

/// Every evidence record, by project-relative path.
fn load_records(root: &Path) -> io::Result<Vec<(String, Value)>> {
    let mut records = Vec::new();
    let mut pending = vec![root.join("evidence")];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                let content = fs::read_to_string(&path)?;
                // A malformed record is a validation error; it is left alone.
                if let Ok(value) = serde_json::from_str(&content) {
                    records.push((display_path(root, &path), value));
                }
            }
        }
    }
    records.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(records)
}

/// Removes the empty directories between `path` and `evidence/`.
fn prune(root: &Path, path: &Path) {
    let evidence = root.join("evidence");
    let mut directory = path.parent();
    while let Some(current) = directory {
        if current == evidence || fs::remove_dir(current).is_err() {
            break;
        }
        directory = current.parent();
    }
}

/// Runs `habeas refactor` with `--map` values, reporting what it changed,
/// or with `dry_run`, what it would change.
// {impl}`requirements.refactor.command`
// {impl}`requirements.refactor.dry_run`
// {impl}`requirements.refactor.record_files`
// {impl}`requirements.refactor.equivalence`
pub fn refactor(root: &Path, values: &[String], dry_run: bool) -> Result<String, RefactorError> {
    let root = fs::canonicalize(root)?;
    let records = load_records(&root)?;
    let mappings = parse_mappings(&root, values, &records)?;
    let states = States::compute(&root, mappings)?;
    let mut changed = Vec::new();
    let mut unchanged = Vec::new();
    let mut targets = BTreeMap::<String, usize>::new();
    for (path, record) in &records {
        match rewrite_record(&states, path, record)? {
            Outcome::Unaffected => {}
            Outcome::Unchanged(reason) => unchanged.push((path.clone(), reason)),
            Outcome::Changed {
                path: target,
                value,
                mentions,
            } => {
                *targets.entry(target.clone()).or_default() += 1;
                changed.push((path.clone(), target, value, mentions));
            }
        }
    }
    let moving = changed
        .iter()
        .filter(|(path, target, ..)| path != target)
        .map(|(path, ..)| path.clone())
        .collect::<BTreeSet<_>>();
    changed.retain(|(path, target, ..)| {
        let conflict = targets[target] > 1
            || (path != target && root.join(target).exists() && !moving.contains(target));
        if conflict {
            unchanged.push((path.clone(), format!("{target} would be overwritten")));
        }
        !conflict
    });
    unchanged.sort();

    if !dry_run {
        for (file, (content, _)) in &states.rewritten {
            fs::write(root.join(file), content)?;
        }
        for (path, ..) in &changed {
            if !changed.iter().any(|(_, target, ..)| target == path) {
                let old = root.join(path);
                if fs::remove_file(&old).is_ok() {
                    prune(&root, &old);
                }
            }
        }
        for (_, target, value, _) in &changed {
            let target = root.join(target);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(
                &target,
                format!(
                    "{}\n",
                    serde_json::to_string_pretty(value).map_err(io::Error::from)?
                ),
            )?;
        }
    }

    let (rewrote, change, leave) = if dry_run {
        ("Would rewrite", "Would change", "Would leave")
    } else {
        ("Rewrote", "Changed", "Left")
    };
    let mut report = String::new();
    if !states.rewritten.is_empty() {
        let _ = writeln!(
            report,
            "{rewrote} references or anchors in {} files:",
            states.rewritten.len()
        );
        for (file, (_, edits)) in &states.rewritten {
            let _ = writeln!(report, "- {file} ({edits})");
        }
    }
    let _ = writeln!(report, "{change} {} records:", changed.len());
    for (path, target, _, mentions) in &changed {
        if path == target {
            let _ = writeln!(report, "- {path}");
        } else {
            let _ = writeln!(report, "- {path} -> {target}");
        }
        for mention in mentions {
            let _ = writeln!(report, "  - review the rationale's {mention}");
        }
    }
    if !unchanged.is_empty() {
        let _ = writeln!(
            report,
            "{leave} {} records unchanged; they stay stale until reassessed:",
            unchanged.len()
        );
        for (path, reason) in &unchanged {
            let _ = writeln!(report, "- {path}: {reason}");
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests;

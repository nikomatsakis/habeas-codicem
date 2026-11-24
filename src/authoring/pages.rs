//! Paging of large source-tool results.

use std::collections::HashMap;
use std::io;

use serde_json::{Map, Value, json};

use super::inputs::AssessmentSources;
use super::searches::Searches;
use super::tools::ToolDelivery;

/// The largest serialized result returned whole, and the largest page.
// {impl}`requirements.authoring.result_pages`
pub(crate) const PAGE_LIMIT: usize = 32 * 1024;

/// The length of every page token, so a page's size is known before its
/// token is chosen.
// {impl}`requirements.authoring.result_pages`
const TOKEN_LENGTH: usize = 16;

/// A source-tool result made of whole entries, which is paged when its
/// serialized JSON exceeds [`PAGE_LIMIT`].
pub(crate) struct Listing {
    /// The keys of the result's entry arrays, in order; none for a result
    /// that is a bare array, whose pages hold it under `entries`.
    arrays: Vec<&'static str>,
    /// Each entry with the index of its array, in the result's order.
    entries: Vec<(usize, Value)>,
    /// Whether the entries are evidence inputs, accompanied under
    /// `definitions` by the additional definitions they need.
    evidence: bool,
}

impl Listing {
    /// A result that is a bare array of `entries`, such as a listing or an
    /// outline.
    pub(crate) fn array(entries: Vec<Value>) -> Self {
        Self {
            arrays: Vec::new(),
            entries: entries.into_iter().map(|entry| (0, entry)).collect(),
            evidence: false,
        }
    }

    /// A result of evidence `inputs` and their additional definitions.
    // {impl}`requirements.authoring.delivered_inputs`
    pub(crate) fn inputs(inputs: Vec<Value>) -> Self {
        Self {
            arrays: vec!["inputs"],
            entries: inputs.into_iter().map(|input| (0, input)).collect(),
            evidence: true,
        }
    }

    /// A result holding one array per key, in the order of `arrays`.
    pub(crate) fn object(arrays: &'static [&'static str], values: Vec<Vec<Value>>) -> Self {
        Self {
            arrays: arrays.to_vec(),
            entries: values
                .into_iter()
                .enumerate()
                .flat_map(|(index, values)| values.into_iter().map(move |value| (index, value)))
                .collect(),
            evidence: false,
        }
    }

    /// The listing's entries, in order.
    pub(crate) fn values(&self) -> impl Iterator<Item = &Value> {
        self.entries.iter().map(|(_, value)| value)
    }

    /// Untracked paths are entries too: place them first so they start on the
    /// first page and can continue without becoming unbounded page overhead.
    // {impl}`requirements.authoring.not_listed`
    pub(crate) fn with_not_listed(mut self, paths: Vec<Value>) -> Self {
        if paths.is_empty() {
            return self;
        }
        if self.arrays.is_empty() {
            self.arrays.push("entries");
        }
        let index = self.arrays.len();
        self.arrays.push("not_listed");
        self.entries
            .splice(0..0, paths.into_iter().map(|path| (index, path)));
        self
    }

    /// The result, or one page of it, holding `entries` with `definitions`.
    // {impl}`requirements.authoring.search_accounting.instructions`
    fn render(
        &self,
        entries: &[&Value],
        indexes: &[usize],
        definitions: &[Value],
        page: Option<Value>,
        search_instructions: bool,
    ) -> Value {
        if self.arrays.is_empty() {
            let entries = Value::Array(entries.iter().map(|entry| (*entry).clone()).collect());
            if page.is_none() {
                return entries;
            }
            let mut object = Map::new();
            object.insert("entries".into(), entries);
            if let Some(page) = page {
                object.insert("page".into(), page);
            }
            return Value::Object(object);
        }
        let mut object = Map::new();
        for (index, key) in self.arrays.iter().enumerate() {
            let values: Vec<Value> = entries
                .iter()
                .zip(indexes)
                .filter(|(_, array)| **array == index)
                .map(|(entry, _)| (*entry).clone())
                .collect();
            if *key != "not_listed" || !values.is_empty() {
                object.insert((*key).to_owned(), Value::Array(values));
            }
        }
        if self.evidence {
            object.insert("definitions".into(), Value::Array(definitions.to_vec()));
        }
        if self.arrays.contains(&"matches") {
            // The real UUID is assigned with the retained dependency snapshot.
            object.insert(
                "search_id".into(),
                json!("00000000-0000-0000-0000-000000000000"),
            );
            if search_instructions {
                object.insert(
                    "instructions".into(),
                    json!(super::approved_text("search_accounting.instructions", 0)),
                );
            }
        }
        if let Some(page) = page {
            object.insert("page".into(), page);
        }
        Value::Object(object)
    }
}

/// Room kept on every page for the redaction note added after paging.
// {impl}`requirements.authoring.result_pages`
const NOTE_RESERVE: usize = super::tools::REDACTION_NOTE.len() + 16;

/// The pages of this assessment's paged results not yet returned, by token.
/// Each assessment has its own, dropped when it finishes.
#[derive(Default)]
pub(crate) struct ResultPages {
    pages: HashMap<String, (Value, Vec<Value>)>,
    searches: Searches,
}

impl ResultPages {
    /// Returns `listing` whole when its JSON is within [`PAGE_LIMIT`], and
    /// otherwise its first page, keeping the rest for `next_page`. Only the
    /// returned page's inputs are delivered.
    // {impl}`requirements.authoring.delivered_inputs`
    // {impl}`requirements.authoring.tool_descriptions.special_cases`
    // {impl}`requirements.authoring.not_listed`
    pub(crate) fn deliver(
        &mut self,
        sources: &AssessmentSources,
        mut listing: Listing,
    ) -> io::Result<ToolDelivery> {
        // Tags are attached before sizing, and never assigned to not_listed.
        // {impl}`requirements.authoring.search_accounting.entry_tags`
        let search = listing.arrays.iter().position(|key| *key == "matches");
        let instructions = search.is_some() && self.searches.needs_instructions();
        if let Some(search) = search {
            let mut tag = 0;
            for (array, entry) in &mut listing.entries {
                if *array == search {
                    tag += 1;
                    entry["tag"] = json!(tag.to_string());
                }
            }
        }
        let values: Vec<&Value> = listing.entries.iter().map(|(_, value)| value).collect();
        let indexes: Vec<usize> = listing.entries.iter().map(|(index, _)| *index).collect();
        let owned: Vec<Value> = values.iter().map(|value| (*value).clone()).collect();
        let definitions = if listing.evidence {
            sources.additional_definitions(&owned)?
        } else {
            Vec::new()
        };
        let mut whole = listing.render(&values, &indexes, &definitions, None, instructions);
        // Room for the redaction note only when it will be added.
        let reserve = if super::tools::needs_redaction_note(sources, &whole) {
            NOTE_RESERVE
        } else {
            0
        };
        if whole.to_string().len() + reserve <= PAGE_LIMIT {
            self.searches.prepare(sources, &mut whole)?;
            self.searches.delivered(&whole);
            return Ok(ToolDelivery {
                result: whole,
                inputs: delivered(&listing, owned, definitions),
            });
        }
        let accompanying = if listing.evidence {
            sources.definitions_each(&owned)?
        } else {
            vec![Vec::new(); owned.len()]
        };
        let total = values.len();
        let pages = paginate(&listing, &values, &accompanying, reserve, instructions);
        let tokens: Vec<String> = (1..pages.len()).map(|_| self.token()).collect();
        let mut rendered = Vec::new();
        for (number, range) in pages.iter().enumerate() {
            let (entries, page_indexes) = (&values[range.clone()], &indexes[range.clone()]);
            let definitions = page_definitions(entries, &accompanying[range.clone()]);
            let mut page = json!({
                "total_entries": total,
                "returned_entries": entries.len(),
                "next": tokens.get(number),
            });
            if tokens.get(number).is_some() {
                page["note"] = json!(PAGE_NOTE);
            }
            // {impl}`requirements.authoring.search_accounting.pagination`
            let mut result = listing.render(
                entries,
                page_indexes,
                &definitions,
                Some(page),
                instructions && number == 0,
            );
            self.searches.prepare(sources, &mut result)?;
            let inputs = delivered(
                &listing,
                entries.iter().map(|entry| (*entry).clone()).collect(),
                definitions,
            );
            rendered.push((result, inputs));
        }
        let mut rendered = rendered.into_iter();
        let (result, inputs) = rendered.next().expect("a paged result has a first page");
        self.searches.delivered(&result);
        for (token, page) in tokens.into_iter().zip(rendered) {
            self.pages.insert(token, page);
        }
        Ok(ToolDelivery { result, inputs })
    }

    /// The page a `next_page` token names, in the shape of the first.
    // {impl}`requirements.authoring.search_accounting.page_replay`
    pub(crate) fn next_page(&mut self, token: &str) -> io::Result<ToolDelivery> {
        let (result, inputs) = self.pages.get(token).ok_or_else(|| {
            io::Error::other(format!(
                "unknown page token {token:?}: a token is valid only in the assessment whose paged result returned it; repeat that tool call to page through its result again"
            ))
        })?;
        self.searches.delivered(result);
        Ok(ToolDelivery {
            result: result.clone(),
            inputs: inputs.clone(),
        })
    }

    // {cite}`habeas:util`
    pub(crate) fn finish_search(&mut self, arguments: &Value) -> io::Result<ToolDelivery> {
        self.searches.finish(arguments)
    }

    // {cite}`habeas:util`
    pub(crate) fn outstanding_searches(&self) -> Vec<String> {
        self.searches.outstanding()
    }

    /// A fresh opaque token of [`TOKEN_LENGTH`] characters.
    fn token(&self) -> String {
        loop {
            let token = format!("{:016x}", fastrand::u64(..));
            debug_assert_eq!(token.len(), TOKEN_LENGTH);
            if !self.pages.contains_key(&token) {
                return token;
            }
        }
    }
}

/// The inputs a delivered result or page records: its entries, when they are
/// inputs, then their definitions.
// {impl}`requirements.authoring.delivered_inputs`
fn delivered(listing: &Listing, entries: Vec<Value>, definitions: Vec<Value>) -> Vec<Value> {
    if !listing.evidence {
        return Vec::new();
    }
    let mut inputs = entries;
    inputs.extend(definitions);
    inputs
}

/// The definitions accompanying a page's entries, each once, in order,
/// leaving out any the page holds as an entry.
fn page_definitions(entries: &[&Value], accompanying: &[Vec<Value>]) -> Vec<Value> {
    let mut definitions = Vec::new();
    for definition in accompanying.iter().flatten() {
        if !definitions.contains(definition) && !entries.contains(&definition) {
            definitions.push(definition.clone());
        }
    }
    definitions
}

/// Splits the entries into consecutive pages of whole entries whose JSON,
/// with their definitions and page fields, stays within [`PAGE_LIMIT`],
/// except that an entry exceeding it forms a page of its own. Sizes are
/// counted from above: every element's comma and every accompanying
/// What a page with more entries after it tells the model, since tool
/// descriptions do not explain paging.
// {impl}`requirements.authoring.result_pages`
// {impl}`requirements.authoring.tool_descriptions.special_cases`
const PAGE_NOTE: &str =
    "More entries remain: call next_page with this page's next token to get them.";

/// definition are counted, so a page never exceeds its estimate.
fn paginate(
    listing: &Listing,
    values: &[&Value],
    accompanying: &[Vec<Value>],
    reserve: usize,
    search_instructions: bool,
) -> Vec<std::ops::Range<usize>> {
    let total = values.len();
    let longest = json!({
        "total_entries": total,
        "returned_entries": total,
        "next": "0".repeat(TOKEN_LENGTH),
        "note": PAGE_NOTE,
    });
    let mut empty = listing.render(&[], &[], &[], Some(longest), search_instructions);
    if listing.arrays.contains(&"not_listed") {
        empty["not_listed"] = json!([]);
    }
    let overhead = empty.to_string().len() + reserve;
    let mut pages = Vec::new();
    let mut start = 0;
    let mut size = overhead;
    let mut seen: Vec<&Value> = Vec::new();
    for (index, value) in values.iter().enumerate() {
        let fresh = |seen: &[&Value]| {
            accompanying[index]
                .iter()
                .filter(|definition| !seen.contains(definition))
                .map(|definition| definition.to_string().len() + 1)
                .sum::<usize>()
        };
        let cost = value.to_string().len() + 1;
        if index > start && size + cost + fresh(&seen) > PAGE_LIMIT {
            pages.push(start..index);
            start = index;
            size = overhead;
            seen.clear();
        }
        size += cost + fresh(&seen);
        seen.extend(accompanying[index].iter());
        if size > PAGE_LIMIT {
            // A single entry exceeding the limit forms a page of its own.
            pages.push(start..index + 1);
            start = index + 1;
            size = overhead;
            seen.clear();
        }
    }
    if start < total {
        pages.push(start..total);
    }
    pages
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::authoring::inputs::tests::{project, role};
    use crate::authoring::tools::dispatch_paged;

    /// Follows a paged result to its end, checking each page's size and
    /// counts, and returns every page's delivery.
    fn all_pages(pages: &mut ResultPages, first: ToolDelivery) -> Vec<ToolDelivery> {
        let page = &first.result["page"];
        let total = page["total_entries"].as_u64().unwrap();
        let mut delivered = vec![first];
        loop {
            let page = delivered.last().unwrap().result["page"].clone();
            assert_eq!(page["total_entries"], total);
            assert!(page["returned_entries"].as_u64().unwrap() > 0);
            let Some(token) = page["next"].as_str() else {
                assert!(page["next"].is_null());
                // The last page needs no note.
                assert!(page.get("note").is_none(), "{page}");
                break;
            };
            // A page with more after it says how to continue.
            assert_eq!(page["note"], PAGE_NOTE);
            delivered.push(pages.next_page(token).unwrap());
        }
        let returned: u64 = delivered
            .iter()
            .map(|page| page.result["page"]["returned_entries"].as_u64().unwrap())
            .sum();
        assert_eq!(returned, total);
        assert!(delivered.len() > 1);
        delivered
    }

    // {test}`requirements.authoring.result_pages`
    #[test]
    fn a_result_just_within_the_limit_is_returned_whole() {
        let root = project();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let entries = |length: usize| {
            (0..4)
                .map(|index| json!({"path": format!("{index}{}", "x".repeat(length)), "kind": "file"}))
                .collect::<Vec<_>>()
        };
        // Grow the entries until the whole result is a few bytes under the limit.
        let size = |length: usize| Value::Array(entries(length)).to_string().len();
        // Each unit of length adds one byte to each of the four entries.
        let length = 1 + (PAGE_LIMIT - 5 - size(1)) / 4;
        assert!(size(length) > PAGE_LIMIT - NOTE_RESERVE);
        let result = ResultPages::default()
            .deliver(&sources, Listing::array(entries(length)))
            .unwrap()
            .result;
        assert!(result.is_array(), "paged: {}", &result.to_string()[..200]);
        assert!(result.to_string().len() <= PAGE_LIMIT);
    }

    // {test}`requirements.authoring.result_pages`
    #[test]
    fn redacted_results_reserve_space_for_the_note_on_every_delivered_page() {
        let root = project();
        let token = crate::redaction::tests::github_token();
        let read = |count: usize, padding: usize| {
            let source = (0..count)
                .map(|index| {
                    format!(
                        "fn item_{index}() {{ let key = \"{token}\"; let padding = \"{}\"; }}\n",
                        "x".repeat(padding)
                    )
                })
                .collect::<String>();
            fs::write(root.path().join("src/secret.rs"), source).unwrap();
            let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
            let mut pages = ResultPages::default();
            let first = dispatch_paged(
                &sources,
                &mut pages,
                "habeas",
                &json!({"file":"src/secret.rs","line_start":1}),
            )
            .unwrap();
            let mut delivered = vec![first];
            while let Some(next) = delivered.last().unwrap().result["page"]["next"].as_str() {
                let next = next.to_owned();
                delivered.push(
                    dispatch_paged(&sources, &mut pages, "next_page", &json!({"token":next}))
                        .unwrap(),
                );
            }
            for page in &delivered {
                let encoded = page.result.to_string();
                assert!(encoded.len() <= PAGE_LIMIT, "{}", encoded.len());
                assert_eq!(page.result["note"], super::super::tools::REDACTION_NOTE);
                // Continuation guidance is inside `page`; the redaction
                // explanation is a separate note on the enclosing result.
                if page.result["page"]["next"].is_string() {
                    assert_eq!(page.result["page"]["note"], PAGE_NOTE);
                } else {
                    assert!(page.result["page"].get("note").is_none());
                }
                assert!(!encoded.contains(&token));
                assert!(encoded.contains("[REDACTED:"));
            }
            delivered
        };
        let baseline = read(4, 1);
        assert_eq!(baseline.len(), 1);
        assert!(baseline[0].result.get("page").is_none());
        let mut without_note = baseline[0].result.clone();
        without_note.as_object_mut().unwrap().remove("note");
        let size = without_note.to_string().len();
        let padding = 1 + (PAGE_LIMIT - 5 - size) / 4;
        // The raw result fits; its redaction note pushes it over the limit.
        assert!(size + 4 * (padding - 1) <= PAGE_LIMIT);
        assert!(size + 4 * (padding - 1) + super::super::tools::REDACTION_NOTE.len() > PAGE_LIMIT);
        for (count, padding) in [(4, padding), (256, 256)] {
            let pages = read(count, padding);
            assert!(pages.len() > 1);
            let items = pages
                .iter()
                .flat_map(|page| page.result["inputs"].as_array().unwrap())
                .map(|input| input["item"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>();
            assert_eq!(
                items,
                (0..count)
                    .map(|index| format!("fn[item_{index}]"))
                    .collect::<Vec<_>>()
            );
        }
    }

    // {test}`requirements.authoring.result_pages`
    // {test}`requirements.authoring.not_listed`
    #[test]
    fn not_listed_comes_on_the_first_page_within_the_page_limit() {
        let root = project();
        for index in 0..800 {
            fs::write(
                root.path()
                    .join(format!("src/file_with_a_rather_long_name_{index:04}.rs")),
                "fn f() {}\n",
            )
            .unwrap();
        }
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .current_dir(root.path())
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
        };
        git(&["init", "--quiet"]);
        git(&["add", "."]);
        for index in 0..40 {
            fs::write(
                root.path().join(format!(
                    "src/untracked_draft_with_a_long_name_{index:02}.rs"
                )),
                "fn f() {}\n",
            )
            .unwrap();
        }
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let mut pages = ResultPages::default();
        let first =
            dispatch_paged(&sources, &mut pages, "list_files", &json!({"path":"src"})).unwrap();
        let delivered = all_pages(&mut pages, first);
        assert_eq!(
            delivered[0].result["not_listed"].as_array().unwrap().len(),
            40,
            "{}",
            delivered[0].result
        );
        for (number, page) in delivered.iter().enumerate() {
            assert!(page.result.to_string().len() <= PAGE_LIMIT, "page {number}");
            if number > 0 {
                assert!(page.result.get("not_listed").is_none(), "page {number}");
            }
        }
    }

    // {test}`requirements.authoring.result_pages`
    // {test}`requirements.authoring.not_listed`
    #[test]
    fn untracked_entries_page_without_tracked_entries_or_oversized_overhead() {
        let root = project();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let untracked: Vec<Value> = (0..1000)
            .map(|i| json!({"path":format!("src/{i:04}_{}.rs", "x".repeat(70)), "reason":"untracked"}))
            .collect();
        for count in [0, 1, 500] {
            let tracked: Vec<Value> = (0..count)
                .map(|i| json!({"path":format!("src/tracked_{i:04}.rs"),"kind":"file"}))
                .collect();
            let mut pages = ResultPages::default();
            let first = pages
                .deliver(
                    &sources,
                    Listing::array(tracked.clone()).with_not_listed(untracked.clone()),
                )
                .unwrap();
            assert!(
                first.result["not_listed"]
                    .as_array()
                    .is_some_and(|v| !v.is_empty())
            );
            let delivered = all_pages(&mut pages, first);
            let mut actual_tracked = Vec::new();
            let mut actual_untracked = Vec::new();
            for page in delivered {
                assert!(page.result.to_string().len() <= PAGE_LIMIT);
                assert!(page.inputs.is_empty(), "untracked paths are never evidence");
                let entries = page.result["entries"].as_array().unwrap();
                let omitted = page
                    .result
                    .get("not_listed")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                assert_eq!(
                    page.result["page"]["total_entries"],
                    count + untracked.len()
                );
                assert_eq!(
                    page.result["page"]["returned_entries"],
                    entries.len() + omitted.len()
                );
                actual_tracked.extend(entries.iter().cloned());
                actual_untracked.extend(omitted);
            }
            assert_eq!(actual_tracked, tracked);
            assert_eq!(actual_untracked, untracked);
        }
    }

    // {test}`requirements.authoring.result_pages`
    // {test}`requirements.authoring.search_accounting.result_ids`
    #[test]
    fn text_and_structural_search_results_page_whole_ordered_matches() {
        let root = project();
        let source: String = (0..600)
            .map(|i| format!("fn needle_{i:04}() {{ let marker = 1; }}\n"))
            .collect();
        fs::write(root.path().join("src/lib.rs"), source).unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        for (tool, arguments) in [
            ("ripgrep", json!({"pattern":"marker", "path":"src/lib.rs"})),
            (
                "ast_grep",
                json!({"language":"rust","rule":{"rule":{"pattern":"let marker = 1;"}},"path":"src/lib.rs"}),
            ),
        ] {
            let mut pages = ResultPages::default();
            let first = dispatch_paged(&sources, &mut pages, tool, &arguments).unwrap();
            let mut items = Vec::new();
            let mut ids = std::collections::BTreeSet::new();
            for page in all_pages(&mut pages, first) {
                let id = uuid::Uuid::parse_str(page.result["search_id"].as_str().unwrap()).unwrap();
                assert!(ids.insert(id), "reused search UUID for {tool}");
                assert!(page.result.to_string().len() <= PAGE_LIMIT, "{tool}");
                let matches = page.result["matches"].as_array().unwrap();
                assert_eq!(page.result["page"]["total_entries"], 600);
                assert_eq!(page.result["page"]["returned_entries"], matches.len());
                items.extend(
                    matches
                        .iter()
                        .map(|m| m["item"].as_str().unwrap().to_owned()),
                );
            }
            assert_eq!(
                items,
                (0..600)
                    .map(|i| format!("fn[needle_{i:04}]"))
                    .collect::<Vec<_>>(),
                "{tool}"
            );
        }
    }

    // {test}`requirements.authoring.search_accounting.pagination`
    // {test}`requirements.authoring.search_accounting.page_replay`
    // {test}`requirements.authoring.search_accounting.outstanding`
    // {test}`requirements.authoring.search_accounting.entry_tags`
    // {test}`requirements.authoring.search_accounting.result_ids`
    // {test}`requirements.authoring.search_accounting.first_result`
    // {test}`requirements.authoring.search_accounting.instructions`
    // {test}`requirements.authoring.finish_search.completion`
    // {test}`requirements.authoring.finish_search.invalid`
    // {test}`requirements.authoring.result_pages`
    // {test}`requirements.authoring.structural_search`
    // {test}`requirements.authoring.finish_search.inputs`
    #[test]
    fn search_pages_are_accounted_only_when_delivered_and_replay_stays_closed() {
        let root = project();
        let source: String = (0..600)
            .map(|i| format!("fn needle_{i:04}() {{ let marker = 1; }}\n"))
            .collect();
        fs::write(root.path().join("src/lib.rs"), source).unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        for (tool, arguments) in [
            ("ripgrep", json!({"pattern":"marker","path":"src/lib.rs"})),
            (
                "ast_grep",
                json!({"language":"rust","rule":{"rule":{"pattern":"let marker = 1;"}}}),
            ),
        ] {
            let mut pages = ResultPages::default();
            let first = dispatch_paged(&sources, &mut pages, tool, &arguments)
                .unwrap()
                .result;
            let token = first["page"]["next"].as_str().unwrap().to_owned();
            assert_eq!(
                first["instructions"],
                super::super::approved_text("search_accounting.instructions", 0)
            );
            uuid::Uuid::parse_str(first["search_id"].as_str().unwrap()).unwrap();
            assert_eq!(
                pages.outstanding_searches(),
                [first["search_id"].as_str().unwrap()]
            );
            let undelivered = pages.pages[&token].0.clone();
            assert!(
                pages
                    .finish_search(&json!({"search_id":undelivered["search_id"],"used":[]}))
                    .is_err()
            );
            assert!(
                pages
                    .finish_search(&json!({"search_id":first["search_id"],
                "used":[undelivered["matches"][0]["tag"]]}))
                    .is_err()
            );
            let second = pages.next_page(&token).unwrap().result;
            uuid::Uuid::parse_str(second["search_id"].as_str().unwrap()).unwrap();
            assert_eq!(second, undelivered);
            assert_ne!(first["search_id"], second["search_id"]);
            assert!(second.get("instructions").is_none());
            assert_eq!(pages.outstanding_searches().len(), 2);
            let finished = pages
                .finish_search(&json!({"search_id":second["search_id"],
                "used":[second["matches"][0]["tag"]]}))
                .unwrap();
            assert_eq!(finished.inputs.len(), 1);
            assert_eq!(finished.inputs[0]["item"], second["matches"][0]["item"]);
            let replay = pages.next_page(&token).unwrap().result;
            assert_eq!(replay, second);
            assert_eq!(
                pages.outstanding_searches(),
                [first["search_id"].as_str().unwrap()]
            );
            pages
                .finish_search(&json!({"search_id":first["search_id"],"used":[]}))
                .unwrap();
            assert!(pages.outstanding_searches().is_empty());
            assert!(sources.recorded_inputs().is_empty());
            assert!(first.to_string().len() <= PAGE_LIMIT);
            assert!(second.to_string().len() <= PAGE_LIMIT);
        }
    }

    // {test}`requirements.authoring.tool_descriptions.special_cases`
    // {test}`requirements.authoring.result_pages`
    #[test]
    // {test}`requirements.authoring.file_discovery`
    fn large_listings_and_outlines_page_whole_entries_in_order() {
        let root = project();
        let names = (0..800)
            .map(|index| format!("src/file_with_a_rather_long_name_{index:04}.rs"))
            .collect::<Vec<_>>();
        for name in &names {
            fs::write(root.path().join(name), "fn f() {}\n").unwrap();
        }
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let mut pages = ResultPages::default();
        let first =
            dispatch_paged(&sources, &mut pages, "list_files", &json!({"path":"src"})).unwrap();
        let delivered = all_pages(&mut pages, first);
        let mut listed = Vec::new();
        for page in &delivered {
            assert!(page.result.to_string().len() <= PAGE_LIMIT);
            assert!(page.inputs.is_empty());
            assert_eq!(page.result.as_object().unwrap().len(), 2, "{}", page.result);
            let entries = page.result["entries"].as_array().unwrap();
            assert_eq!(
                page.result["page"]["returned_entries"],
                entries.len() as u64
            );
            listed.extend(
                entries
                    .iter()
                    .map(|entry| entry["path"].as_str().unwrap().to_owned()),
            );
        }
        // Whole entries, in the listing's order, each once.
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(listed, sorted);
        // A page is filled before the next starts.
        let first = delivered[0].result.to_string().len();
        assert!(first > PAGE_LIMIT - 200, "{first}");

        // An outline pages its top-level nodes, each with its children.
        let mut source = String::new();
        for index in 0..600 {
            source.push_str(&format!("mod module_{index} {{\n    fn inner() {{}}\n}}\n"));
        }
        fs::write(root.path().join("src/lib.rs"), source).unwrap();
        let first = dispatch_paged(
            &sources,
            &mut pages,
            "outline",
            &json!({"file":"src/lib.rs"}),
        )
        .unwrap();
        let delivered = all_pages(&mut pages, first);
        let nodes = delivered
            .iter()
            .flat_map(|page| page.result["entries"].as_array().unwrap().clone())
            .collect::<Vec<_>>();
        assert_eq!(nodes.len(), 600);
        for (index, node) in nodes.iter().enumerate() {
            assert_eq!(node["name"], format!("module_{index}"));
            assert_eq!(node["children"].as_array().unwrap().len(), 1);
        }
    }

    // {test}`requirements.authoring.result_pages`
    // {test}`requirements.authoring.delivered_inputs`
    // {test}`requirements.definitions.authoring_inputs`
    // {test}`requirements.authoring.read_denial`
    #[test]
    // {test}`requirements.authoring.source_tools`
    fn paged_inputs_carry_their_definitions_and_deliver_only_their_own_inputs() {
        let root = project();
        fs::write(
            root.path().join("md/terms.md"),
            ":::{definition} value\n:phrase: value\nA useful value.\n:::\n",
        )
        .unwrap();
        let term = role("term", "terms.value");
        let mut source = String::new();
        for index in 0..400 {
            // Only the last function uses the term, so only its page needs it.
            let comment = if index == 399 { term.as_str() } else { "plain" };
            source.push_str(&format!(
                "// {comment}\nfn needle_{index}() {{\n    let text = \"padding padding padding\";\n}}\n\n"
            ));
        }
        fs::write(root.path().join("src/lib.rs"), source).unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let mut pages = ResultPages::default();
        let error = dispatch_paged(&sources, &mut pages, "next_page", &json!({}))
            .err()
            .unwrap();
        assert!(
            error.to_string().contains("token must be a string"),
            "{error}"
        );
        let first = dispatch_paged(
            &sources,
            &mut pages,
            "habeas",
            &json!({"file":"src/lib.rs","line_start":1}),
        )
        .unwrap();
        let delivered = all_pages(&mut pages, first);
        let mut items = Vec::new();
        for (number, page) in delivered.iter().enumerate() {
            assert!(page.result.to_string().len() <= PAGE_LIMIT);
            let inputs = page.result["inputs"].as_array().unwrap();
            let definitions = page.result["definitions"].as_array().unwrap();
            // A page's delivery is exactly its own inputs and definitions.
            let mut expected = inputs.clone();
            expected.extend(definitions.iter().cloned());
            assert_eq!(page.inputs, expected);
            let last = number == delivered.len() - 1;
            assert_eq!(
                definitions
                    .iter()
                    .any(|definition| definition["definition"] == "terms.value"),
                last,
                "page {number}: {definitions:?}"
            );
            items.extend(
                inputs
                    .iter()
                    .map(|input| input["item"].as_str().unwrap().to_owned()),
            );
        }
        let expected = (0..400)
            .map(|index| format!("fn[needle_{index}]"))
            .collect::<Vec<_>>();
        assert_eq!(items, expected);

        // A token is unknown to another assessment, and an unknown token is
        // an explanatory tool error.
        let token = delivered[0].result["page"]["next"].as_str().unwrap();
        let mut other = ResultPages::default();
        let error = other.next_page(token).err().unwrap();
        assert!(error.to_string().contains("unknown page token"), "{error}");
        let error = dispatch_paged(
            &sources,
            &mut pages,
            "next_page",
            &json!({"token":"not-a-token"}),
        )
        .err()
        .unwrap();
        assert!(
            error.to_string().contains("valid only in the assessment"),
            "{error}"
        );
        // A token's page comes back in the same shape when asked again.
        let again =
            dispatch_paged(&sources, &mut pages, "next_page", &json!({"token": token})).unwrap();
        assert_eq!(again.result, delivered[1].result);
    }

    // {test}`requirements.authoring.result_pages`
    #[test]
    fn an_entry_exceeding_the_limit_forms_a_page_of_its_own_and_small_results_stay_whole() {
        let root = project();
        let big = format!(
            "fn big() {{\n{}}}\n",
            "    let x = \"padding padding padding padding padding\";\n".repeat(900)
        );
        let small = "fn small() {}\n";
        fs::write(
            root.path().join("src/lib.rs"),
            format!("fn before() {{}}\n\n{big}\nfn after() {{}}\n\n{small}"),
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let mut pages = ResultPages::default();
        let first = dispatch_paged(
            &sources,
            &mut pages,
            "habeas",
            &json!({"file":"src/lib.rs","line_start":1}),
        )
        .unwrap();
        let delivered = all_pages(&mut pages, first);
        let sizes = delivered
            .iter()
            .map(|page| {
                (
                    page.result["page"]["returned_entries"].as_u64().unwrap(),
                    page.result.to_string().len(),
                )
            })
            .collect::<Vec<_>>();
        let oversize = delivered
            .iter()
            .find(|page| page.result.to_string().len() > PAGE_LIMIT)
            .expect("the big function exceeds the limit");
        assert_eq!(oversize.result["page"]["returned_entries"], 1, "{sizes:?}");
        assert_eq!(oversize.result["inputs"][0]["item"], "fn[big]");
        for page in &delivered {
            if page.result.to_string().len() > PAGE_LIMIT {
                assert_eq!(page.result["page"]["returned_entries"], 1, "{sizes:?}");
            }
        }

        // A result within the limit is returned whole, without page.
        let whole = dispatch_paged(
            &sources,
            &mut pages,
            "habeas",
            &json!({"file":"src/lib.rs","line_start":1,"line_end":1}),
        )
        .unwrap();
        assert!(whole.result.get("page").is_none(), "{}", whole.result);
        assert_eq!(whole.result["inputs"][0]["item"], "fn[before]");
        let listing =
            dispatch_paged(&sources, &mut pages, "list_files", &json!({"path":"src"})).unwrap();
        assert_eq!(listing.result, json!([{"path":"src/lib.rs","kind":"file"}]));
    }

    // {test}`requirements.authoring.result_pages`
    // {test}`requirements.authoring.reference_index`
    #[test]
    fn reference_results_page_definitions_then_sites() {
        let root = project();
        let mut book = String::new();
        for index in 0..400 {
            book.push_str(&format!(
                ":::{{req}} requirement_with_a_long_name_{index:03}\nThe project MUST do {index}.\n:::\n\n"
            ));
        }
        fs::write(root.path().join("md/requirements.md"), book).unwrap();
        let mut source = String::new();
        for index in 0..400 {
            source.push_str(&format!(
                "// {}\nfn site_{index}() {{}}\n\n",
                role(
                    "impl",
                    &format!("requirements.requirement_with_a_long_name_{index:03}")
                )
            ));
        }
        fs::write(root.path().join("src/lib.rs"), source).unwrap();
        let sources = AssessmentSources::prepare(
            root.path(),
            "requirements.requirement_with_a_long_name_000",
        )
        .unwrap();
        let mut pages = ResultPages::default();
        let first = dispatch_paged(
            &sources,
            &mut pages,
            "references",
            &json!({"selection": "requirements.*"}),
        )
        .unwrap();
        let delivered = all_pages(&mut pages, first);
        let (mut definitions, mut sites) = (Vec::new(), Vec::new());
        for page in &delivered {
            assert!(page.result.to_string().len() <= PAGE_LIMIT);
            assert!(page.inputs.is_empty());
            let held = page.result["definitions"].as_array().unwrap();
            let found = page.result["sites"].as_array().unwrap();
            assert_eq!(
                page.result["page"]["returned_entries"],
                (held.len() + found.len()) as u64
            );
            // Sites follow every definition.
            assert!(found.is_empty() || definitions.len() + held.len() == 400);
            definitions.extend(held.iter().map(|entry| entry["name"].clone()));
            sites.extend(found.iter().map(|entry| entry["name"].clone()));
        }
        assert_eq!(definitions.len(), 400);
        assert_eq!(sites.len(), 400);
        assert_eq!(
            delivered[0].result["page"]["total_entries"], 800,
            "{}",
            delivered[0].result["page"]
        );
    }
}

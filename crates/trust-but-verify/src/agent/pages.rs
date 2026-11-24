use super::ToolDelivery;
use crate::graph::Dependency;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::{collections::HashMap, io};
use uuid::Uuid;

const PAGE_SIZE: usize = 8192;

const INSTRUCTIONS: &str = "Before submitting, close every delivered search page with finish_search({search_id, used:[tags]}). Include tags for excerpts you relied on without a separate source read. Use used:[] if none qualify, including an empty result. Submission waits for every open search. Finishing records the selected inputs without reading more source.";

pub(super) type SearchEntry<N, L> = (Value, Result<Dependency<N, L>, String>);

struct SearchPage<N, L> {
    delivered: bool,
    finished: bool,
    entries: BTreeMap<String, Result<Dependency<N, L>, String>>,
}

/// Continuations for one model conversation, keyed by the UUID supplied in `next`.
pub struct Pages<N, L> {
    retained: HashMap<Uuid, ToolDelivery<N, L>>,
    searches: BTreeMap<Uuid, SearchPage<N, L>>,
}

impl<N: Clone, L: Clone> Pages<N, L> {
    pub(super) fn new() -> Self {
        Self {
            retained: HashMap::new(),
            searches: BTreeMap::new(),
        }
    }

    // {impl}`requirements.sdk.graph.validation-context.pages.behavior`
    // {impl}`requirements.sdk.graph.validation-context.pages.inputs`
    pub(super) fn prepare(
        &mut self,
        metadata: Value,
        field: &str,
        value: Value,
        dependencies: Vec<Dependency<N, L>>,
    ) -> ToolDelivery<N, L> {
        let parts = match value {
            Value::String(text) => text_pages(text),
            Value::Array(entries) => entry_pages(entries),
            _ => unreachable!("only text and sequences are paged"),
        };
        // Build backwards so replaying a page always yields the same successor.
        let mut next = None;
        for (index, part) in parts.into_iter().enumerate().rev() {
            let mut page = metadata.clone();
            page[field] = part;
            if let Some(uuid) = next {
                page["next"] = Value::String(uuid);
            }
            let delivery = ToolDelivery {
                text: page.to_string(),
                dependencies: dependencies.clone(),
            };
            if index == 0 {
                return delivery;
            }
            let uuid = Uuid::new_v4();
            self.retained.insert(uuid, delivery);
            next = Some(uuid.to_string());
        }
        unreachable!("even an empty result has a first page")
    }

    // {impl}`requirements.sdk.graph.search.first_search.instructions`
    // {impl}`requirements.sdk.graph.search.first_search.instructions.content`
    // {impl}`requirements.sdk.graph.search.search_text.mcp`
    // {impl}`requirements.sdk.graph.search.search_structure.mcp`
    // {impl}`requirements.sdk.graph.search.finish_search.behavior`
    pub(super) fn search(
        &mut self,
        results: crate::sources::SearchResults<SearchEntry<N, L>>,
    ) -> ToolDelivery<N, L> {
        let complete = results.complete;
        let entries = results.matches;
        let first_search = self.searches.is_empty();
        let mut next = None;
        for (index, chunk) in chunks(entries, |(value, _)| value.to_string().len())
            .into_iter()
            .enumerate()
            .rev()
        {
            let id = Uuid::new_v4();
            let mut observations = BTreeMap::new();
            let mut matches = Vec::new();
            for (i, (mut entry, input)) in chunk.into_iter().enumerate() {
                let tag = (i + 1).to_string();
                entry["tag"] = json!(tag);
                observations.insert(tag, input);
                matches.push(entry);
            }
            let mut page = json!({"search_id":id,"complete":complete,"matches":matches});
            if first_search && index == 0 {
                page["instructions"] = json!(INSTRUCTIONS);
            }
            if let Some(next) = next {
                page["next"] = json!(next);
            }
            self.searches.insert(
                id,
                SearchPage {
                    delivered: index == 0,
                    finished: false,
                    entries: observations,
                },
            );
            let delivery = ToolDelivery {
                text: page.to_string(),
                dependencies: vec![],
            };
            if index == 0 {
                return delivery;
            }
            self.retained.insert(id, delivery);
            next = Some(id);
        }
        unreachable!("an empty search still has a first page")
    }

    pub(super) fn outstanding(&self) -> Vec<String> {
        self.searches
            .iter()
            .filter(|(_, page)| page.delivered && !page.finished)
            .map(|(id, _)| id.to_string())
            .collect()
    }

    // {impl}`requirements.sdk.graph.search.finish_search.behavior`
    // {impl}`requirements.sdk.graph.search.finish_search.inputs`
    // {impl}`requirements.sdk.graph.search.finish_search.mcp`
    pub(super) fn finish_search(&mut self, args: &Value) -> io::Result<ToolDelivery<N, L>> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Finish {
            search_id: Uuid,
            used: Vec<String>,
        }
        let Finish { search_id, used } =
            serde_json::from_value(args.clone()).map_err(io::Error::other)?;
        let page = self
            .searches
            .get_mut(&search_id)
            .filter(|page| page.delivered)
            .ok_or_else(|| {
                io::Error::other("search_id has not been delivered in this conversation")
            })?;
        let mut dependencies = Vec::new();
        for tag in used {
            let input = page
                .entries
                .get(&tag)
                .ok_or_else(|| io::Error::other(format!("unknown search tag {tag:?}")))?;
            dependencies.push(input.clone().map_err(io::Error::other)?);
        }
        page.finished = true;
        Ok(ToolDelivery {
            text: json!({"search_id":search_id,"finished":true}).to_string(),
            dependencies,
        })
    }

    // {impl}`requirements.sdk.graph.validation-context.pages.behavior`
    pub(super) fn get(&mut self, args: &Value) -> io::Result<ToolDelivery<N, L>> {
        let uuid = args["uuid"]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or_else(|| io::Error::other("uuid must be a UUID string"))?;
        if let Some(search) = self.searches.get_mut(&uuid) {
            search.delivered = true;
        }
        self.retained
            .get(&uuid)
            .cloned()
            .ok_or_else(|| io::Error::other("unknown page UUID"))
    }
}

fn text_pages(text: String) -> Vec<Value> {
    let mut parts = Vec::new();
    let mut start = 0;
    for (end, _) in text.char_indices().step_by(PAGE_SIZE).skip(1) {
        parts.push(Value::String(text[start..end].to_owned()));
        start = end;
    }
    parts.push(Value::String(text[start..].to_owned()));
    parts
}

fn entry_pages(entries: Vec<Value>) -> Vec<Value> {
    chunks(entries, |entry| entry.to_string().len())
        .into_iter()
        .map(Value::Array)
        .collect()
}

fn chunks<T>(entries: Vec<T>, length: impl Fn(&T) -> usize) -> Vec<Vec<T>> {
    let mut parts = Vec::new();
    let mut current = Vec::new();
    let mut size = 0;
    for entry in entries {
        let cost = length(&entry) + 1;
        if !current.is_empty() && size + cost > PAGE_SIZE {
            parts.push(std::mem::take(&mut current));
            size = 0;
        }
        current.push(entry);
        size += cost;
    }
    parts.push(current);
    parts
}

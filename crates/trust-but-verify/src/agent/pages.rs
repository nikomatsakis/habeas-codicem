use super::ToolDelivery;
use crate::graph::Dependency;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::{collections::HashMap, io};
use uuid::Uuid;

const PAGE_SIZE: usize = 8192;

const INSTRUCTIONS: &str = "Before submitting, close every delivered search page with finish_search({search_id, used:[tags]}). Include tags for excerpts you relied on without a separate source read. Use used:[] if none qualify, including an empty result. Submission waits for every open search. Finishing records the selected inputs without reading more source.";

pub(super) type SearchEntry<N, L> = (Value, Vec<Dependency<N, L>>);

struct SearchPage<N, L> {
    delivered: bool,
    finished: bool,
    entries: BTreeMap<String, Vec<Dependency<N, L>>>,
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
        let pages = parts
            .into_iter()
            .map(|part| {
                let mut page = metadata.clone();
                let inputs = match field {
                    "text" => 1,
                    "matches" => part.as_array().map_or(0, Vec::len),
                    _ => 0,
                };
                page[field] = part;
                (page, inputs)
            })
            .collect();
        self.retain(pages, dependencies)
    }

    // {impl}`requirements.sdk.graph.validation-context.read_node.source_children`
    pub(super) fn source_read(
        &mut self,
        metadata: Value,
        text: String,
        children: Vec<Value>,
        dependency: Dependency<N, L>,
    ) -> ToolDelivery<N, L> {
        let mut text = text_pages(text).into_iter();
        let mut children = entry_pages(children).into_iter();
        let pages = (0..text.len().max(children.len()))
            .map(|_| {
                let mut page = metadata.clone();
                page["text"] = text.next().unwrap_or_else(|| json!(""));
                page["children"] = children.next().unwrap_or_else(|| json!([]));
                (page, 1)
            })
            .collect();
        self.retain(pages, vec![dependency])
    }

    fn retain(
        &mut self,
        pages: Vec<(Value, usize)>,
        dependencies: Vec<Dependency<N, L>>,
    ) -> ToolDelivery<N, L> {
        // Build backwards so replaying a page always yields the same successor.
        let mut next = None;
        for (index, (mut page, input_count)) in pages.into_iter().enumerate().rev() {
            if let Some(uuid) = next {
                page["next"] = Value::String(uuid);
            }
            let delivery = ToolDelivery {
                text: page.to_string(),
                input_count,
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
    // {impl}`requirements.sdk.graph.search.agent_search_accounting`
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
            let input_count = chunk.len();
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
                input_count,
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
    // {impl}`requirements.authoring.finish_search.inputs`
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
            dependencies.extend(input.iter().cloned());
        }
        page.finished = true;
        Ok(ToolDelivery {
            text: json!({"search_id":search_id,"finished":true}).to_string(),
            input_count: 0,
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

#[cfg(test)]
mod tests {
    use super::*;

    // {test}`requirements.sdk.graph.validation-context.pages.behavior`
    #[test]
    fn text_listing_and_outline_continuations_replay_the_same_page_and_successor() {
        let entries = json!([
            "a".repeat(PAGE_SIZE),
            "b".repeat(PAGE_SIZE),
            "c".repeat(PAGE_SIZE)
        ]);
        for (field, value) in [
            ("text", json!("abc".repeat(PAGE_SIZE))),
            ("files", entries.clone()),
            ("items", entries),
        ] {
            let mut pages = Pages::<(), ()>::new();
            let first = pages.prepare(json!({}), field, value, vec![]);
            let next =
                |text: &str| json!({"uuid":serde_json::from_str::<Value>(text).unwrap()["next"]});
            let second_id = next(&first.text);
            let second = pages.get(&second_id).unwrap();
            let third_id = next(&second.text);
            assert!(third_id["uuid"].is_string());
            let third = pages.get(&third_id).unwrap();
            assert_eq!(pages.get(&second_id).unwrap().text, second.text);
            assert_eq!(pages.get(&third_id).unwrap().text, third.text);
            assert!(Pages::<(), ()>::new().get(&second_id).is_err());
        }
    }
}

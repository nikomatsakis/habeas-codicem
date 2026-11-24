use super::ToolDelivery;
use crate::graph::Dependency;
use serde_json::Value;
use std::{collections::HashMap, io};
use uuid::Uuid;

const PAGE_SIZE: usize = 8192;

/// Continuations for one model conversation, keyed by the UUID supplied in `next`.
pub struct Pages<N, L> {
    retained: HashMap<Uuid, ToolDelivery<N, L>>,
}

impl<N: Clone, L: Clone> Pages<N, L> {
    pub(super) fn new() -> Self {
        Self {
            retained: HashMap::new(),
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

    // {impl}`requirements.sdk.graph.validation-context.pages.behavior`
    pub(super) fn get(&self, args: &Value) -> io::Result<ToolDelivery<N, L>> {
        let uuid = args["uuid"]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or_else(|| io::Error::other("uuid must be a UUID string"))?;
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
    let mut parts = Vec::new();
    let mut current = Vec::new();
    let mut size = 0;
    for entry in entries {
        let cost = entry.to_string().len() + 1;
        if !current.is_empty() && size + cost > PAGE_SIZE {
            parts.push(Value::Array(std::mem::take(&mut current)));
            size = 0;
        }
        current.push(entry);
        size += cost;
    }
    parts.push(Value::Array(current));
    parts
}

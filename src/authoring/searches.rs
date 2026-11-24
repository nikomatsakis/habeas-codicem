//! Accounting for dependencies discovered through delivered search snippets.

use std::collections::BTreeMap;
use std::io;

use serde_json::{Value, json};
use uuid::Uuid;

use super::inputs::AssessmentSources;
use super::tools::ToolDelivery;

// {impl}`requirements.authoring.search_accounting.outstanding`
#[derive(Default)]
pub(super) struct Searches {
    results: BTreeMap<Uuid, SearchResult>,
    delivered_any: bool,
}

// {cite}`habeas:util`
struct SearchResult {
    inputs: BTreeMap<String, Value>,
    delivered: bool,
    finished: bool,
}

impl Searches {
    // {impl}`requirements.authoring.search_accounting.first_result`
    pub(super) fn needs_instructions(&self) -> bool {
        !self.delivered_any
    }

    /// Assigns an identity and retains hashes from the already-parsed source.
    /// Preparing an undelivered page does not make its search outstanding.
    // {impl}`requirements.authoring.search_accounting.result_ids`
    // {impl}`requirements.authoring.finish_search.snapshot`
    pub(super) fn prepare(
        &mut self,
        sources: &AssessmentSources,
        result: &mut Value,
    ) -> io::Result<()> {
        let Some(matches) = result.get("matches").and_then(Value::as_array) else {
            return Ok(());
        };
        let mut inputs = BTreeMap::new();
        for entry in matches {
            let file = entry["file"].as_str().expect("search file");
            let item = entry["item"].as_str().expect("search item");
            // Search construction already checked access and populated this
            // assessment's immutable outline cache. No model read is needed.
            let parsed = sources.outlines.parse(&sources.root, file);
            let parsed = parsed
                .as_ref()
                .as_ref()
                .map_err(|error| io::Error::other(error.clone()))?;
            let index = parsed.resolve(file, item).map_err(io::Error::other)?;
            inputs.insert(
                entry["tag"].as_str().expect("search tag").to_owned(),
                json!({"file": file, "item": item,
                    "hash": parsed.outline.nodes[index].structural_hash}),
            );
        }
        let id = loop {
            let id = Uuid::new_v4();
            if !self.results.contains_key(&id) {
                break id;
            }
        };
        result["search_id"] = json!(id.to_string());
        self.results.insert(
            id,
            SearchResult {
                inputs,
                delivered: false,
                finished: false,
            },
        );
        Ok(())
    }

    // {impl}`requirements.authoring.search_accounting.outstanding`
    // {impl}`requirements.authoring.search_accounting.page_replay`
    pub(super) fn delivered(&mut self, result: &Value) {
        if let Some(id) = result.get("search_id").and_then(Value::as_str) {
            let id = Uuid::parse_str(id).expect("prepared search UUID");
            self.results
                .get_mut(&id)
                .expect("prepared search")
                .delivered = true;
            self.delivered_any = true;
        }
    }

    // {impl}`requirements.authoring.submission.searches.diagnostic`
    pub(super) fn outstanding(&self) -> Vec<String> {
        self.results
            .iter()
            .filter(|(_, result)| result.delivered && !result.finished)
            .map(|(id, _)| id.to_string())
            .collect()
    }

    // {impl}`requirements.authoring.finish_search.completion`
    // {impl}`requirements.authoring.finish_search.empty`
    // {impl}`requirements.authoring.finish_search.no_additional_source`
    // {impl}`requirements.authoring.finish_search.inputs`
    // {impl}`requirements.authoring.finish_search.unselected`
    // {impl}`requirements.authoring.finish_search.deduplication`
    // {impl}`requirements.authoring.finish_search.repetition`
    // {impl}`requirements.authoring.finish_search.invalid`
    // {impl}`requirements.authoring.finish_search.atomicity`
    pub(super) fn finish(&mut self, arguments: &Value) -> io::Result<ToolDelivery> {
        let args = arguments
            .as_object()
            .filter(|args| {
                args.len() == 2 && args.contains_key("search_id") && args.contains_key("used")
            })
            .ok_or_else(|| io::Error::other("finish_search requires exactly search_id and used"))?;
        let id = args["search_id"]
            .as_str()
            .and_then(|id| Uuid::parse_str(id).ok())
            .ok_or_else(|| io::Error::other("finish_search search_id must be a UUID string"))?;
        let result = self
            .results
            .get_mut(&id)
            .filter(|result| result.delivered)
            .ok_or_else(|| {
                io::Error::other(format!(
                    "unknown search_id {id}: only search results delivered in this assessment can be finished"
                ))
            })?;
        let tags = args["used"].as_array().ok_or_else(|| {
            io::Error::other("finish_search used must be an array of tag strings")
        })?;
        let mut inputs = Vec::new();
        for tag in tags {
            let tag = tag.as_str().ok_or_else(|| {
                io::Error::other("finish_search used must contain only tag strings")
            })?;
            let input = result.inputs.get(tag).ok_or_else(|| {
                io::Error::other(format!(
                    "unknown tag {tag:?} for search {id}: select tags from that result's matches"
                ))
            })?;
            if !inputs.contains(input) {
                inputs.push(input.clone());
            }
        }
        // No state changes until every argument and tag has been accepted.
        result.finished = true;
        Ok(ToolDelivery {
            result: json!({"search_id": id.to_string(), "finished": true}),
            inputs,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::authoring::inputs::tests::project;
    use crate::authoring::pages::ResultPages;
    use crate::authoring::tools::dispatch_paged;

    fn search(sources: &AssessmentSources, pages: &mut ResultPages, pattern: &str) -> Value {
        let delivery = dispatch_paged(
            sources,
            pages,
            "ripgrep",
            &json!({"pattern":pattern,"path":"src/lib.rs","context":0}),
        )
        .unwrap();
        assert!(delivery.inputs.is_empty());
        delivery.result
    }

    // {test}`requirements.authoring.search_accounting.result_ids`
    // {test}`requirements.authoring.search_accounting.entry_tags`
    // {test}`requirements.authoring.search_accounting.outstanding`
    // {test}`requirements.authoring.search_accounting.first_result`
    // {test}`requirements.authoring.search_accounting.instructions`
    // {test}`requirements.authoring.finish_search.empty`
    // {test}`requirements.authoring.finish_search.invalid`
    // {test}`requirements.authoring.finish_search`
    #[test]
    fn delivered_searches_including_empty_results_need_explicit_completion() {
        let root = project();
        fs::write(
            root.path().join("src/lib.rs"),
            "fn first() {}\nfn second() {}\n",
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let mut pages = ResultPages::default();
        assert!(dispatch_paged(&sources, &mut pages, "ripgrep", &json!({"pattern":"("})).is_err());
        assert!(pages.outstanding_searches().is_empty());
        let first = search(&sources, &mut pages, "fn");
        assert_eq!(
            first["instructions"],
            super::super::approved_text("search_accounting.instructions", 0)
        );
        let tags: Vec<_> = first["matches"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["tag"].as_str().unwrap())
            .collect();
        assert_eq!(tags, ["1", "2"]);
        for entry in first["matches"].as_array().unwrap() {
            assert!(entry.get("hash").is_none());
        }
        let empty = search(&sources, &mut pages, "absent");
        assert_eq!(empty["matches"], json!([]));
        assert!(empty.get("instructions").is_none());
        let repeated = search(&sources, &mut pages, "fn");
        assert!(repeated.get("instructions").is_none());
        let ids = [&first, &empty, &repeated]
            .map(|result| Uuid::parse_str(result["search_id"].as_str().unwrap()).unwrap());
        assert_eq!(
            ids.into_iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            3
        );
        assert_eq!(pages.outstanding_searches().len(), 3);
        let mut other = ResultPages::default();
        let error = other
            .finish_search(&json!({"search_id":first["search_id"],"used":[]}))
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("delivered in this assessment"), "{error}");
        for result in [&first, &empty, &repeated] {
            let finished = pages
                .finish_search(&json!({"search_id":result["search_id"],"used":[]}))
                .unwrap();
            assert!(finished.inputs.is_empty());
            assert_eq!(
                finished.result,
                json!({"search_id":result["search_id"],"finished":true})
            );
        }
        assert!(pages.outstanding_searches().is_empty());
        assert!(sources.recorded_inputs().is_empty());
        for (tool, args) in [
            ("ripgrep", json!({"pattern":"absent","path":"src/lib.rs"})),
            (
                "ast_grep",
                json!({"language":"rust","rule":{"rule":{"pattern":"absent()"}}}),
            ),
        ] {
            let mut fresh = ResultPages::default();
            let result = dispatch_paged(&sources, &mut fresh, tool, &args)
                .unwrap()
                .result;
            assert_eq!(result["matches"], json!([]));
            assert_eq!(
                result["instructions"],
                super::super::approved_text("search_accounting.instructions", 0)
            );
            Uuid::parse_str(result["search_id"].as_str().unwrap()).unwrap();
            let again = dispatch_paged(&sources, &mut fresh, tool, &args)
                .unwrap()
                .result;
            assert_ne!(result["search_id"], again["search_id"]);
        }
    }

    // {test}`requirements.authoring.finish_search.completion`
    // {test}`requirements.authoring.finish_search.no_additional_source`
    // {test}`requirements.authoring.finish_search.inputs`
    // {test}`requirements.authoring.finish_search.snapshot`
    // {test}`requirements.authoring.finish_search.unselected`
    // {test}`requirements.authoring.finish_search.deduplication`
    // {test}`requirements.authoring.finish_search.repetition`
    // {test}`requirements.authoring.delivered_inputs`
    // {test}`requirements.authoring.finish_search`
    #[test]
    fn selected_dependencies_keep_the_delivered_hash_and_accumulate_without_reads() {
        let root = project();
        fs::write(
            root.path().join("src/lib.rs"),
            "fn first() { assert!(true); }\nfn second() {}\n",
        )
        .unwrap();
        let mut sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let mut pages = ResultPages::default();
        let result = search(&sources, &mut pages, "fn");
        let original =
            crate::inputs::outline_input(root.path(), "src/lib.rs", "fn[first]").unwrap();
        // Mutation after delivery must not silently substitute a new hash.
        fs::write(
            root.path().join("src/lib.rs"),
            "fn first() {}\nfn second() {}\n",
        )
        .unwrap();
        let finish = pages
            .finish_search(&json!({"search_id":result["search_id"],"used":["1","1"]}))
            .unwrap();
        assert_eq!(finish.inputs, [original]);
        assert_eq!(
            finish.result,
            json!({"search_id":result["search_id"],"finished":true})
        );
        assert!(finish.result.get("text").is_none());
        assert!(pages.outstanding_searches().is_empty());
        sources.record_inputs(&finish.inputs);
        assert_eq!(sources.recorded_inputs().len(), 1);
        assert_eq!(sources.recorded_inputs()[0]["item"], "fn[first]");
        let mut errors = Vec::new();
        crate::evidence::validate_input(
            root.path(),
            &BTreeMap::new(),
            &crate::inputs::OutlineCache::default(),
            &finish.inputs[0],
            "selected",
            &mut errors,
        );
        assert!(
            errors.iter().any(|error| error.contains("hash")),
            "{errors:?}"
        );

        let second = pages
            .finish_search(&json!({"search_id":result["search_id"],"used":["2"]}))
            .unwrap();
        assert_eq!(
            second.inputs,
            [crate::inputs::outline_input(root.path(), "src/lib.rs", "fn[second]").unwrap()]
        );
        sources.record_inputs(&second.inputs);
        assert_eq!(sources.recorded_inputs().len(), 2);
        // A separately read item can be included harmlessly, or omitted.
        let read = dispatch_paged(
            &sources,
            &mut pages,
            "habeas",
            &json!({"file":"src/lib.rs","item":"fn[second]"}),
        )
        .unwrap();
        sources.record_inputs(&read.inputs);
        let repeated = pages
            .finish_search(&json!({"search_id":result["search_id"],"used":["2"]}))
            .unwrap();
        sources.record_inputs(&repeated.inputs);
        let empty = pages
            .finish_search(&json!({"search_id":result["search_id"],"used":[]}))
            .unwrap();
        sources.record_inputs(&empty.inputs);
        assert_eq!(sources.recorded_inputs().len(), 2);
        assert_eq!(sources.recorded_inputs()[0]["item"], "fn[first]");
        assert_eq!(sources.recorded_inputs()[1]["item"], "fn[second]");
    }

    // {test}`requirements.authoring.finish_search.invalid`
    // {test}`requirements.authoring.finish_search.atomicity`
    // {test}`requirements.authoring.finish_search`
    #[test]
    fn invalid_completion_is_atomic_and_can_be_corrected() {
        let root = project();
        fs::write(root.path().join("src/lib.rs"), "fn first() {}\n").unwrap();
        let mut sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let mut pages = ResultPages::default();
        let result = search(&sources, &mut pages, "first");
        let id = result["search_id"].clone();
        for (args, expected) in [
            (json!(null), "exactly search_id and used"),
            (json!([]), "exactly search_id and used"),
            (json!({}), "exactly search_id and used"),
            (json!({"search_id":id}), "exactly search_id and used"),
            (json!({"used":[]}), "exactly search_id and used"),
            (
                json!({"search_id":id,"used":[],"extra":0}),
                "exactly search_id and used",
            ),
            (
                json!({"search_id":0,"used":[]}),
                "search_id must be a UUID string",
            ),
            (
                json!({"search_id":"bad","used":[]}),
                "search_id must be a UUID string",
            ),
            (
                json!({"search_id":Uuid::new_v4().to_string(),"used":[]}),
                "only search results delivered in this assessment",
            ),
            (
                json!({"search_id":id,"used":null}),
                "used must be an array of tag strings",
            ),
            (
                json!({"search_id":id,"used":"1"}),
                "used must be an array of tag strings",
            ),
            (
                json!({"search_id":id,"used":["1",1]}),
                "used must contain only tag strings",
            ),
            (
                json!({"search_id":id,"used":["1","missing"]}),
                "unknown tag \"missing\"",
            ),
        ] {
            let error = pages.finish_search(&args).err().unwrap().to_string();
            assert!(error.contains(expected), "{args}: {error}");
            assert_eq!(pages.outstanding_searches(), [id.as_str().unwrap()]);
            assert!(sources.recorded_inputs().is_empty());
        }
        let good = pages
            .finish_search(&json!({"search_id":id,"used":["1"]}))
            .unwrap();
        sources.record_inputs(&good.inputs);
        assert_eq!(sources.recorded_inputs().len(), 1);
        assert!(pages.outstanding_searches().is_empty());
        assert!(
            pages
                .finish_search(&json!({"search_id":id,"used":["1","missing"]}))
                .is_err()
        );
        assert!(pages.outstanding_searches().is_empty());
        assert_eq!(sources.recorded_inputs().len(), 1);
    }
}

// {cite}`habeas:util`

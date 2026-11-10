//! Paging of large source-tool results.

use std::collections::HashMap;
use std::io;

use serde_json::{Map, Value, json};

use super::inputs::AssessmentSources;
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
    arrays: &'static [&'static str],
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
            arrays: &[],
            entries: entries.into_iter().map(|entry| (0, entry)).collect(),
            evidence: false,
        }
    }

    /// A result of evidence `inputs` and their additional definitions.
    // {impl}`requirements.authoring.delivered_inputs`
    pub(crate) fn inputs(inputs: Vec<Value>) -> Self {
        Self {
            arrays: &["inputs"],
            entries: inputs.into_iter().map(|input| (0, input)).collect(),
            evidence: true,
        }
    }

    /// A result holding one array per key, in the order of `arrays`.
    pub(crate) fn object(arrays: &'static [&'static str], values: Vec<Vec<Value>>) -> Self {
        Self {
            arrays,
            entries: values
                .into_iter()
                .enumerate()
                .flat_map(|(index, values)| values.into_iter().map(move |value| (index, value)))
                .collect(),
            evidence: false,
        }
    }

    /// The result, or one page of it, holding `entries` with `definitions`.
    fn render(
        &self,
        entries: &[&Value],
        indexes: &[usize],
        definitions: &[Value],
        page: Option<Value>,
    ) -> Value {
        if self.arrays.is_empty() {
            let entries = Value::Array(entries.iter().map(|entry| (*entry).clone()).collect());
            return match page {
                None => entries,
                Some(page) => json!({"entries": entries, "page": page}),
            };
        }
        let mut object = Map::new();
        for (index, key) in self.arrays.iter().enumerate() {
            let values = entries
                .iter()
                .zip(indexes)
                .filter(|(_, array)| **array == index)
                .map(|(entry, _)| (*entry).clone())
                .collect();
            object.insert((*key).to_owned(), Value::Array(values));
        }
        if self.evidence {
            object.insert("definitions".into(), Value::Array(definitions.to_vec()));
        }
        if let Some(page) = page {
            object.insert("page".into(), page);
        }
        Value::Object(object)
    }
}

/// The pages of this assessment's paged results not yet returned, by token.
/// Each assessment has its own, dropped when it finishes.
#[derive(Default)]
pub(crate) struct ResultPages {
    pages: HashMap<String, (Value, Vec<Value>)>,
}

impl ResultPages {
    /// Returns `listing` whole when its JSON is within [`PAGE_LIMIT`], and
    /// otherwise its first page, keeping the rest for `next_page`. Only the
    /// returned page's inputs are delivered.
    // {impl}`requirements.authoring.delivered_inputs`
    pub(crate) fn deliver(
        &mut self,
        sources: &AssessmentSources,
        listing: Listing,
    ) -> io::Result<ToolDelivery> {
        let values: Vec<&Value> = listing.entries.iter().map(|(_, value)| value).collect();
        let indexes: Vec<usize> = listing.entries.iter().map(|(index, _)| *index).collect();
        let owned: Vec<Value> = values.iter().map(|value| (*value).clone()).collect();
        let definitions = if listing.evidence {
            sources.additional_definitions(&owned)?
        } else {
            Vec::new()
        };
        let whole = listing.render(&values, &indexes, &definitions, None);
        if whole.to_string().len() <= PAGE_LIMIT {
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
        let pages = paginate(&listing, &values, &accompanying);
        let tokens: Vec<String> = (1..pages.len()).map(|_| self.token()).collect();
        let mut rendered = Vec::new();
        for (number, range) in pages.iter().enumerate() {
            let (entries, page_indexes) = (&values[range.clone()], &indexes[range.clone()]);
            let definitions = page_definitions(entries, &accompanying[range.clone()]);
            let page = json!({
                "total_entries": total,
                "returned_entries": entries.len(),
                "next": tokens.get(number),
            });
            let result = listing.render(entries, page_indexes, &definitions, Some(page));
            let inputs = delivered(
                &listing,
                entries.iter().map(|entry| (*entry).clone()).collect(),
                definitions,
            );
            rendered.push((result, inputs));
        }
        let mut rendered = rendered.into_iter();
        let (result, inputs) = rendered.next().expect("a paged result has a first page");
        for (token, page) in tokens.into_iter().zip(rendered) {
            self.pages.insert(token, page);
        }
        Ok(ToolDelivery { result, inputs })
    }

    /// The page a `next_page` token names, in the shape of the first.
    pub(crate) fn next_page(&self, token: &str) -> io::Result<ToolDelivery> {
        let (result, inputs) = self.pages.get(token).ok_or_else(|| {
            io::Error::other(format!(
                "unknown page token {token:?}: a token is valid only in the assessment whose paged result returned it; repeat that tool call to page through its result again"
            ))
        })?;
        Ok(ToolDelivery {
            result: result.clone(),
            inputs: inputs.clone(),
        })
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
/// definition are counted, so a page never exceeds its estimate.
fn paginate(
    listing: &Listing,
    values: &[&Value],
    accompanying: &[Vec<Value>],
) -> Vec<std::ops::Range<usize>> {
    let total = values.len();
    let longest = json!({
        "total_entries": total,
        "returned_entries": total,
        "next": "0".repeat(TOKEN_LENGTH),
    });
    let overhead = listing
        .render(&[], &[], &[], Some(longest))
        .to_string()
        .len();
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
    fn all_pages(pages: &ResultPages, first: ToolDelivery) -> Vec<ToolDelivery> {
        let page = &first.result["page"];
        let total = page["total_entries"].as_u64().unwrap();
        let mut delivered = vec![first];
        loop {
            let page = delivered.last().unwrap().result["page"].clone();
            assert_eq!(page["total_entries"], total);
            assert!(page["returned_entries"].as_u64().unwrap() > 0);
            let Some(token) = page["next"].as_str() else {
                assert!(page["next"].is_null());
                break;
            };
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
        let delivered = all_pages(&pages, first);
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
        let delivered = all_pages(&pages, first);
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
    #[test]
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
        let first = dispatch_paged(
            &sources,
            &mut pages,
            "habeas",
            &json!({"file":"src/lib.rs","line_start":1}),
        )
        .unwrap();
        let delivered = all_pages(&pages, first);
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
        let other = ResultPages::default();
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
        let delivered = all_pages(&pages, first);
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
        let delivered = all_pages(&pages, first);
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

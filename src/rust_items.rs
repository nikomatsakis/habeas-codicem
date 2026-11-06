use crate::outline::{self, OutlineError};

/// A non-root node of a Rust file's outline.
// {impl}`requirements.references.rust_item_selectors`
// {impl}`requirements.ci.outline_inputs`
#[derive(Clone, Debug)]
pub(crate) struct RustItem {
    pub(crate) selector: String,
    pub(crate) declaration_line: usize,
    pub(crate) line_start: usize,
    pub(crate) line_end: usize,
    pub(crate) byte_start: usize,
    pub(crate) byte_end: usize,
}

/// Lists a Rust source's outline nodes, other than the root, in source order.
// {impl}`requirements.references.rust_item_selectors`
// {impl}`requirements.ci.outline_inputs`
pub(crate) fn parse_items(source: &str) -> Result<Vec<RustItem>, OutlineError> {
    let outline = outline::parse_with(&outline::RUST_PROFILE, source)?;
    Ok(outline.nodes[1..]
        .iter()
        .map(|node| RustItem {
            selector: node.selector.clone(),
            declaration_line: node.declaration_line,
            line_start: node.line_start,
            line_end: node.line_end,
            byte_start: node.byte_start,
            byte_end: node.byte_end,
        })
        .collect())
}

/// An item's source lines over its full line range.
// {impl}`requirements.ci.outline_inputs`
pub(crate) fn item_lines(source: &str, item: &RustItem) -> Vec<String> {
    source
        .lines()
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .skip(item.line_start - 1)
        .take(item.line_end - item.line_start + 1)
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use super::parse_items;

    // {test}`requirements.references.rust_item_selectors`
    // {test}`requirements.references.selector_nested`
    // {test}`requirements.references.selector_impl_and_trait`
    // {test}`requirements.references.selector_inline_and_foreign`
    // {test}`requirements.references.selector_duplicate_names`
    // {test}`requirements.references.selector_use`
    #[test]
    fn selectors_name_syntactic_ancestry_and_duplicates() {
        let source = r#"
use alpha::A;
use beta::B;
mod local { fn run() {} }
mod external;
struct Thing;
impl Thing { fn run() {} }
impl Trait for Thing { fn run() {} }
impl [u8; 4] { fn edge() {} }
trait Trait { fn read(&self); }
extern "C" { fn puts(); }
fn outer() { fn inner() {} }
#[cfg(a)] fn repeated() {}
#[cfg(b)] fn repeated() {}
"#;
        let selectors = parse_items(source)
            .unwrap()
            .into_iter()
            .map(|item| item.selector)
            .collect::<Vec<_>>();
        assert!(selectors[0].starts_with("use@") && selectors[1].starts_with("use@"));
        assert_ne!(selectors[0], selectors[1]);
        for selector in [
            "mod[local]/fn[run]",
            "mod[external]",
            "struct[Thing]",
            "impl[Thing]/fn[run]",
            "impl[Thing/Trait]/fn[run]",
            "impl[%5Bu8;%204%5D]/fn[edge]",
            "trait[Trait]/fn[read]",
            "extern[C]/fn[puts]",
            "fn[outer]/fn[inner]",
            "fn[repeated]#1",
            "fn[repeated]#2",
        ] {
            assert!(
                selectors.contains(&selector.to_owned()),
                "{selector}: {selectors:?}"
            );
        }
    }

    /// Whether a selector passes through an anonymous segment, which the
    /// outline engine and the legacy collector spell differently.
    // {test}`requirements.references.rust_item_selectors`
    fn anonymous(selector: &str) -> bool {
        selector.contains('@')
            || selector.split('/').any(|segment| {
                segment
                    .split_once('#')
                    .is_some_and(|(base, _)| !base.contains('['))
            })
    }

    // {test}`requirements.references.rust_item_selectors`
    #[test]
    fn outline_profile_reproduces_legacy_selectors_for_this_repository() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut errors = Vec::new();
        let files = crate::rust_coverage::rust_files(root, &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
        assert!(files.len() > 10, "{files:?}");
        let mut mismatches = Vec::new();
        let mut compared = 0;
        for file in files {
            let source = std::fs::read_to_string(&file).unwrap();
            let label = file.strip_prefix(root).unwrap().display();
            let legacy = crate::rust_items_legacy::parse_items(&source)
                .unwrap_or_else(|error| panic!("{label}: {error}"))
                .into_iter()
                .filter(|item| !anonymous(&item.selector))
                .map(|item| {
                    (
                        item.selector,
                        (item.declaration_line, item.line_start, item.line_end),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            let current = parse_items(&source)
                .unwrap_or_else(|error| panic!("{label}: {error}"))
                .into_iter()
                .filter(|item| !anonymous(&item.selector))
                .map(|item| {
                    (
                        item.selector,
                        (item.declaration_line, item.line_start, item.line_end),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            compared += legacy.len();
            for (selector, lines) in &legacy {
                match current.get(selector) {
                    None => mismatches.push(format!("{label}: only legacy has {selector}")),
                    Some(current) if current != lines => mismatches.push(format!(
                        "{label}: {selector} declaration, start, and end lines are {current:?}, legacy {lines:?}"
                    )),
                    Some(_) => {}
                }
            }
            for selector in current.keys().filter(|key| !legacy.contains_key(*key)) {
                mismatches.push(format!("{label}: only the outline has {selector}"));
            }
        }
        assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
        assert!(compared > 500, "only {compared} selectors compared");
    }
}

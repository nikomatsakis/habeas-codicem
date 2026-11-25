//! Source ownership queries over the SDK's public outline.
use std::ops::{Range, RangeInclusive};
use trust_but_verify::outline::{AttachmentKind, Outline};

pub(crate) struct Comment {
    pub(crate) owner: usize,
    pub(crate) span: Range<usize>,
    pub(crate) lines: RangeInclusive<usize>,
}

pub(crate) struct OutlineItems {
    pub(crate) diagnostic: Option<trust_but_verify::outline::OutlineError>,
    pub(crate) comments: Vec<Comment>,
    parents: Vec<Option<usize>>,
    children: Vec<Vec<usize>>,
    spans: Vec<Range<usize>>,
    line_starts: Vec<usize>,
}

impl OutlineItems {
    // {impl}`requirements.references.outline_comments`
    pub(crate) fn new(outline: &Outline, source: &str) -> Self {
        let line_starts: Vec<_> = std::iter::once(0)
            .chain(source.match_indices('\n').map(|(offset, _)| offset + 1))
            .collect();
        let line = |byte| line_starts.partition_point(|start| *start <= byte);
        let mut comments = Vec::new();
        let mut ancestors: Vec<usize> = Vec::new();
        let mut parents = Vec::new();
        let mut children = vec![Vec::new(); outline.items.len()];
        let mut spans = Vec::new();
        for (index, item) in outline.items.iter().enumerate() {
            while let Some(&previous) = ancestors.last() {
                let selector = &outline.items[previous].selector;
                if selector.is_empty()
                    || item
                        .selector
                        .strip_prefix(selector)
                        .is_some_and(|rest| rest.starts_with('/'))
                {
                    break;
                }
                ancestors.pop();
            }
            let parent = ancestors.last().copied();
            if let Some(parent) = parent {
                children[parent].push(index);
            }
            parents.push(parent);
            ancestors.push(index);
            spans.push(item.span.start..item.span.end);
            for attachment in &item.attachments {
                if attachment.kind == AttachmentKind::Comment {
                    let span = &attachment.span;
                    comments.push(Comment {
                        owner: index,
                        span: span.start..span.end,
                        lines: line(span.start)..=line(span.end.saturating_sub(1).max(span.start)),
                    });
                }
            }
        }
        comments.sort_by_key(|comment| comment.span.start);
        Self {
            diagnostic: outline.diagnostic.clone(),
            comments,
            line_starts,
            parents,
            children,
            spans,
        }
    }

    pub(crate) fn line_at(&self, byte: usize) -> usize {
        self.line_starts.partition_point(|start| *start <= byte)
    }

    pub(crate) fn children(&self, item: usize) -> &[usize] {
        &self.children[item]
    }

    pub(crate) fn parent(&self, item: usize) -> Option<usize> {
        self.parents[item]
    }

    // {impl}`requirements.references.distributed`
    pub(crate) fn owner(&self, range: &Range<usize>) -> usize {
        let contains = |span: &Range<usize>| span.start <= range.start && range.end <= span.end;
        if let Some(comment) = self.comments.iter().find(|comment| contains(&comment.span)) {
            return comment.owner;
        }
        let mut item = 0;
        while let Some(&child) = self.children[item]
            .iter()
            .find(|&&child| contains(&self.spans[child]))
        {
            item = child;
        }
        item
    }
}

pub(crate) fn for_file(
    cache: &crate::inputs::SourceCache,
    path: &std::path::Path,
    source: &str,
) -> Result<OutlineItems, String> {
    let contents =
        trust_but_verify::outline::OutlineContent::from_source(cache, path, source.into())
            .map_err(|error| error.to_string())?;
    Ok(OutlineItems::new(contents.outline(), source))
}

/// Evidence addresses and inclusive source lines used by Habeas's line-based tools.
/// Selectors, hashes and attachment ownership originate in the SDK outline.
pub(crate) struct EvidenceOutline {
    pub(crate) ownership: OutlineItems,
    pub(crate) nodes: Vec<EvidenceItem>,
    pub(crate) language: &'static str,
}

pub(crate) struct EvidenceItem {
    pub(crate) selector: String,
    pub(crate) kind: String,
    pub(crate) structural_hash: String,
    pub(crate) line_start: usize,
    pub(crate) line_end: usize,
    pub(crate) declaration_line: usize,
    pub(crate) byte_start: usize,
    pub(crate) byte_end: usize,
    pub(crate) source_start: usize,
    pub(crate) parent: Option<usize>,
}

/// Display the decoded name parts of the last segment in an SDK selector.
// {impl}`requirements.authoring.outline_tool`
pub(crate) fn display_name(selector: &str) -> Option<String> {
    use trust_but_verify::outline::selector::{Label, parse_selector};
    match parse_selector(selector)
        .expect("validated outline selector")
        .pop()?
        .label
    {
        Label::Named(parts) => Some(parts.join("/")),
        Label::Hash(_) => None,
    }
}

impl EvidenceOutline {
    pub(crate) fn new(
        public: &Outline,
        items: OutlineItems,
        language: &'static str,
        legacy: Option<&crate::outline::Outline>,
    ) -> Self {
        let nodes = public
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let mut start = item.span.start;
                let mut end = item.span.end.saturating_sub(1).max(start);
                for attachment in &item.attachments {
                    start = start.min(attachment.span.start);
                    end = end.max(attachment.span.end.saturating_sub(1));
                }
                let core = item.header_span.map_or(item.span.start, |span| span.start);
                let old = legacy.map(|outline| &outline.nodes[index]);
                EvidenceItem {
                    selector: old.map_or(&item.selector, |n| &n.selector).clone(),
                    structural_hash: old
                        .map_or(&item.structural_hash, |n| &n.structural_hash)
                        .clone(),
                    kind: item.kind.clone(),
                    line_start: old.map_or_else(|| items.line_at(start), |n| n.line_start),
                    line_end: old.map_or_else(|| items.line_at(end), |n| n.line_end),
                    declaration_line: old.map_or_else(
                        || items.line_at(item.name_span.map_or(core, |s| s.start)),
                        |n| n.declaration_line,
                    ),
                    byte_start: old.map_or(core, |n| n.byte_start),
                    byte_end: item.span.end,
                    source_start: item.span.start,
                    parent: items.parent(index),
                }
            })
            .collect();
        Self {
            nodes,
            language,
            ownership: items,
        }
    }

    pub(crate) fn resolve(&self, selector: &str) -> Result<usize, String> {
        use trust_but_verify::outline::selector::parse_selector;
        let normalized = parse_selector(selector)
            .map_err(|e| e.to_string())?
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("/");
        self.nodes
            .iter()
            .position(|node| node.selector == normalized)
            .ok_or_else(|| {
                let first = format!("{normalized}#1");
                if self.nodes.iter().any(|n| n.selector == first) {
                    format!("outline selector {selector:?} is ambiguous; add a #N disambiguator")
                } else {
                    format!("no outline node matches {selector:?}")
                }
            })
    }

    fn innermost(&self, line: usize, comments: bool) -> Vec<usize> {
        let items = &self.ownership;
        if comments {
            let owners: Vec<_> = items
                .comments
                .iter()
                .filter(|c| c.lines.contains(&line))
                .map(|c| c.owner)
                .collect();
            if !owners.is_empty() {
                return owners;
            }
        }
        let covers = |i: usize| (self.nodes[i].line_start..=self.nodes[i].line_end).contains(&line);
        let mut stack = vec![0];
        let mut found = Vec::new();
        while let Some(index) = stack.pop() {
            if !covers(index) {
                continue;
            }
            let pending = stack.len();
            stack.extend(
                items
                    .children(index)
                    .iter()
                    .rev()
                    .copied()
                    .filter(|&i| covers(i)),
            );
            if stack.len() == pending {
                found.push(index);
            }
        }
        found
    }

    // {impl}`requirements.references.outline_comments`
    pub(crate) fn owner_at_line(&self, line: usize) -> usize {
        let items = &self.ownership;
        if let Some(c) = items.comments.iter().find(|c| c.lines.contains(&line)) {
            return c.owner;
        }
        self.innermost(line, false)
            .into_iter()
            .min_by_key(|&i| {
                let n = &self.nodes[i];
                (n.line_end - n.line_start, n.byte_end - n.byte_start)
            })
            .unwrap_or(0)
    }

    // {impl}`requirements.ci.outline_inputs`
    pub(crate) fn nodes_for_lines(
        &self,
        lines: impl IntoIterator<Item = usize>,
        comments: bool,
    ) -> Vec<usize> {
        let selected: std::collections::BTreeSet<_> = lines
            .into_iter()
            .flat_map(|line| self.innermost(line, comments))
            .collect();
        selected
            .iter()
            .copied()
            .filter(|&node| {
                !std::iter::successors(self.ownership.parent(node), |&i| self.ownership.parent(i))
                    .any(|i| selected.contains(&i))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use trust_but_verify::outline::{OutlineLang, languages::Rust};

    // {test}`requirements.references.distributed`
    // {test}`requirements.references.outline_comments`
    #[test]
    fn search_ranges_resolve_to_comment_owners_or_the_common_enclosing_item() {
        let source = "mod m {\n    fn a() { inner(); } // tail\n    fn b() {}\n}\n";
        let public = Rust.outline(source).unwrap();
        let items = OutlineItems::new(&public, source);
        for (matched, expected) in [
            ("inner()", "mod[m]/fn[a]"),
            ("tail", "mod[m]/fn[a]"),
            ("} // tail\n    fn b", "mod[m]"),
            (source, ""),
        ] {
            let start = source.find(matched).unwrap();
            assert_eq!(
                public.items[items.owner(&(start..start + matched.len()))].selector,
                expected
            );
        }
    }
}

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
            spans.push(item.span.clone());
            for attachment in &item.attachments {
                if attachment.kind == AttachmentKind::Comment {
                    let span = &attachment.span;
                    comments.push(Comment {
                        owner: index,
                        span: span.clone(),
                        lines: line(span.start)..=line(span.end.saturating_sub(1).max(span.start)),
                    });
                }
            }
        }
        comments.sort_by_key(|comment| comment.span.start);
        Self {
            diagnostic: outline.diagnostic.clone(),
            comments,
            parents,
            children,
            spans,
        }
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

pub(crate) fn for_file(path: &std::path::Path, source: &str) -> Result<OutlineItems, String> {
    let snapshot = trust_but_verify::outline::OutlineContent::from_source(path, source.into())
        .map_err(|error| error.to_string())?;
    Ok(OutlineItems::new(snapshot.outline(), source))
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

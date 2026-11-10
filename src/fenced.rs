//! Fenced references: `:::{role} name` on one line, a body of commentary, and
//! a closing colon fence, recognized in Markdown and plain text and, in other
//! supported languages, inside one comment run after removing each line's
//! comment delimiters.

use std::ops::Range;

use crate::outline::Outline;
use crate::{ReferenceKind, colon_fence, overlaps_protected, protected_ranges};

/// One line in which fences may be recognized, with its one-based source line.
// {cite}`habeas:util`
pub(crate) struct FenceLine<'a> {
    pub(crate) text: &'a str,
    pub(crate) line: usize,
    /// Whether the line lies in Markdown code or raw HTML, where fences
    /// remain literal.
    pub(crate) protected: bool,
}

/// A fenced reference found among [`FenceLine`]s, by line index.
// {cite}`habeas:util`
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Fence {
    pub(crate) kind: ReferenceKind,
    pub(crate) name: String,
    pub(crate) open: usize,
    /// Index of the closing fence line; `None` when the fence never closes.
    pub(crate) close: Option<usize>,
}

/// A fenced reference with its body, as found in a whole source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceFence {
    pub(crate) kind: ReferenceKind,
    pub(crate) name: String,
    /// One-based line of the opening fence.
    pub(crate) line: usize,
    /// The body without comment delimiters, trimmed.
    pub(crate) body: String,
    pub(crate) closed: bool,
    /// Owning outline node of the comment run, for fences in comments.
    pub(crate) owner: Option<usize>,
}

/// Parses an opening reference fence: at least three colons, a role, and one
/// name, which is a valid definition name or a built-in name. A `{fixme}`
/// fence may omit its name, which is then empty.
pub(crate) fn opening_fence(line: &str) -> Option<(ReferenceKind, &str, usize)> {
    let (suffix, length) = colon_fence(line)?;
    let (kind, rest) = if let Some(rest) = suffix.strip_prefix("{impl}") {
        (ReferenceKind::Impl, rest)
    } else if let Some(rest) = suffix.strip_prefix("{cite}") {
        (ReferenceKind::Cite, rest)
    } else if let Some(rest) = suffix.strip_prefix("{fixme}") {
        if rest.trim().is_empty() {
            return Some((ReferenceKind::Fixme, "", length));
        }
        (ReferenceKind::Fixme, rest)
    } else {
        (ReferenceKind::Test, suffix.strip_prefix("{test}")?)
    };
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let name = rest.trim();
    (crate::valid_name(name) || crate::builtins::is_builtin_name(name))
        .then_some((kind, name, length))
}

/// Whether a line closes a fence opened with `length` colons.
fn closes(line: &str, length: usize) -> bool {
    colon_fence(line).is_some_and(|(suffix, count)| count >= length && suffix.trim().is_empty())
}

/// Finds fenced references among lines. A fence closes at the next
/// unprotected colon-only line with at least as many colons; fences may nest.
// {impl}`requirements.ci.fence_lengths`
pub(crate) fn find_fences(lines: &[FenceLine<'_>]) -> Vec<Fence> {
    let mut fences = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if line.protected {
            continue;
        }
        let Some((kind, name, length)) = opening_fence(line.text) else {
            continue;
        };
        let close = (index + 1..lines.len()).find(|&candidate| {
            !lines[candidate].protected && closes(lines[candidate].text, length)
        });
        fences.push(Fence {
            kind,
            name: name.to_owned(),
            open: index,
            close,
        });
    }
    fences
}

/// The body lines of a fence, joined and trimmed.
fn body(lines: &[FenceLine<'_>], fence: &Fence) -> String {
    let end = fence.close.unwrap_or(lines.len());
    lines[fence.open + 1..end]
        .iter()
        .map(|line| line.text)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

/// A fenced reference in Markdown, with byte offsets for rendering.
pub(crate) struct MarkdownFence {
    pub(crate) kind: ReferenceKind,
    pub(crate) name: String,
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) body: Range<usize>,
    pub(crate) closed: bool,
    pub(crate) line: usize,
}

/// Splits text into lines with their byte ranges, without line endings.
// {cite}`habeas:util`
fn split_lines(content: &str) -> Vec<(usize, usize, &str)> {
    let mut lines = Vec::new();
    let mut position = 0;
    for segment in content.split_inclusive('\n') {
        let end = position + segment.len();
        lines.push((
            position,
            end,
            segment.trim_end_matches('\n').trim_end_matches('\r'),
        ));
        position = end;
    }
    lines
}

/// Fenced references in Markdown text; fences in code or raw HTML remain
/// literal. Without `markdown`, every line is text, as in a plain-text file.
// {impl}`requirements.references.markdown_literals`
pub(crate) fn markdown_fences(content: &str, markdown: bool) -> Vec<MarkdownFence> {
    let protected = if markdown {
        protected_ranges(content)
    } else {
        Vec::new()
    };
    let raw = split_lines(content);
    let lines = raw
        .iter()
        .enumerate()
        .map(|(index, (start, end, text))| FenceLine {
            text,
            line: index + 1,
            protected: overlaps_protected(*start, (*end).max(start + 1), &protected),
        })
        .collect::<Vec<_>>();
    find_fences(&lines)
        .into_iter()
        .map(|fence| {
            let body_start = raw[fence.open].1;
            let (body_end, end) = match fence.close {
                Some(close) => (raw[close].0, raw[close].1),
                None => (content.len(), content.len()),
            };
            MarkdownFence {
                kind: fence.kind,
                name: fence.name,
                start: raw[fence.open].0,
                end,
                body: body_start.min(body_end)..body_end,
                closed: fence.close.is_some(),
                line: fence.open + 1,
            }
        })
        .collect()
}

/// Removes one line's comment delimiters: leading `//`, `///`, `//!`, `/*`,
/// `/**`, `/*!`, `#`, `<!--`, or a block comment's leading `*`, and a
/// trailing `*/` or `-->`.
// {cite}`habeas:util`
pub(crate) fn strip_comment_delimiters(line: &str) -> &str {
    let mut text = line.trim_start();
    for prefix in ["///", "//!", "//", "/**", "/*!", "/*", "<!--", "#"] {
        if let Some(rest) = text.strip_prefix(prefix) {
            text = rest;
            break;
        }
    }
    if !text.starts_with("*/")
        && let Some(rest) = text.strip_prefix('*')
    {
        text = rest;
    }
    let trimmed = text.trim_end();
    for suffix in ["*/", "-->"] {
        if let Some(rest) = trimmed.strip_suffix(suffix) {
            return rest;
        }
    }
    text
}

/// Fenced references in the comment runs of a parsed source. Each fence must
/// open and close within one run.
pub(crate) fn comment_fences(source: &str, outline: &Outline) -> Vec<SourceFence> {
    let mut fences = Vec::new();
    let mut start = 0;
    while start < outline.comments.len() {
        let run = outline.comments[start].run;
        let end = (start..outline.comments.len())
            .find(|&index| outline.comments[index].run != run)
            .unwrap_or(outline.comments.len());
        let mut lines = Vec::new();
        for comment in &outline.comments[start..end] {
            let text = source
                .get(comment.byte_start..comment.byte_end)
                .unwrap_or("");
            for (offset, line) in text.lines().enumerate() {
                lines.push(FenceLine {
                    text: strip_comment_delimiters(line.strip_suffix('\r').unwrap_or(line)),
                    line: comment.line_start + offset,
                    protected: false,
                });
            }
        }
        let owner = outline.comments[start].owner;
        for fence in find_fences(&lines) {
            fences.push(SourceFence {
                body: body(&lines, &fence),
                kind: fence.kind,
                line: lines[fence.open].line,
                name: fence.name,
                closed: fence.close.is_some(),
                owner: Some(owner),
            });
        }
        start = end;
    }
    fences
}

/// Fenced references in a whole Markdown or plain-text source.
// {cite}`habeas:util`
pub(crate) fn text_fences(content: &str, markdown: bool) -> Vec<SourceFence> {
    markdown_fences(content, markdown)
        .into_iter()
        .map(|fence| SourceFence {
            kind: fence.kind,
            name: fence.name,
            line: fence.line,
            body: content[fence.body].trim().to_owned(),
            closed: fence.closed,
            owner: None,
        })
        .collect()
}

/// Fenced references in a source file: in Markdown text, in every line of a
/// plain-text file, or in the comment runs of another supported language. A
/// source its grammar cannot parse has no recognized fences; its parse
/// failure is reported by the checks that outline it.
// {impl}`requirements.references.roles`
pub(crate) fn source_fences(path: &std::path::Path, content: &str) -> Vec<SourceFence> {
    if !content.contains(":::") {
        return Vec::new();
    }
    if crate::outline::is_markdown_path(path) {
        return text_fences(content, true);
    }
    let profile = crate::outline::profile_for_path(path);
    if profile.language.is_none() {
        return text_fences(content, false);
    }
    crate::outline::parse_with(profile, content)
        .map(|outline| comment_fences(content, &outline))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    // {test}`requirements.references.roles`
    // {test}`requirements.ci.identifiers`
    // {test}`requirements.references.builtin_definitions`
    #[test]
    fn opening_fences_take_a_role_and_one_valid_or_built_in_name() {
        let role = |name: &str| format!(":::{{{name}}}");
        assert_eq!(
            opening_fence(&format!("{} requirements.ready", role("impl"))),
            Some((ReferenceKind::Impl, "requirements.ready", 3))
        );
        assert_eq!(
            opening_fence("::::{cite} habeas:util"),
            Some((ReferenceKind::Cite, "habeas:util", 4))
        );
        // A syntactically built-in name is recognized so validation can report
        // an unknown built-in rather than leaving it literal.
        assert_eq!(
            opening_fence(&format!("{} habeas:unknown", role("impl"))),
            Some((ReferenceKind::Impl, "habeas:unknown", 3))
        );
        assert_eq!(
            opening_fence(&format!("   {} a.b", role("test"))),
            Some((ReferenceKind::Test, "a.b", 3))
        );
        for literal in [
            format!("    {} a.b", role("test")),
            format!("{}a.b", role("cite")),
            format!("{} a b", role("cite")),
            format!("{} a..b", role("cite")),
            format!("{} .a", role("cite")),
            format!("{} a.", role("cite")),
            format!("{} a/b", role("impl")),
            format!("{} caf\u{e9}.b", role("impl")),
            format!("{} other:none", role("cite")),
            format!("{} a.b", role("term")),
            "::{cite} a.b".to_owned(),
        ] {
            assert_eq!(opening_fence(&literal), None, "{literal}");
        }
    }

    // {test}`requirements.references.roles`
    // {test}`requirements.ci.fence_lengths`
    #[test]
    fn markdown_fences_close_with_enough_colons_and_skip_code() {
        let content = "::::{cite} a.b\nouter\n:::{impl} c.d\ninner\n:::\n::::\n```text\n:::{cite} e.f\n```\n:::{test} g.h\nnever closed\n".to_owned();
        let fences = markdown_fences(&content, true);
        assert_eq!(fences.len(), 3);
        assert_eq!(fences[0].name, "a.b");
        assert!(fences[0].closed);
        assert_eq!(
            &content[fences[0].body.clone()],
            "outer\n:::{impl} c.d\ninner\n:::\n"
        );
        assert_eq!(fences[1].name, "c.d");
        assert_eq!(&content[fences[1].body.clone()], "inner\n");
        assert_eq!(fences[2].name, "g.h");
        assert!(!fences[2].closed);
        assert_eq!(fences[2].line, 10);
        // In plain text, the code fence is ordinary text.
        assert_eq!(markdown_fences(&content, false).len(), 4);
    }

    // {test}`requirements.references.roles`
    #[test]
    fn comment_delimiters_are_removed_per_line() {
        assert_eq!(
            strip_comment_delimiters("    // :::{cite} a"),
            " :::{cite} a"
        );
        assert_eq!(strip_comment_delimiters("/// body"), " body");
        assert_eq!(strip_comment_delimiters("//! body"), " body");
        assert_eq!(strip_comment_delimiters("# body"), " body");
        assert_eq!(strip_comment_delimiters("/* body */"), " body ");
        assert_eq!(strip_comment_delimiters("   * body"), " body");
        assert_eq!(strip_comment_delimiters("   ::: */"), "::: ");
        assert_eq!(strip_comment_delimiters("<!-- ::: -->"), " ::: ");
    }

    // {test}`requirements.references.roles`
    // {test}`requirements.references.outline_comments`
    #[test]
    fn comment_fences_close_within_one_comment_run() {
        let source = "// :::{cite} habeas:util\n// Exists for its caller.\n// :::\nfn a() {}\n\n// :::{cite} habeas:util\n// never closed here\n\n// :::\nfn b() {}\n/*\n * :::{impl} requirements.ready\n * Checks it.\n * :::\n */\nfn c() {}\n".to_owned();
        let outline = crate::outline::parse(std::path::Path::new("lib.rs"), &source).unwrap();
        let fences = comment_fences(&source, &outline);
        assert_eq!(fences.len(), 3, "{fences:?}");
        assert_eq!(fences[0].body, "Exists for its caller.");
        assert!(fences[0].closed);
        assert_eq!(fences[0].line, 1);
        assert_eq!(outline.nodes[fences[0].owner.unwrap()].selector, "fn[a]");
        assert!(!fences[1].closed, "a later run's fence does not close it");
        assert_eq!(fences[1].line, 6);
        assert_eq!(fences[2].kind, ReferenceKind::Impl);
        assert_eq!(fences[2].body, "Checks it.");
        assert_eq!(outline.nodes[fences[2].owner.unwrap()].selector, "fn[c]");
    }
}

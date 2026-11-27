//! Fixed credential redaction for source and model-visible text.
//!
//! Redaction locates byte regions of the original source and replaces each
//! with `[REDACTED:<kind>]` followed by the region's line breaks, so line
//! numbers never change. Outlines are built from the original source; their
//! hashes use [`Redaction::slice`] for each text piece.

use std::borrow::Cow;
use std::ops::Range;
use std::sync::OnceLock;

use secretshape::Scanner;

/// The rule name `secretshape` gives findings of its entropy heuristic.
const HEURISTIC: &str = "high-entropy-token";

/// One replaced byte range of a source and the kind its marker names.
#[derive(Clone, Debug, PartialEq, Eq)]
// {cite}`habeas:util`
struct Region {
    pub span: Range<usize>,
    pub kind: String,
}

/// The redacted regions of one source text: sorted, disjoint, and nonempty.
#[derive(Clone, Debug, Default)]
// {cite}`habeas:util`
pub struct Redaction {
    regions: Vec<Region>,
}

impl Redaction {
    /// Locate recognized credentials using the SDK's fixed rules.
    // {impl}`requirements.sdk.sources.redaction`
    // {cite}`habeas:util`
    pub fn of_text(source: &str) -> Self {
        Self::merge(secret_regions(source))
    }

    /// Sorts regions and joins overlapping ones under the first one's kind.
    // {cite}`habeas:util`
    fn merge(mut regions: Vec<Region>) -> Self {
        regions.retain(|region| !region.span.is_empty());
        regions.sort_by_key(|region| (region.span.start, region.span.end));
        let mut merged: Vec<Region> = Vec::with_capacity(regions.len());
        for region in regions {
            match merged.last_mut() {
                Some(last) if region.span.start < last.span.end => {
                    last.span.end = last.span.end.max(region.span.end);
                }
                _ => merged.push(region),
            }
        }
        Self { regions: merged }
    }

    // {cite}`habeas:util`
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    /// The redacted form of `source[range]`. A region contributes its marker
    /// to the slice containing the region's first byte and its line breaks
    /// to every slice it overlaps, so slices of adjacent ranges concatenate
    /// to the redacted form of their union.
    // {cite}`habeas:util`
    // {impl}`requirements.authoring.redaction_notice`
    pub fn slice<'a>(&self, source: &'a str, range: Range<usize>) -> Cow<'a, str> {
        let first = self
            .regions
            .partition_point(|region| region.span.end <= range.start);
        let mut output: Option<String> = None;
        let mut cursor = range.start;
        for region in self.regions[first..]
            .iter()
            .take_while(|region| region.span.start < range.end)
        {
            let output = output.get_or_insert_with(String::new);
            let start = region.span.start.max(range.start);
            let end = region.span.end.min(range.end);
            output.push_str(&source[cursor..start]);
            if region.span.start >= range.start {
                output.push_str("[REDACTED:");
                output.push_str(&region.kind);
                output.push(']');
            }
            push_line_breaks(output, source, start..end);
            cursor = end;
        }
        match output {
            None => Cow::Borrowed(&source[range]),
            Some(mut output) => {
                output.push_str(&source[cursor..range.end]);
                Cow::Owned(output)
            }
        }
    }

    /// The redacted form of the whole source.
    // {cite}`habeas:util`
    pub fn apply<'a>(&self, source: &'a str) -> Cow<'a, str> {
        self.slice(source, 0..source.len())
    }
}

/// Preserve each line-break byte, including when the range splits a CRLF pair.
// {cite}`habeas:util`
fn push_line_breaks(output: &mut String, source: &str, range: Range<usize>) {
    let bytes = source.as_bytes();
    for index in range {
        match bytes[index] {
            b'\n' => output.push('\n'),
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => output.push('\r'),
            _ => {}
        }
    }
}

/// Rule-only and heuristic scanners over the whole input; `secretshape`
/// otherwise stops scanning after its default input cap.
// {cite}`habeas:util`
fn scanners() -> &'static (Scanner, Scanner) {
    static SCANNERS: OnceLock<(Scanner, Scanner)> = OnceLock::new();
    SCANNERS.get_or_init(|| {
        (
            Scanner::new().max_input_bytes(usize::MAX),
            Scanner::new()
                .include_heuristics(true)
                .max_input_bytes(usize::MAX),
        )
    })
}

/// Regions of `secretshape` rule matches, each extended through adjacent
/// heuristic findings separated from it only by whitespace. A heuristic
/// finding never starts a region.
// {cite}`habeas:util`
fn secret_regions(source: &str) -> Vec<Region> {
    let (rules, heuristics) = scanners();
    let findings = rules.scan(source);
    if findings.is_empty() {
        return Vec::new();
    }
    let flagged: Vec<Range<usize>> = heuristics
        .scan(source)
        .into_iter()
        .filter(|finding| finding.rule == HEURISTIC)
        .map(|finding| finding.span)
        .collect();
    let whitespace = |range: Range<usize>| source[range].chars().all(char::is_whitespace);
    let mut regions = Redaction::merge(
        findings
            .into_iter()
            .map(|finding| Region {
                span: finding.span,
                kind: finding.rule.into_owned(),
            })
            .collect(),
    )
    .regions;
    for region in &mut regions {
        // Flagged tokens are disjoint and sorted, so one pass in each
        // direction reaches the fixpoint.
        let mut next = flagged.partition_point(|span| span.end <= region.span.start);
        while let Some(span) = flagged.get(next) {
            if span.start < region.span.end || whitespace(region.span.end..span.start) {
                region.span.end = region.span.end.max(span.end);
                next += 1;
            } else {
                break;
            }
        }
        let mut previous = flagged.partition_point(|span| span.start < region.span.start);
        while previous > 0 {
            let span = &flagged[previous - 1];
            if span.end > region.span.start || whitespace(span.end..region.span.start) {
                region.span.start = span.start;
                previous -= 1;
            } else {
                break;
            }
        }
    }
    regions
}

#[cfg(test)]
pub(crate) mod tests;

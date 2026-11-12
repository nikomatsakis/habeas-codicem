//! In-process, case-sensitive regular-expression search.
use std::io;
use std::path::Path;

// {impl}`requirements.authoring.source_tools`
pub struct RegexSearch {
    matcher: grep_regex::RegexMatcher,
}

impl RegexSearch {
    // {impl}`requirements.authoring.source_tools`
    pub fn compile(pattern: &str) -> io::Result<Self> {
        let matcher = grep_regex::RegexMatcherBuilder::new()
            .case_insensitive(false)
            .build(pattern)
            .map_err(|error| io::Error::other(format!("invalid ripgrep pattern: {error}")))?;
        Ok(Self { matcher })
    }

    /// Matching one-based line numbers, stopping at binary data.
    // {impl}`requirements.authoring.source_tools`
    pub fn search_path(&self, path: &Path) -> io::Result<Vec<usize>> {
        let mut searcher = grep_searcher::SearcherBuilder::new()
            .line_number(true)
            .binary_detection(grep_searcher::BinaryDetection::quit(0))
            .build();
        let mut lines = Vec::new();
        searcher
            .search_path(
                &self.matcher,
                path,
                grep_searcher::sinks::Lossy(|number, _| {
                    lines.push(number as usize);
                    Ok(true)
                }),
            )
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("ripgrep failed to search {}: {error}", path.display()),
                )
            })?;
        Ok(lines)
    }
}

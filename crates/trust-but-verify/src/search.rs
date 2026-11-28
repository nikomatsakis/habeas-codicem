//! In-process, case-sensitive regular-expression search.
use std::io;

pub struct RegexSearch {
    matcher: grep_regex::RegexMatcher,
}

impl RegexSearch {
    pub fn compile(pattern: &str) -> io::Result<Self> {
        let matcher = grep_regex::RegexMatcherBuilder::new()
            .case_insensitive(false)
            .build(pattern)
            .map_err(|error| io::Error::other(format!("invalid ripgrep pattern: {error}")))?;
        Ok(Self { matcher })
    }

    /// Search a provider's supplied bytes without requiring a local path.
    // {cite}`habeas:util`
    pub fn search_bytes(&self, bytes: &[u8]) -> io::Result<Vec<usize>> {
        let mut searcher = grep_searcher::SearcherBuilder::new()
            .line_number(true)
            .binary_detection(grep_searcher::BinaryDetection::quit(0))
            .build();
        let mut lines = Vec::new();
        searcher.search_slice(
            &self.matcher,
            bytes,
            grep_searcher::sinks::Lossy(|number, _| {
                lines.push(number as usize);
                Ok(true)
            }),
        )?;
        Ok(lines)
    }
}

#[cfg(test)]
mod tests {
    use super::RegexSearch;

    // {test}`requirements.authoring.source_tools`
    #[test]
    fn byte_search_reports_case_sensitive_one_based_matching_lines() {
        let search = RegexSearch::compile("hit").unwrap();
        assert_eq!(
            search
                .search_bytes("Hit\nhit hit\nabsent\né hit\n".as_bytes())
                .unwrap(),
            [2, 4]
        );
        assert!(search.search_bytes(b"\0hit\nhit\n").unwrap().is_empty());
        assert!(RegexSearch::compile("[").is_err());
    }
}

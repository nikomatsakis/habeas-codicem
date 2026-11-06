use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;

use crate::escape_html;

// {cite}`requirements.cli.fixme_summary`
#[derive(Default)]
pub(crate) struct FixmeSummary {
    judgments: Vec<FixmeJudgment>,
}

// {cite}`requirements.cli.fixme_summary`
struct FixmeJudgment {
    name: String,
    kind: &'static str,
    entries: Vec<String>,
}

impl FixmeSummary {
    // {impl}`requirements.cli.fixme_summary`
    pub(crate) fn add(&mut self, name: &str, kind: &'static str, entries: &[String]) {
        if !entries.is_empty() {
            self.judgments.push(FixmeJudgment {
                name: name.to_owned(),
                kind,
                entries: entries.to_vec(),
            });
        }
    }

    // {impl}`requirements.cli.fixme_summary`
    pub(crate) fn render(&self, verbose: bool) -> String {
        if self.judgments.is_empty() {
            return String::new();
        }
        let mut groups: BTreeMap<String, Vec<&FixmeJudgment>> = BTreeMap::new();
        let mut without_issue = Vec::new();
        for judgment in &self.judgments {
            let issues = issue_references(&judgment.entries);
            if issues.is_empty() {
                without_issue.push(judgment);
            }
            for issue in issues {
                groups.entry(issue).or_default().push(judgment);
            }
        }
        let mut groups = groups.into_iter().collect::<Vec<_>>();
        groups.sort_by(|(a, left), (b, right)| right.len().cmp(&left.len()).then(a.cmp(b)));

        let mut output = String::from("\n## FIXME judgments\n\n");
        for (issue, judgments) in &groups {
            output.push_str(&format!(
                "- `{issue}` — affects {} {}\n",
                judgments.len(),
                judgment_word(judgments.len())
            ));
        }
        if !without_issue.is_empty() {
            output.push_str(&format!(
                "- {} {} marked FIXME without an issue\n",
                without_issue.len(),
                if without_issue.len() == 1 {
                    "judgment is"
                } else {
                    "judgments are"
                }
            ));
        }
        if verbose {
            for (issue, judgments) in groups {
                output.push_str(&format!("\n### `{issue}`\n\n"));
                render_judgments(&mut output, &judgments);
            }
            if !without_issue.is_empty() {
                output.push_str("\n### FIXME without an issue\n\n");
                render_judgments(&mut output, &without_issue);
            }
        }
        output
    }
}

// {impl}`requirements.cli.fixme_summary`
fn judgment_word(count: usize) -> &'static str {
    if count == 1 { "judgment" } else { "judgments" }
}

// {impl}`requirements.cli.fixme_summary`
fn render_judgments(output: &mut String, judgments: &[&FixmeJudgment]) {
    for judgment in judgments {
        output.push_str(&format!("- `{}` ({})\n\n", judgment.name, judgment.kind));
        output.push_str("<pre><code>");
        output.push_str(&escape_html(&judgment.entries.join("\n")));
        output.push_str("</code></pre>\n\n");
    }
}

// {impl}`requirements.cli.fixme_summary`
fn issue_references(entries: &[String]) -> BTreeSet<String> {
    static ISSUES: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:https?://github\.com/(?P<url_repo>[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)/issues/|(?P<repo>[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)?#)(?P<number>[0-9]+)")
            .expect("valid issue-reference pattern")
    });
    let mut issues = BTreeSet::new();
    for entry in entries {
        for found in ISSUES.captures_iter(entry) {
            let matched = found.get(0).expect("complete match");
            let before = entry[..matched.start()].chars().next_back();
            let after = entry[matched.end()..].chars().next();
            if before.is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '/' | '#'))
                || after.is_some_and(|c| c.is_alphanumeric() || c == '_')
            {
                continue;
            }
            let number = found["number"].trim_start_matches('0');
            if number.is_empty() {
                continue;
            }
            let repository = found.name("url_repo").or_else(|| found.name("repo"));
            issues.insert(format!(
                "{}#{number}",
                repository.map_or("", |m| m.as_str())
            ));
        }
    }
    issues
}

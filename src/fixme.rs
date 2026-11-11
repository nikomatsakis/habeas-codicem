use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;

use crate::escape_html;

#[derive(Default)]
pub(crate) struct FixmeSummary {
    judgments: Vec<FixmeJudgment>,
}

struct FixmeJudgment {
    name: String,
    kind: &'static str,
    entries: Vec<String>,
}

impl FixmeSummary {
    pub(crate) fn add(&mut self, name: &str, kind: &'static str, entries: &[String]) {
        if !entries.is_empty() {
            self.judgments.push(FixmeJudgment {
                name: name.to_owned(),
                kind,
                entries: entries.to_vec(),
            });
        }
    }

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

fn judgment_word(count: usize) -> &'static str {
    if count == 1 { "judgment" } else { "judgments" }
}

fn render_judgments(output: &mut String, judgments: &[&FixmeJudgment]) {
    for judgment in judgments {
        output.push_str(&format!("- `{}` ({})\n\n", judgment.name, judgment.kind));
        output.push_str("<pre><code>");
        output.push_str(&escape_html(&judgment.entries.join("\n")));
        output.push_str("</code></pre>\n\n");
    }
}

/// The issues a judgment's FIXME strings name: each string that is exactly
/// an `owner/repo#N` issue reference. Any other string describes a gap
/// without an issue.
fn issue_references(entries: &[String]) -> BTreeSet<String> {
    entries
        .iter()
        .filter(|entry| is_issue_reference(entry))
        .cloned()
        .collect()
}

/// Whether `value` is a GitHub issue reference `<owner>/<repo>#<number>`,
/// with a positive number written without leading zeros.
pub(crate) fn is_issue_reference(value: &str) -> bool {
    static ISSUE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^[A-Za-z0-9][A-Za-z0-9-]*/[A-Za-z0-9._-]+#[1-9][0-9]*$")
            .expect("valid issue-reference pattern")
    });
    ISSUE.is_match(value)
}

/// A `{fixme}` body split into its leading `:github:` option lines and its
/// description, with the problems validation reports in its option lines.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct FixmeBody {
    /// Issue references as written after `:github:`, in order.
    pub(crate) issues: Vec<String>,
    /// The words of the body after its option lines, joined by single spaces.
    pub(crate) description: String,
    /// Unsupported option lines and malformed `:github:` values.
    pub(crate) problems: Vec<String>,
}

impl FixmeBody {
    /// Parses a body whose comment delimiters are already removed. Option
    /// lines are the leading lines of the form `:name: value`.
    pub(crate) fn parse(body: &str) -> Self {
        let mut parsed = Self::default();
        let mut lines = body.lines().peekable();
        while let Some((name, value)) = lines.peek().and_then(|line| crate::option_line(line)) {
            if name != "github" {
                parsed.problems.push(format!(
                    "unsupported {{fixme}} option :{name}:; only :github: is recognized"
                ));
            } else if is_issue_reference(value) {
                parsed.issues.push(value.to_owned());
            } else {
                parsed.problems.push(format!(
                    ":github: value {value:?} is not of the form <owner>/<repo>#<number>"
                ));
            }
            lines.next();
        }
        parsed.description = lines
            .flat_map(str::split_whitespace)
            .collect::<Vec<_>>()
            .join(" ");
        parsed
    }

    /// The FIXME strings this `{fixme}` contributes when it acknowledges a
    /// gap: its distinct issues, or its description when it names none.
    pub(crate) fn acknowledgments(&self) -> Vec<String> {
        if self.issues.is_empty() {
            return vec![self.description.clone()];
        }
        let mut issues = Vec::new();
        for issue in &self.issues {
            if !issues.contains(issue) {
                issues.push(issue.clone());
            }
        }
        issues
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(entries: &[&str]) -> Vec<String> {
        entries.iter().map(|entry| (*entry).to_owned()).collect()
    }

    /// The judgment names listed under each verbose `###` heading.
    fn verbose_groups(output: &str) -> Vec<(String, Vec<String>)> {
        let mut groups: Vec<(String, Vec<String>)> = Vec::new();
        for line in output.lines() {
            if let Some(heading) = line.strip_prefix("### ") {
                groups.push((heading.to_owned(), Vec::new()));
            } else if let Some(item) = line.strip_prefix("- `")
                && let Some((_, names)) = groups.last_mut()
                && let Some((name, _)) = item.split_once('`')
            {
                names.push(name.to_owned());
            }
        }
        groups
    }

    // {test}`requirements.cli.fixme_summary`
    #[test]
    fn only_exact_issue_references_identify_issues() {
        assert_eq!(
            issue_references(&strings(&[
                "owner/repo#7",
                "https://github.com/owner/repo/issues/7",
                "FIXME(owner/repo#8)",
                "See owner/repo#9 for details",
                "#7",
                "owner/repo#007",
            ])),
            BTreeSet::from(["owner/repo#7".to_owned()])
        );
        let mut summary = FixmeSummary::default();
        summary.add("rules.exact", "requirement", &strings(&["owner/repo#7"]));
        summary.add(
            "rules.url",
            "requirement",
            &strings(&["https://github.com/owner/repo/issues/7"]),
        );
        summary.add("rules.unqualified", "example", &strings(&["#7"]));
        summary.add("rules.zero", "example", &strings(&["owner/repo#007"]));
        let output = summary.render(true);
        assert_eq!(
            verbose_groups(&output),
            [
                ("`owner/repo#7`".to_owned(), strings(&["rules.exact"])),
                (
                    "FIXME without an issue".to_owned(),
                    strings(&["rules.url", "rules.unqualified", "rules.zero"])
                ),
            ],
            "{output}"
        );
        assert!(
            output.contains("- 3 judgments are marked FIXME without an issue\n"),
            "{output}"
        );
    }

    // {test}`requirements.cli.fixme_summary`
    #[test]
    fn verbose_output_lists_each_judgment_under_every_group_it_belongs_to() {
        let mut summary = FixmeSummary::default();
        summary.add(
            "rules.both",
            "requirement",
            &strings(&["o/r#1", "o/r#2", "o/r#1"]),
        );
        summary.add("rules.one", "example", &strings(&["o/r#1"]));
        summary.add("rules.none", "requirement", &strings(&["no issue yet"]));
        summary.add(
            "rules.also_none",
            "example",
            &strings(&["o/r#0 is not one"]),
        );
        let output = summary.render(true);
        assert_eq!(
            verbose_groups(&output),
            [
                ("`o/r#1`".to_owned(), strings(&["rules.both", "rules.one"])),
                ("`o/r#2`".to_owned(), strings(&["rules.both"])),
                (
                    "FIXME without an issue".to_owned(),
                    strings(&["rules.none", "rules.also_none"])
                ),
            ],
            "{output}"
        );
        for line in [
            "- `rules.both` (requirement)",
            "- `rules.one` (example)",
            "- `rules.also_none` (example)",
            "<pre><code>o/r#1\no/r#2\no/r#1</code></pre>",
        ] {
            assert!(output.contains(line), "{output}");
        }
        assert!(
            output.contains("- `o/r#1` — affects 2 judgments\n"),
            "{output}"
        );
        assert!(
            output.contains("- 2 judgments are marked FIXME without an issue\n"),
            "{output}"
        );
        // Without --verbose, only the counts appear.
        assert!(!summary.render(false).contains("rules.both"));
    }

    // {test}`requirements.cli.fixme_summary`
    #[test]
    fn groups_order_by_decreasing_count_before_reference() {
        // `o/r#9` sorts after `o/r#1` as text but affects more judgments, so
        // it comes first; `o/r#1` and `o/r#5` tie and fall back to reference
        // order.
        let mut summary = FixmeSummary::default();
        summary.add("rules.a", "requirement", &strings(&["o/r#9"]));
        summary.add("rules.b", "requirement", &strings(&["o/r#9"]));
        summary.add("rules.c", "requirement", &strings(&["o/r#9", "o/r#5"]));
        summary.add("rules.d", "example", &strings(&["o/r#1"]));
        let output = summary.render(false);
        let order: Vec<_> = output
            .lines()
            .filter(|line| line.starts_with("- `o/r#"))
            .map(|line| line.split('`').nth(1).unwrap().to_owned())
            .collect();
        assert_eq!(order, ["o/r#9", "o/r#1", "o/r#5"], "{output}");
        assert!(
            output.contains("- `o/r#9` — affects 3 judgments"),
            "{output}"
        );
    }

    // {test}`requirements.references.roles`
    #[test]
    fn fixme_bodies_split_github_options_from_their_description() {
        let parsed = FixmeBody::parse(
            " :github: rust-lang/rust#123\n :github: example/io#125\n Retry is\n   untested.",
        );
        assert_eq!(parsed.issues, ["rust-lang/rust#123", "example/io#125"]);
        assert_eq!(parsed.description, "Retry is untested.");
        assert!(parsed.problems.is_empty(), "{parsed:?}");

        let parsed = FixmeBody::parse(
            ":github: o/r#1\n:gitlab: o/r#2\n:github: o/r\n:github: o/r#01\n:github:\nGap.\n:github: o/r#3",
        );
        assert_eq!(parsed.issues, ["o/r#1"]);
        // Option lines end at the first other line.
        assert_eq!(parsed.description, "Gap. :github: o/r#3");
        assert_eq!(parsed.problems.len(), 4, "{parsed:?}");
        assert!(parsed.problems[0].contains(":gitlab:"), "{parsed:?}");
        for (problem, value) in parsed.problems[1..].iter().zip(["o/r", "o/r#01", ""]) {
            assert!(problem.contains(&format!("{value:?}")), "{problem}");
        }

        assert!(FixmeBody::parse(":github: o/r#1").description.is_empty());
        for valid in ["o/r#1", "rust-lang/rust#123", "a1/b.c_d-e#90"] {
            assert!(is_issue_reference(valid), "{valid}");
        }
        for invalid in [
            "#1", "o/r#0", "o/r#01", "o/r/s#1", "-o/r#1", "o/r#1x", " o/r#1",
        ] {
            assert!(!is_issue_reference(invalid), "{invalid}");
        }
    }

    // {test}`requirements.ci.outcomes`
    #[test]
    fn a_fixme_acknowledges_its_distinct_issues_or_its_description() {
        let body = FixmeBody::parse(":github: o/r#1\n:github: o/r#2\n:github: o/r#1\nGap.");
        assert_eq!(body.acknowledgments(), ["o/r#1", "o/r#2"]);
        let body = FixmeBody::parse("Cancellation races are\nnot tested.");
        assert_eq!(
            body.acknowledgments(),
            ["Cancellation races are not tested."]
        );
    }
}

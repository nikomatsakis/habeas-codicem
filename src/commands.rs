use std::collections::BTreeSet;
use std::io;
use std::path::Path;

use crate::definition_glob;
use crate::escape_html;
use crate::evidence::inspect_repository;
use crate::fixme::FixmeSummary;
use crate::github::GithubIssues;
use crate::index::{EvidenceState, LoadedDetailEvidence, RenderSources, render_cli_detail_page};
use crate::markdown::render_evidence_table;

// {impl}`requirements.cli.status`
// {impl}`requirements.cli.fixme_summary`
pub fn status_markdown(root: &Path, all: bool) -> io::Result<String> {
    status_markdown_filtered(root, all, None)
}

// {impl}`requirements.definitions.selection`
pub fn status_markdown_filtered(root: &Path, all: bool, glob: Option<&str>) -> io::Result<String> {
    status_markdown_with_options(root, all, glob, false)
}

// {impl}`requirements.cli.shared_status_rendering`
// {impl}`requirements.references.definition_globs`
pub fn status_markdown_with_options(
    root: &Path,
    all: bool,
    glob: Option<&str>,
    verbose: bool,
) -> io::Result<String> {
    status_markdown_with_github(root, all, glob, verbose, &GithubIssues::offline())
}

/// Renders `habeas status`, reading the issues `{fixme}` references name
/// through `github`. The other status functions read no issues, so every
/// issue counts as open.
// {impl}`requirements.authoring.github_reads`
pub fn status_markdown_with_github(
    root: &Path,
    all: bool,
    glob: Option<&str>,
    verbose: bool,
    github: &GithubIssues,
) -> io::Result<String> {
    let mut inspection = inspect_repository(root)?;
    crate::github::add_issue_findings(&mut inspection, github);
    if let Some(glob) = glob {
        definition_glob::validate(glob)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        if !inspection.definitions.iter().any(|(name, d)| {
            d.kind != crate::DefinitionKind::Term && definition_glob::matches(glob, name)
        }) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no definitions match {glob:?}"),
            ));
        }
    }
    let mut output = String::from("# Definitions status\n\n");
    let mut rows = Vec::new();
    let mut findings = String::new();
    let mut fixmes = FixmeSummary::default();
    let mut associated_errors = BTreeSet::new();

    for (name, definition) in &inspection.definitions {
        if definition.kind == crate::DefinitionKind::Term {
            continue;
        }
        if glob.is_some_and(|glob| !definition_glob::matches(glob, name)) {
            continue;
        }
        let evidence = LoadedDetailEvidence::load(root, name, &inspection.report);
        for error in evidence.errors() {
            associated_errors.insert(error.clone());
        }
        fixmes.add(name, definition.kind.noun(), evidence.fixme_entries());
        if !all && evidence.state() == EvidenceState::Valid {
            continue;
        }
        if !all && !verbose && evidence.state() == EvidenceState::Fixme {
            continue;
        }

        rows.push((
            format!("`{name}` ({})", definition.kind.noun()),
            evidence.state().label(),
        ));
        if !evidence.errors().is_empty() {
            findings.push_str(&format!("\n### `{name}`\n\n<pre><code>"));
            findings.push_str(&escape_html(&evidence.errors().join("\n\n")));
            findings.push_str("</code></pre>\n");
        }
    }

    let fixmes = fixmes.render(verbose);
    if rows.is_empty() {
        output.push_str(if all {
            "No definitions found.\n"
        } else if !fixmes.is_empty() {
            "No other definitions need attention.\n"
        } else {
            "All matching definitions have valid evidence.\n"
        });
    } else {
        output.push_str(&render_evidence_table(&rows));
    }

    output.push_str(&fixmes);
    // Site reviews are checked on their own; list those needing attention,
    // or every one with `--all`, by requirement, file, and selector.
    // {impl}`requirements.references.site_review`
    let mut site_rows = Vec::new();
    for site in inspection
        .sites
        .iter()
        .filter(|(name, _)| glob.is_none_or(|glob| definition_glob::matches(glob, name)))
        .flat_map(|(_, sites)| sites)
    {
        let evidence =
            LoadedDetailEvidence::load_path(root, &site.review_path(), &inspection.report);
        for error in evidence.errors() {
            associated_errors.insert(error.clone());
        }
        if !evidence.errors().is_empty() {
            findings.push_str(&format!(
                "\n### `{}`\n\n<pre><code>",
                crate::sites::subject_label(&site.requirement, &site.file, &site.item)
            ));
            findings.push_str(&escape_html(&evidence.errors().join("\n\n")));
            findings.push_str("</code></pre>\n");
        }
        if all || evidence.state() != EvidenceState::Valid {
            site_rows.push(format!(
                "| `{}` | `{}` | `{}` | {} |\n",
                site.requirement,
                site.file,
                site.selector_label(),
                evidence.state().label()
            ));
        }
    }
    if !site_rows.is_empty() {
        output.push_str(
            "\n## Site reviews\n\n| Requirement | File | Selector | Evidence |\n| --- | --- | --- | --- |\n",
        );
        output.push_str(&site_rows.concat());
    }
    output.push_str(&disagreements(root, &inspection, glob));
    let other_findings = inspection
        .report
        .errors
        .iter()
        .filter(|error| !associated_errors.contains(error.as_str()))
        .collect::<Vec<_>>();
    match crate::evidence::untracked_reference_files(&inspection.root) {
        Ok(untracked) if untracked.is_empty() => {}
        Ok(untracked) => {
            output.push_str(
                "\n## Untracked files with references\n\n\
                 Reference discovery considers only files in the Git index, so these \
                 references are not validated. Stage the files with `git add` to include them.\n\n",
            );
            for file in untracked {
                let file = file.strip_prefix(&inspection.root).unwrap_or(&file);
                output.push_str(&format!("- `{}`\n", file.display()));
            }
        }
        Err(error) => output.push_str(&format!(
            "\n## Untracked files with references\n\nCannot check untracked files for references: {}\n",
            escape_html(&error.to_string())
        )),
    }
    if !findings.is_empty() {
        output.push_str("\n## Validation findings\n");
        output.push_str(&findings);
    }
    if !other_findings.is_empty() {
        output.push_str("\n## Other validation findings\n\n<pre><code>");
        output.push_str(&escape_html(
            &other_findings
                .into_iter()
                .map(|error| with_coverage_loss_review(&inspection, error))
                .collect::<Vec<_>>()
                .join("\n\n"),
        ));
        output.push_str("</code></pre>\n");
    }
    Ok(output)
}

/// A finding as `habeas status` shows it: an uncovered item's finding is
/// followed by the latest coverage-loss review of the item, if any.
// {impl}`requirements.authoring.coverage_loss`
fn with_coverage_loss_review(
    inspection: &crate::evidence::RepositoryInspection,
    error: &str,
) -> String {
    inspection
        .uncovered
        .iter()
        .find(|item| item.error == error)
        .and_then(|item| {
            crate::authoring::coverage_loss::status_lines(&inspection.root, &item.file, &item.item)
        })
        .map_or_else(|| error.to_owned(), |review| format!("{error}\n{review}"))
}

/// The "Disagreements" section of `habeas status`: each selected definition,
/// citation review, and site review whose recorded rationale states that its
/// assessments disagreed, which validation accepts. A citation review is
/// selected when one of its citations names a selected definition, and a site
/// review when its requirement is selected; without a selection, all are.
// {impl}`requirements.authoring.confirmation`
fn disagreements(
    root: &Path,
    inspection: &crate::evidence::RepositoryInspection,
    glob: Option<&str>,
) -> String {
    let selected = |name: &str| glob.is_none_or(|glob| definition_glob::matches(glob, name));
    let mut rows = Vec::new();
    let mut add = |label: String, relative: &str| {
        if let Some(kind) =
            LoadedDetailEvidence::load_path(root, relative, &inspection.report).disagreement()
        {
            rows.push(format!("- {label}: `{kind}`\n"));
        }
    };
    for (name, definition) in &inspection.definitions {
        if definition.kind != crate::DefinitionKind::Term && selected(name) {
            add(
                format!("`{name}` ({})", definition.kind.noun()),
                &format!("evidence/{name}.json"),
            );
        }
    }
    let outlines = crate::inputs::OutlineCache::default();
    for item in crate::citations::citation_items(
        &inspection.root,
        &inspection.definitions,
        &inspection.references,
        &outlines,
    )
    .values()
    {
        if glob.is_none()
            || item
                .citations
                .iter()
                .any(|citation| selected(&citation.name))
        {
            add(
                format!(
                    "`{}`",
                    crate::citations::subject_label(&item.file, &item.item)
                ),
                &item.review_path(),
            );
        }
    }
    for site in inspection
        .sites
        .iter()
        .filter(|(name, _)| selected(name))
        .flat_map(|(_, sites)| sites)
    {
        add(
            format!(
                "`{}`",
                crate::sites::subject_label(&site.requirement, &site.file, &site.item)
            ),
            &site.review_path(),
        );
    }
    if rows.is_empty() {
        return String::new();
    }
    format!(
        "\n## Disagreements\n\nThese records' independent assessments disagreed. Validation accepts them, but each disagreement is a finding: read the rationale with `habeas info` and close the gap or tighten the requirement rather than reassessing.\n\n{}",
        rows.concat()
    )
}

// {impl}`requirements.definitions.selection`
// {impl}`requirements.references.definition_globs`
pub fn info_markdown(root: &Path, selector: &str) -> io::Result<String> {
    let inspection = inspect_repository(root)?;
    definition_glob::validate(selector)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let outlines = crate::inputs::OutlineCache::default();
    let mut pages = Vec::new();
    for (name, definition) in &inspection.definitions {
        if definition.kind == crate::DefinitionKind::Term {
            continue;
        }
        if !definition_glob::matches(selector, name) {
            continue;
        }
        let evidence = LoadedDetailEvidence::load(root, name, &inspection.report);
        let mut page = render_cli_detail_page(
            name,
            &definition.source,
            &definition.text,
            definition.kind,
            &evidence,
            &RenderSources {
                root,
                definitions: &inspection.definitions,
                references: &inspection.references,
                source_links: None,
                outlines: &outlines,
            },
        );
        if let Some(sites) = inspection.sites.get(name) {
            page.push_str(&site_section(root, sites, &inspection.report));
        }
        pages.push(page);
    }
    if pages.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no definitions match {selector:?}"),
        ));
    }
    Ok(pages.join("\n\n---\n\n"))
}

/// A distributed requirement's sites for `habeas info`: how many the
/// requirement does not apply to, and each site with its review's
/// determination.
// {impl}`requirements.references.site_review`
fn site_section(
    root: &Path,
    sites: &[crate::sites::SiteItem],
    report: &crate::evidence::EvidenceReport,
) -> String {
    let mut rows = String::new();
    let mut not_applying = 0;
    for site in sites {
        let path = site.review_path();
        let evidence = LoadedDetailEvidence::load_path(root, &path, report);
        let record = std::fs::read_to_string(root.join(&path))
            .ok()
            .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok());
        let applies = record
            .as_ref()
            .and_then(|record| record.get("applies"))
            .and_then(serde_json::Value::as_bool);
        if applies == Some(false) {
            not_applying += 1;
        }
        let determination = record
            .as_ref()
            .and_then(|record| record.get("determination"))
            .map_or_else(|| "none".to_owned(), ToString::to_string);
        rows.push_str(&format!(
            "| `{}` | `{}` | {} | `{determination}` | {} |\n",
            site.file,
            site.selector_label(),
            applies.map_or("unknown", |applies| if applies { "yes" } else { "no" }),
            evidence.state().label()
        ));
    }
    format!(
        "\n\n## Sites\n\n{} sites; the requirement does not apply to {not_applying} of them.\n\n| File | Selector | Applies | Determination | Evidence |\n| --- | --- | --- | --- | --- |\n{rows}",
        sites.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // {cite}`habeas:util`
    /// Writes a record at `relative` whose rationale may carry the
    /// disagreement note.
    fn write_record(root: &Path, relative: &str, determination: serde_json::Value, note: bool) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut rationale = "Covered.".to_owned();
        if note {
            rationale.push_str(&format!(
                "\n\n{}: the first determination was `true` and an independent second assessment's was `false`.",
                crate::authoring::DISAGREEMENT_NOTE
            ));
        }
        std::fs::write(
            path,
            serde_json::json!({"determination": determination, "rationale": rationale}).to_string(),
        )
        .unwrap();
    }

    // {test}`requirements.cli.status`
    // {test}`requirements.authoring.confirmation`
    #[test]
    fn status_lists_definitions_and_reviews_whose_assessments_disagreed() {
        let chapter = format!(
            "{}\n:::{{req}} other\nOther MUST hold.\n:::\n",
            crate::sites::tests::RULE
        );
        let source = format!(
            "{}// CITE`requirements.other`\nfn cited() {{}}\n",
            crate::sites::tests::SOURCE
        )
        .replace("CITE", "{cite}");
        let temp = crate::sites::tests::project(&chapter, &source);
        let root = temp.path();
        write_record(
            root,
            "evidence/requirements.rule.json",
            serde_json::json!(true),
            true,
        );
        write_record(
            root,
            "evidence/requirements.other.json",
            serde_json::json!(true),
            false,
        );
        write_record(
            root,
            &crate::sites::review_path("requirements.rule", "src/lib.rs", "fn[check_two]"),
            serde_json::json!(true),
            true,
        );
        write_record(
            root,
            &crate::sites::review_path("requirements.rule", "src/lib.rs", "fn[check_one]"),
            serde_json::json!(false),
            false,
        );
        write_record(
            root,
            &crate::citations::review_path("src/lib.rs", "fn[cited]"),
            serde_json::json!({"FIXME": ["example/queue#1"]}),
            true,
        );
        let definition = "- `requirements.rule` (requirement): `true`\n";
        let site = "- `sites:requirements.rule/src/lib.rs#fn[check_two]`: `true`\n";
        let citation = "- `citations:src/lib.rs#fn[cited]`: `FIXME`\n";
        for (all, glob, verbose, expected) in [
            (false, None, false, &[definition, citation, site][..]),
            (true, None, false, &[definition, citation, site]),
            (false, None, true, &[definition, citation, site]),
            (false, Some("requirements.rule"), false, &[definition, site]),
            (false, Some("requirements.other"), false, &[citation]),
        ] {
            let status = status_markdown_with_options(root, all, glob, verbose).unwrap();
            let section = status
                .split_once("\n## Disagreements\n")
                .unwrap_or_else(|| panic!("{glob:?}: {status}"))
                .1;
            let section = section.split("\n## ").next().unwrap();
            let rows = section
                .lines()
                .filter(|line| line.starts_with("- "))
                .map(|line| format!("{line}\n"))
                .collect::<Vec<_>>();
            assert_eq!(rows, expected, "{all} {glob:?}: {status}");
            assert!(!section.contains("requirements.other` ("), "{status}");
            assert!(!section.contains("check_one"), "{status}");
        }
        // Without a disagreement, there is no section.
        write_record(
            root,
            "evidence/requirements.rule.json",
            serde_json::json!(true),
            false,
        );
        let status =
            status_markdown_with_options(root, false, Some("requirements.rule"), false).unwrap();
        assert!(status.contains("## Disagreements"), "{status}");
        write_record(
            root,
            &crate::sites::review_path("requirements.rule", "src/lib.rs", "fn[check_two]"),
            serde_json::json!(true),
            false,
        );
        let status =
            status_markdown_with_options(root, false, Some("requirements.rule"), false).unwrap();
        assert!(!status.contains("## Disagreements"), "{status}");
    }
}

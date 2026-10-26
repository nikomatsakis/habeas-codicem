use std::collections::BTreeSet;
use std::io;
use std::path::Path;

use regex::Regex;

use crate::escape_html;
use crate::evidence::inspect_repository;
use crate::index::{EvidenceState, LoadedDetailEvidence, render_cli_detail_page};

// {impl}`requirements.cli.status`
pub fn status_markdown(root: &Path, all: bool) -> io::Result<String> {
    let inspection = inspect_repository(root)?;
    let mut output = String::from("# Requirements status\n\n");
    let mut rows = Vec::new();
    let mut findings = String::new();
    let mut associated_errors = BTreeSet::new();

    for name in inspection.requirements.keys() {
        let evidence = LoadedDetailEvidence::load(root, name, &inspection.report);
        for error in evidence.errors() {
            associated_errors.insert(error.clone());
        }
        if !all && evidence.state() == EvidenceState::Valid {
            continue;
        }

        rows.push((format!("`{name}`"), evidence.state().label()));
        if !evidence.errors().is_empty() {
            findings.push_str(&format!("\n### `{name}`\n\n<pre><code>"));
            findings.push_str(&escape_html(&evidence.errors().join("\n\n")));
            findings.push_str("</code></pre>\n");
        }
    }

    if rows.is_empty() {
        output.push_str(if all {
            "No requirements found.\n"
        } else {
            "All requirements have valid evidence.\n"
        });
    } else {
        output.push_str(&render_status_table(&rows));
    }

    let other_findings = inspection
        .report
        .errors
        .iter()
        .filter(|error| !associated_errors.contains(error.as_str()))
        .collect::<Vec<_>>();
    if !findings.is_empty() {
        output.push_str("\n## Validation findings\n");
        output.push_str(&findings);
    }
    if !other_findings.is_empty() {
        output.push_str("\n## Other validation findings\n\n<pre><code>");
        output.push_str(&escape_html(
            &other_findings
                .into_iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join("\n\n"),
        ));
        output.push_str("</code></pre>\n");
    }
    Ok(output)
}

// {impl}`requirements.cli.status_table_alignment`
fn render_status_table(rows: &[(String, &'static str)]) -> String {
    let requirement_width = rows
        .iter()
        .map(|(requirement, _)| requirement.len())
        .max()
        .unwrap_or_default()
        .max("Requirement".len());
    let evidence_width = rows
        .iter()
        .map(|(_, evidence)| evidence.len())
        .max()
        .unwrap_or_default()
        .max("Evidence".len());
    let mut table = String::new();
    for (requirement, evidence) in [("Requirement", "Evidence"), ("---", "---")] {
        table.push_str(&format!(
            "| {requirement:<requirement_width$} | {evidence:<evidence_width$} |\n"
        ));
    }
    for (requirement, evidence) in rows {
        table.push_str(&format!(
            "| {requirement:<requirement_width$} | {evidence:<evidence_width$} |\n"
        ));
    }
    table
}

// {impl}`requirements.cli.info`
pub fn info_markdown(root: &Path, selector: &str) -> io::Result<String> {
    let inspection = inspect_repository(root)?;
    let exact = inspection.requirements.contains_key(selector);
    let regex = if exact {
        None
    } else {
        Some(Regex::new(selector).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid requirement regex {selector:?}: {error}"),
            )
        })?)
    };
    let mut pages = Vec::new();
    for (name, requirement) in &inspection.requirements {
        let matches = if exact {
            name == selector
        } else {
            regex.as_ref().is_some_and(|regex| regex.is_match(name))
        };
        if !matches {
            continue;
        }
        let evidence = LoadedDetailEvidence::load(root, name, &inspection.report);
        pages.push(render_cli_detail_page(
            name,
            &requirement.source,
            &requirement.text,
            &evidence,
        ));
    }
    if pages.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no requirements match {selector:?}"),
        ));
    }
    Ok(pages.join("\n\n---\n\n"))
}

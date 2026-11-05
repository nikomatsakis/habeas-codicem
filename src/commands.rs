use std::collections::BTreeSet;
use std::io;
use std::path::Path;

use crate::definition_glob;
use crate::escape_html;
use crate::evidence::inspect_repository;
use crate::index::{EvidenceState, LoadedDetailEvidence, RenderSources, render_cli_detail_page};
use crate::markdown::render_evidence_table;

// {impl}`requirements.cli.status`
pub fn status_markdown(root: &Path, all: bool) -> io::Result<String> {
    status_markdown_filtered(root, all, None)
}

// {impl}`requirements.cli.status`
// {impl}`requirements.definitions.selection`
pub fn status_markdown_filtered(root: &Path, all: bool, glob: Option<&str>) -> io::Result<String> {
    let inspection = inspect_repository(root)?;
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
    let mut fixmes = String::new();
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
        if !all && evidence.state() == EvidenceState::Valid {
            continue;
        }

        rows.push((
            format!("`{name}` ({})", definition.kind.noun()),
            evidence.state().label(),
        ));
        if !evidence.fixme_entries().is_empty() {
            fixmes.push_str(&format!("\n### `{name}`\n\n<pre><code>"));
            fixmes.push_str(&escape_html(&evidence.fixme_entries().join("\n")));
            fixmes.push_str("</code></pre>\n");
        }
        if !evidence.errors().is_empty() {
            findings.push_str(&format!("\n### `{name}`\n\n<pre><code>"));
            findings.push_str(&escape_html(&evidence.errors().join("\n\n")));
            findings.push_str("</code></pre>\n");
        }
    }

    if rows.is_empty() {
        output.push_str(if all {
            "No definitions found.\n"
        } else {
            "All matching definitions have valid evidence.\n"
        });
    } else {
        // {impl}`requirements.cli.shared_status_rendering`
        output.push_str(&render_evidence_table(&rows));
    }

    let other_findings = inspection
        .report
        .errors
        .iter()
        .filter(|error| !associated_errors.contains(error.as_str()))
        .collect::<Vec<_>>();
    if !fixmes.is_empty() {
        output.push_str("\n## FIXME references\n");
        output.push_str(&fixmes);
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
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join("\n\n"),
        ));
        output.push_str("</code></pre>\n");
    }
    Ok(output)
}

// {impl}`requirements.cli.info`
// {impl}`requirements.definitions.selection`
pub fn info_markdown(root: &Path, selector: &str) -> io::Result<String> {
    let inspection = inspect_repository(root)?;
    definition_glob::validate(selector)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let mut pages = Vec::new();
    for (name, definition) in &inspection.definitions {
        if definition.kind == crate::DefinitionKind::Term {
            continue;
        }
        if !definition_glob::matches(selector, name) {
            continue;
        }
        let evidence = LoadedDetailEvidence::load(root, name, &inspection.report);
        pages.push(render_cli_detail_page(
            name,
            &definition.source,
            &definition.text,
            definition.kind,
            &evidence,
            &RenderSources {
                root,
                definitions: &inspection.definitions,
                source_links: None,
            },
        ));
    }
    if pages.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no definitions match {selector:?}"),
        ));
    }
    Ok(pages.join("\n\n---\n\n"))
}

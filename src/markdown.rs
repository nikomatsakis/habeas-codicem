// {impl}`requirements.cli.status_table_alignment`
// {impl}`requirements.cli.shared_status_rendering`
pub(crate) fn render_evidence_table(rows: &[(String, &str)]) -> String {
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

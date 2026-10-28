// {impl}`requirements.cli.status_table_alignment`
// {impl}`requirements.cli.shared_status_rendering`
pub(crate) fn render_evidence_table(rows: &[(String, &str)]) -> String {
    let definition_width = rows
        .iter()
        .map(|(definition, _)| definition.len())
        .max()
        .unwrap_or_default()
        .max("Definition".len());
    let evidence_width = rows
        .iter()
        .map(|(_, evidence)| evidence.len())
        .max()
        .unwrap_or_default()
        .max("Evidence".len());
    let mut table = String::new();
    for (definition, evidence) in [("Definition", "Evidence"), ("---", "---")] {
        table.push_str(&format!(
            "| {definition:<definition_width$} | {evidence:<evidence_width$} |\n"
        ));
    }
    for (definition, evidence) in rows {
        table.push_str(&format!(
            "| {definition:<definition_width$} | {evidence:<evidence_width$} |\n"
        ));
    }
    table
}

use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::setup;

/// The sentence every applicable session receives, pointing at the overview skill.
const SKILL_SENTENCE: &str = "This project tracks requirements with Habeas; load the `habeas` skill before starting any work.";

// {impl}`requirements.integration.symposium_skill_hook`
// {impl}`requirements.integration.symposium_skill_hook.setup_state`
// {impl}`requirements.integration.symposium_skill_hook.not_applicable`
pub(crate) fn session_start() -> Result<(), Box<dyn Error>> {
    let input: Value = serde_json::from_reader(io::stdin()).unwrap_or(Value::Null);
    let response = session_start_response(&input, &std::env::current_dir()?);
    serde_json::to_writer(io::stdout().lock(), &response)?;
    Ok(())
}

fn session_start_response(input: &Value, fallback_cwd: &Path) -> Value {
    let Some(event) = input.get("SessionStart") else {
        return json!({"SessionStart": {}});
    };
    let cwd = event
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| fallback_cwd.to_path_buf());
    let Some(root) = setup::find_project_root(&cwd) else {
        return json!({"SessionStart": {}});
    };
    if !setup::project_depends_on_habeas(&root) {
        return json!({"SessionStart": {}});
    }

    let mut context = SKILL_SENTENCE.to_owned();
    if !setup::is_configured(&root) {
        context.push(' ');
        context.push_str(&format!(
            "Habeas Codicem is not fully configured in {}. Run `habeas setup` in this project to configure its mdBook preprocessor, stylesheet, and validation test.",
            root.display()
        ));
    }
    json!({"SessionStart": {"additionalContext": context}})
}

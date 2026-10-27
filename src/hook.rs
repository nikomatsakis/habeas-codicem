use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::setup;

// {impl}`requirements.integration.symposium_setup_hook`
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
    if setup::is_configured(&root) {
        return json!({"SessionStart": {}});
    }

    json!({
        "SessionStart": {
            "additionalContext": format!(
                "Habeas Codicem is not fully configured in {}. Run `habeas setup` in this project to configure its mdBook preprocessor, stylesheet, and validation test.",
                root.display()
            )
        }
    })
}

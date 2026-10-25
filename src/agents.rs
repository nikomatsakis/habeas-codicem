use std::error::Error;
use std::path::Path;

use crate::setup::{project_root, write_new_or_same};

const SKILLS: [(&str, &str); 3] = [
    (
        "habeas-bootstrap-requirements",
        include_str!("../assets/skills/habeas-bootstrap-requirements/SKILL.md"),
    ),
    (
        "habeas-requirements",
        include_str!("../assets/skills/habeas-requirements/SKILL.md"),
    ),
    (
        "habeas-evidence",
        include_str!("../assets/skills/habeas-evidence/SKILL.md"),
    ),
];

// {impl}`requirements.cli.agents`
pub(crate) fn run() -> Result<(), Box<dyn Error>> {
    let root = project_root(false)?;
    install(&root)
}

// {impl}`requirements.cli.agents`
pub(crate) fn install(root: &Path) -> Result<(), Box<dyn Error>> {
    for (name, contents) in SKILLS {
        let path = root.join(".agents/skills").join(name).join("SKILL.md");
        write_new_or_same(&path, contents)?;
        println!("{}", path.display());
    }
    Ok(())
}

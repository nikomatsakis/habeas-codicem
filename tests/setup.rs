use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

fn fixture() -> TempDir {
    let temp = TempDir::new().unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::create_dir_all(temp.path().join("md")).unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname = \"sample-project\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(temp.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        temp.path().join("book.toml"),
        "[book]\nsrc = \"md\"\n\n[output.html]\nadditional-css = [\"md/custom.css\"]\n",
    )
    .unwrap();
    fs::write(temp.path().join("md/SUMMARY.md"), "# Summary\n").unwrap();
    temp
}

fn run(project: &Path, args: &[&str]) {
    let output = Command::new(env!("CARGO_BIN_EXE_habeas"))
        .args(args)
        .current_dir(project)
        .env("CARGO_NET_OFFLINE", "true")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "habeas {:?} failed:\n{}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

// {test}`requirements.cli.setup`
// {test}`requirements.cli.agents`
// {test}`requirements.cli.setup_css`
// {test}`requirements.cli.setup_local_path`
#[test]
fn setup_and_agents_are_repeatable() {
    let temp = fixture();
    let project = temp.path();
    let local_crate = env!("CARGO_MANIFEST_DIR");

    run(project, &["setup", "--path", local_crate]);
    assert!(!project.join(".agents/skills").exists());
    run(project, &["setup", "--path", local_crate]);
    run(project, &["setup", "--install-skills"]);
    run(
        project,
        &["setup", "--path", local_crate, "--install-skills"],
    );
    run(
        project,
        &["setup", "--install-skills", "--path", local_crate],
    );
    run(project, &["agents"]);
    run(project, &["agents"]);

    let cargo = fs::read_to_string(project.join("Cargo.toml")).unwrap();
    assert!(cargo.contains("[dev-dependencies]"));
    assert!(cargo.contains("habeas"));
    assert!(cargo.contains("path = "));
    let book = fs::read_to_string(project.join("book.toml")).unwrap();
    assert!(book.contains("command = \"habeas mdbook\""));
    assert!(book.contains("md/habeas.css"));
    assert!(book.contains("md/custom.css"));
    assert_eq!(book.matches("md/habeas.css").count(), 1);
    assert_eq!(
        fs::read_to_string(project.join("tests/habeas.rs")).unwrap(),
        "#[test]\nfn habeas() {\n    habeas::validate();\n}\n"
    );
    assert!(project.join("md/habeas.css").is_file());
    assert_eq!(
        fs::read_to_string(project.join(".agents/skills/habeas-bootstrap-requirements/SKILL.md"))
            .unwrap(),
        include_str!("../assets/skills/habeas-bootstrap-requirements/SKILL.md")
    );
    assert_eq!(
        fs::read_to_string(project.join(".agents/skills/habeas-requirements/SKILL.md")).unwrap(),
        include_str!("../assets/skills/habeas-requirements/SKILL.md")
    );
    assert_eq!(
        fs::read_to_string(project.join(".agents/skills/habeas-evidence/SKILL.md")).unwrap(),
        include_str!("../assets/skills/habeas-evidence/SKILL.md")
    );
}

// {test}`requirements.cli.setup`
// {test}`requirements.cli.agents`
#[test]
fn setup_install_skills_preserves_differing_skill() {
    let temp = fixture();
    let project = temp.path();
    run(project, &["setup", "--path", env!("CARGO_MANIFEST_DIR")]);

    let skill = project.join(".agents/skills/habeas-requirements/SKILL.md");
    fs::create_dir_all(skill.parent().unwrap()).unwrap();
    fs::write(&skill, "custom skill\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_habeas"))
        .args(["setup", "--install-skills"])
        .current_dir(project)
        .env("CARGO_NET_OFFLINE", "true")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("already exists with different content")
    );
    assert_eq!(fs::read_to_string(skill).unwrap(), "custom skill\n");
}

// {test}`requirements.cli.setup_project`
// {test}`requirements.cli.setup_css`
#[test]
fn setup_rejects_invalid_book_configuration() {
    let temp = fixture();
    let project = temp.path();
    let binary = env!("CARGO_BIN_EXE_habeas");

    for (book, expected) in [
        (
            "[book]\nsrc = \"../outside\"\n",
            "book.src must be a directory",
        ),
        (
            "[book]\nsrc = \"md\"\n\n[output.html]\nadditional-css = \"wrong\"\n",
            "output.html.additional-css must be an array",
        ),
    ] {
        fs::write(project.join("book.toml"), book).unwrap();
        let output = Command::new(binary)
            .args(["setup", "--path", env!("CARGO_MANIFEST_DIR")])
            .current_dir(project)
            .env("CARGO_NET_OFFLINE", "true")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{:?}: {}",
            book,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fs::write(project.join("book.toml"), "[book]\nsrc = \"md\"\n").unwrap();
    fs::remove_file(project.join("md/SUMMARY.md")).unwrap();
    let output = Command::new(binary)
        .args(["setup", "--path", env!("CARGO_MANIFEST_DIR")])
        .current_dir(project)
        .env("CARGO_NET_OFFLINE", "true")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot find mdBook SUMMARY.md"));
}

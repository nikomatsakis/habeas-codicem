use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use tempfile::TempDir;

// {cite}`habeas:util`
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

// {cite}`habeas:util`
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

// {cite}`habeas:util`
fn session_start(project: &Path, cwd: &Path) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_habeas"))
        .args(["hook", "session-start"])
        .current_dir(project)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let input = json!({
        "SessionStart": {
            "session_id": "test-session",
            "cwd": cwd,
        }
    });
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&input).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "hook failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

// {cite}`habeas:util`
/// Every `habeas` entry in the manifest's dependency tables.
fn habeas_dependencies(project: &Path) -> Vec<(String, toml_edit::Item)> {
    let manifest: toml_edit::DocumentMut = fs::read_to_string(project.join("Cargo.toml"))
        .unwrap()
        .parse()
        .unwrap();
    ["dependencies", "dev-dependencies", "build-dependencies"]
        .into_iter()
        .filter_map(|table| {
            let item = manifest.get(table)?.get("habeas")?.clone();
            Some((table.to_owned(), item))
        })
        .collect()
}

// {test}`requirements.cli.setup_local_path`
#[test]
fn local_path_setup_adds_exactly_that_crate_once() {
    let local_crate = fs::canonicalize(env!("CARGO_MANIFEST_DIR")).unwrap();
    let temp = fixture();
    let project = temp.path();
    // A relative `--path` is resolved from the invocation directory.
    let relative: std::path::PathBuf = if cfg!(unix) {
        let up: std::path::PathBuf = project
            .canonicalize()
            .unwrap()
            .components()
            .skip(1)
            .map(|_| "..")
            .collect();
        up.join(local_crate.strip_prefix("/").unwrap())
    } else {
        local_crate.clone()
    };
    assert!(cfg!(not(unix)) || relative.is_relative());
    run(project, &["setup", "--path", relative.to_str().unwrap()]);
    let first = fs::read_to_string(project.join("Cargo.toml")).unwrap();
    let dependencies = habeas_dependencies(project);
    assert_eq!(dependencies.len(), 1, "{first}");
    let (table, item) = &dependencies[0];
    assert_eq!(table, "dev-dependencies", "{first}");
    let path = item
        .get("path")
        .and_then(|path| path.as_str())
        .unwrap_or_else(|| panic!("habeas has a path: {first}"));
    assert_eq!(
        fs::canonicalize(project.join(path)).unwrap(),
        local_crate,
        "{first}"
    );

    // Repeating setup leaves the manifest exactly as it was.
    run(project, &["setup", "--path", local_crate.to_str().unwrap()]);
    assert_eq!(
        fs::read_to_string(project.join("Cargo.toml")).unwrap(),
        first
    );
}

// {test}`requirements.cli.setup_local_path`
#[test]
fn local_path_setup_keeps_an_existing_habeas_dev_dependency() {
    let temp = fixture();
    let project = temp.path();
    let mut manifest = fs::read_to_string(project.join("Cargo.toml")).unwrap();
    manifest.push_str("\n[dev-dependencies]\nhabeas = \"0.1\"\n");
    fs::write(project.join("Cargo.toml"), &manifest).unwrap();
    run(project, &["setup", "--path", env!("CARGO_MANIFEST_DIR")]);
    assert_eq!(
        fs::read_to_string(project.join("Cargo.toml")).unwrap(),
        manifest
    );
}

// {test}`requirements.cli.setup_local_path`
#[test]
fn local_path_setup_rejects_a_missing_crate_directory() {
    let temp = fixture();
    let project = temp.path();
    let manifest = fs::read_to_string(project.join("Cargo.toml")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_habeas"))
        .args(["setup", "--path", "no-such-crate"])
        .current_dir(project)
        .env("CARGO_NET_OFFLINE", "true")
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    assert_eq!(
        fs::read_to_string(project.join("Cargo.toml")).unwrap(),
        manifest
    );
    assert!(habeas_dependencies(project).is_empty());
}

// {test}`requirements.cli.setup`
// {test}`requirements.cli.setup_css`
// {test}`requirements.cli.setup_local_path`
// {test}`requirements.cli.mdbook`
#[test]
fn setup_is_repeatable() {
    let temp = fixture();
    let project = temp.path();
    let local_crate = env!("CARGO_MANIFEST_DIR");

    run(project, &["setup", "--path", local_crate]);
    assert!(!project.join(".agents/skills").exists());
    run(project, &["setup", "--path", local_crate]);

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
    assert!(!project.join(".agents/skills").exists());
    let book: toml_edit::DocumentMut = book.parse().unwrap();
    assert_eq!(
        book["output"]["html"]["fold"]["enable"].as_bool(),
        Some(true)
    );
    assert_eq!(book.to_string().matches("enable").count(), 1, "{book}");
}

// {test}`requirements.cli.setup`
// {test}`requirements.index.detail_pages`
#[test]
fn setup_keeps_an_existing_fold_setting() {
    for (configuration, expected) in [
        (
            "[output.html.fold]\nenable = false\nlevel = 1\n",
            Some(false),
        ),
        ("[output.html.fold]\nlevel = 1\n", Some(true)),
        ("[output.html]\nfold = { enable = false }\n", Some(false)),
    ] {
        let temp = fixture();
        let project = temp.path();
        fs::write(
            project.join("book.toml"),
            format!("[book]\nsrc = \"md\"\n\n{configuration}"),
        )
        .unwrap();
        run(project, &["setup", "--path", env!("CARGO_MANIFEST_DIR")]);
        let book: toml_edit::DocumentMut = fs::read_to_string(project.join("book.toml"))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(
            book["output"]["html"]["fold"]["enable"].as_bool(),
            expected,
            "{configuration}"
        );
        if configuration.contains("level") {
            assert_eq!(
                book["output"]["html"]["fold"]["level"].as_integer(),
                Some(1)
            );
        }
    }
}

// {test}`requirements.cli.setup`
#[test]
fn setup_preserves_existing_agent_skills() {
    let temp = fixture();
    let project = temp.path();

    let skill = project.join(".agents/skills/habeas-requirements/SKILL.md");
    fs::create_dir_all(skill.parent().unwrap()).unwrap();
    fs::write(&skill, "custom skill\n").unwrap();

    run(project, &["setup", "--path", env!("CARGO_MANIFEST_DIR")]);
    assert_eq!(fs::read_to_string(skill).unwrap(), "custom skill\n");
}

// {test}`requirements.cli.setup_project`
// {test}`requirements.integration.symposium_setup_hook`
// {test}`requirements.cli.setup`
#[test]
fn setup_uses_src_when_book_table_or_source_setting_is_absent() {
    for configuration in ["", "[book]\ntitle = 'Example'\n"] {
        let temp = fixture();
        let project = temp.path();
        fs::write(project.join("book.toml"), configuration).unwrap();
        fs::write(project.join("src/SUMMARY.md"), "# Summary\n").unwrap();
        run(project, &["setup", "--path", env!("CARGO_MANIFEST_DIR")]);
        assert!(project.join("src/habeas.css").is_file());
        assert_eq!(session_start(project, project), json!({"SessionStart": {}}));
        let mut book: toml_edit::DocumentMut = fs::read_to_string(project.join("book.toml"))
            .unwrap()
            .parse()
            .unwrap();
        book["book"]["src"] = toml_edit::value(7);
        fs::write(project.join("book.toml"), book.to_string()).unwrap();
        assert!(session_start(project, project)["SessionStart"]["additionalContext"].is_string());
    }
}

// {test}`requirements.cli.setup_project`
// {test}`requirements.cli.setup_css`
// {test}`requirements.cli.setup`
#[test]
fn setup_rejects_invalid_book_configuration() {
    let temp = fixture();
    let project = temp.path();
    let binary = env!("CARGO_BIN_EXE_habeas");

    for (book, expected) in [
        ("book = 'wrong'\n", "book must be a table"),
        ("[book]\nsrc = 7\n", "book.src must be a string"),
        ("[book]\nsrc = ''\n", "book.src must be a directory"),
        ("[book]\nsrc = '/outside'\n", "book.src must be a directory"),
        (
            "[book]\nsrc = \"../outside\"\n",
            "book.src must be a directory",
        ),
        (
            "[book]\nsrc = \"md\"\n\n[output.html]\nadditional-css = \"wrong\"\n",
            "output.html.additional-css must be an array",
        ),
        (
            "[book]\nsrc = \"md\"\n\n[output.html]\nfold = true\n",
            "output.html.fold must be a table",
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

// {test}`requirements.integration.symposium_setup_hook`
#[test]
fn session_start_advises_only_when_setup_is_incomplete() {
    let temp = fixture();
    let project = temp.path();
    assert_eq!(
        session_start(project, project),
        json!({"SessionStart": {}}),
        "a project without Habeas must not receive an advisory"
    );
    let manifest = fs::read_to_string(project.join("Cargo.toml")).unwrap();
    fs::write(
        project.join("Cargo.toml"),
        format!("{manifest}\n[dependencies]\nhabeas = \"0.2\"\n"),
    )
    .unwrap();
    assert_eq!(
        session_start(project, project),
        json!({"SessionStart": {}}),
        "only a Habeas dev dependency makes the hook applicable"
    );
    fs::write(project.join("Cargo.toml"), manifest).unwrap();
    run(project, &["setup", "--path", env!("CARGO_MANIFEST_DIR")]);

    let nested = project.join("src/nested");
    fs::create_dir_all(&nested).unwrap();
    assert_eq!(session_start(project, &nested), json!({"SessionStart": {}}));

    let css = project.join("md/habeas.css");
    fs::remove_file(&css).unwrap();
    let book = fs::read_to_string(project.join("book.toml")).unwrap();
    let response = session_start(project, &nested);
    let context = response["SessionStart"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.contains("Habeas Codicem is not fully configured"));
    assert!(context.contains("habeas setup"));
    assert!(!css.exists(), "the hook must not recreate the stylesheet");
    assert_eq!(fs::read_to_string(project.join("book.toml")).unwrap(), book);

    fs::write(&css, include_str!("../md/habeas.css")).unwrap();
    fs::write(
        project.join("book.toml"),
        book.replace("habeas mdbook", "wrong-preprocessor"),
    )
    .unwrap();
    let response = session_start(project, &nested);
    assert!(response["SessionStart"]["additionalContext"].is_string());

    fs::write(project.join("book.toml"), &book).unwrap();
    assert_eq!(session_start(project, &nested), json!({"SessionStart": {}}));
    let mut without_fold: toml_edit::DocumentMut = book.parse().unwrap();
    without_fold["output"]["html"]["fold"]
        .as_table_like_mut()
        .unwrap()
        .remove("enable");
    fs::write(project.join("book.toml"), without_fold.to_string()).unwrap();
    let response = session_start(project, &nested);
    assert!(
        response["SessionStart"]["additionalContext"].is_string(),
        "a book without `output.html.fold.enable` is not fully configured"
    );
    let mut disabled_fold = without_fold;
    disabled_fold["output"]["html"]["fold"]["enable"] = toml_edit::value(false);
    fs::write(project.join("book.toml"), disabled_fold.to_string()).unwrap();
    assert_eq!(
        session_start(project, &nested),
        json!({"SessionStart": {}}),
        "setup keeps an existing fold setting, so any value is complete"
    );

    // Each required file and the stylesheet entry is checked on its own.
    fs::write(project.join("book.toml"), &book).unwrap();
    let mut without_css: toml_edit::DocumentMut = book.parse().unwrap();
    without_css["output"]["html"]["additional-css"] = toml_edit::value(toml_edit::Array::new());
    fs::write(project.join("book.toml"), without_css.to_string()).unwrap();
    assert!(
        session_start(project, &nested)["SessionStart"]["additionalContext"].is_string(),
        "a book without the stylesheet entry is not fully configured"
    );
    fs::write(project.join("book.toml"), &book).unwrap();
    let test = project.join("tests/habeas.rs");
    let test_source = fs::read_to_string(&test).unwrap();
    for (path, original) in [
        (&css, include_str!("../md/habeas.css").to_owned()),
        (&test, test_source),
    ] {
        fs::write(path, "user content\n").unwrap();
        assert!(
            session_start(project, &nested)["SessionStart"]["additionalContext"].is_string(),
            "{} with different content is not compatible",
            path.display()
        );
        assert_eq!(fs::read_to_string(path).unwrap(), "user content\n");
        fs::remove_file(path).unwrap();
        assert!(
            session_start(project, &nested)["SessionStart"]["additionalContext"].is_string(),
            "a missing {} is not fully configured",
            path.display()
        );
        assert!(
            !path.exists(),
            "the hook must not recreate {}",
            path.display()
        );
        fs::write(path, original).unwrap();
        assert_eq!(session_start(project, &nested), json!({"SessionStart": {}}));
    }

    fs::remove_file(project.join("book.toml")).unwrap();
    assert_eq!(session_start(project, &nested), json!({"SessionStart": {}}));
}

// {test}`requirements.cli.setup`
#[test]
fn setup_does_not_overwrite_a_differing_test_or_stylesheet() {
    for existing in ["tests/habeas.rs", "md/habeas.css"] {
        let temp = fixture();
        let project = temp.path();
        let path = project.join(existing);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "user content\n").unwrap();
        let book = fs::read_to_string(project.join("book.toml")).unwrap();
        let cargo = fs::read_to_string(project.join("Cargo.toml")).unwrap();

        let output = Command::new(env!("CARGO_BIN_EXE_habeas"))
            .args(["setup", "--path", env!("CARGO_MANIFEST_DIR")])
            .current_dir(project)
            .env("CARGO_NET_OFFLINE", "true")
            .output()
            .unwrap();
        assert!(!output.status.success(), "{existing}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("already exists with different content") && stderr.contains(existing),
            "{existing}: {stderr}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "user content\n");
        assert_eq!(fs::read_to_string(project.join("book.toml")).unwrap(), book);
        assert_eq!(
            fs::read_to_string(project.join("Cargo.toml")).unwrap(),
            cargo
        );
    }
}

// {test}`requirements.cli.setup_project`
#[test]
fn setup_finds_the_nearest_cargo_and_mdbook_ancestor() {
    let binary = env!("CARGO_BIN_EXE_habeas");
    for present in [
        ["Cargo.toml"].as_slice(),
        ["book.toml"].as_slice(),
        [].as_slice(),
    ] {
        let temp = TempDir::new().unwrap();
        for file in present {
            fs::write(temp.path().join(file), "").unwrap();
        }
        let nested = temp.path().join("nested");
        fs::create_dir_all(&nested).unwrap();
        let output = Command::new(binary)
            .args(["setup", "--path", env!("CARGO_MANIFEST_DIR")])
            .current_dir(&nested)
            .env("CARGO_NET_OFFLINE", "true")
            .output()
            .unwrap();
        assert!(!output.status.success(), "{present:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("cannot find a project with Cargo.toml and book.toml"),
            "{present:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let temp = fixture();
    let project = temp.path();
    let nested = project.join("src/nested");
    fs::create_dir_all(&nested).unwrap();
    run(&nested, &["setup", "--path", env!("CARGO_MANIFEST_DIR")]);
    assert!(project.join("tests/habeas.rs").is_file());
    assert!(project.join("md/habeas.css").is_file());
    assert!(!nested.join("tests").exists());
}

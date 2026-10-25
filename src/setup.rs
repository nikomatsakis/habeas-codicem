use std::error::Error;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use toml_edit::{Array, DocumentMut, Item, value};

const TEST_SOURCE: &str = "#[test]\nfn habeas() {\n    habeas::validate();\n}\n";
const BADGE_CSS: &str = include_str!("../md/habeas.css");

// {impl}`requirements.cli.setup_project`
pub(crate) fn project_root(require_book: bool) -> Result<PathBuf, Box<dyn Error>> {
    let cwd = std::env::current_dir()?;
    cwd.ancestors()
        .find(|directory| {
            directory.join("Cargo.toml").is_file()
                && (!require_book || directory.join("book.toml").is_file())
        })
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            if require_book {
                "cannot find a project with Cargo.toml and book.toml".into()
            } else {
                "cannot find a Cargo project".into()
            }
        })
}

// {impl}`requirements.cli.setup`
pub(crate) fn write_new_or_same(path: &Path, content: &str) -> Result<(), Box<dyn Error>> {
    if path.exists() {
        if fs::read_to_string(path)? != content {
            return Err(format!(
                "{} already exists with different content; review it before running setup again",
                path.display()
            )
            .into());
        }
    } else {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content)?;
    }
    Ok(())
}

// {impl}`requirements.cli.setup`
pub(crate) fn run(local_crate: Option<&Path>, install_skills: bool) -> Result<(), Box<dyn Error>> {
    let root = project_root(true)?;
    let book_path = root.join("book.toml");
    let mut book: DocumentMut = fs::read_to_string(&book_path)?.parse()?;
    // {impl}`requirements.cli.setup_project`
    let source = book["book"]["src"].as_str().unwrap_or("src").to_owned();
    let source_path = Path::new(&source);
    if source.is_empty()
        || !source_path
            .components()
            .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
    {
        return Err("book.src must be a directory inside this project".into());
    }
    let source_dir = root.join(source_path);
    if !source_dir.join("SUMMARY.md").is_file() {
        return Err(format!("cannot find mdBook SUMMARY.md in {}", source_dir.display()).into());
    }

    let css_path = source_dir.join("habeas.css");
    let test_path = root.join("tests/habeas.rs");
    for (path, desired) in [(&css_path, BADGE_CSS), (&test_path, TEST_SOURCE)] {
        if path.exists() && fs::read_to_string(path)? != desired {
            return Err(format!("{} already exists with different content", path.display()).into());
        }
    }

    book["preprocessor"]["habeas"]["command"] = value("habeas mdbook");
    let mut renderers = Array::new();
    renderers.push("html");
    book["preprocessor"]["habeas"]["renderers"] = value(renderers);

    let css_relative = source_path
        .join("habeas.css")
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    // {impl}`requirements.cli.setup_css`
    let additional_css = &mut book["output"]["html"]["additional-css"];
    if additional_css.is_none() {
        let mut css = Array::new();
        css.push(css_relative.as_str());
        *additional_css = value(css);
    } else if let Some(css) = additional_css.as_array_mut() {
        if !css
            .iter()
            .any(|existing| existing.as_str() == Some(css_relative.as_str()))
        {
            css.push(css_relative.as_str());
        }
    } else {
        return Err("output.html.additional-css must be an array".into());
    }

    let manifest_path = root.join("Cargo.toml");
    let manifest: DocumentMut = fs::read_to_string(&manifest_path)?.parse()?;
    // {impl}`requirements.cli.setup_local_path`
    let has_dev_dependency = manifest
        .get("dev-dependencies")
        .and_then(Item::as_table_like)
        .and_then(|table| table.get("habeas"))
        .is_some();
    if !has_dev_dependency {
        let mut command = Command::new("cargo");
        command
            .arg("add")
            .arg("--dev")
            .arg("habeas")
            .arg("--manifest-path")
            .arg(&manifest_path);
        if let Some(local_crate) = local_crate {
            command.arg("--path").arg(fs::canonicalize(local_crate)?);
        }
        let output = command.current_dir(&root).output()?;
        if !output.status.success() {
            return Err(format!(
                "cargo add habeas --dev failed:\n{}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
    }

    write_new_or_same(&css_path, BADGE_CSS)?;
    write_new_or_same(&test_path, TEST_SOURCE)?;
    fs::write(&book_path, book.to_string())?;
    // {impl}`requirements.cli.setup`
    if install_skills {
        crate::agents::install(&root)?;
    }
    println!("Habeas configured in {}", root.display());
    Ok(())
}

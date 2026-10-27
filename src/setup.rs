use std::error::Error;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use toml_edit::{Array, DocumentMut, Item, value};

const TEST_SOURCE: &str = "#[test]\nfn habeas() {\n    habeas::validate();\n}\n";
const BADGE_CSS: &str = include_str!("../md/habeas.css");

fn css_relative(source_path: &Path) -> String {
    source_path
        .join("habeas.css")
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn has_habeas_dev_dependency(manifest: &DocumentMut) -> bool {
    manifest
        .get("dev-dependencies")
        .and_then(Item::as_table_like)
        .and_then(|table| table.get("habeas"))
        .is_some()
}

pub(crate) fn project_depends_on_habeas(root: &Path) -> bool {
    fs::read_to_string(root.join("Cargo.toml"))
        .ok()
        .and_then(|source| source.parse::<DocumentMut>().ok())
        .is_some_and(|manifest| has_habeas_dev_dependency(&manifest))
}

pub(crate) fn find_project_root(cwd: &Path) -> Option<PathBuf> {
    cwd.ancestors()
        .find(|directory| {
            directory.join("Cargo.toml").is_file() && directory.join("book.toml").is_file()
        })
        .map(Path::to_path_buf)
}

// {impl}`requirements.cli.setup_project`
pub(crate) fn project_root() -> Result<PathBuf, Box<dyn Error>> {
    let cwd = std::env::current_dir()?;
    find_project_root(&cwd)
        .ok_or_else(|| "cannot find a project with Cargo.toml and book.toml".into())
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
pub(crate) fn run(local_crate: Option<&Path>) -> Result<(), Box<dyn Error>> {
    let root = project_root()?;
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

    let css_relative = css_relative(source_path);
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
    let has_dev_dependency = has_habeas_dev_dependency(&manifest);
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
    println!("Habeas configured in {}", root.display());
    Ok(())
}

// {impl}`requirements.integration.symposium_setup_hook`
pub(crate) fn is_configured(root: &Path) -> bool {
    let manifest = match fs::read_to_string(root.join("Cargo.toml"))
        .ok()
        .and_then(|source| source.parse::<DocumentMut>().ok())
    {
        Some(manifest) => manifest,
        None => return false,
    };
    if !has_habeas_dev_dependency(&manifest) {
        return false;
    }

    let book = match fs::read_to_string(root.join("book.toml"))
        .ok()
        .and_then(|source| source.parse::<DocumentMut>().ok())
    {
        Some(book) => book,
        None => return false,
    };
    let source = book
        .get("book")
        .and_then(Item::as_table_like)
        .and_then(|table| table.get("src"))
        .and_then(Item::as_str)
        .unwrap_or("src");
    let source_path = Path::new(source);
    if source.is_empty()
        || !source_path
            .components()
            .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
        || !root.join(source_path).join("SUMMARY.md").is_file()
    {
        return false;
    }

    let preprocessor = book
        .get("preprocessor")
        .and_then(Item::as_table_like)
        .and_then(|table| table.get("habeas"))
        .and_then(Item::as_table_like);
    let Some(preprocessor) = preprocessor else {
        return false;
    };
    if preprocessor.get("command").and_then(Item::as_str) != Some("habeas mdbook")
        || !preprocessor
            .get("renderers")
            .and_then(Item::as_array)
            .is_some_and(|renderers| {
                renderers.len() == 1 && renderers.get(0).and_then(|r| r.as_str()) == Some("html")
            })
    {
        return false;
    }

    let css_relative = css_relative(source_path);
    let css_list = book
        .get("output")
        .and_then(Item::as_table_like)
        .and_then(|table| table.get("html"))
        .and_then(Item::as_table_like)
        .and_then(|table| table.get("additional-css"))
        .and_then(Item::as_array);
    if !css_list.is_some_and(|css| {
        css.iter()
            .any(|entry| entry.as_str() == Some(css_relative.as_str()))
    }) {
        return false;
    }

    fs::read_to_string(root.join(source_path).join("habeas.css"))
        .is_ok_and(|content| content == BADGE_CSS)
        && fs::read_to_string(root.join("tests/habeas.rs"))
            .is_ok_and(|content| content == TEST_SOURCE)
}

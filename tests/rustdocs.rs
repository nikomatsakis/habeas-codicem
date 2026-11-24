use std::{fs, path::Path, process::Command};

use mdbook_preprocessor::{
    book::{Book, Chapter},
    config::Config,
};
use tempfile::TempDir;

const PAGE: &str = "<!doctype html><html><head><title>Native SDK Rustdoc</title><link rel=\"stylesheet\" href=\"../static.files/style.css\"></head><body>SDK API</body></html>";

fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    for directory in [
        "md",
        "target/doc/trust_but_verify",
        "target/doc/static.files",
    ] {
        fs::create_dir_all(root.path().join(directory)).unwrap();
    }
    fs::write(
        root.path().join("target/doc/trust_but_verify/index.html"),
        PAGE,
    )
    .unwrap();
    fs::write(
        root.path().join("target/doc/static.files/style.css"),
        "body { color: teal; }",
    )
    .unwrap();
    fs::write(
        root.path().join("target/doc/search-index.js"),
        "const search = ['Graph'];",
    )
    .unwrap();
    root
}

fn configuration() -> Config {
    let mut config = Config::default();
    config.book.src = "md".into();
    config
        .set("preprocessor.habeas.rustdocs.path", "target/doc")
        .unwrap();
    config
        .set("preprocessor.habeas.rustdocs.crate", "trust_but_verify")
        .unwrap();
    config
}

fn book() -> Book {
    let mut book = Book::new();
    book.push_item(Chapter::new(
        "Intro",
        "# Intro\n\n[API](rustdocs/trust_but_verify/index.html)".into(),
        "intro.md",
        Vec::new(),
    ));
    book.push_item(Chapter::new(
        "Rustdocs",
        ":::{rustdocs}\n:::\n".into(),
        "appendix/rustdocs.md",
        Vec::new(),
    ));
    book
}

// {test}`requirements.index.rustdocs`
#[test]
fn rustdocs_import_preserves_native_pages_assets_search_and_navigation() {
    let root = fixture();
    let mut book = book();
    habeas::rustdocs::preprocess(&mut book, root.path(), &configuration()).unwrap();
    let entry = book
        .chapters()
        .find(|chapter| chapter.name == "Rustdocs")
        .unwrap();
    assert_eq!(
        entry.path.as_deref(),
        Some(Path::new("rustdocs/trust_but_verify/index.html"))
    );
    assert!(entry.source_path.is_none());
    let assets = root.path().join("md/rustdocs");
    assert_eq!(
        fs::read_to_string(assets.join("trust_but_verify/index.html")).unwrap(),
        PAGE
    );
    assert_eq!(
        fs::read_to_string(assets.join("static.files/style.css")).unwrap(),
        "body { color: teal; }"
    );
    assert_eq!(
        fs::read_to_string(assets.join("search-index.js")).unwrap(),
        "const search = ['Graph'];"
    );
    assert!(
        book.chapters()
            .find(|chapter| chapter.name == "Intro")
            .unwrap()
            .content
            .contains("[API](rustdocs/trust_but_verify/index.html)")
    );
}

// {test}`requirements.index.rustdocs`
#[test]
fn rustdocs_rebuild_replaces_generated_assets_and_removes_obsolete_pages() {
    let root = fixture();
    fs::write(
        root.path().join("target/doc/trust_but_verify/old.html"),
        "obsolete",
    )
    .unwrap();
    habeas::rustdocs::preprocess(&mut book(), root.path(), &configuration()).unwrap();
    fs::remove_file(root.path().join("target/doc/trust_but_verify/old.html")).unwrap();
    fs::write(
        root.path().join("target/doc/static.files/style.css"),
        "updated",
    )
    .unwrap();
    habeas::rustdocs::preprocess(&mut book(), root.path(), &configuration()).unwrap();
    assert!(
        !root
            .path()
            .join("md/rustdocs/trust_but_verify/old.html")
            .exists()
    );
    assert_eq!(
        fs::read_to_string(root.path().join("md/rustdocs/static.files/style.css")).unwrap(),
        "updated"
    );
}

// {test}`requirements.index.rustdocs`
#[test]
fn rustdocs_import_errors_preserve_authored_assets_and_chapters() {
    let root = fixture();
    fs::create_dir_all(root.path().join("md/rustdocs")).unwrap();
    fs::write(root.path().join("md/rustdocs/author.css"), "authored").unwrap();
    let error =
        habeas::rustdocs::preprocess(&mut book(), root.path(), &configuration()).unwrap_err();
    assert!(
        error.to_string().contains("overwrite existing book assets"),
        "{error}"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("md/rustdocs/author.css")).unwrap(),
        "authored"
    );
    fs::remove_dir_all(root.path().join("md/rustdocs")).unwrap();
    let mut collision = book();
    collision.push_item(Chapter::new(
        "Authored API",
        "Keep me".into(),
        "rustdocs/trust_but_verify/index.md",
        Vec::new(),
    ));
    let error =
        habeas::rustdocs::preprocess(&mut collision, root.path(), &configuration()).unwrap_err();
    assert!(
        error.to_string().contains("overwrite book chapter"),
        "{error}"
    );
    assert!(!root.path().join("md/rustdocs").exists());
}

// {test}`requirements.index.rustdocs`
#[test]
fn rustdocs_rejects_collisions_with_configured_assets_and_redirects() {
    let root = fixture();
    let mut config = configuration();
    config.set("output.html.hash-files", false).unwrap();
    config
        .set(
            "output.html.additional-js",
            vec!["./rustdocs/search-index.js"],
        )
        .unwrap();
    let error = habeas::rustdocs::preprocess(&mut book(), root.path(), &config).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("overwrite book asset rustdocs/search-index.js"),
        "{error}"
    );
    assert!(!root.path().join("md/rustdocs").exists());

    config = configuration();
    config
        .set(
            "output.html.redirect",
            std::collections::HashMap::from([(
                "/rustdocs/trust_but_verify/index.html",
                "intro.html",
            )]),
        )
        .unwrap();
    let error = habeas::rustdocs::preprocess(&mut book(), root.path(), &config).unwrap_err();
    assert!(
        error.to_string().contains("overwrite book redirect"),
        "{error}"
    );

    config = configuration();
    fs::create_dir_all(root.path().join("rustdocs")).unwrap();
    fs::write(root.path().join("rustdocs/custom.js"), "authored script").unwrap();
    config
        .set("output.html.additional-js", vec!["rustdocs/custom.js"])
        .unwrap();
    let hash = {
        use sha2::{Digest, Sha256};
        Sha256::digest(b"authored script")[..4]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let hashed = format!("custom-{hash}.js");
    // The native site contains an asset at the renderer's hashed output path.
    fs::write(root.path().join("target/doc").join(&hashed), "collision").unwrap();
    let error = habeas::rustdocs::preprocess(&mut book(), root.path(), &config).unwrap_err();
    assert!(
        error.to_string().contains("overwrite book asset"),
        "{error}"
    );
}

// {test}`requirements.index.rustdocs`
#[test]
fn rustdocs_import_reports_unsupported_entries_and_overlapping_directories() {
    let root = fixture();
    let mut config = configuration();
    config
        .set("preprocessor.habeas.rustdocs.path", "md")
        .unwrap();
    fs::create_dir_all(root.path().join("md/trust_but_verify")).unwrap();
    fs::write(root.path().join("md/trust_but_verify/index.html"), PAGE).unwrap();
    let error = habeas::rustdocs::preprocess(&mut book(), root.path(), &config).unwrap_err();
    assert!(
        error.to_string().contains("must not contain each other"),
        "{error}"
    );

    fs::write(
        root.path().join("target/doc/.habeas-rustdocs-owner"),
        "collision",
    )
    .unwrap();
    let error =
        habeas::rustdocs::preprocess(&mut book(), root.path(), &configuration()).unwrap_err();
    assert!(error.to_string().contains("ownership marker"), "{error}");
    fs::remove_file(root.path().join("target/doc/.habeas-rustdocs-owner")).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            "trust_but_verify/index.html",
            root.path().join("target/doc/link"),
        )
        .unwrap();
        let error =
            habeas::rustdocs::preprocess(&mut book(), root.path(), &configuration()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("not a regular file or directory"),
            "{error}"
        );
    }
}

// {test}`requirements.index.rustdocs`
#[test]
#[cfg(unix)]
fn rustdocs_unreadable_site_file_reports_its_path_without_replacing_assets() {
    use std::os::unix::fs::PermissionsExt;
    let root = fixture();
    habeas::rustdocs::preprocess(&mut book(), root.path(), &configuration()).unwrap();
    let input = root.path().join("target/doc/static.files/style.css");
    fs::set_permissions(&input, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read(&input).is_ok() {
        return;
    }
    let error =
        habeas::rustdocs::preprocess(&mut book(), root.path(), &configuration()).unwrap_err();
    fs::set_permissions(&input, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(error.to_string().contains("import Rustdoc file"), "{error}");
    assert!(error.to_string().contains("style.css"), "{error}");
    assert_eq!(
        fs::read_to_string(root.path().join("md/rustdocs/static.files/style.css")).unwrap(),
        "body { color: teal; }"
    );
}

// {test}`requirements.index.rustdocs`
#[test]
fn rustdocs_missing_inputs_and_configuration_report_readable_errors() {
    let root = fixture();
    let error =
        habeas::rustdocs::preprocess(&mut book(), root.path(), &Config::default()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("requires [preprocessor.habeas.rustdocs]"),
        "{error}"
    );
    let mut config = configuration();
    config
        .set("preprocessor.habeas.rustdocs.path", "missing")
        .unwrap();
    let error = habeas::rustdocs::preprocess(&mut book(), root.path(), &config).unwrap_err();
    assert!(
        error.to_string().contains("cannot read Rustdoc directory"),
        "{error}"
    );
    config = configuration();
    config
        .set("preprocessor.habeas.rustdocs.crate", "missing")
        .unwrap();
    let error = habeas::rustdocs::preprocess(&mut book(), root.path(), &config).unwrap_err();
    assert!(error.to_string().contains("crate landing page"), "{error}");
    config
        .set("preprocessor.habeas.rustdocs.crate", "../outside")
        .unwrap();
    let error = habeas::rustdocs::preprocess(&mut book(), root.path(), &config).unwrap_err();
    assert!(
        error.to_string().contains("crate must name a Rust crate"),
        "{error}"
    );
    let mut ordinary = Book::new();
    ordinary.push_item(Chapter::new(
        "Normal",
        "# Normal".into(),
        "normal.md",
        Vec::new(),
    ));
    habeas::rustdocs::preprocess(&mut ordinary, root.path(), &Config::default()).unwrap();
    assert!(!root.path().join("md/rustdocs").exists());
}

// {test}`requirements.index.rustdocs`
#[test]
fn rustdocs_directives_are_whole_closed_empty_chapters() {
    let root = fixture();
    for content in [
        "```text\n:::{rustdocs}\n:::\n```",
        ":::{rustdocs}\nnonempty\n:::",
        ":::{rustdocs}",
        ":::{rustdocs}\n::",
        "::::{rustdocs}\n:::",
        "# Context\n\n:::{rustdocs}\n:::",
        "    :::{rustdocs}\n    :::",
        "\t:::{rustdocs}\n\t:::",
        " \n    :::{rustdocs}\n    :::\n",
    ] {
        let mut book = Book::new();
        book.push_item(Chapter::new(
            "Literal",
            content.into(),
            "literal.md",
            Vec::new(),
        ));
        habeas::rustdocs::preprocess(&mut book, root.path(), &Config::default()).unwrap();
        assert_eq!(book.chapters().next().unwrap().content, content);
    }
    let mut book = Book::new();
    book.push_item(Chapter::new(
        "Rustdocs",
        " \n   ::::{rustdocs}\n\n   :::::\n \n".into(),
        "rustdocs.md",
        Vec::new(),
    ));
    habeas::rustdocs::preprocess(&mut book, root.path(), &configuration()).unwrap();
    assert_eq!(
        book.chapters().next().unwrap().path.as_deref(),
        Some(Path::new("rustdocs/trust_but_verify/index.html"))
    );
}

// {test}`requirements.index.rustdocs`
#[test]
fn rustdocs_mdbook_build_publishes_native_site_and_prefix_safe_links() {
    let root = fixture();
    fs::write(
        root.path().join("target/doc/static.files/LICENSE.md"),
        "Font licence\n",
    )
    .unwrap();
    let error =
        habeas::rustdocs::preprocess(&mut book(), root.path(), &configuration()).unwrap_err();
    assert!(
        error.to_string().contains("configure [output.habeas]"),
        "{error}"
    );
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let command = format!(
        "'{}' mdbook",
        env!("CARGO_BIN_EXE_habeas").replace('\'', "'\\''")
    );
    let renderer = format!(
        "'{}' mdbook render",
        env!("CARGO_BIN_EXE_habeas").replace('\'', "'\\''")
    );
    fs::write(root.path().join("book.toml"), format!(
        "[book]\nsrc = 'md'\n[preprocessor.habeas]\ncommand = {command:?}\nrenderers = ['habeas']\n[preprocessor.habeas.rustdocs]\npath = 'target/doc'\ncrate = 'trust_but_verify'\n[output.habeas]\ncommand = {renderer:?}\n[output.habeas.html]\nsite-url = '/project-prefix/'\n"
    )).unwrap();
    fs::write(
        root.path().join("md/SUMMARY.md"),
        "# Summary\n\n- [Intro](intro.md)\n\n# Appendix\n\n- [Rustdocs](rustdocs.md)\n",
    )
    .unwrap();
    fs::write(
        root.path().join("md/intro.md"),
        "# Intro\n\n[API](rustdocs/trust_but_verify/index.html)\n",
    )
    .unwrap();
    fs::write(root.path().join("md/rustdocs.md"), ":::{rustdocs}\n:::\n").unwrap();
    let output = Command::new("mdbook")
        .arg("build")
        .current_dir(root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rendered = root.path().join("book");
    assert_eq!(
        fs::read_to_string(rendered.join("rustdocs/trust_but_verify/index.html")).unwrap(),
        PAGE
    );
    assert!(rendered.join("rustdocs/static.files/style.css").is_file());
    assert_eq!(
        fs::read_to_string(rendered.join("rustdocs/static.files/LICENSE.md")).unwrap(),
        "Font licence\n"
    );
    assert!(rendered.join("rustdocs/search-index.js").is_file());
    let intro = fs::read_to_string(rendered.join("intro.html")).unwrap();
    assert!(
        intro.contains("href=\"rustdocs/trust_but_verify/index.html\""),
        "{intro}"
    );
    let toc = fs::read_to_string(rendered.join("toc.html")).unwrap();
    assert!(
        toc.contains("href=\"rustdocs/trust_but_verify/index.html\" target=\"_parent\""),
        "{toc}"
    );
}

// {test}`requirements.index.rustdocs.publication`
#[test]
fn rustdocs_workspace_build_generates_and_refreshes_the_import() {
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = 'trust-but-verify'\nversion = '0.1.0'\nedition = '2024'\n",
    )
    .unwrap();
    let lib = root.path().join("src/lib.rs");
    fs::write(&lib, "pub fn first() {}\n").unwrap();
    assert!(
        Command::new("cargo")
            .args(["generate-lockfile", "--offline"])
            .current_dir(root.path())
            .status()
            .unwrap()
            .success()
    );
    let mut config = configuration();
    config
        .set("output.habeas.command", "habeas mdbook render")
        .unwrap();
    config
        .set("preprocessor.habeas.rustdocs.build-workspace", true)
        .unwrap();
    config
        .set("preprocessor.habeas.rustdocs.path", "custom/doc")
        .unwrap();
    let imported = root.path().join("md/rustdocs/trust_but_verify");
    habeas::rustdocs::preprocess(&mut book(), root.path(), &config).unwrap();
    assert!(imported.join("fn.first.html").is_file());
    fs::write(&lib, "pub fn second() {}\n").unwrap();
    habeas::rustdocs::preprocess(&mut book(), root.path(), &config).unwrap();
    assert!(imported.join("fn.second.html").is_file());
    fs::write(&lib, "pub fn").unwrap();
    let error = habeas::rustdocs::preprocess(&mut book(), root.path(), &config).unwrap_err();
    assert!(error.to_string().contains("cargo doc failed"), "{error}");
    assert!(imported.join("fn.second.html").is_file());
}

// {test}`requirements.index.rustdocs.publication`
#[test]
fn rustdocs_ci_publishes_the_book_with_workspace_generation_enabled() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workflow: serde_json::Value =
        serde_saphyr::from_str(&fs::read_to_string(root.join(".github/workflows/ci.yml")).unwrap())
            .unwrap();
    let steps = workflow["jobs"]["test-and-build"]["steps"]
        .as_array()
        .unwrap();
    let build = steps
        .iter()
        .position(|step| step["run"] == "mdbook build")
        .unwrap();
    let upload = steps
        .iter()
        .position(|step| step["uses"] == "actions/upload-pages-artifact@v4")
        .unwrap();
    assert!(build < upload);
    assert_eq!(steps[upload]["with"]["path"], "book");
    let summary = fs::read_to_string(root.join("md/SUMMARY.md")).unwrap();
    let appendix = summary.split_once("# Appendix").unwrap().1;
    assert!(appendix.contains("[Rustdocs](./rustdocs.md)"));
    assert_eq!(
        fs::read_to_string(root.join("md/rustdocs.md"))
            .unwrap()
            .trim(),
        ":::{rustdocs}\n:::"
    );
    let config: toml::Value =
        toml::from_str(&fs::read_to_string(root.join("book.toml")).unwrap()).unwrap();
    assert_eq!(
        config["preprocessor"]["habeas"]["rustdocs"]["build-workspace"].as_bool(),
        Some(true)
    );
    assert_eq!(
        config["preprocessor"]["habeas"]["rustdocs"]["crate"].as_str(),
        Some("trust_but_verify")
    );
    assert_eq!(
        config["preprocessor"]["habeas"]["rustdocs"]["path"].as_str(),
        Some("target/doc")
    );
    assert_eq!(
        config["output"]["habeas"]["command"].as_str(),
        Some("./target/release/habeas mdbook render")
    );
}

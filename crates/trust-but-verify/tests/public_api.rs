use regex::Regex;
use std::{collections::BTreeSet, fs, path::Path, process::Command};

fn specified_exports() -> BTreeSet<(String, String)> {
    let chapter = include_str!("../../../md/requirements/sdk/public-api.md");
    let requirement = chapter
        .split_once(":::{req} exports\n")
        .unwrap()
        .1
        .split_once("\n:::")
        .unwrap()
        .0;
    let entry = Regex::new(
        r"^( *)- `(crate|mod|struct|enum|trait|type|fn|const|static|union|macro) (\w+)`$",
    )
    .unwrap();
    let mut parents: Vec<(String, String)> = Vec::new();
    let mut exports = BTreeSet::new();
    for line in requirement
        .lines()
        .filter(|line| line.trim_start().starts_with("- "))
    {
        let captures = entry
            .captures(line)
            .expect("Malformed public API inventory entry");
        let indentation = captures[1].len();
        assert_eq!(
            indentation % 2,
            0,
            "Inventory indentation must use two spaces"
        );
        let depth = indentation / 2;
        assert!(depth <= parents.len(), "Inventory skips a parent");
        parents.truncate(depth);
        if depth > 0 {
            assert!(
                matches!(
                    parents.last().map(|(_, kind)| kind.as_str()),
                    Some("crate" | "mod")
                ),
                "Only crates and modules can contain inventory entries"
            );
        } else {
            assert_eq!(&captures[2], "crate", "The inventory root must be a crate");
        }
        parents.push((captures[3].to_owned(), captures[2].to_owned()));
        assert!(
            exports.insert((
                parents
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>()
                    .join("::"),
                captures[2].to_owned()
            )),
            "Duplicate inventory entry"
        );
    }
    assert!(
        !exports.is_empty(),
        "The requirement must enumerate exports"
    );
    exports
}

fn documented_exports(root: &Path) -> BTreeSet<(String, String)> {
    let tables = Regex::new(r#"(?s)<dl class="item-table(?: [^"]*)?">(.*?)</dl>"#).unwrap();
    let links = Regex::new(r#"(?s)<a class="(\w+)" href="([^"]+)"[^>]*>(.*?)</a>"#).unwrap();
    let mut pending = vec![("trust_but_verify".to_owned(), "index.html".to_owned())];
    let mut exports = BTreeSet::from([("trust_but_verify".to_owned(), "crate".to_owned())]);
    while let Some((module, page)) = pending.pop() {
        let html = fs::read_to_string(root.join(&page)).unwrap();
        let directory = Path::new(&page).parent().unwrap();
        let mut found_table = false;
        for table in tables.captures_iter(&html) {
            found_table = true;
            for link in links.captures_iter(&table[1]) {
                let module_item = match &link[1] {
                    "mod" => true,
                    "struct" | "enum" | "trait" | "type" | "fn" | "constant" | "static"
                    | "union" | "macro" => false,
                    other => panic!("Unrecognized rustdoc export category {other} in {page}"),
                };
                // Rustdoc inserts word-break hints into long identifiers.
                // Reject other markup rather than dropping a public item.
                let name = link[3].replace("<wbr>", "");
                assert!(
                    !name.is_empty() && name.chars().all(|c| c == '_' || c.is_alphanumeric()),
                    "Unrecognized rustdoc export name {name:?} in {page}"
                );
                let destination = directory.join(&link[2]);
                assert!(
                    destination.is_relative(),
                    "Rustdoc export link is not local"
                );
                assert!(
                    root.join(&destination).is_file(),
                    "Missing rustdoc export page"
                );
                let destination = destination
                    .to_str()
                    .unwrap()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                let path = format!("{module}::{name}");
                if module_item {
                    pending.push((path.clone(), destination.clone()));
                }
                assert!(
                    exports.insert((
                        path,
                        if &link[1] == "constant" {
                            "const"
                        } else {
                            &link[1]
                        }
                        .to_owned()
                    )),
                    "Duplicate rustdoc export"
                );
            }
        }
        assert!(found_table, "No rustdoc export table found in {page}");
    }
    exports
}

// {test}`requirements.sdk.public-api.exports`
// {test}`requirements.sdk.public_api`
#[test]
fn public_exports_match_the_appendix() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let target = workspace.join("target");
    // Use stable rustdoc's published HTML to check paths and item kinds. Always
    // generate it for this source tree rather than accepting an old book copy.
    let output = Command::new(env!("CARGO"))
        .current_dir(workspace)
        .args([
            "doc",
            "--locked",
            "--offline",
            "--no-deps",
            "-p",
            "trust-but-verify",
        ])
        .env("CARGO_TARGET_DIR", &target)
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTDOCFLAGS")
        .env_remove("CARGO_ENCODED_RUSTDOCFLAGS")
        .env_remove("LLVM_PROFILE_FILE")
        .output()
        .expect("Run cargo doc for the SDK inventory");
    assert!(
        output.status.success(),
        "Generating SDK rustdoc failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let specified = specified_exports();
    let documented = documented_exports(&target.join("doc/trust_but_verify"));
    let missing = specified.difference(&documented).collect::<Vec<_>>();
    let unexpected = documented.difference(&specified).collect::<Vec<_>>();
    assert!(
        missing.is_empty() && unexpected.is_empty(),
        "SDK export inventory differs from rustdoc.\nMissing: {missing:#?}\nUnexpected: {unexpected:#?}"
    );
}

// {test}`requirements.sdk.public_api`
#[cfg(feature = "default-languages")]
#[test]
fn sdk_has_no_public_reexports() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![source_root];
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .unwrap();
    let mut reexports = Vec::new();
    while let Some(path) = files.pop() {
        if path.is_dir() {
            files.extend(
                fs::read_dir(&path)
                    .unwrap()
                    .map(|entry| entry.unwrap().path()),
            );
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
            continue;
        }
        let source = fs::read_to_string(&path).unwrap();
        let tree = parser.parse(&source, None).unwrap();
        assert!(
            !tree.root_node().has_error(),
            "Cannot parse {}",
            path.display()
        );
        let mut nodes = vec![tree.root_node()];
        while let Some(node) = nodes.pop() {
            let mut cursor = node.walk();
            if node.kind() == "use_declaration"
                && node.named_children(&mut cursor).any(|child| {
                    child.kind() == "visibility_modifier"
                        && child.utf8_text(source.as_bytes()).unwrap() == "pub"
                })
            {
                reexports.push(format!(
                    "{}:{}",
                    path.display(),
                    node.start_position().row + 1
                ));
            }
            nodes.extend(node.named_children(&mut cursor));
        }
    }
    assert!(reexports.is_empty(), "Public SDK reexports: {reexports:#?}");
}

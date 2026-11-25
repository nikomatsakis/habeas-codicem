use regex::Regex;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    process::Command,
};

const INVENTORY: &str = include_str!("../../../md/requirements/sdk/public-api.md");

type Exports = BTreeSet<(String, String)>;

struct DeclarationLink<'a> {
    chapter: &'a str,
    requirement: &'a str,
    members: BTreeSet<String>,
    linked_methods: Vec<(&'a str, &'a str, &'a str)>,
}

fn declaration_target(target: &str) -> (&str, &str) {
    let (chapter, fragment) = target
        .split_once('#')
        .expect("declaration link with an anchor");
    assert_ne!(
        chapter, "public-api.md",
        "declarations belong in narrative chapters"
    );
    let chapter_id = chapter.strip_suffix(".md").unwrap();
    let namespace = match chapter_id {
        "../sdk" => "requirements.sdk".to_owned(),
        relative => format!("requirements.sdk.{}", relative.replace('/', ".")),
    };
    let requirement = fragment
        .strip_prefix(&format!("habeas-req-{namespace}."))
        .unwrap_or_else(|| panic!("Invalid requirement anchor for {target}"));
    (chapter, requirement)
}

fn specified_exports() -> (Exports, BTreeMap<String, DeclarationLink<'static>>) {
    let requirement = INVENTORY
        .split_once(":::{req} exports\n")
        .unwrap()
        .1
        .split_once("\n:::")
        .unwrap()
        .0;
    // Every annotation links to the TBV overview or a chapter under sdk/.
    let sdk_link = r"\[[^]]+\]\((?:\.\./sdk|[a-z0-9_-]+(?:/[a-z0-9_-]+)*)\.md(?:#[\w.-]+)?\)";
    let entry = Regex::new(&format!(
        r"^( *)- `(crate|mod|struct|enum|trait|type|fn|const|static|union|macro) (\w+)` — {sdk_link}(?: · {sdk_link})*$"
    ))
    .unwrap();
    let mut declarations = BTreeMap::new();
    let mut parents: Vec<(String, String)> = Vec::new();
    let mut exports = BTreeSet::new();
    let member = Regex::new(r"^( *)- `(\w+(?:\(\))?)`(?: — \[Declaration\]\(([^)]+)\))?$").unwrap();
    let mut bullets = requirement
        .lines()
        .filter(|line| line.trim_start().starts_with("- "))
        .peekable();
    while let Some(line) = bullets.next() {
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
        let path = parents
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join("::");
        if !matches!(&captures[2], "mod" | "crate") {
            let (chapter, requirement) = declaration_target(
                line.split_once(" — [Declaration](")
                    .and_then(|(_, rest)| rest.split_once(')'))
                    .unwrap_or_else(|| panic!("Missing declaration link for {path}"))
                    .0,
            );
            let mut members = BTreeSet::new();
            let mut linked_methods = Vec::new();
            while let Some(item) = bullets.next_if(|line| member.is_match(line)) {
                let item = member.captures(item).unwrap();
                assert_eq!(
                    item[1].len(),
                    indentation + 2,
                    "Member must nest under {path}"
                );
                let name = item.get(2).unwrap().as_str();
                if let Some(target) = item.get(3) {
                    let method = name.strip_suffix("()").expect("linked method name");
                    let (chapter, requirement) = declaration_target(target.as_str());
                    linked_methods.push((method, chapter, requirement));
                }
                assert!(
                    members.insert(name.to_owned()),
                    "Duplicate member {path}::{name}"
                );
            }
            declarations.insert(
                path.clone(),
                DeclarationLink {
                    chapter,
                    requirement,
                    members,
                    linked_methods,
                },
            );
        }
        assert!(
            exports.insert((path, captures[2].to_owned())),
            "Duplicate inventory entry"
        );
    }
    assert!(
        !exports.is_empty(),
        "The requirement must enumerate exports"
    );
    (exports, declarations)
}

// Rustdoc permits a final comma in a where clause. Keep commas in tuple types.
fn normalize(part: &str) -> String {
    static COMMA: std::sync::LazyLock<Regex> =
        std::sync::LazyLock::new(|| Regex::new(r",\s*([;{])").unwrap());
    static TOKEN: std::sync::LazyLock<Regex> =
        std::sync::LazyLock::new(|| Regex::new(r"\w+|[^\s]").unwrap());
    let part = COMMA.replace_all(part, "$1");
    TOKEN
        .find_iter(part.trim().trim_end_matches(';'))
        .map(|m| m.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Compare Rustdoc's item declaration and each inherent member against its
/// owning requirement. Keying members by their impl bounds permits regrouping
/// or reordering methods without changing the requirement.
fn signature_parts(text: &str) -> BTreeSet<(String, String)> {
    let member =
        Regex::new(r"(?m)^\s*pub (?:async |unsafe |const |extern )*(?:fn |const |type )").unwrap();
    let mut blocks = text.split("\nimpl");
    let mut signatures = BTreeSet::from([(String::new(), normalize(blocks.next().unwrap()))]);
    for block in blocks {
        let (bounds, body) = block.split_once('{').expect("inherent impl body");
        let body = body
            .trim_end()
            .strip_suffix('}')
            .expect("impl closing brace");
        let starts: Vec<_> = member
            .find_iter(body)
            .map(|m| m.start())
            .chain([body.len()])
            .collect();
        assert!(
            starts.len() > 1,
            "empty or unrecognized inherent impl: {block}"
        );
        for span in starts.windows(2) {
            signatures.insert((
                normalize(bounds.trim_end().trim_end_matches(',')),
                normalize(&body[span[0]..span[1]]),
            ));
        }
    }
    signatures
}

// {test}`requirements.sdk.public_api`
#[test]
fn member_order_is_free_but_field_and_method_types_are_committed() {
    let contract = "pub struct Example { pub field: T }\nimpl Example {\npub fn get(&self) -> T;\npub fn clear(&mut self);\n}";
    let expected = signature_parts(contract);
    let reordered = contract.replace(
        "pub fn get(&self) -> T;\npub fn clear(&mut self);",
        "pub fn clear(&mut self);\npub fn get(&self) -> T;",
    );
    assert_eq!(expected, signature_parts(&reordered));
    for (old, new) in [
        ("field: T", "field: (T,)"),
        ("-> T", "-> Option<T>"),
        ("&mut self", "&self"),
    ] {
        assert_ne!(expected, signature_parts(&contract.replace(old, new)));
    }
    assert_ne!(normalize("(T,)"), normalize("(T)"));
}

struct RustdocSignatures {
    tags: Regex,
    headers: Regex,
    error_path: Regex,
    member_id: Regex,
}

impl RustdocSignatures {
    fn new() -> Self {
        Self {
            member_id: Regex::new(r#" id="(structfield|method|tymethod|associatedtype|associatedconstant)\.([^"]+)""#).unwrap(),
            tags: Regex::new(r"<[^>]+>").unwrap(),
            headers: Regex::new(r#"(?s)<h([34]) class="code-header">(.*?)</h[34]>"#).unwrap(),
            error_path: Regex::new(r#"<a [^>]*title="struct ((?:std|core)::io::error|serde_json::error)::Error"[^>]*>Error</a>"#).unwrap(),
        }
    }

    fn code(&self, html: &str) -> String {
        let qualified = self
            .error_path
            .replace_all(html, |c: &regex::Captures<'_>| {
                if &c[1] == "serde_json::error" {
                    "serde_json::Error"
                } else {
                    "std::io::Error"
                }
            });
        let qualified = qualified.replace(r#"<div class="where">"#, " ");
        let text = self.tags.replace_all(&qualified, "");
        text.lines()
            .filter(|line| !line.trim_start().starts_with("// "))
            .collect::<Vec<_>>()
            .join("\n")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&amp;", "&")
    }

    fn member_names(&self, page: &str) -> BTreeSet<String> {
        page.split("<h2 id=\"")
            .skip(1)
            .filter(|section| {
                let (id, _) = section.split_once('"').unwrap();
                matches!(
                    id,
                    "fields"
                        | "implementations"
                        | "required-methods"
                        | "provided-methods"
                        | "required-associated-types"
                        | "provided-associated-types"
                        | "required-associated-consts"
                        | "provided-associated-consts"
                )
            })
            .flat_map(|section| {
                self.member_id.captures_iter(section).map(|c| match &c[1] {
                    "method" | "tymethod" => format!("{}()", &c[2]),
                    _ => c[2].to_owned(),
                })
            })
            .collect()
    }

    fn trait_impls(&self, page: &str) -> BTreeSet<String> {
        let mut result = BTreeSet::new();
        // The trait's page lists implementations for foreign types, e.g. Context
        // for str. Implementations for SDK types live on the implementing type.
        for kind in ["trait-implementations", "foreign-impls"] {
            let start = format!(r#"<h2 id="{kind}""#);
            let Some((_, rest)) = page.split_once(&start) else {
                continue;
            };
            let section = rest.split("<h2 ").next().unwrap();
            for block in section.split(r#"<section id="impl-"#).skip(1) {
                let mut headers = self.headers.captures_iter(block);
                let first = headers.next().expect("rustdoc trait implementation header");
                let mut signature = self.code(&first[2]).trim_end_matches(',').to_owned();
                // Derived PartialEq adds this compiler marker alongside PartialEq.
                if signature.contains(" StructuralPartialEq for ") {
                    continue;
                }
                let associated: Vec<_> = headers
                    .map(|h| self.code(&h[2]))
                    .filter(|h| h.starts_with("type ") || h.starts_with("const "))
                    .collect();
                if !associated.is_empty() {
                    signature.push_str(" { ");
                    signature.push_str(&associated.join("; "));
                    signature.push_str("; }");
                }
                result.insert(normalize(&signature));
            }
        }
        result
    }

    fn declaration(&self, page: &str) -> String {
        let mut code = self.code(
            page.split_once(r#"<pre class="rust item-decl"><code>"#)
                .expect("rustdoc declaration")
                .1
                .split_once("</code></pre>")
                .unwrap()
                .0,
        );
        if let Some((_, section)) = page.split_once(r#"<h2 id="implementations""#) {
            let section = section.split("<h2 ").next().unwrap();
            let mut in_impl = false;
            for h in self.headers.captures_iter(section) {
                if &h[1] == "3" {
                    if in_impl {
                        code.push_str("\n}");
                    }
                    code.push('\n');
                    code.push_str(&self.code(&h[2]));
                    code.push_str(" {\n");
                    in_impl = true;
                } else {
                    assert!(in_impl, "rustdoc member without impl");
                    code.push_str(&self.code(&h[2]));
                    code.push_str(";\n");
                }
            }
            assert!(in_impl, "unrecognized rustdoc implementations section");
            code.push('}');
        }
        code
    }
}

fn check_declaration(
    chapters: &Path,
    link: &DeclarationLink<'_>,
    id: &str,
    page: &str,
    rustdoc: &RustdocSignatures,
) {
    let spec = fs::read_to_string(chapters.join(link.chapter))
        .unwrap_or_else(|err| panic!("Reading declaration for {id}: {err}"));
    let marker = format!(":::{{req}} {}\n", link.requirement);
    let body = spec
        .split_once(&marker)
        .unwrap_or_else(|| panic!("Missing API requirement {id}"))
        .1
        .split_once("\n:::")
        .unwrap()
        .0;
    let (description, listing) = body
        .split_once("```rust,ignore\n")
        .unwrap_or_else(|| panic!("Missing Rust declaration for {id}"));
    assert!(
        description
            .lines()
            .any(|line| !line.trim().is_empty() && !line.starts_with(":")),
        "Missing contract for {id}"
    );
    let (declaration, rest) = listing.split_once("\n```").unwrap();
    let impls = rest
        .split_once("```rust,ignore\n")
        .map(|(_, code)| code.split_once("\n```").unwrap().0)
        .unwrap_or("");
    let expected_impls = impls
        .split("\nimpl")
        .filter(|s| !s.is_empty())
        .map(|s| normalize(&format!("impl{}", s.strip_prefix("impl").unwrap_or(s))))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        expected_impls,
        rustdoc.trait_impls(page),
        "Trait implementations for {id}"
    );
    assert_eq!(
        link.members,
        rustdoc.member_names(page),
        "Indexed members for {id}"
    );
    let mut declaration = declaration.to_owned();
    for (method, chapter, requirement) in &link.linked_methods {
        let spec = fs::read_to_string(chapters.join(chapter)).unwrap();
        let marker = format!(":::{{req}} {requirement}\n");
        let (_, section) = spec
            .split_once(&marker)
            .unwrap_or_else(|| panic!("Missing declaration for {id}::{method}"));
        let body = section.split_once("\n:::").unwrap().0;
        let (description, listing) = body.split_once("```rust,ignore\n").unwrap();
        let signature = listing.split_once("\n```").unwrap().0;
        assert!(
            description
                .lines()
                .any(|line| !line.is_empty() && !line.starts_with(':'))
        );
        assert!(signature.starts_with("impl") && signature.contains(&format!("pub fn {method}")));
        declaration.push('\n');
        declaration.push_str(signature);
    }
    let expected = signature_parts(&declaration);
    let published = rustdoc.declaration(page);
    if published.contains("pub struct ") && page.contains(r#" id="structfield."#) {
        assert!(
            published.contains("#[non_exhaustive]"),
            "{id} exposes public fields and must be non-exhaustive"
        );
    }
    let actual = signature_parts(&published);
    let missing: Vec<_> = expected.difference(&actual).collect();
    let unexpected: Vec<_> = actual.difference(&expected).collect();
    assert!(
        missing.is_empty() && unexpected.is_empty(),
        "Public API contract differs for {id}.\nOnly in requirements: {missing:#?}\nOnly in Rustdoc: {unexpected:#?}"
    );
}

fn documented_exports(
    root: &Path,
    chapters: &Path,
    declarations: &BTreeMap<String, DeclarationLink<'_>>,
) -> Exports {
    let signatures = RustdocSignatures::new();
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
                } else {
                    let page = fs::read_to_string(root.join(destination)).unwrap();
                    let link = declarations
                        .get(&path)
                        .unwrap_or_else(|| panic!("Missing inventory declaration for {path}"));
                    check_declaration(chapters, link, &path, &page, &signatures);
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
    let (specified, declarations) = specified_exports();
    let documented = documented_exports(
        &target.join("doc/trust_but_verify"),
        &workspace.join("md/requirements/sdk"),
        &declarations,
    );
    let missing = specified.difference(&documented).collect::<Vec<_>>();
    let unexpected = documented.difference(&specified).collect::<Vec<_>>();
    assert!(
        missing.is_empty() && unexpected.is_empty(),
        "SDK export inventory differs from rustdoc.\nMissing: {missing:#?}\nUnexpected: {unexpected:#?}"
    );
}

// {test}`requirements.sdk.public_api`
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

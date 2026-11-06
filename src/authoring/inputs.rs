use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::access::SourceAccess;
use crate::evidence::{Declaration, SourceReference, collect_references, load_definitions};
use crate::inputs::{InputSelector, read_project_file};
use crate::{DefinitionKind, ReferenceKind};

pub(crate) enum AssessmentKind {
    Definition(DefinitionKind),
}

impl AssessmentKind {
    // {impl}`requirements.authoring.usage_storage`
    // {impl}`requirements.authoring.initial_prompt`
    pub(crate) fn noun(&self) -> &'static str {
        match self {
            Self::Definition(kind) => kind.noun(),
        }
    }
}

pub(crate) struct AssessmentSources {
    pub(crate) root: PathBuf,
    pub(crate) access: SourceAccess,
    pub(crate) subject_name: String,
    pub(crate) subject_kind: AssessmentKind,
    pub(crate) subject_text: String,
    mandatory: Vec<Value>,
    recorded: Vec<Value>,
}

impl AssessmentSources {
    // {impl}`requirements.authoring.invocation`
    // {impl}`requirements.authoring.mandatory_inputs`
    // {impl}`requirements.authoring_input`
    // {impl}`requirements.examples.authoring_inputs`
    // {impl}`requirements.references.authoring_inputs`
    // {impl}`requirements.definitions.authoring_inputs`
    // {impl}`requirements.ci.authoring_inputs`
    // {impl}`requirements.authoring.source_tools`
    // {impl}`requirements.authoring.read_denial`
    pub(crate) fn prepare(root: &Path, subject_name: &str) -> io::Result<Self> {
        let root = fs::canonicalize(root)?;
        let access = SourceAccess::load(&root)?;
        let definitions = load_definitions(&root)?;
        let subject = definitions
            .get(subject_name)
            .filter(|definition| definition.kind != DefinitionKind::Term)
            .ok_or_else(|| {
                io::Error::other(format!(
                    "{subject_name}: no matching requirement or named example"
                ))
            })?;
        let mut selectors = vec![InputSelector::Requirements {
            requirements: subject_name.to_owned(),
        }];

        for reference in crate::reference_roles(&subject.text, true) {
            if reference.kind == ReferenceKind::Term {
                continue;
            }
            if !definitions
                .get(reference.name)
                .is_some_and(|definition| definition.kind != DefinitionKind::Term)
            {
                return Err(io::Error::other(format!(
                    "{subject_name}: referenced subject {} does not exist",
                    reference.name
                )));
            }
            // Only `{cite}` leaves the subject; its `{impl}` and `{test}`
            // references point into it from the definitions it implements or tests.
            if reference.kind != ReferenceKind::Cite {
                continue;
            }
            insert_selector(
                &mut selectors,
                InputSelector::Requirements {
                    requirements: reference.name.to_owned(),
                },
            );
        }

        let mut errors = Vec::new();
        let references = collect_references(&root, &mut errors);
        if !errors.is_empty() {
            return Err(io::Error::other(errors.join("\n")));
        }
        // `{impl}` and `{test}` point from the subject to their sources; `{cite}`
        // sites point into the subject and are discoverable with `references`.
        for reference in references.iter().filter(|reference| {
            reference.name == subject_name
                && matches!(reference.kind, ReferenceKind::Impl | ReferenceKind::Test)
        }) {
            access.file(&root, &reference.file)?;
            insert_selector(
                &mut selectors,
                reference_selector(&root, &definitions, reference)?,
            );
        }

        // Existing evidence only decides whether to reassess; its inputs are
        // not carried forward.
        let mut mandatory = Vec::new();
        for selector in selectors {
            if !generated_evidence_selector(&root, &access, &selector) {
                mandatory.push(source_input(&root, &access, &definitions, &selector)?);
            }
        }
        let dependencies = definition_inputs(
            &root,
            &access,
            &definitions,
            &mandatory,
            Some(&subject.text),
        )?;
        for input in dependencies {
            if !mandatory.contains(&input) {
                mandatory.push(input);
            }
        }
        Ok(Self {
            root,
            access,
            subject_name: subject_name.to_owned(),
            subject_kind: AssessmentKind::Definition(subject.kind),
            subject_text: subject.text.clone(),
            mandatory,
            recorded: Vec::new(),
        })
    }

    // {impl}`requirements.authoring.mandatory_inputs`
    pub(crate) fn mandatory_inputs(&self) -> &[Value] {
        &self.mandatory
    }

    // {impl}`requirements.authoring.delivered_inputs`
    pub(crate) fn recorded_inputs(&self) -> &[Value] {
        &self.recorded
    }

    // {impl}`requirements.authoring.delivered_inputs`
    pub(crate) fn record_inputs(&mut self, inputs: &[Value]) {
        for input in inputs {
            if !self.recorded.contains(input) {
                self.recorded.push(input.clone());
            }
        }
    }

    // {impl}`requirements.authoring.source_tools`
    // {impl}`requirements.authoring.text_search`
    pub(crate) fn read_selector(&self, selector: &InputSelector) -> io::Result<Value> {
        let definitions = load_definitions(&self.root)?;
        source_input(&self.root, &self.access, &definitions, selector)
    }

    // {impl}`requirements.definitions.authoring_inputs`
    // {impl}`requirements.authoring.source_tools`
    // {impl}`requirements.authoring.text_search`
    pub(crate) fn additional_definitions(&self, inputs: &[Value]) -> io::Result<Vec<Value>> {
        let definitions = load_definitions(&self.root)?;
        Ok(
            definition_inputs(&self.root, &self.access, &definitions, inputs, None)?
                .into_iter()
                .filter(|input| !inputs.contains(input) && !self.recorded.contains(input))
                .collect(),
        )
    }
}

// {impl}`requirements.authoring.mandatory_inputs`
// {impl}`requirements.authoring.source_tools`
// {impl}`requirements.authoring.file_discovery`
// {impl}`requirements.authoring.text_search`
// {impl}`requirements.ci.authoring_inputs`
pub(crate) fn generated_evidence_path(root: &Path, path: &Path) -> bool {
    let path = root.join(path);
    let evidence = root.join("evidence");
    path.starts_with(&evidence)
        || fs::canonicalize(&path)
            .is_ok_and(|path| path.starts_with(fs::canonicalize(&evidence).unwrap_or(evidence)))
}

// {impl}`requirements.authoring.mandatory_inputs`
// {impl}`requirements.authoring.source_tools`
// {impl}`requirements.ci.authoring_inputs`
// {impl}`requirements.configuration.evidence_paths`
fn generated_evidence_selector(
    root: &Path,
    access: &SourceAccess,
    selector: &InputSelector,
) -> bool {
    match selector {
        InputSelector::FileRange { file, .. }
        | InputSelector::RustItem {
            rust_file: file, ..
        } => access.omit_generated_evidence(root, Path::new(file)),
        InputSelector::Requirements { .. } | InputSelector::Definition { .. } => false,
    }
}

// {impl}`requirements.authoring.mandatory_inputs`
// {impl}`requirements.authoring.source_tools`
// {impl}`requirements.definitions.authoring_inputs`
// {impl}`requirements.ci.authoring_inputs`
// {impl}`requirements.configuration.evidence_paths`
// {impl}`requirements.authoring.read_denial`
// {impl}`requirements.definitions.selection`
// {impl}`requirements.references.definition_globs`
fn source_input(
    root: &Path,
    access: &SourceAccess,
    definitions: &BTreeMap<String, Declaration>,
    selector: &InputSelector,
) -> io::Result<Value> {
    match selector {
        InputSelector::FileRange { file, .. }
        | InputSelector::RustItem {
            rust_file: file, ..
        } => {
            access.file(root, Path::new(file))?;
        }
        InputSelector::Definition { definition } => {
            if let Some(definition) = definitions.get(definition) {
                access.file(root, Path::new(&definition.file))?;
            }
        }
        InputSelector::Requirements { requirements } => {
            for (name, definition) in definitions {
                if definition.kind != DefinitionKind::Term
                    && crate::definition_glob::matches(requirements, name)
                {
                    access.file(root, Path::new(&definition.file))?;
                }
            }
        }
    }
    let resolved = selector
        .resolve(root, definitions)
        .map_err(io::Error::other)?;
    for range in &resolved.ranges {
        access.file(root, Path::new(&range.file))?;
    }
    Ok(selector.to_input(&resolved))
}

// {impl}`requirements.authoring.mandatory_inputs`
fn insert_selector(selectors: &mut Vec<InputSelector>, selector: InputSelector) {
    if !selectors.contains(&selector) {
        selectors.push(selector);
    }
}

// {impl}`requirements.references.authoring_inputs`
fn reference_selector(
    root: &Path,
    definitions: &BTreeMap<String, Declaration>,
    reference: &SourceReference,
) -> io::Result<InputSelector> {
    let file = reference
        .file
        .strip_prefix(root)
        .map_err(|_| io::Error::other("reference is outside project root"))?
        .to_str()
        .ok_or_else(|| io::Error::other("reference path is not UTF-8"))?
        .to_owned();
    if reference
        .file
        .extension()
        .is_some_and(|extension| extension == "rs")
    {
        let (_, source) = read_project_file(root, &file).map_err(io::Error::other)?;
        let items = crate::rust_items::parse_items(&source).map_err(|error| {
            io::Error::other(format!(
                "{file}: cannot parse referenced Rust source: {error}"
            ))
        })?;
        if let Some(item) = items
            .iter()
            .filter(|item| item.line_start <= reference.line && reference.line <= item.line_end)
            .min_by_key(|item| {
                (
                    item.line_end - item.line_start,
                    item.byte_end - item.byte_start,
                )
            })
        {
            return Ok(InputSelector::RustItem {
                rust_file: file,
                rust_item: item.selector.clone(),
            });
        }
    }
    if reference
        .file
        .extension()
        .is_some_and(|extension| extension == "md")
        && let Some((name, declaration)) = definitions
            .iter()
            .filter(|(_, declaration)| {
                declaration.file == file
                    && declaration.line_start <= reference.line
                    && reference.line <= declaration.line_end
            })
            .min_by_key(|(_, declaration)| declaration.line_end - declaration.line_start)
    {
        return Ok(if declaration.kind == DefinitionKind::Term {
            InputSelector::Definition {
                definition: name.clone(),
            }
        } else {
            InputSelector::Requirements {
                requirements: name.clone(),
            }
        });
    }
    let (_, source) = read_project_file(root, &file).map_err(io::Error::other)?;
    Ok(InputSelector::FileRange {
        file,
        line_start: 1,
        line_end: source.lines().count(),
    })
}

// {impl}`requirements.definitions.authoring_inputs`
// {impl}`requirements.authoring.mandatory_inputs`
fn definition_inputs(
    root: &Path,
    access: &SourceAccess,
    definitions: &BTreeMap<String, Declaration>,
    inputs: &[Value],
    subject: Option<&str>,
) -> io::Result<Vec<Value>> {
    let record = json!({ "inputs": inputs });
    crate::terms::dependencies(definitions, record.as_object().unwrap(), subject)
        .map_err(io::Error::other)?
        .into_iter()
        .map(|definition| {
            let selector = InputSelector::Definition { definition };
            source_input(root, access, definitions, &selector)
        })
        .collect()
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::authoring::access::tests::git;

    // {test}`requirements.authoring.mandatory_inputs`
    pub(crate) fn role(kind: &str, name: &str) -> String {
        format!("{{{kind}}}`{name}`")
    }

    // {test}`requirements.authoring.mandatory_inputs`
    pub(crate) fn project() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("md")).unwrap();
        fs::create_dir_all(root.path().join("src")).unwrap();
        fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::write(root.path().join("book.toml"), "[book]\nsrc='md'\n").unwrap();
        fs::write(
            root.path().join("md/requirements.md"),
            ":::{req} ready\nThe project MUST be ready.\n:::\n",
        )
        .unwrap();
        root
    }

    // {test}`requirements.authoring.complete_initial_bundle`
    // {test}`requirements.authoring.mandatory_inputs`
    // {test}`requirements.authoring_input`
    // {test}`requirements.examples.authoring_inputs`
    // {test}`requirements.references.authoring_inputs`
    // {test}`requirements.definitions.authoring_inputs`
    // {test}`requirements.ci.authoring_inputs`
    #[test]
    fn bundle_combines_direct_references_terms_and_current_existing_selectors() {
        let root = project();
        let terms = format!(
            ":::{{definition}} unit\n:phrase: unit\nA {}.\n:::\n\n:::{{definition}} value\n:phrase: value\nA {}.\n:::\n",
            role("term", "terms.value"),
            role("term", "terms.unit")
        );
        fs::write(root.path().join("md/terms.md"), terms).unwrap();
        let declarations = format!(
            ":::{{req}} ready\nReady uses {} and {}.\n:::\n\n:::{{req}} base\nBase cites {}.\n:::\n\n:::{{req}} distant\nDistant MUST work.\n:::\n\n:::{{example}} case\nReady example. {} {}\n:::\n",
            role("cite", "requirements.base"),
            role("term", "terms.unit"),
            role("cite", "requirements.distant"),
            role("cite", "requirements.ready"),
            role("test", "requirements.ready")
        );
        fs::write(root.path().join("md/requirements.md"), declarations).unwrap();
        fs::write(root.path().join("src/lib.rs"), format!("struct Thing;\nimpl Thing {{\n    // {}\n    fn run(&self) {{}}\n    fn unrelated(&self) {{}}\n}}\n// {}\nfn test_ready() {{}}\n", role("impl", "requirements.ready"), role("test", "requirements.ready"))).unwrap();
        fs::write(root.path().join("notes.txt"), "new source text\n").unwrap();
        fs::create_dir(root.path().join("evidence")).unwrap();
        fs::write(
            root.path().join("evidence/requirements.ready.json"),
            json!({
                "determination": false, "rationale": "old secret judgment",
                "inputs": [{"file":"notes.txt", "line_start":1, "line_end":1, "text":["old text"]}]
            })
            .to_string(),
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let inputs = sources.mandatory_inputs();
        assert!(sources.recorded_inputs().is_empty());
        assert!(
            inputs
                .iter()
                .any(|input| input["rust_item"] == "impl[Thing]/fn[run]")
        );
        assert!(
            inputs
                .iter()
                .any(|input| input["rust_item"] == "fn[test_ready]")
        );
        assert!(
            !inputs
                .iter()
                .any(|input| input["rust_item"] == "impl[Thing]")
        );
        assert!(
            inputs
                .iter()
                .any(|input| input["requirements"] == "requirements.case")
        );
        assert!(
            inputs
                .iter()
                .any(|input| input["requirements"] == "requirements.base")
        );
        assert!(
            !inputs
                .iter()
                .any(|input| input["requirements"] == "requirements.distant")
        );
        assert_eq!(
            inputs
                .iter()
                .filter(|input| input.get("definition").is_some())
                .count(),
            2
        );
        // Earlier evidence inputs are not carried forward.
        assert!(!inputs.iter().any(|input| input["file"] == "notes.txt"));
        assert!(
            !serde_json::to_string(inputs)
                .unwrap()
                .contains("old secret judgment")
        );
        let example = AssessmentSources::prepare(root.path(), "requirements.case").unwrap();
        assert!(matches!(
            example.subject_kind,
            AssessmentKind::Definition(DefinitionKind::Example)
        ));
        assert_eq!(
            example.mandatory_inputs()[0]["requirements"],
            "requirements.case"
        );
    }

    // {test}`requirements.authoring.mandatory_inputs`
    // {test}`requirements.ci.authoring_inputs`
    #[test]
    fn unresolvable_earlier_inputs_do_not_block_reassessment() {
        let root = project();
        fs::create_dir(root.path().join("evidence")).unwrap();
        fs::write(
            root.path().join("evidence/requirements.ready.json"),
            json!({
                "inputs": [
                    {"file":"missing.txt", "line_start":1, "line_end":1},
                    {"rust_file":"src/lib.rs", "rust_item":"fn[removed]", "text":["fn removed() {}"]},
                    {"requirements":"requirements.removed", "matches":["requirements.removed"], "text":["gone"]}
                ]
            })
            .to_string(),
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        assert_eq!(
            sources.mandatory_inputs(),
            &[json!({
                "requirements": "requirements.ready",
                "matches": ["requirements.ready"],
                "text": [":::{req} ready", "The project MUST be ready.", ":::"]
            })]
        );
        assert!(AssessmentSources::prepare(root.path(), "missing").is_err());
    }

    // {test}`requirements.authoring.mandatory_inputs`
    // {test}`requirements.ci.authoring_inputs`
    #[test]
    fn earlier_evidence_inputs_and_judgments_are_not_carried_forward() {
        let root = project();
        fs::write(root.path().join("notes.txt"), "current source\n").unwrap();
        fs::create_dir_all(root.path().join("evidence")).unwrap();
        let old = json!({
            "requirement": "The project MUST be ready.",
            "determination": false,
            "rationale": "old secret judgment",
            "inputs": [
                {"file":"evidence/requirements.ready.json", "line_start":1, "line_end":1, "text":["old judgment"]},
                {"file":"notes.txt", "line_start":1, "line_end":1, "text":["old source"]},
            ]
        });
        let path = root.path().join("evidence/requirements.ready.json");
        fs::write(&path, old.to_string()).unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let supplied = serde_json::to_string(sources.mandatory_inputs()).unwrap();
        assert!(!supplied.contains("notes.txt"), "{supplied}");
        assert!(!supplied.contains("evidence/"), "{supplied}");
        assert!(!supplied.contains("old secret judgment"), "{supplied}");
        assert_eq!(
            serde_json::from_str::<Value>(&fs::read_to_string(path).unwrap()).unwrap(),
            old
        );
    }

    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.mandatory_inputs`
    // {test}`requirements.definitions.authoring_inputs`
    // {test}`requirements.authoring_input`
    // {test}`requirements.references.authoring_inputs`
    // {test}`requirements.authoring.read_denial`
    #[test]
    fn mandatory_git_inputs_require_staging_declarations_sources_and_terms() {
        let root = project();
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["add", "Cargo.toml", "book.toml"]);
        let error = AssessmentSources::prepare(root.path(), "requirements.ready")
            .err()
            .unwrap();
        assert!(
            error.to_string().contains("md/requirements.md")
                && error.to_string().contains("git add"),
            "{error}"
        );
        git(root.path(), &["add", "md"]);
        fs::write(
            root.path().join("src/lib.rs"),
            format!(
                "// {}\nfn ready() {{}}\n",
                role("impl", "requirements.ready")
            ),
        )
        .unwrap();
        let error = AssessmentSources::prepare(root.path(), "requirements.ready")
            .err()
            .unwrap();
        assert!(
            error.to_string().contains("src/lib.rs") && error.to_string().contains("git add"),
            "{error}"
        );
        git(root.path(), &["add", "src"]);
        fs::write(
            root.path().join("md/terms.md"),
            ":::{definition} ready\n:phrase: ready\nReady to proceed.\n:::\n",
        )
        .unwrap();
        fs::write(
            root.path().join("src/lib.rs"),
            format!(
                "// {} {}\nfn ready() {{}}\n",
                role("impl", "requirements.ready"),
                role("term", "terms.ready")
            ),
        )
        .unwrap();
        let error = AssessmentSources::prepare(root.path(), "requirements.ready")
            .err()
            .unwrap();
        assert!(
            error.to_string().contains("md/terms.md") && error.to_string().contains("git add"),
            "{error}"
        );
        git(root.path(), &["add", "md/terms.md"]);
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        assert!(
            sources
                .mandatory_inputs()
                .iter()
                .any(|input| input["definition"] == "terms.ready")
        );
    }

    // {test}`requirements.configuration.evidence_paths`
    // {test}`requirements.authoring.mandatory_inputs`
    // {test}`requirements.authoring.read_denial`
    #[test]
    fn disallowed_mandatory_inputs_abort_preparation() {
        let root = project();
        fs::write(
            root.path().join("habeas.toml"),
            "version = 1\n[evidence]\ndisallow = ['md/requirements.md']\n",
        )
        .unwrap();
        let error = AssessmentSources::prepare(root.path(), "requirements.ready")
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
        assert!(error.to_string().contains("md/requirements.md"), "{error}");
    }

    // {test}`requirements.references.authoring_inputs`
    // {test}`requirements.references.targets`
    // {test}`requirements.authoring.mandatory_inputs`
    #[test]
    fn only_edges_leaving_the_subject_contribute_inputs() {
        let root = project();
        let declarations = format!(
            ":::{{req}} ready\nReady relies on {} and refines {}.\n:::\n\n:::{{req}} base\nBase MUST hold.\n:::\n\n:::{{req}} parent\nParent MUST hold.\n:::\n\n:::{{example}} citer\nA case relying on ready. {}\n:::\n\n:::{{example}} tester\nA ready case. {} {}\n:::\n",
            role("cite", "requirements.base"),
            role("impl", "requirements.parent"),
            role("cite", "requirements.ready"),
            role("cite", "requirements.base"),
            role("test", "requirements.ready")
        );
        fs::write(root.path().join("md/requirements.md"), declarations).unwrap();
        fs::write(
            root.path().join("src/lib.rs"),
            format!(
                "// {}\nfn run() {{}}\n// {}\nfn helper() {{}}\n// {}\nfn check() {{}}\n",
                role("impl", "requirements.ready"),
                role("cite", "requirements.ready"),
                role("test", "requirements.ready")
            ),
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let inputs = sources.mandatory_inputs();
        let has_requirements =
            |name: &str| inputs.iter().any(|input| input["requirements"] == name);
        let has_item = |item: &str| inputs.iter().any(|input| input["rust_item"] == item);
        assert!(has_requirements("requirements.ready"));
        assert!(has_requirements("requirements.base"), "{inputs:#?}");
        assert!(has_requirements("requirements.tester"), "{inputs:#?}");
        assert!(has_item("fn[run]"), "{inputs:#?}");
        assert!(has_item("fn[check]"), "{inputs:#?}");
        assert!(!has_item("fn[helper]"), "{inputs:#?}");
        assert!(!has_requirements("requirements.citer"), "{inputs:#?}");
        assert!(!has_requirements("requirements.parent"), "{inputs:#?}");

        let parent = AssessmentSources::prepare(root.path(), "requirements.parent").unwrap();
        let inputs = parent.mandatory_inputs();
        assert!(
            inputs
                .iter()
                .any(|input| input["requirements"] == "requirements.ready"),
            "{inputs:#?}"
        );
        assert!(
            !inputs
                .iter()
                .any(|input| input["requirements"] == "requirements.base"),
            "{inputs:#?}"
        );
    }
}

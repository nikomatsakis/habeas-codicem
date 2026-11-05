use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::access::SourceAccess;
use crate::evidence::{Declaration, SourceReference, collect_references, load_definitions};
use crate::inputs::{InputSelector, read_project_file};
use crate::{DefinitionKind, ReferenceKind};

pub(crate) enum AssessmentKind {
    Definition(DefinitionKind),
    RustFunction {
        rust_item: String,
        citations: Vec<Value>,
    },
}

impl AssessmentKind {
    // {impl}`requirements.authoring.function_attestations`
    // {impl}`requirements.authoring.usage_storage`
    // {impl}`requirements.authoring.initial_prompt`
    pub(crate) fn noun(&self) -> &'static str {
        match self {
            Self::Definition(kind) => kind.noun(),
            Self::RustFunction { .. } => "rust_function",
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
    pub(crate) fn prepare(root: &Path, subject_name: &str) -> Result<Self, String> {
        let root = fs::canonicalize(root).map_err(|error| format!("project root: {error}"))?;
        let access = SourceAccess::load(&root)?;
        let definitions = load_definitions(&root).map_err(|error| error.to_string())?;
        let subject = definitions
            .get(subject_name)
            .filter(|definition| definition.kind != DefinitionKind::Term)
            .ok_or_else(|| format!("{subject_name}: no matching requirement or named example"))?;
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
                return Err(format!(
                    "{subject_name}: referenced subject {} does not exist",
                    reference.name
                ));
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
            return Err(errors.join("\n"));
        }
        for reference in references.iter().filter(|reference| {
            reference.name == subject_name && reference.kind != ReferenceKind::Term
        }) {
            insert_selector(
                &mut selectors,
                reference_selector(&root, &definitions, reference)?,
            );
        }

        let evidence_path = root.join("evidence").join(format!("{subject_name}.json"));
        match fs::read_to_string(&evidence_path) {
            Ok(content) => {
                let evidence: Value = serde_json::from_str(&content)
                    .map_err(|error| format!("{}: {error}", evidence_path.display()))?;
                let inputs = evidence
                    .get("inputs")
                    .and_then(Value::as_array)
                    .ok_or_else(|| {
                        format!(
                            "{}: existing evidence inputs must be an array",
                            evidence_path.display()
                        )
                    })?;
                for (index, input) in inputs.iter().enumerate() {
                    let selector = InputSelector::from_input(input).ok_or_else(|| {
                        format!(
                            "{}: inputs[{index}] has no resolvable selector",
                            evidence_path.display()
                        )
                    })?;
                    insert_selector(&mut selectors, selector);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("{}: {error}", evidence_path.display())),
        }

        let mut mandatory = Vec::new();
        for selector in selectors {
            if !generated_evidence_selector(&root, &selector) {
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

    // {impl}`requirements.authoring.function_attestations`
    // {impl}`requirements.authoring.mandatory_inputs`
    // {impl}`requirements.ci.authoring_inputs`
    // {impl}`requirements.references.rust_function_attestations`
    // {impl}`requirements.definitions.authoring_inputs`
    pub(crate) fn prepare_function(
        root: &Path,
        rust_file: &str,
        rust_item: &str,
    ) -> Result<Self, String> {
        let root = fs::canonicalize(root).map_err(|error| format!("project root: {error}"))?;
        let access = SourceAccess::load(&root)?;
        let definitions = load_definitions(&root).map_err(|error| error.to_string())?;
        access.file(&root, Path::new(rust_file))?;
        let (_, source) = read_project_file(&root, rust_file)?;
        let (_, citations) = crate::rust_items::function_citations(&source, &definitions)
            .map_err(|error| format!("{rust_file}: cannot parse Rust source: {error}"))?
            .into_iter()
            .find(|(item, _)| item.selector == rust_item)
            .ok_or_else(|| format!("{rust_file}: no function matches Rust item {rust_item:?}"))?;
        let mut selectors = vec![
            InputSelector::RustItem {
                rust_file: rust_file.to_owned(),
                rust_item: rust_item.to_owned(),
            },
            InputSelector::Requirements {
                requirements: "*".to_owned(),
            },
        ];
        let path = root.join("evidence/rust").join(format!("{rust_file}.json"));
        match fs::read_to_string(&path) {
            Ok(content) => {
                let sidecar: Value = serde_json::from_str(&content)
                    .map_err(|error| format!("{}: {error}", path.display()))?;
                let functions = sidecar["functions"].as_array().ok_or_else(|| {
                    format!("{}: existing functions must be an array", path.display())
                })?;
                if let Some(entry) = functions
                    .iter()
                    .find(|entry| entry["rust_item"] == rust_item)
                {
                    let inputs = entry["inputs"].as_array().ok_or_else(|| {
                        format!("{}: existing inputs must be an array", path.display())
                    })?;
                    for (index, input) in inputs.iter().enumerate() {
                        let selector = InputSelector::from_input(input).ok_or_else(|| {
                            format!(
                                "{}: inputs[{index}] has no resolvable selector",
                                path.display()
                            )
                        })?;
                        insert_selector(&mut selectors, selector);
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("{}: {error}", path.display())),
        }
        let mut mandatory = Vec::new();
        for selector in selectors {
            if !generated_evidence_selector(&root, &selector) {
                mandatory.push(source_input(&root, &access, &definitions, &selector)?);
            }
        }
        for input in definition_inputs(&root, &access, &definitions, &mandatory, None)? {
            if !mandatory.contains(&input) {
                mandatory.push(input);
            }
        }
        Ok(Self {
            root,
            access,
            subject_name: format!("{rust_file}:{rust_item}"),
            subject_kind: AssessmentKind::RustFunction {
                rust_item: rust_item.to_owned(),
                citations: citations
                    .into_iter()
                    .map(|(role, name)| json!({"role": role, "name": name}))
                    .collect(),
            },
            subject_text: String::new(),
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
    pub(crate) fn read_selector(&self, selector: &InputSelector) -> Result<Value, String> {
        let definitions = load_definitions(&self.root).map_err(|error| error.to_string())?;
        source_input(&self.root, &self.access, &definitions, selector)
    }

    // {impl}`requirements.definitions.authoring_inputs`
    // {impl}`requirements.authoring.source_tools`
    // {impl}`requirements.authoring.text_search`
    pub(crate) fn additional_definitions(&self, inputs: &[Value]) -> Result<Vec<Value>, String> {
        let definitions = load_definitions(&self.root).map_err(|error| error.to_string())?;
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
fn generated_evidence_selector(root: &Path, selector: &InputSelector) -> bool {
    match selector {
        InputSelector::FileRange { file, .. }
        | InputSelector::RustItem {
            rust_file: file, ..
        } => generated_evidence_path(root, Path::new(file)),
        InputSelector::Requirements { .. } | InputSelector::Definition { .. } => false,
    }
}

// {impl}`requirements.authoring.mandatory_inputs`
// {impl}`requirements.authoring.source_tools`
// {impl}`requirements.definitions.authoring_inputs`
// {impl}`requirements.ci.authoring_inputs`
fn source_input(
    root: &Path,
    access: &SourceAccess,
    definitions: &BTreeMap<String, Declaration>,
    selector: &InputSelector,
) -> Result<Value, String> {
    if let InputSelector::FileRange { file, .. }
    | InputSelector::RustItem {
        rust_file: file, ..
    } = selector
    {
        access.file(root, Path::new(file))?;
    }
    let resolved = selector.resolve(root, definitions)?;
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
) -> Result<InputSelector, String> {
    let file = reference
        .file
        .strip_prefix(root)
        .map_err(|_| "reference is outside project root".to_owned())?
        .to_str()
        .ok_or_else(|| "reference path is not UTF-8".to_owned())?
        .to_owned();
    if reference
        .file
        .extension()
        .is_some_and(|extension| extension == "rs")
    {
        let (_, source) = read_project_file(root, &file)?;
        let items = crate::rust_items::parse_items(&source)
            .map_err(|error| format!("{file}: cannot parse referenced Rust source: {error}"))?;
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
    let (_, source) = read_project_file(root, &file)?;
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
) -> Result<Vec<Value>, String> {
    let record = json!({ "inputs": inputs });
    crate::terms::dependencies(definitions, record.as_object().unwrap(), subject)?
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
            ":::{{req}} ready\nReady uses {} and {}.\n:::\n\n:::{{req}} base\nBase cites {}.\n:::\n\n:::{{req}} distant\nDistant MUST work.\n:::\n\n:::{{example}} case\nReady example. {}\n:::\n",
            role("cite", "requirements.base"),
            role("term", "terms.unit"),
            role("cite", "requirements.distant"),
            role("cite", "requirements.ready")
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
        assert!(inputs.iter().any(
            |input| input["file"] == "notes.txt" && input["text"] == json!(["new source text"])
        ));
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
    fn unresolved_existing_selector_fails_before_inference() {
        let root = project();
        fs::create_dir(root.path().join("evidence")).unwrap();
        fs::write(
            root.path().join("evidence/requirements.ready.json"),
            json!({
                "inputs": [{"file":"missing.txt", "line_start":1, "line_end":1}]
            })
            .to_string(),
        )
        .unwrap();
        assert!(
            AssessmentSources::prepare(root.path(), "requirements.ready")
                .err()
                .unwrap()
                .contains("missing.txt")
        );
        assert!(AssessmentSources::prepare(root.path(), "missing").is_err());
    }

    // {test}`requirements.authoring.function_attestations`
    // {test}`requirements.authoring.mandatory_inputs`
    // {test}`requirements.definitions.authoring_inputs`
    // {test}`requirements.ci.authoring_inputs`
    #[test]
    fn function_inputs_include_uncited_definitions_and_current_sources() {
        let root = project();
        fs::write(
            root.path().join("md/requirements.md"),
            format!(
                ":::{{req}} ready\nThe project MUST be ready.\n:::\n\n:::{{req}} uncited\nUse {}.\n:::\n",
                role("term", "terms.unit")
            ),
        )
        .unwrap();
        fs::write(
            root.path().join("md/terms.md"),
            ":::{definition} unit\n:phrase: unit\nA complete unit.\n:::\n",
        )
        .unwrap();
        fs::write(
            root.path().join("src/lib.rs"),
            format!(
                "// {}\npub fn ready() {{}}\n",
                role("impl", "requirements.ready")
            ),
        )
        .unwrap();
        fs::write(root.path().join("notes.txt"), "current notes\n").unwrap();
        fs::create_dir_all(root.path().join("evidence/rust/src")).unwrap();
        fs::write(
            root.path().join("evidence/rust/src/lib.rs.json"),
            json!({"rust_file": "src/lib.rs", "functions": [{
                "rust_item": "fn[ready]", "determination": true,
                "rationale": "OLD_PRIVATE_RATIONALE", "inputs": [
                    {"rust_file": "src/lib.rs", "rust_item": "fn[ready]", "text": ["old function"]},
                    {"file": "notes.txt", "line_start": 1, "line_end": 1, "text": ["old notes"]}
                ]
            }]})
            .to_string(),
        )
        .unwrap();
        let sources =
            AssessmentSources::prepare_function(root.path(), "src/lib.rs", "fn[ready]").unwrap();
        let inputs = sources.mandatory_inputs();
        assert_eq!(sources.subject_name, "src/lib.rs:fn[ready]");
        assert_eq!(sources.subject_kind.noun(), "rust_function");
        let AssessmentKind::RustFunction { citations, .. } = &sources.subject_kind else {
            panic!("expected function assessment");
        };
        assert_eq!(
            citations,
            &vec![json!({"role": "impl", "name": "requirements.ready"})]
        );
        assert_eq!(
            inputs
                .iter()
                .filter(|input| input["rust_item"] == "fn[ready]")
                .count(),
            1
        );
        assert!(inputs.iter().any(|input| input["requirements"] == "*"
            && input["matches"] == json!(["requirements.ready", "requirements.uncited"])));
        assert!(
            inputs
                .iter()
                .any(|input| input["definition"] == "terms.unit")
        );
        assert!(
            inputs
                .iter()
                .any(|input| input["text"] == json!(["current notes"]))
        );
        assert!(
            !serde_json::to_string(inputs)
                .unwrap()
                .contains("OLD_PRIVATE_RATIONALE")
        );
        assert!(
            AssessmentSources::prepare_function(root.path(), "src/lib.rs", "fn[missing]").is_err()
        );
    }

    // {test}`requirements.authoring.mandatory_inputs`
    // {test}`requirements.authoring.function_attestations`
    // {test}`requirements.ci.authoring_inputs`
    #[test]
    fn existing_generated_evidence_inputs_are_omitted_before_resolution() {
        let root = project();
        fs::write(
            root.path().join("src/lib.rs"),
            format!(
                "// {}\npub fn ready() {{}}\n",
                role("impl", "requirements.ready")
            ),
        )
        .unwrap();
        fs::write(root.path().join("notes.txt"), "current source\n").unwrap();
        fs::create_dir_all(root.path().join("evidence/rust/src")).unwrap();
        let mut old_inputs = vec![
            json!({"file":"evidence/requirements.ready.json", "line_start":1, "line_end":999999, "text":["old judgment"]}),
            json!({"file":"evidence/removed.json", "line_start":1, "line_end":2, "text":["removed evidence"]}),
            json!({"file":"notes.txt", "line_start":1, "line_end":1, "text":["old source"]}),
        ];
        if cfg!(unix) {
            old_inputs.push(json!({"file":"previous-evidence.json", "line_start":1, "line_end":999999, "text":["aliased judgment"]}));
        }
        let path = root.path().join("evidence/requirements.ready.json");
        fs::write(&path, json!({"inputs":old_inputs}).to_string()).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&path, root.path().join("previous-evidence.json")).unwrap();
        fs::write(
            root.path().join("evidence/rust/src/lib.rs.json"),
            json!({"rust_file":"src/lib.rs", "functions":[{
                "rust_item":"fn[ready]", "inputs":old_inputs,
            }]})
            .to_string(),
        )
        .unwrap();
        for sources in [
            AssessmentSources::prepare(root.path(), "requirements.ready").unwrap(),
            AssessmentSources::prepare_function(root.path(), "src/lib.rs", "fn[ready]").unwrap(),
        ] {
            let file_inputs = sources
                .mandatory_inputs()
                .iter()
                .filter(|input| input.get("file").is_some())
                .collect::<Vec<_>>();
            assert_eq!(file_inputs.len(), 1);
            assert_eq!(file_inputs[0]["file"], "notes.txt");
            assert_eq!(file_inputs[0]["text"], json!(["current source"]));
        }
        assert_eq!(
            serde_json::from_str::<Value>(&fs::read_to_string(path).unwrap()).unwrap()["inputs"],
            json!(old_inputs)
        );
    }

    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.mandatory_inputs`
    // {test}`requirements.definitions.authoring_inputs`
    // {test}`requirements.ci.authoring_inputs`
    // {test}`requirements.authoring.function_attestations`
    // {test}`requirements.authoring_input`
    // {test}`requirements.references.authoring_inputs`
    #[test]
    fn mandatory_git_inputs_require_staging_declarations_sources_terms_and_old_selectors() {
        let root = project();
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["add", "Cargo.toml", "book.toml"]);
        let error = AssessmentSources::prepare(root.path(), "requirements.ready")
            .err()
            .unwrap();
        assert!(
            error.contains("md/requirements.md") && error.contains("git add"),
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
        for result in [
            AssessmentSources::prepare(root.path(), "requirements.ready"),
            AssessmentSources::prepare_function(root.path(), "src/lib.rs", "fn[ready]"),
        ] {
            let error = result.err().unwrap();
            assert!(
                error.contains("src/lib.rs") && error.contains("git add"),
                "{error}"
            );
        }
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
            error.contains("md/terms.md") && error.contains("git add"),
            "{error}"
        );
        git(root.path(), &["add", "md/terms.md"]);
        fs::write(root.path().join("notes.txt"), "current notes\n").unwrap();
        fs::create_dir_all(root.path().join("evidence/rust/src")).unwrap();
        let inputs =
            json!([{"file":"notes.txt", "line_start":1, "line_end":1, "text":["old notes"]}]);
        fs::write(
            root.path().join("evidence/requirements.ready.json"),
            json!({"inputs":inputs}).to_string(),
        )
        .unwrap();
        fs::write(root.path().join("evidence/rust/src/lib.rs.json"), json!({"rust_file":"src/lib.rs", "functions":[{"rust_item":"fn[ready]", "inputs":inputs}]}).to_string()).unwrap();
        for result in [
            AssessmentSources::prepare(root.path(), "requirements.ready"),
            AssessmentSources::prepare_function(root.path(), "src/lib.rs", "fn[ready]"),
        ] {
            let error = result.err().unwrap();
            assert!(
                error.contains("notes.txt") && error.contains("git add"),
                "{error}"
            );
        }
        git(root.path(), &["add", "notes.txt"]);
        for sources in [
            AssessmentSources::prepare(root.path(), "requirements.ready").unwrap(),
            AssessmentSources::prepare_function(root.path(), "src/lib.rs", "fn[ready]").unwrap(),
        ] {
            assert!(
                sources
                    .mandatory_inputs()
                    .iter()
                    .any(|input| input["file"] == "notes.txt"
                        && input["text"] == json!(["current notes"]))
            );
        }
    }
}

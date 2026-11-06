use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use serde_json::{Map, Value, json};

use super::inputs::AssessmentSources;
use crate::inputs::{InputSelector, read_project_file};

pub(crate) struct ToolDelivery {
    pub(crate) result: Value,
    pub(crate) inputs: Vec<Value>,
}

// {impl}`requirements.authoring.source_tools`
// {impl}`requirements.authoring.file_discovery`
// {impl}`requirements.authoring.text_search`
// {impl}`requirements.configuration.evidence_paths`
// {impl}`requirements.authoring.generated_inputs`
// {impl}`requirements.authoring.read_denial`
// {impl}`requirements.authoring.credentials`
pub(crate) fn definitions() -> Vec<Value> {
    // Providers such as Anthropic require a plain object at the schema root.
    // Describe the alternative forms here and enforce them in read().
    vec![
        json!({
            "name": "habeas",
            "description": "Read project source as one evidence input. Supply exactly one request form: file + line_start (optional line_end), rust_file + rust_item, requirements, or definition. Do not combine forms. Paths may be project-relative or absolute when permitted by the evidence policy; line numbers are one-based and inclusive; omitted line_end reads through EOF. Example: {\"file\":\"src/lib.rs\",\"line_start\":1}. Rust selectors name complete syntactic items, with / separating enclosing items, kind[name] segments, and #N for duplicate siblings (one-based), e.g. {\"rust_file\":\"src/lib.rs\",\"rust_item\":\"impl[Thing]/fn[run]\"}. Trait impls use impl[Thing/Trait], and positional imports use use#1. Delimiters %, [, ], /, # and whitespace inside names or impl types use percent-encoded UTF-8 bytes. The requirements form takes a definition selection: a full requirement or example name, or a glob matching complete names where * matches any characters including dots, e.g. {\"requirements\":\"requirements.cli.*\"}. Definition reads use the full terminology name, e.g. {\"definition\":\"glossary.command\"}. Required terminology is returned alongside the selected input. Defaults permit project files (Git-tracked in Git worktrees) and existing Cargo build-script output directories; evidence/ and a workspace habeas.toml containing credentials are excluded. The nearest habeas.toml can explicitly allow other paths or disallow paths; disallow wins. Denied reads abort the assessment and report to the operator. Paths outside the project may be absolute when allowed.",
            "parameters": {"type":"object", "properties": {
                "file":{"type":"string"},
                "line_start":{"type":"integer","minimum":1},
                "line_end":{"type":"integer","minimum":1},
                "rust_file":{"type":"string"},
                "rust_item":{"type":"string"},
                "requirements":{"type":"string"},
                "definition":{"type":"string"}
            }, "additionalProperties":false}
        }),
        json!({
            "name": "list_files", "description": "List immediate files and directories at a project-relative or policy-permitted absolute path (default .). Returned paths can be listed or read in another call. Directory listings are not evidence inputs. Only eligible sources are listed. Root listings also expose allowed paths and discovered Cargo build-script output directories. Defaults exclude evidence/, untracked files, and workspace configuration containing credentials; do not request excluded paths. Workspace habeas.toml allow/disallow rules apply; denied explicit requests abort the assessment.",
            "parameters":{"type":"object","properties":{"path":{"type":"string","default":"."}},"additionalProperties":false}
        }),
        json!({
            "name":"ripgrep", "description":"Search a project-relative or policy-permitted absolute file or directory (default .) using ripgrep regex syntax, case-sensitive matching, and normal ignore rules (including .gitignore and hidden-file exclusions). Return matching lines plus context lines before and after each match (default 5; zero means matches only). Overlapping windows are merged, recorded as file-range evidence, and accompanied by required terminology. No matches returns empty arrays. Results include only eligible sources. Defaults exclude evidence/, untracked files, and workspace configuration containing credentials; do not request excluded paths. Explicitly allowed paths and existing Cargo build-script outputs are searched even when Git ignores them. Workspace disallow rules win; denied explicit requests abort the assessment.",
            "parameters":{"type":"object","properties":{"pattern":{"type":"string"},"path":{"type":"string","default":"."},"context":{"type":"integer","minimum":0,"default":5}},"required":["pattern"],"additionalProperties":false}
        }),
        json!({
            "name":"references", "description":"List definitions and the sources that reference them. Without selection, return every requirement and named example name with its kind. With selection (a full requirement or example name, or a glob where * matches any characters including dots, e.g. requirements.cli.*), return the selected definitions plus every {impl}, {cite}, and {test} reference to them: definition name, role, file, line, and the enclosing Rust item selector when the reference is in a Rust item. An :impl: or :test: anchor on a definition names a complete file and is reported with line null. {impl} and {test} mark sources that implement or verify the definition; {cite} marks sources that only rely on or refer to it. Results contain no source text and are not evidence inputs; read the sources you rely on with habeas.",
            "parameters":{"type":"object","properties":{"selection":{"type":"string"}},"additionalProperties":false}
        }),
    ]
}

// {impl}`requirements.authoring.source_tools`
// {impl}`requirements.authoring.file_discovery`
// {impl}`requirements.authoring.text_search`
pub(crate) fn dispatch(
    sources: &AssessmentSources,
    name: &str,
    arguments: &Value,
) -> io::Result<ToolDelivery> {
    let arguments = arguments
        .as_object()
        .ok_or_else(|| io::Error::other("tool arguments must be an object"))?;
    match name {
        "habeas" => read(sources, arguments),
        "list_files" => list_files(sources, arguments),
        "ripgrep" => search(sources, arguments),
        "references" => references(sources, arguments),
        _ => Err(io::Error::other(format!("unknown read tool {name:?}"))),
    }
}

// {impl}`requirements.authoring.source_tools`
// {impl}`requirements.definitions.authoring_inputs`
// {impl}`requirements.authoring.delivered_inputs`
// {impl}`requirements.authoring.read_denial`
fn read(sources: &AssessmentSources, args: &Map<String, Value>) -> io::Result<ToolDelivery> {
    let selector = if args.contains_key("file") {
        check_keys(args, &["file", "line_start", "line_end"])?;
        let file = string(args, "file")?.to_owned();
        sources.access.file(&sources.root, Path::new(&file))?;
        let line_start = integer(args, "line_start")?;
        let line_end = if args.contains_key("line_end") {
            integer(args, "line_end")?
        } else {
            read_project_file(&sources.root, &file)
                .map_err(io::Error::other)?
                .1
                .lines()
                .count()
        };
        InputSelector::FileRange {
            file,
            line_start,
            line_end,
        }
    } else if args.contains_key("rust_file") {
        check_keys(args, &["rust_file", "rust_item"])?;
        InputSelector::RustItem {
            rust_file: string(args, "rust_file")?.to_owned(),
            rust_item: string(args, "rust_item")?.to_owned(),
        }
    } else if args.contains_key("requirements") {
        check_keys(args, &["requirements"])?;
        InputSelector::Requirements {
            requirements: string(args, "requirements")?.to_owned(),
        }
    } else {
        check_keys(args, &["definition"])?;
        InputSelector::Definition {
            definition: string(args, "definition")?.to_owned(),
        }
    };
    let input = sources.read_selector(&selector)?;
    let definitions = sources.additional_definitions(std::slice::from_ref(&input))?;
    let result = json!({ "input": input, "definitions": definitions });
    let mut inputs = vec![input];
    inputs.extend(definitions);
    Ok(ToolDelivery { result, inputs })
}

// {impl}`requirements.authoring.file_discovery`
// {impl}`requirements.authoring.source_tools`
// {impl}`requirements.configuration.evidence_paths`
// {impl}`requirements.authoring.generated_inputs`
// {impl}`requirements.authoring.read_denial`
fn list_files(sources: &AssessmentSources, args: &Map<String, Value>) -> io::Result<ToolDelivery> {
    check_keys(args, &["path"])?;
    let path = optional_path(args)?;
    let directory = project_path(sources, path)?;
    let mut entries = Vec::new();
    for entry in fs::read_dir(&directory)
        .map_err(|error| io::Error::new(error.kind(), format!("cannot list {path}: {error}")))?
    {
        let entry = entry.map_err(|error| {
            io::Error::new(error.kind(), format!("cannot list {path}: {error}"))
        })?;
        let relative = relative_path(&sources.root, &entry.path())?;
        if sources
            .access
            .path(&sources.root, Path::new(&relative))
            .is_err()
        {
            continue;
        }
        let metadata = fs::metadata(entry.path()).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("cannot inspect {}: {error}", entry.path().display()),
            )
        })?;
        let kind = if metadata.is_dir() {
            "directory"
        } else if metadata.is_file() {
            "file"
        } else {
            continue;
        };
        entries.push((relative, kind));
    }
    if directory == sources.root {
        for extra in sources.access.extra_roots() {
            if sources.access.path(&sources.root, extra).is_err() {
                continue;
            }
            let kind = if extra.is_dir() {
                "directory"
            } else if extra.is_file() {
                "file"
            } else {
                continue;
            };
            entries.push((relative_path(&sources.root, extra)?, kind));
        }
    }
    entries.sort();
    entries.dedup();
    Ok(ToolDelivery {
        result: Value::Array(
            entries
                .into_iter()
                .map(|(path, kind)| json!({"path":path,"kind":kind}))
                .collect(),
        ),
        inputs: Vec::new(),
    })
}

// {impl}`requirements.authoring.reference_index`
// {impl}`requirements.references.definition_globs`
// {impl}`requirements.references.anchors`
fn references(sources: &AssessmentSources, args: &Map<String, Value>) -> io::Result<ToolDelivery> {
    check_keys(args, &["selection"])?;
    let selection = if args.contains_key("selection") {
        let selection = string(args, "selection")?;
        crate::definition_glob::validate(selection).map_err(io::Error::other)?;
        Some(selection)
    } else {
        None
    };
    let definitions = crate::evidence::load_definitions(&sources.root)?;
    let selected: Vec<_> = definitions
        .iter()
        .filter(|(name, definition)| {
            definition.kind != crate::DefinitionKind::Term
                && selection
                    .is_none_or(|selection| crate::definition_glob::matches(selection, name))
        })
        .map(|(name, definition)| json!({"name": name, "kind": definition.kind.noun()}))
        .collect();
    let Some(selection) = selection else {
        return Ok(ToolDelivery {
            result: json!({"definitions": selected, "sites": []}),
            inputs: Vec::new(),
        });
    };
    if selected.is_empty() {
        return Err(io::Error::other(format!(
            "selection {selection:?} matches no requirement or named example"
        )));
    }
    let mut errors = Vec::new();
    let mut found = crate::evidence::collect_references(&sources.root, &mut errors);
    if !errors.is_empty() {
        return Err(io::Error::other(errors.join("\n")));
    }
    found.retain(|reference| {
        reference.kind != crate::ReferenceKind::Term
            && crate::definition_glob::matches(selection, &reference.name)
            && definitions
                .get(&reference.name)
                .is_some_and(|definition| definition.kind != crate::DefinitionKind::Term)
            // Discovery omits excluded sources rather than aborting.
            && sources.access.file(&sources.root, &reference.file).is_ok()
    });
    let mut items: BTreeMap<PathBuf, Vec<crate::rust_items::RustItem>> = BTreeMap::new();
    let mut sites = Vec::new();
    for reference in &found {
        let file = relative_path(&sources.root, &reference.file)?;
        let rust_item = if reference
            .file
            .extension()
            .is_some_and(|extension| extension == "rs")
        {
            let parsed = items.entry(reference.file.clone()).or_insert_with(|| {
                fs::read_to_string(&reference.file)
                    .ok()
                    .and_then(|source| crate::rust_items::parse_items(&source).ok())
                    .unwrap_or_default()
            });
            parsed
                .iter()
                .filter(|item| item.line_start <= reference.line && reference.line <= item.line_end)
                .min_by_key(|item| item.line_end - item.line_start)
                .map(|item| item.selector.clone())
        } else {
            None
        };
        sites.push((
            reference.name.clone(),
            file,
            Some(reference.line),
            json!({
                "name": reference.name,
                "role": reference.kind.label(),
                "file": relative_path(&sources.root, &reference.file)?,
                "line": reference.line,
                "rust_item": rust_item,
            }),
        ));
    }
    // {impl}`requirements.references.anchors`
    for (name, definition) in &definitions {
        if definition.kind == crate::DefinitionKind::Term
            || !crate::definition_glob::matches(selection, name)
        {
            continue;
        }
        for anchor in &definition.anchors {
            // Discovery omits excluded sources rather than aborting.
            let Ok(path) = sources.access.file(&sources.root, Path::new(&anchor.file)) else {
                continue;
            };
            let file = relative_path(&sources.root, &path)?;
            sites.push((
                name.clone(),
                file.clone(),
                None,
                json!({
                    "name": name,
                    "role": anchor.kind.label(),
                    "file": file,
                    "line": null,
                    "rust_item": null,
                }),
            ));
        }
    }
    sites.sort_by(|a, b| (&a.0, &a.1, a.2).cmp(&(&b.0, &b.1, b.2)));
    Ok(ToolDelivery {
        result: json!({
            "definitions": selected,
            "sites": sites.into_iter().map(|site| site.3).collect::<Vec<_>>(),
        }),
        inputs: Vec::new(),
    })
}

// {impl}`requirements.authoring.text_search`
// {impl}`requirements.authoring.source_tools`
// {impl}`requirements.configuration.evidence_paths`
// {impl}`requirements.authoring.generated_inputs`
// {impl}`requirements.authoring.read_denial`
// {impl}`requirements.definitions.authoring_inputs`
// {impl}`requirements.authoring.delivered_inputs`
fn search(sources: &AssessmentSources, args: &Map<String, Value>) -> io::Result<ToolDelivery> {
    check_keys(args, &["pattern", "path", "context"])?;
    let pattern = string(args, "pattern")?;
    let path = optional_path(args)?;
    let requested = project_path(sources, path)?;
    let context = if args.contains_key("context") {
        integer(args, "context")?
    } else {
        5
    };
    let mut searches = vec![(requested.clone(), false)];
    for extra in sources.access.extra_roots() {
        let Ok(extra) = sources.access.path(&sources.root, extra) else {
            continue;
        };
        let candidate = if requested.starts_with(&extra) {
            requested.clone()
        } else if extra.starts_with(&requested) || requested == sources.root {
            extra.clone()
        } else {
            continue;
        };
        if sources.access.path(&sources.root, &candidate).is_ok() {
            searches.push((candidate, true));
        }
    }
    searches.sort();
    searches.dedup();
    let matcher = grep_regex::RegexMatcherBuilder::new()
        .case_insensitive(false)
        .build(pattern)
        .map_err(|error| io::Error::other(format!("invalid ripgrep pattern: {error}")))?;
    let matches = Mutex::new(BTreeMap::<String, Vec<usize>>::new());
    let failure = Mutex::new(None::<io::Error>);
    for (path, include_ignored) in searches {
        let mut walker = ignore::WalkBuilder::new(&path);
        walker
            .hidden(!include_ignored)
            .ignore(!include_ignored)
            .git_ignore(!include_ignored)
            .git_global(!include_ignored)
            .git_exclude(!include_ignored)
            .parents(!include_ignored)
            .add_custom_ignore_filename(".rgignore");
        walker.build_parallel().run(|| {
            let matcher = &matcher;
            let matches = &matches;
            let failure = &failure;
            let mut searcher = grep_searcher::SearcherBuilder::new()
                .line_number(true)
                .binary_detection(grep_searcher::BinaryDetection::quit(0))
                .build();
            Box::new(move |entry| {
                let Ok(entry) = entry else {
                    return ignore::WalkState::Continue;
                };
                if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                    return ignore::WalkState::Continue;
                }
                // Apply source-access policy while walking: denied files are
                // never opened.
                if sources.access.file(&sources.root, entry.path()).is_err() {
                    return ignore::WalkState::Continue;
                }
                let mut lines = Vec::new();
                let searched = searcher.search_path(
                    matcher,
                    entry.path(),
                    grep_searcher::sinks::Lossy(|number, _| {
                        lines.push(number as usize);
                        Ok(true)
                    }),
                );
                if searched.is_err() || lines.is_empty() {
                    return ignore::WalkState::Continue;
                }
                match relative_path(&sources.root, entry.path()) {
                    Ok(file) => matches
                        .lock()
                        .expect("search results lock")
                        .entry(file)
                        .or_default()
                        .extend(lines),
                    Err(error) => {
                        *failure.lock().expect("search failure lock") = Some(error);
                        return ignore::WalkState::Quit;
                    }
                }
                ignore::WalkState::Continue
            })
        });
    }
    if let Some(error) = failure.into_inner().expect("search failure lock") {
        return Err(error);
    }
    // Parallel walks finish in any order; the map and per-file sort make results
    // independent of scheduling.
    let matches = matches.into_inner().expect("search results lock");
    let mut inputs = Vec::new();
    for (file, mut lines) in matches {
        let (_, source) = read_project_file(&sources.root, &file).map_err(io::Error::other)?;
        let count = source.lines().count();
        lines.sort_unstable();
        lines.dedup();
        let mut windows: Vec<(usize, usize)> = Vec::new();
        for line in lines {
            if line == 0 || line > count {
                return Err(io::Error::other(format!(
                    "{file}: ripgrep returned an out-of-range match"
                )));
            }
            let start = line.saturating_sub(context).max(1);
            let end = line.saturating_add(context).min(count);
            if let Some(last) = windows.last_mut()
                && start <= last.1.saturating_add(1)
            {
                last.1 = last.1.max(end);
            } else {
                windows.push((start, end));
            }
        }
        for (line_start, line_end) in windows {
            inputs.push(sources.read_selector(&InputSelector::FileRange {
                file: file.clone(),
                line_start,
                line_end,
            })?);
        }
    }
    let definitions = sources.additional_definitions(&inputs)?;
    let result = json!({"inputs":inputs,"definitions":definitions});
    inputs.extend(definitions);
    Ok(ToolDelivery { result, inputs })
}

// {impl}`requirements.authoring.source_tools`
// {impl}`requirements.authoring.file_discovery`
// {impl}`requirements.authoring.text_search`
fn check_keys(args: &Map<String, Value>, allowed: &[&str]) -> io::Result<()> {
    for key in args.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(io::Error::other(format!(
                "unexpected tool argument {key:?}"
            )));
        }
    }
    Ok(())
}

// {impl}`requirements.authoring.source_tools`
// {impl}`requirements.authoring.text_search`
fn string<'a>(args: &'a Map<String, Value>, key: &str) -> io::Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other(format!("{key} must be a string")))
}

// {impl}`requirements.authoring.source_tools`
// {impl}`requirements.authoring.text_search`
fn integer(args: &Map<String, Value>, key: &str) -> io::Result<usize> {
    args.get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| io::Error::other(format!("{key} must be a nonnegative integer")))
}

// {impl}`requirements.authoring.file_discovery`
// {impl}`requirements.authoring.text_search`
fn optional_path(args: &Map<String, Value>) -> io::Result<&str> {
    if args.contains_key("path") {
        string(args, "path")
    } else {
        Ok(".")
    }
}

// {impl}`requirements.authoring.file_discovery`
// {impl}`requirements.authoring.text_search`
// {impl}`requirements.ci.input_containment`
// {impl}`requirements.authoring.source_tools`
// {impl}`requirements.configuration.evidence_paths`
// {impl}`requirements.authoring.read_denial`
fn project_path(sources: &AssessmentSources, relative: &str) -> io::Result<PathBuf> {
    sources.access.path(&sources.root, Path::new(relative))
}

// {impl}`requirements.authoring.file_discovery`
// {impl}`requirements.authoring.text_search`
// {impl}`requirements.configuration.evidence_paths`
fn relative_path(root: &Path, path: &Path) -> io::Result<String> {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .filter(|part| *part != Component::CurDir)
        .collect::<PathBuf>()
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| io::Error::other("path is not UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoring::access::tests::git;
    use crate::authoring::inputs::tests::{project, role};

    // {test}`requirements.authoring.additional_read`
    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.file_discovery`
    // {test}`requirements.authoring.delivered_inputs`
    // {test}`requirements.definitions.authoring_inputs`
    #[test]
    fn reading_records_only_delivered_sources_and_their_terms() {
        let root = project();
        fs::write(
            root.path().join("md/terms.md"),
            ":::{definition} value\n:phrase: value\nA useful value.\n:::\n",
        )
        .unwrap();
        fs::write(
            root.path().join("src/lib.rs"),
            format!("// {}\nfn work() {{}}\n", role("term", "terms.value")),
        )
        .unwrap();
        let mut sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let listing = dispatch(&sources, "list_files", &json!({"path":"src"})).unwrap();
        assert_eq!(listing.result, json!([{"path":"src/lib.rs","kind":"file"}]));
        assert!(listing.inputs.is_empty());
        let read = dispatch(
            &sources,
            "habeas",
            &json!({"rust_file":"src/lib.rs","rust_item":"fn[work]"}),
        )
        .unwrap();
        assert_eq!(read.inputs.len(), 2);
        assert_eq!(read.result["definitions"][0]["definition"], "terms.value");
        assert!(sources.recorded_inputs().is_empty());
        sources.record_inputs(&read.inputs);
        sources.record_inputs(&read.inputs);
        assert_eq!(sources.recorded_inputs().len(), 2);
        let eof = dispatch(
            &sources,
            "habeas",
            &json!({"file":"src/lib.rs","line_start":2}),
        )
        .unwrap();
        assert_eq!(eof.result["input"]["line_end"], 2);
        assert_eq!(eof.result["input"]["text"], json!(["fn work() {}"]));
        assert!(
            dispatch(
                &sources,
                "habeas",
                &json!({"file":"src/lib.rs","line_start":0})
            )
            .is_err()
        );
        // The provider-facing schema leaves selector fields optional; the
        // harness still rejects incomplete, mixed, and unknown request forms.
        for arguments in [
            json!({}),
            json!({"file":"src/lib.rs"}),
            json!({"line_start":1}),
            json!({"rust_file":"src/lib.rs"}),
            json!({"rust_item":"fn[work]"}),
            json!({"file":"src/lib.rs","line_start":1,"definition":"terms.value"}),
            json!({"rust_file":"src/lib.rs","rust_item":"fn[work]","requirements":"requirements.*"}),
            json!({"requirements":"requirements.*","definition":"terms.value"}),
            json!({"definition":"terms.value","extra":true}),
        ] {
            assert!(
                dispatch(&sources, "habeas", &arguments).is_err(),
                "unexpectedly accepted {arguments}"
            );
        }
        assert!(
            dispatch(
                &sources,
                "habeas",
                &json!({"requirements":"requirements.*"})
            )
            .is_ok()
        );
        assert!(dispatch(&sources, "habeas", &json!({"definition":"terms.value"})).is_ok());
    }

    // {test}`requirements.authoring.text_search`
    // {test}`requirements.authoring.additional_read`
    // {test}`requirements.authoring.file_discovery`
    #[test]
    fn search_merges_context_windows_and_respects_ignore_rules() {
        let root = project();
        git(root.path(), &["init", "--quiet"]);
        fs::write(root.path().join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(root.path().join("ignored.txt"), "needle\n").unwrap();
        fs::write(root.path().join(".hidden.txt"), "needle\n").unwrap();
        let lines = (1..=20)
            .map(|number| {
                if number == 2 || number == 6 {
                    "needle".to_owned()
                } else {
                    format!("line {number}")
                }
            })
            .collect::<Vec<_>>();
        fs::write(root.path().join("sample.txt"), lines.join("\n")).unwrap();
        git(root.path(), &["add", "--force", "."]);
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let search = dispatch(&sources, "ripgrep", &json!({"pattern":"needle"})).unwrap();
        assert_eq!(search.inputs.len(), 1);
        assert_eq!(search.inputs[0]["file"], "sample.txt");
        assert_eq!(search.inputs[0]["line_start"], 1);
        assert_eq!(search.inputs[0]["line_end"], 11);
        assert_eq!(search.inputs[0]["text"], json!(&lines[..11]));
        let exact = dispatch(
            &sources,
            "ripgrep",
            &json!({"pattern":"needle","path":"sample.txt","context":0}),
        )
        .unwrap();
        assert_eq!(exact.inputs.len(), 2);
        assert_eq!(exact.inputs[0]["line_start"], 2);
        assert_eq!(exact.inputs[0]["line_end"], 2);
        let absent = dispatch(&sources, "ripgrep", &json!({"pattern":"NEEDLE"})).unwrap();
        assert_eq!(absent.result, json!({"inputs":[],"definitions":[]}));
        assert!(dispatch(&sources, "ripgrep", &json!({"pattern":"["})).is_err());
        assert!(
            dispatch(
                &sources,
                "ripgrep",
                &json!({"pattern":"needle","context":-1})
            )
            .is_err()
        );
        assert!(dispatch(&sources, "list_files", &json!({"path":"../"})).is_err());
    }

    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.file_discovery`
    // {test}`requirements.authoring.text_search`
    // {test}`requirements.authoring.read_denial`
    #[test]
    fn generated_evidence_is_excluded_from_reads_search_and_discovery() {
        let root = project();
        fs::create_dir(root.path().join("evidence")).unwrap();
        fs::create_dir(root.path().join("src/evidence")).unwrap();
        fs::write(
            root.path().join("evidence/old.json"),
            "needle in a previous judgment\n",
        )
        .unwrap();
        fs::write(
            root.path().join("evidence/generated.rs"),
            "fn generated() {}\n",
        )
        .unwrap();
        fs::write(root.path().join("source.txt"), "needle in source\n").unwrap();
        fs::write(
            root.path().join("src/evidence/source.txt"),
            "needle in nested source\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                root.path().join("evidence"),
                root.path().join("old-output"),
            )
            .unwrap();
            std::os::unix::fs::symlink(
                root.path().join("evidence/old.json"),
                root.path().join("old.json"),
            )
            .unwrap();
        }
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["add", "."]);
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let search = dispatch(
            &sources,
            "ripgrep",
            &json!({"pattern":"needle", "context":0}),
        )
        .unwrap();
        let files = search
            .inputs
            .iter()
            .map(|input| input["file"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(files, vec!["source.txt", "src/evidence/source.txt"]);
        let listing = dispatch(&sources, "list_files", &json!({})).unwrap();
        assert!(listing.result.as_array().unwrap().iter().all(|entry| {
            !["evidence", "old-output", "old.json"].contains(&entry["path"].as_str().unwrap())
        }));
        for path in ["evidence/old.json", "./evidence/old.json"] {
            assert!(
                dispatch(&sources, "habeas", &json!({"file":path, "line_start":1}))
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("generated evidence")
            );
        }
        assert!(
            dispatch(
                &sources,
                "habeas",
                &json!({"rust_file":"evidence/generated.rs", "rust_item":"fn[generated]"})
            )
            .err()
            .unwrap()
            .to_string()
            .contains("generated evidence")
        );
        assert!(
            dispatch(&sources, "list_files", &json!({"path":"evidence"}))
                .err()
                .unwrap()
                .to_string()
                .contains("generated evidence")
        );
        assert!(
            dispatch(
                &sources,
                "ripgrep",
                &json!({"path":"evidence", "pattern":"needle"})
            )
            .err()
            .unwrap()
            .to_string()
            .contains("generated evidence")
        );
        #[cfg(unix)]
        {
            for path in ["old.json", "old-output/old.json"] {
                assert!(
                    dispatch(&sources, "habeas", &json!({"file":path, "line_start":1}))
                        .err()
                        .unwrap()
                        .to_string()
                        .contains("generated evidence")
                );
                assert!(
                    dispatch(
                        &sources,
                        "ripgrep",
                        &json!({"path":path, "pattern":"needle"})
                    )
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("generated evidence")
                );
            }
            assert!(
                dispatch(&sources, "list_files", &json!({"path":"old-output"}))
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("generated evidence")
            );
        }
    }

    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.file_discovery`
    // {test}`requirements.authoring.text_search`
    #[cfg(unix)]
    #[test]
    fn outside_symlinks_are_excluded_from_reads_search_and_discovery() {
        let root = project();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.rs"), "fn outside_needle() {}\n").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("outside")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("secret.rs"),
            root.path().join("outside.rs"),
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let listing = dispatch(&sources, "list_files", &json!({})).unwrap();
        assert!(listing.result.as_array().unwrap().iter().all(|entry| {
            !["outside", "outside.rs"].contains(&entry["path"].as_str().unwrap())
        }));
        let search = dispatch(&sources, "ripgrep", &json!({"pattern":"outside_needle"})).unwrap();
        assert!(search.inputs.is_empty());
        for path in ["outside/secret.rs", "outside.rs"] {
            for arguments in [
                json!({"file":path, "line_start":1}),
                json!({"file":path, "line_start":1, "line_end":1}),
                json!({"rust_file":path, "rust_item":"fn[outside_needle]"}),
            ] {
                assert!(dispatch(&sources, "habeas", &arguments).is_err());
            }
            assert!(
                dispatch(
                    &sources,
                    "ripgrep",
                    &json!({"path":path, "pattern":"needle"})
                )
                .is_err()
            );
        }
        assert!(dispatch(&sources, "list_files", &json!({"path":"outside"})).is_err());
    }

    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.file_discovery`
    // {test}`requirements.authoring.text_search`
    // {test}`requirements.authoring.read_denial`
    #[test]
    fn git_tools_allow_staged_and_dirty_sources_but_hide_untracked_files() {
        let root = project();
        fs::create_dir(root.path().join("tracked-dir")).unwrap();
        fs::write(root.path().join("tracked-dir/file.txt"), "original\n").unwrap();
        fs::write(root.path().join("staged.txt"), "needle staged\n").unwrap();
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["add", "."]);
        fs::write(root.path().join("tracked-dir/file.txt"), "needle dirty\n").unwrap();
        fs::create_dir(root.path().join("untracked-dir")).unwrap();
        fs::write(
            root.path().join("untracked-dir/file.txt"),
            "needle hidden\n",
        )
        .unwrap();
        fs::write(root.path().join("untracked.rs"), "fn needle() {}\n").unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let listing = dispatch(&sources, "list_files", &json!({})).unwrap();
        assert_eq!(
            listing
                .result
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| entry["path"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["Cargo.toml", "book.toml", "md", "staged.txt", "tracked-dir"]
        );
        for (path, text) in [
            ("staged.txt", "needle staged"),
            ("tracked-dir/file.txt", "needle dirty"),
        ] {
            let read = dispatch(&sources, "habeas", &json!({"file":path, "line_start":1})).unwrap();
            assert_eq!(read.result["input"]["text"], json!([text]));
        }
        let search = dispatch(&sources, "ripgrep", &json!({"pattern":"needle"})).unwrap();
        assert_eq!(
            search
                .inputs
                .iter()
                .map(|input| input["file"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["staged.txt", "tracked-dir/file.txt"]
        );
        for arguments in [
            json!({"file":"untracked.rs", "line_start":1}),
            json!({"file":"untracked.rs", "line_start":1, "line_end":1}),
            json!({"rust_file":"untracked.rs", "rust_item":"fn[needle]"}),
        ] {
            let error = dispatch(&sources, "habeas", &arguments).err().unwrap();
            assert!(error.to_string().contains("git add"), "{error}");
        }
        assert!(dispatch(&sources, "list_files", &json!({"path":"untracked-dir"})).is_err());
        assert!(
            dispatch(
                &sources,
                "ripgrep",
                &json!({"path":"untracked.rs", "pattern":"needle"})
            )
            .is_err()
        );
    }

    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.file_discovery`
    // {test}`requirements.authoring.text_search`
    // {test}`requirements.authoring.read_denial`
    #[cfg(unix)]
    #[test]
    fn git_symlink_reads_require_both_the_alias_and_target_to_be_tracked() {
        let root = project();
        fs::write(root.path().join("tracked.txt"), "needle allowed\n").unwrap();
        fs::write(root.path().join("untracked.txt"), "needle hidden\n").unwrap();
        for (alias, target) in [
            ("tracked-alias.txt", "tracked.txt"),
            ("untracked-target.txt", "untracked.txt"),
            ("untracked-alias.txt", "tracked.txt"),
        ] {
            std::os::unix::fs::symlink(target, root.path().join(alias)).unwrap();
        }
        git(root.path(), &["init", "--quiet"]);
        git(
            root.path(),
            &[
                "add",
                "Cargo.toml",
                "book.toml",
                "md",
                "tracked.txt",
                "tracked-alias.txt",
                "untracked-target.txt",
            ],
        );
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let allowed = dispatch(
            &sources,
            "habeas",
            &json!({"file":"tracked-alias.txt", "line_start":1}),
        )
        .unwrap();
        assert_eq!(allowed.result["input"]["text"], json!(["needle allowed"]));
        for alias in ["untracked-target.txt", "untracked-alias.txt"] {
            assert!(
                dispatch(&sources, "habeas", &json!({"file":alias, "line_start":1}))
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("git add")
            );
            assert!(
                dispatch(
                    &sources,
                    "ripgrep",
                    &json!({"path":alias, "pattern":"needle"})
                )
                .is_err()
            );
        }
        let listing = dispatch(&sources, "list_files", &json!({})).unwrap();
        let paths = listing
            .result
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["path"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(paths.contains(&"tracked-alias.txt"));
        assert!(!paths.contains(&"untracked-target.txt"));
        assert!(!paths.contains(&"untracked-alias.txt"));
        assert!(!paths.contains(&"untracked.txt"));
    }

    // {test}`requirements.configuration.evidence_paths`
    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.file_discovery`
    // {test}`requirements.authoring.text_search`
    // {test}`requirements.authoring.read_denial`
    #[test]
    fn allowed_ignored_and_external_sources_are_discoverable_but_disallowed_descendants_are_not() {
        let root = project();
        let external = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("generated/private")).unwrap();
        fs::write(
            root.path().join("generated/visible.txt"),
            "needle allowed\n",
        )
        .unwrap();
        fs::write(
            root.path().join("generated/private/hidden.txt"),
            "needle denied\n",
        )
        .unwrap();
        fs::write(external.path().join("outside.txt"), "needle external\n").unwrap();
        fs::write(root.path().join(".gitignore"), "generated/\n").unwrap();
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["add", "."]);
        let external_toml = toml::Value::String(external.path().display().to_string());
        fs::write(root.path().join("habeas.toml"), format!(
            "version = 1\n[evidence]\nallow = ['generated', {external_toml}]\ndisallow = ['generated/private']\n"
        )).unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let listing = dispatch(&sources, "list_files", &json!({})).unwrap();
        let paths = listing
            .result
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["path"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(paths.contains(&"generated"));
        assert!(paths.contains(&external.path().to_str().unwrap()));
        let search = dispatch(
            &sources,
            "ripgrep",
            &json!({"pattern":"needle", "context":0}),
        )
        .unwrap();
        assert_eq!(search.inputs.len(), 2);
        assert!(
            search
                .inputs
                .iter()
                .any(|input| input["file"] == "generated/visible.txt")
        );
        assert!(search.inputs.iter().any(|input| input["file"] == external.path().join("outside.txt").to_str().unwrap()));
        let read = dispatch(
            &sources,
            "habeas",
            &json!({"file":external.path().join("outside.txt"), "line_start":1}),
        )
        .unwrap();
        assert_eq!(read.result["input"]["text"], json!(["needle external"]));
        for (name, args) in [
            (
                "habeas",
                json!({"file":"generated/private/hidden.txt", "line_start":1}),
            ),
            ("list_files", json!({"path":"generated/private"})),
            (
                "ripgrep",
                json!({"path":"generated/private", "pattern":"needle"}),
            ),
        ] {
            assert_eq!(
                dispatch(&sources, name, &args).err().unwrap().kind(),
                io::ErrorKind::PermissionDenied
            );
        }
    }

    // {test}`requirements.authoring.generated_inputs`
    // {test}`requirements.authoring.file_discovery`
    // {test}`requirements.authoring.text_search`
    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.configuration.evidence_paths`
    // {test}`requirements.authoring.read_denial`
    #[test]
    fn cargo_output_is_discovered_and_read_without_building_or_allowing_other_target_files() {
        let root = project();
        fs::write(root.path().join("src/lib.rs"), "pub fn ready() {}\n").unwrap();
        fs::write(
            root.path().join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = 'fixture'\nversion = '0.1.0'\n",
        )
        .unwrap();
        fs::write(root.path().join(".gitignore"), "target/\n").unwrap();
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["add", "."]);
        let unbuilt = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        assert!(
            dispatch(
                &unbuilt,
                "habeas",
                &json!({"file":"src/lib.rs", "line_start":1})
            )
            .is_ok()
        );
        assert!(!root.path().join("target").exists());
        let out = root.path().join("target/debug/build/fixture-hash/out");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("generated.rs"), "pub fn generated_needle() {}\n").unwrap();
        fs::write(
            out.parent().unwrap().join("root-output"),
            out.to_str().unwrap(),
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let listing = dispatch(&sources, "list_files", &json!({})).unwrap();
        assert!(
            listing
                .result
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["path"] == "target/debug/build/fixture-hash/out")
        );
        let search = dispatch(&sources, "ripgrep", &json!({"pattern":"generated_needle"})).unwrap();
        assert_eq!(search.inputs.len(), 1);
        let read = dispatch(&sources, "habeas", &json!({"rust_file":"target/debug/build/fixture-hash/out/generated.rs", "rust_item":"fn[generated_needle]"})).unwrap();
        assert_eq!(
            read.result["input"]["text"],
            json!(["pub fn generated_needle() {}"])
        );
        assert_eq!(
            dispatch(
                &sources,
                "habeas",
                &json!({"file":"target/debug/build/fixture-hash/root-output", "line_start":1})
            )
            .err()
            .unwrap()
            .kind(),
            io::ErrorKind::PermissionDenied
        );
        fs::write(
            root.path().join("habeas.toml"),
            "version = 1\n[evidence]\ndisallow = ['target/debug/build/fixture-hash/out']\n",
        )
        .unwrap();
        let denied = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let listing = dispatch(&denied, "list_files", &json!({})).unwrap();
        assert!(
            !listing
                .result
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["path"] == "target/debug/build/fixture-hash/out")
        );
        assert!(
            dispatch(&denied, "ripgrep", &json!({"pattern":"generated_needle"}))
                .unwrap()
                .inputs
                .is_empty()
        );
        for (name, arguments) in [
            (
                "habeas",
                json!({"file":"target/debug/build/fixture-hash/out/generated.rs", "line_start":1}),
            ),
            (
                "list_files",
                json!({"path":"target/debug/build/fixture-hash/out"}),
            ),
            (
                "ripgrep",
                json!({"path":"target/debug/build/fixture-hash/out", "pattern":"generated_needle"}),
            ),
        ] {
            let error = dispatch(&denied, name, &arguments).err().unwrap();
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
            assert!(error.to_string().contains("disallowed"), "{error}");
        }
    }

    // {test}`requirements.authoring.text_search`
    // {test}`requirements.configuration.evidence_paths`
    // {test}`requirements.ci.input_containment`
    #[cfg(unix)]
    #[test]
    fn search_uses_the_validated_target_when_symlinks_precede_parent_components() {
        let root = project();
        fs::create_dir_all(root.path().join("actual/nested")).unwrap();
        fs::write(root.path().join("actual/needle.txt"), "actual needle\n").unwrap();
        fs::write(root.path().join("needle.txt"), "wrong needle\n").unwrap();
        std::os::unix::fs::symlink(root.path().join("actual/nested"), root.path().join("alias"))
            .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let result = dispatch(
            &sources,
            "ripgrep",
            &json!({"path":"alias/../needle.txt", "pattern":"needle", "context":0}),
        )
        .unwrap();
        assert_eq!(result.inputs.len(), 1);
        assert_eq!(result.inputs[0]["file"], "actual/needle.txt");
        assert_eq!(result.inputs[0]["text"], json!(["actual needle"]));
    }

    // {test}`requirements.authoring.reference_index`
    #[test]
    fn references_list_names_and_flattened_sites_without_recording_inputs() {
        let root = project();
        fs::write(
            root.path().join("md/requirements.md"),
            format!(
                ":::{{req}} ready\nThe project MUST be ready.\n:::\n\n:::{{req}} other\nOther MUST hold.\n:::\n\n:::{{example}} case\nA ready case. {}\n:::\n",
                role("cite", "requirements.ready")
            ),
        )
        .unwrap();
        fs::write(
            root.path().join("src/lib.rs"),
            format!(
                "// {}\nfn work() {{\n    // {}\n}}\n\n// {}\nfn check() {{}}\n",
                role("impl", "requirements.ready"),
                role("cite", "requirements.other"),
                role("test", "requirements.ready"),
            ),
        )
        .unwrap();
        fs::create_dir_all(root.path().join("private")).unwrap();
        fs::write(
            root.path().join("private/notes.md"),
            format!("{}\n", role("cite", "requirements.ready")),
        )
        .unwrap();
        fs::write(
            root.path().join("habeas.toml"),
            "version = 1\n\n[evidence]\ndisallow = [\"private\"]\n",
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();

        let all = dispatch(&sources, "references", &json!({})).unwrap();
        assert_eq!(
            all.result,
            json!({"definitions": [
                {"name": "requirements.case", "kind": "example"},
                {"name": "requirements.other", "kind": "requirement"},
                {"name": "requirements.ready", "kind": "requirement"},
            ], "sites": []})
        );
        assert!(all.inputs.is_empty());

        let ready = dispatch(
            &sources,
            "references",
            &json!({"selection": "requirements.ready"}),
        )
        .unwrap();
        assert_eq!(
            ready.result,
            json!({"definitions": [{"name": "requirements.ready", "kind": "requirement"}], "sites": [
                {"name": "requirements.ready", "role": "cite", "file": "md/requirements.md", "line": 10, "rust_item": null},
                {"name": "requirements.ready", "role": "impl", "file": "src/lib.rs", "line": 1, "rust_item": "fn[work]"},
                {"name": "requirements.ready", "role": "test", "file": "src/lib.rs", "line": 6, "rust_item": "fn[check]"},
            ]})
        );
        assert!(ready.inputs.is_empty());

        let flattened = dispatch(
            &sources,
            "references",
            &json!({"selection": "requirements.*"}),
        )
        .unwrap();
        let sites = flattened.result["sites"].as_array().unwrap();
        assert_eq!(flattened.result["definitions"].as_array().unwrap().len(), 3);
        assert_eq!(sites.len(), 4);
        assert_eq!(sites[0]["name"], "requirements.other");
        assert_eq!(sites[0]["rust_item"], "fn[work]");
        assert!(flattened.inputs.is_empty());

        for selection in ["requirements.missing", "bad name", ""] {
            assert!(
                dispatch(&sources, "references", &json!({"selection": selection})).is_err(),
                "accepted {selection:?}"
            );
        }
        assert!(dispatch(&sources, "references", &json!({"selection": 1})).is_err());
        assert!(
            dispatch(
                &sources,
                "references",
                &json!({"name": "requirements.ready"})
            )
            .is_err()
        );
    }

    // {test}`requirements.authoring.text_search`
    #[test]
    fn parallel_search_orders_results_by_path_and_line() {
        let root = project();
        for index in (0..40).rev() {
            fs::write(
                root.path().join(format!("src/file{index:02}.rs")),
                "// needle\nfn a() {}\n// needle\n",
            )
            .unwrap();
        }
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let first = dispatch(
            &sources,
            "ripgrep",
            &json!({"pattern": "needle", "path": "src", "context": 0}),
        )
        .unwrap();
        let files: Vec<_> = first.result["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|input| {
                (
                    input["file"].as_str().unwrap().to_owned(),
                    input["line_start"].as_u64().unwrap(),
                )
            })
            .collect();
        let mut sorted = files.clone();
        sorted.sort();
        assert_eq!(files, sorted);
        assert_eq!(files.len(), 80);
        for _ in 0..5 {
            let again = dispatch(
                &sources,
                "ripgrep",
                &json!({"pattern": "needle", "path": "src", "context": 0}),
            )
            .unwrap();
            assert_eq!(again.result, first.result);
        }
        assert!(dispatch(&sources, "ripgrep", &json!({"pattern": "("})).is_err());
    }

    // {test}`requirements.authoring.reference_index`
    // {test}`requirements.references.anchors`
    #[test]
    fn references_report_anchors_as_whole_file_sites() {
        let root = project();
        fs::create_dir_all(root.path().join("skills")).unwrap();
        fs::write(root.path().join("skills/ready.md"), "# Skill\n").unwrap();
        fs::create_dir_all(root.path().join("private")).unwrap();
        fs::write(root.path().join("private/notes.md"), "hidden\n").unwrap();
        fs::write(
            root.path().join("md/requirements.md"),
            format!(
                ":::{{req}} ready\n:impl: skills/ready.md\nThe project MUST be ready.\n:::\n\n:::{{example}} case\n:test: skills/ready.md\n:test: private/notes.md\nA ready case. {}\n:::\n",
                role("cite", "requirements.ready")
            ),
        )
        .unwrap();
        fs::write(
            root.path().join("habeas.toml"),
            "version = 1\n\n[evidence]\ndisallow = [\"private\"]\n",
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let result = dispatch(
            &sources,
            "references",
            &json!({"selection": "requirements.*"}),
        )
        .unwrap();
        assert_eq!(
            result.result["sites"],
            json!([
                {"name": "requirements.case", "role": "test", "file": "skills/ready.md", "line": null, "rust_item": null},
                {"name": "requirements.ready", "role": "cite", "file": "md/requirements.md", "line": 9, "rust_item": null},
                {"name": "requirements.ready", "role": "impl", "file": "skills/ready.md", "line": null, "rust_item": null},
            ])
        );
        assert!(result.inputs.is_empty());
    }
}

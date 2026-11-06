use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use serde_json::{Map, Value, json};

use super::inputs::AssessmentSources;
use crate::inputs::InputSelector;

// {cite}`requirements.authoring.delivered_inputs`
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
// {impl}`requirements.authoring.outline_tool`
// {impl}`requirements.authoring.reference_index`
pub(crate) fn definitions() -> Vec<Value> {
    // Providers such as Anthropic require a plain object at the schema root.
    // Describe the alternative forms here and enforce them in read().
    vec![
        json!({
            "name": "habeas",
            "description": "Read project source as evidence. Supply exactly one request form: file + line_start (optional line_end), file + item, requirements, or definition. Do not combine forms. Paths may be project-relative or absolute when permitted by the evidence policy. A line range is one-based and inclusive (omitted line_end reads through EOF) and returns {inputs, definitions}: the innermost outline nodes covering those lines, each with its item selector, kind, current line_start and line_end, and source text, e.g. {\"file\":\"src/lib.rs\",\"line_start\":10,\"line_end\":20}. Line numbers only locate sources; evidence records each node's selector and structural hash. The item form reads one outline node and returns {input, definitions}, e.g. {\"file\":\"src/lib.rs\",\"item\":\"impl[Thing]/fn[run]\"}; use the outline tool to list a file's selectors. Selectors are /-separated paths from the file's root: kind[name] for named nodes (Rust trait impls use impl[Thing/Trait]), kind@hash for anonymous nodes such as use declarations and plain-text lines (line@hash), and #N (one-based) for otherwise identical siblings; the empty item selects the whole file. Characters %, [, ], /, #, @ and whitespace in names are percent-encoded UTF-8 bytes. The requirements form takes a definition selection: a full requirement or example name, or a glob matching complete names where * matches any characters including dots, e.g. {\"requirements\":\"requirements.cli.*\"}. Definition reads use the full terminology name, e.g. {\"definition\":\"glossary.command\"}. Required terminology is returned alongside the selected input. Defaults permit project files (Git-tracked in Git worktrees) and existing Cargo build-script output directories; evidence/ and a workspace habeas.toml containing credentials are excluded. The nearest habeas.toml can explicitly allow other paths or disallow paths; disallow wins. Denied reads of project paths abort the assessment and report to the operator; a path outside the project that no rule permits returns an ordinary tool error. Paths outside the project may be absolute when allowed.",
            "parameters": {"type":"object", "properties": {
                "file":{"type":"string"},
                "line_start":{"type":"integer","minimum":1},
                "line_end":{"type":"integer","minimum":1},
                "item":{"type":"string"},
                "requirements":{"type":"string"},
                "definition":{"type":"string"}
            }, "additionalProperties":false}
        }),
        json!({
            "name": "list_files", "description": "List immediate files and directories at a project-relative or policy-permitted absolute path (default .). Returned paths can be listed or read in another call. Directory listings are not evidence inputs. Only eligible sources are listed. Root listings also expose allowed paths and discovered Cargo build-script output directories. Defaults exclude evidence/, untracked files, and workspace configuration containing credentials; do not request excluded paths. Workspace habeas.toml allow/disallow rules apply; denied explicit requests for project paths abort the assessment, while unpermitted paths outside the project return a tool error.",
            "parameters":{"type":"object","properties":{"path":{"type":"string","default":"."}},"additionalProperties":false}
        }),
        json!({
            "name":"outline", "description":"List a permitted file's outline: its selectable nodes nested by ancestry, each with its item selector (for habeas {file, item} reads), kind, name (null when anonymous), and current one-based inclusive line_start and line_end. Rust, Python, TypeScript, CSS, Markdown, TOML, and YAML files have language outlines; other text files list one line node per nonblank line. Results contain no source text and are not evidence inputs; read the nodes you rely on with habeas. Requesting an excluded project file aborts the assessment; an unpermitted file outside the project returns a tool error.",
            "parameters":{"type":"object","properties":{"file":{"type":"string"}},"required":["file"],"additionalProperties":false}
        }),
        json!({
            "name":"ripgrep", "description":"Search a project-relative or policy-permitted absolute file or directory (default .) using ripgrep regex syntax, case-sensitive matching, and normal ignore rules (including .gitignore and hidden-file exclusions). Return matching lines plus context lines before and after each match (default 5; zero means matches only). Overlapping windows are merged and returned as the innermost outline nodes containing the matching and context lines (a match in a comment returns the node the comment belongs to), each with its item selector, kind, current line range, and source text; those nodes are recorded as evidence and accompanied by required terminology. No matches returns empty arrays. Results include only eligible sources. Defaults exclude evidence/, untracked files, and workspace configuration containing credentials; do not request excluded paths. Explicitly allowed paths and existing Cargo build-script outputs are searched even when Git ignores them. Workspace disallow rules win; denied explicit requests for project paths abort the assessment, while unpermitted paths outside the project return a tool error.",
            "parameters":{"type":"object","properties":{"pattern":{"type":"string"},"path":{"type":"string","default":"."},"context":{"type":"integer","minimum":0,"default":5}},"required":["pattern"],"additionalProperties":false}
        }),
        json!({
            "name":"references", "description":"List definitions and the sources that reference them. Without selection, return every requirement and named example name with its kind. With selection (a full requirement or example name, or a glob where * matches any characters including dots, e.g. requirements.cli.*), return the selected definitions plus every {impl}, {cite}, and {test} reference to them: definition name, role, file, line, and item, the selector of the smallest enclosing outline node when the file has a language outline (null otherwise). An :impl: or :test: anchor on a definition names a complete file, or one node as path#selector, and is reported with line null and item null or that selector. {impl} and {test} mark sources that implement or verify the definition; {cite} marks sources that only rely on or refer to it. Results contain no source text and are not evidence inputs; read the sources you rely on with habeas.",
            "parameters":{"type":"object","properties":{"selection":{"type":"string"}},"additionalProperties":false}
        }),
    ]
}

// {impl}`requirements.authoring.source_tools`
// {impl}`requirements.authoring.file_discovery`
// {impl}`requirements.authoring.text_search`
// {impl}`requirements.authoring.outline_tool`
// {impl}`requirements.authoring.reference_index`
// {impl}`requirements.authoring.read_denial`
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
        "outline" => outline(sources, arguments),
        "ripgrep" => search(sources, arguments),
        "references" => references(sources, arguments),
        _ => Err(io::Error::other(format!("unknown read tool {name:?}"))),
    }
    .map_err(super::access::recoverable_os_denial)
    .map_err(super::access::recoverable_outside_request)
}

// {impl}`requirements.authoring.source_tools`
// {impl}`requirements.definitions.authoring_inputs`
// {impl}`requirements.authoring.delivered_inputs`
// {impl}`requirements.authoring.read_denial`
fn read(sources: &AssessmentSources, args: &Map<String, Value>) -> io::Result<ToolDelivery> {
    let selector = if args.contains_key("item") {
        check_keys(args, &["file", "item"])?;
        InputSelector::Outline {
            file: string(args, "file")?.to_owned(),
            item: string(args, "item")?.to_owned(),
        }
    } else if args.contains_key("file") {
        check_keys(args, &["file", "line_start", "line_end"])?;
        let file = string(args, "file")?;
        let line_start = integer(args, "line_start")?;
        let line_end = if args.contains_key("line_end") {
            Some(integer(args, "line_end")?)
        } else {
            None
        };
        let inputs = sources.read_lines(file, line_start, line_end, false)?;
        let definitions = sources.additional_definitions(&inputs)?;
        let result = json!({ "inputs": inputs, "definitions": definitions });
        let mut inputs = inputs;
        inputs.extend(definitions);
        return Ok(ToolDelivery { result, inputs });
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

// {impl}`requirements.authoring.outline_tool`
// {impl}`requirements.authoring.read_denial`
// {impl}`requirements.references.outlines`
fn outline(sources: &AssessmentSources, args: &Map<String, Value>) -> io::Result<ToolDelivery> {
    check_keys(args, &["file"])?;
    let file = string(args, "file")?;
    let parsed = sources.parse_file(file)?;
    let parsed = parsed
        .as_ref()
        .as_ref()
        .map_err(|error| io::Error::other(error.clone()))?;
    Ok(ToolDelivery {
        result: Value::Array(outline_entries(&parsed.outline, 0)),
        inputs: Vec::new(),
    })
}

/// The outline entries for a node's children, nested by ancestry.
// {impl}`requirements.authoring.outline_tool`
fn outline_entries(outline: &crate::outline::Outline, parent: usize) -> Vec<Value> {
    outline.nodes[parent]
        .children
        .iter()
        .map(|child| {
            let node = &outline.nodes[*child];
            json!({
                "item": node.selector,
                "kind": node.kind,
                "name": node.display_name(),
                "line_start": node.line_start,
                "line_end": node.line_end,
                "children": outline_entries(outline, *child),
            })
        })
        .collect()
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
    let mut sites = Vec::new();
    for reference in &found {
        let file = relative_path(&sources.root, &reference.file)?;
        let parsed = sources.outlines.parse(&sources.root, &file);
        let item = match parsed.as_ref() {
            Ok(parsed) if parsed.outline.profile.language.is_some() => {
                let index = parsed.outline.owner_at_line(reference.line);
                Some(parsed.outline.nodes[index].selector.clone())
            }
            _ => None,
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
                "item": item,
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
                    "item": (!anchor.item.is_empty()).then_some(&anchor.item),
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
        let parsed = sources.parse_file(&file)?;
        let count = match parsed.as_ref() {
            Ok(parsed) => parsed.source.lines().count(),
            Err(error) => return Err(io::Error::other(error.clone())),
        };
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
            for input in sources.read_lines(&file, line_start, Some(line_end), true)? {
                if !inputs.contains(&input) {
                    inputs.push(input);
                }
            }
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
    #[test]
    fn additional_read_flow_searches_outlines_and_reads_a_rust_file() {
        let root = project();
        fs::write(
            root.path().join("md/terms.md"),
            ":::{definition} value\n:phrase: value\nA useful value.\n:::\n",
        )
        .unwrap();
        fs::write(
            root.path().join("src/lib.rs"),
            format!(
                "// {}\nfn work() {{}}\n\nfn other() {{}}\n",
                role("term", "terms.value")
            ),
        )
        .unwrap();
        let mut sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        // The listing finds the directory's file and records nothing.
        let listing = dispatch(&sources, "list_files", &json!({"path":"src"})).unwrap();
        assert_eq!(listing.result, json!([{"path":"src/lib.rs","kind":"file"}]));
        assert!(listing.inputs.is_empty());
        // Searching the Rust file returns the innermost item containing the match.
        let search = dispatch(
            &sources,
            "ripgrep",
            &json!({"path":"src/lib.rs","pattern":"work","context":0}),
        )
        .unwrap();
        let searched = search
            .inputs
            .iter()
            .filter(|input| input["file"] == "src/lib.rs")
            .collect::<Vec<_>>();
        assert_eq!(searched.len(), 1, "{:#?}", search.inputs);
        assert_eq!(searched[0]["item"], "fn[work]");
        sources.record_inputs(&search.inputs);
        // Outlining it lists its items without content and records nothing.
        let outline = dispatch(&sources, "outline", &json!({"file":"src/lib.rs"})).unwrap();
        assert!(outline.inputs.is_empty());
        let items = outline
            .result
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["item"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(items, ["fn[work]", "fn[other]"]);
        assert!(!outline.result.to_string().contains("fn other()"));
        // Reading one item by selector returns it with its terminology.
        let item = dispatch(
            &sources,
            "habeas",
            &json!({"file":"src/lib.rs","item":"fn[other]"}),
        )
        .unwrap();
        assert_eq!(item.result["input"]["item"], "fn[other]");
        assert_eq!(item.result["input"]["text"][0], "fn other() {}");
        sources.record_inputs(&item.inputs);
        // A line range without line_end reads through EOF.
        let eof = dispatch(
            &sources,
            "habeas",
            &json!({"file":"src/lib.rs","line_start":2}),
        )
        .unwrap();
        let covered = eof.result["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|input| input["item"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(covered, ["fn[work]", "fn[other]"]);
        sources.record_inputs(&eof.inputs);
        let recorded = sources.recorded_inputs();
        for item in ["fn[work]", "fn[other]"] {
            assert!(
                recorded.contains(
                    &crate::inputs::outline_input(root.path(), "src/lib.rs", item).unwrap()
                ),
                "{item} recorded: {recorded:?}"
            );
        }
        assert!(
            recorded
                .iter()
                .any(|input| input["definition"] == "terms.value"),
            "terminology recorded: {recorded:?}"
        );
    }

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
            &json!({"file":"src/lib.rs","item":"fn[work]"}),
        )
        .unwrap();
        assert_eq!(read.inputs.len(), 2);
        assert_eq!(read.result["input"]["item"], "fn[work]");
        assert_eq!(read.result["input"]["kind"], "fn");
        assert_eq!(read.result["input"]["line_start"], 1);
        assert_eq!(read.result["input"]["line_end"], 2);
        assert_eq!(read.result["input"]["text"][1], "fn work() {}");
        assert_eq!(read.result["definitions"][0]["definition"], "terms.value");
        assert!(sources.recorded_inputs().is_empty());
        sources.record_inputs(&read.inputs);
        sources.record_inputs(&read.inputs);
        assert_eq!(sources.recorded_inputs().len(), 2);
        // Recorded outline inputs carry only the file, selector, and hash.
        assert_eq!(
            sources.recorded_inputs()[0],
            crate::inputs::outline_input(root.path(), "src/lib.rs", "fn[work]").unwrap()
        );
        let eof = dispatch(
            &sources,
            "habeas",
            &json!({"file":"src/lib.rs","line_start":2}),
        )
        .unwrap();
        assert_eq!(eof.result["inputs"].as_array().unwrap().len(), 1);
        assert_eq!(eof.result["inputs"][0]["item"], "fn[work]");
        assert_eq!(eof.result["inputs"][0]["line_end"], 2);
        assert!(
            dispatch(
                &sources,
                "habeas",
                &json!({"file":"src/lib.rs","line_start":3})
            )
            .is_err()
        );
        assert!(
            dispatch(
                &sources,
                "habeas",
                &json!({"file":"src/lib.rs","item":"fn[missing]"})
            )
            .is_err()
        );
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
            json!({"rust_file":"src/lib.rs","rust_item":"fn[work]"}),
            json!({"item":"fn[work]"}),
            json!({"file":"src/lib.rs","line_start":1,"definition":"terms.value"}),
            json!({"file":"src/lib.rs","item":"fn[work]","line_start":1}),
            json!({"file":"src/lib.rs","item":"fn[work]","requirements":"requirements.*"}),
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

    // {test}`requirements.definitions.selection`
    #[test]
    fn requirements_reads_select_requirements_and_examples_but_not_definitions() {
        let root = project();
        fs::write(
            root.path().join("md/requirements.md"),
            format!(
                ":::{{req}} ready\nThe project MUST be ready.\n:::\n\n\
                 :::{{example}} case\nA ready case. {}\n:::\n\n\
                 :::{{definition}} unused\n:phrase: unused\n\nNot referenced.\n:::\n",
                role("cite", "requirements.ready")
            ),
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let delivery = dispatch(
            &sources,
            "habeas",
            &json!({"requirements": "requirements.*"}),
        )
        .unwrap();
        assert_eq!(
            delivery.result["input"]["matches"],
            json!(["requirements.case", "requirements.ready"]),
            "{}",
            delivery.result
        );
        assert_eq!(delivery.result["definitions"], json!([]));
        let error = dispatch(
            &sources,
            "habeas",
            &json!({"requirements": "requirements.unused"}),
        )
        .err()
        .unwrap();
        assert!(
            error
                .to_string()
                .contains("matches no requirement or example"),
            "{error}"
        );
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
        // The merged window covers lines 1-11, one plain-text `line` node per
        // line; the two identical needle lines are one interchangeable node.
        assert_eq!(search.inputs.len(), 10, "{:#?}", search.inputs);
        assert!(
            search
                .inputs
                .iter()
                .all(|input| input["file"] == "sample.txt"
                    && input["kind"] == "line"
                    && input["line_start"] == input["line_end"])
        );
        let texts = search
            .inputs
            .iter()
            .map(|input| input["text"][0].as_str().unwrap())
            .collect::<Vec<_>>();
        let mut expected = lines[..11].iter().map(String::as_str).collect::<Vec<_>>();
        expected.remove(5);
        assert_eq!(texts, expected);
        let exact = dispatch(
            &sources,
            "ripgrep",
            &json!({"pattern":"needle","path":"sample.txt","context":0}),
        )
        .unwrap();
        assert_eq!(exact.inputs.len(), 1);
        assert_eq!(exact.inputs[0]["line_start"], 2);
        assert_eq!(exact.inputs[0]["text"], json!(["needle"]));
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
                &json!({"file":"evidence/generated.rs", "item":"fn[generated]"})
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
                json!({"file":path, "item":"fn[outside_needle]"}),
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

    // {test}`requirements.authoring.read_denial`
    #[cfg(unix)]
    #[test]
    fn operating_system_permission_failures_are_ordinary_read_failures() {
        use std::os::unix::fs::PermissionsExt;
        let root = project();
        fs::create_dir(root.path().join("locked")).unwrap();
        fs::write(root.path().join("locked.rs"), "fn locked() {}\n").unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        for path in ["locked", "locked.rs"] {
            fs::set_permissions(root.path().join(path), fs::Permissions::from_mode(0o000)).unwrap();
        }
        let unreadable = fs::read(root.path().join("locked.rs")).is_err();
        let results = [
            dispatch(&sources, "list_files", &json!({"path": "locked"})),
            dispatch(
                &sources,
                "habeas",
                &json!({"file": "locked.rs", "line_start": 1}),
            ),
        ];
        for path in ["locked", "locked.rs"] {
            fs::set_permissions(root.path().join(path), fs::Permissions::from_mode(0o755)).unwrap();
        }
        if !unreadable {
            // Permission bits do not restrict this user, e.g. root.
            return;
        }
        for result in results {
            let error = result.err().expect("the read fails");
            assert_ne!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
        }
    }

    // {test}`requirements.authoring.read_denial`
    #[test]
    fn untracked_diagnostics_name_git_add_and_an_allow_rule() {
        let root = project();
        fs::write(root.path().join("tracked.txt"), "tracked\n").unwrap();
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["add", "."]);
        fs::create_dir(root.path().join("untracked-dir")).unwrap();
        fs::write(root.path().join("untracked-dir/file.rs"), "fn f() {}\n").unwrap();
        fs::write(root.path().join("untracked.rs"), "fn g() {}\n").unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        for (name, arguments) in [
            ("habeas", json!({"file": "untracked.rs", "line_start": 1})),
            ("list_files", json!({"path": "untracked-dir"})),
        ] {
            let error = dispatch(&sources, name, &arguments).err().unwrap();
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
            let message = error.to_string();
            assert!(message.contains("git add"), "{message}");
            assert!(message.contains("habeas.toml allow rule"), "{message}");
        }
        // The harness neither stages nor permits the files itself.
        let status = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(root.path())
            .output()
            .unwrap();
        let status = String::from_utf8(status.stdout).unwrap();
        assert!(status.contains("?? untracked-dir/"), "{status}");
        assert!(status.contains("?? untracked.rs"), "{status}");
        assert!(!root.path().join("habeas.toml").exists());
    }

    // {test}`requirements.authoring.read_denial`
    #[cfg(unix)]
    #[test]
    fn requests_through_symlinks_leaving_the_project_are_denials() {
        let root = project();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.rs"), "fn secret() {}\n").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("outside")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("secret.rs"),
            root.path().join("outside.rs"),
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        for (name, arguments) in [
            ("habeas", json!({"file": "outside.rs", "line_start": 1})),
            (
                "habeas",
                json!({"file": "outside.rs", "item": "fn[secret]"}),
            ),
            ("outline", json!({"file": "outside.rs"})),
            (
                "ripgrep",
                json!({"path": "outside.rs", "pattern": "secret"}),
            ),
            ("list_files", json!({"path": "outside"})),
        ] {
            let error = dispatch(&sources, name, &arguments).err().unwrap();
            assert_eq!(
                error.kind(),
                io::ErrorKind::PermissionDenied,
                "{name} {arguments}: {error}"
            );
            assert!(
                error.to_string().contains("outside the project"),
                "{name} {arguments}: {error}"
            );
        }
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
            assert_eq!(read.result["inputs"][0]["text"], json!([text]));
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
            json!({"file":"untracked.rs", "item":"fn[needle]"}),
        ] {
            let error = dispatch(&sources, "habeas", &arguments).err().unwrap();
            assert!(error.to_string().contains("git add"), "{error}");
        }
        let error = dispatch(&sources, "outline", &json!({"file":"untracked.rs"}))
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
        assert!(error.to_string().contains("git add"), "{error}");
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
        assert_eq!(
            allowed.result["inputs"][0]["text"],
            json!(["needle allowed"])
        );
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
        assert_eq!(read.result["inputs"][0]["text"], json!(["needle external"]));
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
        let read = dispatch(&sources, "habeas", &json!({"file":"target/debug/build/fixture-hash/out/generated.rs", "item":"fn[generated_needle]"})).unwrap();
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
        let mut result = ready.result.clone();
        let markdown = result["sites"][0]["item"].take();
        assert!(
            markdown.as_str().unwrap().starts_with("section@"),
            "{markdown}"
        );
        assert_eq!(
            result,
            json!({"definitions": [{"name": "requirements.ready", "kind": "requirement"}], "sites": [
                {"name": "requirements.ready", "role": "cite", "file": "md/requirements.md", "line": 10, "item": null},
                {"name": "requirements.ready", "role": "impl", "file": "src/lib.rs", "line": 1, "item": "fn[work]"},
                {"name": "requirements.ready", "role": "test", "file": "src/lib.rs", "line": 6, "item": "fn[check]"},
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
        assert_eq!(sites[0]["item"], "fn[work]");
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
                "// needle\nfn a() {\n    // needle\n}\n",
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
        // Both comment matches in each file belong to its one function.
        assert_eq!(files.len(), 40);
        assert!(
            first.result["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .all(|input| input["item"] == "fn[a]" && input["line_end"] == 4)
        );
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
    fn references_report_anchors_as_file_or_node_sites() {
        let root = project();
        fs::create_dir_all(root.path().join("skills")).unwrap();
        fs::write(root.path().join("skills/ready.md"), "# Skill\n").unwrap();
        fs::create_dir_all(root.path().join("private")).unwrap();
        fs::write(root.path().join("private/notes.md"), "hidden\n").unwrap();
        fs::write(
            root.path().join("md/requirements.md"),
            format!(
                ":::{{req}} ready\n:impl: skills/ready.md#section[Skill]\nThe project MUST be ready.\n:::\n\n:::{{example}} case\n:test: skills/ready.md\n:test: private/notes.md\nA ready case. {}\n:::\n",
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
        let mut sites = result.result["sites"].clone();
        assert!(
            sites[1]["item"]
                .take()
                .as_str()
                .unwrap()
                .starts_with("section@")
        );
        assert_eq!(
            sites,
            json!([
                {"name": "requirements.case", "role": "test", "file": "skills/ready.md", "line": null, "item": null},
                {"name": "requirements.ready", "role": "cite", "file": "md/requirements.md", "line": 9, "item": null},
                {"name": "requirements.ready", "role": "impl", "file": "skills/ready.md", "line": null, "item": "section[Skill]"},
            ])
        );
        assert!(result.inputs.is_empty());
    }

    // {test}`requirements.authoring.read_denial`
    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.file_discovery`
    // {test}`requirements.authoring.text_search`
    // {test}`requirements.authoring.outline_tool`
    #[test]
    fn unpermitted_requests_outside_the_project_are_recoverable_tool_errors() {
        let root = project();
        let external = tempfile::tempdir().unwrap();
        fs::write(external.path().join("lib.rs"), "fn external() {}\n").unwrap();
        let file = external.path().join("lib.rs");
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let requests = [
            ("habeas", json!({"file": file, "line_start": 1})),
            ("habeas", json!({"file": file, "item": "fn[external]"})),
            ("list_files", json!({"path": external.path()})),
            ("outline", json!({"file": file})),
            (
                "ripgrep",
                json!({"path": external.path(), "pattern": "external"}),
            ),
        ];
        for (name, arguments) in &requests {
            let error = dispatch(&sources, name, arguments).err().unwrap();
            assert_ne!(
                error.kind(),
                io::ErrorKind::PermissionDenied,
                "{name}: {error}"
            );
            assert!(error.to_string().contains("outside the project"), "{error}");
        }
        assert!(sources.recorded_inputs().is_empty());

        // A matching disallow still aborts, as do in-project denials.
        fs::write(
            root.path().join("habeas.toml"),
            format!(
                "version = 1\n[evidence]\ndisallow = [{:?}]\n",
                external.path().to_str().unwrap()
            ),
        )
        .unwrap();
        fs::create_dir_all(root.path().join("evidence")).unwrap();
        fs::write(root.path().join("evidence/old.json"), "{}\n").unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        for (name, arguments) in &requests[..1] {
            let error = dispatch(&sources, name, arguments).err().unwrap();
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
        }
        let error = dispatch(
            &sources,
            "habeas",
            &json!({"file": "evidence/old.json", "item": ""}),
        )
        .err()
        .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
    }

    // {test}`requirements.authoring.read_denial`
    // {test}`requirements.authoring.mandatory_inputs`
    #[test]
    fn mandatory_inputs_outside_the_project_still_abort() {
        let root = project();
        let external = tempfile::tempdir().unwrap();
        fs::write(external.path().join("skill.md"), "# Skill\n").unwrap();
        fs::write(
            root.path().join("md/requirements.md"),
            format!(
                ":::{{req}} ready\n:impl: {}\nThe project MUST be ready.\n:::\n",
                external.path().join("skill.md").display()
            ),
        )
        .unwrap();
        let error = AssessmentSources::prepare(root.path(), "requirements.ready")
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
        assert!(error.to_string().contains("outside the project"), "{error}");
    }

    // {test}`requirements.authoring.source_tools`
    #[test]
    fn habeas_tool_describes_its_argument_conventions_with_an_example_per_form() {
        let definitions = definitions();
        let habeas = definitions
            .iter()
            .find(|definition| definition["name"] == "habeas")
            .expect("habeas tool is advertised");
        let description = habeas["description"].as_str().unwrap();
        for phrase in [
            // Request forms and line conventions.
            "exactly one request form",
            "one-based and inclusive",
            "omitted line_end reads through EOF",
            "{inputs, definitions}",
            "{input, definitions}",
            // Outline selector syntax.
            "kind[name]",
            "impl[Thing/Trait]",
            "kind@hash",
            "#N",
            "percent-encoded",
            // Definition selections and full definition names.
            "definition selection",
            "* matches any characters including dots",
            "full terminology name",
            // A short request example for each form.
            r#"{"file":"src/lib.rs","line_start":10,"line_end":20}"#,
            r#"{"file":"src/lib.rs","item":"impl[Thing]/fn[run]"}"#,
            r#"{"requirements":"requirements.cli.*"}"#,
            r#"{"definition":"glossary.command"}"#,
        ] {
            assert!(
                description.contains(phrase),
                "missing {phrase:?}: {description}"
            );
        }
        assert_eq!(
            habeas["parameters"]["properties"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            [
                "file",
                "line_start",
                "line_end",
                "item",
                "requirements",
                "definition"
            ]
        );
    }

    // {test}`requirements.authoring.outline_tool`
    #[test]
    fn outline_tool_is_advertised_with_a_required_file_argument() {
        let definitions = definitions();
        let outline = definitions
            .iter()
            .find(|definition| definition["name"] == "outline")
            .expect("outline tool is advertised");
        assert_eq!(
            outline["parameters"],
            json!({
                "type": "object",
                "properties": {"file": {"type": "string"}},
                "required": ["file"],
                "additionalProperties": false
            })
        );
        let description = outline["description"].as_str().unwrap();
        for phrase in [
            "item selector",
            "kind",
            "name",
            "line_start",
            "line_end",
            "no source text",
        ] {
            assert!(description.contains(phrase), "{description}");
        }
    }

    // {test}`requirements.authoring.reference_index`
    #[test]
    fn references_tool_is_advertised_with_an_optional_selection() {
        let definitions = definitions();
        let references = definitions
            .iter()
            .find(|definition| definition["name"] == "references")
            .expect("references tool is advertised");
        assert_eq!(
            references["parameters"],
            json!({
                "type": "object",
                "properties": {"selection": {"type": "string"}},
                "additionalProperties": false
            })
        );
        let description = references["description"].as_str().unwrap();
        for phrase in ["{impl}", "{cite}", "{test}", "glob", ":impl:", "null"] {
            assert!(description.contains(phrase), "{phrase}: {description}");
        }
    }

    // {test}`requirements.authoring.outline_tool`
    // {test}`requirements.references.outlines`
    #[test]
    fn outline_lists_nested_nodes_without_text_or_inputs() {
        let root = project();
        fs::write(
            root.path().join("src/lib.rs"),
            "/// Docs.\nstruct Thing;\n\nimpl Thing {\n    fn run(&self) {}\n}\n",
        )
        .unwrap();
        fs::write(root.path().join("notes.txt"), "alpha\n\nbeta\n").unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let outline = dispatch(&sources, "outline", &json!({"file":"src/lib.rs"})).unwrap();
        assert!(outline.inputs.is_empty());
        assert_eq!(
            outline.result,
            json!([
                {"item": "struct[Thing]", "kind": "struct", "name": "Thing", "line_start": 1, "line_end": 2, "children": []},
                {"item": "impl[Thing]", "kind": "impl", "name": "Thing", "line_start": 4, "line_end": 6, "children": [
                    {"item": "impl[Thing]/fn[run]", "kind": "fn", "name": "run", "line_start": 5, "line_end": 5, "children": []},
                ]},
            ])
        );
        assert!(!outline.result.to_string().contains("Docs"));
        let plain = dispatch(&sources, "outline", &json!({"file":"notes.txt"})).unwrap();
        let entries = plain.result.as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|entry| entry["kind"] == "line"
            && entry["name"].is_null()
            && entry["item"].as_str().unwrap().starts_with("line@")));
        assert_eq!(entries[1]["line_start"], 3);
        assert!(dispatch(&sources, "outline", &json!({"file":"missing.rs"})).is_err());
        assert!(dispatch(&sources, "outline", &json!({})).is_err());

        fs::write(
            root.path().join("habeas.toml"),
            "version = 1\n[evidence]\ndisallow = ['notes.txt']\n",
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let error = dispatch(&sources, "outline", &json!({"file":"notes.txt"}))
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
    }

    // {test}`requirements.authoring.source_tools`
    // {test}`requirements.authoring.delivered_inputs`
    // {test}`requirements.ci.outline_inputs`
    #[test]
    fn line_range_reads_record_the_innermost_covering_nodes() {
        let root = project();
        let source = "use std::fmt;\n\nfn first() {}\n\nmod inner {\n    fn second() {\n        let x = 1;\n    }\n}\n";
        fs::write(root.path().join("src/lib.rs"), source).unwrap();
        let mut sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        // Blank lines between nodes do not select their common ancestor.
        let read = dispatch(
            &sources,
            "habeas",
            &json!({"file":"src/lib.rs","line_start":3,"line_end":7}),
        )
        .unwrap();
        let items = read.result["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|input| {
                (
                    input["item"].as_str().unwrap(),
                    input["line_start"].as_u64().unwrap(),
                    input["line_end"].as_u64().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            items,
            [("fn[first]", 3, 3), ("mod[inner]", 5, 9)],
            "line 5 is mod inner's own line, so the module is selected"
        );
        let nested = dispatch(
            &sources,
            "habeas",
            &json!({"file":"src/lib.rs","line_start":7,"line_end":7}),
        )
        .unwrap();
        assert_eq!(nested.result["inputs"][0]["item"], "mod[inner]/fn[second]");
        assert_eq!(
            nested.result["inputs"][0]["text"],
            json!(["    fn second() {", "        let x = 1;", "    }"])
        );
        sources.record_inputs(&nested.inputs);
        assert_eq!(
            sources.recorded_inputs(),
            [
                crate::inputs::outline_input(root.path(), "src/lib.rs", "mod[inner]/fn[second]")
                    .unwrap()
            ]
        );
        assert!(sources.recorded_inputs()[0].get("line_start").is_none());
        assert!(sources.recorded_inputs()[0].get("text").is_none());
        let whole = dispatch(
            &sources,
            "habeas",
            &json!({"file":"src/lib.rs","line_start":1}),
        )
        .unwrap();
        assert!(
            whole.result["inputs"][0]["item"]
                .as_str()
                .unwrap()
                .starts_with("use@")
        );
        assert_eq!(whole.result["inputs"].as_array().unwrap().len(), 3);
    }

    // {test}`requirements.authoring.text_search`
    // {test}`requirements.references.outline_comments`
    #[test]
    fn search_returns_innermost_nodes_and_comment_owners() {
        let root = project();
        fs::write(
            root.path().join("src/lib.rs"),
            "fn outer() {\n    fn inner() {\n        needle();\n    }\n}\n\n// needle in a comment\nfn after() {}\n",
        )
        .unwrap();
        let sources = AssessmentSources::prepare(root.path(), "requirements.ready").unwrap();
        let search = dispatch(
            &sources,
            "ripgrep",
            &json!({"pattern":"needle","path":"src/lib.rs","context":0}),
        )
        .unwrap();
        let items = search
            .inputs
            .iter()
            .map(|input| input["item"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(items, ["fn[outer]/fn[inner]", "fn[after]"]);
        assert_eq!(search.inputs[1]["line_start"], 7);
        assert_eq!(
            search.inputs[1]["text"],
            json!(["// needle in a comment", "fn after() {}"])
        );
        assert!(search.result["inputs"][0]["hash"].is_string());
    }
}

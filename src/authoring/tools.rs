use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use serde_json::{Map, Value, json};

use super::inputs::{AssessmentSources, generated_evidence_path};
use crate::inputs::{InputSelector, read_project_file};

pub(crate) struct ToolDelivery {
    pub(crate) result: Value,
    pub(crate) inputs: Vec<Value>,
}

// {impl}`requirements.authoring.source_tools`
// {impl}`requirements.authoring.file_discovery`
// {impl}`requirements.authoring.text_search`
pub(crate) fn definitions() -> Vec<Value> {
    // Providers such as Anthropic require a plain object at the schema root.
    // Describe the alternative forms here and enforce them in read().
    vec![
        json!({
            "name": "habeas",
            "description": "Read project source as one evidence input. Supply exactly one request form: file + line_start (optional line_end), rust_file + rust_item, requirements, or definition. Do not combine forms. File paths are relative to the project; line numbers are one-based and inclusive; omitted line_end reads through EOF. Example: {\"file\":\"src/lib.rs\",\"line_start\":1}. Rust selectors name complete syntactic items, with / separating enclosing items, kind[name] segments, and #N for duplicate siblings (one-based), e.g. {\"rust_file\":\"src/lib.rs\",\"rust_item\":\"impl[Thing]/fn[run]\"}. Trait impls use impl[Thing/Trait], and positional imports use use#1. Delimiters %, [, ], /, # and whitespace inside names or impl types use percent-encoded UTF-8 bytes. Requirements globs match complete requirement/example names; * matches any characters including dots, e.g. {\"requirements\":\"requirements.cli.*\"}. Definition reads use the full terminology name, e.g. {\"definition\":\"glossary.command\"}. Required terminology is returned alongside the selected input. Generated evidence under evidence/ is excluded from source reads.",
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
            "name": "list_files", "description": "List immediate files and directories at a project-relative path (default .). Returned paths can be listed or read in another call. Directory listings are not evidence inputs. Generated evidence under evidence/ is excluded from discovery.",
            "parameters":{"type":"object","properties":{"path":{"type":"string","default":"."}},"additionalProperties":false}
        }),
        json!({
            "name":"ripgrep", "description":"Search a project-relative file or directory (default .) using ripgrep regex syntax, case-sensitive matching, and normal ignore rules (including .gitignore and hidden-file exclusions). Return matching lines plus context lines before and after each match (default 5; zero means matches only). Overlapping windows are merged, recorded as file-range evidence, and accompanied by required terminology. No matches returns empty arrays. Generated evidence under evidence/ is excluded from searches.",
            "parameters":{"type":"object","properties":{"pattern":{"type":"string"},"path":{"type":"string","default":"."},"context":{"type":"integer","minimum":0,"default":5}},"required":["pattern"],"additionalProperties":false}
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
) -> Result<ToolDelivery, String> {
    let arguments = arguments
        .as_object()
        .ok_or_else(|| "tool arguments must be an object".to_owned())?;
    match name {
        "habeas" => read(sources, arguments),
        "list_files" => list_files(sources, arguments),
        "ripgrep" => search(sources, arguments),
        _ => Err(format!("unknown read tool {name:?}")),
    }
}

// {impl}`requirements.authoring.source_tools`
// {impl}`requirements.definitions.authoring_inputs`
// {impl}`requirements.authoring.delivered_inputs`
fn read(sources: &AssessmentSources, args: &Map<String, Value>) -> Result<ToolDelivery, String> {
    let selector = if args.contains_key("file") {
        check_keys(args, &["file", "line_start", "line_end"])?;
        let file = string(args, "file")?.to_owned();
        if generated_evidence_path(&sources.root, Path::new(&file)) {
            return Err("generated evidence is output, not assessment source".to_owned());
        }
        let line_start = integer(args, "line_start")?;
        let line_end = if args.contains_key("line_end") {
            integer(args, "line_end")?
        } else {
            read_project_file(&sources.root, &file)?.1.lines().count()
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
fn list_files(
    sources: &AssessmentSources,
    args: &Map<String, Value>,
) -> Result<ToolDelivery, String> {
    check_keys(args, &["path"])?;
    let path = optional_path(args)?;
    let directory = project_path(&sources.root, path)?;
    let mut entries = Vec::new();
    for entry in fs::read_dir(&directory).map_err(|error| format!("cannot list {path}: {error}"))? {
        let entry = entry.map_err(|error| format!("cannot list {path}: {error}"))?;
        if generated_evidence_path(&sources.root, &entry.path()) {
            continue;
        }
        let metadata = fs::metadata(entry.path())
            .map_err(|error| format!("cannot inspect {}: {error}", entry.path().display()))?;
        let kind = if metadata.is_dir() {
            "directory"
        } else if metadata.is_file() {
            "file"
        } else {
            continue;
        };
        let relative = relative_path(&sources.root, &entry.path())?;
        entries.push((relative, kind));
    }
    entries.sort();
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

// {impl}`requirements.authoring.text_search`
// {impl}`requirements.authoring.source_tools`
fn search(sources: &AssessmentSources, args: &Map<String, Value>) -> Result<ToolDelivery, String> {
    check_keys(args, &["pattern", "path", "context"])?;
    let pattern = string(args, "pattern")?;
    let path = optional_path(args)?;
    project_path(&sources.root, path)?;
    let context = if args.contains_key("context") {
        integer(args, "context")?
    } else {
        5
    };
    let output = Command::new("rg")
        .current_dir(&sources.root)
        .args([
            "--json",
            "--no-config",
            "--case-sensitive",
            "--color=never",
            "--glob",
            "!/evidence/**",
            "--",
            pattern,
            path,
        ])
        .output()
        .map_err(|error| format!("cannot run ripgrep: {error}"))?;
    if !output.status.success() && output.status.code() != Some(1) {
        return Err(format!(
            "ripgrep failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let mut matches: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for line in output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let event: Value = serde_json::from_slice(line)
            .map_err(|error| format!("invalid ripgrep output: {error}"))?;
        if event["type"] != "match" {
            continue;
        }
        let file = event["data"]["path"]["text"]
            .as_str()
            .ok_or_else(|| "ripgrep found a non-UTF-8 path".to_owned())?;
        if generated_evidence_path(&sources.root, Path::new(file)) {
            continue;
        }
        let number = event["data"]["line_number"]
            .as_u64()
            .and_then(|number| usize::try_from(number).ok())
            .ok_or_else(|| "ripgrep returned an invalid line number".to_owned())?;
        let file = relative_path(&sources.root, &sources.root.join(file))?;
        matches.entry(file).or_default().push(number);
    }
    let mut inputs = Vec::new();
    for (file, mut lines) in matches {
        let (_, source) = read_project_file(&sources.root, &file)?;
        let count = source.lines().count();
        lines.sort_unstable();
        let mut windows: Vec<(usize, usize)> = Vec::new();
        for line in lines {
            if line == 0 || line > count {
                return Err(format!("{file}: ripgrep returned an out-of-range match"));
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
fn check_keys(args: &Map<String, Value>, allowed: &[&str]) -> Result<(), String> {
    for key in args.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("unexpected tool argument {key:?}"));
        }
    }
    Ok(())
}

// {impl}`requirements.authoring.source_tools`
// {impl}`requirements.authoring.text_search`
fn string<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{key} must be a string"))
}

// {impl}`requirements.authoring.source_tools`
// {impl}`requirements.authoring.text_search`
fn integer(args: &Map<String, Value>, key: &str) -> Result<usize, String> {
    args.get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| format!("{key} must be a nonnegative integer"))
}

// {impl}`requirements.authoring.file_discovery`
// {impl}`requirements.authoring.text_search`
fn optional_path(args: &Map<String, Value>) -> Result<&str, String> {
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
fn project_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    if relative.is_empty()
        || !Path::new(relative)
            .components()
            .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
    {
        return Err("path must be relative to the project".to_owned());
    }
    let path = fs::canonicalize(root.join(relative))
        .map_err(|error| format!("cannot open {relative}: {error}"))?;
    if !path.starts_with(root) {
        return Err(format!("{relative} resolves outside the project"));
    }
    if generated_evidence_path(root, &path) {
        return Err("generated evidence is output, not assessment source".to_owned());
    }
    Ok(path)
}

// {impl}`requirements.authoring.file_discovery`
// {impl}`requirements.authoring.text_search`
fn relative_path(root: &Path, path: &Path) -> Result<String, String> {
    path.strip_prefix(root)
        .map_err(|_| "path is outside the project".to_owned())?
        .components()
        .filter(|part| *part != Component::CurDir)
        .collect::<PathBuf>()
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| "path is not UTF-8".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
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
    #[test]
    fn search_merges_context_windows_and_respects_ignore_rules() {
        let root = project();
        fs::create_dir(root.path().join(".git")).unwrap();
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
            .contains("generated evidence")
        );
        assert!(
            dispatch(&sources, "list_files", &json!({"path":"evidence"}))
                .err()
                .unwrap()
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
            .contains("generated evidence")
        );
        #[cfg(unix)]
        {
            for path in ["old.json", "old-output/old.json"] {
                assert!(
                    dispatch(&sources, "habeas", &json!({"file":path, "line_start":1}))
                        .err()
                        .unwrap()
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
                    .contains("generated evidence")
                );
            }
            assert!(
                dispatch(&sources, "list_files", &json!({"path":"old-output"}))
                    .err()
                    .unwrap()
                    .contains("generated evidence")
            );
        }
    }
}

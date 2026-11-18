//! Versioned SDK fields alongside Habeas's existing application fields.

use crate::{
    judgment::Judgment,
    sdk::{HabeasDomain, HabeasEdgeLabel, HabeasNode, input_fingerprint},
};
use habeas_sdk::graph::{Dependency, Fingerprint, Record};
use serde_json::{Value, json};
use std::{
    io::{self, Write},
    path::Path,
};

const SDK_FIELDS: [&str; 3] = ["format_version", "review_version", "dependencies"];

pub(crate) fn is_versioned(value: &Value) -> bool {
    SDK_FIELDS.iter().any(|field| value.get(*field).is_some())
}

fn versioned_fields(
    value: &Value,
) -> io::Result<(String, Vec<Dependency<HabeasNode, HabeasEdgeLabel>>)> {
    if value["format_version"].as_u64() != Some(1) {
        return Err(io::Error::other(format!(
            "unsupported evidence format version {:?}",
            value["format_version"]
        )));
    }
    let version = value["review_version"]
        .as_str()
        .filter(|version| !version.is_empty())
        .ok_or_else(|| io::Error::other("versioned evidence requires a nonempty review_version"))?;
    let mut wire_dependencies: Value = value
        .get("dependencies")
        .cloned()
        .ok_or_else(|| io::Error::other("versioned evidence requires dependencies"))?;
    normalize_dependencies(&mut wire_dependencies)?;
    let dependencies = serde_json::from_value(wire_dependencies).map_err(io::Error::other)?;
    Ok((version.into(), dependencies))
}

// {cite}`habeas:util`
fn normalize_dependencies(dependencies: &mut Value) -> io::Result<()> {
    let entries = dependencies
        .as_array_mut()
        .ok_or_else(|| io::Error::other("dependencies must be an array"))?;
    for entry in entries {
        if let Some(old) = entry
            .get("OutgoingEdges")
            .and_then(Value::as_array)
            .filter(|old| old.len() == 2)
        {
            let node: HabeasNode =
                serde_json::from_value(old[0].clone()).map_err(io::Error::other)?;
            let fingerprint: Fingerprint =
                serde_json::from_value(old[1].clone()).map_err(io::Error::other)?;
            let HabeasNode::Requirement(id) = node else {
                return Err(io::Error::other(
                    "historical outgoing dependency requires a requirement",
                ));
            };
            if fingerprint.scheme != "habeas-reference-set-v1" {
                return Err(io::Error::other("unknown historical outgoing fingerprint"));
            }
            *entry = serde_json::to_value(Dependency::<HabeasNode, HabeasEdgeLabel>::Node(
                HabeasNode::ReferenceSet(id),
                fingerprint,
            ))
            .map_err(io::Error::other)?;
        }
    }
    Ok(())
}

/// Read-only semantic view used by the existing application checks.
// {impl}`requirements.sdk.habeas.evidence_format`
// {impl}`requirements.sdk.habeas.evidence_format.errors`
pub(crate) fn view(value: &Value) -> io::Result<Value> {
    if !is_versioned(value) {
        return Ok(value.clone());
    }
    versioned_fields(value)?;
    let mut object = value
        .as_object()
        .cloned()
        .ok_or_else(|| io::Error::other("expected an evidence object"))?;
    for field in SDK_FIELDS {
        object.remove(field);
    }
    Ok(Value::Object(object))
}

// {impl}`requirements.sdk.habeas.evidence_conversion.provenance`
// {impl}`requirements.sdk.habeas.evidence_conversion.legacy_freshness`
pub(crate) fn decode(
    value: &Value,
    subject: &HabeasNode,
) -> io::Result<Record<HabeasNode, Value, HabeasEdgeLabel>> {
    let metadata = view(value)?;
    check_shape(&metadata, subject)?;
    let determination =
        Judgment::from_value(&metadata["determination"]).map_err(io::Error::other)?;
    let rationale = metadata["rationale"]
        .as_str()
        .filter(|rationale| !rationale.trim().is_empty())
        .ok_or_else(|| io::Error::other("evidence rationale must be nonempty"))?
        .to_owned();
    let (procedure, dependencies) = if is_versioned(value) {
        versioned_fields(value)?
    } else {
        let inputs = metadata["inputs"]
            .as_array()
            .ok_or_else(|| io::Error::other("evidence inputs must be an array"))?;
        let mut dependencies = Vec::new();
        for input in inputs {
            let selector = crate::inputs::InputSelector::from_input(input)
                .ok_or_else(|| io::Error::other("invalid historical evidence input"))?;
            dependencies.push(Dependency::Node(
                HabeasNode::from(&selector),
                input_fingerprint(input),
            ));
        }
        if matches!(subject, HabeasNode::Requirement(_)) {
            let digest = metadata["references"]
                .as_str()
                .ok_or_else(|| io::Error::other("evidence requires references"))?;
            dependencies.push(Dependency::Node(
                match subject {
                    HabeasNode::Requirement(id) => HabeasNode::ReferenceSet(id.clone()),
                    _ => unreachable!(),
                },
                Fingerprint {
                    scheme: "habeas-reference-set-v1".into(),
                    value: digest.into(),
                },
            ));
        }
        ("*".into(), dependencies)
    };
    Ok(Record {
        procedure,
        dependencies,
        output: habeas_sdk::graph::Judgment {
            result: determination != Judgment::False,
            rationale,
            metadata,
        },
    })
}

/// Validate stored forms without consulting current sources or review policy.
fn check_shape(value: &Value, subject: &HabeasNode) -> io::Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| io::Error::other("evidence must be an object"))?;
    let fields: &[&str] = match subject {
        HabeasNode::Requirement(_) if object.contains_key("example") => &[
            "example",
            "determination",
            "rationale",
            "references",
            "inputs",
        ],
        HabeasNode::Requirement(_) => &[
            "requirement",
            "determination",
            "rationale",
            "references",
            "inputs",
        ],
        HabeasNode::CitationReview(_) => &[
            "file",
            "item",
            "citations",
            "determination",
            "rationale",
            "inputs",
        ],
        HabeasNode::SiteReview { .. } => &[
            "requirement",
            "file",
            "item",
            "queries",
            "applies",
            "fixmes",
            "determination",
            "rationale",
            "inputs",
        ],
        _ => return Err(io::Error::other("not a Habeas review identity")),
    };
    if object.len() != fields.len() || fields.iter().any(|field| !object.contains_key(*field)) {
        return Err(io::Error::other(
            "evidence fields do not match the review kind",
        ));
    }
    for field in fields.iter().filter(|field| {
        !matches!(
            **field,
            "determination" | "inputs" | "citations" | "queries" | "fixmes" | "applies"
        )
    }) {
        if !value[*field].is_string() {
            return Err(io::Error::other(format!(
                "evidence {field} must be a string"
            )));
        }
    }
    for field in ["inputs", "citations", "queries", "fixmes"] {
        if object.contains_key(field) && !value[field].is_array() {
            return Err(io::Error::other(format!(
                "evidence {field} must be an array"
            )));
        }
    }
    if object.contains_key("applies") && !value["applies"].is_boolean() {
        return Err(io::Error::other("evidence applies must be a boolean"));
    }
    for input in value["inputs"].as_array().expect("checked inputs") {
        let object = input
            .as_object()
            .ok_or_else(|| io::Error::other("evidence input must be an object"))?;
        let selector = crate::inputs::InputSelector::from_input(input)
            .ok_or_else(|| io::Error::other("invalid historical evidence input"))?;
        let valid = match selector {
            crate::inputs::InputSelector::Outline { .. } => {
                object.len() == 3
                    && input["file"].is_string()
                    && input["item"].is_string()
                    && input["hash"].as_str().is_some_and(hex_digest)
            }
            crate::inputs::InputSelector::Definition { .. } => {
                object.len() == 2 && input["definition"].is_string() && string_array(&input["text"])
            }
            crate::inputs::InputSelector::Requirements { .. } => {
                object.len() == 3
                    && input["requirements"].is_string()
                    && string_array(&input["matches"])
                    && string_array(&input["text"])
            }
        };
        if !valid {
            return Err(io::Error::other("malformed historical evidence input"));
        }
    }
    if object.contains_key("references") && !value["references"].as_str().is_some_and(hex_digest) {
        return Err(io::Error::other(
            "evidence references must be a lowercase hexadecimal SHA-256 digest",
        ));
    }
    Ok(())
}

fn string_array(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|values| values.iter().all(Value::is_string))
}
fn hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

// {impl}`requirements.sdk.habeas.evidence_format`
pub(crate) fn encode(record: &Record<HabeasNode, Value, HabeasEdgeLabel>) -> io::Result<Value> {
    let mut value = record.output.metadata.clone();
    let object = value
        .as_object_mut()
        .ok_or_else(|| io::Error::other("Habeas judgment metadata must be an object"))?;
    let determination = Judgment::from_value(object.get("determination").unwrap_or(&Value::Null))
        .map_err(io::Error::other)?;
    if (determination != Judgment::False) != record.output.result
        || object.get("rationale").and_then(Value::as_str) != Some(record.output.rationale.as_str())
    {
        return Err(io::Error::other(
            "SDK judgment and Habeas determination or rationale disagree",
        ));
    }
    object.insert("format_version".into(), json!(1));
    object.insert("review_version".into(), json!(record.procedure));
    object.insert(
        "dependencies".into(),
        serde_json::to_value(&record.dependencies).map_err(io::Error::other)?,
    );
    Ok(value)
}

// {impl}`requirements.sdk.habeas.evidence_conversion.atomic`
pub(crate) fn write_atomic(path: &Path, value: &Value) -> io::Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    bytes.push(b'\n');
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("evidence path has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    if let Err(error) = file.write_all(&bytes) {
        file.close()?;
        return Err(error);
    }
    match file.persist(path) {
        Ok(_) => Ok(()),
        Err(error) => {
            error.file.close()?;
            Err(error.error)
        }
    }
}

/// Called only for application-validated current legacy records in a write path.
// {impl}`requirements.sdk.habeas.evidence_conversion`
// {impl}`requirements.sdk.habeas.evidence_conversion.writes`
// {impl}`requirements.sdk.habeas.evidence_conversion.idempotent`
pub(crate) fn convert_file(path: &Path, subject: &HabeasNode) -> io::Result<()> {
    let bytes = std::fs::read(path)?;
    let value: Value = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if is_versioned(&value) {
        // Validate an existing version before declaring conversion unnecessary.
        decode(&value, subject)?;
        return Ok(());
    }
    write_atomic(path, &encode(&decode(&value, subject)?)?)
}

/// Application checks still report their detailed field-specific failures.
pub(crate) fn check_native(
    value: &Value,
    subject: &HabeasNode,
    domain: &HabeasDomain<'_>,
    label: &str,
    errors: &mut Vec<String>,
) {
    if !is_versioned(value) {
        return;
    }
    let current = crate::sdk::review_version(subject);
    // The application validators report application fields individually.
    // Freshness needs only the SDK header, not a second strict application decode.
    let record = versioned_fields(value).map(|(procedure, dependencies)| Record {
        procedure,
        dependencies,
        output: habeas_sdk::graph::Judgment {
            result: false,
            rationale: String::new(),
            metadata: value.clone(),
        },
    });
    match current.and_then(|version| {
        record.and_then(|mut record| {
            let reference_set_changed = match subject {
                HabeasNode::Requirement(id)
                    if record.dependencies.iter().any(|dependency| {
                        matches!(dependency, Dependency::OutgoingEdges(node, HabeasEdgeLabel::Mandatory, _) if node == subject)
                    }) =>
                {
                    record.output.metadata["references"]
                        .as_str()
                        .is_some_and(|references| {
                            domain.reference_set(id).is_ok_and(|current| current != references)
                        })
                }
                _ => false,
            };
            // Existing application checks give precise diagnostics for these
            // same historical observations. Keep generic findings for versions
            // and additional SDK observations that those checks cannot explain.
            record.dependencies.retain(|dependency| match dependency {
                Dependency::Node(HabeasNode::ReferenceSet(id), fingerprint)
                    if matches!(subject, HabeasNode::Requirement(subject_id) if subject_id == id)
                        && fingerprint.scheme == "habeas-reference-set-v1" =>
                {
                    record.output.metadata["references"].as_str() != Some(&fingerprint.value)
                }
                Dependency::Node(node, fingerprint) => !record.output.metadata["inputs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|input| {
                        crate::inputs::InputSelector::from_input(input).is_some_and(|selector| {
                            node.selector().ok().as_ref() == Some(&selector)
                        }) && input_fingerprint(input) == *fingerprint
                    }),
                Dependency::OutgoingEdges(node, _label, fingerprint) => {
                    !(node == subject
                        && matches!(subject, HabeasNode::Requirement(_))
                        && ((_label == &HabeasEdgeLabel::Mandatory && reference_set_changed)
                            || (fingerprint.scheme == "habeas-reference-set-v1"
                                && record.output.metadata["references"].as_str()
                                    == Some(&fingerprint.value))))
                }
            });
            habeas_sdk::graph::record_is_current(domain, &version, &record)
        })
    }) {
        Ok(true) => {}
        Ok(false) => errors.push(format!("{label}: review version or SDK dependency changed")),
        Err(error) => errors.push(format!("{label}: cannot check SDK evidence: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{authoring::AuthoringSubject, inputs::OutlineCache};
    use habeas_sdk::graph::{EvidenceStorage, Graph};
    use std::collections::BTreeMap;

    fn legacy(subject: &AuthoringSubject) -> Value {
        let base = json!({"determination":{"FIXME":["example/project#7"]},
            "rationale":"Historical rationale.", "inputs":[
                {"file":"deleted.rs","item":"fn[old]","hash":"a".repeat(64)},
                {"requirements":"old.ready","matches":["old.ready"],"text":["Historical text."]}]});
        let mut value = base;
        match subject {
            AuthoringSubject::Definition(_) => {
                value["requirement"] = json!("Historical text.");
                value["references"] = json!("b".repeat(64));
            }
            AuthoringSubject::Citation { file, item } => {
                value["file"] = json!(file);
                value["item"] = json!(item);
                value["citations"] = json!([{"role":"cite","name":"old.ready","commentary":"Historical commentary."}]);
            }
            AuthoringSubject::Site {
                requirement,
                file,
                item,
            } => {
                value["requirement"] = json!(requirement);
                value["file"] = json!(file);
                value["item"] = json!(item);
                value["queries"] = json!(["kind: function_item"]);
                value["applies"] = json!(true);
                value["fixmes"] = json!([]);
            }
        }
        value
    }

    // {test}`requirements.sdk.habeas.evidence_format`
    // {test}`requirements.sdk.habeas.evidence_conversion`
    // {test}`requirements.sdk.habeas.evidence_conversion.provenance`
    // {test}`requirements.sdk.habeas.evidence_conversion.legacy_freshness`
    #[test]
    fn legacy_conversion_preserves_every_historical_field_without_resolving_sources() {
        let subjects = [
            AuthoringSubject::Definition("old.ready".into()),
            AuthoringSubject::Citation {
                file: "deleted.rs".into(),
                item: "fn[old]".into(),
            },
            AuthoringSubject::Site {
                requirement: "old.ready".into(),
                file: "deleted.rs".into(),
                item: "fn[old]".into(),
            },
        ];
        for subject in subjects {
            let old = legacy(&subject);
            let node = HabeasNode::review_subject(&subject);
            let record = decode(&old, &node).unwrap();
            assert_eq!(record.procedure, "*");
            assert_eq!(record.output.metadata, old);
            assert_eq!(record.output.rationale, "Historical rationale.");
            assert!(record.output.result);
            assert_eq!(
                record.dependencies[0],
                Dependency::Node(
                    HabeasNode::from(
                        &crate::inputs::InputSelector::from_input(&old["inputs"][0]).unwrap()
                    ),
                    input_fingerprint(&old["inputs"][0])
                )
            );
            assert_eq!(
                record.dependencies.len(),
                if matches!(subject, AuthoringSubject::Definition(_)) {
                    3
                } else {
                    2
                }
            );
            let native = encode(&record).unwrap();
            assert_eq!(native["format_version"], 1);
            assert_eq!(native["review_version"], "*");
            assert_eq!(view(&native).unwrap(), old);
            assert_eq!(
                decode(&native, &node).unwrap().dependencies,
                record.dependencies
            );
            assert!(!native.to_string().contains("source text"));
        }
    }

    // {test}`requirements.sdk.habeas.evidence_format.errors`
    #[test]
    fn partial_headers_and_invalid_records_never_fall_back_to_legacy() {
        let subject = AuthoringSubject::Definition("old.ready".into());
        let node = HabeasNode::review_subject(&subject);
        for header in [
            json!({"format_version":2}),
            json!({"review_version":"v1"}),
            json!({"dependencies":[]}),
            json!({"format_version":1,"review_version":"","dependencies":[]}),
            json!({"format_version":1,"review_version":"v1","dependencies":[{"bad":true}]}),
        ] {
            let mut value = legacy(&subject);
            value
                .as_object_mut()
                .unwrap()
                .extend(header.as_object().unwrap().clone());
            assert!(view(&value).is_err());
            assert!(decode(&value, &node).is_err());
        }
        let mut value = legacy(&subject);
        value["determination"] = json!("bad");
        assert!(decode(&value, &node).is_err());
        for field in ["requirement", "references", "inputs"] {
            let mut malformed = legacy(&subject);
            malformed.as_object_mut().unwrap().remove(field);
            assert!(decode(&malformed, &node).is_err());
        }
        let mut malformed = legacy(&subject);
        malformed["inputs"][0]["hash"] = json!("not a digest");
        assert!(decode(&malformed, &node).is_err());
        let mut record = decode(&legacy(&subject), &node).unwrap();
        record.output.result = false;
        assert!(encode(&record).is_err());
    }

    // {test}`requirements.sdk.habeas.evidence_conversion.atomic`
    // {test}`requirements.sdk.habeas.evidence_conversion.idempotent`
    // {test}`requirements.sdk.habeas.evidence_conversion.writes`
    // {test}`requirements.sdk.habeas.evidence_format.loading`
    #[tokio::test]
    async fn loading_is_read_only_and_conversion_is_in_place_idempotent_and_preserves_failures() {
        let root = tempfile::tempdir().unwrap();
        let subject = AuthoringSubject::Definition("old.ready".into());
        let node = HabeasNode::review_subject(&subject);
        let path = subject.evidence_path(root.path());
        write_atomic(&path, &legacy(&subject)).unwrap();
        let before = std::fs::read(&path).unwrap();
        let definitions = BTreeMap::new();
        let outlines = OutlineCache::default();
        let graph = HabeasDomain {
            root: root.path(),
            definitions: &definitions,
            outlines: &outlines,
        };
        let record = graph.load_evidence(&node).await.unwrap().unwrap();
        assert_eq!(record.procedure, "*");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        convert_file(&path, &node).unwrap();
        let after = std::fs::read(&path).unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_ne!(after, before);
        convert_file(&path, &node).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), after);
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            modified
        );
        std::fs::write(&path, b"malformed").unwrap();
        assert!(convert_file(&path, &node).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"malformed");
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
        let missing = HabeasNode::Requirement(crate::sdk::RequirementId("missing".into()));
        assert!(graph.load_evidence(&missing).await.unwrap().is_none());
        assert!(!missing.evidence_path(root.path()).unwrap().exists());
    }

    // {test}`requirements.sdk.habeas.evidence_conversion.legacy_freshness`
    // {test}`requirements.sdk.habeas.evidence_format`
    #[test]
    fn native_freshness_checks_versions_inputs_and_application_fields() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("md")).unwrap();
        std::fs::write(root.path().join("book.toml"), "[book]\nsrc='md'\n").unwrap();
        std::fs::write(
            root.path().join("md/rules.md"),
            ":::{req} ready\nReady MUST work.\n:::\n",
        )
        .unwrap();
        let definitions = crate::evidence::load_definitions(root.path()).unwrap();
        let outlines = OutlineCache::default();
        let graph = HabeasDomain {
            root: root.path(),
            definitions: &definitions,
            outlines: &outlines,
        };
        let node = HabeasNode::Requirement(crate::sdk::RequirementId("rules.ready".into()));
        let input = node
            .selector()
            .unwrap()
            .to_input(&graph.node_content(&node).unwrap());
        let metadata = json!({"requirement":"Ready MUST work.","references":"b".repeat(64),
            "determination":false,"rationale":"Still negative.","inputs":[input]});
        let mut record = decode(&metadata, &node).unwrap();
        // Isolate input/version comparison; reference checks remain application policy.
        record
            .dependencies
            .retain(|dependency| matches!(dependency, Dependency::Node(node, _) if !matches!(node, HabeasNode::ReferenceSet(_))));
        assert!(
            habeas_sdk::graph::record_is_current(&graph, "future-review-version", &record).unwrap()
        );
        record.procedure = "old-concrete-version".into();
        assert!(
            !habeas_sdk::graph::record_is_current(&graph, crate::sdk::REVIEW_VERSION, &record)
                .unwrap()
        );
        record.procedure = "*".into();
        std::fs::write(
            root.path().join("md/rules.md"),
            ":::{req} ready\nChanged MUST work.\n:::\n",
        )
        .unwrap();
        let changed_definitions = crate::evidence::load_definitions(root.path()).unwrap();
        let changed = HabeasDomain {
            definitions: &changed_definitions,
            ..graph
        };
        assert!(!habeas_sdk::graph::record_is_current(&changed, "any", &record).unwrap());
    }
    // {test}`requirements.sdk.habeas.evidence_conversion.atomic`
    #[test]
    fn atomic_conversion_replaces_the_file_and_never_exposes_partial_bytes() {
        use std::sync::{
            Arc, Barrier,
            atomic::{AtomicBool, Ordering},
        };
        let root = tempfile::tempdir().unwrap();
        let subject = AuthoringSubject::Definition("old.ready".into());
        let node = HabeasNode::review_subject(&subject);
        let path = subject.evidence_path(root.path());
        let old = legacy(&subject);
        write_atomic(&path, &old).unwrap();
        let before = std::fs::read(&path).unwrap();
        let mut after =
            serde_json::to_vec_pretty(&encode(&decode(&old, &node).unwrap()).unwrap()).unwrap();
        after.push(b'\n');
        #[cfg(unix)]
        let inode = {
            use std::os::unix::fs::MetadataExt;
            std::fs::metadata(&path).unwrap().ino()
        };
        let barrier = Arc::new(Barrier::new(2));
        let done = Arc::new(AtomicBool::new(false));
        std::thread::scope(|scope| {
            let writer_barrier = barrier.clone();
            let writer_done = done.clone();
            let path = &path;
            scope.spawn(move || {
                writer_barrier.wait();
                convert_file(path, &node).unwrap();
                writer_done.store(true, Ordering::SeqCst);
            });
            barrier.wait();
            loop {
                let bytes = std::fs::read(&path).unwrap();
                assert!(bytes == before || bytes == after);
                if done.load(Ordering::SeqCst) {
                    break;
                }
            }
        });
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_ne!(std::fs::metadata(&path).unwrap().ino(), inode);
        }
        assert_eq!(std::fs::read(&path).unwrap(), after);
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("keep"), b"original").unwrap();
        assert!(write_atomic(&path, &old).is_err());
        assert_eq!(std::fs::read(path.join("keep")).unwrap(), b"original");
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
    }
}

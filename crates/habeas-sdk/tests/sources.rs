use habeas_sdk::language::Registry;
use habeas_sdk::{resource::ResourceId, sources::*};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

fn id(location: &str) -> ResourceId {
    ResourceId {
        scheme: "memory".into(),
        location: location.into(),
    }
}
fn registry() -> Registry {
    #[cfg(feature = "default-languages")]
    {
        habeas_languages::registry()
    }
    #[cfg(not(feature = "default-languages"))]
    {
        Registry::new([])
    }
}
struct Memory {
    events: Mutex<Vec<String>>,
    reads: AtomicUsize,
}

struct FailingProvider {
    list_fails: bool,
}
impl SourceProvider for FailingProvider {
    type Error = SourceError;
    async fn list(&self, _: &SourceScope) -> Result<Vec<ResourceId>, SourceError> {
        if self.list_fails {
            Err(SourceError::Invalid("listing failed".into()))
        } else {
            Ok(vec![id("src/one.rs")])
        }
    }
    async fn read(&self, _: &ResourceId) -> Result<SourceFile, SourceError> {
        Err(SourceError::Invalid("reading failed".into()))
    }
}

// {test}`requirements.sdk.sources.provider.errors`
#[tokio::test]
async fn failed_provider_operations_never_become_successful_empty_results() {
    let query = TextQuery {
        scope: SourceScope::default(),
        pattern: "hit".into(),
        context_lines: 0,
        limit: None,
    };
    for list_fails in [true, false] {
        let provider = FailingProvider { list_fails };
        let error = provider
            .search_text(&query)
            .await
            .err()
            .expect("must propagate provider failure");
        assert_eq!(
            error.to_string(),
            if list_fails {
                "listing failed"
            } else {
                "reading failed"
            }
        );
        assert_eq!(
            provider
                .parse(&id("src/one.rs"), &registry())
                .await
                .err()
                .unwrap()
                .to_string(),
            "reading failed"
        );
        #[cfg(feature = "default-languages")]
        {
            let query = StructuralQuery {
                scope: SourceScope::default(),
                language: "rust".into(),
                rule: serde_json::json!({"rule":{"pattern":"call()"}})
                    .as_object()
                    .unwrap()
                    .clone(),
                limit: None,
            };
            let error = provider
                .search_structure(&query, &registry())
                .await
                .err()
                .unwrap();
            assert_eq!(
                error.to_string(),
                if list_fails {
                    "listing failed"
                } else {
                    "reading failed"
                }
            );
        }
    }
}
impl SourceProvider for Memory {
    type Error = SourceError;
    async fn list(&self, _: &SourceScope) -> Result<Vec<ResourceId>, SourceError> {
        self.events.lock().unwrap().push("list".into());
        Ok(vec![id("src/one.txt"), id("other/two.txt")])
    }
    async fn read(&self, resource: &ResourceId) -> Result<SourceFile, SourceError> {
        self.events.lock().unwrap().push(resource.location.clone());
        let count = self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(SourceFile {
            bytes: format!("version {count}\nhit one\nhit two\nend\n")
                .into_bytes()
                .into(),
            language: None,
        })
    }
}

// {test}`requirements.sdk.sources.provider`
// {test}`requirements.sdk.sources.provider_defaults`
// {test}`requirements.sdk.sources.provider_defaults.composition`
// {test}`requirements.sdk.sources.provider_results.parse_snapshot`
#[tokio::test]
async fn defaults_use_provider_primitives_and_keep_one_parse_snapshot() {
    let memory = Memory {
        events: Mutex::default(),
        reads: AtomicUsize::new(0),
    };
    let parsed = memory.parse(&id("src/one.txt"), &registry()).await.unwrap();
    assert!(parsed.source.text().unwrap().starts_with("version 0"));
    assert_eq!(memory.reads.load(Ordering::SeqCst), 1);
    let newer = memory.read(&id("src/one.txt")).await.unwrap();
    assert!(newer.text().unwrap().starts_with("version 1"));
    assert!(parsed.source.text().unwrap().starts_with("version 0"));
    assert_eq!(
        parsed
            .outline
            .node_lines(parsed.source.text().unwrap(), 0)
            .concat()
            .contains("version 0"),
        true
    );
    let result = memory
        .search_text(&TextQuery {
            scope: SourceScope {
                prefix: "src".into(),
            },
            pattern: "hit".into(),
            context_lines: 1,
            limit: None,
        })
        .await
        .unwrap();
    assert!(result.complete);
    assert_eq!(result.matches.len(), 2);
    assert_eq!(result.matches[0].line, 2);
    assert_eq!(result.matches[0].excerpt, "version 2\nhit one\nhit two");
    assert_eq!(
        memory.events.lock().unwrap().as_slice(),
        ["src/one.txt", "src/one.txt", "list", "src/one.txt"]
    );
}

// {test}`requirements.sdk.sources.provider_results.search_completeness`
// {test}`requirements.sdk.sources.provider.errors`
#[tokio::test]
async fn completeness_is_false_only_when_a_match_is_omitted_and_errors_propagate() {
    let memory = Memory {
        events: Mutex::default(),
        reads: AtomicUsize::new(0),
    };
    for (limit, count, complete) in [
        (None, 2, true),
        (Some(2), 2, true),
        (Some(1), 1, false),
        (Some(0), 0, false),
    ] {
        let result = memory
            .search_text(&TextQuery {
                scope: SourceScope {
                    prefix: "src".into(),
                },
                pattern: "hit".into(),
                context_lines: 0,
                limit,
            })
            .await
            .unwrap();
        assert_eq!(result.matches.len(), count);
        assert_eq!(result.complete, complete);
    }
    let result = memory
        .search_text(&TextQuery {
            scope: SourceScope::default(),
            pattern: "absent".into(),
            context_lines: 0,
            limit: Some(0),
        })
        .await
        .unwrap();
    assert!(result.complete);
    assert!(result.matches.is_empty());
    assert!(
        memory
            .search_text(&TextQuery {
                scope: SourceScope::default(),
                pattern: "[".into(),
                context_lines: 0,
                limit: None
            })
            .await
            .is_err()
    );
    let bad = SourceFile {
        bytes: Arc::from([0xff]),
        language: None,
    };
    assert!(parse_snapshot(&id("file"), bad, &registry()).is_err());
    let unknown = SourceFile {
        bytes: Arc::from(b"text".as_slice()),
        language: Some("unknown-language".into()),
    };
    assert!(parse_snapshot(&id("file"), unknown, &registry()).is_err());
}

// {test}`requirements.sdk.sources.local_provider`
// {test}`requirements.sdk.sources.source_addresses`
#[tokio::test]
async fn local_provider_scopes_paths_and_rejects_resources_outside_its_root() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::create_dir(root.path().join("other")).unwrap();
    std::fs::write(root.path().join("src/one.txt"), "hit\n").unwrap();
    std::fs::write(root.path().join("other/two.txt"), "other\n").unwrap();
    let sources = LocalSources::new(root.path());
    let all = sources.list(&SourceScope::default()).await.unwrap();
    assert_eq!(
        all.iter()
            .map(|id| id.location.as_str())
            .collect::<Vec<_>>(),
        ["other/two.txt", "src/one.txt"]
    );
    let selected = sources
        .list(&SourceScope {
            prefix: "src".into(),
        })
        .await
        .unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(
        sources.read(&selected[0]).await.unwrap().text().unwrap(),
        "hit\n"
    );
    let matches = sources
        .search_text(&TextQuery {
            scope: SourceScope { prefix: ".".into() },
            pattern: "hit".into(),
            context_lines: 0,
            limit: None,
        })
        .await
        .unwrap();
    assert_eq!(matches.matches.len(), 1);
    assert!(sources.read(&id("src/one.txt")).await.is_err());
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("secret.txt");
    std::fs::write(&secret, "secret").unwrap();
    assert!(
        sources
            .read(&ResourceId {
                scheme: "repo".into(),
                location: secret.to_string_lossy().into()
            })
            .await
            .is_err()
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&secret, root.path().join("alias.txt")).unwrap();
        assert!(
            sources
                .read(&ResourceId {
                    scheme: "repo".into(),
                    location: "alias.txt".into()
                })
                .await
                .is_err()
        );
    }
}

#[cfg(feature = "default-languages")]
struct CachedParse {
    parses: AtomicUsize,
}
#[cfg(feature = "default-languages")]
impl SourceProvider for CachedParse {
    type Error = SourceError;
    async fn list(&self, _: &SourceScope) -> Result<Vec<ResourceId>, SourceError> {
        Ok(vec![id("src/one.rs"), id("other/two.rs")])
    }
    async fn read(&self, _: &ResourceId) -> Result<SourceFile, SourceError> {
        Err(SourceError::Invalid(
            "cached parser must not refetch".into(),
        ))
    }
    async fn parse(
        &self,
        resource: &ResourceId,
        languages: &Registry,
    ) -> Result<ParsedSource, SourceError> {
        self.parses.fetch_add(1, Ordering::SeqCst);
        parse_snapshot(
            resource,
            SourceFile {
                bytes: b"fn cached() { call(); }\n".as_slice().into(),
                language: Some("rust".into()),
            },
            languages,
        )
    }
}

// {test}`requirements.sdk.sources.provider_defaults.composition`
// {test}`requirements.sdk.sources.provider_results`
// {test}`requirements.sdk.sources.provider_results.search_completeness`
#[cfg(feature = "default-languages")]
#[tokio::test]
async fn structural_search_uses_overridden_parses_their_snapshot_and_requested_scope() {
    let provider = CachedParse {
        parses: AtomicUsize::new(0),
    };
    for (limit, count, complete) in [(None, 1, true), (Some(1), 1, true), (Some(0), 0, false)] {
        let matches = provider
            .search_structure(
                &StructuralQuery {
                    scope: SourceScope {
                        prefix: "src".into(),
                    },
                    language: "rust".into(),
                    rule: serde_json::json!({"rule":{"pattern":"call()"}})
                        .as_object()
                        .unwrap()
                        .clone(),
                    limit,
                },
                &registry(),
            )
            .await
            .unwrap();
        assert_eq!(matches.matches.len(), count);
        assert_eq!(matches.complete, complete);
        if let Some(found) = matches.matches.first() {
            assert_eq!(found.node.resource, id("src/one.rs"));
            assert_eq!(found.node.selector, "fn[cached]");
            assert_eq!(found.excerpt, "call()");
        }
    }
    assert_eq!(provider.parses.load(Ordering::SeqCst), 3);
}

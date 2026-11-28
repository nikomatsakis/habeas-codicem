#!/usr/bin/env bash
# Preview the issue #51 migration, or pass --apply to remove the markers.
set -euo pipefail
cd "$(dirname "$0")/.."

# Use Habeas's own private parsers so examples in strings and Markdown do not
# become edges. Build a temporary Cargo target with the project's dependencies.
created_dir=false
if [[ ! -d examples ]]; then
    mkdir examples
    created_dir=true
fi
source_file=examples/remove_tested_impl_migration.rs
# Reserve the target without overwriting an existing file or concurrent run.
(set -C; : > "$source_file")
cleanup() {
    rm -f "$source_file"
    if "$created_dir"; then rmdir examples 2>/dev/null || true; fi
}
trap cleanup EXIT
cat > "$source_file" <<'RUST'
include!("../src/lib.rs");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::collections::BTreeSet;
    use std::fs;

    let apply = match std::env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => false,
        [arg] if arg == "--apply" => true,
        _ => return Err("expected no arguments or --apply".into()),
    };
    let root = std::env::current_dir()?.canonicalize()?;
    let definitions = evidence::load_definitions(&root)?;
    let mut errors = Vec::new();
    let references = evidence::collect_references(&root, &mut errors);
    if !errors.is_empty() {
        return Err(errors.join("\n").into());
    }
    let tests: BTreeSet<_> = references
        .iter()
        .filter(|reference| reference.kind == ReferenceKind::Test)
        .map(|reference| &reference.name)
        .collect();
    let tested: BTreeSet<_> = definitions
        .iter()
        .filter(|(name, definition)| {
            definition.kind == DefinitionKind::Requirement
                && (tests.contains(name)
                    || definition
                        .anchors
                        .iter()
                        .any(|a| a.kind == ReferenceKind::Test))
        })
        .map(|(name, _)| name)
        .collect();
    let mut affected = BTreeSet::new();
    let mut files = BTreeMap::new();
    let mut count = 0;
    for reference in references.iter().filter(|reference| {
        reference.kind == ReferenceKind::Impl
            && tested.contains(&reference.name)
            && reference.file.extension().is_some_and(|ext| ext == "rs")
    }) {
        files
            .entry(&reference.file)
            .or_insert_with(Vec::new)
            .push(reference);
        affected.insert(&reference.name);
        count += 1;
    }
    let mut changes = Vec::new();
    for (file, markers) in files {
        let text = fs::read_to_string(file)?;
        let lines: Vec<_> = text.split_inclusive('\n').collect();
        let mut remove = BTreeSet::new();
        for reference in markers {
            let index = reference.line - 1;
            let source_line = lines[index].trim_start();
            let body = fenced::strip_comment_delimiters(source_line).trim();
            let fence = fenced::opening_fence(body);
            let matches = match fence {
                Some((ReferenceKind::Impl, name, _)) => {
                    name == reference.name && reference.commentary.is_some()
                }
                _ => {
                    body == format!("{{impl}}`{}`", reference.name)
                        && reference.commentary.is_none()
                }
            };
            if !source_line.starts_with("//") || !matches || !reference.closed {
                return Err(format!(
                    "{}:{}: expected a standalone implementation marker",
                    file.display(),
                    reference.line
                )
                .into());
            }
            remove.insert(index);
            if let Some((_, _, length)) = fence {
                let end = lines[index + 1..]
                    .iter()
                    .take_while(|line| line.trim_start().starts_with("//"))
                    .position(|line| {
                        colon_fence(fenced::strip_comment_delimiters(line)).is_some_and(
                            |(rest, colons)| colons >= length && rest.trim().is_empty(),
                        )
                    })
                    .ok_or("missing fence end")?;
                remove.insert(index + 1 + end);
            }
        }
        let after: String = lines
            .into_iter()
            .enumerate()
            .filter(|(index, _)| !remove.contains(index))
            .map(|(_, line)| line)
            .collect();
        changes.push((file, after));
    }
    println!(
        "{} Rust implementation edges across {} tested requirements in {} files",
        count,
        affected.len(),
        changes.len()
    );
    if apply {
        for (file, content) in changes {
            fs::write(file, content)?;
        }
    }
    Ok(())
}
RUST
cargo run --locked --example remove_tested_impl_migration -- "$@"

---
name: habeas-refactor
description: Use when a change moves code, tests, or requirements in a Habeas project to another file, chapter, or outline node, such as moving a function to another module, renaming or splitting a file, splitting or merging modules or chapters, or moving a requirement to another chapter, so that evidence whose content is unchanged is not invalidated. Covers making the move and running `habeas refactor` to update evidence, references, and anchors. To change what code or requirements say, use habeas-requirements; to reassess evidence the move leaves stale, use habeas-evidence; to derive an initial requirement set for an existing codebase, use habeas-bootstrap-requirements.
---

# Habeas refactor

Evidence records where it read code and requirements: files, outline selectors
such as `fn[load]`, and full definition names. A move changes those addresses
without changing what was reviewed. `habeas refactor` rewrites the addresses,
so the evidence stays current without a model reassessing it.

## Workflow

Run commands from the project root, the directory with `book.toml` and
`Cargo.toml`.

- [ ] 1. Make the move with your own edits: move the file, cut and paste the
      function, or move the `{req}` directive to its new chapter. Change
      nothing else in this step. Keep the moved code's text, including doc
      comments, and the moved requirement's text exactly as they were. Leave
      `{impl}`, `{test}`, `{cite}`, `{fixme}`, and `{term}` references to a
      moved definition, and `:impl:` and `:test:` anchors to a moved file,
      as they are; the command rewrites them.
- [ ] 2. `git add` the new files. Reference discovery reads only files in the
      Git index.
- [ ] 3. `habeas refactor --dry-run` with one `--map <old>=<new>` for every
      moved file or directory, node, and definition name:
      - a file or directory: `--map src/config.rs=src/settings.rs`, which also
        covers every file under a directory;
      - a node: `--map 'src/config.rs#fn[load]=src/config/load.rs#fn[load]'`,
        which also covers the nodes inside it (quote it for the shell);
      - a definition: `--map requirements.cli.check=requirements.core.check`,
        which also covers the names beneath it, such as
        `requirements.cli.check.case`.

      To find a node's selector, `habeas info <name>` lists the inputs of a
      requirement that reads it.
- [ ] 4. Review the report:
      - "Would rewrite" lists the sources whose references or anchors change;
      - "Would change" lists the records it updates, with each code span in
        their rationales that still mentions an old path, for you to check;
      - "Would leave ... unchanged" lists records a mapping affects that fail a
        check, with the reason. An unexpected one usually means the moved text
        changed, or a mapping is missing; fix that and run the dry run again.
- [ ] 5. Run the same command without `--dry-run`.
- [ ] 6. `habeas status`. Only the records the report left unchanged, and any
      that were stale before, should need attention.
- [ ] 7. Commit the move, the rewritten references, and the `evidence/` changes
      together, before any change to behavior or text.
- [ ] 8. Reassess what remains stale with the habeas-evidence workflow, then
      make any behavior or text changes as separate commits.

## Rules

- Never edit evidence files by hand to follow a move; `habeas refactor` updates
  them only where it can check that nothing reviewed changed.
- A rename is not a move. Renaming a function, type, or other item changes its
  code, and so its structural hash: its evidence needs reassessment with
  habeas-evidence. Renaming a requirement's ID is a definition mapping, and
  works when its text is otherwise unchanged.
- Editing moved code or text in the same step leaves every record that read it
  stale. Move first, run the command, commit, then edit.
- A requirements input that selects a glob, such as `requirements.cli.*`, stays
  stale when a definition moves out of it or into it: what it matches changed.
- An invalid mapping, such as one whose new side does not exist or one that
  overlaps another, fails with a usage error before anything changes.

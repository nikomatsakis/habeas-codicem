# Initial requirements coverage audit

This inventory was made by reading the first-party Rust source and runnable tests. Generated code, dependencies, and the test fixture source embedded in a string are excluded. No doctests or parameterized test declarations were found.

## Findings

The audit added 13 requirement definitions and matching evidence records for behavior that had no precise requirement: the validation report API, mdBook source configuration, identifier rules, invalid definitions, input containment, literal Markdown, safe badge markup, index path conflicts, invalid detail pages, CLI argument handling, setup project checks, CSS preservation, and local path dependencies. The audit also found a broken generated link for names containing `.md`; the existing `requirements.index.navigation` requirement was tightened and the link was fixed.

## Production source units

A source unit here is a production Rust function or method. Each row records the behavior that function implements; observable branches are reviewed separately below. **59 of 59 production function units are mapped.**

| Source unit | Requirement IDs |
| --- | --- |
| `src/lib.rs:19` `validate` | `requirements.integration.validate` |
| `src/lib.rs:61` `label` | `requirements.references.book_badges` |
| `src/lib.rs:69` `color` | `requirements.references.book_badges` |
| `src/lib.rs:86` `preprocess_book` | `requirements.cli.mdbook`, `requirements.index.source_links` |
| `src/lib.rs:144` `chapter_namespace` | `requirements.ci.identifiers` |
| `src/lib.rs:166` `replace_requirements` | `requirements.index.navigation`, `requirements.references.book_badges`, `requirements.references.safe_badges` |
| `src/lib.rs:214` `replace_references` | `requirements.references.book_badges`, `requirements.references.safe_badges` |
| `src/lib.rs:252` `shield_badge` | `requirements.references.book_badges`, `requirements.references.safe_badges` |
| `src/lib.rs:267` `shield_path_component` | `requirements.references.safe_badges` |
| `src/lib.rs:282` `chapter_href` | `requirements.index.navigation` |
| `src/lib.rs:316` `full_requirement_name` | `requirements.ci.identifiers` |
| `src/lib.rs:327` `requirement_blocks` | `requirements.ci.span`, `requirements.ci.identifiers`, `requirements.references.markdown_literals` |
| `src/lib.rs:392` `colon_fence` | `requirements.ci.span` |
| `src/lib.rs:402` `overlaps_protected` | `requirements.references.markdown_literals` |
| `src/lib.rs:411` `reference_roles` | `requirements.references.targets`, `requirements.ci.identifiers`, `requirements.references.markdown_literals` |
| `src/lib.rs:453` `protected_ranges` | `requirements.references.markdown_literals` |
| `src/lib.rs:468` `is_protected` | `requirements.references.markdown_literals` |
| `src/lib.rs:475` `is_escaped` | `requirements.references.markdown_literals` |
| `src/lib.rs:486` `valid_name` | `requirements.ci.identifiers` |
| `src/lib.rs:497` `escape_html` | `requirements.references.safe_badges` |
| `src/evidence.rs:28` `validate_repository` | `requirements.integration.report_api`, `requirements.ci.coverage`, `requirements.ci.orphans`, `requirements.references.targets`, `requirements.references.evidence_coverage` |
| `src/evidence.rs:85` `book_source_dir` | `requirements.ci.book_source` |
| `src/evidence.rs:116` `collect_requirements` | `requirements.ci.definition_errors`, `requirements.ci.identifiers`, `requirements.ci.span` |
| `src/evidence.rs:180` `files_with_extension` | `requirements.ci.coverage` |
| `src/evidence.rs:234` `validate_evidence_file` | `requirements.ci.schema`, `requirements.ci.requirement_text`, `requirements.ci.determination` |
| `src/evidence.rs:311` `validate_input` | `requirements.ci.schema`, `requirements.ci.input_containment` |
| `src/evidence.rs:367` `validate_input_lines` | `requirements.ci.inputs`, `requirements.ci.input_containment` |
| `src/evidence.rs:422` `compact_diff` | `requirements.ci.diagnostics` |
| `src/evidence.rs:449` `check_keys` | `requirements.ci.schema` |
| `src/evidence.rs:463` `string_field` | `requirements.ci.schema` |
| `src/evidence.rs:483` `boolean_field` | `requirements.ci.schema` |
| `src/evidence.rs:503` `positive_integer_field` | `requirements.ci.schema` |
| `src/evidence.rs:525` `array_field` | `requirements.ci.schema` |
| `src/evidence.rs:545` `display_path` | `requirements.ci.diagnostics` |
| `src/evidence.rs:567` `validate_references` | `requirements.references.targets`, `requirements.references.evidence_coverage` |
| `src/evidence.rs:613` `collect_references` | `requirements.references.targets` |
| `src/evidence.rs:700` `evidence_input_ranges` | `requirements.references.evidence_coverage` |
| `src/index.rs:28` `from_config` | `requirements.index.source_links` |
| `src/index.rs:70` `input_url` | `requirements.index.source_links` |
| `src/index.rs:94` `encode_url_segment` | `requirements.index.source_links` |
| `src/index.rs:123` `label` | `requirements.index.state` |
| `src/index.rs:133` `color` | `requirements.index.state` |
| `src/index.rs:145` `find` | `requirements.index.chapter`, `requirements.index.path_conflicts` |
| `src/index.rs:188` `detail_path` | `requirements.index.navigation` |
| `src/index.rs:193` `is_detail_path` | `requirements.index.navigation` |
| `src/index.rs:198` `populate` | `requirements.index.state`, `requirements.index.navigation`, `requirements.index.invalid_detail`, `requirements.index.source_links` |
| `src/index.rs:314` `evidence_edit_path` | `requirements.index.source_links` |
| `src/index.rs:328` `render_detail_page` | `requirements.index.detail_format`, `requirements.index.invalid_detail`, `requirements.index.navigation` |
| `src/index.rs:399` `render_input` | `requirements.index.detail_format`, `requirements.index.source_links` |
| `src/index.rs:440` `push_code_block` | `requirements.index.detail_format` |
| `src/index.rs:457` `code_language` | `requirements.index.detail_format` |
| `src/index.rs:475` `marker_ranges` | `requirements.index.chapter` |
| `src/index.rs:529` `invalid_index` | `requirements.index.chapter` |
| `src/main.rs:11` `main` | `requirements.cli.mdbook`, `requirements.cli.invocation`, `requirements.cli.setup`, `requirements.cli.agents` |
| `src/setup.rs:12` `project_root` | `requirements.cli.setup_project`, `requirements.cli.agents` |
| `src/setup.rs:30` `write_new_or_same` | `requirements.cli.setup`, `requirements.cli.agents` |
| `src/setup.rs:49` `run` | `requirements.cli.setup`, `requirements.cli.setup_project`, `requirements.cli.setup_css`, `requirements.cli.setup_local_path` |
| `src/agents.rs:22` `run` | `requirements.cli.agents` |
| `src/agents.rs:28` `install` | `requirements.cli.agents` |

## Runnable tests

Each test declaration has its own requirement reference. **30 of 30 first-party tests are mapped.**

| Test | Requirement IDs |
| --- | --- |
| `src/evidence.rs:755` `validates_full_names_requirement_text_and_input_lines` | `requirements.integration.report_api`, `requirements.ci.book_source`, `requirements.ci.coverage`, `requirements.ci.requirement_text`, `requirements.ci.inputs` |
| `src/evidence.rs:791` `collects_missing_orphan_format_false_and_changed_line_errors` | `requirements.integration.report_api`, `requirements.ci.errors`, `requirements.ci.diagnostics`, `requirements.ci.input_containment`, `requirements.ci.definition_errors` |
| `src/evidence.rs:868` `source_references_are_covered_by_evidence_inputs` | `requirements.references.evidence_coverage` |
| `src/evidence.rs:914` `reports_unknown_references_and_uncovered_lines` | `requirements.references.targets` |
| `src/evidence.rs:959` `colon_requirement_span_includes_markdown_and_excludes_following_text` | `requirements.ci.span` |
| `src/evidence.rs:992` `reports_a_missing_closing_colon_fence` | `requirements.ci.span`, `requirements.ci.definition_errors` |
| `src/evidence.rs:1024` `validates_four_or_more_colon_fences_and_longer_closers` | `requirements.ci.fence_lengths` |
| `src/index.rs:552` `marker_accepts_long_fences_and_ignores_examples` | `requirements.index.chapter` |
| `src/index.rs:575` `index_shows_evidence_states_and_links_generated_chapters` | `requirements.index.chapter`, `requirements.index.state`, `requirements.index.navigation`, `requirements.index.source_links` |
| `src/index.rs:721` `detail_page_renders_markdown_and_highlighted_input_sections` | `requirements.index.detail_format` |
| `src/index.rs:778` `source_links_use_configured_repository_and_branch` | `requirements.index.source_links` |
| `src/index.rs:840` `common_input_extensions_select_highlighting_languages` | `requirements.index.detail_format` |
| `src/index.rs:857` `repeated_index_markers_fail_with_a_clear_error` | `requirements.index.chapter` |
| `src/index.rs:873` `generated_detail_path_cannot_replace_a_chapter` | `requirements.index.path_conflicts` |
| `src/index.rs:903` `invalid_evidence_detail_remains_readable` | `requirements.index.invalid_detail` |
| `src/lib.rs:523` `chapter_path_prefixes_the_requirement_name` | `requirements.ci.identifiers` |
| `src/lib.rs:545` `links_preserve_requirement_names_containing_md` | `requirements.index.navigation` |
| `src/lib.rs:566` `code_examples_and_legacy_roles_remain_literal` | `requirements.references.markdown_literals` |
| `src/lib.rs:578` `invalid_names_remain_literal_and_html_is_escaped` | `requirements.ci.identifiers`, `requirements.references.safe_badges` |
| `src/lib.rs:593` `shields_badge_urls_escape_requirement_names` | `requirements.references.book_badges`, `requirements.references.safe_badges` |
| `src/lib.rs:606` `references_in_nested_chapters_link_to_the_requirement_definition` | `requirements.references.book_badges` |
| `src/lib.rs:645` `markdown_examples_and_escaped_references_stay_literal` | `requirements.references.markdown_literals` |
| `src/lib.rs:665` `colon_directives_render_markdown_bodies_and_legacy_roles_stay_literal` | `requirements.references.book_badges` |
| `src/lib.rs:679` `references_in_requirement_bodies_are_scanned_as_markdown` | `requirements.references.targets` |
| `src/lib.rs:690` `a_longer_colon_fence_contains_a_shorter_nested_directive` | `requirements.ci.span` |
| `tests/cli.rs:5` `renderer_support_and_invalid_commands` | `requirements.cli.invocation` |
| `tests/habeas.rs:3` `habeas` | `requirements.integration.self_test` |
| `tests/setup.rs:46` `setup_and_agents_are_repeatable` | `requirements.cli.setup`, `requirements.cli.agents`, `requirements.cli.setup_css`, `requirements.cli.setup_local_path` |
| `tests/setup.rs:98` `setup_install_skills_preserves_differing_skill` | `requirements.cli.setup`, `requirements.cli.agents` |
| `tests/setup.rs:123` `setup_rejects_invalid_book_configuration` | `requirements.cli.setup_project`, `requirements.cli.setup_css` |

## Observable branch groups

These are the distinct user-visible decisions found while reading the source. **24 of 24 branch groups are mapped.**

| Branch location | Observable behavior | Requirement IDs |
| --- | --- | --- |
| `src/evidence.rs:28` | inaccessible root returns an I/O error | `requirements.integration.report_api` |
| `src/evidence.rs:41` | orphan evidence file | `requirements.ci.orphans` |
| `src/evidence.rs:65` | missing evidence file | `requirements.ci.coverage` |
| `src/evidence.rs:84` | invalid book configuration or source path | `requirements.ci.book_source` |
| `src/evidence.rs:141` | unclosed, empty, or duplicate definition | `requirements.ci.definition_errors` |
| `src/evidence.rs:233` | unreadable, malformed, or wrong-shaped evidence | `requirements.ci.schema` |
| `src/evidence.rs:266` | changed requirement text or false determination | `requirements.ci.requirement_text`, `requirements.ci.determination` |
| `src/evidence.rs:310` | malformed input fields | `requirements.ci.schema`, `requirements.ci.input_containment` |
| `src/evidence.rs:366` | missing, escaping, or changed input file | `requirements.ci.inputs`, `requirements.ci.input_containment` |
| `src/evidence.rs:566` | unknown or uncovered reference | `requirements.references.targets`, `requirements.references.evidence_coverage` |
| `src/lib.rs:165` | definition badge with or without a detail link | `requirements.index.navigation`, `requirements.references.book_badges` |
| `src/lib.rs:213` | known reference badge or literal unknown role | `requirements.references.book_badges` |
| `src/lib.rs:326` | requirement fence inside literal Markdown | `requirements.ci.span`, `requirements.references.markdown_literals` |
| `src/lib.rs:410` | reference role inside literal Markdown | `requirements.references.markdown_literals` |
| `src/index.rs:145` | index directive cardinality and shape | `requirements.index.chapter` |
| `src/index.rs:172` | generated path collision | `requirements.index.path_conflicts` |
| `src/index.rs:198` | valid, missing, false, or invalid evidence state | `requirements.index.state` |
| `src/index.rs:269` | generated detail chapter insertion | `requirements.index.navigation` |
| `src/index.rs:327` | missing or invalid detail page placeholders | `requirements.index.invalid_detail` |
| `src/main.rs:21` | renderer support and command usage failures | `requirements.cli.invocation` |
| `src/setup.rs:53` | invalid project source or missing summary | `requirements.cli.setup_project` |
| `src/setup.rs:90` | existing, missing, or invalid CSS configuration | `requirements.cli.setup_css` |
| `src/setup.rs:109` | existing dependency or local path addition | `requirements.cli.setup_local_path` |
| `src/agents.rs:28` | existing identical or differing installed skill | `requirements.cli.agents` |

## Remaining gaps

None found in the reviewed first-party code and tests. Habeas validates written references and evidence; this inventory supplies the separate check for missing tags. Symposium installation behavior remains outside this audit as previously deferred.

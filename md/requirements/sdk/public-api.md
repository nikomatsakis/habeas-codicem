# Public API inventory

This appendix collects the exports introduced throughout the SDK chapters.

:::{req} exports
With its default features, `trust-but-verify` exposes exactly the crate and module
items listed below, following {cite}`requirements.sdk.public_api`. Each re-export
appears at every supported public path. Associated methods, fields, and variants
belong to their types' individual contracts.

The internal `habeas-integration` feature supports Habeas interoperability; its
hidden items are outside this supported interface.

- `trust_but_verify`
  - `agent`
    - `BedrockConfig`
    - `BedrockCredentials`
    - `OllamaConfig`
    - `OpenAiConfig`
    - `SecretString`
    - `AgentConfig`
    - `OptionValue`
  - `graph`
    - `CriterionPath`
    - `Fingerprint`
    - `Fingerprinted`
    - `InspectionContext`
    - `NodePath`
    - `PageId`
    - `PagedText`
    - `Record`
    - `Snapshot`
    - `ValidationContext`
    - `Validator`
    - `Dependency`
    - `PageError`
    - `ValidationError`
    - `Criterion`
    - `edge_fingerprint`
    - `procedure_matches`
    - `record_is_current`
  - `outline`
    - `languages`
      - `Css`
      - `Markdown`
      - `PlainText`
      - `Python`
      - `Rust`
      - `Toml`
      - `Tsx`
      - `TypeScript`
      - `Yaml`
    - `Outline`
    - `OutlineContent`
    - `OutlineError`
    - `OutlineId`
    - `OutlineItem`
    - `Segment`
    - `SelectorError`
    - `Label`
    - `HASH_DIGITS`
    - `OutlineLang`
    - `parse_selector`
  - `storage`
    - `FileEvidenceStorage`
    - `FileStorageError`
  - `Ask`
  - `ContextPhase`
  - `CriterionPath`
  - `FileEvidenceStorage`
  - `Fingerprint`
  - `Fingerprinted`
  - `InspectionContext`
  - `NodePath`
  - `NodePhase`
  - `OutlineContent`
  - `OutlineId`
  - `PageId`
  - `PagedText`
  - `Record`
  - `SearchResults`
  - `Snapshot`
  - `SourceFile`
  - `SourceScope`
  - `Sources`
  - `StructuralMatch`
  - `StructuralQuery`
  - `TextMatch`
  - `TextQuery`
  - `ValidationContext`
  - `Validator`
  - `AgentConfig`
  - `Dependency`
  - `FileStorageError`
  - `PageError`
  - `SourceError`
  - `ValidationError`
  - `Context`
  - `Criterion`
  - `Domain`
  - `NodeContents`
  - `OutlineLang`
  - `SourceNodes`
  - `LanguageId`
  - `StructuralMatches`
  - `TextMatches`
:::

# Public API inventory

This appendix collects the items introduced throughout the SDK chapters.

:::{req} exports
With its default features, `trust-but-verify` exposes exactly the crate and module
items listed below, following {cite}`requirements.sdk.public_api`. Each entry
states its Rust item kind and its name within the enclosing module. Associated
methods, fields, and variants belong to their types' individual contracts.

The internal `habeas-integration` feature supports Habeas interoperability; its
hidden items are outside this supported interface.

- `crate trust_but_verify`
  - `mod agent`
    - `enum AgentConfig`
    - `enum OptionValue`
    - `struct BedrockConfig`
    - `struct BedrockCredentials`
    - `struct OllamaConfig`
    - `struct OpenAiConfig`
    - `struct SecretString`
  - `mod domain`
    - `trait Domain`
    - `trait NodeContents`
    - `trait SourceNodes`
  - `mod graph`
    - `trait Criterion`
    - `struct CriterionPath`
    - `struct Fingerprint`
    - `struct Fingerprinted`
    - `struct InspectionContext`
    - `struct NodePath`
    - `struct Record`
    - `struct Snapshot`
    - `enum Dependency`
    - `enum ValidationError`
    - `fn edge_fingerprint`
    - `fn procedure_matches`
    - `fn record_is_current`
    - `mod validator`
      - `struct Validator`
  - `mod outline`
    - `struct Outline`
    - `struct OutlineContent`
    - `struct OutlineError`
    - `struct OutlineId`
    - `struct OutlineItem`
    - `trait OutlineLang`
    - `mod languages`
      - `struct Css`
      - `struct Markdown`
      - `struct PlainText`
      - `struct Python`
      - `struct Rust`
      - `struct Toml`
      - `struct Tsx`
      - `struct TypeScript`
      - `struct Yaml`
    - `mod selector`
      - `enum Label`
      - `struct Segment`
      - `struct SelectorError`
      - `const HASH_DIGITS`
      - `fn parse_selector`
  - `mod request`
    - `struct Ask`
    - `struct ContextPhase`
    - `struct NodePhase`
    - `struct ValidationContext`
    - `trait Context`
  - `mod sources`
    - `struct SearchResults`
    - `struct SourceFile`
    - `struct SourceScope`
    - `struct Sources`
    - `struct StructuralMatch`
    - `struct StructuralQuery`
    - `struct TextMatch`
    - `struct TextQuery`
    - `enum SourceError`
    - `type LanguageId`
    - `type StructuralMatches`
    - `type TextMatches`
  - `mod storage`
    - `struct FileEvidenceStorage`
    - `enum FileStorageError`
:::

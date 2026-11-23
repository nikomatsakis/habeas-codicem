//! Typed graph observations, deterministic checks, and explicit model authoring.
//!
//! Consumers own domain meaning, source-node mappings, and evidence persistence.
//! [`Sources`] inspects project files using `.gitignore` defaults. Source
//! outlines and their bundled languages live in [`outline`].

pub mod outline;
pub mod redaction;
pub mod search;
pub mod structural;

pub mod agent;
pub mod domain;
pub mod graph;
pub mod procedure;
mod sources;
pub use sources::{
    LanguageId, ParsedSource, SearchResults, SourceError, SourceFile, SourceScope, Sources,
    StructuralMatch, StructuralMatches, StructuralQuery, TextMatch, TextMatches, TextQuery,
};
pub mod storage;
pub use outline::language;
pub mod resource;

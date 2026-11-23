//! Typed graph observations, deterministic checks, and explicit model authoring.
//!
//! Consumers own domain meaning, resource resolution, and evidence persistence.
//! Source tooling uses extensible Tree-sitter language providers; the default
//! language collection is available through the `default-languages` feature.

pub mod access;
pub mod outline;
pub mod redaction;
pub mod search;
pub mod structural;

pub mod agent;
pub mod domain;
pub mod graph;
pub mod procedure;
pub mod sources;
pub mod storage;
pub use habeas_language as language;
pub mod resource;

//! Typed graph observations, deterministic checks, and explicit model authoring.
//!
//! Consumers own domain meaning, source-node mappings, and evidence persistence.
//! [`sources::Sources`] inspects project files using `.gitignore` defaults. Source
//! outlines and their bundled languages live in [`outline`].
//!

/// Configure the SDK's supplied Bedrock, OpenAI, and Ollama agents.
#[path = "agent/config.rs"]
pub mod agent;
pub mod domain;
pub mod graph;
// {impl}`requirements.sdk.outlines.independent`
/// Selectable source contents and bundled language rules.
#[path = "outline/contents.rs"]
pub mod outline;
/// Typed agent requests and their execution context.
#[path = "procedure.rs"]
pub mod request;
/// Project inspection using the SDK's built-in defaults.
#[path = "sources/public.rs"]
pub mod sources;
pub mod storage;

#[path = "sources.rs"]
mod source_engine;

#[path = "agent.rs"]
mod agent_runtime;

#[path = "outline/mod.rs"]
mod outline_engine;

#[path = "redaction/mod.rs"]
mod redaction;

mod resource;
mod search;

mod structural;

use request as procedure;
// {impl}`requirements.sdk.public-api.exports`

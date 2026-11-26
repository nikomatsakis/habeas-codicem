//! Typed graph observations, deterministic checks, and explicit model authoring.
//!
//! Consumers own domain meaning, source-node mappings, and evidence persistence.
//! [`sources::Sources`] inspects project files using `.gitignore` defaults. Source
//! outlines and their bundled languages live in [`outline`].
//!
//! {impl}`requirements.sdk.public_api`

/// Configure the SDK's supplied Bedrock, OpenAI, and Ollama agents.
#[path = "agent/config.rs"]
pub mod agent;
pub mod domain;
pub mod graph;
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

// :::{fixme} requirements.sdk.public-api.exports
// Habeas still enables the `habeas-integration` Cargo feature to reach TBV's
// structural search and agent internals. The public API
// inventory test covers the documented exports. It does not yet check that
// the manifest defines no features or that Habeas builds using those exports alone.
// :::
#[cfg(feature = "habeas-integration")]
#[doc(hidden)]
#[path = "agent.rs"]
pub mod agent_runtime;
#[cfg(not(feature = "habeas-integration"))]
#[path = "agent.rs"]
mod agent_runtime;

#[path = "outline/mod.rs"]
mod outline_engine;

#[path = "redaction/mod.rs"]
mod redaction;

mod resource;
mod search;

#[cfg(feature = "habeas-integration")]
#[doc(hidden)]
#[path = "structural.rs"]
pub mod structural;
#[cfg(not(feature = "habeas-integration"))]
#[path = "structural.rs"]
mod structural;

use request as procedure;

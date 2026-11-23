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

#[cfg(feature = "habeas-integration")]
#[doc(hidden)]
#[path = "agent.rs"]
pub mod agent_runtime;
#[cfg(not(feature = "habeas-integration"))]
#[path = "agent.rs"]
mod agent_runtime;

#[cfg(feature = "habeas-integration")]
#[doc(hidden)]
#[path = "outline/mod.rs"]
pub mod outline_engine;
#[cfg(not(feature = "habeas-integration"))]
#[path = "outline/mod.rs"]
mod outline_engine;

#[cfg(feature = "habeas-integration")]
#[doc(hidden)]
#[path = "redaction/mod.rs"]
pub mod redaction;
#[cfg(not(feature = "habeas-integration"))]
#[path = "redaction/mod.rs"]
mod redaction;

#[cfg(feature = "habeas-integration")]
#[doc(hidden)]
#[path = "resource.rs"]
pub mod resource;
#[cfg(not(feature = "habeas-integration"))]
#[path = "resource.rs"]
mod resource;

#[cfg(feature = "habeas-integration")]
#[doc(hidden)]
#[path = "search.rs"]
pub mod search;
#[cfg(not(feature = "habeas-integration"))]
#[path = "search.rs"]
mod search;

#[cfg(feature = "habeas-integration")]
#[doc(hidden)]
#[path = "structural.rs"]
pub mod structural;
#[cfg(not(feature = "habeas-integration"))]
#[path = "structural.rs"]
mod structural;

use request as procedure;

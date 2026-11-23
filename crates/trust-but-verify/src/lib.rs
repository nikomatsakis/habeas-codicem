//! Typed graph observations, deterministic checks, and explicit model authoring.
//!
//! Consumers own domain meaning, source-node mappings, and evidence persistence.
//! [`Sources`] inspects project files using `.gitignore` defaults. Source
//! outlines and their bundled languages live in [`outline`].
//!
//! {impl}`requirements.sdk.public_api`

#[path = "outline/mod.rs"]
mod outline_engine;
/// Selectable source contents and bundled language rules.
pub mod outline {
    #[cfg(feature = "habeas-integration")]
    #[doc(hidden)]
    pub use crate::outline_engine::integration;
    pub(crate) use crate::outline_engine::*;
    #[doc(inline)]
    pub use crate::outline_engine::{
        HASH_DIGITS, Label, Outline, OutlineContent, OutlineError, OutlineId, OutlineItem,
        OutlineLang, Segment, SelectorError, languages, parse_selector,
    };
}
mod redaction;
mod search;
mod structural;

#[path = "agent.rs"]
mod agent_runtime;
/// Configure the SDK's supplied Bedrock, OpenAI, and Ollama agents.
pub mod agent {
    pub(crate) use crate::agent_runtime::*;
    #[doc(inline)]
    pub use crate::agent_runtime::{
        AgentConfig, BedrockConfig, BedrockCredentials, OllamaConfig, OpenAiConfig, OptionValue,
        SecretString,
    };
}
mod domain;
pub mod graph;
mod procedure;
mod sources;
#[doc(inline)]
pub use agent::AgentConfig;
#[doc(inline)]
pub use domain::{Domain, NodeContents, SourceNodes};
#[doc(inline)]
pub use graph::{
    Criterion, CriterionPath, Dependency, Fingerprint, Fingerprinted, InspectionContext, NodePath,
    PageError, PageId, PagedText, Record, Snapshot, ValidationError, Validator,
};
#[doc(inline)]
pub use outline::{OutlineContent, OutlineId, OutlineLang};
#[doc(inline)]
pub use procedure::{Ask, Context, ContextPhase, NodePhase, ValidationContext};
#[doc(inline)]
pub use sources::{
    LanguageId, SearchResults, SourceError, SourceFile, SourceScope, Sources, StructuralMatch,
    StructuralMatches, StructuralQuery, TextMatch, TextMatches, TextQuery,
};
pub mod storage;
#[doc(inline)]
pub use storage::{FileEvidenceStorage, FileStorageError};
mod resource;

/// Internal interoperability for Habeas; not part of the supported SDK API.
#[doc(hidden)]
#[cfg(feature = "habeas-integration")]
pub mod __habeas {
    pub use crate::agent_runtime::providers::RigProvider;
    pub use crate::agent_runtime::{
        CompletionProvider, Conversation, RETRY_LIMIT, Submission, converse, failure_codes,
    };
    pub use crate::procedure::{Agent, AgentRequest};
    pub mod procedure {
        pub use crate::procedure::*;
    }
    pub mod resource {
        pub use crate::resource::*;
    }
    pub mod redaction {
        pub use crate::redaction::*;
    }
    pub mod structural {
        pub use crate::structural::*;
    }
    pub mod search {
        pub use crate::search::*;
    }
}

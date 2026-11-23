//! Habeas configuration adapts to the SDK's supplied model transports.

use crate::config::BackendConfig;
use rig_core::completion::{CompletionRequest, CompletionResponse};
use std::io;

#[cfg(test)]
pub(super) use trust_but_verify::__habeas::RETRY_LIMIT;
pub(super) use trust_but_verify::__habeas::{CompletionProvider, failure_codes};

/// Habeas retains configuration resolution; TBV owns the supported transport.
pub(super) struct RigProvider(trust_but_verify::__habeas::RigProvider);

impl RigProvider {
    // {impl}`requirements.authoring.provider`
    // {impl}`requirements.configuration.serialization`
    pub(super) async fn connect(config: &BackendConfig) -> io::Result<Self> {
        let config = serde_json::to_value(config).map_err(io::Error::other)?;
        let config = serde_json::from_value(config).map_err(io::Error::other)?;
        trust_but_verify::__habeas::RigProvider::connect(&config)
            .await
            .map(Self)
    }
}

impl CompletionProvider for RigProvider {
    async fn complete(&self, request: CompletionRequest) -> io::Result<CompletionResponse> {
        self.0.complete(request).await
    }

    fn output_limit(&self) -> Option<u64> {
        self.0.output_limit()
    }
}

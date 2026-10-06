use serde::Serialize;

use crate::plugin_rpc::PluginRpcError;
use planeai_plugin_contract::provider::ProviderErrorCode;

/// Why a provider session request failed, as its UI receives it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProviderError {
    pub code: ProviderErrorCode,
    pub message: String,
}

impl ProviderError {
    pub(super) fn new(code: ProviderErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub(super) fn not_running(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorCode::NotRunning, message)
    }

    pub(super) fn too_large(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorCode::PromptTooLarge, message)
    }

    pub(super) fn handed_off() -> Self {
        Self::new(
            ProviderErrorCode::HandedOff,
            "The session continues in a terminal. Send it there, or close the terminal to return to the chat.",
        )
    }
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<ProviderError> for String {
    fn from(error: ProviderError) -> Self {
        error.message
    }
}

impl From<PluginRpcError> for ProviderError {
    fn from(error: PluginRpcError) -> Self {
        let code = error
            .code()
            .map_or(ProviderErrorCode::PluginError, ProviderErrorCode::from_rpc);
        Self::new(code, error.to_string())
    }
}

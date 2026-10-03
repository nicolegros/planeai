//! The provider session protocol (ADR-0013), shared by the host and `planeai-cli plugin test`
//! so a plugin that passes the conformance check is one the host accepts.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const START: &str = "provider.session.start";
pub const RESUME: &str = "provider.session.resume";
pub const SEND: &str = "provider.session.send";
pub const INTERRUPT: &str = "provider.session.interrupt";
pub const STOP: &str = "provider.session.stop";
pub const HANDOFF: &str = "provider.session.handoff";
pub const HANDBACK: &str = "provider.session.handback";

pub const EVENT_NOTIFICATION: &str = "host.session.event";
pub const STATUS_NOTIFICATION: &str = "host.session.status";

/// Prompts must fit one JSON-RPC frame, as JSON-escaped text, with room for the envelope.
pub const MAX_PROMPT_BYTES: usize = 48 * 1024;

/// Most arguments a handoff command may have.
pub const MAX_HANDOFF_ARGS: usize = 64;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSessionStatus {
    Busy,
    Idle,
    NeedsAttention,
    Exited,
}

/// Why a provider session ended; sent to the plugin with `provider.session.stop`.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    Archive,
    Destroy,
    Exit,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionEventParams {
    pub session_id: String,
    pub seq: u64,
    pub payload: Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionStatusParams {
    pub session_id: String,
    pub status: ProviderSessionStatus,
}

#[derive(Debug, Deserialize)]
pub struct HandoffResponse {
    pub argv: Vec<String>,
}

pub fn check_prompt_size(text: &str) -> Result<(), String> {
    let encoded = serde_json::to_string(text).map_or(usize::MAX, |json| json.len());
    if encoded > MAX_PROMPT_BYTES {
        return Err(format!(
            "The message is too long ({} KB); the limit is {} KB.",
            encoded / 1024,
            MAX_PROMPT_BYTES / 1024
        ));
    }
    Ok(())
}

/// The command a handoff returns is run as is, so it must be a runnable argv.
pub fn validate_handoff_argv(argv: Vec<String>) -> Result<Vec<String>, String> {
    if argv.is_empty()
        || argv.len() > MAX_HANDOFF_ARGS
        || argv.iter().any(|arg| arg.is_empty() || arg.contains('\0'))
    {
        return Err(format!(
            "provider handoff must return a nonempty argv of at most {MAX_HANDOFF_ARGS} nonempty arguments without NUL"
        ));
    }
    Ok(argv)
}

/// Methods only the host sends: plugin UIs cannot call them, and scenarios cannot script them.
/// Provider session methods carry host-owned lifecycle and routing.
pub fn is_host_controlled_method(method: &str) -> bool {
    method.starts_with("provider.")
        || matches!(
            method,
            "plugin.handshake"
                | "plugin.shutdown"
                | "plugin.taskLifecycle"
                | "plugin.sessionLifecycle"
                | "$/cancelRequest"
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_must_fit_one_frame() {
        assert!(check_prompt_size("hello").is_ok());
        assert!(check_prompt_size(&"x".repeat(MAX_PROMPT_BYTES)).is_err());
        // Escaping counts: newlines double in JSON.
        assert!(check_prompt_size(&"\n".repeat(MAX_PROMPT_BYTES / 2)).is_err());
    }

    #[test]
    fn handoff_argv_must_be_runnable() {
        assert!(validate_handoff_argv(vec!["claude".into(), "--resume".into()]).is_ok());
        assert!(validate_handoff_argv(Vec::new()).is_err());
        assert!(validate_handoff_argv(vec![String::new()]).is_err());
        assert!(validate_handoff_argv(vec!["a\0b".into()]).is_err());
        assert!(validate_handoff_argv(vec!["x".into(); MAX_HANDOFF_ARGS + 1]).is_err());
    }

    #[test]
    fn notifications_validate_strictly() {
        let status: SessionStatusParams = serde_json::from_value(
            serde_json::json!({ "session_id": "s", "status": "needs_attention" }),
        )
        .unwrap();
        assert_eq!(status.status, ProviderSessionStatus::NeedsAttention);
        assert!(serde_json::from_value::<SessionStatusParams>(
            serde_json::json!({ "session_id": "s", "status": "sleeping" })
        )
        .is_err());
        assert!(serde_json::from_value::<SessionEventParams>(
            serde_json::json!({ "session_id": "s", "seq": 1, "payload": {}, "extra": true })
        )
        .is_err());
        assert!(serde_json::from_value::<SessionEventParams>(
            serde_json::json!({ "session_id": "s", "seq": 1 })
        )
        .is_err());
    }

    #[test]
    fn provider_and_lifecycle_methods_are_host_controlled() {
        assert!(is_host_controlled_method(SEND));
        assert!(is_host_controlled_method("plugin.shutdown"));
        assert!(!is_host_controlled_method("claude.snapshot"));
    }
}

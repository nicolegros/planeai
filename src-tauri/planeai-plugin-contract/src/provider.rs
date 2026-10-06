//! The provider session protocol (ADR-0014), shared by the host and `planeai-cli plugin test`
//! so a plugin that passes the conformance check is one the host accepts.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const START: &str = "provider.session.start";
pub const RESUME: &str = "provider.session.resume";
pub const SEND: &str = "provider.session.send";
pub const INTERRUPT: &str = "provider.session.interrupt";
pub const STOP: &str = "provider.session.stop";
pub const HANDOFF: &str = "provider.session.handoff";
pub const HANDBACK: &str = "provider.session.handback";
/// Sent once after the handshake with the sessions the host still runs on the plugin's providers.
pub const RECONCILE: &str = "provider.sessions.reconcile";

pub const EVENT_NOTIFICATION: &str = "host.providerSession.event";
pub const STATUS_NOTIFICATION: &str = "host.providerSession.status";

/// Features the host offers plugins, sent with `plugin.handshake` as `host_features`.
pub const HOST_FEATURES: &[&str] = &["provider_sessions.reconcile"];

/// JSON-RPC error codes provider methods answer with, which the host acts on.
pub mod error_code {
    /// The plugin does not know the session; the host resumes it once and retries.
    pub const SESSION_NOT_FOUND: i64 = -32010;
    /// The session is handed off to the agent's terminal UI.
    pub const HANDED_OFF: i64 = -32011;
    pub const PROMPT_TOO_LARGE: i64 = -32012;
    /// The agent cannot run now, as when its CLI is missing or signed out.
    pub const UNAVAILABLE: i64 = -32013;
}

/// Highest event `seq`: 2^53 - 1, the largest integer every JSON parser keeps exact.
pub const MAX_SEQ: u64 = (1 << 53) - 1;

/// Why a provider session request failed, as the session UI receives it.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderErrorCode {
    /// The session continues in a terminal.
    HandedOff,
    PromptTooLarge,
    /// The session or its plugin is not running.
    NotRunning,
    /// The provider does not offer this.
    Unsupported,
    /// The provider's agent cannot run now.
    Unavailable,
    PluginError,
}

impl ProviderErrorCode {
    /// What a provider method's JSON-RPC error code means for the session UI.
    pub fn from_rpc(code: i64) -> Self {
        match code {
            error_code::HANDED_OFF => Self::HandedOff,
            error_code::PROMPT_TOO_LARGE => Self::PromptTooLarge,
            error_code::UNAVAILABLE => Self::Unavailable,
            _ => Self::PluginError,
        }
    }
}

/// How long the host waits for a provider method, `None` for other methods. Starting,
/// resuming and handing off may launch the agent, so they get longer than the rest.
pub fn request_timeout(method: &str) -> Option<Duration> {
    match method {
        START | RESUME | HANDOFF | HANDBACK => Some(Duration::from_secs(30)),
        method if method.starts_with("provider.") => Some(Duration::from_secs(5)),
        _ => None,
    }
}

/// Whether a notification method is one provider sessions report through.
pub fn is_provider_notification(method: &str) -> bool {
    matches!(method, EVENT_NOTIFICATION | STATUS_NOTIFICATION)
}

/// Plugin provider keys are `<plugin id>:<provider id>`; plugin and provider ids never contain `:`.
pub fn parse_provider_key(key: &str) -> Option<(&str, &str)> {
    let (plugin_id, provider_id) = key.split_once(':')?;
    (!plugin_id.is_empty() && !provider_id.is_empty()).then_some((plugin_id, provider_id))
}

/// Keys with `:` belong to plugin providers, so a configured provider cannot take one.
pub fn is_reserved_provider_key(key: &str) -> bool {
    key.contains(':')
}

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

/// Unknown fields are ignored, left for newer hosts.
#[derive(Debug, Deserialize)]
pub struct SessionEventParams {
    pub session_id: String,
    pub seq: u64,
    pub payload: Value,
}

impl SessionEventParams {
    /// Checks `seq` is in range and above the session's last one, `0` before any event.
    /// Gaps are allowed.
    pub fn check_seq(&self, last: u64) -> Result<(), String> {
        if self.seq == 0 || self.seq > MAX_SEQ {
            return Err(format!("event seq {} is outside 1..=2^53-1", self.seq));
        }
        if self.seq <= last {
            return Err(format!(
                "event seq {} does not follow the session's last seq {last}",
                self.seq
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
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
    fn provider_keys_split_plugin_and_provider() {
        assert_eq!(
            parse_provider_key("claude-chat:claude"),
            Some(("claude-chat", "claude"))
        );
        for key in ["claude", ":claude", "plugin:"] {
            assert_eq!(parse_provider_key(key), None, "{key}");
        }
        // Every key routed to a plugin is one configured providers cannot take.
        for key in ["claude-chat:claude", ":claude", "plugin:"] {
            assert!(is_reserved_provider_key(key), "{key}");
        }
        assert!(!is_reserved_provider_key("claude"));
    }

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
    fn event_seq_increases_within_range() {
        let event = |seq: u64| SessionEventParams {
            session_id: "s".into(),
            seq,
            payload: Value::Null,
        };
        assert!(event(1).check_seq(0).is_ok());
        assert!(event(5).check_seq(2).is_ok());
        assert!(event(0).check_seq(0).is_err());
        assert!(event(2).check_seq(2).is_err());
        assert!(event(MAX_SEQ).check_seq(0).is_ok());
        assert!(event(MAX_SEQ + 1).check_seq(0).is_err());
    }

    #[test]
    fn launching_methods_wait_longer() {
        assert_eq!(request_timeout(START), Some(Duration::from_secs(30)));
        assert_eq!(request_timeout(HANDBACK), Some(Duration::from_secs(30)));
        assert_eq!(request_timeout(SEND), Some(Duration::from_secs(5)));
        assert_eq!(request_timeout(RECONCILE), Some(Duration::from_secs(5)));
        assert_eq!(request_timeout("jira.status"), None);
    }

    #[test]
    fn provider_error_codes_tell_the_session_ui_why() {
        assert_eq!(
            ProviderErrorCode::from_rpc(error_code::HANDED_OFF),
            ProviderErrorCode::HandedOff
        );
        assert_eq!(
            ProviderErrorCode::from_rpc(error_code::UNAVAILABLE),
            ProviderErrorCode::Unavailable
        );
        assert_eq!(
            ProviderErrorCode::from_rpc(-32000),
            ProviderErrorCode::PluginError
        );
        // The session UI matches on these names.
        let names = [
            ProviderErrorCode::HandedOff,
            ProviderErrorCode::PromptTooLarge,
            ProviderErrorCode::NotRunning,
            ProviderErrorCode::Unsupported,
            ProviderErrorCode::Unavailable,
            ProviderErrorCode::PluginError,
        ]
        .map(|code| serde_json::to_value(code).unwrap());
        assert_eq!(
            names,
            [
                "handed_off",
                "prompt_too_large",
                "not_running",
                "unsupported",
                "unavailable",
                "plugin_error"
            ]
        );
    }

    #[test]
    fn notifications_validate_known_fields() {
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
        .is_ok());
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

//! Session restart-on-focus logic.
//!
//! When a user focuses an exited session, we restart it with the provider's
//! interactive resume command (or fall back to the original command).

use crate::config::Provider;

/// Minimum seconds a session must live before auto-restart is allowed.
const CIRCUIT_BREAKER_SECS: u64 = 5;

/// Determine the restart command for a session, if any.
///
/// Returns `Some(command)` if the session should be restarted, `None` otherwise.
/// - Only restarts sessions with status "exited"
/// - Uses `interactive_resume_command` if set, otherwise falls back to `command`
/// - Circuit breaker: returns `None` if session lived less than `CIRCUIT_BREAKER_SECS`
pub fn restart_command(
    session_status: &str,
    provider: &Provider,
    session_age_secs: Option<u64>,
) -> Option<String> {
    if session_status != "exited" {
        return None;
    }
    // Circuit breaker: if session died too quickly, don't restart
    if let Some(age) = session_age_secs {
        if age < CIRCUIT_BREAKER_SECS {
            return None;
        }
    }
    Some(
        provider
            .interactive_resume_command
            .clone()
            .unwrap_or_else(|| provider.command.clone()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider_with_resume() -> Provider {
        Provider {
            command: "kiro-cli chat --trust-all-tools".to_string(),
            yolo_flag: None,
            resume_flag: None,
            interactive_resume_command: Some(
                "kiro-cli chat --trust-all-tools --resume-picker".to_string(),
            ),
            list_sessions_command: None,
            session_id_pattern: None,
            prompt_command: None,
            autonomous_prompt_template: None,
        }
    }

    fn provider_without_resume() -> Provider {
        Provider {
            command: "aider".to_string(),
            yolo_flag: None,
            resume_flag: None,
            interactive_resume_command: None,
            list_sessions_command: None,
            session_id_pattern: None,
            prompt_command: None,
            autonomous_prompt_template: None,
        }
    }

    // Slice 2: Restart with interactive_resume_command
    #[test]
    fn exited_session_restarts_with_interactive_resume_command() {
        let cmd = restart_command("exited", &provider_with_resume(), Some(60));
        assert_eq!(
            cmd,
            Some("kiro-cli chat --trust-all-tools --resume-picker".to_string())
        );
    }

    // Slice 3: Fallback to original command
    #[test]
    fn exited_session_falls_back_to_command_when_no_resume() {
        let cmd = restart_command("exited", &provider_without_resume(), Some(60));
        assert_eq!(cmd, Some("aider".to_string()));
    }

    // Slice 4: No restart for terminal states
    #[test]
    fn active_session_does_not_restart() {
        let cmd = restart_command("active", &provider_with_resume(), Some(60));
        assert_eq!(cmd, None);
    }

    #[test]
    fn archived_session_does_not_restart() {
        let cmd = restart_command("archived", &provider_with_resume(), Some(60));
        assert_eq!(cmd, None);
    }

    #[test]
    fn done_session_does_not_restart() {
        let cmd = restart_command("done", &provider_with_resume(), Some(60));
        assert_eq!(cmd, None);
    }

    #[test]
    fn deleted_session_does_not_restart() {
        let cmd = restart_command("deleted", &provider_with_resume(), Some(60));
        assert_eq!(cmd, None);
    }

    // Slice 5: Circuit breaker
    #[test]
    fn circuit_breaker_blocks_restart_if_session_too_young() {
        let cmd = restart_command("exited", &provider_with_resume(), Some(3));
        assert_eq!(cmd, None);
    }

    #[test]
    fn circuit_breaker_allows_restart_at_threshold() {
        let cmd = restart_command("exited", &provider_with_resume(), Some(5));
        assert!(cmd.is_some());
    }

    #[test]
    fn circuit_breaker_allows_restart_when_age_unknown() {
        let cmd = restart_command("exited", &provider_with_resume(), None);
        assert!(cmd.is_some());
    }
}


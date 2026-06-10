use serde::Deserialize;
use std::path::Path;

use crate::template;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PrStatus {
    pub url: String,
    pub state: String,
    #[serde(default, rename = "isDraft")]
    pub is_draft: bool,
}

/// Run the configured pr_status command and parse the result.
/// Returns Ok(None) if command exits non-zero (no PR exists).
/// Returns Err if the command can't be executed or output is malformed.
pub fn check_pr_status(
    command_template: &str,
    branch: &str,
    cwd: &Path,
) -> Result<Option<PrStatus>, String> {
    let mut vars = std::collections::HashMap::new();
    vars.insert("branch", branch);
    let cmd_str = template::render(command_template, &vars);
    let output = match planeai_core::command::run_command(&cmd_str, cwd) {
        Ok(stdout) => stdout,
        Err(planeai_core::command::CommandError::NonZeroExit { .. }) => return Ok(None),
        Err(planeai_core::command::CommandError::SpawnFailed { command, source }) => {
            return Err(format!("pr_status: failed to run '{command}': {source}"));
        }
    };
    let mut status: PrStatus = serde_json::from_str(output.trim())
        .map_err(|e| format!("pr_status: invalid JSON output: {e}"))?;
    status.state = status.state.to_lowercase();
    if status.is_draft {
        status.state = "draft".to_string();
    }
    Ok(Some(status))
}

#[derive(Debug, Clone, PartialEq)]
pub enum PrTransition {
    Opened,
    Merged,
}

/// Detect a meaningful state transition between persisted state and newly fetched state.
pub fn detect_transition(old_state: Option<&str>, new: &PrStatus) -> Option<PrTransition> {
    match (old_state, new.state.as_str()) {
        (None | Some("") | Some("draft"), "open") => Some(PrTransition::Opened),
        (Some("open"), "merged") => Some(PrTransition::Merged),
        _ => None,
    }
}

/// Fire the appropriate task manager hook for a PR transition.
/// Returns the status the task was moved to, or None if no hook configured.
pub fn fire_pr_hook(
    tm: &crate::config::TaskManager,
    transition: &PrTransition,
    task_key: &str,
    cwd: &Path,
) -> Option<String> {
    let hook = match transition {
        PrTransition::Opened => tm.on_pr_open.as_ref(),
        PrTransition::Merged => tm.on_pr_merge.as_ref(),
    };
    let h = hook?;
    let _ = crate::task_manager::move_task(tm, task_key, &h.move_to, cwd);
    Some(h.move_to.clone())
}

/// Check if a session is eligible for PR status polling.
pub fn is_poll_eligible(session_status: &str, pr_state: Option<&str>) -> bool {
    if session_status != "active" {
        return false;
    }
    !matches!(pr_state, Some("merged") | Some("closed"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn check_pr_status_parses_valid_json() {
        let dir = tempdir().unwrap();
        let script = dir.path().join("pr.sh");
        fs::write(&script, "#!/bin/sh\necho '{\"url\":\"https://github.com/org/repo/pull/42\",\"state\":\"open\"}'").unwrap();
        #[cfg(unix)]
        fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

        let template = format!("{} {{branch}}", script.display());
        let result = check_pr_status(&template, "feat/foo", dir.path());

        assert_eq!(
            result.unwrap(),
            Some(PrStatus {
                url: "https://github.com/org/repo/pull/42".to_string(),
                state: "open".to_string(),
                is_draft: false,
            })
        );
    }

    #[test]
    fn check_pr_status_returns_none_on_non_zero_exit() {
        let dir = tempdir().unwrap();
        let script = dir.path().join("pr.sh");
        fs::write(&script, "#!/bin/sh\nexit 1").unwrap();
        #[cfg(unix)]
        fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

        let template = format!("{}", script.display());
        let result = check_pr_status(&template, "feat/foo", dir.path());

        assert_eq!(result.unwrap(), None);
    }

    #[test]
    fn check_pr_status_returns_error_on_invalid_json() {
        let dir = tempdir().unwrap();
        let script = dir.path().join("pr.sh");
        fs::write(&script, "#!/bin/sh\necho 'not json'").unwrap();
        #[cfg(unix)]
        fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

        let template = format!("{}", script.display());
        let result = check_pr_status(&template, "main", dir.path());

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid JSON"));
    }

    #[test]
    fn check_pr_status_returns_none_on_nonexistent_command() {
        let dir = tempdir().unwrap();
        let result = check_pr_status("nonexistent_binary_xyz {branch}", "main", dir.path());
        assert_eq!(result.unwrap(), None);
    }

    #[test]
    fn detect_transition_none_to_open_is_opened() {
        let status = PrStatus {
            url: "http://x".into(),
            state: "open".into(),
            is_draft: false,
        };
        assert_eq!(detect_transition(None, &status), Some(PrTransition::Opened));
    }

    #[test]
    fn detect_transition_open_to_merged_is_merged() {
        let status = PrStatus {
            url: "http://x".into(),
            state: "merged".into(),
            is_draft: false,
        };
        assert_eq!(
            detect_transition(Some("open"), &status),
            Some(PrTransition::Merged)
        );
    }

    #[test]
    fn detect_transition_same_state_is_none() {
        let status = PrStatus {
            url: "http://x".into(),
            state: "open".into(),
            is_draft: false,
        };
        assert_eq!(detect_transition(Some("open"), &status), None);
    }

    #[test]
    fn detect_transition_none_to_merged_is_none() {
        let status = PrStatus {
            url: "http://x".into(),
            state: "merged".into(),
            is_draft: false,
        };
        assert_eq!(detect_transition(None, &status), None);
    }

    #[test]
    fn fire_pr_hook_on_opened_calls_move_task() {
        let dir = tempdir().unwrap();
        let script = dir.path().join("move.sh");
        fs::write(&script, "#!/bin/sh\nexit 0").unwrap();
        #[cfg(unix)]
        fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

        let tm = crate::config::TaskManager {
            get_task: String::new(),
            move_task: format!("{} {{key}} {{status}}", script.display()),
            list_tasks: String::new(),
            templates: None,
            on_start: None,
            on_notify: None,
            on_restart: None,
            on_complete: None,
            on_pr_open: Some(crate::config::LifecycleHook {
                move_to: "in_review".into(),
            }),
            on_pr_merge: None,
            list_all_tasks: None,
            create_task: None,
            edit_task: None,
            auto_dispatch: None,
        };

        let result = fire_pr_hook(&tm, &PrTransition::Opened, "TASK-1", dir.path());
        assert_eq!(result, Some("in_review".to_string()));
    }

    #[test]
    fn fire_pr_hook_on_merged_calls_move_task() {
        let dir = tempdir().unwrap();
        let script = dir.path().join("move.sh");
        fs::write(&script, "#!/bin/sh\nexit 0").unwrap();
        #[cfg(unix)]
        fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

        let tm = crate::config::TaskManager {
            get_task: String::new(),
            move_task: format!("{} {{key}} {{status}}", script.display()),
            list_tasks: String::new(),
            templates: None,
            on_start: None,
            on_notify: None,
            on_restart: None,
            on_complete: None,
            on_pr_open: None,
            on_pr_merge: Some(crate::config::LifecycleHook {
                move_to: "done".into(),
            }),
            list_all_tasks: None,
            create_task: None,
            edit_task: None,
            auto_dispatch: None,
        };

        let result = fire_pr_hook(&tm, &PrTransition::Merged, "TASK-1", dir.path());
        assert_eq!(result, Some("done".to_string()));
    }

    #[test]
    fn fire_pr_hook_returns_none_when_no_hook_configured() {
        let dir = tempdir().unwrap();
        let tm = crate::config::TaskManager {
            get_task: String::new(),
            move_task: String::new(),
            list_tasks: String::new(),
            templates: None,
            on_start: None,
            on_notify: None,
            on_restart: None,
            on_complete: None,
            on_pr_open: None,
            on_pr_merge: None,
            list_all_tasks: None,
            create_task: None,
            edit_task: None,
            auto_dispatch: None,
        };

        let result = fire_pr_hook(&tm, &PrTransition::Opened, "TASK-1", dir.path());
        assert_eq!(result, None);
    }

    #[test]
    fn poll_eligible_active_session_no_pr_state() {
        assert!(is_poll_eligible("active", None));
    }

    #[test]
    fn poll_eligible_active_session_open_pr() {
        assert!(is_poll_eligible("active", Some("open")));
    }

    #[test]
    fn poll_ineligible_merged_pr() {
        assert!(!is_poll_eligible("active", Some("merged")));
    }

    #[test]
    fn poll_ineligible_closed_pr() {
        assert!(!is_poll_eligible("active", Some("closed")));
    }

    #[test]
    fn poll_ineligible_exited_session() {
        assert!(!is_poll_eligible("exited", None));
    }
}

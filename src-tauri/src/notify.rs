use std::io::{BufRead, BufReader};
use std::path::Path;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

use planeai::ipc::{Channel, IpcListener};

// Re-export shared types from planeai-core
pub use planeai_core::notify::{
    is_claude_hook_installed_at, is_copilot_hook_installed_at, is_kiro_hook_installed_at,
    parse_notify_message, AgentState, NotifyEvent, NotifyMessage, NotifyState, SharedNotifyState,
};
// Used in tests
#[cfg(test)]
pub(crate) use planeai_core::notify::{install_claude_hook_at, install_copilot_hook_at};

const SILENCE_CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// Adapter that implements OutputObserver by forwarding to NotifyState + emitting Tauri events.
pub struct NotifyObserver {
    state: SharedNotifyState,
    app: AppHandle,
}

impl NotifyObserver {
    pub fn new(state: SharedNotifyState, app: AppHandle) -> Self {
        Self { state, app }
    }
}

impl crate::output_observer::OutputObserver for NotifyObserver {
    fn on_output(&self, session_id: &str, _byte_count: usize) {
        let mut s = self.state.lock().unwrap();
        let was_idle = s.get_state(session_id) != Some(AgentState::Busy);
        s.notify_output(session_id);
        if was_idle {
            drop(s);
            let _ = self.app.emit(
                "agent-state-change",
                serde_json::json!({
                    "session_id": session_id,
                    "state": "Busy"
                }),
            );
        }
    }
}

/// Start the IPC listener thread.
pub fn start_socket_listener(app_dir: &Path, state: SharedNotifyState, app: AppHandle) {
    let listener =
        IpcListener::bind(Channel::Notify, app_dir).expect("failed to bind notify IPC listener");

    thread::spawn(move || loop {
        let stream = match listener.accept() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[notify] accept error: {e}");
                continue;
            }
        };
        let reader = BufReader::new(stream);
        for line in reader.lines().map_while(Result::ok) {
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }
            let msg = parse_notify_message(&line);
            if msg.session_id.is_empty() {
                continue;
            }
            dispatch_message(&msg, &state, &app);
        }
    });
}

fn dispatch_message(msg: &NotifyMessage, state: &SharedNotifyState, app: &AppHandle) {
    match msg.event {
        NotifyEvent::SessionCreated => {
            eprintln!("[notify] session_created: {}", msg.session_id);
            let _ = app.emit("session-created", msg.session_id.clone());
        }
        NotifyEvent::SessionChanged => {
            eprintln!("[notify] session_changed: {}", msg.session_id);
            let _ = app.emit("sessions-changed", ());
        }
        NotifyEvent::Busy => {
            let mut s = state.lock().unwrap();
            let name = s
                .get_meta(&msg.session_id)
                .map(|m| m.name.as_str())
                .unwrap_or("?");
            eprintln!("[notify] \"{name}\" is now busy (hook)");
            s.notify_output(&msg.session_id);
            drop(s);
            emit_state_change(app, &msg.session_id, AgentState::Busy);
        }
        NotifyEvent::Notification => {
            let fired = {
                let mut s = state.lock().unwrap();
                let name = s
                    .get_meta(&msg.session_id)
                    .map(|m| m.name.as_str())
                    .unwrap_or("?");
                eprintln!("[notify] \"{name}\" received immediate signal (notification)");
                s.notify_stop_immediate(&msg.session_id)
            };
            if fired {
                emit_state_change(app, &msg.session_id, AgentState::Idle);
                fire_notification(app, &msg.session_id, state);
            }
        }
        NotifyEvent::Stop => {
            let mut s = state.lock().unwrap();
            let name = s
                .get_meta(&msg.session_id)
                .map(|m| m.name.clone())
                .unwrap_or_else(|| "?".into());
            let hook_enabled = s.get_meta(&msg.session_id).is_some_and(|m| m.hook_enabled);
            if hook_enabled {
                eprintln!("[notify] \"{name}\" received stop (debouncing 2s)");
                s.notify_stop_debounced(&msg.session_id);
            } else {
                eprintln!("[notify] \"{name}\" received stop (immediate, no hook)");
                let fired = s.notify_stop(&msg.session_id);
                drop(s);
                if fired {
                    emit_state_change(app, &msg.session_id, AgentState::Idle);
                    fire_notification(app, &msg.session_id, state);
                }
            }
        }
    }
}

/// Start the silence checker thread.
pub fn start_silence_checker(state: SharedNotifyState, app: AppHandle) {
    thread::spawn(move || loop {
        thread::sleep(SILENCE_CHECK_INTERVAL);
        let mut to_notify: Vec<String> = Vec::new();
        {
            let mut s = state.lock().unwrap();
            let busy = s.busy_sessions();
            for id in busy {
                if s.check_silence(&id) {
                    to_notify.push(id);
                }
            }
            let debounced = s.debounced_sessions();
            for id in debounced {
                if s.check_debounce(&id) {
                    to_notify.push(id);
                }
            }
        }
        for session_id in to_notify {
            let name = {
                let s = state.lock().unwrap();
                s.get_meta(&session_id)
                    .map(|m| m.name.clone())
                    .unwrap_or_else(|| "?".into())
            };
            eprintln!("[notify] \"{name}\" notifying (idle timeout)");
            emit_state_change(&app, &session_id, AgentState::Idle);
            fire_notification(&app, &session_id, &state);
        }
    });
}

fn emit_state_change(app: &AppHandle, session_id: &str, state: AgentState) {
    #[derive(serde::Serialize, Clone)]
    struct StateChangePayload {
        session_id: String,
        state: AgentState,
    }
    let _ = app.emit(
        "agent-state-change",
        StateChangePayload {
            session_id: session_id.to_string(),
            state,
        },
    );
}

fn fire_notification(app: &AppHandle, session_id: &str, state: &SharedNotifyState) {
    let (title, body) = {
        let s = state.lock().unwrap();
        match s.get_meta(session_id) {
            Some(meta) => (meta.project_name.clone(), format!("{} is ready", meta.name)),
            None => ("planeai".to_string(), "Agent is ready".to_string()),
        }
    };

    use tauri_plugin_notification::NotificationExt;
    let _ = app
        .notification()
        .builder()
        .title(&title)
        .body(&body)
        .show();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // ─── NotifyState tests (via planeai-core) ────────────────────────────────

    #[test]
    fn socket_notification_transitions_session_to_idle() {
        let mut state = NotifyState::new();
        state.notify_stop("s1");
        assert_eq!(state.get_state("s1"), Some(AgentState::Idle));
    }

    #[test]
    fn pty_output_transitions_session_to_busy() {
        let mut state = NotifyState::new();
        state.notify_stop("s1");
        state.notify_output("s1");
        assert_eq!(state.get_state("s1"), Some(AgentState::Busy));
    }

    #[test]
    fn silence_timeout_transitions_to_idle() {
        let mut state = NotifyState::new();
        state.register_session("s1", "test", "project", false);
        state.notify_output("s1");
        state.advance_time("s1", Duration::from_secs(5));
        assert!(state.check_silence("s1"));
        assert_eq!(state.get_state("s1"), Some(AgentState::Idle));
    }

    #[test]
    fn notification_fires_once_per_idle_transition() {
        let mut state = NotifyState::new();
        assert!(state.notify_stop("s1"));
        assert!(!state.notify_stop("s1"));
        state.notify_output("s1");
        assert!(state.notify_stop("s1"));
    }

    #[test]
    fn silence_check_skipped_for_hook_enabled_sessions() {
        let mut state = NotifyState::new();
        state.register_session("s1", "test", "project", true);
        state.notify_output("s1");
        state.advance_time("s1", Duration::from_secs(10));
        assert!(!state.check_silence("s1"));
        assert_eq!(state.get_state("s1"), Some(AgentState::Busy));
    }

    #[test]
    fn debounced_stop_does_not_fire_immediately_but_fires_after_threshold() {
        let mut state = NotifyState::new();
        state.register_session("s1", "test", "project", true);
        state.notify_output("s1");
        assert!(!state.notify_stop_debounced("s1"));
        state.advance_time("s1", Duration::from_secs(1));
        assert!(!state.check_debounce("s1"));
        state.advance_time("s1", Duration::from_secs(1));
        assert!(state.check_debounce("s1"));
    }

    #[test]
    fn debounced_stop_cancelled_by_output() {
        let mut state = NotifyState::new();
        state.register_session("s1", "test", "project", true);
        state.notify_output("s1");
        state.notify_stop_debounced("s1");
        state.advance_time("s1", Duration::from_secs(1));
        state.notify_output("s1");
        state.advance_time("s1", Duration::from_secs(2));
        assert!(!state.check_debounce("s1"));
    }

    #[test]
    fn immediate_notification_fires_instantly() {
        let mut state = NotifyState::new();
        state.register_session("s1", "test", "project", true);
        state.notify_output("s1");
        assert!(state.notify_stop_immediate("s1"));
        assert_eq!(state.get_state("s1"), Some(AgentState::Idle));
    }

    // ─── parse_notify_message tests ──────────────────────────────────────────

    #[test]
    fn parse_json_stop_event() {
        let msg = parse_notify_message(r#"{"session_id":"abc123","event":"stop"}"#);
        assert_eq!(msg.session_id, "abc123");
        assert_eq!(msg.event, NotifyEvent::Stop);
    }

    #[test]
    fn parse_bare_session_id_as_debounced_stop() {
        let msg = parse_notify_message("abc123");
        assert_eq!(msg.session_id, "abc123");
        assert_eq!(msg.event, NotifyEvent::Stop);
    }

    #[test]
    fn parse_json_notification_event() {
        let msg = parse_notify_message(r#"{"session_id":"abc123","event":"notification"}"#);
        assert_eq!(msg.session_id, "abc123");
        assert_eq!(msg.event, NotifyEvent::Notification);
    }

    #[test]
    fn parse_json_busy_event() {
        let msg = parse_notify_message(r#"{"session_id":"abc123","event":"busy"}"#);
        assert_eq!(msg.session_id, "abc123");
        assert_eq!(msg.event, NotifyEvent::Busy);
    }

    // ─── Hook detection tests ────────────────────────────────────────────────

    #[test]
    fn detect_claude_hook_installed() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");
        std::fs::write(&settings_path, r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"/path/to/planeai-stop-notify-claude.sh"}]}],"StopFailure":[{}],"Notification":[{}],"UserPromptSubmit":[{}]}}"#).unwrap();
        assert!(is_claude_hook_installed_at(&settings_path));
    }

    #[test]
    fn detect_claude_hook_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");
        std::fs::write(&settings_path, r#"{"hooks":{}}"#).unwrap();
        assert!(!is_claude_hook_installed_at(&settings_path));
    }

    #[test]
    fn detect_claude_hook_missing_event_triggers_reinstall() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");
        std::fs::write(&settings_path, r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"/path/to/planeai-stop-notify-claude.sh"}]}],"StopFailure":[{}],"Notification":[{}]}}"#).unwrap();
        assert!(!is_claude_hook_installed_at(&settings_path));
    }

    #[test]
    fn detect_claude_hook_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_claude_hook_installed_at(
            &dir.path().join("settings.json")
        ));
    }

    #[test]
    fn detect_copilot_hook_installed() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        std::fs::write(hooks_dir.join("planeai-notify.json"), r#"{"version":1,"hooks":{"agentStop":[{"type":"command","bash":"planeai-stop-notify-copilot.sh stop"}]}}"#).unwrap();
        assert!(is_copilot_hook_installed_at(dir.path()));
    }

    #[test]
    fn detect_copilot_hook_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_copilot_hook_installed_at(dir.path()));
    }

    #[test]
    fn detect_copilot_hook_at_custom_home() {
        let dir = tempfile::tempdir().unwrap();
        let custom_home = dir.path().join("custom-copilot");
        assert!(!is_copilot_hook_installed_at(&custom_home));
        install_copilot_hook_at(
            &custom_home,
            "/usr/local/bin/planeai-stop-notify-copilot.sh",
            "/usr/local/bin/planeai-stop-notify-copilot.ps1",
        )
        .unwrap();
        assert!(is_copilot_hook_installed_at(&custom_home));
    }

    #[test]
    fn detect_copilot_hook_wrong_content() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        std::fs::write(
            hooks_dir.join("planeai-notify.json"),
            r#"{"version":1,"hooks":{}}"#,
        )
        .unwrap();
        assert!(!is_copilot_hook_installed_at(dir.path()));
    }

    #[test]
    fn install_copilot_hook_creates_correct_structure() {
        let dir = tempfile::tempdir().unwrap();
        install_copilot_hook_at(dir.path(), "/path/script.sh", "/path/script.ps1").unwrap();
        let content =
            std::fs::read_to_string(dir.path().join("hooks/planeai-notify.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(v["version"], 1);
        assert!(v["hooks"]["agentStop"][0]["bash"]
            .as_str()
            .unwrap()
            .contains("stop"));
        assert!(v["hooks"]["userPromptSubmitted"][0]["bash"]
            .as_str()
            .unwrap()
            .contains("busy"));
        assert!(v["hooks"]["errorOccurred"][0]["bash"]
            .as_str()
            .unwrap()
            .contains("notification"));
    }

    #[test]
    fn install_copilot_hook_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        install_copilot_hook_at(dir.path(), "/path/script.sh", "/path/script.ps1").unwrap();
        install_copilot_hook_at(dir.path(), "/path/script.sh", "/path/script.ps1").unwrap();
        let content =
            std::fs::read_to_string(dir.path().join("hooks/planeai-notify.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(v["hooks"]["agentStop"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn install_claude_hook_creates_correct_structure() {
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        install_claude_hook_at(
            &claude_dir,
            "/home/user/.claude/hooks/planeai-stop-notify-claude.sh",
        )
        .unwrap();
        let settings: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(claude_dir.join("settings.json")).unwrap(),
        )
        .unwrap();
        assert!(settings["hooks"]["Stop"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .contains("planeai-stop-notify"));
        assert_eq!(
            settings["hooks"]["Notification"][0]["matcher"],
            "idle_prompt|permission_prompt"
        );
    }

    #[test]
    fn install_claude_hook_merges_with_existing_settings() {
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        let existing = serde_json::json!({
            "permissions": {"allow": ["Read"]},
            "hooks": {
                "PostToolUse": [{"matcher": "Write", "hooks": [{"type": "command", "command": "lint.sh"}]}]
            }
        });
        std::fs::write(
            claude_dir.join("settings.json"),
            serde_json::to_string_pretty(&existing).unwrap(),
        )
        .unwrap();
        install_claude_hook_at(
            &claude_dir,
            "/home/user/.claude/hooks/planeai-stop-notify-claude.sh",
        )
        .unwrap();
        let settings: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(claude_dir.join("settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(settings["permissions"]["allow"][0], "Read");
        assert_eq!(settings["hooks"]["PostToolUse"][0]["matcher"], "Write");
        assert!(settings["hooks"]["Stop"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .contains("planeai-stop-notify"));
    }

    #[test]
    fn detect_kiro_hook_installed() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("default.json");
        let config = serde_json::json!({
            "name": "default",
            "hooks": {
                "stop": [{ "command": "/path/to/planeai-stop-notify.sh" }],
                "userPromptSubmit": [{ "command": "/path/to/planeai-stop-notify.sh" }]
            }
        });
        std::fs::write(&config_path, serde_json::to_string(&config).unwrap()).unwrap();
        assert!(is_kiro_hook_installed_at(&config_path));
    }

    #[test]
    fn detect_kiro_hook_missing_user_prompt_submit_triggers_reinstall() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("default.json");
        let config = serde_json::json!({
            "name": "default",
            "hooks": {
                "stop": [{ "command": "/path/to/planeai-stop-notify.sh" }]
            }
        });
        std::fs::write(&config_path, serde_json::to_string(&config).unwrap()).unwrap();
        assert!(!is_kiro_hook_installed_at(&config_path));
    }

    #[test]
    fn detect_kiro_hook_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("default.json");
        std::fs::write(&config_path, r#"{"name":"default","tools":["*"]}"#).unwrap();
        assert!(!is_kiro_hook_installed_at(&config_path));
    }

    #[test]
    fn detect_kiro_hook_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_kiro_hook_installed_at(&dir.path().join("default.json")));
    }
}

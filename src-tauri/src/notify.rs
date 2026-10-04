use std::io::{BufRead, BufReader};
use std::path::Path;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use planeai::ipc::{Channel, IpcListener};

// Re-export shared types from planeai-core
#[cfg(test)]
use planeai_core::agent_hooks::{
    install_claude_hook_at, install_copilot_hook_at, is_claude_hook_installed_at,
    is_copilot_hook_installed_at, is_kiro_hook_installed_at,
};
pub use planeai_core::notify::{
    parse_notify_message, AgentState, NotifyEvent, NotifyMessage, NotifyState, SharedNotifyState,
};

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
        // Only emit Busy if the state actually transitioned. For hook-enabled
        // sessions, notify_output preserves Idle state (PTY echo is not an
        // authoritative busy signal), so we must re-check after the call.
        let is_now_busy = s.get_state(session_id) == Some(AgentState::Busy);
        if was_idle && is_now_busy {
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

    tracing::info!(path = %app_dir.join("notify.sock").display(), "notify socket listener started");

    thread::spawn(move || loop {
        let stream = match listener.accept() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("notify accept error: {e}");
                continue;
            }
        };
        let reader = BufReader::new(stream);
        for line in reader.lines().map_while(Result::ok) {
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }
            tracing::debug!(raw_line = %line, "socket message received");
            let msg = parse_notify_message(&line);
            if msg.session_id.is_empty() && !matches!(msg.event, NotifyEvent::TaskLifecycle) {
                continue;
            }
            dispatch_message(&msg, &state, &app);
        }
    });
}

fn dispatch_message(msg: &NotifyMessage, state: &SharedNotifyState, app: &AppHandle) {
    if ignores_socket_status(msg, state) {
        tracing::debug!(session_id = %msg.session_id, event = ?msg.event, "ignored hook status for provider-owned session");
        return;
    }
    match msg.event {
        NotifyEvent::SessionCreated => {
            tracing::info!(session_id = %msg.session_id, "[DEBUG-lsr1] session created via socket — emitting to webview");
            let _ = app.emit("session-created", msg.session_id.clone());
        }
        NotifyEvent::SessionChanged => {
            tracing::info!(session_id = %msg.session_id, "session changed via socket");
            let _ = app.emit("sessions-changed", ());
            reconcile_provider_sessions(app);
        }
        NotifyEvent::Busy => {
            let mut s = state.lock().unwrap();
            let name = s
                .get_meta(&msg.session_id)
                .map(|m| m.name.as_str())
                .unwrap_or("?");
            tracing::info!(session_id = %msg.session_id, name, "agent busy (hook signal)");
            let resumed = s.notify_busy(&msg.session_id);
            drop(s);
            emit_state_change(app, &msg.session_id, AgentState::Busy);
            // Only the agent's own hook reliably marks a resume; PTY output may be a redraw or echo.
            if resumed {
                let _ = app.emit(
                    "agent-resumed",
                    serde_json::json!({ "session_id": msg.session_id }),
                );
            }
        }
        NotifyEvent::Notification => {
            let fired = {
                let mut s = state.lock().unwrap();
                let name = s
                    .get_meta(&msg.session_id)
                    .map(|m| m.name.as_str())
                    .unwrap_or("?");
                tracing::info!(session_id = %msg.session_id, name, "immediate notification signal received");
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
                tracing::info!(session_id = %msg.session_id, %name, "stop received, debouncing 2s");
                s.notify_stop_debounced(&msg.session_id);
            } else {
                tracing::info!(session_id = %msg.session_id, %name, "stop received, firing immediately (no hook)");
                let fired = s.notify_stop(&msg.session_id);
                drop(s);
                if fired {
                    emit_state_change(app, &msg.session_id, AgentState::Idle);
                    fire_notification(app, &msg.session_id, state);
                }
            }
        }
        NotifyEvent::TaskLifecycle => {
            let Some(batch) = msg.lifecycle_batch.clone() else {
                tracing::warn!("discarding malformed task lifecycle notify message without batch");
                return;
            };
            let _ = app.emit("tasks-changed", ());
            app.state::<crate::plugins::PluginRuntimeHandle>()
                .0
                .dispatch_task_lifecycle(batch);
            // Completing a task archives its sessions from whichever process moved it.
            reconcile_provider_sessions(app);
        }
        NotifyEvent::SendPrompt => {
            let Some(text) = msg.text.clone() else {
                return;
            };
            let app = app.clone();
            let session_id = msg.session_id.clone();
            tauri::async_runtime::spawn(
                async move { deliver_prompt(&app, &session_id, text).await },
            );
        }
    }
}

fn reconcile_provider_sessions(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let runtime = crate::plugin_providers::AppRuntime::new(&app);
        // Ended outside the GUI, these sessions' handoff programs would otherwise run on unseen.
        for session_id in crate::plugin_providers::reconcile(&runtime).await {
            app.state::<crate::state::PtyState>()
                .0
                .end_programs(&session_id);
        }
    });
}

/// Provider sessions get busy/idle/attention from their plugin only (ADR-0013);
/// the user's own agent hooks still fire inside them and must not compete.
fn ignores_socket_status(msg: &NotifyMessage, state: &SharedNotifyState) -> bool {
    matches!(
        msg.event,
        NotifyEvent::Busy | NotifyEvent::Stop | NotifyEvent::Notification
    ) && state.lock().unwrap().is_provider_owned(&msg.session_id)
}

/// Delivers a prompt by the session's stored backend, so routing never depends on
/// whether this run has registered the session yet.
async fn deliver_prompt(app: &AppHandle, session_id: &str, text: String) {
    let db = app.state::<crate::state::DbState>().0.clone();
    let id = session_id.to_string();
    let provider_backed = crate::commands::blocking(move || {
        let conn = db.lock().map_err(|e| e.to_string())?;
        Ok(crate::db::get_session(&conn, &id)
            .map_err(|e| e.to_string())?
            .is_some_and(|session| session.backend == crate::session_ops::PLUGIN_BACKEND))
    })
    .await;
    let runtime = crate::plugin_providers::AppRuntime::new(app);
    let pty_manager = app.state::<crate::state::PtyState>().0.clone();
    let write_pty = |bytes: Vec<u8>| async move { pty_manager.write(session_id, &bytes).await };
    let routed = route_prompt(&runtime, provider_backed, session_id, &text, write_pty).await;
    if let Err(UndeliveredPrompt { error, report }) = routed {
        tracing::warn!(%session_id, %error, "send_prompt delivery failed");
        if report {
            let _ = app.emit(
                "prompt-delivery-failed",
                serde_json::json!({ "session_id": session_id, "error": error }),
            );
        }
    }
}

#[derive(Debug, PartialEq)]
struct UndeliveredPrompt {
    error: String,
    /// Its sender was told it was sent; a provider prompt fails for reasons a PTY write does not.
    report: bool,
}

async fn route_prompt<R, W, F>(
    runtime: &R,
    provider_backed: Result<bool, String>,
    session_id: &str,
    text: &str,
    write_pty: W,
) -> Result<(), UndeliveredPrompt>
where
    R: crate::plugin_providers::ProviderRuntime,
    W: FnOnce(Vec<u8>) -> F,
    F: std::future::Future<Output = Result<(), String>>,
{
    let (result, report) = match provider_backed {
        Ok(true) => (
            crate::plugin_providers::send(runtime, session_id, text).await,
            true,
        ),
        Ok(false) => (write_pty(format!("{text}\n").into_bytes()).await, false),
        Err(error) => (Err(error), false),
    };
    result.map_err(|error| UndeliveredPrompt { error, report })
}

/// Apply a status reported by a session's plugin provider.
pub fn apply_provider_status(
    app: &AppHandle,
    session_id: &str,
    status: crate::plugin_providers::ProviderSessionStatus,
) {
    use crate::plugin_providers::ProviderSessionStatus;
    let state = app.state::<crate::state::NotifyHandle>().0.clone();
    match status {
        ProviderSessionStatus::Busy => {
            let resumed = state.lock().unwrap().provider_busy(session_id);
            emit_state_change(app, session_id, AgentState::Busy);
            if resumed {
                let _ = app.emit(
                    "agent-resumed",
                    serde_json::json!({ "session_id": session_id }),
                );
            }
        }
        ProviderSessionStatus::Idle | ProviderSessionStatus::NeedsAttention => {
            let fired = state.lock().unwrap().provider_settled(session_id);
            if fired {
                emit_state_change(app, session_id, AgentState::Idle);
                fire_notification(app, session_id, &state);
            }
        }
        ProviderSessionStatus::Exited => {
            // A turn the agent was running ends with it, without a completion notification.
            release_provider_status(app, session_id);
            mark_provider_session_exited(app, session_id);
        }
    }
}

/// The sidecar driving a provider session stopped or died: a turn it was running will never
/// report its end, so the session stops showing as busy, without a completion notification.
pub fn release_provider_status(app: &AppHandle, session_id: &str) {
    let state = app.state::<crate::state::NotifyHandle>().0.clone();
    let released = state.lock().unwrap().provider_released(session_id);
    // Not `agent-state-change` Idle: the frontend treats that as a finished turn.
    if released {
        let _ = app.emit(
            "agent-released",
            serde_json::json!({ "session_id": session_id }),
        );
    }
}

fn mark_provider_session_exited(app: &AppHandle, session_id: &str) {
    let app = app.clone();
    let session_id = session_id.to_string();
    tauri::async_runtime::spawn(async move {
        let db = app.state::<crate::state::DbState>().0.clone();
        let id = session_id.clone();
        let exited = crate::commands::blocking(move || {
            let conn = db.lock().map_err(|e| e.to_string())?;
            crate::commands::sessions::lifecycle::exit_active_session(&conn, &id)
        })
        .await;
        match exited {
            Ok(Some(event)) => {
                let _ = app.emit("pty-exited", serde_json::json!({ "pty_key": session_id }));
                let _ = app.emit("sessions-changed", ());
                app.state::<crate::plugins::PluginRuntimeHandle>()
                    .0
                    .dispatch_session_lifecycle(event);
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(%session_id, %error, "failed to mark provider session exited")
            }
        }
    });
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
            // Emitted under the lock so a busy signal cannot publish its resume before this Idle.
            for id in &to_notify {
                emit_state_change(&app, id, AgentState::Idle);
            }
        }
        for session_id in to_notify {
            let name = {
                let s = state.lock().unwrap();
                s.get_meta(&session_id)
                    .map(|m| m.name.clone())
                    .unwrap_or_else(|| "?".into())
            };
            tracing::info!(%session_id, %name, "idle timeout, firing notification");
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
    match app
        .notification()
        .builder()
        .title(&title)
        .body(&body)
        .show()
    {
        Ok(_) => tracing::info!(%session_id, %title, %body, "OS notification sent"),
        Err(e) => {
            tracing::warn!(%session_id, %title, error = %e, "OS notification failed")
        }
    }
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
        let _ = state.notify_busy("s1");
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
    fn debounced_stop_not_cancelled_by_pty_output() {
        let mut state = NotifyState::new();
        state.register_session("s1", "test", "project", true);
        state.notify_output("s1");
        state.notify_stop_debounced("s1");
        state.advance_time("s1", Duration::from_secs(1));
        state.notify_output("s1");
        state.advance_time("s1", Duration::from_secs(1));
        assert!(state.check_debounce("s1"));
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

    mod prompt_routing {
        use super::super::{route_prompt, UndeliveredPrompt};
        use crate::plugin_providers::fake::FakeRuntime;
        use planeai_plugin_contract::provider as protocol;
        use std::sync::Mutex;

        #[tokio::test]
        async fn a_provider_session_receives_its_prompt_through_the_provider() {
            let runtime = FakeRuntime::running(&[("s1", "active")]);
            let typed = Mutex::new(None);
            let write = |bytes: Vec<u8>| {
                *typed.lock().unwrap() = Some(bytes);
                async { Ok(()) }
            };
            route_prompt(&runtime, Ok(true), "s1", "hello", write)
                .await
                .unwrap();
            assert_eq!(runtime.sent(), [protocol::RESUME, protocol::SEND]);
            assert!(typed.lock().unwrap().is_none());
        }

        #[tokio::test]
        async fn a_failed_provider_prompt_is_reported_to_its_sender() {
            let runtime = FakeRuntime::running(&[("s1", "active")]);
            runtime.failing.lock().unwrap().push(protocol::SEND);
            let error = route_prompt(&runtime, Ok(true), "s1", "hello", |_| async { Ok(()) })
                .await
                .unwrap_err();
            assert!(error.report);
        }

        #[tokio::test]
        async fn a_terminal_session_has_its_prompt_typed_with_a_newline() {
            let runtime = FakeRuntime::running(&[]);
            let typed = Mutex::new(Vec::new());
            let write = |bytes: Vec<u8>| {
                *typed.lock().unwrap() = bytes;
                async { Err("pty gone".to_string()) }
            };
            let error = route_prompt(&runtime, Ok(false), "s1", "hi", write)
                .await
                .unwrap_err();
            assert_eq!(*typed.lock().unwrap(), b"hi\n");
            assert_eq!(
                error,
                UndeliveredPrompt {
                    error: "pty gone".into(),
                    report: false
                }
            );
            assert!(runtime.sent().is_empty());
        }

        #[tokio::test]
        async fn an_unknown_backend_is_not_reported() {
            let runtime = FakeRuntime::running(&[]);
            let error = route_prompt(&runtime, Err("db locked".into()), "s1", "hi", |_| async {
                Ok(())
            })
            .await
            .unwrap_err();
            assert!(!error.report);
        }
    }
}

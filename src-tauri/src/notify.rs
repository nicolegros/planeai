use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

use planeai::ipc::{Channel, IpcListener};

const SILENCE_THRESHOLD: Duration = Duration::from_secs(5);
const SILENCE_CHECK_INTERVAL: Duration = Duration::from_secs(1);
const DEBOUNCE_THRESHOLD: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum AgentState {
    Busy,
    Idle,
}

pub struct NotifyState {
    states: HashMap<String, AgentState>,
    last_output: HashMap<String, Instant>,
    meta: HashMap<String, SessionMeta>,
    /// Sessions that have already fired a notification and are waiting for user focus.
    notified: std::collections::HashSet<String>,
    /// Sessions waiting for debounce threshold before notification fires.
    idle_since: HashMap<String, Instant>,
    #[cfg(test)]
    time_offset: HashMap<String, Duration>,
}

impl NotifyState {
    pub fn new() -> Self {
        Self {
            states: HashMap::new(),
            last_output: HashMap::new(),
            meta: HashMap::new(),
            notified: std::collections::HashSet::new(),
            idle_since: HashMap::new(),
            #[cfg(test)]
            time_offset: HashMap::new(),
        }
    }

    pub fn register_session(
        &mut self,
        session_id: &str,
        name: &str,
        project_name: &str,
        hook_enabled: bool,
    ) {
        self.meta.insert(
            session_id.to_string(),
            SessionMeta {
                name: name.to_string(),
                project_name: project_name.to_string(),
                hook_enabled,
            },
        );
    }

    pub fn get_meta(&self, session_id: &str) -> Option<&SessionMeta> {
        self.meta.get(session_id)
    }

    pub fn notify_stop(&mut self, session_id: &str) -> bool {
        if self.get_state(session_id) == Some(AgentState::Idle) {
            return false;
        }
        self.states.insert(session_id.to_string(), AgentState::Idle);
        // Only fire notification if we haven't already notified for this idle period
        if self.notified.contains(session_id) {
            return false;
        }
        self.notified.insert(session_id.to_string());
        true
    }

    /// Fire notification immediately (permission prompts, idle prompts, errors).
    pub fn notify_stop_immediate(&mut self, session_id: &str) -> bool {
        self.idle_since.remove(session_id);
        self.notify_stop(session_id)
    }

    /// Record idle transition but don't fire notification yet — wait for debounce threshold.
    /// Returns false always (notification is deferred to check_debounce).
    pub fn notify_stop_debounced(&mut self, session_id: &str) -> bool {
        self.states.insert(session_id.to_string(), AgentState::Idle);
        if !self.notified.contains(session_id) {
            self.idle_since
                .insert(session_id.to_string(), Instant::now());
        }
        false
    }

    /// Check if a debounced session has been idle long enough to fire notification.
    pub fn check_debounce(&mut self, session_id: &str) -> bool {
        if let Some(&since) = self.idle_since.get(session_id) {
            let elapsed = self.elapsed_since(session_id, since);
            if elapsed >= DEBOUNCE_THRESHOLD {
                self.idle_since.remove(session_id);
                if self.notified.contains(session_id) {
                    return false;
                }
                self.notified.insert(session_id.to_string());
                return true;
            }
        }
        false
    }

    pub fn notify_output(&mut self, session_id: &str) {
        let was = self.get_state(session_id);
        self.states.insert(session_id.to_string(), AgentState::Busy);
        self.last_output
            .insert(session_id.to_string(), Instant::now());
        self.idle_since.remove(session_id);
        let had_notified = self.notified.remove(session_id);
        if was == Some(AgentState::Idle) || had_notified {
            let name = self
                .meta
                .get(session_id)
                .map(|m| m.name.as_str())
                .unwrap_or("?");
            eprintln!("[notify] \"{name}\" is now busy");
        }
    }

    /// Clear the notified flag when the user focuses/acknowledges a session.
    pub fn acknowledge(&mut self, session_id: &str) {
        self.notified.remove(session_id);
    }

    pub fn check_silence(&mut self, session_id: &str) -> bool {
        if self.get_state(session_id) != Some(AgentState::Busy) {
            return false;
        }
        // Skip silence-based detection for sessions with a working hook
        if self.meta.get(session_id).is_some_and(|m| m.hook_enabled) {
            return false;
        }
        if let Some(&last) = self.last_output.get(session_id) {
            let elapsed = self.elapsed_since(session_id, last);
            if elapsed >= SILENCE_THRESHOLD {
                self.states.insert(session_id.to_string(), AgentState::Idle);
                if self.notified.contains(session_id) {
                    return false;
                }
                self.notified.insert(session_id.to_string());
                return true;
            }
        }
        false
    }

    /// Returns all session IDs that are currently being tracked as Busy.
    pub fn busy_sessions(&self) -> Vec<String> {
        self.states
            .iter()
            .filter(|(_, &s)| s == AgentState::Busy)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Returns all session IDs waiting for debounce check.
    pub fn debounced_sessions(&self) -> Vec<String> {
        self.idle_since.keys().cloned().collect()
    }

    pub fn get_state(&self, session_id: &str) -> Option<AgentState> {
        self.states.get(session_id).copied()
    }

    #[cfg(not(test))]
    fn elapsed_since(&self, _session_id: &str, since: Instant) -> Duration {
        since.elapsed()
    }

    #[cfg(test)]
    fn elapsed_since(&self, session_id: &str, since: Instant) -> Duration {
        let offset = self
            .time_offset
            .get(session_id)
            .copied()
            .unwrap_or_default();
        since.elapsed() + offset
    }

    #[cfg(test)]
    pub fn advance_time(&mut self, session_id: &str, duration: Duration) {
        let entry = self.time_offset.entry(session_id.to_string()).or_default();
        *entry += duration;
    }
}

/// Shared handle to NotifyState used across threads.
pub type SharedNotifyState = Arc<Mutex<NotifyState>>;

/// Adapter that implements OutputObserver by forwarding to NotifyState + emitting Tauri events.
/// Preserves the exact behavior previously inlined in the PTY reader thread.
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotifyEvent {
    Stop,
    Notification,
    Busy,
    SessionCreated,
    SessionChanged,
}

pub struct NotifyMessage {
    pub session_id: String,
    pub event: NotifyEvent,
}

/// Parse a JSONL line from the socket. Falls back to treating the line as a bare session ID.
pub fn parse_notify_message(line: &str) -> NotifyMessage {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
        let session_id = v
            .get("session_id")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        let event = match v.get("event").and_then(|e| e.as_str()) {
            Some("notification") => NotifyEvent::Notification,
            Some("busy") => NotifyEvent::Busy,
            Some("session_created") => NotifyEvent::SessionCreated,
            Some("session_changed") => NotifyEvent::SessionChanged,
            _ => NotifyEvent::Stop,
        };
        NotifyMessage { session_id, event }
    } else {
        NotifyMessage {
            session_id: line.trim().to_string(),
            event: NotifyEvent::Stop,
        }
    }
}

/// Check if the Kiro CLI hook is installed at the given agents config path.
/// Returns true only if both stop and userPromptSubmit hooks are configured.
pub fn is_kiro_hook_installed_at(config_path: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(config_path) else {
        return false;
    };
    if !content.contains("planeai-stop-notify") {
        return false;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    let Some(hooks) = v.get("hooks").and_then(|h| h.as_object()) else {
        return false;
    };
    ["stop", "userPromptSubmit"].iter().all(|event| {
        hooks
            .get(*event)
            .and_then(|a| a.as_array())
            .is_some_and(|arr| {
                arr.iter().any(|h| {
                    h.get("command")
                        .and_then(|c| c.as_str())
                        .is_some_and(|c| c.contains("planeai-stop-notify"))
                })
            })
    })
}

/// Check if the Claude Code hook is installed at the given settings path.
/// Returns true only if all expected hook events are configured.
pub fn is_claude_hook_installed_at(settings_path: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(settings_path) else {
        return false;
    };
    if !content.contains("planeai-stop-notify") {
        return false;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    let Some(hooks) = v.get("hooks").and_then(|h| h.as_object()) else {
        return false;
    };
    ["Stop", "StopFailure", "Notification", "UserPromptSubmit"]
        .iter()
        .all(|event| hooks.contains_key(*event))
}

/// Check if the Copilot CLI hook is installed at the given copilot home directory.
/// Looks for `hooks/planeai-notify.json` containing our hook script reference.
pub fn is_copilot_hook_installed_at(copilot_dir: &Path) -> bool {
    let notify_path = copilot_dir.join("hooks").join("planeai-notify.json");
    let Ok(content) = std::fs::read_to_string(notify_path) else {
        return false;
    };
    content.contains("planeai-stop-notify-copilot")
}

/// Install the Copilot CLI notification hook into the given copilot directory.
/// Creates `hooks/planeai-notify.json` with agentStop, userPromptSubmitted, and errorOccurred hooks.
pub fn install_copilot_hook_at(
    copilot_dir: &Path,
    bash_script: &str,
    ps_script: &str,
) -> Result<(), String> {
    let hooks_dir = copilot_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir).map_err(|e| format!("failed to create hooks dir: {e}"))?;

    let config = serde_json::json!({
        "version": 1,
        "hooks": {
            "agentStop": [{ "type": "command", "bash": format!("{bash_script} stop"), "powershell": format!("{ps_script} stop"), "timeoutSec": 5 }],
            "userPromptSubmitted": [{ "type": "command", "bash": format!("{bash_script} busy"), "powershell": format!("{ps_script} busy"), "timeoutSec": 5 }],
            "errorOccurred": [{ "type": "command", "bash": format!("{bash_script} notification"), "powershell": format!("{ps_script} notification"), "timeoutSec": 5 }]
        }
    });

    let output = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(hooks_dir.join("planeai-notify.json"), output)
        .map_err(|e| format!("failed to write planeai-notify.json: {e}"))?;
    Ok(())
}

/// Install the Claude Code notification hook into the given .claude directory.
/// `script_command` is the path to the hook script to reference in the config.
pub fn install_claude_hook_at(claude_dir: &Path, script_command: &str) -> Result<(), String> {
    std::fs::create_dir_all(claude_dir)
        .map_err(|e| format!("failed to create .claude dir: {e}"))?;

    let settings_path = claude_dir.join("settings.json");
    let mut settings: serde_json::Value = if let Ok(content) =
        std::fs::read_to_string(&settings_path)
    {
        serde_json::from_str(&content).map_err(|e| format!("failed to parse settings.json: {e}"))?
    } else {
        serde_json::json!({})
    };

    let hooks = settings
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    let hooks_obj = hooks.as_object_mut().unwrap();

    let hook_entry = serde_json::json!({
        "type": "command",
        "command": script_command,
        "args": []
    });

    let mut ensure_hook = |event: &str, matcher: Option<&str>| {
        let arr = hooks_obj
            .entry(event)
            .or_insert_with(|| serde_json::json!([]));
        let arr = arr.as_array_mut().unwrap();
        if arr.iter().any(|g| {
            serde_json::to_string(g)
                .unwrap_or_default()
                .contains("planeai-stop-notify")
        }) {
            return;
        }
        let mut group = serde_json::json!({ "hooks": [hook_entry] });
        if let Some(m) = matcher {
            group
                .as_object_mut()
                .unwrap()
                .insert("matcher".to_string(), serde_json::json!(m));
        }
        arr.push(group);
    };

    ensure_hook("Stop", None);
    ensure_hook("StopFailure", None);
    ensure_hook("Notification", Some("idle_prompt|permission_prompt"));
    ensure_hook("UserPromptSubmit", None);

    let output = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(&settings_path, output)
        .map_err(|e| format!("failed to write settings.json: {e}"))?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub name: String,
    pub project_name: String,
    pub hook_enabled: bool,
}

/// Start the IPC listener thread. Incoming JSONL lines are parsed and dispatched
/// to immediate or debounced notification paths.
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

/// Start the silence checker thread. Periodically checks all busy sessions
/// and fires debounced notifications that have exceeded the threshold.
pub fn start_silence_checker(state: SharedNotifyState, app: AppHandle) {
    thread::spawn(move || loop {
        thread::sleep(SILENCE_CHECK_INTERVAL);
        let mut to_notify: Vec<String> = Vec::new();
        {
            let mut s = state.lock().unwrap();
            // Check silence-based idle detection
            let busy = s.busy_sessions();
            for id in busy {
                if s.check_silence(&id) {
                    to_notify.push(id);
                }
            }
            // Check debounced hook-based notifications
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

/// Emit a Tauri event for state change.
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

/// Fire native notification.
fn fire_notification(app: &AppHandle, session_id: &str, state: &SharedNotifyState) {
    let (title, body) = {
        let s = state.lock().unwrap();
        match s.get_meta(session_id) {
            Some(meta) => (meta.project_name.clone(), format!("{} is ready", meta.name)),
            None => ("planeai".to_string(), "Agent is ready".to_string()),
        }
    };

    // Native notification via Tauri plugin (uses app icon)
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

    #[test]
    fn socket_notification_transitions_session_to_idle() {
        let mut state = NotifyState::new();
        let session_id = "test-session-1";

        state.notify_stop(session_id);

        assert_eq!(state.get_state(session_id), Some(AgentState::Idle));
    }

    #[test]
    fn pty_output_transitions_session_to_busy() {
        let mut state = NotifyState::new();
        let session_id = "test-session-1";

        state.notify_stop(session_id);
        assert_eq!(state.get_state(session_id), Some(AgentState::Idle));

        state.notify_output(session_id);
        assert_eq!(state.get_state(session_id), Some(AgentState::Busy));
    }

    #[test]
    fn silence_timeout_transitions_to_idle() {
        let mut state = NotifyState::new();
        let session_id = "test-session-1";

        state.notify_output(session_id);
        assert_eq!(state.get_state(session_id), Some(AgentState::Busy));

        state.advance_time(session_id, Duration::from_secs(5));
        let timed_out = state.check_silence(session_id);

        assert!(timed_out);
        assert_eq!(state.get_state(session_id), Some(AgentState::Idle));
    }

    #[test]
    fn notification_fires_once_per_idle_transition() {
        let mut state = NotifyState::new();
        let session_id = "test-session-1";

        // First stop fires notification
        assert!(state.notify_stop(session_id));
        // Second stop does not (already idle)
        assert!(!state.notify_stop(session_id));

        // Output transitions to busy and clears notified flag
        state.notify_output(session_id);
        // Stop fires again since output cleared the flag
        assert!(state.notify_stop(session_id));
    }

    #[test]
    fn silence_check_skipped_for_hook_enabled_sessions() {
        let mut state = NotifyState::new();
        let session_id = "test-session-hook";

        state.register_session(session_id, "test", "project", true);
        state.notify_output(session_id);
        state.advance_time(session_id, Duration::from_secs(10));

        // Should NOT transition to idle via silence
        assert!(!state.check_silence(session_id));
        assert_eq!(state.get_state(session_id), Some(AgentState::Busy));

        // But hook can still transition it
        assert!(state.notify_stop(session_id));
        assert_eq!(state.get_state(session_id), Some(AgentState::Idle));
    }

    #[test]
    fn debounced_stop_does_not_fire_immediately_but_fires_after_threshold() {
        let mut state = NotifyState::new();
        let session_id = "test-debounce";

        state.register_session(session_id, "test", "project", true);
        state.notify_output(session_id);

        // Debounced stop should not fire immediately
        assert!(!state.notify_stop_debounced(session_id));
        assert_eq!(state.get_state(session_id), Some(AgentState::Idle));

        // Before 2s, check_debounce should not fire
        state.advance_time(session_id, Duration::from_secs(1));
        assert!(!state.check_debounce(session_id));

        // After 2s, check_debounce should fire
        state.advance_time(session_id, Duration::from_secs(1));
        assert!(state.check_debounce(session_id));
    }

    #[test]
    fn debounced_stop_cancelled_by_output() {
        let mut state = NotifyState::new();
        let session_id = "test-debounce-cancel";

        state.register_session(session_id, "test", "project", true);
        state.notify_output(session_id);

        // Debounced stop
        state.notify_stop_debounced(session_id);

        // Output arrives within 2s — cancels the debounce
        state.advance_time(session_id, Duration::from_secs(1));
        state.notify_output(session_id);

        // Even after threshold, check_debounce should not fire
        state.advance_time(session_id, Duration::from_secs(2));
        assert!(!state.check_debounce(session_id));
    }

    #[test]
    fn immediate_notification_fires_instantly() {
        let mut state = NotifyState::new();
        let session_id = "test-immediate";

        state.register_session(session_id, "test", "project", true);
        state.notify_output(session_id);

        // Immediate notification fires right away (returns true)
        assert!(state.notify_stop_immediate(session_id));
        assert_eq!(state.get_state(session_id), Some(AgentState::Idle));
    }

    #[test]
    fn parse_json_stop_event() {
        let line = r#"{"session_id":"abc123","event":"stop"}"#;
        let msg = parse_notify_message(line);
        assert_eq!(msg.session_id, "abc123");
        assert_eq!(msg.event, NotifyEvent::Stop);
    }

    #[test]
    fn parse_json_notification_event() {
        let line = r#"{"session_id":"abc123","event":"notification"}"#;
        let msg = parse_notify_message(line);
        assert_eq!(msg.session_id, "abc123");
        assert_eq!(msg.event, NotifyEvent::Notification);
    }

    #[test]
    fn parse_bare_session_id_as_debounced_stop() {
        let line = "abc123";
        let msg = parse_notify_message(line);
        assert_eq!(msg.session_id, "abc123");
        assert_eq!(msg.event, NotifyEvent::Stop);
    }

    #[test]
    fn parse_json_busy_event() {
        let line = r#"{"session_id":"abc123","event":"busy"}"#;
        let msg = parse_notify_message(line);
        assert_eq!(msg.session_id, "abc123");
        assert_eq!(msg.event, NotifyEvent::Busy);
    }

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
        // Missing UserPromptSubmit — should trigger reinstall
        std::fs::write(&settings_path, r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"/path/to/planeai-stop-notify-claude.sh"}]}],"StopFailure":[{}],"Notification":[{}]}}"#).unwrap();

        assert!(!is_claude_hook_installed_at(&settings_path));
    }

    #[test]
    fn detect_claude_hook_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");

        assert!(!is_claude_hook_installed_at(&settings_path));
    }

    #[test]
    fn detect_copilot_hook_installed() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        let notify_path = hooks_dir.join("planeai-notify.json");
        std::fs::write(&notify_path, r#"{"version":1,"hooks":{"agentStop":[{"type":"command","bash":"planeai-stop-notify-copilot.sh stop"}],"userPromptSubmitted":[{"type":"command","bash":"planeai-stop-notify-copilot.sh busy"}],"errorOccurred":[{"type":"command","bash":"planeai-stop-notify-copilot.sh notification"}]}}"#).unwrap();

        assert!(is_copilot_hook_installed_at(dir.path()));
    }

    #[test]
    fn detect_copilot_hook_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_copilot_hook_installed_at(dir.path()));
    }

    #[test]
    fn detect_copilot_hook_at_custom_home() {
        // Simulates COPILOT_HOME pointing to a non-default location
        let dir = tempfile::tempdir().unwrap();
        let custom_home = dir.path().join("custom-copilot");
        let hooks_dir = custom_home.join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();

        // Not installed yet
        assert!(!is_copilot_hook_installed_at(&custom_home));

        // Install at custom home
        install_copilot_hook_at(
            &custom_home,
            "/usr/local/bin/planeai-stop-notify-copilot.sh",
            "/usr/local/bin/planeai-stop-notify-copilot.ps1",
        )
        .unwrap();

        // Now detected
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
        let copilot_dir = dir.path();

        install_copilot_hook_at(
            copilot_dir,
            "/path/to/planeai-stop-notify-copilot.sh",
            "/path/to/planeai-stop-notify-copilot.ps1",
        )
        .unwrap();

        let content =
            std::fs::read_to_string(copilot_dir.join("hooks").join("planeai-notify.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();

        assert_eq!(v["version"], 1);
        // agentStop hook with bash and powershell
        let agent_stop = &v["hooks"]["agentStop"][0];
        assert_eq!(agent_stop["type"], "command");
        assert!(agent_stop["bash"]
            .as_str()
            .unwrap()
            .contains("planeai-stop-notify-copilot"));
        assert!(agent_stop["bash"].as_str().unwrap().contains("stop"));
        assert!(agent_stop["powershell"]
            .as_str()
            .unwrap()
            .contains("planeai-stop-notify-copilot"));
        // userPromptSubmitted
        let prompt = &v["hooks"]["userPromptSubmitted"][0];
        assert!(prompt["bash"].as_str().unwrap().contains("busy"));
        // errorOccurred
        let error = &v["hooks"]["errorOccurred"][0];
        assert!(error["bash"].as_str().unwrap().contains("notification"));
    }

    #[test]
    fn install_copilot_hook_writes_script_files() {
        let dir = tempfile::tempdir().unwrap();
        let copilot_dir = dir.path();
        let hooks_dir = copilot_dir.join("hooks");

        let bash_script = hooks_dir.join("planeai-stop-notify-copilot.sh");
        let ps_script = hooks_dir.join("planeai-stop-notify-copilot.ps1");

        install_copilot_hook_at(
            copilot_dir,
            bash_script.to_str().unwrap(),
            ps_script.to_str().unwrap(),
        )
        .unwrap();

        // Write scripts manually (as install_copilot_hook in main.rs would)
        let bash_content = include_str!("../resources/planeai-stop-notify-copilot.sh");
        let ps_content = include_str!("../resources/planeai-stop-notify-copilot.ps1");
        std::fs::write(&bash_script, bash_content).unwrap();
        std::fs::write(&ps_script, ps_content).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bash_script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        // Verify script content
        let content = std::fs::read_to_string(&bash_script).unwrap();
        assert!(content.contains("PLANEAI_SESSION_ID"));
        assert!(content.contains("case \"$1\""));

        // Verify permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::metadata(&bash_script).unwrap().permissions();
            assert_eq!(perms.mode() & 0o777, 0o755);
        }
    }

    #[test]
    fn install_copilot_hook_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let copilot_dir = dir.path();

        install_copilot_hook_at(copilot_dir, "/path/script.sh", "/path/script.ps1").unwrap();
        install_copilot_hook_at(copilot_dir, "/path/script.sh", "/path/script.ps1").unwrap();

        let content =
            std::fs::read_to_string(copilot_dir.join("hooks").join("planeai-notify.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();

        // Should still have exactly one entry per hook, not duplicates
        assert_eq!(v["hooks"]["agentStop"].as_array().unwrap().len(), 1);
        assert_eq!(
            v["hooks"]["userPromptSubmitted"].as_array().unwrap().len(),
            1
        );
        assert_eq!(v["hooks"]["errorOccurred"].as_array().unwrap().len(), 1);
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

        // Check Stop hook exists with our command
        let stop = &settings["hooks"]["Stop"][0]["hooks"][0];
        assert_eq!(stop["type"], "command");
        assert!(stop["command"]
            .as_str()
            .unwrap()
            .contains("planeai-stop-notify"));

        // Check StopFailure hook exists
        let stop_failure = &settings["hooks"]["StopFailure"][0]["hooks"][0];
        assert_eq!(stop_failure["type"], "command");

        // Check Notification hook with matcher
        let notif = &settings["hooks"]["Notification"][0];
        assert_eq!(notif["matcher"], "idle_prompt|permission_prompt");
        assert_eq!(notif["hooks"][0]["type"], "command");
    }

    #[test]
    fn install_claude_hook_merges_with_existing_settings() {
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();

        // Pre-existing settings with user hooks and other config
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

        // Existing config preserved
        assert_eq!(settings["permissions"]["allow"][0], "Read");
        // Existing hooks preserved
        assert_eq!(settings["hooks"]["PostToolUse"][0]["matcher"], "Write");
        // Our hooks added
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
            "tools": ["*"],
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
        // Old install with only "stop" hook
        let config = serde_json::json!({
            "name": "default",
            "tools": ["*"],
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
        let config = serde_json::json!({ "name": "default", "tools": ["*"] });
        std::fs::write(&config_path, serde_json::to_string(&config).unwrap()).unwrap();

        assert!(!is_kiro_hook_installed_at(&config_path));
    }

    #[test]
    fn detect_kiro_hook_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("default.json");

        assert!(!is_kiro_hook_installed_at(&config_path));
    }
}

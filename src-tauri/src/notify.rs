use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

const SILENCE_THRESHOLD: Duration = Duration::from_secs(5);
const SILENCE_CHECK_INTERVAL: Duration = Duration::from_secs(1);
const SOCK_NAME: &str = "notify.sock";

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
            #[cfg(test)]
            time_offset: HashMap::new(),
        }
    }

    pub fn register_session(&mut self, session_id: &str, name: &str, project_name: &str, hook_enabled: bool) {
        self.meta.insert(session_id.to_string(), SessionMeta {
            name: name.to_string(),
            project_name: project_name.to_string(),
            hook_enabled,
        });
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

    pub fn notify_output(&mut self, session_id: &str) {
        let was = self.get_state(session_id);
        self.states.insert(session_id.to_string(), AgentState::Busy);
        self.last_output.insert(session_id.to_string(), Instant::now());
        let had_notified = self.notified.remove(session_id);
        if was != Some(AgentState::Busy) || had_notified {
            eprintln!("[notify] output for {session_id} | was={was:?} | cleared_notified={had_notified}");
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
        if self.meta.get(session_id).map_or(false, |m| m.hook_enabled) {
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

    pub fn get_state(&self, session_id: &str) -> Option<AgentState> {
        self.states.get(session_id).copied()
    }

    #[cfg(not(test))]
    fn elapsed_since(&self, _session_id: &str, since: Instant) -> Duration {
        since.elapsed()
    }

    #[cfg(test)]
    fn elapsed_since(&self, session_id: &str, since: Instant) -> Duration {
        let offset = self.time_offset.get(session_id).copied().unwrap_or_default();
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

#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub name: String,
    pub project_name: String,
    pub hook_enabled: bool,
}

/// Returns the socket path inside the given app data directory.
pub fn socket_path(app_dir: &Path) -> PathBuf {
    app_dir.join(SOCK_NAME)
}

/// Start the Unix socket listener thread. Incoming lines are treated as session IDs
/// that just finished a turn (stop hook fired).
pub fn start_socket_listener(app_dir: &Path, state: SharedNotifyState, app: AppHandle) {
    let sock = socket_path(app_dir);
    // Remove stale socket
    let _ = std::fs::remove_file(&sock);

    let listener = UnixListener::bind(&sock).expect("failed to bind notify socket");

    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let reader = BufReader::new(stream);
            for line in reader.lines().map_while(Result::ok) {
                let session_id = line.trim().to_string();
                if session_id.is_empty() {
                    continue;
                }
                let fired = {
                    let mut s = state.lock().unwrap();
                    let current_state = s.get_state(&session_id);
                    let result = s.notify_stop(&session_id);
                    eprintln!("[notify] socket received stop for {session_id} | was={current_state:?} | fired={result}");
                    result
                };
                if fired {
                    emit_state_change(&app, &session_id, AgentState::Idle);
                    fire_notification(&app, &session_id, &state);
                }
            }
        }
    });
}

/// Start the silence checker thread. Periodically checks all busy sessions.
pub fn start_silence_checker(state: SharedNotifyState, app: AppHandle) {
    thread::spawn(move || loop {
        thread::sleep(SILENCE_CHECK_INTERVAL);
        let timed_out: Vec<String> = {
            let mut s = state.lock().unwrap();
            let busy = s.busy_sessions();
            busy.into_iter()
                .filter(|id| s.check_silence(id))
                .collect()
        };
        for session_id in timed_out {
            eprintln!("[notify] silence timeout fired for {session_id}");
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
    let _ = app.notification().builder()
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
}

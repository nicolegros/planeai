use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::task_lifecycle::TaskLifecycleBatch;

pub const SILENCE_THRESHOLD: Duration = Duration::from_secs(5);
pub const DEBOUNCE_THRESHOLD: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum AgentState {
    Busy,
    Idle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotifyEvent {
    Stop,
    Notification,
    Busy,
    SessionCreated,
    SessionChanged,
    SendPrompt,
    TaskLifecycle,
}

#[derive(Debug, Clone)]
pub struct NotifyMessage {
    pub session_id: String,
    pub event: NotifyEvent,
    pub text: Option<String>,
    pub lifecycle_batch: Option<TaskLifecycleBatch>,
}

#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub name: String,
    pub project_name: String,
    pub hook_enabled: bool,
}

pub struct NotifyState {
    states: HashMap<String, AgentState>,
    last_output: HashMap<String, Instant>,
    meta: HashMap<String, SessionMeta>,
    notified: std::collections::HashSet<String>,
    idle_since: HashMap<String, Instant>,
    /// Sessions whose status comes only from their plugin provider (ADR-0013).
    provider_owned: std::collections::HashSet<String>,
    #[cfg(any(test, feature = "test-support"))]
    time_offset: HashMap<String, Duration>,
}

impl Default for NotifyState {
    fn default() -> Self {
        Self::new()
    }
}

impl NotifyState {
    pub fn new() -> Self {
        Self {
            states: HashMap::new(),
            last_output: HashMap::new(),
            meta: HashMap::new(),
            notified: std::collections::HashSet::new(),
            idle_since: HashMap::new(),
            provider_owned: std::collections::HashSet::new(),
            #[cfg(any(test, feature = "test-support"))]
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

    /// Hand a session's status to its plugin provider: hook and PTY signals stop applying.
    pub fn mark_provider_owned(&mut self, session_id: &str) {
        self.provider_owned.insert(session_id.to_string());
    }

    /// A provider session failed to launch; its id will never report status.
    pub fn release_provider_owned(&mut self, session_id: &str) {
        self.provider_owned.remove(session_id);
    }

    pub fn is_provider_owned(&self, session_id: &str) -> bool {
        self.provider_owned.contains(session_id)
    }

    /// A provider reported a running turn. Returns whether the session resumed.
    pub fn provider_busy(&mut self, session_id: &str) -> bool {
        self.mark_provider_owned(session_id);
        self.notify_busy(session_id)
    }

    /// A provider reported its session idle or waiting on the user. Returns whether that
    /// ends a turn worth notifying: providers also report idle when they start or resume a
    /// session, which is not a finished turn.
    pub fn provider_settled(&mut self, session_id: &str) -> bool {
        self.mark_provider_owned(session_id);
        if self.get_state(session_id) != Some(AgentState::Busy) {
            self.states.insert(session_id.to_string(), AgentState::Idle);
            self.idle_since.remove(session_id);
            return false;
        }
        self.notify_stop_immediate(session_id)
    }

    /// The provider driving a session went away, so a running turn will never report its
    /// end. Returns whether the session was busy; it is idle afterwards, without notifying.
    pub fn provider_released(&mut self, session_id: &str) -> bool {
        if self.get_state(session_id) != Some(AgentState::Busy) {
            return false;
        }
        self.states.insert(session_id.to_string(), AgentState::Idle);
        self.idle_since.remove(session_id);
        true
    }

    pub fn get_meta(&self, session_id: &str) -> Option<&SessionMeta> {
        self.meta.get(session_id)
    }

    pub fn notify_stop(&mut self, session_id: &str) -> bool {
        if self.get_state(session_id) == Some(AgentState::Idle) {
            return false;
        }
        self.states.insert(session_id.to_string(), AgentState::Idle);
        if self.notified.contains(session_id) {
            return false;
        }
        self.notified.insert(session_id.to_string());
        true
    }

    pub fn notify_stop_immediate(&mut self, session_id: &str) -> bool {
        self.idle_since.remove(session_id);
        self.notify_stop(session_id)
    }

    pub fn notify_stop_debounced(&mut self, session_id: &str) -> bool {
        self.states.insert(session_id.to_string(), AgentState::Idle);
        if !self.notified.contains(session_id) {
            self.idle_since
                .insert(session_id.to_string(), Instant::now());
        }
        false
    }

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
        // For hook-enabled sessions, PTY output should not override the idle state.
        // The hook is the authoritative signal for when the agent stops — PTY output
        // is just terminal rendering noise that may trail after the stop hook fires.
        let hook_enabled = self.meta.get(session_id).is_some_and(|m| m.hook_enabled);
        // Only hooks mark a hook-enabled session busy; a redraw after attach must not.
        if hook_enabled && self.get_state(session_id) != Some(AgentState::Busy) {
            self.last_output
                .insert(session_id.to_string(), Instant::now());
            return;
        }

        self.states.insert(session_id.to_string(), AgentState::Busy);
        self.last_output
            .insert(session_id.to_string(), Instant::now());
        self.idle_since.remove(session_id);
        self.notified.remove(session_id);
    }

    /// Transition to Busy from an authoritative hook signal (e.g., userPromptSubmit).
    /// Unlike `notify_output`, this always cancels any pending debounce and resets state,
    /// regardless of hook_enabled status.
    ///
    /// Returns true unless the agent was already busy, so repeated signals (one per tool
    /// call) are not resumes; the first prompt after a restart is one.
    pub fn notify_busy(&mut self, session_id: &str) -> bool {
        let resumed = self.get_state(session_id) != Some(AgentState::Busy);
        self.states.insert(session_id.to_string(), AgentState::Busy);
        self.last_output
            .insert(session_id.to_string(), Instant::now());
        self.idle_since.remove(session_id);
        self.notified.remove(session_id);
        resumed
    }

    pub fn acknowledge(&mut self, session_id: &str) {
        self.notified.remove(session_id);
    }

    pub fn check_silence(&mut self, session_id: &str) -> bool {
        if self.get_state(session_id) != Some(AgentState::Busy) {
            return false;
        }
        // Providers report their own status, even before their session registers.
        if self.is_provider_owned(session_id) {
            return false;
        }
        // Skip sessions without meta (not registered) or with hooks enabled
        match self.meta.get(session_id) {
            None => return false,
            Some(m) if m.hook_enabled => return false,
            _ => {}
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

    pub fn busy_sessions(&self) -> Vec<String> {
        self.states
            .iter()
            .filter(|(_, &s)| s == AgentState::Busy)
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn debounced_sessions(&self) -> Vec<String> {
        self.idle_since.keys().cloned().collect()
    }

    pub fn get_state(&self, session_id: &str) -> Option<AgentState> {
        self.states.get(session_id).copied()
    }

    #[cfg(not(any(test, feature = "test-support")))]
    fn elapsed_since(&self, _session_id: &str, since: Instant) -> Duration {
        since.elapsed()
    }

    #[cfg(any(test, feature = "test-support"))]
    fn elapsed_since(&self, session_id: &str, since: Instant) -> Duration {
        let offset = self
            .time_offset
            .get(session_id)
            .copied()
            .unwrap_or_default();
        since.elapsed() + offset
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn advance_time(&mut self, session_id: &str, duration: Duration) {
        let entry = self.time_offset.entry(session_id.to_string()).or_default();
        *entry += duration;
    }
}

pub type SharedNotifyState = Arc<Mutex<NotifyState>>;

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
            Some("send_prompt") => NotifyEvent::SendPrompt,
            Some("task_lifecycle") => NotifyEvent::TaskLifecycle,
            _ => NotifyEvent::Stop,
        };
        let text = v
            .get("text")
            .and_then(|t| t.as_str())
            .map(|s| s.to_string());
        let lifecycle_batch = v
            .get("batch")
            .cloned()
            .and_then(|batch| serde_json::from_value(batch).ok());
        NotifyMessage {
            session_id,
            event,
            text,
            lifecycle_batch,
        }
    } else {
        NotifyMessage {
            session_id: line.trim().to_string(),
            event: NotifyEvent::Stop,
            text: None,
            lifecycle_batch: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // ─── NotifyState tests ───────────────────────────────────────────────────

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
    fn a_provider_settling_notifies_only_when_a_turn_ran() {
        let mut state = NotifyState::new();
        // Providers report idle when they start or resume a session: not a finished turn.
        assert!(!state.provider_settled("s1"));
        assert_eq!(state.get_state("s1"), Some(AgentState::Idle));
        assert!(state.is_provider_owned("s1"));
        assert!(!state.provider_settled("s1"));

        assert!(state.provider_busy("s1"));
        assert!(!state.provider_busy("s1"));
        assert!(state.provider_settled("s1"));
        assert!(!state.provider_settled("s1"));
    }

    #[test]
    fn a_released_provider_session_stops_being_busy_without_a_notification() {
        let mut state = NotifyState::new();
        assert!(!state.provider_released("s1"));
        state.provider_busy("s1");
        assert!(state.provider_released("s1"));
        assert_eq!(state.get_state("s1"), Some(AgentState::Idle));
        // The turn did not finish, so its next real end still notifies.
        state.provider_busy("s1");
        assert!(state.provider_settled("s1"));
    }

    #[test]
    fn silence_check_skipped_for_provider_owned_sessions() {
        let mut state = NotifyState::new();
        state.register_session("s1", "test", "project", false);
        state.mark_provider_owned("s1");
        state.notify_busy("s1");
        state.advance_time("s1", SILENCE_THRESHOLD + Duration::from_secs(1));
        assert!(!state.check_silence("s1"));
        assert_eq!(state.get_state("s1"), Some(AgentState::Busy));
        assert!(state.is_provider_owned("s1"));
        assert!(!state.is_provider_owned("s2"));
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
        // For hook-enabled sessions, PTY output should NOT cancel a pending debounce.
        // Only an explicit busy hook signal (notify_busy) should cancel it.
        let mut state = NotifyState::new();
        state.register_session("s1", "test", "project", true);
        state.notify_output("s1");
        state.notify_stop_debounced("s1");
        state.advance_time("s1", Duration::from_secs(1));
        // PTY output arrives (terminal rendering trailing bytes) — should NOT cancel debounce
        state.notify_output("s1");
        state.advance_time("s1", Duration::from_secs(1));
        assert!(state.check_debounce("s1"));
    }

    #[test]
    fn busy_hook_is_a_resume_unless_already_busy() {
        let mut state = NotifyState::new();
        state.register_session("s1", "test", "project", true);
        assert!(state.notify_busy("s1"));
        // Repeated busy signals while working (one per tool call).
        assert!(!state.notify_busy("s1"));

        state.notify_stop_debounced("s1");
        state.advance_time("s1", Duration::from_secs(3));
        assert!(state.check_debounce("s1"));
        assert_eq!(state.get_state("s1"), Some(AgentState::Idle));
        assert!(state.notify_busy("s1"));
    }

    #[test]
    fn first_prompt_after_restart_resumes_a_hook_session_despite_redraw_output() {
        let mut state = NotifyState::new();
        state.register_session("s1", "test", "project", true);
        // The attach redraw must not mark it busy; only hooks do.
        state.notify_output("s1");
        assert_ne!(state.get_state("s1"), Some(AgentState::Busy));
        assert!(state.notify_busy("s1"));
    }

    #[test]
    fn debounced_stop_cancelled_by_busy_hook() {
        // An explicit busy hook signal (notify_busy) SHOULD cancel the debounce.
        let mut state = NotifyState::new();
        state.register_session("s1", "test", "project", true);
        state.notify_output("s1");
        state.notify_stop_debounced("s1");
        state.advance_time("s1", Duration::from_secs(1));
        // Busy hook fires (user submitted a new prompt)
        let _ = state.notify_busy("s1");
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

    // ─── Iced integration pattern tests ──────────────────────────────────────

    /// Simulates the iced app's full cycle:
    /// output → Busy → stop → Idle → user input → cleared
    #[test]
    fn full_cycle_output_stop_acknowledge() {
        let state = Arc::new(Mutex::new(NotifyState::new()));
        let mut agent_states: HashMap<String, AgentState> = HashMap::new();
        let sid = "session-1";

        // 1. PTY output arrives → state becomes Busy
        {
            let mut ns = state.lock().unwrap();
            ns.notify_output(sid);
        }
        agent_states.insert(sid.to_string(), AgentState::Busy);
        assert_eq!(agent_states.get(sid), Some(&AgentState::Busy));

        // 2. Stop signal arrives → state becomes Idle
        {
            let fired = state.lock().unwrap().notify_stop(sid);
            assert!(fired);
        }
        agent_states.insert(sid.to_string(), AgentState::Idle);
        assert_eq!(agent_states.get(sid), Some(&AgentState::Idle));

        // 3. User types → state cleared
        {
            state.lock().unwrap().acknowledge(sid);
        }
        agent_states.remove(sid);
        assert_eq!(agent_states.get(sid), None);

        // 4. Next stop should not fire (already acknowledged, no new output)
        {
            let fired = state.lock().unwrap().notify_stop(sid);
            assert!(!fired); // already idle
        }
    }

    /// PLA-189: For hook-enabled sessions, PTY output (user typing echo) while Idle
    /// must NOT transition state to Busy. The notify_output method preserves Idle
    /// state for these sessions, so callers that check state before/after should
    /// not see a transition.
    #[test]
    fn hook_enabled_pty_output_while_idle_does_not_transition_to_busy() {
        let mut state = NotifyState::new();
        state.register_session("s1", "agent-1", "project-a", true);

        // Simulate: agent worked, then stopped → Idle
        let _ = state.notify_busy("s1");
        assert_eq!(state.get_state("s1"), Some(AgentState::Busy));
        let fired = state.notify_stop_immediate("s1");
        assert!(fired);
        assert_eq!(state.get_state("s1"), Some(AgentState::Idle));

        // User types → PTY echo triggers notify_output while Idle.
        // For hook-enabled sessions, state must stay Idle (not transition to Busy).
        let was_idle = state.get_state("s1") != Some(AgentState::Busy);
        state.notify_output("s1");
        let is_now_busy = state.get_state("s1") == Some(AgentState::Busy);

        assert!(was_idle, "session was idle before output");
        assert!(
            !is_now_busy,
            "hook-enabled session must NOT become Busy from PTY echo"
        );
        assert_eq!(state.get_state("s1"), Some(AgentState::Idle));
    }

    /// For non-hook sessions, PTY output while Idle SHOULD transition to Busy
    /// (this is the standard silence-based detection path).
    #[test]
    fn non_hook_pty_output_while_idle_transitions_to_busy() {
        let mut state = NotifyState::new();
        state.register_session("s1", "agent-1", "project-a", false);

        // Agent was busy, then went idle via silence check
        state.notify_output("s1");
        assert_eq!(state.get_state("s1"), Some(AgentState::Busy));
        state.advance_time("s1", Duration::from_secs(10));
        assert!(state.check_silence("s1"));
        assert_eq!(state.get_state("s1"), Some(AgentState::Idle));

        // New PTY output → should transition back to Busy
        let was_idle = state.get_state("s1") != Some(AgentState::Busy);
        state.notify_output("s1");
        let is_now_busy = state.get_state("s1") == Some(AgentState::Busy);

        assert!(was_idle);
        assert!(
            is_now_busy,
            "non-hook session SHOULD become Busy from PTY output"
        );
    }

    /// Simulates boot registration: sessions loaded from DB get registered
    #[test]
    fn session_registration_on_boot() {
        let state = Arc::new(Mutex::new(NotifyState::new()));

        // Simulate boot: register 3 sessions with different hook states
        {
            let mut ns = state.lock().unwrap();
            ns.register_session("s1", "agent-1", "project-a", true);
            ns.register_session("s2", "agent-2", "project-a", false);
            ns.register_session("s3", "agent-3", "project-b", true);
        }

        // Verify meta is populated
        {
            let ns = state.lock().unwrap();
            let m1 = ns.get_meta("s1").unwrap();
            assert_eq!(m1.name, "agent-1");
            assert_eq!(m1.project_name, "project-a");
            assert!(m1.hook_enabled);

            let m2 = ns.get_meta("s2").unwrap();
            assert!(!m2.hook_enabled);
        }

        // Verify silence check respects hook_enabled
        {
            let mut ns = state.lock().unwrap();
            ns.notify_output("s1");
            ns.notify_output("s2");
            ns.advance_time("s1", Duration::from_secs(10));
            ns.advance_time("s2", Duration::from_secs(10));
            assert!(!ns.check_silence("s1")); // hook_enabled, skip
            assert!(ns.check_silence("s2")); // no hook, fires
        }
    }

    // ─── parse_notify_message tests ──────────────────────────────────────────

    #[test]
    fn parse_json_stop_event() {
        let msg = parse_notify_message(r#"{"session_id":"abc123","event":"stop"}"#);
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

    #[test]
    fn parse_bare_session_id_as_stop() {
        let msg = parse_notify_message("abc123");
        assert_eq!(msg.session_id, "abc123");
        assert_eq!(msg.event, NotifyEvent::Stop);
    }

    #[test]
    fn parse_task_lifecycle_message_preserves_batch_without_session_id() {
        let batch = TaskLifecycleBatch::new(
            crate::task_lifecycle::TaskLifecycleOrigin::Cli,
            "project-1",
            "PLA",
            vec![crate::task_lifecycle::TaskLifecycleEvent::StatusChanged {
                task_key: "PLA-1".into(),
                parent_key: None,
                previous_status: "todo".into(),
                new_status: "done".into(),
                cause: crate::task_lifecycle::StatusChangeCause::Direct,
            }],
        );
        let message = serde_json::json!({"event":"task_lifecycle","batch":batch});
        let parsed = parse_notify_message(&message.to_string());

        assert!(matches!(parsed.event, NotifyEvent::TaskLifecycle));
        assert!(parsed.session_id.is_empty());
        assert_eq!(
            parsed.lifecycle_batch.unwrap().project.project_prefix,
            "PLA"
        );
    }
}

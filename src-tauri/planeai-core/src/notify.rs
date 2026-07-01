use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
}

#[derive(Debug, Clone)]
pub struct NotifyMessage {
    pub session_id: String,
    pub event: NotifyEvent,
    pub text: Option<String>,
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
        if hook_enabled && self.get_state(session_id) == Some(AgentState::Idle) {
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
    pub fn notify_busy(&mut self, session_id: &str) {
        self.states.insert(session_id.to_string(), AgentState::Busy);
        self.last_output
            .insert(session_id.to_string(), Instant::now());
        self.idle_since.remove(session_id);
        self.notified.remove(session_id);
    }

    pub fn acknowledge(&mut self, session_id: &str) {
        self.notified.remove(session_id);
    }

    pub fn check_silence(&mut self, session_id: &str) -> bool {
        if self.get_state(session_id) != Some(AgentState::Busy) {
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
            _ => NotifyEvent::Stop,
        };
        let text = v
            .get("text")
            .and_then(|t| t.as_str())
            .map(|s| s.to_string());
        NotifyMessage {
            session_id,
            event,
            text,
        }
    } else {
        NotifyMessage {
            session_id: line.trim().to_string(),
            event: NotifyEvent::Stop,
            text: None,
        }
    }
}

// ─── Hook detection ──────────────────────────────────────────────────────────

/// Case-insensitive lookup in a JSON object for a key.
fn get_ignore_case<'a>(
    obj: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<&'a serde_json::Value> {
    obj.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v)
}

/// Check if a v3 hooks array contains a trigger with the given name (case-insensitive)
/// whose action command contains the planeai notify marker.
fn has_v3_notify_trigger(hooks: &[serde_json::Value], trigger_name: &str) -> bool {
    hooks.iter().any(|h| {
        let trigger = h.get("trigger").and_then(|t| t.as_str()).unwrap_or("");
        let cmd = h
            .get("action")
            .and_then(|a| a.get("command"))
            .and_then(|c| c.as_str())
            .unwrap_or("");
        trigger.eq_ignore_ascii_case(trigger_name) && cmd.contains("planeai-stop-notify")
    })
}

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
    let has_stop = get_ignore_case(hooks, "stop")
        .and_then(|a| a.as_array())
        .is_some_and(|arr| {
            arr.iter().any(|h| {
                h.get("command")
                    .and_then(|c| c.as_str())
                    .is_some_and(|c| c.contains("planeai-stop-notify"))
            })
        });
    let has_prompt = get_ignore_case(hooks, "userPromptSubmit")
        .and_then(|a| a.as_array())
        .is_some_and(|arr| {
            arr.iter().any(|h| {
                h.get("command")
                    .and_then(|c| c.as_str())
                    .is_some_and(|c| c.contains("planeai-stop-notify"))
            })
        });
    has_stop && has_prompt
}

/// Check if the planeai notification hook is installed in the Kiro CLI v3 hooks directory.
///
/// v3 hooks live in `.kiro/hooks/<name>.json` with schema:
/// ```json
/// { "version": "v1", "hooks": [{ "trigger": "Stop", "action": { "type": "command", "command": "..." } }] }
/// ```
pub fn is_kiro_v3_hook_installed_at(hooks_dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(hooks_dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        if !content.contains("planeai-stop-notify") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        let Some(hooks) = v.get("hooks").and_then(|h| h.as_array()) else {
            continue;
        };
        if has_v3_notify_trigger(hooks, "Stop") && has_v3_notify_trigger(hooks, "UserPromptSubmit")
        {
            return true;
        }
    }
    false
}

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

pub fn is_copilot_hook_installed_at(copilot_dir: &Path) -> bool {
    let notify_path = copilot_dir.join("hooks").join("planeai-notify.json");
    let Ok(content) = std::fs::read_to_string(notify_path) else {
        return false;
    };
    content.contains("planeai-stop-notify-copilot")
}

/// Check if the notification hook is installed for a given provider command.
pub fn is_hook_installed_for_provider(command: &str) -> bool {
    let home = std::env::var("HOME").unwrap_or_default();
    if command.contains("kiro") {
        // Check v2 agent config location
        if is_kiro_hook_installed_at(&Path::new(&home).join(".kiro/agents/default.json")) {
            return true;
        }
        // Check v3 hooks directory
        is_kiro_v3_hook_installed_at(&Path::new(&home).join(".kiro/hooks"))
    } else if command.contains("claude") {
        is_claude_hook_installed_at(&Path::new(&home).join(".claude/settings.json"))
    } else if command.contains("copilot") {
        let copilot_dir = std::env::var("COPILOT_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| Path::new(&home).join(".copilot"));
        is_copilot_hook_installed_at(&copilot_dir)
    } else {
        false
    }
}

// ─── Hook installation ───────────────────────────────────────────────────────

/// Install all notification hooks for providers found on this system.
pub fn install_all_hooks(home: &str) -> Result<(), String> {
    let kiro_dir = std::path::Path::new(home).join(".kiro");
    if kiro_dir.exists() {
        install_kiro_hook(home)?;
    }
    let claude_dir = std::path::Path::new(home).join(".claude");
    if claude_dir.exists() {
        install_claude_hook(home)?;
    }
    install_copilot_hook(home)?;
    Ok(())
}

/// Refresh hook script files to the latest bundled version.
///
/// Called on every app startup to ensure script content stays in sync with
/// the app version. Only writes scripts for hooks that are already installed
/// (detected via `is_*_hook_installed_at`). Does not modify agent/settings
/// config files — those are managed by `install_*_hook`.
pub fn refresh_hook_scripts(home: &str) {
    let kiro_config = std::path::Path::new(home).join(".kiro/agents/default.json");
    let kiro_v3_dir = std::path::Path::new(home).join(".kiro/hooks");
    if is_kiro_hook_installed_at(&kiro_config) || is_kiro_v3_hook_installed_at(&kiro_v3_dir) {
        let _ = install_kiro_hook(home);
    }

    let claude_settings = std::path::Path::new(home).join(".claude/settings.json");
    if is_claude_hook_installed_at(&claude_settings) {
        let _ = install_claude_hook(home);
    }

    let copilot_dir = std::env::var("COPILOT_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(format!("{home}/.copilot")));
    if is_copilot_hook_installed_at(&copilot_dir) {
        let _ = install_copilot_hook(home);
    }
}

pub fn install_kiro_hook(home: &str) -> Result<(), String> {
    let hooks_dir = format!("{home}/.kiro/hooks");
    std::fs::create_dir_all(&hooks_dir).map_err(|e| format!("failed to create hooks dir: {e}"))?;

    #[cfg(not(windows))]
    let (script_path, script_content) = (
        format!("{hooks_dir}/planeai-stop-notify.sh"),
        include_str!("../resources/planeai-stop-notify.sh"),
    );
    #[cfg(windows)]
    let (script_path, script_content) = (
        format!("{hooks_dir}/planeai-stop-notify.ps1"),
        include_str!("../resources/planeai-stop-notify.ps1"),
    );

    std::fs::write(&script_path, script_content)
        .map_err(|e| format!("failed to write hook script: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("failed to chmod hook: {e}"))?;
    }

    let agents_dir = format!("{home}/.kiro/agents");
    std::fs::create_dir_all(&agents_dir)
        .map_err(|e| format!("failed to create agents dir: {e}"))?;
    let config_path = format!("{agents_dir}/default.json");

    let mut config: serde_json::Value = if let Ok(content) = std::fs::read_to_string(&config_path) {
        serde_json::from_str(&content).map_err(|e| format!("failed to parse default.json: {e}"))?
    } else {
        serde_json::json!({ "name": "default", "tools": ["*"] })
    };

    #[cfg(not(windows))]
    let hook_command = format!("{hooks_dir}/planeai-stop-notify.sh");
    #[cfg(windows)]
    let hook_command =
        format!("powershell -NoProfile -File \"{hooks_dir}/planeai-stop-notify.ps1\"");

    let hooks = config
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    let hooks_obj = hooks.as_object_mut().unwrap();

    let mut ensure_hook = |event: &str| {
        let arr = hooks_obj
            .entry(event)
            .or_insert_with(|| serde_json::json!([]));
        let arr = arr.as_array_mut().unwrap();
        let already = arr.iter().any(|h| {
            h.get("command")
                .and_then(|c| c.as_str())
                .is_some_and(|c| c.contains("planeai-stop-notify"))
        });
        if !already {
            arr.push(serde_json::json!({ "command": hook_command }));
        }
    };

    ensure_hook("stop");
    ensure_hook("userPromptSubmit");

    let output = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(&config_path, output)
        .map_err(|e| format!("failed to write default.json: {e}"))?;
    Ok(())
}

pub fn install_claude_hook(home: &str) -> Result<(), String> {
    let claude_dir = std::path::PathBuf::from(format!("{home}/.claude"));
    let hooks_dir = claude_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir).map_err(|e| format!("failed to create hooks dir: {e}"))?;

    #[cfg(not(windows))]
    let (script_path, script_content) = (
        hooks_dir.join("planeai-stop-notify-claude.sh"),
        include_str!("../resources/planeai-stop-notify-claude.sh"),
    );
    #[cfg(windows)]
    let (script_path, script_content) = (
        hooks_dir.join("planeai-stop-notify-claude.ps1"),
        include_str!("../resources/planeai-stop-notify-claude.ps1"),
    );

    std::fs::write(&script_path, script_content)
        .map_err(|e| format!("failed to write hook script: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("failed to chmod hook: {e}"))?;
    }

    let script_command = script_path.to_string_lossy().to_string();
    install_claude_hook_at(&claude_dir, &script_command)
}

pub fn install_copilot_hook(home: &str) -> Result<(), String> {
    let copilot_dir = std::env::var("COPILOT_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(format!("{home}/.copilot")));
    let hooks_dir = copilot_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir).map_err(|e| format!("failed to create hooks dir: {e}"))?;

    #[cfg(not(windows))]
    let (bash_path, bash_content) = (
        hooks_dir.join("planeai-stop-notify-copilot.sh"),
        include_str!("../resources/planeai-stop-notify-copilot.sh"),
    );
    #[cfg(not(windows))]
    {
        std::fs::write(&bash_path, bash_content)
            .map_err(|e| format!("failed to write hook script: {e}"))?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bash_path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("failed to chmod hook: {e}"))?;
    }

    #[cfg(windows)]
    let bash_path = hooks_dir.join("planeai-stop-notify-copilot.sh");

    let ps_path = hooks_dir.join("planeai-stop-notify-copilot.ps1");
    let ps_content = include_str!("../resources/planeai-stop-notify-copilot.ps1");
    std::fs::write(&ps_path, ps_content)
        .map_err(|e| format!("failed to write hook script: {e}"))?;

    install_copilot_hook_at(
        &copilot_dir,
        &bash_path.to_string_lossy(),
        &ps_path.to_string_lossy(),
    )
}

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
    fn debounced_stop_cancelled_by_busy_hook() {
        // An explicit busy hook signal (notify_busy) SHOULD cancel the debounce.
        let mut state = NotifyState::new();
        state.register_session("s1", "test", "project", true);
        state.notify_output("s1");
        state.notify_stop_debounced("s1");
        state.advance_time("s1", Duration::from_secs(1));
        // Busy hook fires (user submitted a new prompt)
        state.notify_busy("s1");
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

    // ─── Hook detection tests ────────────────────────────────────────────────

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
    fn detect_kiro_hook_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("default.json");
        std::fs::write(&config_path, r#"{"name":"default"}"#).unwrap();
        assert!(!is_kiro_hook_installed_at(&config_path));
    }

    #[test]
    fn detect_kiro_hook_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_kiro_hook_installed_at(&dir.path().join("nope.json")));
    }

    #[test]
    fn detect_kiro_hook_pascal_case_keys() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("default.json");
        let config = serde_json::json!({
            "name": "default",
            "hooks": {
                "Stop": [{ "command": "/path/to/planeai-stop-notify.sh" }],
                "UserPromptSubmit": [{ "command": "/path/to/planeai-stop-notify.sh" }]
            }
        });
        std::fs::write(&config_path, serde_json::to_string(&config).unwrap()).unwrap();
        assert!(is_kiro_hook_installed_at(&config_path));
    }

    #[test]
    fn detect_kiro_v3_hook_installed() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        let hook = serde_json::json!({
            "version": "v1",
            "hooks": [
                { "name": "planeai-stop", "trigger": "Stop", "action": { "type": "command", "command": "/path/to/planeai-stop-notify.sh" } },
                { "name": "planeai-busy", "trigger": "UserPromptSubmit", "action": { "type": "command", "command": "/path/to/planeai-stop-notify.sh" } }
            ]
        });
        std::fs::write(
            hooks_dir.join("planeai-notify.json"),
            serde_json::to_string(&hook).unwrap(),
        )
        .unwrap();
        assert!(is_kiro_v3_hook_installed_at(&hooks_dir));
    }

    #[test]
    fn detect_kiro_v3_hook_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_kiro_v3_hook_installed_at(&dir.path().join("hooks")));
    }

    #[test]
    fn detect_kiro_v3_hook_missing_trigger() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        let hook = serde_json::json!({
            "version": "v1",
            "hooks": [
                { "name": "planeai-stop", "trigger": "Stop", "action": { "type": "command", "command": "/path/to/planeai-stop-notify.sh" } }
            ]
        });
        std::fs::write(
            hooks_dir.join("planeai-notify.json"),
            serde_json::to_string(&hook).unwrap(),
        )
        .unwrap();
        assert!(!is_kiro_v3_hook_installed_at(&hooks_dir));
    }

    #[test]
    fn detect_claude_hook_installed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"/path/to/planeai-stop-notify-claude.sh"}]}],"StopFailure":[{}],"Notification":[{}],"UserPromptSubmit":[{}]}}"#).unwrap();
        assert!(is_claude_hook_installed_at(&path));
    }

    #[test]
    fn detect_claude_hook_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"hooks":{}}"#).unwrap();
        assert!(!is_claude_hook_installed_at(&path));
    }

    #[test]
    fn detect_copilot_hook_installed() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        std::fs::write(
            hooks_dir.join("planeai-notify.json"),
            r#"{"hooks":{"agentStop":[{"bash":"planeai-stop-notify-copilot.sh stop"}]}}"#,
        )
        .unwrap();
        assert!(is_copilot_hook_installed_at(dir.path()));
    }

    #[test]
    fn detect_copilot_hook_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_copilot_hook_installed_at(dir.path()));
    }

    // ─── Hook install tests ──────────────────────────────────────────────────

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
    }

    #[test]
    fn install_copilot_hook_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        install_copilot_hook_at(dir.path(), "/path/s.sh", "/path/s.ps1").unwrap();
        install_copilot_hook_at(dir.path(), "/path/s.sh", "/path/s.ps1").unwrap();
        let content =
            std::fs::read_to_string(dir.path().join("hooks/planeai-notify.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(v["hooks"]["agentStop"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn install_claude_hook_creates_correct_structure() {
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        install_claude_hook_at(&claude_dir, "/path/planeai-stop-notify-claude.sh").unwrap();
        let settings: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(claude_dir.join("settings.json")).unwrap(),
        )
        .unwrap();
        assert!(settings["hooks"]["Stop"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .contains("planeai-stop-notify"));
    }

    #[test]
    fn install_claude_hook_merges_with_existing_settings() {
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::write(
            claude_dir.join("settings.json"),
            r#"{"permissions":{"allow":["Read"]}}"#,
        )
        .unwrap();
        install_claude_hook_at(&claude_dir, "/path/planeai-stop-notify-claude.sh").unwrap();
        let settings: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(claude_dir.join("settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(settings["permissions"]["allow"][0], "Read");
        assert!(settings["hooks"]["Stop"].is_array());
    }
}

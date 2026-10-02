//! Plugin-provided session runtimes (ADR-0013).
//!
//! The host owns the session row, worktree, lifecycle and status display; the
//! provider plugin owns the agent process and its conversation. Provider
//! notifications are routed here by the sidecar stdout reader.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::db;
use crate::plugins::{PluginProvider, PluginRuntimeSupervisor, ProviderFeature};

use crate::session_ops::PROVIDER_BACKEND;
/// Frontend event carrying opaque provider session events to the mounted UI.
pub const SESSION_EVENT: &str = "plugin-provider-session-event";

const EVENT_NOTIFICATION: &str = "host.session.event";
/// Prompts must fit one JSON-RPC frame with room for the envelope.
const MAX_PROMPT_BYTES: usize = 48 * 1024;
const STATUS_NOTIFICATION: &str = "host.session.status";

/// Provider keys are `<plugin id>:<provider id>`; plugin and provider ids never contain `:`.
pub fn parse_provider_key(key: &str) -> Option<(&str, &str)> {
    let (plugin_id, provider_id) = key.split_once(':')?;
    (!plugin_id.is_empty() && !provider_id.is_empty()).then_some((plugin_id, provider_id))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Binding {
    plugin_id: String,
    provider_id: String,
    /// Runtime instance that accepted start/resume; a restarted sidecar must resume again.
    instance: u64,
}

/// Which plugin currently drives each provider session.
#[derive(Default)]
pub struct ProviderSessions {
    bindings: Mutex<HashMap<String, Binding>>,
    /// Serializes `ensure` per session so concurrent first uses resume it once.
    ensuring: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl ProviderSessions {
    fn ensure_lock(&self, session_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.ensuring
            .lock()
            .unwrap()
            .entry(session_id.to_string())
            .or_default()
            .clone()
    }

    fn bind(&self, session_id: &str, binding: Binding) {
        self.bindings
            .lock()
            .unwrap()
            .insert(session_id.to_string(), binding);
    }

    fn unbind(&self, session_id: &str) -> Option<Binding> {
        self.ensuring.lock().unwrap().remove(session_id);
        self.bindings.lock().unwrap().remove(session_id)
    }

    fn get(&self, session_id: &str) -> Option<Binding> {
        self.bindings.lock().unwrap().get(session_id).cloned()
    }

    fn session_ids(&self) -> Vec<String> {
        self.bindings.lock().unwrap().keys().cloned().collect()
    }

    fn is_owned_by(&self, session_id: &str, plugin_id: &str) -> bool {
        self.get(session_id)
            .is_some_and(|binding| binding.plugin_id == plugin_id)
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSessionStatus {
    Busy,
    Idle,
    NeedsAttention,
    Exited,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionEventParams {
    session_id: String,
    seq: u64,
    payload: Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionStatusParams {
    session_id: String,
    status: ProviderSessionStatus,
}

/// Why a provider session ended; sent to the plugin with `provider.session.stop`.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    Archive,
    Destroy,
    Exit,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProviderSessionEvent {
    pub plugin_id: String,
    pub session_id: String,
    pub seq: u64,
    pub payload: Value,
}

/// Routes provider notifications for one sidecar. Lives on the stdout reader task.
pub struct NotificationSink {
    app: AppHandle,
    plugin_id: String,
    sessions: Arc<ProviderSessions>,
}

impl NotificationSink {
    pub fn new(app: AppHandle, plugin_id: &str, sessions: Arc<ProviderSessions>) -> Self {
        Self {
            app,
            plugin_id: plugin_id.to_string(),
            sessions,
        }
    }

    /// Returns true when the frame was a provider notification and has been consumed.
    /// Anything else stays on the request path, which keeps rejecting stray notifications.
    pub fn try_handle(&self, frame: &str) -> bool {
        let Some((method, params)) = provider_notification(frame) else {
            return false;
        };
        if let Err(error) = self.route(&method, params) {
            tracing::warn!(plugin_id = %self.plugin_id, %method, %error, "dropped provider notification");
        }
        true
    }

    fn route(&self, method: &str, params: Value) -> Result<(), String> {
        match method {
            EVENT_NOTIFICATION => {
                let params: SessionEventParams = serde_json::from_value(params)
                    .map_err(|error| format!("invalid session event: {error}"))?;
                self.require_owner(&params.session_id)?;
                self.app
                    .emit(
                        SESSION_EVENT,
                        ProviderSessionEvent {
                            plugin_id: self.plugin_id.clone(),
                            session_id: params.session_id,
                            seq: params.seq,
                            payload: params.payload,
                        },
                    )
                    .map_err(|error| format!("failed to emit session event: {error}"))
            }
            STATUS_NOTIFICATION => {
                let params: SessionStatusParams = serde_json::from_value(params)
                    .map_err(|error| format!("invalid session status: {error}"))?;
                self.require_owner(&params.session_id)?;
                if params.status == ProviderSessionStatus::Exited {
                    self.sessions.unbind(&params.session_id);
                }
                crate::notify::apply_provider_status(&self.app, &params.session_id, params.status);
                Ok(())
            }
            _ => unreachable!("provider_notification only yields provider methods"),
        }
    }

    fn require_owner(&self, session_id: &str) -> Result<(), String> {
        if self.sessions.is_owned_by(session_id, &self.plugin_id) {
            Ok(())
        } else {
            Err(format!("session {session_id} is not driven by this plugin"))
        }
    }
}

fn provider_notification(frame: &str) -> Option<(String, Value)> {
    let Value::Object(mut object) = serde_json::from_str::<Value>(frame.trim_end()).ok()? else {
        return None;
    };
    if object.contains_key("id") || object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return None;
    }
    let method = object.get("method")?.as_str()?.to_string();
    if !matches!(method.as_str(), EVENT_NOTIFICATION | STATUS_NOTIFICATION) {
        return None;
    }
    Some((method, object.remove("params").unwrap_or(Value::Null)))
}

/// Everything a provider needs to run one session in its worktree.
pub struct SessionRuntimeContext {
    pub session_id: String,
    pub provider_key: String,
    pub cwd: String,
    pub yolo: bool,
    pub extra_path_dirs: Vec<String>,
}

impl SessionRuntimeContext {
    pub fn for_session(
        conn: &rusqlite::Connection,
        session: &db::Session,
        extra_path_dirs: Vec<String>,
    ) -> Result<Self, String> {
        if session.backend != PROVIDER_BACKEND {
            return Err(format!("session {} is not provider-backed", session.id));
        }
        let provider_key = session
            .provider
            .clone()
            .ok_or_else(|| format!("session {} has no provider", session.id))?;
        let cwd = match &session.worktree_path {
            Some(path) => path.clone(),
            None => {
                db::get_project(conn, &session.project_id)
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| format!("project not found for session {}", session.id))?
                    .path
            }
        };
        Ok(Self {
            session_id: session.id.clone(),
            provider_key,
            cwd,
            yolo: session.auto_approve,
            extra_path_dirs,
        })
    }

    fn env(&self) -> Value {
        serde_json::json!({
            "PLANEAI_SESSION_ID": self.session_id,
            "PLANEAI_SOCKET": planeai_ipc::address(
                planeai_ipc::Channel::Notify,
                &planeai_paths::app_data_dir(),
            ),
            "PATH": planeai_core::command::augmented_path(&self.extra_path_dirs),
        })
    }

    fn params(&self, provider: &PluginProvider) -> serde_json::Map<String, Value> {
        let mut params = serde_json::Map::new();
        params.insert("session_id".into(), self.session_id.clone().into());
        params.insert("provider_id".into(), provider.id.clone().into());
        params.insert("cwd".into(), self.cwd.clone().into());
        params.insert("env".into(), self.env());
        params.insert("yolo".into(), self.yolo.into());
        params
    }
}

pub struct ResolvedProvider {
    pub plugin_id: String,
    pub provider: PluginProvider,
}

/// Resolve a provider key to a running plugin that declares it.
pub async fn resolve(
    supervisor: &PluginRuntimeSupervisor,
    provider_key: &str,
) -> Result<ResolvedProvider, String> {
    let (plugin_id, provider_id) = parse_provider_key(provider_key)
        .ok_or_else(|| format!("Unknown provider: {provider_key}"))?;
    let provider = supervisor.running_provider(plugin_id, provider_id).await?;
    Ok(ResolvedProvider {
        plugin_id: plugin_id.to_string(),
        provider,
    })
}

/// Start a brand-new provider session. The binding exists before the request so
/// notifications emitted while starting are accepted.
pub async fn start(
    supervisor: &PluginRuntimeSupervisor,
    context: &SessionRuntimeContext,
    initial_prompt: Option<&str>,
) -> Result<(), String> {
    let resolved = resolve(supervisor, &context.provider_key).await?;
    if context.yolo && !resolved.provider.supports(ProviderFeature::Yolo) {
        return Err(format!(
            "{} does not support auto-approve",
            resolved.provider.label
        ));
    }
    let mut params = context.params(&resolved.provider);
    if let Some(prompt) = initial_prompt.filter(|prompt| !prompt.trim().is_empty()) {
        check_prompt_size(prompt)?;
        params.insert("initial_prompt".into(), prompt.into());
    }
    let started = bind_and_request(
        supervisor,
        &context.session_id,
        &resolved,
        "provider.session.start",
        params,
    )
    .await;
    if started.is_err() {
        // A start cancelled by its deadline may still be running in the sidecar,
        // and the launch rollback is about to remove its worktree.
        let stop =
            serde_json::json!({ "session_id": context.session_id, "reason": StopReason::Destroy });
        let _ = supervisor
            .provider_request(&resolved.plugin_id, "provider.session.stop", stop)
            .await;
    }
    started
}

fn check_prompt_size(text: &str) -> Result<(), String> {
    if text.len() > MAX_PROMPT_BYTES {
        return Err(format!(
            "The message is too long ({} KB); the limit is {} KB.",
            text.len() / 1024,
            MAX_PROMPT_BYTES / 1024
        ));
    }
    Ok(())
}

/// Make sure the current sidecar instance drives this session, resuming it after
/// an app or plugin restart.
pub async fn ensure(
    supervisor: &PluginRuntimeSupervisor,
    context: &SessionRuntimeContext,
) -> Result<(), String> {
    let lock = supervisor
        .provider_sessions()
        .ensure_lock(&context.session_id);
    let _ensuring = lock.lock().await;
    let resolved = resolve(supervisor, &context.provider_key).await?;
    let instance = supervisor.running_instance(&resolved.plugin_id).await;
    if let Some(binding) = supervisor.provider_sessions().get(&context.session_id) {
        if binding.plugin_id == resolved.plugin_id && Some(binding.instance) == instance {
            return Ok(());
        }
    }
    let params = context.params(&resolved.provider);
    bind_and_request(
        supervisor,
        &context.session_id,
        &resolved,
        "provider.session.resume",
        params,
    )
    .await
}

async fn bind_and_request(
    supervisor: &PluginRuntimeSupervisor,
    session_id: &str,
    resolved: &ResolvedProvider,
    method: &str,
    params: serde_json::Map<String, Value>,
) -> Result<(), String> {
    let instance = supervisor
        .running_instance(&resolved.plugin_id)
        .await
        .ok_or_else(|| format!("plugin {} is not running", resolved.plugin_id))?;
    let sessions = supervisor.provider_sessions();
    sessions.bind(
        session_id,
        Binding {
            plugin_id: resolved.plugin_id.clone(),
            provider_id: resolved.provider.id.clone(),
            instance,
        },
    );
    let result = supervisor
        .provider_request(&resolved.plugin_id, method, Value::Object(params))
        .await;
    if result.is_err() {
        sessions.unbind(session_id);
    }
    result.map(|_| ())
}

/// Deliver user or automation input to a provider session.
async fn send(
    supervisor: &PluginRuntimeSupervisor,
    context: &SessionRuntimeContext,
    text: &str,
) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("prompt text must not be empty".to_string());
    }
    check_prompt_size(text)?;
    ensure(supervisor, context).await?;
    request_bound(
        supervisor,
        &context.session_id,
        "provider.session.send",
        serde_json::json!({ "session_id": context.session_id, "text": text }),
    )
    .await
}

async fn interrupt(
    supervisor: &PluginRuntimeSupervisor,
    context: &SessionRuntimeContext,
) -> Result<(), String> {
    ensure(supervisor, context).await?;
    request_bound(
        supervisor,
        &context.session_id,
        "provider.session.interrupt",
        serde_json::json!({ "session_id": context.session_id }),
    )
    .await
}

async fn request_bound(
    supervisor: &PluginRuntimeSupervisor,
    session_id: &str,
    method: &str,
    params: Value,
) -> Result<(), String> {
    let binding = supervisor
        .provider_sessions()
        .get(session_id)
        .ok_or_else(|| format!("session {session_id} has no running provider"))?;
    supervisor
        .provider_request(&binding.plugin_id, method, params)
        .await
        .map(|_| ())
}

#[derive(Debug, Deserialize)]
struct HandoffResponse {
    argv: Vec<String>,
}

/// Hand a session's conversation to its agent's own TUI. The provider detaches
/// and returns the command that continues the conversation in a terminal.
async fn handoff(
    supervisor: &PluginRuntimeSupervisor,
    context: &SessionRuntimeContext,
) -> Result<Vec<String>, String> {
    let resolved = resolve(supervisor, &context.provider_key).await?;
    if !resolved.provider.supports(ProviderFeature::Handoff) {
        return Err(format!(
            "{} cannot continue in a terminal",
            resolved.provider.label
        ));
    }
    ensure(supervisor, context).await?;
    let value = supervisor
        .provider_request(
            &resolved.plugin_id,
            "provider.session.handoff",
            serde_json::json!({ "session_id": context.session_id }),
        )
        .await?;
    let response: HandoffResponse = serde_json::from_value(value)
        .map_err(|error| format!("invalid provider handoff: {error}"))?;
    validate_handoff_argv(response.argv)
}

fn validate_handoff_argv(argv: Vec<String>) -> Result<Vec<String>, String> {
    if argv.is_empty()
        || argv.len() > 64
        || argv.iter().any(|arg| arg.is_empty() || arg.contains('\0'))
    {
        return Err(
            "provider handoff must return a nonempty argv of nonempty arguments".to_string(),
        );
    }
    Ok(argv)
}

/// The terminal that continued a session closed; the provider drives it again.
pub async fn handback(
    app: &AppHandle,
    supervisor: &PluginRuntimeSupervisor,
    session_id: &str,
) -> Result<(), String> {
    // A sidecar restarted during the handoff restores the handoff from its transcript,
    // so it must be resumed to hear that the terminal closed. Ended sessions need nothing.
    let Ok(context) = load_context(app, session_id).await else {
        return Ok(());
    };
    ensure(supervisor, &context).await?;
    request_bound(
        supervisor,
        session_id,
        "provider.session.handback",
        serde_json::json!({ "session_id": session_id }),
    )
    .await
}

/// Stop a provider session that left the active state. Sessions the current
/// sidecar never resumed have nothing running and need no request.
pub async fn stop(supervisor: &PluginRuntimeSupervisor, session_id: &str, reason: StopReason) {
    let Some(binding) = supervisor.provider_sessions().unbind(session_id) else {
        return;
    };
    if supervisor.running_instance(&binding.plugin_id).await != Some(binding.instance) {
        return;
    }
    let params = serde_json::json!({ "session_id": session_id, "reason": reason });
    match supervisor
        .provider_request(&binding.plugin_id, "provider.session.stop", params)
        .await
    {
        Ok(_) => {
            tracing::info!(session_id, plugin_id = %binding.plugin_id, ?reason, "stopped provider session")
        }
        Err(error) => {
            tracing::warn!(session_id, plugin_id = %binding.plugin_id, provider_id = %binding.provider_id, %error, "provider session stop failed")
        }
    }
}

/// Lifecycle statuses that end a provider session, mapped to the stop reason.
pub fn stop_reason(status: &str) -> Option<StopReason> {
    match status {
        "archived" => Some(StopReason::Archive),
        "destroyed" => Some(StopReason::Destroy),
        "exited" => Some(StopReason::Exit),
        _ => None,
    }
}

/// The stop reason for a bound provider session given its stored status, if it ended.
fn ended_reason(status: Option<&str>) -> Option<StopReason> {
    match status {
        None => Some(StopReason::Destroy),
        Some(status) => stop_reason(status),
    }
}

/// Stop provider sessions whose rows ended outside the GUI (CLI archive or delete,
/// task completion). Those paths only notify that sessions changed.
pub async fn reconcile(app: &AppHandle, supervisor: &PluginRuntimeSupervisor) {
    let bound = supervisor.provider_sessions().session_ids();
    if bound.is_empty() {
        return;
    }
    let db = app.state::<crate::state::DbState>().0.clone();
    let ended = crate::commands::blocking(move || {
        let conn = db.lock().map_err(|error| error.to_string())?;
        let mut ended = Vec::new();
        for session_id in bound {
            let status = db::get_session(&conn, &session_id)
                .map_err(|error| error.to_string())?
                .map(|session| session.status);
            if let Some(reason) = ended_reason(status.as_deref()) {
                ended.push((session_id, reason));
            }
        }
        Ok(ended)
    })
    .await;
    match ended {
        Ok(ended) => {
            for (session_id, reason) in ended {
                stop(supervisor, &session_id, reason).await;
            }
        }
        Err(error) => tracing::warn!(%error, "provider session reconciliation failed"),
    }
}

/// Resume a stored session if needed; called when its UI mounts.
pub async fn ensure_session(
    app: &AppHandle,
    supervisor: &PluginRuntimeSupervisor,
    session_id: &str,
) -> Result<(), String> {
    ensure(supervisor, &load_context(app, session_id).await?).await
}

pub async fn send_to_session(
    app: &AppHandle,
    supervisor: &PluginRuntimeSupervisor,
    session_id: &str,
    text: &str,
) -> Result<(), String> {
    send(supervisor, &load_context(app, session_id).await?, text).await
}

pub async fn interrupt_session(
    app: &AppHandle,
    supervisor: &PluginRuntimeSupervisor,
    session_id: &str,
) -> Result<(), String> {
    interrupt(supervisor, &load_context(app, session_id).await?).await
}

pub async fn handoff_session(
    app: &AppHandle,
    supervisor: &PluginRuntimeSupervisor,
    session_id: &str,
) -> Result<Vec<String>, String> {
    handoff(supervisor, &load_context(app, session_id).await?).await
}

/// Look up a session and its runtime context in one blocking DB read.
pub async fn load_context(
    app: &AppHandle,
    session_id: &str,
) -> Result<SessionRuntimeContext, String> {
    let db = app.state::<crate::state::DbState>().0.clone();
    let extra_path_dirs = crate::plugins::configured_extra_path_dirs(app);
    let session_id = session_id.to_string();
    crate::commands::blocking(move || {
        let conn = db.lock().map_err(|error| error.to_string())?;
        let session = db::get_session(&conn, &session_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("session not found: {session_id}"))?;
        if session.status != "active" {
            return Err(format!(
                "session is not active (status: {})",
                session.status
            ));
        }
        SessionRuntimeContext::for_session(&conn, &session, extra_path_dirs)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_keys_split_plugin_and_provider() {
        assert_eq!(
            parse_provider_key("claude-headless:claude"),
            Some(("claude-headless", "claude"))
        );
        assert_eq!(parse_provider_key("claude"), None);
        assert_eq!(parse_provider_key(":claude"), None);
        assert_eq!(parse_provider_key("plugin:"), None);
    }

    #[test]
    fn only_provider_notifications_are_consumed() {
        let event = r#"{"jsonrpc":"2.0","method":"host.session.event","params":{"session_id":"s","seq":1,"payload":{}}}"#;
        assert_eq!(
            provider_notification(event).map(|(method, _)| method),
            Some(EVENT_NOTIFICATION.to_string())
        );
        let status = r#"{"jsonrpc":"2.0","method":"host.session.status","params":{}}"#;
        assert!(provider_notification(status).is_some());
        // Callbacks carry an id and stay on the request path.
        let callback = r#"{"jsonrpc":"2.0","id":7,"method":"host.session.event","params":{}}"#;
        assert!(provider_notification(callback).is_none());
        let other = r#"{"jsonrpc":"2.0","method":"host.sessions.prompt","params":{}}"#;
        assert!(provider_notification(other).is_none());
        let response = r#"{"jsonrpc":"2.0","id":1,"result":{}}"#;
        assert!(provider_notification(response).is_none());
    }

    #[test]
    fn notifications_validate_strictly() {
        assert!(serde_json::from_value::<SessionEventParams>(
            serde_json::json!({ "session_id": "s", "seq": 1, "payload": null, "extra": 1 })
        )
        .is_err());
        assert!(serde_json::from_value::<SessionStatusParams>(
            serde_json::json!({ "session_id": "s", "status": "thinking" })
        )
        .is_err());
        let status: SessionStatusParams = serde_json::from_value(
            serde_json::json!({ "session_id": "s", "status": "needs_attention" }),
        )
        .unwrap();
        assert_eq!(status.status, ProviderSessionStatus::NeedsAttention);
    }

    #[test]
    fn bindings_track_ownership() {
        let sessions = ProviderSessions::default();
        sessions.bind(
            "s1",
            Binding {
                plugin_id: "a".into(),
                provider_id: "chat".into(),
                instance: 1,
            },
        );
        assert!(sessions.is_owned_by("s1", "a"));
        assert!(!sessions.is_owned_by("s1", "b"));
        assert!(!sessions.is_owned_by("s2", "a"));
        assert!(sessions.unbind("s1").is_some());
        assert!(!sessions.is_owned_by("s1", "a"));
    }

    #[test]
    fn reconciliation_stops_sessions_that_ended_or_vanished() {
        assert_eq!(ended_reason(Some("active")), None);
        assert_eq!(ended_reason(Some("archived")), Some(StopReason::Archive));
        assert_eq!(ended_reason(Some("destroyed")), Some(StopReason::Destroy));
        assert_eq!(ended_reason(None), Some(StopReason::Destroy));
        assert_eq!(
            serde_json::to_value(StopReason::Archive).unwrap(),
            "archive"
        );
    }

    #[test]
    fn handoff_argv_must_be_runnable() {
        assert_eq!(
            validate_handoff_argv(vec![
                "/usr/local/bin/claude".into(),
                "--resume".into(),
                "s1".into()
            ]),
            Ok(vec![
                "/usr/local/bin/claude".into(),
                "--resume".into(),
                "s1".into()
            ])
        );
        assert!(validate_handoff_argv(vec![]).is_err());
        assert!(validate_handoff_argv(vec!["claude".into(), String::new()]).is_err());
        assert!(validate_handoff_argv(vec!["claude\0".into()]).is_err());
    }

    #[test]
    fn prompts_must_fit_one_frame() {
        assert!(check_prompt_size(&"x".repeat(MAX_PROMPT_BYTES)).is_ok());
        assert!(check_prompt_size(&"x".repeat(MAX_PROMPT_BYTES + 1))
            .unwrap_err()
            .contains("too long"));
    }

    #[test]
    fn terminal_lifecycle_statuses_stop_provider_sessions() {
        assert_eq!(stop_reason("archived"), Some(StopReason::Archive));
        assert_eq!(stop_reason("destroyed"), Some(StopReason::Destroy));
        assert_eq!(stop_reason("exited"), Some(StopReason::Exit));
        assert_eq!(stop_reason("active"), None);
    }
}

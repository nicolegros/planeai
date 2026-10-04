//! Plugin-provided session runtimes (ADR-0013).
//!
//! The host owns the session row, worktree, lifecycle and status display; the
//! provider plugin owns the agent process and its conversation. Provider
//! notifications are routed here by the sidecar stdout reader.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::db;
use crate::plugins::{PluginProvider, PluginRuntimeSupervisor, ProviderFeature};
use crate::session_ops::PLUGIN_BACKEND;
use planeai_plugin_contract::provider::{
    self as protocol, validate_handoff_argv, HandoffResponse, SessionEventParams,
    SessionStatusParams, EVENT_NOTIFICATION, STATUS_NOTIFICATION,
};
pub use planeai_plugin_contract::provider::{
    check_prompt_size, parse_provider_key, ProviderSessionStatus, StopReason,
};

/// Frontend event carrying opaque provider session events to the mounted UI.
const SESSION_EVENT: &str = "plugin-provider-session-event";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Binding {
    plugin_id: String,
    provider_id: String,
    /// Runtime instance that accepted start/resume; a restarted sidecar must resume again.
    instance: u64,
}

#[derive(Default)]
struct State {
    bindings: HashMap<String, Binding>,
    /// Sessions whose start is in flight: their row does not exist yet, which
    /// reconciliation would otherwise read as a deleted session.
    launching: HashSet<String>,
    /// Sessions continuing in a terminal: their provider is detached until the handback.
    handed_off: HashSet<String>,
}

/// Which plugin currently drives each provider session.
#[derive(Default)]
pub struct ProviderSessions {
    state: Mutex<State>,
    /// Serializes `ensure` and `stop` per session, so concurrent first uses resume it once
    /// and a session stopped while resuming is never left running.
    locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl ProviderSessions {
    fn session_lock(&self, session_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.locks
            .lock()
            .unwrap()
            .entry(session_id.to_string())
            .or_default()
            .clone()
    }

    /// Drops a stopped session's lock unless another caller holds or awaits it.
    fn forget_lock(&self, session_id: &str, lock: &Arc<tokio::sync::Mutex<()>>) {
        let mut locks = self.locks.lock().unwrap();
        // One reference is the map's, the other the caller's.
        if Arc::strong_count(lock) == 2 {
            locks.remove(session_id);
        }
    }

    fn bind(&self, session_id: &str, binding: Binding) {
        self.state
            .lock()
            .unwrap()
            .bindings
            .insert(session_id.to_string(), binding);
    }

    fn unbind(&self, session_id: &str) -> Option<Binding> {
        self.state.lock().unwrap().bindings.remove(session_id)
    }

    fn get(&self, session_id: &str) -> Option<Binding> {
        self.state.lock().unwrap().bindings.get(session_id).cloned()
    }

    fn set_handed_off(&self, session_id: &str, handed_off: bool) {
        let mut state = self.state.lock().unwrap();
        if handed_off {
            state.handed_off.insert(session_id.to_string());
        } else {
            state.handed_off.remove(session_id);
        }
    }

    fn is_handed_off(&self, session_id: &str) -> bool {
        self.state.lock().unwrap().handed_off.contains(session_id)
    }

    fn begin_launch(&self, session_id: &str) {
        self.state
            .lock()
            .unwrap()
            .launching
            .insert(session_id.to_string());
    }

    /// The launch committed the session's row or was abandoned.
    fn end_launch(&self, session_id: &str) {
        self.state.lock().unwrap().launching.remove(session_id);
    }

    /// Bound sessions whose rows exist, which reconciliation may check.
    fn launched_session_ids(&self) -> Vec<String> {
        let state = self.state.lock().unwrap();
        state
            .bindings
            .keys()
            .filter(|session_id| !state.launching.contains(*session_id))
            .cloned()
            .collect()
    }

    /// Whether this sidecar instance of the plugin currently drives the session.
    fn is_driven_by(&self, session_id: &str, plugin_id: &str, instance: u64) -> bool {
        self.get(session_id)
            .is_some_and(|binding| binding.plugin_id == plugin_id && binding.instance == instance)
    }

    /// Unbinds the sessions a sidecar instance drove, returning their ids.
    fn unbind_instance(&self, plugin_id: &str, instance: u64) -> Vec<String> {
        let mut state = self.state.lock().unwrap();
        let ended: Vec<String> = state
            .bindings
            .iter()
            .filter(|(_, binding)| binding.plugin_id == plugin_id && binding.instance == instance)
            .map(|(session_id, _)| session_id.clone())
            .collect();
        for session_id in &ended {
            state.bindings.remove(session_id);
        }
        ended
    }
}

/// Where provider session status and events go: the app's notify state and UI, or a test's record.
pub trait ProviderStatus: Send + Sync {
    /// The provider reports the session's status; hook and PTY signals stop applying.
    fn mark_owned(&self, session_id: &str);
    /// The session never started, so it will never report status.
    fn release_owned(&self, session_id: &str);
    fn apply(&self, session_id: &str, status: ProviderSessionStatus);
    /// Nothing will report the end of a running turn, so the session stops showing as busy.
    fn release(&self, session_id: &str);
    fn emit_event(&self, event: ProviderSessionEvent) -> Result<(), String>;
}

impl ProviderStatus for AppHandle {
    fn mark_owned(&self, session_id: &str) {
        let notify = self.state::<crate::state::NotifyHandle>().0.clone();
        notify.lock().unwrap().mark_provider_owned(session_id);
    }

    fn release_owned(&self, session_id: &str) {
        let notify = self.state::<crate::state::NotifyHandle>().0.clone();
        notify.lock().unwrap().release_provider_owned(session_id);
    }

    fn apply(&self, session_id: &str, status: ProviderSessionStatus) {
        crate::notify::apply_provider_status(self, session_id, status);
    }

    fn release(&self, session_id: &str) {
        crate::notify::release_provider_status(self, session_id);
    }

    fn emit_event(&self, event: ProviderSessionEvent) -> Result<(), String> {
        self.emit(SESSION_EVENT, event)
            .map_err(|error| format!("failed to emit session event: {error}"))
    }
}

/// A plugin's sidecar instance is stopping or died. Its sessions are unbound first, so any
/// status it still sends, such as `exited` from its own shutdown, is dropped; a turn it was
/// running stops showing as busy. The next use resumes each session on a new instance.
pub fn runtime_stopped(
    status: &impl ProviderStatus,
    sessions: &ProviderSessions,
    plugin_id: &str,
    instance: u64,
) {
    for session_id in sessions.unbind_instance(plugin_id, instance) {
        status.release(&session_id);
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProviderSessionEvent {
    pub plugin_id: String,
    pub session_id: String,
    pub seq: u64,
    pub payload: Value,
}

/// Routes provider notifications for one sidecar. Lives on the stdout reader task.
pub struct NotificationSink<S: ProviderStatus = AppHandle> {
    status: S,
    plugin_id: String,
    /// The sidecar instance whose stdout this reads; a previous instance's sessions are not its own.
    instance: u64,
    sessions: Arc<ProviderSessions>,
}

impl<S: ProviderStatus> NotificationSink<S> {
    pub fn new(status: S, plugin_id: &str, instance: u64, sessions: Arc<ProviderSessions>) -> Self {
        Self {
            status,
            plugin_id: plugin_id.to_string(),
            instance,
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
                self.status.emit_event(ProviderSessionEvent {
                    plugin_id: self.plugin_id.clone(),
                    session_id: params.session_id,
                    seq: params.seq,
                    payload: params.payload,
                })
            }
            STATUS_NOTIFICATION => {
                let params: SessionStatusParams = serde_json::from_value(params)
                    .map_err(|error| format!("invalid session status: {error}"))?;
                self.require_owner(&params.session_id)?;
                if params.status == ProviderSessionStatus::Exited {
                    self.sessions.unbind(&params.session_id);
                }
                self.status.apply(&params.session_id, params.status);
                // Stopped while this applied: its stop released the session before it was busy.
                if params.status == ProviderSessionStatus::Busy
                    && !self.sessions.is_driven_by(
                        &params.session_id,
                        &self.plugin_id,
                        self.instance,
                    )
                {
                    self.status.release(&params.session_id);
                }
                Ok(())
            }
            _ => unreachable!("provider_notification only yields provider methods"),
        }
    }

    fn require_owner(&self, session_id: &str) -> Result<(), String> {
        if self
            .sessions
            .is_driven_by(session_id, &self.plugin_id, self.instance)
        {
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
    let method = protocol::provider_notification_method(&object)?;
    Some((
        method.to_string(),
        object.remove("params").unwrap_or(Value::Null),
    ))
}

/// Everything a provider needs to run one session in its worktree.
#[derive(Debug)]
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
        if session.backend != PLUGIN_BACKEND {
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

#[derive(Debug)]
pub struct ResolvedProvider {
    pub plugin_id: String,
    pub provider: PluginProvider,
}

/// Resolve a provider key to a running plugin that declares it.
pub async fn resolve<R: ProviderRuntime>(
    runtime: &R,
    provider_key: &str,
) -> Result<ResolvedProvider, String> {
    let (plugin_id, provider_id) = parse_provider_key(provider_key)
        .ok_or_else(|| format!("Unknown provider: {provider_key}"))?;
    let provider = runtime.running_provider(plugin_id, provider_id).await?;
    Ok(ResolvedProvider {
        plugin_id: plugin_id.to_string(),
        provider,
    })
}

/// Start a brand-new provider session. The binding exists before the request so
/// notifications emitted while starting are accepted.
async fn start<R: ProviderRuntime>(
    runtime: &R,
    context: &SessionRuntimeContext,
    initial_prompt: Option<&str>,
) -> Result<(), String> {
    runtime.sessions().begin_launch(&context.session_id);
    let started = start_launching(runtime, context, initial_prompt).await;
    if started.is_err() {
        runtime.sessions().end_launch(&context.session_id);
    }
    started
}

/// A provider session whose launch has not committed its row yet. Dropping it undoes the
/// start: the host stops treating the session's status as the provider's, and the plugin
/// is told to destroy the session, since nothing else would stop it without a row.
pub struct Launch<R: ProviderRuntime + Send + 'static> {
    runtime: Arc<R>,
    session_id: String,
    committed: bool,
}

impl<R: ProviderRuntime + Send + 'static> Launch<R> {
    /// The session's row exists: reconciliation may check it, and the launch stands.
    pub fn commit(mut self) {
        self.committed = true;
        self.runtime.sessions().end_launch(&self.session_id);
    }
}

impl<R: ProviderRuntime + Send + 'static> Drop for Launch<R> {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        self.runtime.status().release_owned(&self.session_id);
        let runtime = self.runtime.clone();
        let session_id = std::mem::take(&mut self.session_id);
        tauri::async_runtime::spawn(async move {
            stop(&*runtime, &session_id, StopReason::Destroy).await;
        });
    }
}

/// Start a new session's provider; the caller commits the returned launch once its row exists.
pub async fn launch<R: ProviderRuntime + Send + 'static>(
    runtime: Arc<R>,
    context: &SessionRuntimeContext,
    initial_prompt: Option<&str>,
) -> Result<Launch<R>, String> {
    // Owned before start so hooks firing while the agent boots cannot claim its status.
    runtime.status().mark_owned(&context.session_id);
    if let Err(error) = start(&*runtime, context, initial_prompt).await {
        runtime.status().release_owned(&context.session_id);
        return Err(error);
    }
    Ok(Launch {
        runtime,
        session_id: context.session_id.clone(),
        committed: false,
    })
}

async fn start_launching<R: ProviderRuntime>(
    runtime: &R,
    context: &SessionRuntimeContext,
    initial_prompt: Option<&str>,
) -> Result<(), String> {
    let resolved = resolve(runtime, &context.provider_key).await?;
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
        runtime,
        &context.session_id,
        &resolved,
        protocol::START,
        params,
    )
    .await;
    if started.is_err() {
        // A start cancelled by its deadline may still be running in the sidecar,
        // and the launch rollback is about to remove its worktree.
        let stop =
            serde_json::json!({ "session_id": context.session_id, "reason": StopReason::Destroy });
        let _ = runtime
            .request(&resolved.plugin_id, protocol::STOP, stop)
            .await;
    }
    started
}

/// Make sure the current sidecar instance drives this active session, resuming it after
/// an app or plugin restart, and return its provider. The session is read under its lock,
/// so one stopped meanwhile, for example archived while this waited, is never resumed.
pub async fn ensure<R: ProviderRuntime>(
    runtime: &R,
    session_id: &str,
) -> Result<ResolvedProvider, String> {
    let lock = runtime.sessions().session_lock(session_id);
    let _ensuring = lock.lock().await;
    let context = runtime.session_context(session_id).await?;
    let resolved = resolve(runtime, &context.provider_key).await?;
    let instance = runtime.running_instance(&resolved.plugin_id).await;
    if let Some(binding) = runtime.sessions().get(session_id) {
        if binding.plugin_id == resolved.plugin_id && Some(binding.instance) == instance {
            return Ok(resolved);
        }
    }
    let params = context.params(&resolved.provider);
    bind_and_request(runtime, session_id, &resolved, protocol::RESUME, params).await?;
    Ok(resolved)
}

async fn bind_and_request<R: ProviderRuntime>(
    runtime: &R,
    session_id: &str,
    resolved: &ResolvedProvider,
    method: &str,
    params: serde_json::Map<String, Value>,
) -> Result<(), String> {
    let instance = runtime
        .running_instance(&resolved.plugin_id)
        .await
        .ok_or_else(|| format!("plugin {} is not running", resolved.plugin_id))?;
    let sessions = runtime.sessions();
    sessions.bind(
        session_id,
        Binding {
            plugin_id: resolved.plugin_id.clone(),
            provider_id: resolved.provider.id.clone(),
            instance,
        },
    );
    let result = runtime
        .request(&resolved.plugin_id, method, Value::Object(params))
        .await;
    if result.is_err() {
        sessions.unbind(session_id);
    }
    result.map(|_| ())
}

/// Deliver user or automation input to a provider session.
pub async fn send<R: ProviderRuntime>(
    runtime: &R,
    session_id: &str,
    text: &str,
) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("prompt text must not be empty".to_string());
    }
    check_prompt_size(text)?;
    if runtime.sessions().is_handed_off(session_id) {
        return Err(
            "The session continues in a terminal. Send it there, or close the terminal to return to the chat."
                .to_string(),
        );
    }
    ensure(runtime, session_id).await?;
    request_bound(
        runtime,
        session_id,
        protocol::SEND,
        serde_json::json!({ "session_id": session_id, "text": text }),
    )
    .await
    .map(|_| ())
}

pub async fn interrupt<R: ProviderRuntime>(runtime: &R, session_id: &str) -> Result<(), String> {
    ensure(runtime, session_id).await?;
    request_bound(
        runtime,
        session_id,
        protocol::INTERRUPT,
        serde_json::json!({ "session_id": session_id }),
    )
    .await
    .map(|_| ())
}

async fn request_bound<R: ProviderRuntime>(
    runtime: &R,
    session_id: &str,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let binding = runtime
        .sessions()
        .get(session_id)
        .ok_or_else(|| format!("session {session_id} has no running provider"))?;
    runtime.request(&binding.plugin_id, method, params).await
}

/// Hand a session's conversation to its agent's own TUI. The provider detaches
/// and returns the command that continues the conversation in a terminal.
pub async fn handoff<R: ProviderRuntime>(
    runtime: &R,
    session_id: &str,
) -> Result<Vec<String>, String> {
    let provider = ensure(runtime, session_id).await?.provider;
    if !provider.supports(ProviderFeature::Handoff) {
        return Err(format!("{} cannot continue in a terminal", provider.label));
    }
    let value = request_bound(
        runtime,
        session_id,
        protocol::HANDOFF,
        serde_json::json!({ "session_id": session_id }),
    )
    .await?;
    // The provider detached when it answered, whatever its answer.
    runtime.sessions().set_handed_off(session_id, true);
    let response: HandoffResponse = serde_json::from_value(value)
        .map_err(|error| format!("invalid provider handoff: {error}"))?;
    validate_handoff_argv(response.argv)
}

/// The terminal that continued a session closed; the provider drives it again.
pub async fn handback<R: ProviderRuntime>(runtime: &R, session_id: &str) -> Result<(), String> {
    // The terminal is gone, so prompts go to the provider again.
    runtime.sessions().set_handed_off(session_id, false);
    // A sidecar restarted during the handoff restores the handoff from its transcript,
    // so it must be resumed to hear that the terminal closed. Ended sessions need nothing.
    if runtime.session_context(session_id).await.is_err() {
        return Ok(());
    }
    ensure(runtime, session_id).await?;
    request_bound(
        runtime,
        session_id,
        protocol::HANDBACK,
        serde_json::json!({ "session_id": session_id }),
    )
    .await
    .map(|_| ())
}

/// Stop a provider session that left the active state. A turn it was running stops showing
/// as busy. Sessions the current sidecar never resumed have nothing running and need no request.
pub async fn stop<R: ProviderRuntime>(runtime: &R, session_id: &str, reason: StopReason) {
    let sessions = runtime.sessions();
    let lock = sessions.session_lock(session_id);
    {
        let _stopping = lock.lock().await;
        sessions.end_launch(session_id);
        sessions.set_handed_off(session_id, false);
        let binding = sessions.unbind(session_id);
        runtime.status().release(session_id);
        if let Some(binding) = binding {
            request_stop(runtime, session_id, &binding, reason).await;
        }
    }
    sessions.forget_lock(session_id, &lock);
}

async fn request_stop<R: ProviderRuntime>(
    runtime: &R,
    session_id: &str,
    binding: &Binding,
    reason: StopReason,
) {
    if runtime.running_instance(&binding.plugin_id).await != Some(binding.instance) {
        return;
    }
    let params = serde_json::json!({ "session_id": session_id, "reason": reason });
    match runtime
        .request(&binding.plugin_id, protocol::STOP, params)
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
pub async fn reconcile<R: ProviderRuntime>(runtime: &R) {
    let bound = runtime.sessions().launched_session_ids();
    if bound.is_empty() {
        return;
    }
    match runtime.session_statuses(bound).await {
        Ok(statuses) => {
            for (session_id, status) in statuses {
                if let Some(reason) = ended_reason(status.as_deref()) {
                    stop(runtime, &session_id, reason).await;
                }
            }
        }
        Err(error) => tracing::warn!(%error, "provider session reconciliation failed"),
    }
}

/// What session routing needs from the app: the plugin runtimes, the session store and
/// where status goes. `AppRuntime` provides them for real; tests drive routing through a fake.
pub trait ProviderRuntime: Sync {
    type Status: ProviderStatus;
    fn sessions(&self) -> &ProviderSessions;
    fn status(&self) -> &Self::Status;
    /// The provider, when its plugin is running and declares it.
    fn running_provider(
        &self,
        plugin_id: &str,
        provider_id: &str,
    ) -> impl Future<Output = Result<PluginProvider, String>> + Send;
    fn running_instance(&self, plugin_id: &str) -> impl Future<Output = Option<u64>> + Send;
    fn request(
        &self,
        plugin_id: &str,
        method: &str,
        params: Value,
    ) -> impl Future<Output = Result<Value, String>> + Send;
    /// The session's runtime context; an error when it is unknown or not active.
    fn session_context(
        &self,
        session_id: &str,
    ) -> impl Future<Output = Result<SessionRuntimeContext, String>> + Send;
    /// Each session's stored status, `None` when it has no row.
    fn session_statuses(
        &self,
        session_ids: Vec<String>,
    ) -> impl Future<Output = Result<Vec<(String, Option<String>)>, String>> + Send;
}

/// The app's plugin supervisor and database.
pub struct AppRuntime {
    app: AppHandle,
    supervisor: Arc<PluginRuntimeSupervisor>,
}

impl AppRuntime {
    pub fn new(app: &AppHandle) -> Self {
        Self {
            app: app.clone(),
            supervisor: app.state::<crate::plugins::PluginRuntimeHandle>().0.clone(),
        }
    }
}

impl ProviderRuntime for AppRuntime {
    type Status = AppHandle;

    fn sessions(&self) -> &ProviderSessions {
        self.supervisor.provider_sessions()
    }

    fn status(&self) -> &AppHandle {
        &self.app
    }

    async fn running_provider(
        &self,
        plugin_id: &str,
        provider_id: &str,
    ) -> Result<PluginProvider, String> {
        self.supervisor
            .running_provider(plugin_id, provider_id)
            .await
    }

    async fn running_instance(&self, plugin_id: &str) -> Option<u64> {
        self.supervisor.running_instance(plugin_id).await
    }

    async fn request(&self, plugin_id: &str, method: &str, params: Value) -> Result<Value, String> {
        self.supervisor
            .provider_request(plugin_id, method, params)
            .await
    }

    async fn session_context(&self, session_id: &str) -> Result<SessionRuntimeContext, String> {
        let db = self.app.state::<crate::state::DbState>().0.clone();
        let extra_path_dirs = crate::plugins::configured_extra_path_dirs(&self.app);
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

    async fn session_statuses(
        &self,
        session_ids: Vec<String>,
    ) -> Result<Vec<(String, Option<String>)>, String> {
        let db = self.app.state::<crate::state::DbState>().0.clone();
        crate::commands::blocking(move || {
            let conn = db.lock().map_err(|error| error.to_string())?;
            session_ids
                .into_iter()
                .map(|session_id| {
                    let status = db::get_session(&conn, &session_id)
                        .map_err(|error| error.to_string())?
                        .map(|session| session.status);
                    Ok((session_id, status))
                })
                .collect()
        })
        .await
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use super::*;

    /// Records what reaches the app's notify state and UI.
    #[derive(Default)]
    pub(crate) struct FakeStatus {
        pub(crate) calls: Mutex<Vec<String>>,
    }

    impl FakeStatus {
        pub(crate) fn take(&self) -> Vec<String> {
            std::mem::take(&mut *self.calls.lock().unwrap())
        }

        fn record(&self, call: String) {
            self.calls.lock().unwrap().push(call);
        }
    }

    impl ProviderStatus for FakeStatus {
        fn mark_owned(&self, session_id: &str) {
            self.record(format!("own {session_id}"));
        }

        fn release_owned(&self, session_id: &str) {
            self.record(format!("disown {session_id}"));
        }

        fn apply(&self, session_id: &str, status: ProviderSessionStatus) {
            self.record(format!("{status:?} {session_id}"));
        }

        fn release(&self, session_id: &str) {
            self.record(format!("release {session_id}"));
        }

        fn emit_event(&self, event: ProviderSessionEvent) -> Result<(), String> {
            self.record(format!("event {} {}", event.session_id, event.seq));
            Ok(())
        }
    }

    impl ProviderStatus for Arc<FakeStatus> {
        fn mark_owned(&self, session_id: &str) {
            (**self).mark_owned(session_id)
        }

        fn release_owned(&self, session_id: &str) {
            (**self).release_owned(session_id)
        }

        fn apply(&self, session_id: &str, status: ProviderSessionStatus) {
            (**self).apply(session_id, status)
        }

        fn release(&self, session_id: &str) {
            (**self).release(session_id)
        }

        fn emit_event(&self, event: ProviderSessionEvent) -> Result<(), String> {
            (**self).emit_event(event)
        }
    }

    /// A plugin sidecar and session store driven by the tests.
    #[derive(Default)]
    pub(crate) struct FakeRuntime {
        pub(crate) sessions: ProviderSessions,
        pub(crate) instance: Mutex<Option<u64>>,
        /// Requests sent, with whether the session was bound when each was sent.
        pub(crate) requests: Mutex<Vec<(String, bool)>>,
        /// Stored session statuses; a session without one has no row.
        pub(crate) statuses: Mutex<HashMap<String, String>>,
        pub(crate) failing: Mutex<Vec<&'static str>>,
        /// While set, a resume waits for it.
        pub(crate) resume_gate: Mutex<Option<Arc<tokio::sync::Notify>>>,
        pub(crate) status: FakeStatus,
    }

    impl FakeRuntime {
        pub(crate) fn running(statuses: &[(&str, &str)]) -> Self {
            let runtime = Self::default();
            *runtime.instance.lock().unwrap() = Some(1);
            runtime.statuses.lock().unwrap().extend(
                statuses
                    .iter()
                    .map(|(id, status)| (id.to_string(), status.to_string())),
            );
            runtime
        }

        pub(crate) fn sent(&self) -> Vec<String> {
            self.requests
                .lock()
                .unwrap()
                .iter()
                .map(|(method, _)| method.clone())
                .collect()
        }
    }

    pub(crate) fn context(session_id: &str) -> SessionRuntimeContext {
        SessionRuntimeContext {
            session_id: session_id.into(),
            provider_key: "chat:claude".into(),
            cwd: "/workspace".into(),
            yolo: false,
            extra_path_dirs: Vec::new(),
        }
    }

    impl ProviderRuntime for FakeRuntime {
        type Status = FakeStatus;

        fn sessions(&self) -> &ProviderSessions {
            &self.sessions
        }

        fn status(&self) -> &FakeStatus {
            &self.status
        }

        async fn running_provider(
            &self,
            _plugin_id: &str,
            provider_id: &str,
        ) -> Result<PluginProvider, String> {
            Ok(PluginProvider {
                id: provider_id.into(),
                label: "Claude".into(),
                entrypoint: "ui/chat.js".into(),
                supports: vec![ProviderFeature::Handoff],
            })
        }

        async fn running_instance(&self, _plugin_id: &str) -> Option<u64> {
            *self.instance.lock().unwrap()
        }

        async fn request(
            &self,
            _plugin_id: &str,
            method: &str,
            params: Value,
        ) -> Result<Value, String> {
            let session_id = params["session_id"].as_str().unwrap_or_default();
            let bound = self.sessions.get(session_id).is_some();
            self.requests
                .lock()
                .unwrap()
                .push((method.to_string(), bound));
            let gate = self.resume_gate.lock().unwrap().clone();
            if let (protocol::RESUME, Some(gate)) = (method, gate) {
                gate.notified().await;
            }
            if self.failing.lock().unwrap().contains(&method) {
                return Err(format!("{method} failed"));
            }
            if method == protocol::HANDOFF {
                return Ok(serde_json::json!({ "argv": ["claude", "--resume", session_id] }));
            }
            Ok(serde_json::json!({}))
        }

        async fn session_context(&self, session_id: &str) -> Result<SessionRuntimeContext, String> {
            match self
                .statuses
                .lock()
                .unwrap()
                .get(session_id)
                .map(String::as_str)
            {
                Some("active") => Ok(context(session_id)),
                Some(status) => Err(format!("session is not active (status: {status})")),
                None => Err(format!("session not found: {session_id}")),
            }
        }

        async fn session_statuses(
            &self,
            session_ids: Vec<String>,
        ) -> Result<Vec<(String, Option<String>)>, String> {
            let statuses = self.statuses.lock().unwrap();
            Ok(session_ids
                .into_iter()
                .map(|id| {
                    let status = statuses.get(&id).cloned();
                    (id, status)
                })
                .collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;

    #[tokio::test]
    async fn a_start_binds_before_it_asks_and_undoes_itself_when_refused() {
        let runtime = FakeRuntime::running(&[]);
        start(&runtime, &context("s1"), None).await.unwrap();
        // Notifications the sidecar sends while starting must find the session bound.
        assert_eq!(
            *runtime.requests.lock().unwrap(),
            [(protocol::START.to_string(), true)]
        );
        assert_eq!(
            runtime.sessions.launched_session_ids(),
            Vec::<String>::new()
        );

        runtime.failing.lock().unwrap().push(protocol::START);
        assert!(start(&runtime, &context("s2"), None).await.is_err());
        assert!(runtime.sessions.get("s2").is_none());
        assert!(!runtime
            .sessions
            .state
            .lock()
            .unwrap()
            .launching
            .contains("s2"));
        // A start cut short may still run in the sidecar, so it is told to stop.
        assert_eq!(
            runtime.sent().last().map(String::as_str),
            Some(protocol::STOP)
        );
    }

    #[tokio::test]
    async fn sessions_resume_once_per_sidecar_instance() {
        let runtime = FakeRuntime::running(&[("s1", "active")]);
        ensure(&runtime, "s1").await.unwrap();
        ensure(&runtime, "s1").await.unwrap();
        assert_eq!(runtime.sent(), [protocol::RESUME]);
        *runtime.instance.lock().unwrap() = Some(2);
        ensure(&runtime, "s1").await.unwrap();
        assert_eq!(runtime.sent(), [protocol::RESUME, protocol::RESUME]);
    }

    #[tokio::test]
    async fn a_refused_resume_leaves_the_session_unbound() {
        let runtime = FakeRuntime::running(&[("s1", "active")]);
        runtime.failing.lock().unwrap().push(protocol::RESUME);
        assert!(ensure(&runtime, "s1").await.is_err());
        assert!(runtime.sessions.get("s1").is_none());
    }

    #[tokio::test]
    async fn ended_sessions_are_never_resumed() {
        let runtime = FakeRuntime::running(&[("s1", "archived")]);
        assert!(ensure(&runtime, "s1")
            .await
            .unwrap_err()
            .contains("not active"));
        assert!(runtime.sent().is_empty());
    }

    #[tokio::test]
    async fn stop_skips_a_session_its_current_sidecar_never_resumed() {
        let runtime = FakeRuntime::running(&[("s1", "active")]);
        ensure(&runtime, "s1").await.unwrap();
        *runtime.instance.lock().unwrap() = Some(2);
        stop(&runtime, "s1", StopReason::Archive).await;
        assert_eq!(runtime.sent(), [protocol::RESUME]);
        assert!(runtime.sessions.get("s1").is_none());
    }

    #[tokio::test]
    async fn a_session_archived_while_resuming_is_stopped_after_the_resume() {
        let runtime = Arc::new(FakeRuntime::running(&[("s1", "active")]));
        let gate = Arc::new(tokio::sync::Notify::new());
        *runtime.resume_gate.lock().unwrap() = Some(gate.clone());
        let resuming = tokio::spawn({
            let runtime = runtime.clone();
            async move { ensure(&*runtime, "s1").await.map(|_| ()) }
        });
        while runtime.sent().is_empty() {
            tokio::task::yield_now().await;
        }
        runtime
            .statuses
            .lock()
            .unwrap()
            .insert("s1".into(), "archived".into());
        let stopping = tokio::spawn({
            let runtime = runtime.clone();
            async move { stop(&*runtime, "s1", StopReason::Archive).await }
        });
        tokio::task::yield_now().await;
        // The stop waits for the resume it would otherwise race.
        assert_eq!(runtime.sent(), [protocol::RESUME]);
        gate.notify_one();
        resuming.await.unwrap().unwrap();
        stopping.await.unwrap();
        assert_eq!(runtime.sent(), [protocol::RESUME, protocol::STOP]);
        assert!(runtime.sessions.get("s1").is_none());
        // Anything that asks for it later finds it archived.
        *runtime.resume_gate.lock().unwrap() = None;
        assert!(ensure(&*runtime, "s1").await.is_err());
        assert_eq!(runtime.sent(), [protocol::RESUME, protocol::STOP]);
    }

    #[tokio::test]
    async fn reconciliation_stops_ended_sessions_but_not_launching_ones() {
        let runtime = FakeRuntime::running(&[("live", "active"), ("archived", "active")]);
        for id in ["live", "archived"] {
            ensure(&runtime, id).await.unwrap();
        }
        start(&runtime, &context("launching"), None).await.unwrap();
        runtime
            .statuses
            .lock()
            .unwrap()
            .insert("archived".into(), "archived".into());
        runtime.requests.lock().unwrap().clear();

        reconcile(&runtime).await;
        assert_eq!(runtime.sent(), [protocol::STOP]);
        assert!(runtime.sessions.get("archived").is_none());
        assert!(runtime.sessions.get("live").is_some());
        assert!(runtime.sessions.get("launching").is_some());
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
        assert!(sessions.is_driven_by("s1", "a", 1));
        assert!(!sessions.is_driven_by("s1", "b", 1));
        assert!(!sessions.is_driven_by("s2", "a", 1));
        // A restarted sidecar is a new instance: the previous one no longer drives the session.
        assert!(!sessions.is_driven_by("s1", "a", 2));
        assert!(sessions.unbind("s1").is_some());
        assert!(!sessions.is_driven_by("s1", "a", 1));
    }

    #[test]
    fn reconciliation_skips_sessions_still_launching() {
        // A launching session has no row yet, which reconciliation would read as deleted.
        let sessions = ProviderSessions::default();
        let binding = Binding {
            plugin_id: "a".into(),
            provider_id: "chat".into(),
            instance: 1,
        };
        sessions.bind("s1", binding.clone());
        sessions.bind("s2", binding);
        sessions.begin_launch("s2");
        assert_eq!(sessions.launched_session_ids(), ["s1"]);
        sessions.end_launch("s2");
        let mut ids = sessions.launched_session_ids();
        ids.sort();
        assert_eq!(ids, ["s1", "s2"]);
    }

    #[test]
    fn a_stopped_instance_releases_only_its_own_sessions() {
        let sessions = ProviderSessions::default();
        let binding = |plugin_id: &str, instance| Binding {
            plugin_id: plugin_id.into(),
            provider_id: "chat".into(),
            instance,
        };
        sessions.bind("s1", binding("a", 1));
        sessions.bind("s2", binding("a", 1));
        sessions.bind("s3", binding("a", 2));
        sessions.bind("s4", binding("b", 1));
        let mut released = sessions.unbind_instance("a", 1);
        released.sort();
        assert_eq!(released, ["s1", "s2"]);
        assert!(sessions.get("s1").is_none());
        assert!(sessions.is_driven_by("s3", "a", 2));
        assert!(sessions.is_driven_by("s4", "b", 1));
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

    fn bound(sessions: &ProviderSessions, session_id: &str, instance: u64) {
        sessions.bind(
            session_id,
            Binding {
                plugin_id: "chat".into(),
                provider_id: "claude".into(),
                instance,
            },
        );
    }

    fn status_frame(session_id: &str, status: &str) -> String {
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": STATUS_NOTIFICATION,
            "params": { "session_id": session_id, "status": status },
        })
        .to_string()
    }

    #[tokio::test]
    async fn an_abandoned_launch_disowns_and_destroys_its_session() {
        let runtime = Arc::new(FakeRuntime::running(&[]));
        let launch = launch(runtime.clone(), &context("s1"), None).await.unwrap();
        assert_eq!(runtime.status.take(), ["own s1"]);
        drop(launch);
        // The stop runs in the background, since a launch can be dropped outside async code.
        while !runtime.sent().contains(&protocol::STOP.to_string()) {
            tokio::task::yield_now().await;
        }
        assert_eq!(runtime.status.take(), ["disown s1", "release s1"]);
        assert!(runtime.sessions.get("s1").is_none());
    }

    #[tokio::test]
    async fn a_committed_launch_becomes_reconcilable() {
        let runtime = Arc::new(FakeRuntime::running(&[]));
        let launch = launch(runtime.clone(), &context("s1"), None).await.unwrap();
        launch.commit();
        assert_eq!(runtime.sessions.launched_session_ids(), ["s1"]);
        assert_eq!(runtime.sent(), [protocol::START]);
        assert_eq!(runtime.status.take(), ["own s1"]);
    }

    #[tokio::test]
    async fn a_refused_launch_gives_status_back() {
        let runtime = Arc::new(FakeRuntime::running(&[]));
        runtime.failing.lock().unwrap().push(protocol::START);
        assert!(launch(runtime.clone(), &context("s1"), None).await.is_err());
        assert_eq!(runtime.status.take(), ["own s1", "disown s1"]);
    }

    #[test]
    fn notifications_from_a_previous_sidecar_instance_are_dropped() {
        let sessions = Arc::new(ProviderSessions::default());
        bound(&sessions, "s1", 1);
        let status = Arc::new(FakeStatus::default());
        let stale = NotificationSink::new(status.clone(), "chat", 2, sessions.clone());
        assert!(stale.try_handle(&status_frame("s1", "busy")));
        assert!(status.take().is_empty());

        let current = NotificationSink::new(status.clone(), "chat", 1, sessions.clone());
        assert!(current.try_handle(&status_frame("s1", "busy")));
        let event = r#"{"jsonrpc":"2.0","method":"host.session.event","params":{"session_id":"s1","seq":3,"payload":{}}}"#;
        assert!(current.try_handle(event));
        assert_eq!(status.take(), ["Busy s1", "event s1 3"]);
    }

    #[test]
    fn an_exited_session_is_unbound_before_its_status_applies() {
        let sessions = Arc::new(ProviderSessions::default());
        bound(&sessions, "s1", 1);
        let status = Arc::new(FakeStatus::default());
        let sink = NotificationSink::new(status.clone(), "chat", 1, sessions.clone());
        assert!(sink.try_handle(&status_frame("s1", "exited")));
        assert_eq!(status.take(), ["Exited s1"]);
        assert!(sessions.get("s1").is_none());
        // Whatever it sends afterwards is no longer its own.
        assert!(sink.try_handle(&status_frame("s1", "idle")));
        assert!(status.take().is_empty());
    }

    #[tokio::test]
    async fn stopping_a_session_ends_a_running_turn_and_forgets_its_lock() {
        let runtime = FakeRuntime::running(&[("s1", "active")]);
        ensure(&runtime, "s1").await.unwrap();
        stop(&runtime, "s1", StopReason::Archive).await;
        assert_eq!(runtime.status.take(), ["release s1"]);
        assert_eq!(runtime.sent(), [protocol::RESUME, protocol::STOP]);
        assert!(runtime.sessions.locks.lock().unwrap().is_empty());
    }

    #[test]
    fn a_stopped_sidecar_releases_the_turns_it_was_running() {
        let sessions = ProviderSessions::default();
        bound(&sessions, "s1", 1);
        bound(&sessions, "s2", 2);
        let status = FakeStatus::default();
        runtime_stopped(&status, &sessions, "chat", 1);
        assert_eq!(status.take(), ["release s1"]);
        assert!(sessions.get("s2").is_some());
    }

    #[tokio::test]
    async fn prompts_are_refused_while_the_session_continues_in_a_terminal() {
        let runtime = FakeRuntime::running(&[("s1", "active")]);
        assert_eq!(
            handoff(&runtime, "s1").await.unwrap(),
            ["claude", "--resume", "s1"]
        );
        assert!(send(&runtime, "s1", "hi")
            .await
            .unwrap_err()
            .contains("continues in a terminal"));
        handback(&runtime, "s1").await.unwrap();
        send(&runtime, "s1", "hi").await.unwrap();
        assert_eq!(
            runtime.sent(),
            [
                protocol::RESUME,
                protocol::HANDOFF,
                protocol::HANDBACK,
                protocol::SEND
            ]
        );
    }

    /// Unbinds the session while a status applies, as a concurrent stop can.
    struct StoppedMeanwhile {
        sessions: Arc<ProviderSessions>,
        status: FakeStatus,
    }

    impl ProviderStatus for StoppedMeanwhile {
        fn mark_owned(&self, _session_id: &str) {}
        fn release_owned(&self, _session_id: &str) {}
        fn apply(&self, session_id: &str, status: ProviderSessionStatus) {
            self.sessions.unbind(session_id);
            self.status.apply(session_id, status);
        }
        fn release(&self, session_id: &str) {
            self.status.release(session_id);
        }
        fn emit_event(&self, _event: ProviderSessionEvent) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn a_turn_starting_as_its_session_stops_is_not_left_busy() {
        let sessions = Arc::new(ProviderSessions::default());
        bound(&sessions, "s1", 1);
        let status = StoppedMeanwhile {
            sessions: sessions.clone(),
            status: FakeStatus::default(),
        };
        let sink = NotificationSink::new(status, "chat", 1, sessions);
        assert!(sink.try_handle(&status_frame("s1", "busy")));
        assert_eq!(sink.status.status.take(), ["Busy s1", "release s1"]);
    }
}

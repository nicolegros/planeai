use std::sync::Arc;

use serde_json::Value;

use super::errors::ProviderError;
use super::registry::Binding;
use super::runtime::ProviderRuntime;
use super::sink::ProviderStatus;
use crate::db;
use crate::plugins::{PluginProvider, ProviderFeature};
use crate::session_ops::PLUGIN_BACKEND;
use planeai_plugin_contract::provider::{
    self as protocol, check_prompt_size, error_code, parse_provider_key, validate_handoff_argv,
    HandoffResponse, ProviderErrorCode, StopReason,
};

/// Everything a provider needs to run one session in its worktree.
#[derive(Debug)]
pub struct SessionRuntimeContext {
    pub session_id: String,
    pub provider_key: String,
    pub cwd: String,
    pub auto_approve: bool,
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
            auto_approve: session.auto_approve,
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
        params.insert("auto_approve".into(), self.auto_approve.into());
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
) -> Result<ResolvedProvider, ProviderError> {
    let (plugin_id, provider_id) = parse_provider_key(provider_key)
        .ok_or_else(|| ProviderError::not_running(format!("Unknown provider: {provider_key}")))?;
    let provider = runtime
        .running_provider(plugin_id, provider_id)
        .await
        .map_err(ProviderError::not_running)?;
    Ok(ResolvedProvider {
        plugin_id: plugin_id.to_string(),
        provider,
    })
}

/// Start a brand-new provider session. The binding exists before the request so
/// notifications emitted while starting are accepted.
pub(super) async fn start<R: ProviderRuntime>(
    runtime: &R,
    context: &SessionRuntimeContext,
    initial_prompt: Option<&str>,
) -> Result<(), ProviderError> {
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
    target: StopTarget,
    committed: bool,
}

impl<R: ProviderRuntime + Send + 'static> Launch<R> {
    /// The session's row exists: reconciliation may check it, and the launch stands.
    pub fn commit(mut self) {
        self.committed = true;
        self.runtime.sessions().end_launch(&self.target.session_id);
    }
}

impl<R: ProviderRuntime + Send + 'static> Drop for Launch<R> {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        self.runtime.status().release_owned(&self.target.session_id);
        let runtime = self.runtime.clone();
        let target = self.target.clone();
        tauri::async_runtime::spawn(async move {
            stop(&*runtime, &target, StopReason::Destroy).await;
        });
    }
}

/// Start a new session's provider; the caller commits the returned launch once its row exists.
pub async fn launch<R: ProviderRuntime + Send + 'static>(
    runtime: Arc<R>,
    context: &SessionRuntimeContext,
    initial_prompt: Option<&str>,
) -> Result<Launch<R>, ProviderError> {
    // Owned before start so hooks firing while the agent boots cannot claim its status.
    runtime.status().mark_owned(&context.session_id);
    if let Err(error) = start(&*runtime, context, initial_prompt).await {
        runtime.status().release_owned(&context.session_id);
        return Err(error);
    }
    let target = StopTarget::of(&context.session_id, &context.provider_key)
        .expect("a started session's provider key names its plugin");
    Ok(Launch {
        runtime,
        target,
        committed: false,
    })
}

async fn start_launching<R: ProviderRuntime>(
    runtime: &R,
    context: &SessionRuntimeContext,
    initial_prompt: Option<&str>,
) -> Result<(), ProviderError> {
    let resolved = resolve(runtime, &context.provider_key).await?;
    if context.auto_approve && !resolved.provider.supports(ProviderFeature::AutoApprove) {
        return Err(ProviderError::new(
            ProviderErrorCode::Unsupported,
            format!("{} does not support auto-approve", resolved.provider.label),
        ));
    }
    let mut params = context.params(&resolved.provider);
    if let Some(prompt) = initial_prompt.filter(|prompt| !prompt.trim().is_empty()) {
        check_prompt_size(prompt).map_err(ProviderError::too_large)?;
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
) -> Result<ResolvedProvider, ProviderError> {
    let lock = runtime.sessions().session_lock(session_id);
    let _ensuring = lock.lock().await;
    let context = runtime
        .session_context(session_id)
        .await
        .map_err(ProviderError::not_running)?;
    let resolved = resolve(runtime, &context.provider_key).await?;
    let instance = runtime.running_instance(&resolved.plugin_id).await;
    if let Some(binding) = runtime.sessions().binding(session_id) {
        if binding.plugin_id == resolved.plugin_id && Some(binding.instance) == instance {
            return Ok(resolved);
        }
    }
    let mut params = context.params(&resolved.provider);
    // The host's record wins: a sidecar restarted during a handoff must not forget it,
    // and one an app restart ended is over, its terminal gone with the app.
    params.insert(
        "handed_off".into(),
        runtime.sessions().is_handed_off(session_id).into(),
    );
    bind_and_request(runtime, session_id, &resolved, protocol::RESUME, params).await?;
    Ok(resolved)
}

async fn bind_and_request<R: ProviderRuntime>(
    runtime: &R,
    session_id: &str,
    resolved: &ResolvedProvider,
    method: &str,
    params: serde_json::Map<String, Value>,
) -> Result<(), ProviderError> {
    let instance = runtime
        .running_instance(&resolved.plugin_id)
        .await
        .ok_or_else(|| {
            ProviderError::not_running(format!("plugin {} is not running", resolved.plugin_id))
        })?;
    let sessions = runtime.sessions();
    sessions.bind(
        session_id,
        Binding {
            plugin_id: resolved.plugin_id.clone(),
            instance,
        },
    );
    let result = runtime
        .request(&resolved.plugin_id, method, Value::Object(params))
        .await;
    if result.is_err() {
        sessions.unbind(session_id);
    }
    result.map(|_| ()).map_err(ProviderError::from)
}

/// Sends a request to the provider driving the session, resuming it first when needed.
/// A plugin that answers it does not know the session, as one that lost it, gets it
/// resumed and the request once more.
async fn request_session<R: ProviderRuntime>(
    runtime: &R,
    session_id: &str,
    method: &str,
    params: Value,
) -> Result<(ResolvedProvider, Value), ProviderError> {
    let resolved = ensure(runtime, session_id).await?;
    match runtime
        .request(&resolved.plugin_id, method, params.clone())
        .await
    {
        Err(error) if error.code() == Some(error_code::SESSION_NOT_FOUND) => {
            tracing::info!(session_id, %method, "provider lost the session; resuming it");
            runtime.sessions().unbind(session_id);
            let resolved = ensure(runtime, session_id).await?;
            let value = runtime.request(&resolved.plugin_id, method, params).await?;
            Ok((resolved, value))
        }
        result => Ok((resolved, result?)),
    }
}

/// Deliver user or automation input to a provider session.
pub async fn send<R: ProviderRuntime>(
    runtime: &R,
    session_id: &str,
    text: &str,
) -> Result<(), ProviderError> {
    if text.trim().is_empty() {
        return Err(ProviderError::new(
            ProviderErrorCode::PluginError,
            "prompt text must not be empty",
        ));
    }
    check_prompt_size(text).map_err(ProviderError::too_large)?;
    if runtime.sessions().is_handed_off(session_id) {
        return Err(ProviderError::handed_off());
    }
    request_session(
        runtime,
        session_id,
        protocol::SEND,
        serde_json::json!({ "session_id": session_id, "text": text }),
    )
    .await
    .map(|_| ())
}

pub async fn interrupt<R: ProviderRuntime>(
    runtime: &R,
    session_id: &str,
) -> Result<(), ProviderError> {
    request_session(
        runtime,
        session_id,
        protocol::INTERRUPT,
        serde_json::json!({ "session_id": session_id }),
    )
    .await
    .map(|_| ())
}

/// Hand a session's conversation to its agent's own TUI. The provider detaches
/// and returns the command that continues the conversation in a terminal.
pub async fn handoff<R: ProviderRuntime>(
    runtime: &R,
    session_id: &str,
) -> Result<Vec<String>, ProviderError> {
    let provider = ensure(runtime, session_id).await?.provider;
    if !provider.supports(ProviderFeature::Handoff) {
        return Err(ProviderError::new(
            ProviderErrorCode::Unsupported,
            format!("{} cannot continue in a terminal", provider.label),
        ));
    }
    let (_, value) = request_session(
        runtime,
        session_id,
        protocol::HANDOFF,
        serde_json::json!({ "session_id": session_id }),
    )
    .await?;
    // The provider detached when it answered, whatever its answer.
    runtime.sessions().set_handed_off(session_id, true);
    let argv = serde_json::from_value::<HandoffResponse>(value)
        .map_err(|error| format!("invalid provider handoff: {error}"))
        .and_then(|response| validate_handoff_argv(response.argv))
        .map_err(|message| ProviderError::new(ProviderErrorCode::PluginError, message));
    if argv.is_err() {
        // Without a command to continue in, nothing would ever drive the session again.
        if let Err(error) = handback(runtime, session_id).await {
            tracing::warn!(session_id, %error, "provider session handback after a bad handoff failed");
        }
    }
    argv
}

/// The terminal that continued a session closed; the provider drives it again.
pub async fn handback<R: ProviderRuntime>(
    runtime: &R,
    session_id: &str,
) -> Result<(), ProviderError> {
    // The terminal is gone, so prompts go to the provider again.
    runtime.sessions().set_handed_off(session_id, false);
    // Ended sessions need nothing; a sidecar restarted during the handoff is resumed first,
    // handed off, and hears the terminal closed through this handback.
    if runtime.session_context(session_id).await.is_err() {
        return Ok(());
    }
    request_session(
        runtime,
        session_id,
        protocol::HANDBACK,
        serde_json::json!({ "session_id": session_id }),
    )
    .await
    .map(|_| ())
}

/// A provider session to stop, and the plugin that hears of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopTarget {
    pub session_id: String,
    pub plugin_id: String,
}

impl StopTarget {
    /// The target for a session on `provider_key`; `None` when no plugin provides it.
    pub fn of(session_id: &str, provider_key: &str) -> Option<Self> {
        let (plugin_id, _) = parse_provider_key(provider_key)?;
        Some(Self {
            session_id: session_id.to_string(),
            plugin_id: plugin_id.to_string(),
        })
    }
}

/// Stop a provider session that left the active state. A turn it was running stops showing
/// as busy, and everything the host tracked for it is forgotten at once. The plugin hears of
/// it even when no sidecar drives the session, so it can drop what it keeps for it.
pub async fn stop<R: ProviderRuntime>(runtime: &R, target: &StopTarget, reason: StopReason) {
    let sessions = runtime.sessions();
    let session_id = target.session_id.as_str();
    let lock = sessions.session_lock(session_id);
    {
        let _stopping = lock.lock().await;
        sessions.forget(session_id);
        runtime.status().release(session_id);
        request_stop(runtime, target, reason).await;
    }
    sessions.forget_lock(session_id, &lock);
}

async fn request_stop<R: ProviderRuntime>(runtime: &R, target: &StopTarget, reason: StopReason) {
    let StopTarget {
        session_id,
        plugin_id,
    } = target;
    // A plugin that is not running hears of it through its next reconciliation.
    if runtime.running_instance(plugin_id).await.is_none() {
        return;
    }
    let params = serde_json::json!({ "session_id": session_id, "reason": reason });
    match runtime.request(plugin_id, protocol::STOP, params).await {
        Ok(_) => tracing::info!(%session_id, %plugin_id, ?reason, "stopped provider session"),
        Err(error) => {
            tracing::warn!(%session_id, %plugin_id, %error, "provider session stop failed")
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
pub(super) fn ended_reason(status: Option<&str>) -> Option<StopReason> {
    match status {
        None => Some(StopReason::Destroy),
        Some(status) => stop_reason(status),
    }
}

/// Among `session_ids`, those whose stored status says they ended (or whose row is gone).
pub async fn ended_sessions<R: ProviderRuntime>(
    runtime: &R,
    session_ids: Vec<String>,
) -> Vec<String> {
    if session_ids.is_empty() {
        return Vec::new();
    }
    match runtime.session_statuses(session_ids).await {
        Ok(statuses) => statuses
            .into_iter()
            .filter(|(_, status)| ended_reason(status.as_deref()).is_some())
            .map(|(session_id, _)| session_id)
            .collect(),
        Err(error) => {
            tracing::warn!(%error, "ended session lookup failed");
            Vec::new()
        }
    }
}

/// Stop the bound provider sessions whose rows ended outside the GUI (CLI archive or
/// delete, task completion), which only notify that sessions changed. Returns the sessions
/// it stopped, whose terminals must end with them.
pub async fn reconcile<R: ProviderRuntime>(runtime: &R) -> Vec<String> {
    let bound = runtime.sessions().launched();
    if bound.is_empty() {
        return Vec::new();
    }
    let session_ids = bound.iter().map(|(id, _)| id.clone()).collect();
    let statuses = match runtime.session_statuses(session_ids).await {
        Ok(statuses) => statuses,
        Err(error) => {
            tracing::warn!(%error, "provider session reconciliation failed");
            return Vec::new();
        }
    };
    let mut stopped = Vec::new();
    for ((session_id, binding), (_, status)) in bound.into_iter().zip(statuses) {
        if let Some(reason) = ended_reason(status.as_deref()) {
            let target = StopTarget {
                session_id: session_id.clone(),
                plugin_id: binding.plugin_id,
            };
            stop(runtime, &target, reason).await;
            stopped.push(session_id);
        }
    }
    stopped
}

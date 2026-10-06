use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use super::registry::ProviderSessions;
use planeai_plugin_contract::provider::{
    is_provider_notification, ProviderSessionStatus, SessionEventParams, SessionStatusParams,
    EVENT_NOTIFICATION, STATUS_NOTIFICATION,
};

/// Frontend event carrying opaque provider session events to the mounted UI.
const SESSION_EVENT: &str = "plugin-provider-session-event";

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
    pub(super) status: S,
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

    /// Whether this sink routes notifications to `method`.
    pub fn handles(&self, method: &str) -> bool {
        is_provider_notification(method)
    }

    /// Routes a provider notification, dropping one that is invalid or not this sidecar's.
    pub fn route(&self, method: &str, params: Value) {
        let routed = match method {
            EVENT_NOTIFICATION => self.route_event(params),
            STATUS_NOTIFICATION => self.route_status(params),
            _ => Err(format!("{method} is not a provider notification")),
        };
        if let Err(error) = routed {
            tracing::warn!(plugin_id = %self.plugin_id, %method, %error, "dropped provider notification");
        }
    }

    fn route_event(&self, params: Value) -> Result<(), String> {
        let params: SessionEventParams = serde_json::from_value(params)
            .map_err(|error| format!("invalid session event: {error}"))?;
        self.require_owner(&params.session_id)?;
        self.sessions.advance_seq(&params)?;
        self.status.emit_event(ProviderSessionEvent {
            plugin_id: self.plugin_id.clone(),
            session_id: params.session_id,
            seq: params.seq,
            payload: params.payload,
        })
    }

    fn route_status(&self, params: Value) -> Result<(), String> {
        let params: SessionStatusParams = serde_json::from_value(params)
            .map_err(|error| format!("invalid session status: {error}"))?;
        self.require_owner(&params.session_id)?;
        if params.status == ProviderSessionStatus::Exited {
            self.sessions.unbind(&params.session_id);
        }
        self.status.apply(&params.session_id, params.status);
        // Stopped while this applied: its stop released the session before it was busy.
        if params.status == ProviderSessionStatus::Busy
            && !self
                .sessions
                .is_driven_by(&params.session_id, &self.plugin_id, self.instance)
        {
            self.status.release(&params.session_id);
        }
        Ok(())
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

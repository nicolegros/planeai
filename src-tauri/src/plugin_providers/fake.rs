use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use super::registry::ProviderSessions;
use super::routing::SessionRuntimeContext;
use super::runtime::ProviderRuntime;
use super::sink::{ProviderSessionEvent, ProviderStatus};
use crate::plugin_rpc::PluginRpcError;
use crate::plugins::{PluginProvider, ProviderFeature};
use planeai_plugin_contract::provider::{self as protocol, error_code, ProviderSessionStatus};

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
    /// What the provider answers a handoff with, instead of a runnable argv.
    pub(crate) handoff_answer: Mutex<Option<Value>>,
    /// A method the provider answers once with "session not found", as one that lost it.
    pub(crate) loses_session_on: Mutex<Option<&'static str>>,
    /// The params of the last request of each method.
    pub(crate) params: Mutex<HashMap<String, Value>>,
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
        auto_approve: false,
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
    ) -> Result<Value, PluginRpcError> {
        let session_id = params["session_id"].as_str().unwrap_or_default();
        let bound = self.sessions.binding(session_id).is_some();
        self.requests
            .lock()
            .unwrap()
            .push((method.to_string(), bound));
        self.params
            .lock()
            .unwrap()
            .insert(method.to_string(), params.clone());
        {
            let mut loses = self.loses_session_on.lock().unwrap();
            if *loses == Some(method) {
                *loses = None;
                return Err(PluginRpcError::Rpc {
                    code: error_code::SESSION_NOT_FOUND,
                    message: "unknown session".into(),
                });
            }
        }
        let gate = self.resume_gate.lock().unwrap().clone();
        if let (protocol::RESUME, Some(gate)) = (method, gate) {
            gate.notified().await;
        }
        if self.failing.lock().unwrap().contains(&method) {
            return Err(PluginRpcError::Rpc {
                code: -32000,
                message: format!("{method} failed"),
            });
        }
        if method == protocol::HANDOFF {
            return Ok(self.handoff_answer.lock().unwrap().clone().unwrap_or_else(
                || serde_json::json!({ "argv": ["claude", "--resume", session_id] }),
            ));
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

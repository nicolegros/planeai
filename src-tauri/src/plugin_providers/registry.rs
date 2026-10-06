use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use planeai_plugin_contract::provider::SessionEventParams;

/// The sidecar instance driving a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Binding {
    pub(super) plugin_id: String,
    /// The instance that accepted start or resume; a restarted sidecar must resume again.
    pub(super) instance: u64,
}

/// What the host tracks for one provider session during this run.
#[derive(Debug, Default)]
pub(super) struct SessionState {
    pub(super) binding: Option<Binding>,
    /// Its start is in flight: its row does not exist yet, which reconciliation would
    /// otherwise read as a deleted session.
    pub(super) launching: bool,
    /// It continues in a terminal: its provider is detached until the handback.
    pub(super) handed_off: bool,
    /// The last event `seq` its provider sent, kept across sidecar restarts.
    pub(super) last_seq: u64,
}

/// Which plugin drives each provider session, and what else the host tracks for it.
#[derive(Default)]
pub struct ProviderSessions {
    pub(super) sessions: Mutex<HashMap<String, SessionState>>,
    /// Serializes `ensure` and `stop` per session, so concurrent first uses resume it once
    /// and a session stopped while resuming is never left running.
    pub(super) locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl ProviderSessions {
    fn update<T>(&self, session_id: &str, update: impl FnOnce(&mut SessionState) -> T) -> T {
        update(
            self.sessions
                .lock()
                .unwrap()
                .entry(session_id.to_string())
                .or_default(),
        )
    }

    fn read<T>(&self, session_id: &str, read: impl FnOnce(&SessionState) -> T) -> Option<T> {
        self.sessions.lock().unwrap().get(session_id).map(read)
    }

    pub(super) fn session_lock(&self, session_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.locks
            .lock()
            .unwrap()
            .entry(session_id.to_string())
            .or_default()
            .clone()
    }

    /// Drops a stopped session's lock unless another caller holds or awaits it.
    pub(super) fn forget_lock(&self, session_id: &str, lock: &Arc<tokio::sync::Mutex<()>>) {
        let mut locks = self.locks.lock().unwrap();
        // One reference is the map's, the other the caller's.
        if Arc::strong_count(lock) == 2 {
            locks.remove(session_id);
        }
    }

    pub(super) fn bind(&self, session_id: &str, binding: Binding) {
        self.update(session_id, |session| session.binding = Some(binding));
    }

    pub(super) fn unbind(&self, session_id: &str) -> Option<Binding> {
        self.sessions
            .lock()
            .unwrap()
            .get_mut(session_id)?
            .binding
            .take()
    }

    pub(super) fn binding(&self, session_id: &str) -> Option<Binding> {
        self.read(session_id, |session| session.binding.clone())
            .flatten()
    }

    /// Clearing it leaves sessions the host does not track, such as ended ones, untracked.
    pub(super) fn set_handed_off(&self, session_id: &str, handed_off: bool) {
        if handed_off {
            self.update(session_id, |session| session.handed_off = true);
        } else if let Some(session) = self.sessions.lock().unwrap().get_mut(session_id) {
            session.handed_off = false;
        }
    }

    pub(super) fn is_handed_off(&self, session_id: &str) -> bool {
        self.read(session_id, |session| session.handed_off)
            .unwrap_or(false)
    }

    /// Accepts an event whose `seq` follows the session's last one.
    pub(super) fn advance_seq(&self, event: &SessionEventParams) -> Result<(), String> {
        self.update(&event.session_id, |session| {
            event.check_seq(session.last_seq)?;
            session.last_seq = event.seq;
            Ok(())
        })
    }

    pub(super) fn begin_launch(&self, session_id: &str) {
        self.update(session_id, |session| session.launching = true);
    }

    /// The launch committed the session's row or was abandoned.
    pub(super) fn end_launch(&self, session_id: &str) {
        self.update(session_id, |session| session.launching = false);
    }

    /// Forgets a stopped session at once, returning the binding it had.
    pub(super) fn forget(&self, session_id: &str) -> Option<Binding> {
        self.sessions
            .lock()
            .unwrap()
            .remove(session_id)
            .and_then(|session| session.binding)
    }

    /// Bound sessions whose rows exist, which reconciliation may check.
    pub(super) fn launched(&self) -> Vec<(String, Binding)> {
        self.sessions
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, session)| !session.launching)
            .filter_map(|(session_id, session)| {
                Some((session_id.clone(), session.binding.clone()?))
            })
            .collect()
    }

    /// Whether this sidecar instance of the plugin currently drives the session.
    pub(super) fn is_driven_by(&self, session_id: &str, plugin_id: &str, instance: u64) -> bool {
        self.binding(session_id)
            .is_some_and(|binding| binding.plugin_id == plugin_id && binding.instance == instance)
    }

    /// Unbinds the sessions a sidecar instance drove, returning their ids.
    pub(super) fn unbind_instance(&self, plugin_id: &str, instance: u64) -> Vec<String> {
        let mut sessions = self.sessions.lock().unwrap();
        let mut ended = Vec::new();
        for (session_id, session) in sessions.iter_mut() {
            if session.binding.as_ref().is_some_and(|binding| {
                binding.plugin_id == plugin_id && binding.instance == instance
            }) {
                session.binding = None;
                ended.push(session_id.clone());
            }
        }
        ended
    }
}

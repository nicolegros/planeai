use crate::session::DaemonSession;
use crate::types::{SpawnMode, SpawnOutcome};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::time::Duration;

/// Default time to retain exited/killed sessions before GC removes them.
pub const DEFAULT_GC_TTL: Duration = Duration::from_secs(30 * 60); // 30 minutes
/// Default time to retain correlated spawn request records before GC removes them.
pub const DEFAULT_SPAWN_REQUEST_TTL: Duration = Duration::from_secs(30 * 60); // 30 minutes

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SessionState {
    Running,
    Exited { ended_at: DateTime<Utc> },
    Killed { ended_at: DateTime<Utc> },
}

impl SessionState {
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }

    pub fn status_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Exited { .. } => "exited",
            Self::Killed { .. } => "killed",
        }
    }

    pub fn ended_at(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::Exited { ended_at, .. } | Self::Killed { ended_at } => Some(*ended_at),
            Self::Running => None,
        }
    }
}

pub struct RegistryEntry {
    session: DaemonSession,
    state: SessionState,
    created_at: DateTime<Utc>,
    spawn_request_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SpawnRequestState {
    CancelledBeforeSpawn,
    Spawned {
        session_id: String,
        outcome: SpawnOutcome,
        cancelled: bool,
    },
    Failed(String),
}

struct SpawnRequestRecord {
    state: SpawnRequestState,
    recorded_at: DateTime<Utc>,
}

/// Result of handling a correlated spawn request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpawnRequestOutcome {
    Spawned {
        session_id: String,
        outcome: SpawnOutcome,
    },
    Cancelled,
}

impl RegistryEntry {
    pub fn session(&self) -> &DaemonSession {
        &self.session
    }

    pub fn state(&self) -> &SessionState {
        &self.state
    }
}

/// Info returned to clients about a session.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub alive: bool,
    pub status: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
}

pub struct SessionRegistry {
    sessions: HashMap<String, RegistryEntry>,
    spawn_requests: HashMap<String, SpawnRequestRecord>,
    gc_ttl: Duration,
    spawn_request_ttl: Duration,
}

impl Default for SessionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionRegistry {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            spawn_requests: HashMap::new(),
            gc_ttl: DEFAULT_GC_TTL,
            spawn_request_ttl: DEFAULT_SPAWN_REQUEST_TTL,
        }
    }

    pub fn with_gc_ttl(mut self, ttl: Duration) -> Self {
        self.gc_ttl = ttl;
        self
    }

    pub fn with_spawn_request_ttl(mut self, ttl: Duration) -> Self {
        self.spawn_request_ttl = ttl;
        self
    }

    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        &mut self,
        session_id: impl Into<String>,
        command: &str,
        args: &[&str],
        cwd: Option<&str>,
        env: Option<&HashMap<String, String>>,
        buffer_capacity: usize,
        mode: SpawnMode,
    ) -> anyhow::Result<SpawnOutcome> {
        let id: String = session_id.into();

        let outcome = match mode {
            SpawnMode::CreateOnly => {
                if self.sessions.contains_key(&id) {
                    anyhow::bail!("session already exists: {id}");
                }
                SpawnOutcome::Spawned
            }
            SpawnMode::AttachIfRunning => {
                if let Some(entry) = self.sessions.get(&id) {
                    if entry.state.is_running() {
                        return Ok(SpawnOutcome::AlreadyRunning);
                    }
                    anyhow::bail!("session exists but not running: {id}");
                }
                anyhow::bail!("session not found: {id}");
            }
            SpawnMode::ReplaceExited => {
                if let Some(entry) = self.sessions.get(&id) {
                    if entry.state.is_running() {
                        anyhow::bail!("session is still running: {id}");
                    }
                    self.sessions.remove(&id);
                }
                SpawnOutcome::Spawned
            }
            SpawnMode::Restart => {
                if let Some(entry) = self.sessions.get(&id) {
                    if entry.state.is_running() {
                        let _ = entry.session.kill();
                    }
                    self.sessions.remove(&id);
                    SpawnOutcome::Restarted
                } else {
                    SpawnOutcome::Spawned
                }
            }
        };

        let session = DaemonSession::spawn(&id, command, args, cwd, env, buffer_capacity)?;
        self.sessions.insert(
            id,
            RegistryEntry {
                session,
                state: SessionState::Running,
                created_at: Utc::now(),
                spawn_request_id: None,
            },
        );
        Ok(outcome)
    }

    /// Spawn once for `request_id`, replaying the original result for duplicate
    /// requests. A cancellation recorded before this call suppresses spawning.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_for_request(
        &mut self,
        request_id: &str,
        session_id: impl Into<String>,
        command: &str,
        args: &[&str],
        cwd: Option<&str>,
        env: Option<&HashMap<String, String>>,
        buffer_capacity: usize,
        mode: SpawnMode,
    ) -> anyhow::Result<SpawnRequestOutcome> {
        if let Some(record) = self.spawn_requests.get(request_id) {
            return match &record.state {
                SpawnRequestState::CancelledBeforeSpawn => Ok(SpawnRequestOutcome::Cancelled),
                SpawnRequestState::Spawned {
                    cancelled: true, ..
                } => Ok(SpawnRequestOutcome::Cancelled),
                SpawnRequestState::Spawned {
                    session_id,
                    outcome,
                    ..
                } => Ok(SpawnRequestOutcome::Spawned {
                    session_id: session_id.clone(),
                    outcome: *outcome,
                }),
                SpawnRequestState::Failed(error) => Err(anyhow::anyhow!(error.clone())),
            };
        }

        let session_id = session_id.into();
        match self.spawn(
            session_id.clone(),
            command,
            args,
            cwd,
            env,
            buffer_capacity,
            mode,
        ) {
            Ok(outcome) => {
                if outcome != SpawnOutcome::AlreadyRunning {
                    if let Some(entry) = self.sessions.get_mut(&session_id) {
                        entry.spawn_request_id = Some(request_id.to_string());
                    }
                }
                self.spawn_requests.insert(
                    request_id.to_string(),
                    SpawnRequestRecord {
                        state: SpawnRequestState::Spawned {
                            session_id: session_id.clone(),
                            outcome,
                            cancelled: false,
                        },
                        recorded_at: Utc::now(),
                    },
                );
                Ok(SpawnRequestOutcome::Spawned {
                    session_id,
                    outcome,
                })
            }
            Err(error) => {
                let error = error.to_string();
                self.spawn_requests.insert(
                    request_id.to_string(),
                    SpawnRequestRecord {
                        state: SpawnRequestState::Failed(error.clone()),
                        recorded_at: Utc::now(),
                    },
                );
                Err(anyhow::anyhow!(error))
            }
        }
    }

    /// Cancel a correlated spawn. If it has not reached the registry yet, record
    /// the cancellation so a later spawn is suppressed. If it has spawned, kill
    /// only the exact registry entry created for this request ID.
    pub fn cancel_spawn(&mut self, request_id: &str) -> anyhow::Result<()> {
        let session_to_kill = match self.spawn_requests.get(request_id) {
            None => {
                self.spawn_requests.insert(
                    request_id.to_string(),
                    SpawnRequestRecord {
                        state: SpawnRequestState::CancelledBeforeSpawn,
                        recorded_at: Utc::now(),
                    },
                );
                return Ok(());
            }
            Some(SpawnRequestRecord {
                state: SpawnRequestState::CancelledBeforeSpawn | SpawnRequestState::Failed(_),
                ..
            }) => return Ok(()),
            Some(SpawnRequestRecord {
                state: SpawnRequestState::Spawned { cancelled, .. },
                ..
            }) if *cancelled => return Ok(()),
            Some(SpawnRequestRecord {
                state: SpawnRequestState::Spawned { session_id, .. },
                ..
            }) => self
                .sessions
                .get(session_id)
                .filter(|entry| entry.spawn_request_id.as_deref() == Some(request_id))
                .map(|_| session_id.clone()),
        };

        if let Some(session_id) = session_to_kill {
            self.kill(&session_id)?;
        }
        if let Some(SpawnRequestRecord {
            state: SpawnRequestState::Spawned { cancelled, .. },
            ..
        }) = self.spawn_requests.get_mut(request_id)
        {
            *cancelled = true;
        }
        Ok(())
    }

    pub fn kill(&mut self, session_id: &str) -> anyhow::Result<()> {
        let entry = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| anyhow::anyhow!("session not found: {session_id}"))?;
        if !entry.state.is_running() {
            // Retained terminal sessions are already closed. Keep both their
            // terminal variant and original timestamp for reconciliation.
            return Ok(());
        }
        entry.session.kill()?;
        entry.state = SessionState::Killed {
            ended_at: Utc::now(),
        };
        Ok(())
    }

    pub fn get(&self, session_id: &str) -> Option<&DaemonSession> {
        self.sessions.get(session_id).map(|e| &e.session)
    }

    pub fn list(&self) -> Vec<SessionInfo> {
        self.sessions
            .values()
            .map(|e| SessionInfo {
                session_id: e.session.session_id().to_string(),
                alive: e.state.is_running(),
                status: e.state.status_str().to_string(),
                started_at: Some(e.created_at.to_rfc3339()),
                ended_at: e.state.ended_at().map(|t| t.to_rfc3339()),
            })
            .collect()
    }

    /// Transition sessions whose PTY has exited from Running to Exited.
    /// Returns IDs of newly-exited sessions.
    pub fn poll_exits(&mut self) -> Vec<String> {
        let mut newly_exited = Vec::new();
        for (id, entry) in self.sessions.iter_mut() {
            if entry.state.is_running() && !entry.session.is_alive() {
                entry.state = SessionState::Exited {
                    ended_at: Utc::now(),
                };
                newly_exited.push(id.clone());
            }
        }
        newly_exited
    }

    /// Remove sessions and correlated spawn records that have exceeded their TTLs.
    /// Returns IDs of garbage-collected sessions.
    pub fn gc(&mut self) -> Vec<String> {
        let now = Utc::now();
        let ttl = chrono::Duration::from_std(self.gc_ttl).unwrap_or(chrono::Duration::hours(1));
        let request_ttl = chrono::Duration::from_std(self.spawn_request_ttl)
            .unwrap_or(chrono::Duration::hours(1));
        let expired: Vec<String> = self
            .sessions
            .iter()
            .filter_map(|(id, entry)| {
                if let Some(ended) = entry.state.ended_at() {
                    if now - ended > ttl {
                        return Some(id.clone());
                    }
                }
                None
            })
            .collect();
        for id in &expired {
            self.sessions.remove(id);
        }
        // A request can be retried long after its initial response was lost.
        // Retain its correlation record only while the exact session generation
        // it created remains live; a reused ID must not keep a stale record.
        // Tombstones, failures, and records for terminal sessions still expire.
        let live_session_requests: std::collections::HashSet<(&str, &str)> = self
            .sessions
            .iter()
            .filter(|(_, entry)| entry.state.is_running())
            .filter_map(|(session_id, entry)| {
                entry
                    .spawn_request_id
                    .as_deref()
                    .map(|request_id| (session_id.as_str(), request_id))
            })
            .collect();
        self.spawn_requests.retain(|request_id, record| {
            let associated_session_is_live = matches!(
                &record.state,
                SpawnRequestState::Spawned { session_id, .. }
                    if live_session_requests.contains(&(session_id.as_str(), request_id.as_str()))
            );
            associated_session_is_live || now - record.recorded_at <= request_ttl
        });
        expired
    }

    /// Count of sessions with live PTY processes.
    pub fn live_count(&self) -> usize {
        self.sessions
            .values()
            .filter(|e| e.state.is_running())
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// True if there are no live (running) sessions.
    pub fn no_live_sessions(&self) -> bool {
        self.live_count() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn_echo(reg: &mut SessionRegistry, id: &str) -> SpawnOutcome {
        reg.spawn(
            id,
            "/bin/sh",
            &["-c", "echo hi"],
            None,
            None,
            4096,
            SpawnMode::CreateOnly,
        )
        .unwrap()
    }

    fn spawn_sleep(reg: &mut SessionRegistry, id: &str) -> SpawnOutcome {
        reg.spawn(
            id,
            "sleep",
            &["999"],
            None,
            None,
            4096,
            SpawnMode::CreateOnly,
        )
        .unwrap()
    }

    #[test]
    fn running_to_exited_via_poll() {
        let mut reg = SessionRegistry::new();
        spawn_echo(&mut reg, "s1");
        std::thread::sleep(std::time::Duration::from_millis(500));
        let exited = reg.poll_exits();
        assert!(exited.contains(&"s1".to_string()));
        assert_eq!(reg.list()[0].status, "exited");
        // Session is still in registry
        assert!(reg.get("s1").is_some());
    }

    #[test]
    fn running_to_killed() {
        let mut reg = SessionRegistry::new();
        spawn_sleep(&mut reg, "s1");
        reg.kill("s1").unwrap();
        let info = &reg.list()[0];
        assert_eq!(info.status, "killed");
        assert!(!info.alive);
    }

    #[test]
    fn kill_is_idempotent_and_preserves_retained_terminal_states() {
        let mut reg = SessionRegistry::new();
        spawn_echo(&mut reg, "exited");
        spawn_sleep(&mut reg, "killed");

        std::thread::sleep(Duration::from_millis(500));
        reg.poll_exits();
        let exited_before = reg
            .sessions
            .get("exited")
            .expect("exited session should be retained")
            .state
            .clone();

        reg.kill("killed").unwrap();
        let killed_before = reg
            .sessions
            .get("killed")
            .expect("killed session should be retained")
            .state
            .clone();

        reg.kill("exited").unwrap();
        reg.kill("killed").unwrap();

        assert_eq!(reg.sessions["exited"].state, exited_before);
        assert_eq!(reg.sessions["killed"].state, killed_before);
    }

    #[test]
    fn exited_retained_until_gc() {
        let mut reg = SessionRegistry::new().with_gc_ttl(Duration::from_millis(50));
        spawn_echo(&mut reg, "s1");
        std::thread::sleep(std::time::Duration::from_millis(500));
        reg.poll_exits();
        // Not yet GC'd
        assert_eq!(reg.list().len(), 1);
        // Wait for GC TTL
        std::thread::sleep(std::time::Duration::from_millis(100));
        let gc_ids = reg.gc();
        assert!(gc_ids.contains(&"s1".to_string()));
        assert!(reg.list().is_empty());
    }

    #[test]
    fn live_count_ignores_exited() {
        let mut reg = SessionRegistry::new();
        spawn_sleep(&mut reg, "live");
        spawn_echo(&mut reg, "dead");
        std::thread::sleep(std::time::Duration::from_millis(500));
        reg.poll_exits();
        assert_eq!(reg.live_count(), 1);
        assert_eq!(reg.list().len(), 2);
        reg.kill("live").unwrap();
    }

    #[test]
    fn create_only_fails_if_exists() {
        let mut reg = SessionRegistry::new();
        spawn_sleep(&mut reg, "s1");
        let err = reg
            .spawn(
                "s1",
                "echo",
                &["x"],
                None,
                None,
                4096,
                SpawnMode::CreateOnly,
            )
            .unwrap_err();
        assert!(err.to_string().contains("already exists"));
        reg.kill("s1").unwrap();
    }

    #[test]
    fn replace_exited_works() {
        let mut reg = SessionRegistry::new();
        spawn_echo(&mut reg, "s1");
        std::thread::sleep(std::time::Duration::from_millis(500));
        reg.poll_exits();
        let outcome = reg
            .spawn(
                "s1",
                "sleep",
                &["999"],
                None,
                None,
                4096,
                SpawnMode::ReplaceExited,
            )
            .unwrap();
        assert_eq!(outcome, SpawnOutcome::Spawned);
        assert!(reg.get("s1").unwrap().is_alive());
        reg.kill("s1").unwrap();
    }

    #[test]
    fn replace_exited_fails_if_running() {
        let mut reg = SessionRegistry::new();
        spawn_sleep(&mut reg, "s1");
        let err = reg
            .spawn(
                "s1",
                "echo",
                &["x"],
                None,
                None,
                4096,
                SpawnMode::ReplaceExited,
            )
            .unwrap_err();
        assert!(err.to_string().contains("still running"));
        reg.kill("s1").unwrap();
    }

    #[test]
    fn restart_kills_and_replaces() {
        let mut reg = SessionRegistry::new();
        spawn_sleep(&mut reg, "s1");
        let outcome = reg
            .spawn(
                "s1",
                "sleep",
                &["999"],
                None,
                None,
                4096,
                SpawnMode::Restart,
            )
            .unwrap();
        assert_eq!(outcome, SpawnOutcome::Restarted);
        assert!(reg.get("s1").unwrap().is_alive());
        reg.kill("s1").unwrap();
    }

    #[test]
    fn attach_if_running_returns_already_running() {
        let mut reg = SessionRegistry::new();
        spawn_sleep(&mut reg, "s1");
        let outcome = reg
            .spawn(
                "s1",
                "echo",
                &["x"],
                None,
                None,
                4096,
                SpawnMode::AttachIfRunning,
            )
            .unwrap();
        assert_eq!(outcome, SpawnOutcome::AlreadyRunning);
        reg.kill("s1").unwrap();
    }

    #[test]
    fn attach_if_running_errors_when_exited() {
        let mut reg = SessionRegistry::new();
        spawn_echo(&mut reg, "s1");
        std::thread::sleep(std::time::Duration::from_millis(500));
        reg.poll_exits();
        let err = reg
            .spawn(
                "s1",
                "echo",
                &["x"],
                None,
                None,
                4096,
                SpawnMode::AttachIfRunning,
            )
            .unwrap_err();
        assert!(err.to_string().contains("not running"));
    }

    #[test]
    fn buffer_snapshot_available_after_exit() {
        let mut reg = SessionRegistry::new();
        spawn_echo(&mut reg, "s1");
        std::thread::sleep(std::time::Duration::from_millis(1000));
        reg.poll_exits();
        let session = reg.get("s1").unwrap();
        let snap = session.buffer_snapshot();
        assert!(
            String::from_utf8_lossy(&snap).contains("hi"),
            "expected 'hi' in buffer, got: {:?}",
            String::from_utf8_lossy(&snap)
        );
    }

    #[test]
    fn cancellation_before_spawn_suppresses_the_request() {
        let mut reg = SessionRegistry::new();
        reg.cancel_spawn("request-1").unwrap();

        let outcome = reg
            .spawn_for_request(
                "request-1",
                "s1",
                "sleep",
                &["999"],
                None,
                None,
                4096,
                SpawnMode::CreateOnly,
            )
            .unwrap();

        assert_eq!(outcome, SpawnRequestOutcome::Cancelled);
        assert!(reg.list().is_empty());
    }

    #[test]
    fn duplicate_spawn_request_id_replays_without_spawning_again() {
        let mut reg = SessionRegistry::new();
        let first = reg
            .spawn_for_request(
                "request-1",
                "s1",
                "sleep",
                &["999"],
                None,
                None,
                4096,
                SpawnMode::CreateOnly,
            )
            .unwrap();
        let duplicate = reg
            .spawn_for_request(
                "request-1",
                "other-session",
                "echo",
                &["should-not-run"],
                None,
                None,
                4096,
                SpawnMode::CreateOnly,
            )
            .unwrap();

        assert_eq!(
            first,
            SpawnRequestOutcome::Spawned {
                session_id: "s1".to_string(),
                outcome: SpawnOutcome::Spawned,
            }
        );
        assert_eq!(duplicate, first);
        assert_eq!(reg.list().len(), 1);
        assert_eq!(reg.list()[0].session_id, "s1");
        reg.kill("s1").unwrap();
    }

    #[test]
    fn gc_retains_live_spawn_request_records_and_expires_terminal_ones() {
        let mut reg = SessionRegistry::new().with_spawn_request_ttl(Duration::from_millis(10));
        reg.cancel_spawn("cancelled-request").unwrap();
        reg.spawn_for_request(
            "completed-request",
            "first-session",
            "sleep",
            &["999"],
            None,
            None,
            4096,
            SpawnMode::CreateOnly,
        )
        .unwrap();
        reg.spawn_for_request(
            "failed-request",
            "failed-session",
            "definitely-not-a-real-command",
            &[],
            None,
            None,
            4096,
            SpawnMode::CreateOnly,
        )
        .unwrap_err();

        std::thread::sleep(Duration::from_millis(25));
        reg.gc();

        let cancelled_tombstone_expired = reg
            .spawn_for_request(
                "cancelled-request",
                "after-tombstone-expiry",
                "sleep",
                &["999"],
                None,
                None,
                4096,
                SpawnMode::CreateOnly,
            )
            .unwrap();
        let live_completed_record = reg
            .spawn_for_request(
                "completed-request",
                "after-live-completed-retry",
                "sleep",
                &["999"],
                None,
                None,
                4096,
                SpawnMode::CreateOnly,
            )
            .unwrap();
        let failed_record_expired = reg
            .spawn_for_request(
                "failed-request",
                "after-failure-expiry",
                "sleep",
                &["999"],
                None,
                None,
                4096,
                SpawnMode::CreateOnly,
            )
            .unwrap();

        assert!(matches!(
            cancelled_tombstone_expired,
            SpawnRequestOutcome::Spawned { .. }
        ));
        assert_eq!(
            live_completed_record,
            SpawnRequestOutcome::Spawned {
                session_id: "first-session".to_string(),
                outcome: SpawnOutcome::Spawned,
            }
        );
        assert!(matches!(
            failed_record_expired,
            SpawnRequestOutcome::Spawned { .. }
        ));

        // The record is already older than its TTL. Once its associated session
        // becomes terminal, the next GC may expire it and allow a fresh request.
        reg.kill("first-session").unwrap();
        reg.gc();
        let terminal_completed_record_expired = reg
            .spawn_for_request(
                "completed-request",
                "after-terminal-completed-expiry",
                "sleep",
                &["999"],
                None,
                None,
                4096,
                SpawnMode::CreateOnly,
            )
            .unwrap();
        assert!(matches!(
            terminal_completed_record_expired,
            SpawnRequestOutcome::Spawned { .. }
        ));

        for session_id in [
            "after-tombstone-expiry",
            "after-failure-expiry",
            "after-terminal-completed-expiry",
        ] {
            reg.kill(session_id).unwrap();
        }
    }

    #[test]
    fn gc_expires_a_request_record_when_its_session_id_is_reused() {
        let mut reg = SessionRegistry::new().with_spawn_request_ttl(Duration::from_millis(10));
        reg.spawn_for_request(
            "request-1",
            "s1",
            "sleep",
            &["999"],
            None,
            None,
            4096,
            SpawnMode::CreateOnly,
        )
        .unwrap();

        // Restart creates a new running generation with the same ID, not owned
        // by request-1. It must not pin the stale request record indefinitely.
        reg.spawn(
            "s1",
            "sleep",
            &["999"],
            None,
            None,
            4096,
            SpawnMode::Restart,
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(25));
        reg.gc();

        assert!(!reg.spawn_requests.contains_key("request-1"));
        reg.kill("s1").unwrap();
    }

    #[test]
    fn cancellation_does_not_kill_a_session_attached_by_request() {
        let mut reg = SessionRegistry::new();
        spawn_sleep(&mut reg, "existing-session");
        let outcome = reg
            .spawn_for_request(
                "attach-request",
                "existing-session",
                "ignored",
                &[],
                None,
                None,
                4096,
                SpawnMode::AttachIfRunning,
            )
            .unwrap();
        assert!(matches!(
            outcome,
            SpawnRequestOutcome::Spawned {
                outcome: SpawnOutcome::AlreadyRunning,
                ..
            }
        ));

        reg.cancel_spawn("attach-request").unwrap();

        assert!(reg.get("existing-session").unwrap().is_alive());
        reg.kill("existing-session").unwrap();
    }

    #[test]
    fn cancellation_only_kills_the_session_created_for_that_request() {
        let mut reg = SessionRegistry::new();
        reg.spawn_for_request(
            "request-1",
            "s1",
            "sleep",
            &["999"],
            None,
            None,
            4096,
            SpawnMode::CreateOnly,
        )
        .unwrap();
        // A later restart replaces the session ID with a different generation.
        reg.spawn(
            "s1",
            "sleep",
            &["999"],
            None,
            None,
            4096,
            SpawnMode::Restart,
        )
        .unwrap();

        reg.cancel_spawn("request-1").unwrap();

        assert!(reg.get("s1").unwrap().is_alive());
        reg.kill("s1").unwrap();
    }
}

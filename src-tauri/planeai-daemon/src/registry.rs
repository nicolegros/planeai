use crate::session::DaemonSession;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::time::Duration;

/// Default time to retain exited/killed sessions before GC removes them.
pub const DEFAULT_GC_TTL: Duration = Duration::from_secs(30 * 60); // 30 minutes

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SessionState {
    Running,
    Exited {
        exit_status: Option<i32>,
        ended_at: DateTime<Utc>,
    },
    Killed {
        ended_at: DateTime<Utc>,
    },
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

    pub fn exit_status(&self) -> Option<i32> {
        match self {
            Self::Exited { exit_status, .. } => *exit_status,
            _ => None,
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
    pub session: DaemonSession,
    pub state: SessionState,
    pub created_at: DateTime<Utc>,
}

/// Spawn mode controls how a spawn request interacts with existing sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpawnMode {
    /// Fail if session_id already exists.
    CreateOnly,
    /// Return AlreadyRunning if live; error if exited/missing.
    AttachIfRunning,
    /// Spawn if missing or exited/killed; error if running.
    ReplaceExited,
    /// Kill running if needed, then spawn.
    Restart,
}

impl Default for SpawnMode {
    fn default() -> Self {
        Self::ReplaceExited
    }
}

/// Outcome of a spawn request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpawnOutcome {
    Spawned,
    AlreadyRunning,
    ReplacedExited,
    Restarted,
}

/// Info returned to clients about a session.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub alive: bool,
    pub status: String,
    pub exit_status: Option<i32>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
}

pub struct SessionRegistry {
    sessions: HashMap<String, RegistryEntry>,
    gc_ttl: Duration,
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
            gc_ttl: DEFAULT_GC_TTL,
        }
    }

    pub fn with_gc_ttl(mut self, ttl: Duration) -> Self {
        self.gc_ttl = ttl;
        self
    }

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

        match mode {
            SpawnMode::CreateOnly => {
                if self.sessions.contains_key(&id) {
                    anyhow::bail!("session already exists: {id}");
                }
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
                    // Remove exited/killed entry to replace
                    self.sessions.remove(&id);
                }
            }
            SpawnMode::Restart => {
                if let Some(entry) = self.sessions.get(&id) {
                    if entry.state.is_running() {
                        let _ = entry.session.kill();
                    }
                    self.sessions.remove(&id);
                    let session =
                        DaemonSession::spawn(&id, command, args, cwd, env, buffer_capacity)?;
                    self.sessions.insert(
                        id,
                        RegistryEntry {
                            session,
                            state: SessionState::Running,
                            created_at: Utc::now(),
                        },
                    );
                    return Ok(SpawnOutcome::Restarted);
                }
                // Not found — spawn fresh
            }
        }

        let session = DaemonSession::spawn(&id, command, args, cwd, env, buffer_capacity)?;
        let outcome = if mode == SpawnMode::ReplaceExited
            && !self.sessions.contains_key(&id)
            && mode == SpawnMode::ReplaceExited
        {
            // We only know we replaced if we removed above; detect via lacking entry
            SpawnOutcome::Spawned
        } else {
            SpawnOutcome::Spawned
        };
        self.sessions.insert(
            id,
            RegistryEntry {
                session,
                state: SessionState::Running,
                created_at: Utc::now(),
            },
        );
        Ok(outcome)
    }

    pub fn kill(&mut self, session_id: &str) -> anyhow::Result<()> {
        let entry = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| anyhow::anyhow!("session not found: {session_id}"))?;
        entry.session.kill()?;
        entry.state = SessionState::Killed {
            ended_at: Utc::now(),
        };
        Ok(())
    }

    pub fn get(&self, session_id: &str) -> Option<&DaemonSession> {
        self.sessions.get(session_id).map(|e| &e.session)
    }

    pub fn get_entry(&self, session_id: &str) -> Option<&RegistryEntry> {
        self.sessions.get(session_id)
    }

    pub fn list(&self) -> Vec<SessionInfo> {
        self.sessions
            .values()
            .map(|e| SessionInfo {
                session_id: e.session.session_id().to_string(),
                alive: e.state.is_running(),
                status: e.state.status_str().to_string(),
                exit_status: e.state.exit_status(),
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
                    exit_status: None, // portable-pty doesn't expose exit code via is_alive
                    ended_at: Utc::now(),
                };
                newly_exited.push(id.clone());
            }
        }
        newly_exited
    }

    /// Remove sessions that have been exited/killed longer than gc_ttl.
    /// Returns IDs of garbage-collected sessions.
    pub fn gc(&mut self) -> Vec<String> {
        let now = Utc::now();
        let ttl = chrono::Duration::from_std(self.gc_ttl).unwrap_or(chrono::Duration::hours(1));
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
}

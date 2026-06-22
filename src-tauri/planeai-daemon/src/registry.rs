use crate::session::DaemonSession;
use std::collections::HashMap;

pub struct SessionInfo {
    pub session_id: String,
    pub alive: bool,
}

pub struct SessionRegistry {
    sessions: HashMap<String, DaemonSession>,
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
        }
    }

    pub fn spawn(
        &mut self,
        session_id: impl Into<String>,
        command: &str,
        args: &[&str],
        cwd: Option<&str>,
        env: Option<&HashMap<String, String>>,
        buffer_capacity: usize,
    ) -> anyhow::Result<()> {
        let id: String = session_id.into();
        let session = DaemonSession::spawn(&id, command, args, cwd, env, buffer_capacity)?;
        self.sessions.insert(id, session);
        Ok(())
    }

    pub fn kill(&mut self, session_id: &str) -> anyhow::Result<()> {
        let session = self
            .sessions
            .get(session_id)
            .ok_or_else(|| anyhow::anyhow!("session not found: {session_id}"))?;
        session.kill()
    }

    pub fn get(&self, session_id: &str) -> Option<&DaemonSession> {
        self.sessions.get(session_id)
    }

    pub fn list(&self) -> Vec<SessionInfo> {
        self.sessions
            .values()
            .map(|s| SessionInfo {
                session_id: s.session_id().to_string(),
                alive: s.is_alive(),
            })
            .collect()
    }

    pub fn remove_dead(&mut self) -> Vec<String> {
        let dead: Vec<String> = self
            .sessions
            .iter()
            .filter(|(_, s)| !s.is_alive())
            .map(|(id, _)| id.clone())
            .collect();
        for id in &dead {
            self.sessions.remove(id);
        }
        dead
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

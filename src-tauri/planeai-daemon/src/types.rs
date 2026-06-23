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
    Restarted,
}

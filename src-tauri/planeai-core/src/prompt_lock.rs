//! Per-session prompt locking via SQLite.
//!
//! Prevents concurrent prompt sends to the same session from multiple processes
//! (e.g., GUI + CLI). Stale locks are automatically cleaned up.

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

/// Default stale lock timeout in seconds (2 minutes — conservative for long prompts).
const STALE_TIMEOUT_SECS: i64 = 120;

#[derive(Debug, PartialEq)]
pub enum LockError {
    /// Another prompt is already in flight for this session.
    Busy {
        session_id: String,
        owner_id: String,
        acquired_at: String,
    },
    /// Database error.
    Db(String),
}

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy { .. } => write!(f, "session prompt already in progress"),
            Self::Db(msg) => write!(f, "prompt lock error: {msg}"),
        }
    }
}

#[derive(Debug)]
pub struct PromptLock {
    pub session_id: String,
    pub owner_id: String,
}

/// Create the prompt_locks table. Idempotent.
pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS prompt_locks (
            session_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            acquired_at TEXT NOT NULL
        );",
    )?;
    Ok(())
}

/// Attempt to acquire a prompt lock for the given session.
/// Cleans up stale locks before attempting acquisition.
/// Returns `Ok(PromptLock)` on success, `Err(LockError::Busy{..})` if already held.
pub fn acquire(conn: &Connection, session_id: &str) -> Result<PromptLock, LockError> {
    // Clean stale locks
    let cutoff = Utc::now() - chrono::Duration::seconds(STALE_TIMEOUT_SECS);
    conn.execute(
        "DELETE FROM prompt_locks WHERE acquired_at < ?1",
        params![cutoff.to_rfc3339()],
    )
    .map_err(|e| LockError::Db(e.to_string()))?;

    // Check for existing lock
    let existing: Option<(String, String)> = conn
        .query_row(
            "SELECT owner_id, acquired_at FROM prompt_locks WHERE session_id = ?1",
            params![session_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| LockError::Db(e.to_string()))?;

    if let Some((owner_id, acquired_at)) = existing {
        return Err(LockError::Busy {
            session_id: session_id.to_string(),
            owner_id,
            acquired_at,
        });
    }

    // Acquire
    let owner_id = Uuid::new_v4().to_string();
    let acquired_at = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO prompt_locks (session_id, owner_id, acquired_at) VALUES (?1, ?2, ?3)",
        params![session_id, owner_id, acquired_at],
    )
    .map_err(|e| LockError::Db(e.to_string()))?;

    Ok(PromptLock {
        session_id: session_id.to_string(),
        owner_id,
    })
}

/// Release a prompt lock. Should always be called after prompt send completes (success or error).
pub fn release(conn: &Connection, lock: &PromptLock) -> Result<(), LockError> {
    conn.execute(
        "DELETE FROM prompt_locks WHERE session_id = ?1 AND owner_id = ?2",
        params![lock.session_id, lock.owner_id],
    )
    .map_err(|e| LockError::Db(e.to_string()))?;
    Ok(())
}

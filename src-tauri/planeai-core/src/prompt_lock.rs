//! Per-session prompt locking via SQLite.
//!
//! Prevents concurrent prompt sends to the same session from multiple processes
//! (e.g., GUI + CLI). Stale locks are automatically cleaned up.

use chrono::Utc;
use rusqlite::{params, Connection};
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
    let cutoff = Utc::now() - chrono::Duration::seconds(STALE_TIMEOUT_SECS);
    conn.execute(
        "DELETE FROM prompt_locks WHERE acquired_at < ?1",
        params![cutoff.to_rfc3339()],
    )
    .map_err(|e| LockError::Db(e.to_string()))?;

    let owner_id = Uuid::new_v4().to_string();
    let acquired_at = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT OR IGNORE INTO prompt_locks (session_id, owner_id, acquired_at) VALUES (?1, ?2, ?3)",
        params![session_id, owner_id, acquired_at],
    )
    .map_err(|e| LockError::Db(e.to_string()))?;

    if conn.changes() == 0 {
        let (existing_owner, existing_at): (String, String) = conn
            .query_row(
                "SELECT owner_id, acquired_at FROM prompt_locks WHERE session_id = ?1",
                params![session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| LockError::Db(e.to_string()))?;

        return Err(LockError::Busy {
            session_id: session_id.to_string(),
            owner_id: existing_owner,
            acquired_at: existing_at,
        });
    }

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

/// RAII guard that releases the prompt lock on drop.
///
/// Use `acquire_guard` to obtain one. The guard ensures the lock is released
/// even if the caller returns early via `?` or panics.
pub struct PromptLockGuard<'a> {
    conn: &'a Connection,
    lock: Option<PromptLock>,
}

impl<'a> PromptLockGuard<'a> {
    /// Access the underlying lock info.
    pub fn lock(&self) -> &PromptLock {
        self.lock.as_ref().expect("guard already consumed")
    }

    /// Explicitly release the lock, returning any error from the release.
    /// If release fails, Drop will still attempt cleanup (lock remains in self.lock).
    /// If not called, Drop will release silently (logging on failure).
    pub fn release(mut self) -> Result<(), LockError> {
        if let Some(ref lock) = self.lock {
            release(self.conn, lock)?;
        }
        // Only clear after successful release — if release() returned Err above,
        // we never reach here, and Drop will retry.
        self.lock = None;
        Ok(())
    }
}

impl<'a> Drop for PromptLockGuard<'a> {
    fn drop(&mut self) {
        if let Some(ref lock) = self.lock {
            if let Err(e) = self.conn.execute(
                "DELETE FROM prompt_locks WHERE session_id = ?1 AND owner_id = ?2",
                params![lock.session_id, lock.owner_id],
            ) {
                tracing::warn!(
                    session_id = %lock.session_id,
                    error = %e,
                    "PromptLockGuard: failed to release lock on drop"
                );
            }
        }
    }
}

/// Acquire a prompt lock and return an RAII guard that releases on drop.
///
/// Prefer this over raw `acquire` + `release` to prevent lock leaks on early returns.
pub fn acquire_guard<'a>(
    conn: &'a Connection,
    session_id: &str,
) -> Result<PromptLockGuard<'a>, LockError> {
    let lock = acquire(conn, session_id)?;
    Ok(PromptLockGuard {
        conn,
        lock: Some(lock),
    })
}

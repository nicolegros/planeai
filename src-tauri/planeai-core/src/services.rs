//! Shared PlaneAI domain services — UI-neutral persistence for projects and sessions.
//!
//! Both Tauri and Iced call into these services. No UI framework dependency.
//! Migration uses the same idempotent ALTER TABLE pattern as the production db.rs.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, Result as SqlResult};

use crate::app_data_dir;

// ─── Database ────────────────────────────────────────────────────────────────

/// Open (or create) the shared PlaneAI database.
pub fn open_db() -> SqlResult<Connection> {
    let dir = app_data_dir();
    std::fs::create_dir_all(&dir).ok();
    let path = dir.join("planeai.db");
    let conn = Connection::open(&path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    migrate(&conn)?;
    Ok(conn)
}

/// Open the shared DB at a custom path (for testing).
pub fn open_db_at(path: &Path) -> SqlResult<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    migrate(&conn)?;
    Ok(conn)
}

/// Convenience alias — runs project/session migrations only.
pub fn migrate(conn: &Connection) -> SqlResult<()> {
    migrate_project_session_schema(conn)?;
    crate::prompt_lock::migrate(conn)?;
    LayoutService::migrate(conn)?;
    crate::loop_service::LoopService::migrate(conn)?;
    Ok(())
}

/// Idempotent project/session schema migration — the single source of truth.
/// Called by both Tauri (`db.rs::migrate()`) and Iced/domain code.
/// Safe to run on fresh DBs and existing production DBs alike.
pub fn migrate_project_session_schema(conn: &Connection) -> SqlResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS projects (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            path TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES projects(id),
            name TEXT NOT NULL DEFAULT '',
            tmux_name TEXT,
            branch TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT NOT NULL
        );",
    )?;
    // Idempotent column additions
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN name TEXT NOT NULL DEFAULT ''");
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN worktree_path TEXT");
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN provider TEXT");
    let _ =
        conn.execute_batch("ALTER TABLE sessions ADD COLUMN backend TEXT NOT NULL DEFAULT 'tmux'");
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN provider_session_id TEXT");
    let _ =
        conn.execute_batch("ALTER TABLE sessions ADD COLUMN tab_count INTEGER NOT NULL DEFAULT 1");
    let _ = conn
        .execute_batch("ALTER TABLE sessions ADD COLUMN auto_approve INTEGER NOT NULL DEFAULT 1");
    let _ =
        conn.execute_batch("ALTER TABLE projects ADD COLUMN status TEXT NOT NULL DEFAULT 'active'");
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN task_key TEXT");
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN base_branch TEXT");
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN mru_position INTEGER");
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN pr_url TEXT");
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN pr_state TEXT");
    let _ = conn.execute_batch(
        "ALTER TABLE sessions ADD COLUMN auto_dispatched INTEGER NOT NULL DEFAULT 0",
    );
    let _ =
        conn.execute_batch("ALTER TABLE projects ADD COLUMN auto_mode INTEGER NOT NULL DEFAULT 0");
    let _ = conn.execute_batch("ALTER TABLE projects ADD COLUMN task_manager TEXT");
    let _ = conn.execute_batch("ALTER TABLE projects ADD COLUMN prefix TEXT NOT NULL DEFAULT ''");
    // Backfill prefix for existing projects that don't have one yet.
    // If a project already has tasks under the old (first-3-chars) prefix, keep it to avoid
    // orphaning tasks. Only derive new-style prefix for projects without existing tasks.
    {
        let mut stmt = conn.prepare("SELECT id, name FROM projects WHERE prefix = ''")?;
        let rows: Vec<(String, String)> = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();
        for (id, name) in rows {
            let old_prefix: String = name
                .chars()
                .filter(|c| c.is_alphanumeric())
                .take(3)
                .collect::<String>()
                .to_uppercase();
            // Check if tasks exist under the old prefix
            let has_tasks: bool = conn
                .prepare("SELECT EXISTS(SELECT 1 FROM task_projects WHERE prefix = ?1)")
                .and_then(|mut s| s.query_row(params![old_prefix], |r| r.get(0)))
                .unwrap_or(false);
            let prefix = if has_tasks {
                old_prefix
            } else {
                planeai_tasks::sqlite::derive_prefix(&name)
            };
            conn.execute(
                "UPDATE projects SET prefix = ?1 WHERE id = ?2",
                params![prefix, id],
            )?;
        }
    }
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN updated_at TEXT");
    let _ =
        conn.execute_batch("UPDATE sessions SET updated_at = created_at WHERE updated_at IS NULL");
    let _ = conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS sessions_updated_at
         AFTER UPDATE ON sessions
         BEGIN
           UPDATE sessions SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = NEW.id;
         END;",
    );

    // Migrate tmux_name from NOT NULL to nullable for DBs created before dual-backend.
    let has_not_null: bool = conn
        .prepare("SELECT sql FROM sqlite_master WHERE type='table' AND name='sessions'")
        .and_then(|mut s| s.query_row([], |r| r.get::<_, String>(0)))
        .map(|sql| sql.contains("tmux_name TEXT NOT NULL"))
        .unwrap_or(false);
    if has_not_null {
        conn.execute_batch(
            "ALTER TABLE sessions RENAME TO sessions_old;
             CREATE TABLE sessions (
                 id TEXT PRIMARY KEY,
                 project_id TEXT NOT NULL REFERENCES projects(id),
                 name TEXT NOT NULL DEFAULT '',
                 tmux_name TEXT,
                 branch TEXT NOT NULL,
                 status TEXT NOT NULL DEFAULT 'active',
                 created_at TEXT NOT NULL,
                 worktree_path TEXT,
                 provider TEXT,
                 backend TEXT NOT NULL DEFAULT 'tmux',
                 provider_session_id TEXT,
                 tab_count INTEGER NOT NULL DEFAULT 1,
                 auto_approve INTEGER NOT NULL DEFAULT 1,
                 task_key TEXT,
                 base_branch TEXT,
                 mru_position INTEGER,
                 pr_url TEXT,
                 pr_state TEXT,
                 auto_dispatched INTEGER NOT NULL DEFAULT 0,
                 updated_at TEXT
             );
             INSERT INTO sessions (id, project_id, name, tmux_name, branch, status, created_at, worktree_path, provider, backend, provider_session_id, tab_count, auto_approve, task_key, base_branch, mru_position, pr_url, pr_state, auto_dispatched, updated_at)
                 SELECT id, project_id, name, tmux_name, branch, status, created_at, worktree_path, provider, backend, provider_session_id, tab_count, auto_approve, task_key, base_branch, mru_position, pr_url, pr_state, auto_dispatched, created_at FROM sessions_old;
             DROP TABLE sessions_old;"
        )?;
    }

    // Migrate legacy direct backend → local
    let _ = conn.execute_batch("UPDATE sessions SET backend = 'local' WHERE backend = 'direct'");

    // Track whether a session has been attached at least once (avoids time-based heuristic)
    let _ = conn
        .execute_batch("ALTER TABLE sessions ADD COLUMN attached_once INTEGER NOT NULL DEFAULT 0");

    // Add status_changed_at column (tracks when status last changed, immune to unrelated updates)
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN status_changed_at TEXT");
    let _ = conn.execute_batch(
        "UPDATE sessions SET status_changed_at = updated_at WHERE status_changed_at IS NULL",
    );
    let _ = conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS sessions_status_changed_at
         AFTER UPDATE OF status ON sessions
         BEGIN
           UPDATE sessions SET status_changed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = NEW.id;
         END;",
    );

    // Track which session spawned this one (orchestration / parent-child relationships)
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN parent_session_id TEXT");

    Ok(())
}

// ─── Project types ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub status: String,
    pub prefix: String,
}

// ─── Session types (matches production db::Session) ──────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionRecord {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub tmux_name: Option<String>,
    pub branch: String,
    pub status: String,
    pub created_at: String,
    pub worktree_path: Option<String>,
    pub provider: Option<String>,
    pub backend: String,
    pub provider_session_id: Option<String>,
    pub tab_count: i64,
    pub auto_approve: bool,
    pub task_key: Option<String>,
    pub base_branch: Option<String>,
    pub pr_url: Option<String>,
    pub pr_state: Option<String>,
    pub mru_position: Option<i64>,
    pub auto_dispatched: bool,
    pub attached_once: bool,
    pub parent_session_id: Option<String>,
}

/// Column list matching production SESSION_COLUMNS + mru_position + auto_dispatched.
const SESSION_COLUMNS: &str = "id, project_id, name, tmux_name, branch, status, created_at, worktree_path, provider, backend, provider_session_id, tab_count, auto_approve, task_key, base_branch, pr_url, pr_state, mru_position, auto_dispatched, attached_once, parent_session_id";

fn row_to_session(row: &rusqlite::Row) -> rusqlite::Result<SessionRecord> {
    Ok(SessionRecord {
        id: row.get(0)?,
        project_id: row.get(1)?,
        name: row.get(2)?,
        tmux_name: row.get(3)?,
        branch: row.get(4)?,
        status: row.get(5)?,
        created_at: row.get(6)?,
        worktree_path: row.get(7)?,
        provider: row.get(8)?,
        backend: row.get(9)?,
        provider_session_id: row.get(10)?,
        tab_count: row.get(11)?,
        auto_approve: row.get(12)?,
        task_key: row.get(13)?,
        base_branch: row.get(14)?,
        pr_url: row.get(15)?,
        pr_state: row.get(16)?,
        mru_position: row.get(17)?,
        auto_dispatched: row.get::<_, bool>(18).unwrap_or(false),
        attached_once: row.get::<_, bool>(19).unwrap_or(false),
        parent_session_id: row.get(20)?,
    })
}

#[derive(Debug, Clone, Default)]
pub struct CreateSessionParams {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub tmux_name: Option<String>,
    pub branch: String,
    pub worktree_path: Option<String>,
    pub provider: Option<String>,
    pub backend: String,
    pub auto_approve: bool,
    pub task_key: Option<String>,
    pub base_branch: Option<String>,
    pub auto_dispatched: bool,
    pub parent_session_id: Option<String>,
}

// ─── ProjectService ──────────────────────────────────────────────────────────

pub struct ProjectService;

impl ProjectService {
    /// Find or create a project for the given path. Returns existing if path matches.
    pub fn ensure_project(conn: &Connection, path: &str) -> SqlResult<Project> {
        let existing: Option<Project> = conn
            .prepare(
                "SELECT id, name, path, status, prefix FROM projects WHERE path = ?1 AND status = 'active'",
            )?
            .query_row(params![path], |row| {
                Ok(Project {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    status: row.get(3)?,
                    prefix: row.get(4)?,
                })
            })
            .ok();

        if let Some(p) = existing {
            return Ok(p);
        }

        let name = Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "project".to_string());

        let id = uuid::Uuid::new_v4().to_string();
        let prefix = Self::unique_prefix(conn, &name)?;
        conn.execute(
            "INSERT INTO projects (id, name, path, status, prefix) VALUES (?1, ?2, ?3, 'active', ?4)",
            params![id, name, path, prefix],
        )?;
        Ok(Project {
            id,
            name,
            path: path.to_string(),
            status: "active".to_string(),
            prefix,
        })
    }

    pub fn list_active(conn: &Connection) -> SqlResult<Vec<Project>> {
        let mut stmt = conn.prepare(
            "SELECT id, name, path, status, prefix FROM projects WHERE status = 'active'",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                status: row.get(3)?,
                prefix: row.get(4)?,
            })
        })?;
        rows.collect()
    }

    pub fn get_by_path(conn: &Connection, path: &str) -> SqlResult<Option<Project>> {
        conn.prepare(
            "SELECT id, name, path, status, prefix FROM projects WHERE path = ?1 AND status = 'active'",
        )?
        .query_row(params![path], |row| {
            Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                status: row.get(3)?,
                prefix: row.get(4)?,
            })
        })
        .ok()
        .map_or(Ok(None), |p| Ok(Some(p)))
    }

    pub fn get_by_id(conn: &Connection, id: &str) -> SqlResult<Option<Project>> {
        conn.prepare("SELECT id, name, path, status, prefix FROM projects WHERE id = ?1")?
            .query_row(params![id], |row| {
                Ok(Project {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    status: row.get(3)?,
                    prefix: row.get(4)?,
                })
            })
            .ok()
            .map_or(Ok(None), |p| Ok(Some(p)))
    }

    pub fn create(conn: &Connection, name: &str, path: &str) -> SqlResult<Project> {
        let id = uuid::Uuid::new_v4().to_string();
        let prefix = Self::unique_prefix(conn, name)?;
        conn.execute(
            "INSERT INTO projects (id, name, path, prefix) VALUES (?1, ?2, ?3, ?4)",
            params![id, name, path, prefix],
        )?;
        Ok(Project {
            id,
            name: name.to_string(),
            path: path.to_string(),
            status: "active".to_string(),
            prefix,
        })
    }

    pub fn archive(conn: &Connection, id: &str) -> SqlResult<()> {
        conn.execute(
            "UPDATE sessions SET status = 'archived' WHERE project_id = ?1",
            params![id],
        )?;
        conn.execute(
            "UPDATE projects SET status = 'archived' WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn list_archived(conn: &Connection) -> SqlResult<Vec<Project>> {
        let mut stmt = conn.prepare(
            "SELECT id, name, path, status, prefix FROM projects WHERE status = 'archived'",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                status: row.get(3)?,
                prefix: row.get(4)?,
            })
        })?;
        rows.collect()
    }

    pub fn restore(conn: &Connection, id: &str) -> SqlResult<()> {
        conn.execute(
            "UPDATE projects SET status = 'active' WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn delete(conn: &Connection, id: &str) -> SqlResult<()> {
        conn.execute("DELETE FROM sessions WHERE project_id = ?1", params![id])?;
        conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn name_exists(conn: &Connection, name: &str) -> SqlResult<bool> {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM projects WHERE name = ?1",
            params![name],
            |r| r.get(0),
        )?;
        Ok(count > 0)
    }

    /// Derive a unique prefix for a project name, appending a numeric disambiguator if needed.
    fn unique_prefix(conn: &Connection, name: &str) -> SqlResult<String> {
        let base = planeai_tasks::sqlite::derive_prefix(name);
        let exists = |p: &str| -> SqlResult<bool> {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM projects WHERE prefix = ?1",
                params![p],
                |r| r.get(0),
            )?;
            Ok(count > 0)
        };
        if !exists(&base)? {
            return Ok(base);
        }
        for i in 2..=99 {
            let candidate = format!("{base}{i}");
            if !exists(&candidate)? {
                return Ok(candidate);
            }
        }
        Ok(base) // fallback — shouldn't happen in practice
    }
}

// ─── SessionService ──────────────────────────────────────────────────────────

pub struct SessionService;

impl SessionService {
    /// Create a session record. Used by both Tauri and Iced launch paths.
    pub fn create(conn: &Connection, params: &CreateSessionParams) -> SqlResult<SessionRecord> {
        let created_at = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO sessions (id, project_id, name, tmux_name, branch, status, created_at, worktree_path, provider, backend, auto_approve, task_key, base_branch, auto_dispatched, parent_session_id)
             VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                params.id,
                params.project_id,
                params.name,
                params.tmux_name,
                params.branch,
                created_at,
                params.worktree_path,
                params.provider,
                params.backend,
                params.auto_approve,
                params.task_key,
                params.base_branch,
                params.auto_dispatched,
                params.parent_session_id,
            ],
        )?;
        Ok(SessionRecord {
            id: params.id.clone(),
            project_id: params.project_id.clone(),
            name: params.name.clone(),
            tmux_name: params.tmux_name.clone(),
            branch: params.branch.clone(),
            status: "active".to_string(),
            created_at,
            worktree_path: params.worktree_path.clone(),
            provider: params.provider.clone(),
            backend: params.backend.clone(),
            provider_session_id: None,
            tab_count: 1,
            auto_approve: params.auto_approve,
            task_key: params.task_key.clone(),
            base_branch: params.base_branch.clone(),
            pr_url: None,
            pr_state: None,
            mru_position: None,
            auto_dispatched: params.auto_dispatched,
            attached_once: false,
            parent_session_id: params.parent_session_id.clone(),
        })
    }

    /// List sessions for a project using production filtering/ordering:
    /// - Include active and exited (not archived/destroyed)
    /// - Exclude exited sessions whose task_key is in a 'done' task
    /// - Order by mru_position ASC NULLS LAST, then created_at ASC
    pub fn list_for_project(conn: &Connection, project_id: &str) -> SqlResult<Vec<SessionRecord>> {
        // Use a safe fallback: if the tasks table doesn't exist, skip the task filter.
        let has_tasks_table: bool = conn
            .prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name='tasks'")
            .and_then(|mut s| s.query_row([], |_| Ok(true)))
            .unwrap_or(false);

        let sql = if has_tasks_table {
            format!(
                "SELECT {SESSION_COLUMNS} FROM sessions \
                 WHERE project_id = ?1 AND status IN ('active', 'exited') \
                 AND (status = 'active' OR task_key IS NULL OR task_key NOT IN (SELECT key FROM tasks WHERE status = 'done')) \
                 ORDER BY mru_position ASC NULLS LAST, created_at ASC"
            )
        } else {
            format!(
                "SELECT {SESSION_COLUMNS} FROM sessions \
                 WHERE project_id = ?1 AND status IN ('active', 'exited') \
                 ORDER BY mru_position ASC NULLS LAST, created_at ASC"
            )
        };

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![project_id], row_to_session)?;
        rows.collect()
    }

    /// Update session status.
    pub fn set_status(conn: &Connection, session_id: &str, status: &str) -> SqlResult<()> {
        conn.execute(
            "UPDATE sessions SET status = ?1 WHERE id = ?2",
            params![status, session_id],
        )?;
        Ok(())
    }

    /// Get a single session by ID.
    pub fn get(conn: &Connection, session_id: &str) -> SqlResult<Option<SessionRecord>> {
        let sql = format!("SELECT {SESSION_COLUMNS} FROM sessions WHERE id = ?1");
        conn.prepare(&sql)?
            .query_row(params![session_id], row_to_session)
            .ok()
            .map_or(Ok(None), |s| Ok(Some(s)))
    }

    /// Link the durable log directory for a session (returns expected path).
    pub fn durable_log_dir(session_id: &str) -> Option<PathBuf> {
        std::env::var("PLANEAI_SESSION_LOG_DIR")
            .ok()
            .map(|dir| PathBuf::from(dir).join("sessions").join(session_id))
    }

    /// Check if a durable log exists for a session.
    pub fn has_durable_log(session_id: &str) -> bool {
        Self::durable_log_dir(session_id)
            .map(|d| d.exists())
            .unwrap_or(false)
    }

    /// List all sessions (active + exited, not archived/destroyed, excluding exited sessions
    /// whose task_key is in a 'done' task). Ordered by mru_position ASC NULLS LAST, then created_at ASC.
    pub fn list_active(conn: &Connection) -> SqlResult<Vec<SessionRecord>> {
        let has_tasks_table: bool = conn
            .prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name='tasks'")
            .and_then(|mut s| s.query_row([], |_| Ok(true)))
            .unwrap_or(false);

        let sql = if has_tasks_table {
            format!(
                "SELECT {SESSION_COLUMNS} FROM sessions \
                 WHERE status IN ('active', 'exited') \
                 AND (status = 'active' OR task_key IS NULL OR task_key NOT IN (SELECT key FROM tasks WHERE status = 'done')) \
                 ORDER BY mru_position ASC NULLS LAST, created_at ASC"
            )
        } else {
            format!(
                "SELECT {SESSION_COLUMNS} FROM sessions \
                 WHERE status IN ('active', 'exited') \
                 ORDER BY mru_position ASC NULLS LAST, created_at ASC"
            )
        };
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], row_to_session)?;
        rows.collect()
    }

    /// List archived sessions.
    pub fn list_archived(conn: &Connection) -> SqlResult<Vec<SessionRecord>> {
        let sql = format!("SELECT {SESSION_COLUMNS} FROM sessions WHERE status = 'archived'");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], row_to_session)?;
        rows.collect()
    }

    /// List all sessions for a project regardless of status.
    pub fn list_all_for_project(
        conn: &Connection,
        project_id: &str,
    ) -> SqlResult<Vec<SessionRecord>> {
        let sql = format!("SELECT {SESSION_COLUMNS} FROM sessions WHERE project_id = ?1");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![project_id], row_to_session)?;
        rows.collect()
    }

    /// Archive a session.
    pub fn archive(conn: &Connection, session_id: &str) -> SqlResult<()> {
        conn.execute(
            "UPDATE sessions SET status = 'archived' WHERE id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    /// Destroy (soft-delete) a session.
    pub fn destroy(conn: &Connection, session_id: &str) -> SqlResult<()> {
        conn.execute(
            "UPDATE sessions SET status = 'destroyed' WHERE id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    /// Mark an active session as exited.
    pub fn mark_exited(conn: &Connection, session_id: &str) -> SqlResult<()> {
        conn.execute(
            "UPDATE sessions SET status = 'exited' WHERE id = ?1 AND status = 'active'",
            params![session_id],
        )?;
        Ok(())
    }

    /// Restore a session to active.
    pub fn restore(conn: &Connection, session_id: &str) -> SqlResult<()> {
        conn.execute(
            "UPDATE sessions SET status = 'active' WHERE id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    /// Delete a session record permanently.
    pub fn delete(conn: &Connection, session_id: &str) -> SqlResult<()> {
        conn.execute("DELETE FROM sessions WHERE id = ?1", params![session_id])?;
        Ok(())
    }

    /// Rename a session.
    pub fn rename(conn: &Connection, session_id: &str, name: &str) -> SqlResult<()> {
        conn.execute(
            "UPDATE sessions SET name = ?2 WHERE id = ?1",
            params![session_id, name],
        )?;
        Ok(())
    }

    /// Set provider_session_id.
    pub fn set_provider_session_id(
        conn: &Connection,
        session_id: &str,
        provider_session_id: &str,
    ) -> SqlResult<()> {
        conn.execute(
            "UPDATE sessions SET provider_session_id = ?2 WHERE id = ?1",
            params![session_id, provider_session_id],
        )?;
        Ok(())
    }

    /// Update tab count.
    pub fn update_tab_count(conn: &Connection, session_id: &str, tab_count: i64) -> SqlResult<()> {
        conn.execute(
            "UPDATE sessions SET tab_count = ?2 WHERE id = ?1",
            params![session_id, tab_count],
        )?;
        Ok(())
    }

    /// Save MRU ordering. Clears all positions then sets for the given IDs.
    pub fn save_mru_order(conn: &Connection, session_ids: &[&str]) -> SqlResult<()> {
        let tx = conn.unchecked_transaction()?;
        tx.execute("UPDATE sessions SET mru_position = NULL", [])?;
        for (i, id) in session_ids.iter().enumerate() {
            tx.execute(
                "UPDATE sessions SET mru_position = ?2 WHERE id = ?1",
                params![id, i as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Update PR state.
    pub fn update_pr_state(
        conn: &Connection,
        session_id: &str,
        pr_url: &str,
        pr_state: &str,
    ) -> SqlResult<()> {
        conn.execute(
            "UPDATE sessions SET pr_url = ?1, pr_state = ?2 WHERE id = ?3",
            params![pr_url, pr_state, session_id],
        )?;
        Ok(())
    }

    /// Check if there's an active checkout (non-worktree) session for a project.
    pub fn has_active_checkout(conn: &Connection, project_id: &str) -> SqlResult<bool> {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE project_id = ?1 AND status = 'active' AND worktree_path IS NULL",
            params![project_id],
            |r| r.get(0),
        )?;
        Ok(count > 0)
    }

    /// Mark a session as having been attached at least once.
    pub fn mark_attached(conn: &Connection, session_id: &str) -> SqlResult<()> {
        conn.execute(
            "UPDATE sessions SET attached_once = 1 WHERE id = ?1 AND attached_once = 0",
            params![session_id],
        )?;
        Ok(())
    }

    /// Return direct child sessions of the given parent session ID.
    /// Includes all statuses (active, exited, archived, destroyed) for observability.
    pub fn children(conn: &Connection, parent_id: &str) -> SqlResult<Vec<SessionRecord>> {
        let sql = format!(
            "SELECT {SESSION_COLUMNS} FROM sessions WHERE parent_session_id = ?1 ORDER BY created_at ASC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![parent_id], row_to_session)?;
        rows.collect()
    }

    /// Return the full session tree rooted at the given session ID.
    /// Walks up to the root (session with no parent_session_id, or whose parent doesn't exist),
    /// then returns all descendants in a flat list (root first, then BFS order).
    pub fn tree(conn: &Connection, session_id: &str) -> SqlResult<Vec<SessionRecord>> {
        let root_id = Self::find_root(conn, session_id)?;

        let mut result = Vec::new();
        let mut queue = std::collections::VecDeque::new();

        if let Some(root) = Self::get(conn, &root_id)? {
            queue.push_back(root.id.clone());
            result.push(root);
        }

        while let Some(current_id) = queue.pop_front() {
            let children = Self::children(conn, &current_id)?;
            for child in children {
                queue.push_back(child.id.clone());
                result.push(child);
            }
        }

        Ok(result)
    }

    /// Walk parent_session_id links upward to find the root of the tree.
    /// Stops when a session has no parent or its parent doesn't exist in the DB.
    fn find_root(conn: &Connection, session_id: &str) -> SqlResult<String> {
        let mut current = session_id.to_string();
        let mut current_session = Self::get(conn, &current)?;
        for _ in 0..100 {
            match current_session {
                Some(session) => match session.parent_session_id {
                    Some(ref parent_id) => match Self::get(conn, parent_id)? {
                        Some(parent) => {
                            current = parent_id.clone();
                            current_session = Some(parent);
                        }
                        None => break,
                    },
                    None => break,
                },
                None => break,
            }
        }
        Ok(current)
    }
}

// ─── WorktreeMode ────────────────────────────────────────────────────────────

/// Explicit worktree launch mode — shared by Tauri, Iced, and CLI.
#[derive(Debug, Clone, PartialEq)]
pub enum WorktreeMode {
    /// Launch in the project root directory (no worktree).
    None,
    /// Use an existing worktree at a known path.
    Existing {
        path: PathBuf,
        branch_name: Option<String>,
    },
    /// Create a new worktree off a base branch.
    Create {
        base_project_path: PathBuf,
        branch_name: String,
        task_key: Option<String>,
    },
}

/// Result of resolving a WorktreeMode for session launch.
#[derive(Debug, Clone)]
pub struct ResolvedWorktree {
    pub cwd: PathBuf,
    pub worktree_path: Option<String>,
    pub branch_name: String,
    pub base_branch: Option<String>,
}

// ─── WorktreeService ─────────────────────────────────────────────────────────

pub struct WorktreeService;

impl WorktreeService {
    /// Returns the worktree root for a project.
    /// Uses `PLANEAI_WORKTREE_ROOT` env if set (for testing), else `$HOME/.planeai/worktrees`.
    pub fn worktree_root(project_name: &str) -> PathBuf {
        if let Ok(root) = std::env::var("PLANEAI_WORKTREE_ROOT") {
            return PathBuf::from(root).join(project_name);
        }
        let home = std::env::var("HOME").unwrap_or_default();
        PathBuf::from(home)
            .join(".planeai")
            .join("worktrees")
            .join(project_name)
    }

    /// Generate a worktree path for a session.
    pub fn worktree_path(project_name: &str, short_id: &str) -> PathBuf {
        Self::worktree_root(project_name).join(short_id)
    }

    /// Generate a branch name from task key and short id.
    /// If parent_key is provided, it is used instead of task_key for the branch prefix.
    pub fn branch_name(task_key: &str, parent_key: Option<&str>, short_id: &str) -> String {
        let key = parent_key
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(task_key);
        format!("{}/{}", key.to_lowercase().replace(' ', "-"), short_id)
    }

    /// Compute the short_id from a session UUID (first 8 hex chars, dashes removed).
    pub fn short_id(session_id: &str) -> String {
        session_id.replace('-', "")[..8].to_string()
    }

    /// Validate a branch name (no spaces, no .., no control chars, non-empty).
    pub fn validate_branch_name(branch: &str) -> Result<(), String> {
        if branch.is_empty() {
            return Err("branch name cannot be empty".to_string());
        }
        if branch.contains("..") {
            return Err("branch name cannot contain '..'".to_string());
        }
        if branch.contains(' ') {
            return Err("branch name cannot contain spaces".to_string());
        }
        if branch.chars().any(|c| c.is_control()) {
            return Err("branch name cannot contain control characters".to_string());
        }
        if branch.starts_with('-') {
            return Err("branch name cannot start with '-'".to_string());
        }
        if branch.ends_with('.') || branch.ends_with('/') {
            return Err("branch name cannot end with '.' or '/'".to_string());
        }
        Ok(())
    }

    /// Check if a worktree path already exists on disk.
    pub fn worktree_exists(project_name: &str, short_id: &str) -> bool {
        Self::worktree_path(project_name, short_id).exists()
    }

    /// Resolve a WorktreeMode into a concrete cwd and metadata for session creation.
    ///
    /// For `WorktreeMode::Create`, this calls `git worktree add` and returns
    /// the resolved worktree path as the session cwd.
    pub fn resolve_worktree(
        mode: &WorktreeMode,
        project_name: &str,
        project_path: &Path,
        session_id: &str,
        base_branch: &str,
    ) -> Result<ResolvedWorktree, String> {
        match mode {
            WorktreeMode::None => Ok(ResolvedWorktree {
                cwd: project_path.to_path_buf(),
                worktree_path: None,
                branch_name: String::new(),
                base_branch: None,
            }),
            WorktreeMode::Existing { path, branch_name } => {
                if !path.is_dir() {
                    return Err(format!(
                        "existing worktree path does not exist: {}",
                        path.display()
                    ));
                }
                Ok(ResolvedWorktree {
                    cwd: path.clone(),
                    worktree_path: Some(path.to_string_lossy().to_string()),
                    branch_name: branch_name.clone().unwrap_or_default(),
                    base_branch: None,
                })
            }
            WorktreeMode::Create {
                base_project_path,
                branch_name,
                task_key: _,
            } => {
                Self::validate_branch_name(branch_name)?;
                let short_id = Self::short_id(session_id);
                let wt_path = Self::worktree_path(project_name, &short_id);
                let wt_path_str = wt_path.to_string_lossy().to_string();

                // Manual branch input is used exactly. task_key is metadata only.
                let final_branch = branch_name.clone();

                let repo_path = base_project_path.to_string_lossy();
                crate::git::worktree_add(&repo_path, &wt_path_str, &final_branch, base_branch)?;

                Ok(ResolvedWorktree {
                    cwd: wt_path,
                    worktree_path: Some(wt_path_str),
                    branch_name: final_branch,
                    base_branch: Some(base_branch.to_string()),
                })
            }
        }
    }
}

// ─── TaskService ─────────────────────────────────────────────────────────────

pub struct TaskService;

impl TaskService {
    /// Check if a session has a linked task.
    pub fn session_task_key(conn: &Connection, session_id: &str) -> SqlResult<Option<String>> {
        conn.prepare("SELECT task_key FROM sessions WHERE id = ?1")?
            .query_row(params![session_id], |row| row.get::<_, Option<String>>(0))
            .or(Ok(None))
    }

    /// List tasks for a project prefix (non-done tasks). Uses planeai_tasks SqliteRepository.
    pub fn list_for_project(
        db_path: &Path,
        prefix: &str,
    ) -> Result<Vec<planeai_tasks::model::Task>, String> {
        let repo =
            planeai_tasks::sqlite::SqliteRepository::open(db_path.to_str().unwrap_or(""), prefix)
                .map_err(|e| e.to_string())?;
        use planeai_tasks::provider::TaskProvider;
        repo.list(planeai_tasks::model::ListFilter {
            exclude_status: Some(planeai_tasks::model::Status::Done),
            ..Default::default()
        })
        .map_err(|e| e.to_string())
    }

    /// Get a single task by key.
    pub fn get_task(
        db_path: &Path,
        prefix: &str,
        key: &str,
    ) -> Result<planeai_tasks::model::Task, String> {
        let repo =
            planeai_tasks::sqlite::SqliteRepository::open(db_path.to_str().unwrap_or(""), prefix)
                .map_err(|e| e.to_string())?;
        use planeai_tasks::provider::TaskProvider;
        repo.get(key).map_err(|e| e.to_string())
    }

    /// Resolve the task prompt from task title + description using a template.
    /// Template uses {key}, {title}, {description} placeholders.
    pub fn resolve_task_prompt(
        task: &planeai_tasks::model::Task,
        template: Option<&str>,
    ) -> String {
        let tmpl = template.unwrap_or("{title}\n\n{description}");
        let mut vars = std::collections::HashMap::new();
        vars.insert("key", task.key.as_str());
        vars.insert("title", task.title.as_str());
        vars.insert("description", task.description.as_str());
        crate::template::render(tmpl, &vars)
    }

    /// Link a session to a task and optionally move task to a new status (on_start hook).
    pub fn fire_lifecycle_hook(
        db_path: &Path,
        prefix: &str,
        task_key: &str,
        move_to: &str,
    ) -> Result<(), String> {
        let status = planeai_tasks::model::Status::parse(move_to)
            .ok_or_else(|| format!("invalid status: {move_to}"))?;
        let repo =
            planeai_tasks::sqlite::SqliteRepository::open(db_path.to_str().unwrap_or(""), prefix)
                .map_err(|e| e.to_string())?;
        use planeai_tasks::provider::TaskProvider;
        repo.update(
            task_key,
            planeai_tasks::model::UpdateParams {
                status: Some(status),
                ..Default::default()
            },
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Resolve a task launch request into everything needed to spawn the session.
    /// Does NOT spawn — only resolves config, worktree mode, prompt, and command.
    pub fn resolve_task_launch(
        request: &TaskLaunchRequest,
        config: &crate::session_launch::LaunchConfig,
        prompt_template: Option<&str>,
    ) -> Result<(crate::session_launch::ResolvedLaunchConfig, WorktreeMode), String> {
        let task = planeai_tasks::model::Task {
            key: request.task_key.clone(),
            title: request.task_title.clone(),
            description: request.task_description.clone(),
            status: planeai_tasks::model::Status::Todo,
            priority: 0,
            parent_key: request.parent_key.clone(),
            blocked_by: Vec::new(),
            tags: Vec::new(),
            base_branch: request.task_base_branch.clone(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let prompt = Self::resolve_task_prompt(&task, prompt_template);

        let overrides = crate::session_launch::SessionLaunchOverrides {
            cwd: Some(request.project_path.clone()),
            provider_id: request.provider_id.clone(),
            auto_approve: request.auto_approve,
            task_prompt: Some(prompt),
            autonomous: request.autonomous,
            cols: Some(request.cols),
            rows: Some(request.rows),
            ..Default::default()
        };

        let resolved = crate::session_launch::resolve_from_config(config, &overrides)
            .map_err(|e| e.to_string())?;

        let short_id = WorktreeService::short_id(&resolved.request.session_id);
        let branch_name = WorktreeService::branch_name(
            &request.task_key,
            request.parent_key.as_deref(),
            &short_id,
        );

        let worktree_mode = WorktreeMode::Create {
            base_project_path: request.project_path.clone(),
            branch_name,
            task_key: Some(request.task_key.clone()),
        };

        Ok((resolved, worktree_mode))
    }
}

// ─── Task-driven launch types ────────────────────────────────────────────────

/// Request to launch a session from a task.
#[derive(Debug, Clone)]
pub struct TaskLaunchRequest {
    pub project_id: String,
    pub project_name: String,
    pub project_path: PathBuf,
    pub task_key: String,
    pub task_title: String,
    pub task_description: String,
    pub task_base_branch: String,
    pub parent_key: Option<String>,
    pub provider_id: Option<String>,
    pub auto_approve: bool,
    pub autonomous: bool,
    pub cols: u16,
    pub rows: u16,
}

/// Result of a task-driven session launch.
#[derive(Debug, Clone)]
pub struct TaskLaunchResult {
    pub task_key: String,
    pub session_id: String,
    pub project_id: String,
    pub worktree_path: Option<String>,
    pub branch_name: String,
    pub command_label: String,
    pub log_path: Option<PathBuf>,
    pub prompt_was_injected: bool,
    pub auto_approve_was_applied: bool,
}

// ─── Layout persistence ──────────────────────────────────────────────────────

pub struct LayoutService;

impl LayoutService {
    pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS layout_state (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
            [],
        )?;
        Ok(())
    }

    pub fn get(conn: &Connection, key: &str, default: f32) -> f32 {
        conn.query_row(
            "SELECT value FROM layout_state WHERE key = ?1",
            [key],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(default)
    }

    pub fn set(conn: &Connection, key: &str, value: f32) {
        let _ = conn.execute(
            "INSERT OR REPLACE INTO layout_state (key, value) VALUES (?1, ?2)",
            rusqlite::params![key, format!("{}", value)],
        );
    }
}

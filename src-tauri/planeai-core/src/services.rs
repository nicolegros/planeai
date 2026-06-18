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

/// Idempotent migration — identical logic to production src-tauri/src/db.rs::migrate().
/// Safe to run on fresh DBs and existing production DBs alike.
pub fn migrate(conn: &Connection) -> SqlResult<()> {
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
    // Idempotent column additions (same as production db.rs)
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
    // Migrate direct backend → daemon
    let _ = conn.execute_batch("UPDATE sessions SET backend = 'daemon' WHERE backend = 'direct'");
    Ok(())
}

// ─── Project types ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub status: String,
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
}

/// Column list matching production SESSION_COLUMNS + mru_position + auto_dispatched.
const SESSION_COLUMNS: &str = "id, project_id, name, tmux_name, branch, status, created_at, worktree_path, provider, backend, provider_session_id, tab_count, auto_approve, task_key, base_branch, pr_url, pr_state, mru_position, auto_dispatched";

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
}

// ─── ProjectService ──────────────────────────────────────────────────────────

pub struct ProjectService;

impl ProjectService {
    /// Find or create a project for the given path. Returns existing if path matches.
    pub fn ensure_project(conn: &Connection, path: &str) -> SqlResult<Project> {
        let existing: Option<Project> = conn
            .prepare(
                "SELECT id, name, path, status FROM projects WHERE path = ?1 AND status = 'active'",
            )?
            .query_row(params![path], |row| {
                Ok(Project {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    status: row.get(3)?,
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
        conn.execute(
            "INSERT INTO projects (id, name, path, status) VALUES (?1, ?2, ?3, 'active')",
            params![id, name, path],
        )?;
        Ok(Project {
            id,
            name,
            path: path.to_string(),
            status: "active".to_string(),
        })
    }

    pub fn list_active(conn: &Connection) -> SqlResult<Vec<Project>> {
        let mut stmt =
            conn.prepare("SELECT id, name, path, status FROM projects WHERE status = 'active'")?;
        let rows = stmt.query_map([], |row| {
            Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                status: row.get(3)?,
            })
        })?;
        rows.collect()
    }

    pub fn get_by_path(conn: &Connection, path: &str) -> SqlResult<Option<Project>> {
        conn.prepare(
            "SELECT id, name, path, status FROM projects WHERE path = ?1 AND status = 'active'",
        )?
        .query_row(params![path], |row| {
            Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                status: row.get(3)?,
            })
        })
        .ok()
        .map_or(Ok(None), |p| Ok(Some(p)))
    }
}

// ─── SessionService ──────────────────────────────────────────────────────────

pub struct SessionService;

impl SessionService {
    /// Create a session record. Used by both Tauri and Iced launch paths.
    pub fn create(conn: &Connection, params: &CreateSessionParams) -> SqlResult<SessionRecord> {
        let created_at = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO sessions (id, project_id, name, tmux_name, branch, status, created_at, worktree_path, provider, backend, auto_approve, task_key, base_branch, auto_dispatched)
             VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
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
}

// ─── WorktreeService ─────────────────────────────────────────────────────────

pub struct WorktreeService;

impl WorktreeService {
    /// Returns the worktree root for a project.
    pub fn worktree_root(project_name: &str) -> PathBuf {
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
    pub fn branch_name(task_key: &str, short_id: &str) -> String {
        format!("{}/{}", task_key.to_lowercase().replace(' ', "-"), short_id)
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
}

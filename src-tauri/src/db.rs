use rusqlite::{params, Connection, Result, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
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
}

/// Column list for SELECT statements returning a Session.
/// Keep in sync with `row_to_session`.
pub const SESSION_COLUMNS: &str = "id, project_id, name, tmux_name, branch, status, created_at, worktree_path, provider, backend, provider_session_id, tab_count, auto_approve, task_key, base_branch, pr_url, pr_state";

/// Map a row (selected with SESSION_COLUMNS) to a Session struct.
pub fn row_to_session(row: &Row) -> rusqlite::Result<Session> {
    Ok(Session {
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
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub terminal_theme_dark: String,
    pub terminal_theme_light: String,
    pub font_size: u32,
    pub font_family: String,
    pub appearance_mode: String,
}

pub fn get_settings(conn: &Connection) -> Result<Settings> {
    conn.query_row(
        "SELECT terminal_theme_dark, terminal_theme_light, font_size, font_family, appearance_mode FROM settings WHERE id = 1",
        [],
        |row| Ok(Settings {
            terminal_theme_dark: row.get(0)?,
            terminal_theme_light: row.get(1)?,
            font_size: row.get(2)?,
            font_family: row.get(3)?,
            appearance_mode: row.get(4)?,
        }),
    )
}

#[allow(dead_code)]
pub fn update_settings(conn: &Connection, settings: &Settings) -> Result<()> {
    conn.execute(
        "UPDATE settings SET terminal_theme_dark = ?1, terminal_theme_light = ?2, font_size = ?3, font_family = ?4, appearance_mode = ?5 WHERE id = 1",
        params![settings.terminal_theme_dark, settings.terminal_theme_light, settings.font_size, settings.font_family, settings.appearance_mode],
    )?;
    Ok(())
}

pub fn migrate(conn: &Connection) -> Result<()> {
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
        );
        CREATE TABLE IF NOT EXISTS settings (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            terminal_theme_dark TEXT NOT NULL DEFAULT 'one-dark',
            terminal_theme_light TEXT NOT NULL DEFAULT 'one-light',
            font_size INTEGER NOT NULL DEFAULT 14,
            font_family TEXT NOT NULL DEFAULT 'Menlo',
            appearance_mode TEXT NOT NULL DEFAULT 'system'
        );
        INSERT OR IGNORE INTO settings (id) VALUES (1);",
    )?;
    // Add name column to existing databases
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN name TEXT NOT NULL DEFAULT ''");
    // Add worktree_path column
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN worktree_path TEXT");
    // Add font_family column
    let _ = conn
        .execute_batch("ALTER TABLE settings ADD COLUMN font_family TEXT NOT NULL DEFAULT 'Menlo'");
    // Migrate old settings schema
    let _ = conn.execute_batch(
        "ALTER TABLE settings ADD COLUMN terminal_theme_dark TEXT NOT NULL DEFAULT 'one-dark'",
    );
    let _ = conn.execute_batch(
        "ALTER TABLE settings ADD COLUMN terminal_theme_light TEXT NOT NULL DEFAULT 'one-light'",
    );
    let _ = conn.execute_batch(
        "ALTER TABLE settings ADD COLUMN appearance_mode TEXT NOT NULL DEFAULT 'system'",
    );
    // Copy old terminal_theme to terminal_theme_dark if it existed
    let _ = conn.execute_batch(
        "UPDATE settings SET terminal_theme_dark = terminal_theme WHERE terminal_theme IS NOT NULL",
    );
    // Add provider column to sessions
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN provider TEXT");
    // Add backend column (defaults to 'tmux' for existing sessions)
    let _ =
        conn.execute_batch("ALTER TABLE sessions ADD COLUMN backend TEXT NOT NULL DEFAULT 'tmux'");
    // Add provider_session_id column
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN provider_session_id TEXT");
    // Add tab_count column
    let _ =
        conn.execute_batch("ALTER TABLE sessions ADD COLUMN tab_count INTEGER NOT NULL DEFAULT 1");
    // Add auto_approve column (defaults to true for existing sessions)
    let _ = conn
        .execute_batch("ALTER TABLE sessions ADD COLUMN auto_approve INTEGER NOT NULL DEFAULT 1");

    // Migrate tmux_name from NOT NULL to nullable for existing databases created before dual-backend.
    // SQLite doesn't support ALTER COLUMN, so we rebuild the table.
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
                 auto_approve INTEGER NOT NULL DEFAULT 1
             );
             INSERT INTO sessions (id, project_id, name, tmux_name, branch, status, created_at, worktree_path, provider, backend, provider_session_id, tab_count)
                 SELECT id, project_id, name, tmux_name, branch, status, created_at, worktree_path, provider, backend, provider_session_id, tab_count FROM sessions_old;
             DROP TABLE sessions_old;"
        )?;
    }

    // Add status column to projects
    let _ =
        conn.execute_batch("ALTER TABLE projects ADD COLUMN status TEXT NOT NULL DEFAULT 'active'");

    // Add task_key column to sessions (nullable — only set for task-linked sessions)
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN task_key TEXT");

    // Add base_branch column (nullable — NULL for existing sessions, detected on demand)
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN base_branch TEXT");

    // Add mru_position column (nullable — NULL means "not yet ordered")
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN mru_position INTEGER");

    // Add pr_url and pr_state columns for PR integration
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN pr_url TEXT");
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN pr_state TEXT");

    // Add auto_dispatched column (0 for manually created sessions)
    let _ = conn.execute_batch(
        "ALTER TABLE sessions ADD COLUMN auto_dispatched INTEGER NOT NULL DEFAULT 0",
    );

    // Add auto_mode and task_manager columns to projects (for symphony orchestrator)
    let _ =
        conn.execute_batch("ALTER TABLE projects ADD COLUMN auto_mode INTEGER NOT NULL DEFAULT 0");
    let _ = conn.execute_batch("ALTER TABLE projects ADD COLUMN task_manager TEXT");

    Ok(())
}

// Project CRUD

pub fn create_project(conn: &Connection, name: &str, path: &str) -> Result<Project> {
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO projects (id, name, path) VALUES (?1, ?2, ?3)",
        params![id, name, path],
    )?;
    Ok(Project {
        id,
        name: name.to_string(),
        path: path.to_string(),
    })
}

pub fn list_projects(conn: &Connection) -> Result<Vec<Project>> {
    let mut stmt = conn.prepare("SELECT id, name, path FROM projects WHERE status = 'active'")?;
    let rows = stmt.query_map([], |row| {
        Ok(Project {
            id: row.get(0)?,
            name: row.get(1)?,
            path: row.get(2)?,
        })
    })?;
    rows.collect()
}

pub fn archive_project(conn: &Connection, id: &str) -> Result<()> {
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

pub fn list_archived_projects(conn: &Connection) -> Result<Vec<Project>> {
    let mut stmt = conn.prepare("SELECT id, name, path FROM projects WHERE status = 'archived'")?;
    let rows = stmt.query_map([], |row| {
        Ok(Project {
            id: row.get(0)?,
            name: row.get(1)?,
            path: row.get(2)?,
        })
    })?;
    rows.collect()
}

pub fn restore_project(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE projects SET status = 'active' WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

pub fn delete_project(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM sessions WHERE project_id = ?1", params![id])?;
    conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn get_project(conn: &Connection, id: &str) -> Result<Option<Project>> {
    let mut stmt = conn.prepare("SELECT id, name, path FROM projects WHERE id = ?1")?;
    let mut rows = stmt.query_map(params![id], |row| {
        Ok(Project {
            id: row.get(0)?,
            name: row.get(1)?,
            path: row.get(2)?,
        })
    })?;
    rows.next().transpose()
}

pub fn get_project_sessions(conn: &Connection, project_id: &str) -> Result<Vec<Session>> {
    let sql = format!("SELECT {SESSION_COLUMNS} FROM sessions WHERE project_id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![project_id], row_to_session)?;
    rows.collect()
}

// Session CRUD

pub fn create_session(
    conn: &Connection,
    project_id: &str,
    name: &str,
    tmux_name: &str,
    branch: &str,
    worktree_path: Option<&str>,
) -> Result<Session> {
    let id = uuid::Uuid::new_v4().to_string();
    create_session_with_id(
        conn,
        &id,
        project_id,
        name,
        Some(tmux_name),
        branch,
        worktree_path,
        None,
        "tmux",
        true,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn create_session_with_id(
    conn: &Connection,
    id: &str,
    project_id: &str,
    name: &str,
    tmux_name: Option<&str>,
    branch: &str,
    worktree_path: Option<&str>,
    provider: Option<&str>,
    backend: &str,
    auto_approve: bool,
    task_key: Option<&str>,
    base_branch: Option<&str>,
) -> Result<Session> {
    let created_at = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO sessions (id, project_id, name, tmux_name, branch, status, created_at, worktree_path, provider, backend, auto_approve, task_key, base_branch) VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![id, project_id, name, tmux_name, branch, created_at, worktree_path, provider, backend, auto_approve, task_key, base_branch],
    )?;
    Ok(Session {
        id: id.to_string(),
        project_id: project_id.to_string(),
        name: name.to_string(),
        tmux_name: tmux_name.map(|s| s.to_string()),
        branch: branch.to_string(),
        status: "active".to_string(),
        created_at,
        worktree_path: worktree_path.map(|s| s.to_string()),
        provider: provider.map(|s| s.to_string()),
        backend: backend.to_string(),
        provider_session_id: None,
        tab_count: 1,
        auto_approve,
        task_key: task_key.map(|s| s.to_string()),
        base_branch: base_branch.map(|s| s.to_string()),
        pr_url: None,
        pr_state: None,
    })
}

pub fn list_sessions(conn: &Connection) -> Result<Vec<Session>> {
    let sql = format!(
        "SELECT {SESSION_COLUMNS} FROM sessions WHERE status IN ('active', 'exited') \
         AND (task_key IS NULL OR task_key NOT IN (SELECT key FROM tasks WHERE status = 'done')) \
         ORDER BY mru_position ASC NULLS LAST, created_at ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_session)?;
    rows.collect()
}

pub fn list_archived_sessions(conn: &Connection) -> Result<Vec<Session>> {
    let sql = format!("SELECT {SESSION_COLUMNS} FROM sessions WHERE status = 'archived'");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_session)?;
    rows.collect()
}

pub fn archive_session(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET status = 'archived' WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

pub fn delete_session(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn destroy_session(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET status = 'destroyed' WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

pub fn mark_session_exited(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET status = 'exited' WHERE id = ?1 AND status = 'active'",
        params![id],
    )?;
    Ok(())
}

pub fn restore_session(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET status = 'active' WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

#[allow(dead_code)]
pub fn has_active_checkout_session(conn: &Connection, project_id: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sessions WHERE project_id = ?1 AND status = 'active' AND worktree_path IS NULL",
        params![project_id],
        |r| r.get(0),
    )?;
    Ok(count > 0)
}

pub fn project_name_exists(conn: &Connection, name: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM projects WHERE name = ?1",
        params![name],
        |r| r.get(0),
    )?;
    Ok(count > 0)
}

pub fn rename_session(conn: &Connection, id: &str, name: &str) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET name = ?2 WHERE id = ?1",
        params![id, name],
    )?;
    Ok(())
}

pub fn set_provider_session_id(
    conn: &Connection,
    id: &str,
    provider_session_id: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET provider_session_id = ?2 WHERE id = ?1",
        params![id, provider_session_id],
    )?;
    Ok(())
}

pub fn update_tab_count(conn: &Connection, id: &str, tab_count: i64) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET tab_count = ?2 WHERE id = ?1",
        params![id, tab_count],
    )?;
    Ok(())
}

pub fn save_mru_order(conn: &Connection, session_ids: &[&str]) -> Result<()> {
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

pub fn get_session(conn: &Connection, id: &str) -> Result<Option<Session>> {
    let sql = format!("SELECT {SESSION_COLUMNS} FROM sessions WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![id], row_to_session)?;
    rows.next().transpose()
}

pub fn update_pr_state(conn: &Connection, id: &str, pr_url: &str, pr_state: &str) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET pr_url = ?1, pr_state = ?2 WHERE id = ?3",
        params![pr_url, pr_state, id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        planeai_tasks::sqlite::migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn test_migrate_creates_tables() {
        let conn = setup();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('projects', 'sessions')",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_migration_adds_backend_column_defaulting_to_tmux() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let s = create_session(&conn, &p.id, "test", "planeai-myapp-aaa", "main", None).unwrap();
        assert_eq!(s.backend, "tmux");
    }

    #[test]
    fn test_create_direct_session_with_null_tmux_name() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let s = create_session_with_id(
            &conn,
            "sess-1",
            &p.id,
            "direct session",
            None,
            "main",
            None,
            None,
            "direct",
            true,
            None,
            None,
        )
        .unwrap();
        assert_eq!(s.backend, "direct");
        assert!(s.tmux_name.is_none());
        // Verify round-trip through DB
        let loaded = get_session(&conn, "sess-1").unwrap().unwrap();
        assert_eq!(loaded.backend, "direct");
        assert!(loaded.tmux_name.is_none());
    }

    #[test]
    fn test_mark_session_exited() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let s = create_session(&conn, &p.id, "agent", "planeai-myapp-aaa", "main", None).unwrap();
        assert_eq!(s.status, "active");
        mark_session_exited(&conn, &s.id).unwrap();
        let loaded = get_session(&conn, &s.id).unwrap().unwrap();
        assert_eq!(loaded.status, "exited");
    }

    #[test]
    fn test_list_sessions_includes_exited() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let s1 = create_session(
            &conn,
            &p.id,
            "active one",
            "planeai-myapp-aaa",
            "main",
            None,
        )
        .unwrap();
        let s2 = create_session(
            &conn,
            &p.id,
            "exited one",
            "planeai-myapp-bbb",
            "feat",
            None,
        )
        .unwrap();
        mark_session_exited(&conn, &s2.id).unwrap();
        let sessions = list_sessions(&conn).unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(sessions
            .iter()
            .any(|s| s.id == s1.id && s.status == "active"));
        assert!(sessions
            .iter()
            .any(|s| s.id == s2.id && s.status == "exited"));
    }

    #[test]
    fn test_create_and_list_projects() {
        let conn = setup();
        create_project(&conn, "myapp", "/home/user/myapp").unwrap();
        create_project(&conn, "other", "/home/user/other").unwrap();
        let projects = list_projects(&conn).unwrap();
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].name, "myapp");
    }

    #[test]
    fn test_delete_project_cascades_sessions() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        create_session(
            &conn,
            &p.id,
            "main session",
            "planeai-myapp-abc123",
            "main",
            None,
        )
        .unwrap();
        delete_project(&conn, &p.id).unwrap();
        assert_eq!(list_projects(&conn).unwrap().len(), 0);
        assert_eq!(list_sessions(&conn).unwrap().len(), 0);
    }

    #[test]
    fn test_create_and_list_sessions() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        create_session(
            &conn,
            &p.id,
            "feat session",
            "planeai-myapp-aaa",
            "feat-x",
            None,
        )
        .unwrap();
        let sessions = list_sessions(&conn).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "feat session");
        assert_eq!(sessions[0].branch, "feat-x");
        assert_eq!(sessions[0].status, "active");
        assert!(sessions[0].worktree_path.is_none());
    }

    #[test]
    fn test_delete_session() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let s =
            create_session(&conn, &p.id, "to delete", "planeai-myapp-bbb", "main", None).unwrap();
        delete_session(&conn, &s.id).unwrap();
        assert_eq!(list_sessions(&conn).unwrap().len(), 0);
    }

    #[test]
    fn test_worktree_session() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let s = create_session(
            &conn,
            &p.id,
            "wt session",
            "planeai-myapp-ccc",
            "feat-wt",
            Some("/home/.planeai/worktrees/myapp/ccc"),
        )
        .unwrap();
        assert_eq!(
            s.worktree_path.as_deref(),
            Some("/home/.planeai/worktrees/myapp/ccc")
        );
        let sessions = list_sessions(&conn).unwrap();
        assert_eq!(
            sessions[0].worktree_path.as_deref(),
            Some("/home/.planeai/worktrees/myapp/ccc")
        );
    }

    #[test]
    fn test_has_active_checkout_session() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        assert!(!has_active_checkout_session(&conn, &p.id).unwrap());
        create_session(&conn, &p.id, "s1", "planeai-myapp-aaa", "main", None).unwrap();
        assert!(has_active_checkout_session(&conn, &p.id).unwrap());
        // Worktree session doesn't count
        let conn2 = setup();
        let p2 = create_project(&conn2, "myapp2", "/tmp/myapp2").unwrap();
        create_session(
            &conn2,
            &p2.id,
            "wt",
            "planeai-myapp2-bbb",
            "feat",
            Some("/tmp/wt"),
        )
        .unwrap();
        assert!(!has_active_checkout_session(&conn2, &p2.id).unwrap());
    }

    #[test]
    fn test_project_name_exists() {
        let conn = setup();
        assert!(!project_name_exists(&conn, "myapp").unwrap());
        create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        assert!(project_name_exists(&conn, "myapp").unwrap());
    }

    #[test]
    fn test_rename_session() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let s =
            create_session(&conn, &p.id, "old name", "planeai-myapp-aaa", "main", None).unwrap();
        rename_session(&conn, &s.id, "new name").unwrap();
        let updated = get_session(&conn, &s.id).unwrap().unwrap();
        assert_eq!(updated.name, "new name");
    }

    #[test]
    fn test_list_archived_sessions() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        create_session(
            &conn,
            &p.id,
            "active one",
            "planeai-myapp-aaa",
            "main",
            None,
        )
        .unwrap();
        let s2 = create_session(
            &conn,
            &p.id,
            "archived one",
            "planeai-myapp-bbb",
            "feat",
            None,
        )
        .unwrap();
        archive_session(&conn, &s2.id).unwrap();
        let archived = list_archived_sessions(&conn).unwrap();
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].name, "archived one");
    }

    #[test]
    fn test_list_sessions_excludes_archived_and_destroyed() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        create_session(&conn, &p.id, "active", "planeai-myapp-aaa", "main", None).unwrap();
        let s2 =
            create_session(&conn, &p.id, "archived", "planeai-myapp-bbb", "feat", None).unwrap();
        archive_session(&conn, &s2.id).unwrap();
        let s3 =
            create_session(&conn, &p.id, "destroyed", "planeai-myapp-ccc", "fix", None).unwrap();
        destroy_session(&conn, &s3.id).unwrap();
        let sessions = list_sessions(&conn).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "active");
    }

    #[test]
    fn test_orphan_cleanup_soft_deletes() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let s = create_session(&conn, &p.id, "orphan", "planeai-myapp-dead", "main", None).unwrap();
        // Simulate orphan cleanup: tmux is dead, so we destroy
        destroy_session(&conn, &s.id).unwrap();
        // Not in active list
        assert_eq!(list_sessions(&conn).unwrap().len(), 0);
        // Still in DB
        let row = get_session(&conn, &s.id).unwrap().unwrap();
        assert_eq!(row.status, "destroyed");
    }

    #[test]
    fn test_restore_session() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let s = create_session(
            &conn,
            &p.id,
            "to restore",
            "planeai-myapp-aaa",
            "main",
            None,
        )
        .unwrap();
        archive_session(&conn, &s.id).unwrap();
        assert_eq!(list_sessions(&conn).unwrap().len(), 0);
        restore_session(&conn, &s.id).unwrap();
        let sessions = list_sessions(&conn).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].status, "active");
    }

    #[test]
    fn test_provider_session_id_round_trips_through_create_and_get() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let s = create_session_with_id(
            &conn,
            "s1",
            &p.id,
            "test",
            None,
            "main",
            None,
            Some("kiro"),
            "direct",
            true,
            None,
            None,
        )
        .unwrap();
        // Initially null
        assert_eq!(s.provider_session_id, None);
        // Set it
        set_provider_session_id(&conn, "s1", "f4165541-f370-4fdd-9ccd-14b103a4f712").unwrap();
        let loaded = get_session(&conn, "s1").unwrap().unwrap();
        assert_eq!(
            loaded.provider_session_id,
            Some("f4165541-f370-4fdd-9ccd-14b103a4f712".to_string())
        );
    }

    #[test]
    fn test_archive_project_hides_from_list_projects() {
        let conn = setup();
        let p1 = create_project(&conn, "keep", "/tmp/keep").unwrap();
        let p2 = create_project(&conn, "hide", "/tmp/hide").unwrap();
        archive_project(&conn, &p2.id).unwrap();
        let projects = list_projects(&conn).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, p1.id);
    }

    #[test]
    fn test_list_archived_projects_returns_only_archived() {
        let conn = setup();
        create_project(&conn, "active", "/tmp/active").unwrap();
        let p2 = create_project(&conn, "archived", "/tmp/archived").unwrap();
        archive_project(&conn, &p2.id).unwrap();
        let archived = list_archived_projects(&conn).unwrap();
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].name, "archived");
    }

    #[test]
    fn test_restore_project_sessions_stay_archived() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let s = create_session(&conn, &p.id, "sess", "planeai-myapp-aaa", "main", None).unwrap();
        archive_session(&conn, &s.id).unwrap();
        archive_project(&conn, &p.id).unwrap();
        restore_project(&conn, &p.id).unwrap();
        // Project is visible again
        assert_eq!(list_projects(&conn).unwrap().len(), 1);
        // Session stays archived
        assert_eq!(list_sessions(&conn).unwrap().len(), 0);
        assert_eq!(list_archived_sessions(&conn).unwrap().len(), 1);
    }

    #[test]
    fn test_get_project_sessions_for_deletion() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        create_session(&conn, &p.id, "s1", "planeai-myapp-aaa", "main", None).unwrap();
        create_session(
            &conn,
            &p.id,
            "s2",
            "planeai-myapp-bbb",
            "feat",
            Some("/tmp/wt/feat"),
        )
        .unwrap();
        let s3 = create_session(&conn, &p.id, "s3", "planeai-myapp-ccc", "fix", None).unwrap();
        archive_session(&conn, &s3.id).unwrap();
        // get_project_sessions returns ALL sessions regardless of status
        let sessions = get_project_sessions(&conn, &p.id).unwrap();
        assert_eq!(sessions.len(), 3);
        let wt_sessions: Vec<_> = sessions
            .iter()
            .filter(|s| s.worktree_path.is_some())
            .collect();
        assert_eq!(wt_sessions.len(), 1);
        assert_eq!(
            wt_sessions[0].worktree_path.as_deref(),
            Some("/tmp/wt/feat")
        );
    }

    #[test]
    fn test_archive_project_archives_sessions_without_killing() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        create_session(&conn, &p.id, "active", "planeai-myapp-aaa", "main", None).unwrap();
        create_session(&conn, &p.id, "exited", "planeai-myapp-bbb", "feat", None).unwrap();
        mark_session_exited(&conn, &list_sessions(&conn).unwrap()[1].id).unwrap();
        archive_project(&conn, &p.id).unwrap();
        // Sessions are archived in DB (not visible in list_sessions)
        assert_eq!(list_sessions(&conn).unwrap().len(), 0);
        // But sessions still exist (not deleted)
        let all = get_project_sessions(&conn, &p.id).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().all(|s| s.status == "archived"));
        // tmux_name preserved (caller can check if tmux is still running)
        assert!(all.iter().all(|s| s.tmux_name.is_some()));
    }

    #[test]
    fn test_migrate_relaxes_tmux_name_not_null() {
        // Simulate old schema with tmux_name NOT NULL
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT NOT NULL, path TEXT NOT NULL);
             CREATE TABLE sessions (
                 id TEXT PRIMARY KEY,
                 project_id TEXT NOT NULL REFERENCES projects(id),
                 tmux_name TEXT NOT NULL,
                 branch TEXT NOT NULL,
                 status TEXT NOT NULL DEFAULT 'active',
                 created_at TEXT NOT NULL
             );",
        )
        .unwrap();
        // Insert a row with the old schema
        conn.execute_batch(
            "INSERT INTO projects VALUES ('p1', 'myapp', '/tmp/myapp');
             INSERT INTO sessions VALUES ('s1', 'p1', 'planeai-myapp-old', 'main', 'active', '2024-01-01');"
        ).unwrap();
        // Run migration — should rebuild table with nullable tmux_name
        migrate(&conn).unwrap();
        // Now inserting NULL tmux_name should work
        let s = create_session_with_id(
            &conn, "s2", "p1", "direct", None, "feat", None, None, "direct", true, None, None,
        )
        .unwrap();
        assert!(s.tmux_name.is_none());
        // Old data preserved
        let old = get_session(&conn, "s1").unwrap().unwrap();
        assert_eq!(old.tmux_name, Some("planeai-myapp-old".to_string()));
    }

    #[test]
    fn test_migration_adds_base_branch_column_defaulting_to_null() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let s = create_session(&conn, &p.id, "test", "planeai-myapp-aaa", "main", None).unwrap();
        assert!(s.base_branch.is_none());
    }

    #[test]
    fn test_create_session_with_base_branch() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let s = create_session_with_id(
            &conn,
            "sess-bb",
            &p.id,
            "feat session",
            None,
            "feat/x",
            None,
            None,
            "direct",
            true,
            None,
            Some("main"),
        )
        .unwrap();
        assert_eq!(s.base_branch, Some("main".to_string()));
        let loaded = get_session(&conn, "sess-bb").unwrap().unwrap();
        assert_eq!(loaded.base_branch, Some("main".to_string()));
    }

    #[test]
    fn test_save_mru_order_persists_rank_indices() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        create_session_with_id(
            &conn, "a", &p.id, "A", None, "main", None, None, "direct", true, None, None,
        )
        .unwrap();
        create_session_with_id(
            &conn, "b", &p.id, "B", None, "main", None, None, "direct", true, None, None,
        )
        .unwrap();
        create_session_with_id(
            &conn, "c", &p.id, "C", None, "main", None, None, "direct", true, None, None,
        )
        .unwrap();

        save_mru_order(&conn, &["a", "b", "c"]).unwrap();

        let pos_a: Option<i64> = conn
            .query_row(
                "SELECT mru_position FROM sessions WHERE id = 'a'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let pos_b: Option<i64> = conn
            .query_row(
                "SELECT mru_position FROM sessions WHERE id = 'b'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let pos_c: Option<i64> = conn
            .query_row(
                "SELECT mru_position FROM sessions WHERE id = 'c'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pos_a, Some(0));
        assert_eq!(pos_b, Some(1));
        assert_eq!(pos_c, Some(2));
    }

    #[test]
    fn test_list_sessions_returns_mru_order() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        create_session_with_id(
            &conn, "a", &p.id, "A", None, "main", None, None, "direct", true, None, None,
        )
        .unwrap();
        create_session_with_id(
            &conn, "b", &p.id, "B", None, "main", None, None, "direct", true, None, None,
        )
        .unwrap();
        create_session_with_id(
            &conn, "c", &p.id, "C", None, "main", None, None, "direct", true, None, None,
        )
        .unwrap();

        save_mru_order(&conn, &["b", "a", "c"]).unwrap();

        let sessions = list_sessions(&conn).unwrap();
        let ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "a", "c"]);
    }

    #[test]
    fn test_null_mru_position_sorts_last_by_created_at() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        create_session_with_id(
            &conn, "a", &p.id, "A", None, "main", None, None, "direct", true, None, None,
        )
        .unwrap();
        create_session_with_id(
            &conn, "b", &p.id, "B", None, "main", None, None, "direct", true, None, None,
        )
        .unwrap();
        create_session_with_id(
            &conn, "c", &p.id, "C", None, "main", None, None, "direct", true, None, None,
        )
        .unwrap();

        // Only position "b" — a and c remain NULL
        save_mru_order(&conn, &["b"]).unwrap();

        let sessions = list_sessions(&conn).unwrap();
        let ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
        // b first (position 0), then a and c in created_at order
        assert_eq!(ids, vec!["b", "a", "c"]);
    }

    #[test]
    fn test_save_partial_mru_resets_unlisted_positions() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        create_session_with_id(
            &conn, "a", &p.id, "A", None, "main", None, None, "direct", true, None, None,
        )
        .unwrap();
        create_session_with_id(
            &conn, "b", &p.id, "B", None, "main", None, None, "direct", true, None, None,
        )
        .unwrap();
        create_session_with_id(
            &conn, "c", &p.id, "C", None, "main", None, None, "direct", true, None, None,
        )
        .unwrap();

        // First save positions all three
        save_mru_order(&conn, &["a", "b", "c"]).unwrap();
        // Now save only "c" — b and a should become NULL
        save_mru_order(&conn, &["c"]).unwrap();

        let pos_a: Option<i64> = conn
            .query_row(
                "SELECT mru_position FROM sessions WHERE id = 'a'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let pos_b: Option<i64> = conn
            .query_row(
                "SELECT mru_position FROM sessions WHERE id = 'b'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let pos_c: Option<i64> = conn
            .query_row(
                "SELECT mru_position FROM sessions WHERE id = 'c'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pos_c, Some(0));
        assert_eq!(pos_a, None);
        assert_eq!(pos_b, None);
    }

    #[test]
    fn test_pr_state_round_trips_through_update_and_get() {
        let conn = setup();
        let proj = create_project(&conn, "test", "/tmp/test").unwrap();
        let session =
            create_session(&conn, &proj.id, "feat", "tmux-1", "feat/pr-test", None).unwrap();

        // Initially null
        let s = get_session(&conn, &session.id).unwrap().unwrap();
        assert_eq!(s.pr_url, None);
        assert_eq!(s.pr_state, None);

        // Update
        update_pr_state(
            &conn,
            &session.id,
            "https://github.com/org/repo/pull/42",
            "open",
        )
        .unwrap();

        let s = get_session(&conn, &session.id).unwrap().unwrap();
        assert_eq!(
            s.pr_url.as_deref(),
            Some("https://github.com/org/repo/pull/42")
        );
        assert_eq!(s.pr_state.as_deref(), Some("open"));

        // Update again (state transition)
        update_pr_state(
            &conn,
            &session.id,
            "https://github.com/org/repo/pull/42",
            "merged",
        )
        .unwrap();
        let s = get_session(&conn, &session.id).unwrap().unwrap();
        assert_eq!(s.pr_state.as_deref(), Some("merged"));
    }

    #[test]
    fn test_list_sessions_excludes_done_task_sessions() {
        let conn = setup();
        // Run task migrations so the tasks table exists
        planeai_tasks::sqlite::migrate(&conn).unwrap();

        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        // Session with no task_key — should always appear
        let s1 = create_session_with_id(
            &conn, "s1", &p.id, "no task", None, "main", None, None, "direct", false, None, None,
        )
        .unwrap();

        // Session linked to a done task — should be excluded
        let s2 = create_session_with_id(
            &conn,
            "s2",
            &p.id,
            "done task",
            None,
            "feat-a",
            None,
            None,
            "direct",
            false,
            Some("MYA-1"),
            None,
        )
        .unwrap();

        // Session linked to an in_progress task — should appear
        let s3 = create_session_with_id(
            &conn,
            "s3",
            &p.id,
            "active task",
            None,
            "feat-b",
            None,
            None,
            "direct",
            false,
            Some("MYA-2"),
            None,
        )
        .unwrap();

        // Insert tasks
        conn.execute(
            "INSERT INTO task_projects (prefix, next_seq) VALUES ('MYA', 3)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tasks (key, project_prefix, title, status, created_at, updated_at) VALUES ('MYA-1', 'MYA', 'Done task', 'done', '2024-01-01', '2024-01-01')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tasks (key, project_prefix, title, status, created_at, updated_at) VALUES ('MYA-2', 'MYA', 'Active task', 'in_progress', '2024-01-01', '2024-01-01')",
            [],
        )
        .unwrap();

        let sessions = list_sessions(&conn).unwrap();
        let ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&s1.id.as_str()));
        assert!(!ids.contains(&s2.id.as_str())); // done task — excluded
        assert!(ids.contains(&s3.id.as_str()));
    }
}

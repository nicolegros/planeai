use rusqlite::{Connection, Result, params};
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
        INSERT OR IGNORE INTO settings (id) VALUES (1);"
    )?;
    // Add name column to existing databases
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN name TEXT NOT NULL DEFAULT ''");
    // Add worktree_path column
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN worktree_path TEXT");
    // Add font_family column
    let _ = conn.execute_batch("ALTER TABLE settings ADD COLUMN font_family TEXT NOT NULL DEFAULT 'Menlo'");
    // Migrate old settings schema
    let _ = conn.execute_batch("ALTER TABLE settings ADD COLUMN terminal_theme_dark TEXT NOT NULL DEFAULT 'one-dark'");
    let _ = conn.execute_batch("ALTER TABLE settings ADD COLUMN terminal_theme_light TEXT NOT NULL DEFAULT 'one-light'");
    let _ = conn.execute_batch("ALTER TABLE settings ADD COLUMN appearance_mode TEXT NOT NULL DEFAULT 'system'");
    // Copy old terminal_theme to terminal_theme_dark if it existed
    let _ = conn.execute_batch("UPDATE settings SET terminal_theme_dark = terminal_theme WHERE terminal_theme IS NOT NULL");
    // Add provider column to sessions
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN provider TEXT");
    // Add backend column (defaults to 'tmux' for existing sessions)
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN backend TEXT NOT NULL DEFAULT 'tmux'");
    // Add provider_session_id column
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN provider_session_id TEXT");
    // Add tab_count column
    let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN tab_count INTEGER NOT NULL DEFAULT 1");
    Ok(())
}

// Project CRUD

pub fn create_project(conn: &Connection, name: &str, path: &str) -> Result<Project> {
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO projects (id, name, path) VALUES (?1, ?2, ?3)",
        params![id, name, path],
    )?;
    Ok(Project { id, name: name.to_string(), path: path.to_string() })
}

pub fn list_projects(conn: &Connection) -> Result<Vec<Project>> {
    let mut stmt = conn.prepare("SELECT id, name, path FROM projects")?;
    let rows = stmt.query_map([], |row| {
        Ok(Project {
            id: row.get(0)?,
            name: row.get(1)?,
            path: row.get(2)?,
        })
    })?;
    rows.collect()
}

pub fn delete_project(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM sessions WHERE project_id = ?1", params![id])?;
    conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
    Ok(())
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
    create_session_with_id(conn, &id, project_id, name, Some(tmux_name), branch, worktree_path, None, "tmux")
}

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
) -> Result<Session> {
    let created_at = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO sessions (id, project_id, name, tmux_name, branch, status, created_at, worktree_path, provider, backend) VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?7, ?8, ?9)",
        params![id, project_id, name, tmux_name, branch, created_at, worktree_path, provider, backend],
    )?;
    Ok(Session { id: id.to_string(), project_id: project_id.to_string(), name: name.to_string(), tmux_name: tmux_name.map(|s| s.to_string()), branch: branch.to_string(), status: "active".to_string(), created_at, worktree_path: worktree_path.map(|s| s.to_string()), provider: provider.map(|s| s.to_string()), backend: backend.to_string(), provider_session_id: None, tab_count: 1 })
}

pub fn list_sessions(conn: &Connection) -> Result<Vec<Session>> {
    let mut stmt = conn.prepare("SELECT id, project_id, name, tmux_name, branch, status, created_at, worktree_path, provider, backend, provider_session_id, tab_count FROM sessions WHERE status IN ('active', 'exited')")?;
    let rows = stmt.query_map([], |row| {
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
        })
    })?;
    rows.collect()
}

pub fn list_archived_sessions(conn: &Connection) -> Result<Vec<Session>> {
    let mut stmt = conn.prepare("SELECT id, project_id, name, tmux_name, branch, status, created_at, worktree_path, provider, backend, provider_session_id, tab_count FROM sessions WHERE status = 'archived'")?;
    let rows = stmt.query_map([], |row| {
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
        })
    })?;
    rows.collect()
}

pub fn archive_session(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("UPDATE sessions SET status = 'archived' WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn delete_session(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn destroy_session(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("UPDATE sessions SET status = 'destroyed' WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn mark_session_exited(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("UPDATE sessions SET status = 'exited' WHERE id = ?1", params![id])?;
    Ok(())
}

/// Startup reconciliation: mark stale active sessions as exited.
/// Direct sessions are always marked exited (process died with app).
/// Tmux sessions are checked via the provided `has_session` function.
pub fn reconcile_sessions<F>(conn: &Connection, has_tmux_session: F) -> Result<()>
where
    F: Fn(&str) -> bool,
{
    let mut stmt = conn.prepare("SELECT id, tmux_name, backend FROM sessions WHERE status = 'active'")?;
    let stale: Vec<String> = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let tmux_name: Option<String> = row.get(1)?;
        let backend: String = row.get(2)?;
        Ok((id, tmux_name, backend))
    })?
    .filter_map(|r| r.ok())
    .filter(|(_, tmux_name, backend)| {
        if backend == "direct" {
            return true;
        }
        // tmux backend: mark exited if tmux session is gone
        match tmux_name {
            Some(ref name) => !has_tmux_session(name),
            None => true,
        }
    })
    .map(|(id, _, _)| id)
    .collect();

    for id in &stale {
        mark_session_exited(conn, id)?;
    }
    Ok(())
}

pub fn restore_session(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("UPDATE sessions SET status = 'active' WHERE id = ?1", params![id])?;
    Ok(())
}

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
    conn.execute("UPDATE sessions SET name = ?2 WHERE id = ?1", params![id, name])?;
    Ok(())
}

pub fn set_provider_session_id(conn: &Connection, id: &str, provider_session_id: &str) -> Result<()> {
    conn.execute("UPDATE sessions SET provider_session_id = ?2 WHERE id = ?1", params![id, provider_session_id])?;
    Ok(())
}

pub fn update_tab_count(conn: &Connection, id: &str, tab_count: i64) -> Result<()> {
    conn.execute("UPDATE sessions SET tab_count = ?2 WHERE id = ?1", params![id, tab_count])?;
    Ok(())
}

pub fn get_session(conn: &Connection, id: &str) -> Result<Option<Session>> {
    let mut stmt = conn.prepare("SELECT id, project_id, name, tmux_name, branch, status, created_at, worktree_path, provider, backend, provider_session_id, tab_count FROM sessions WHERE id = ?1")?;
    let mut rows = stmt.query_map(params![id], |row| {
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
        })
    })?;
    Ok(rows.next().transpose()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
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
        let s = create_session_with_id(&conn, "sess-1", &p.id, "direct session", None, "main", None, None, "direct").unwrap();
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
        let s1 = create_session(&conn, &p.id, "active one", "planeai-myapp-aaa", "main", None).unwrap();
        let s2 = create_session(&conn, &p.id, "exited one", "planeai-myapp-bbb", "feat", None).unwrap();
        mark_session_exited(&conn, &s2.id).unwrap();
        let sessions = list_sessions(&conn).unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(sessions.iter().any(|s| s.id == s1.id && s.status == "active"));
        assert!(sessions.iter().any(|s| s.id == s2.id && s.status == "exited"));
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
        create_session(&conn, &p.id, "main session", "planeai-myapp-abc123", "main", None).unwrap();
        delete_project(&conn, &p.id).unwrap();
        assert_eq!(list_projects(&conn).unwrap().len(), 0);
        assert_eq!(list_sessions(&conn).unwrap().len(), 0);
    }

    #[test]
    fn test_create_and_list_sessions() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        create_session(&conn, &p.id, "feat session", "planeai-myapp-aaa", "feat-x", None).unwrap();
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
        let s = create_session(&conn, &p.id, "to delete", "planeai-myapp-bbb", "main", None).unwrap();
        delete_session(&conn, &s.id).unwrap();
        assert_eq!(list_sessions(&conn).unwrap().len(), 0);
    }

    #[test]
    fn test_worktree_session() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let s = create_session(&conn, &p.id, "wt session", "planeai-myapp-ccc", "feat-wt", Some("/home/.planeai/worktrees/myapp/ccc")).unwrap();
        assert_eq!(s.worktree_path.as_deref(), Some("/home/.planeai/worktrees/myapp/ccc"));
        let sessions = list_sessions(&conn).unwrap();
        assert_eq!(sessions[0].worktree_path.as_deref(), Some("/home/.planeai/worktrees/myapp/ccc"));
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
        create_session(&conn2, &p2.id, "wt", "planeai-myapp2-bbb", "feat", Some("/tmp/wt")).unwrap();
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
        let s = create_session(&conn, &p.id, "old name", "planeai-myapp-aaa", "main", None).unwrap();
        rename_session(&conn, &s.id, "new name").unwrap();
        let updated = get_session(&conn, &s.id).unwrap().unwrap();
        assert_eq!(updated.name, "new name");
    }

    #[test]
    fn test_list_archived_sessions() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        create_session(&conn, &p.id, "active one", "planeai-myapp-aaa", "main", None).unwrap();
        let s2 = create_session(&conn, &p.id, "archived one", "planeai-myapp-bbb", "feat", None).unwrap();
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
        let s2 = create_session(&conn, &p.id, "archived", "planeai-myapp-bbb", "feat", None).unwrap();
        archive_session(&conn, &s2.id).unwrap();
        let s3 = create_session(&conn, &p.id, "destroyed", "planeai-myapp-ccc", "fix", None).unwrap();
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
        let s = create_session(&conn, &p.id, "to restore", "planeai-myapp-aaa", "main", None).unwrap();
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
        let s = create_session_with_id(&conn, "s1", &p.id, "test", None, "main", None, Some("kiro"), "direct").unwrap();
        // Initially null
        assert_eq!(s.provider_session_id, None);
        // Set it
        set_provider_session_id(&conn, "s1", "f4165541-f370-4fdd-9ccd-14b103a4f712").unwrap();
        let loaded = get_session(&conn, "s1").unwrap().unwrap();
        assert_eq!(loaded.provider_session_id, Some("f4165541-f370-4fdd-9ccd-14b103a4f712".to_string()));
    }

    #[test]
    fn test_reconcile_sessions_marks_stale_as_exited() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        // Active direct session → should be marked exited (process died with app)
        let s1 = create_session_with_id(&conn, "s1", &p.id, "direct", None, "main", None, None, "direct").unwrap();
        // Active tmux session with dead tmux → should be marked exited
        let s2 = create_session_with_id(&conn, "s2", &p.id, "tmux dead", Some("planeai-dead"), "main", None, None, "tmux").unwrap();
        // Active tmux session with alive tmux → should stay active
        let s3 = create_session_with_id(&conn, "s3", &p.id, "tmux alive", Some("planeai-alive"), "main", None, None, "tmux").unwrap();

        // Reconcile: mock tmux checker that only knows "planeai-alive"
        reconcile_sessions(&conn, |name| name == "planeai-alive").unwrap();

        assert_eq!(get_session(&conn, &s1.id).unwrap().unwrap().status, "exited");
        assert_eq!(get_session(&conn, &s2.id).unwrap().unwrap().status, "exited");
        assert_eq!(get_session(&conn, &s3.id).unwrap().unwrap().status, "active");
    }
}

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
    pub tmux_name: String,
    pub branch: String,
    pub status: String,
    pub created_at: String,
    pub worktree_path: Option<String>,
    pub provider: Option<String>,
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
            tmux_name TEXT NOT NULL,
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
    create_session_with_id(conn, &id, project_id, name, tmux_name, branch, worktree_path, None)
}

pub fn create_session_with_id(
    conn: &Connection,
    id: &str,
    project_id: &str,
    name: &str,
    tmux_name: &str,
    branch: &str,
    worktree_path: Option<&str>,
    provider: Option<&str>,
) -> Result<Session> {
    let created_at = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO sessions (id, project_id, name, tmux_name, branch, status, created_at, worktree_path, provider) VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?7, ?8)",
        params![id, project_id, name, tmux_name, branch, created_at, worktree_path, provider],
    )?;
    Ok(Session { id: id.to_string(), project_id: project_id.to_string(), name: name.to_string(), tmux_name: tmux_name.to_string(), branch: branch.to_string(), status: "active".to_string(), created_at, worktree_path: worktree_path.map(|s| s.to_string()), provider: provider.map(|s| s.to_string()) })
}

pub fn list_sessions(conn: &Connection) -> Result<Vec<Session>> {
    let mut stmt = conn.prepare("SELECT id, project_id, name, tmux_name, branch, status, created_at, worktree_path, provider FROM sessions")?;
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

pub fn get_session(conn: &Connection, id: &str) -> Result<Option<Session>> {
    let mut stmt = conn.prepare("SELECT id, project_id, name, tmux_name, branch, status, created_at, worktree_path, provider FROM sessions WHERE id = ?1")?;
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
}

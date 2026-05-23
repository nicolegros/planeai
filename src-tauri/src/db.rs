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
    pub tmux_name: String,
    pub branch: String,
    pub status: String,
    pub created_at: String,
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
            tmux_name TEXT NOT NULL,
            branch TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT NOT NULL
        );"
    )
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
    tmux_name: &str,
    branch: &str,
) -> Result<Session> {
    let id = uuid::Uuid::new_v4().to_string();
    let created_at = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO sessions (id, project_id, tmux_name, branch, status, created_at) VALUES (?1, ?2, ?3, ?4, 'active', ?5)",
        params![id, project_id, tmux_name, branch, created_at],
    )?;
    Ok(Session { id, project_id: project_id.to_string(), tmux_name: tmux_name.to_string(), branch: branch.to_string(), status: "active".to_string(), created_at })
}

pub fn list_sessions(conn: &Connection) -> Result<Vec<Session>> {
    let mut stmt = conn.prepare("SELECT id, project_id, tmux_name, branch, status, created_at FROM sessions")?;
    let rows = stmt.query_map([], |row| {
        Ok(Session {
            id: row.get(0)?,
            project_id: row.get(1)?,
            tmux_name: row.get(2)?,
            branch: row.get(3)?,
            status: row.get(4)?,
            created_at: row.get(5)?,
        })
    })?;
    rows.collect()
}

pub fn delete_session(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
    Ok(())
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
        create_session(&conn, &p.id, "planeai-myapp-abc123", "main").unwrap();
        delete_project(&conn, &p.id).unwrap();
        assert_eq!(list_projects(&conn).unwrap().len(), 0);
        assert_eq!(list_sessions(&conn).unwrap().len(), 0);
    }

    #[test]
    fn test_create_and_list_sessions() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        create_session(&conn, &p.id, "planeai-myapp-aaa", "feat-x").unwrap();
        let sessions = list_sessions(&conn).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].branch, "feat-x");
        assert_eq!(sessions[0].status, "active");
    }

    #[test]
    fn test_delete_session() {
        let conn = setup();
        let p = create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let s = create_session(&conn, &p.id, "planeai-myapp-bbb", "main").unwrap();
        delete_session(&conn, &s.id).unwrap();
        assert_eq!(list_sessions(&conn).unwrap().len(), 0);
    }
}

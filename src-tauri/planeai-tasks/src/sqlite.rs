use chrono::Utc;
use rusqlite::{params, Connection};
use std::sync::Mutex;

use crate::model::{CreateParams, ListFilter, Status, Task, UpdateParams, DEFAULT_BASE_BRANCH};
use crate::provider::{Error, TaskProvider};

/// Run task-table migrations on an existing connection.
/// Call this on the shared app DB connection before constructing a repository.
pub fn migrate(conn: &Connection) -> Result<(), Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_schema_version (version INTEGER NOT NULL);
         INSERT OR IGNORE INTO task_schema_version (version) SELECT 0 WHERE NOT EXISTS (SELECT 1 FROM task_schema_version);",
    )
    .map_err(|e| Error::Storage(e.to_string()))?;

    let version: i32 = conn
        .query_row("SELECT version FROM task_schema_version", [], |r| r.get(0))
        .map_err(|e| Error::Storage(e.to_string()))?;

    let migrations = [
        "CREATE TABLE IF NOT EXISTS task_projects (
            prefix TEXT PRIMARY KEY,
            next_seq INTEGER NOT NULL DEFAULT 1
        );
        CREATE TABLE IF NOT EXISTS tasks (
            key TEXT PRIMARY KEY,
            project_prefix TEXT NOT NULL REFERENCES task_projects(prefix),
            title TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'todo',
            priority INTEGER NOT NULL DEFAULT 0,
            parent_key TEXT REFERENCES tasks(key) ON DELETE CASCADE,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS task_blockers (
            task_key TEXT NOT NULL REFERENCES tasks(key) ON DELETE CASCADE,
            blocked_by_key TEXT NOT NULL REFERENCES tasks(key) ON DELETE CASCADE,
            PRIMARY KEY (task_key, blocked_by_key)
        );
        CREATE TABLE IF NOT EXISTS task_tags (
            task_key TEXT NOT NULL REFERENCES tasks(key) ON DELETE CASCADE,
            tag TEXT NOT NULL,
            PRIMARY KEY (task_key, tag)
        );",
        "ALTER TABLE tasks ADD COLUMN base_branch TEXT;",
    ];

    for (i, sql) in migrations.iter().enumerate() {
        if (i as i32) >= version {
            conn.execute_batch(sql)
                .map_err(|e| Error::Storage(e.to_string()))?;
            conn.execute(
                "UPDATE task_schema_version SET version = ?1",
                params![i + 1],
            )
            .map_err(|e| Error::Storage(e.to_string()))?;
        }
    }
    Ok(())
}

/// Derive a project prefix from a project name.
/// Multi-word names (separated by `-`, `_`, or space) use word initials (e.g. "deployment-pipeline" → "DP").
/// Single-word names use the first 3 alphanumeric chars (e.g. "planeai" → "PLA").
pub fn derive_prefix(name: &str) -> String {
    let words: Vec<&str> = name
        .split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .collect();
    if words.len() > 1 {
        words
            .iter()
            .filter_map(|w| w.chars().find(|c| c.is_alphanumeric()))
            .collect::<String>()
            .to_uppercase()
    } else {
        name.chars()
            .filter(|c| c.is_alphanumeric())
            .take(3)
            .collect::<String>()
            .to_uppercase()
    }
}

pub struct SqliteRepository {
    conn: Mutex<Connection>,
    prefix: String,
}

impl SqliteRepository {
    /// Create a repository from an existing connection. Runs migrations automatically.
    pub fn new(conn: Connection, prefix: &str) -> Result<Self, Error> {
        migrate(&conn)?;
        conn.execute(
            "INSERT OR IGNORE INTO task_projects (prefix) VALUES (?1)",
            params![prefix],
        )
        .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(Self {
            conn: Mutex::new(conn),
            prefix: prefix.to_string(),
        })
    }

    /// Convenience: open a file-backed DB.
    pub fn open(db_path: &str, prefix: &str) -> Result<Self, Error> {
        let conn = Connection::open(db_path).map_err(|e| Error::Storage(e.to_string()))?;
        Self::new(conn, prefix)
    }

    /// Convenience: open an in-memory DB (for testing).
    pub fn open_in_memory(prefix: &str) -> Result<Self, Error> {
        let conn = Connection::open_in_memory().map_err(|e| Error::Storage(e.to_string()))?;
        Self::new(conn, prefix)
    }

    fn next_key(&self, conn: &Connection) -> Result<String, Error> {
        let seq: i32 = conn
            .query_row(
                "UPDATE task_projects SET next_seq = next_seq + 1 WHERE prefix = ?1 RETURNING next_seq - 1",
                params![self.prefix],
                |r| r.get(0),
            )
            .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(format!("{}-{}", self.prefix, seq))
    }

    fn load_blockers(&self, conn: &Connection, key: &str) -> Result<Vec<String>, Error> {
        let mut stmt = conn
            .prepare("SELECT blocked_by_key FROM task_blockers WHERE task_key = ?1")
            .map_err(|e| Error::Storage(e.to_string()))?;
        let rows = stmt
            .query_map(params![key], |r| r.get(0))
            .map_err(|e| Error::Storage(e.to_string()))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(e.to_string()))?);
        }
        Ok(out)
    }

    fn load_tags(&self, conn: &Connection, key: &str) -> Result<Vec<String>, Error> {
        let mut stmt = conn
            .prepare("SELECT tag FROM task_tags WHERE task_key = ?1 ORDER BY tag")
            .map_err(|e| Error::Storage(e.to_string()))?;
        let rows = stmt
            .query_map(params![key], |r| r.get(0))
            .map_err(|e| Error::Storage(e.to_string()))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(e.to_string()))?);
        }
        Ok(out)
    }

    fn row_to_task(&self, conn: &Connection, row: TaskRow) -> Result<Task, Error> {
        let status = Status::parse(&row.status).ok_or(Error::InvalidStatus(row.status))?;
        let blocked_by = self.load_blockers(conn, &row.key)?;
        let tags = self.load_tags(conn, &row.key)?;
        Ok(Task {
            key: row.key,
            title: row.title,
            description: row.description,
            status,
            priority: row.priority,
            parent_key: row.parent_key,
            blocked_by,
            tags,
            base_branch: row
                .base_branch
                .unwrap_or_else(|| DEFAULT_BASE_BRANCH.to_string()),
            created_at: chrono::DateTime::parse_from_rfc3339(&row.created_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            updated_at: chrono::DateTime::parse_from_rfc3339(&row.updated_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
        })
    }

    fn get_with_conn(&self, conn: &Connection, key: &str) -> Result<Task, Error> {
        let mut stmt = conn
            .prepare("SELECT key, title, description, status, priority, parent_key, base_branch, created_at, updated_at FROM tasks WHERE key = ?1")
            .map_err(|e| Error::Storage(e.to_string()))?;

        stmt.query_row(params![key], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i32>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
            ))
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Error::NotFound,
            _ => Error::Storage(e.to_string()),
        })
        .and_then(
            |(k, title, desc, status, priority, parent, base_branch, created, updated)| {
                self.row_to_task(
                    conn,
                    TaskRow {
                        key: k,
                        title,
                        description: desc,
                        status,
                        priority,
                        parent_key: parent,
                        base_branch,
                        created_at: created,
                        updated_at: updated,
                    },
                )
            },
        )
    }
}

struct TaskRow {
    key: String,
    title: String,
    description: String,
    status: String,
    priority: i32,
    parent_key: Option<String>,
    base_branch: Option<String>,
    created_at: String,
    updated_at: String,
}

impl TaskProvider for SqliteRepository {
    fn create(&self, params: CreateParams) -> Result<Task, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Storage(e.to_string()))?;
        let key = match params.key {
            Some(ref k) => k.clone(),
            None => self.next_key(&conn)?,
        };
        let now = Utc::now().to_rfc3339();
        let status = params.status.unwrap_or(Status::Todo);

        let result = conn.execute(
            "INSERT OR IGNORE INTO tasks (key, project_prefix, title, description, status, priority, parent_key, base_branch, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![key, self.prefix, params.title, params.description, status.as_str(), params.priority, params.parent_key, params.base_branch, now, now],
        ).map_err(|e| Error::Storage(e.to_string()))?;

        if result == 0 {
            // Key already exists — idempotent create: return existing task
            return self.get_with_conn(&conn, &key);
        }

        for bk in &params.blocked_by {
            conn.execute(
                "INSERT INTO task_blockers (task_key, blocked_by_key) VALUES (?1, ?2)",
                params![key, bk],
            )
            .map_err(|e| Error::Storage(e.to_string()))?;
        }

        for tag in &params.tags {
            conn.execute(
                "INSERT INTO task_tags (task_key, tag) VALUES (?1, ?2)",
                params![key, tag],
            )
            .map_err(|e| Error::Storage(e.to_string()))?;
        }

        self.row_to_task(
            &conn,
            TaskRow {
                key,
                title: params.title,
                description: params.description,
                status: status.as_str().to_string(),
                priority: params.priority,
                parent_key: params.parent_key,
                base_branch: Some(params.base_branch),
                created_at: now.clone(),
                updated_at: now,
            },
        )
    }

    fn get(&self, key: &str) -> Result<Task, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Storage(e.to_string()))?;
        let mut stmt = conn
            .prepare("SELECT key, title, description, status, priority, parent_key, base_branch, created_at, updated_at FROM tasks WHERE key = ?1")
            .map_err(|e| Error::Storage(e.to_string()))?;

        stmt.query_row(params![key], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i32>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
            ))
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Error::NotFound,
            _ => Error::Storage(e.to_string()),
        })
        .and_then(
            |(k, title, desc, status, priority, parent, base_branch, created, updated)| {
                self.row_to_task(
                    &conn,
                    TaskRow {
                        key: k,
                        title,
                        description: desc,
                        status,
                        priority,
                        parent_key: parent,
                        base_branch,
                        created_at: created,
                        updated_at: updated,
                    },
                )
            },
        )
    }

    fn list(&self, filter: ListFilter) -> Result<Vec<Task>, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Storage(e.to_string()))?;

        let mut sql = "SELECT key, title, description, status, priority, parent_key, base_branch, created_at, updated_at FROM tasks WHERE project_prefix = ?1".to_string();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> =
            vec![Box::new(self.prefix.clone())];

        if let Some(ref s) = filter.status {
            sql.push_str(" AND status = ?");
            param_values.push(Box::new(s.as_str().to_string()));
        }
        if let Some(ref s) = filter.exclude_status {
            sql.push_str(" AND status != ?");
            param_values.push(Box::new(s.as_str().to_string()));
        }
        if let Some(ref pk) = filter.parent_key {
            match pk {
                Some(k) => {
                    sql.push_str(" AND parent_key = ?");
                    param_values.push(Box::new(k.clone()));
                }
                None => sql.push_str(" AND parent_key IS NULL"),
            }
        }
        if !filter.tags.is_empty() {
            let placeholders: Vec<String> = filter.tags.iter().map(|_| "?".to_string()).collect();
            sql.push_str(&format!(
                " AND key IN (SELECT task_key FROM task_tags WHERE tag IN ({}))",
                placeholders.join(",")
            ));
            for tag in &filter.tags {
                param_values.push(Box::new(tag.clone()));
            }
        }

        sql.push_str(
            " ORDER BY CASE WHEN priority > 0 THEN 0 ELSE 1 END, priority ASC, created_at ASC",
        );

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| Error::Storage(e.to_string()))?;
        let rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i32>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                ))
            })
            .map_err(|e| Error::Storage(e.to_string()))?;

        let mut tasks = Vec::new();
        for row in rows {
            let (k, title, desc, status, priority, parent, base_branch, created, updated) =
                row.map_err(|e| Error::Storage(e.to_string()))?;
            tasks.push(self.row_to_task(
                &conn,
                TaskRow {
                    key: k,
                    title,
                    description: desc,
                    status,
                    priority,
                    parent_key: parent,
                    base_branch,
                    created_at: created,
                    updated_at: updated,
                },
            )?);
        }
        Ok(tasks)
    }

    fn update(&self, key: &str, params: UpdateParams) -> Result<Task, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Storage(e.to_string()))?;

        let existing = {
            let mut stmt = conn
                .prepare("SELECT title, description, status, priority, parent_key, base_branch FROM tasks WHERE key = ?1")
                .map_err(|e| Error::Storage(e.to_string()))?;
            stmt.query_row(rusqlite::params![key], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i32>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Error::NotFound,
                _ => Error::Storage(e.to_string()),
            })?
        };

        let title = params.title.unwrap_or(existing.0);
        let description = params.description.unwrap_or(existing.1);
        let status_str = params
            .status
            .map(|s| s.as_str().to_string())
            .unwrap_or(existing.2);
        let priority = params.priority.unwrap_or(existing.3);
        let parent_key = params.parent_key.unwrap_or(existing.4);
        let base_branch = params
            .base_branch
            .or(existing.5)
            .unwrap_or_else(|| DEFAULT_BASE_BRANCH.to_string());
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "UPDATE tasks SET title = ?1, description = ?2, status = ?3, priority = ?4, parent_key = ?5, base_branch = ?6, updated_at = ?7 WHERE key = ?8",
            rusqlite::params![title, description, status_str, priority, parent_key, base_branch, now, key],
        ).map_err(|e| Error::Storage(e.to_string()))?;

        if let Some(ref blockers) = params.blocked_by {
            conn.execute(
                "DELETE FROM task_blockers WHERE task_key = ?1",
                rusqlite::params![key],
            )
            .map_err(|e| Error::Storage(e.to_string()))?;
            for bk in blockers {
                conn.execute(
                    "INSERT INTO task_blockers (task_key, blocked_by_key) VALUES (?1, ?2)",
                    rusqlite::params![key, bk],
                )
                .map_err(|e| Error::Storage(e.to_string()))?;
            }
        }

        if let Some(ref tags) = params.tags {
            conn.execute(
                "DELETE FROM task_tags WHERE task_key = ?1",
                rusqlite::params![key],
            )
            .map_err(|e| Error::Storage(e.to_string()))?;
            for tag in tags {
                conn.execute(
                    "INSERT INTO task_tags (task_key, tag) VALUES (?1, ?2)",
                    rusqlite::params![key, tag],
                )
                .map_err(|e| Error::Storage(e.to_string()))?;
            }
        }

        self.row_to_task(
            &conn,
            TaskRow {
                key: key.to_string(),
                title,
                description,
                status: status_str,
                priority,
                parent_key,
                base_branch: Some(base_branch),
                created_at: now.clone(),
                updated_at: now,
            },
        )
    }

    fn delete(&self, key: &str) -> Result<(), Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Storage(e.to_string()))?;
        let n = conn
            .execute("DELETE FROM tasks WHERE key = ?1", params![key])
            .map_err(|e| Error::Storage(e.to_string()))?;
        if n == 0 {
            return Err(Error::NotFound);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn repo() -> SqliteRepository {
        SqliteRepository::open_in_memory("TEST").unwrap()
    }

    #[test]
    fn create_and_get() {
        let r = repo();
        let task = r
            .create(CreateParams {
                title: "First task".into(),
                description: "desc".into(),
                priority: 1,
                tags: vec!["backend".into()],
                ..Default::default()
            })
            .unwrap();

        assert_eq!(task.key, "TEST-1");
        assert_eq!(task.status, Status::Todo);
        assert_eq!(task.tags, vec!["backend"]);

        let fetched = r.get("TEST-1").unwrap();
        assert_eq!(fetched.title, "First task");
        assert_eq!(fetched.priority, 1);
    }

    #[test]
    fn sequential_keys() {
        let r = repo();
        let t1 = r
            .create(CreateParams {
                title: "a".into(),
                ..Default::default()
            })
            .unwrap();
        let t2 = r
            .create(CreateParams {
                title: "b".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(t1.key, "TEST-1");
        assert_eq!(t2.key, "TEST-2");
    }

    #[test]
    fn get_not_found() {
        let r = repo();
        assert!(matches!(r.get("NOPE-1"), Err(Error::NotFound)));
    }

    #[test]
    fn update_partial() {
        let r = repo();
        r.create(CreateParams {
            title: "original".into(),
            priority: 1,
            ..Default::default()
        })
        .unwrap();
        let updated = r
            .update(
                "TEST-1",
                UpdateParams {
                    title: Some("renamed".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(updated.title, "renamed");
        assert_eq!(updated.priority, 1);
    }

    #[test]
    fn update_status() {
        let r = repo();
        r.create(CreateParams {
            title: "task".into(),
            ..Default::default()
        })
        .unwrap();
        let updated = r
            .update(
                "TEST-1",
                UpdateParams {
                    status: Some(Status::InProgress),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(updated.status, Status::InProgress);
    }

    #[test]
    fn update_not_found() {
        let r = repo();
        assert!(matches!(
            r.update("NOPE-1", UpdateParams::default()),
            Err(Error::NotFound)
        ));
    }

    #[test]
    fn delete_task() {
        let r = repo();
        r.create(CreateParams {
            title: "doomed".into(),
            ..Default::default()
        })
        .unwrap();
        r.delete("TEST-1").unwrap();
        assert!(matches!(r.get("TEST-1"), Err(Error::NotFound)));
    }

    #[test]
    fn delete_not_found() {
        let r = repo();
        assert!(matches!(r.delete("NOPE-1"), Err(Error::NotFound)));
    }

    #[test]
    fn list_all() {
        let r = repo();
        r.create(CreateParams {
            title: "a".into(),
            ..Default::default()
        })
        .unwrap();
        r.create(CreateParams {
            title: "b".into(),
            ..Default::default()
        })
        .unwrap();
        let tasks = r.list(ListFilter::default()).unwrap();
        assert_eq!(tasks.len(), 2);
    }

    #[test]
    fn list_filter_by_status() {
        let r = repo();
        r.create(CreateParams {
            title: "a".into(),
            ..Default::default()
        })
        .unwrap();
        r.create(CreateParams {
            title: "b".into(),
            ..Default::default()
        })
        .unwrap();
        r.update(
            "TEST-1",
            UpdateParams {
                status: Some(Status::Done),
                ..Default::default()
            },
        )
        .unwrap();

        let todo = r
            .list(ListFilter {
                status: Some(Status::Todo),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(todo.len(), 1);
        assert_eq!(todo[0].key, "TEST-2");

        let not_done = r
            .list(ListFilter {
                exclude_status: Some(Status::Done),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(not_done.len(), 1);
    }

    #[test]
    fn list_filter_by_tags() {
        let r = repo();
        r.create(CreateParams {
            title: "a".into(),
            tags: vec!["ui".into()],
            ..Default::default()
        })
        .unwrap();
        r.create(CreateParams {
            title: "b".into(),
            tags: vec!["backend".into()],
            ..Default::default()
        })
        .unwrap();

        let ui = r
            .list(ListFilter {
                tags: vec!["ui".into()],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(ui.len(), 1);
        assert_eq!(ui[0].key, "TEST-1");
    }

    #[test]
    fn list_priority_ordering() {
        let r = repo();
        r.create(CreateParams {
            title: "low".into(),
            priority: 0,
            ..Default::default()
        })
        .unwrap();
        r.create(CreateParams {
            title: "high".into(),
            priority: 1,
            ..Default::default()
        })
        .unwrap();
        r.create(CreateParams {
            title: "medium".into(),
            priority: 2,
            ..Default::default()
        })
        .unwrap();

        let tasks = r.list(ListFilter::default()).unwrap();
        assert_eq!(tasks[0].title, "high");
        assert_eq!(tasks[1].title, "medium");
        assert_eq!(tasks[2].title, "low");
    }

    #[test]
    fn blockers_roundtrip() {
        let r = repo();
        r.create(CreateParams {
            title: "first".into(),
            ..Default::default()
        })
        .unwrap();
        r.create(CreateParams {
            title: "second".into(),
            blocked_by: vec!["TEST-1".into()],
            ..Default::default()
        })
        .unwrap();

        let t = r.get("TEST-2").unwrap();
        assert_eq!(t.blocked_by, vec!["TEST-1"]);

        r.update(
            "TEST-2",
            UpdateParams {
                blocked_by: Some(vec![]),
                ..Default::default()
            },
        )
        .unwrap();
        let t = r.get("TEST-2").unwrap();
        assert!(t.blocked_by.is_empty());
    }

    #[test]
    fn parent_key() {
        let r = repo();
        r.create(CreateParams {
            title: "parent".into(),
            ..Default::default()
        })
        .unwrap();
        r.create(CreateParams {
            title: "child".into(),
            parent_key: Some("TEST-1".into()),
            ..Default::default()
        })
        .unwrap();

        let child = r.get("TEST-2").unwrap();
        assert_eq!(child.parent_key, Some("TEST-1".to_string()));

        let roots = r
            .list(ListFilter {
                parent_key: Some(None),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].key, "TEST-1");
    }

    #[test]
    fn derive_prefix_from_name() {
        assert_eq!(derive_prefix("planeai"), "PLA");
        assert_eq!(derive_prefix("nomi"), "NOM");
        assert_eq!(derive_prefix("budget-buddy"), "BB");
        assert_eq!(derive_prefix("AB"), "AB");
    }

    #[test]
    fn derive_prefix_unique_for_similar_names() {
        // These must produce different prefixes
        let a = derive_prefix("deployment-pipeline");
        let b = derive_prefix("deployment-pipeline-api");
        assert_ne!(
            a, b,
            "deployment-pipeline and deployment-pipeline-api must have different prefixes"
        );
        assert_eq!(a, "DP");
        assert_eq!(b, "DPA");
    }

    #[test]
    fn new_with_existing_connection() {
        let conn = Connection::open_in_memory().unwrap();
        let repo = SqliteRepository::new(conn, "FOO").unwrap();
        let task = repo
            .create(CreateParams {
                title: "works".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(task.key, "FOO-1");
    }

    #[test]
    fn base_branch_defaults_to_main() {
        let r = repo();
        let task = r
            .create(CreateParams {
                title: "task".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(task.base_branch, "main");
        let fetched = r.get("TEST-1").unwrap();
        assert_eq!(fetched.base_branch, "main");
    }

    #[test]
    fn base_branch_custom_on_create() {
        let r = repo();
        let task = r
            .create(CreateParams {
                title: "task".into(),
                base_branch: "develop".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(task.base_branch, "develop");
        let fetched = r.get("TEST-1").unwrap();
        assert_eq!(fetched.base_branch, "develop");
    }

    #[test]
    fn base_branch_update() {
        let r = repo();
        r.create(CreateParams {
            title: "task".into(),
            ..Default::default()
        })
        .unwrap();
        let updated = r
            .update(
                "TEST-1",
                UpdateParams {
                    base_branch: Some("release/v2".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(updated.base_branch, "release/v2");
    }

    #[test]
    fn base_branch_preserved_on_unrelated_update() {
        let r = repo();
        r.create(CreateParams {
            title: "task".into(),
            base_branch: "develop".into(),
            ..Default::default()
        })
        .unwrap();
        let updated = r
            .update(
                "TEST-1",
                UpdateParams {
                    title: Some("renamed".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(updated.base_branch, "develop");
    }

    #[test]
    fn base_branch_in_list() {
        let r = repo();
        r.create(CreateParams {
            title: "a".into(),
            base_branch: "develop".into(),
            ..Default::default()
        })
        .unwrap();
        r.create(CreateParams {
            title: "b".into(),
            ..Default::default()
        })
        .unwrap();
        let tasks = r.list(ListFilter::default()).unwrap();
        assert_eq!(tasks[0].base_branch, "develop");
        assert_eq!(tasks[1].base_branch, "main");
    }

    #[test]
    fn create_with_custom_key() {
        let r = repo();
        let task = r
            .create(CreateParams {
                key: Some("PES-3206".into()),
                title: "Jira task".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(task.key, "PES-3206");
        let fetched = r.get("PES-3206").unwrap();
        assert_eq!(fetched.title, "Jira task");
    }

    #[test]
    fn create_with_duplicate_key_is_idempotent() {
        let r = repo();
        let t1 = r
            .create(CreateParams {
                key: Some("PES-1".into()),
                title: "Original".into(),
                ..Default::default()
            })
            .unwrap();
        let t2 = r
            .create(CreateParams {
                key: Some("PES-1".into()),
                title: "Duplicate".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(t1.key, t2.key);
        assert_eq!(t2.title, "Original"); // returns existing, not new
    }

    #[test]
    fn create_without_key_still_auto_generates() {
        let r = repo();
        let t1 = r
            .create(CreateParams {
                title: "auto".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(t1.key, "TEST-1");
    }
}

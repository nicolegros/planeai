use chrono::Utc;
use rusqlite::{params, Connection};
use std::collections::HashMap;
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

    /// Fetch multiple tasks by key (cross-prefix, ignores project_prefix filter).
    pub fn list_by_keys(&self, keys: &[&str]) -> Result<Vec<Task>, Error> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Storage(e.to_string()))?;
        let mut tasks = Vec::new();
        for key in keys {
            match self.query_task(&conn, key, None) {
                Ok(t) => tasks.push(t),
                Err(Error::NotFound) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(tasks)
    }

    /// Count children for given parent keys (cross-prefix).
    pub fn count_children(&self, parent_keys: &[&str]) -> Result<HashMap<String, usize>, Error> {
        if parent_keys.is_empty() {
            return Ok(HashMap::new());
        }
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Storage(e.to_string()))?;
        let ph: String = parent_keys
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT parent_key, COUNT(*) FROM tasks WHERE parent_key IN ({ph}) GROUP BY parent_key"
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| Error::Storage(e.to_string()))?;
        let params: Vec<&dyn rusqlite::types::ToSql> = parent_keys
            .iter()
            .map(|k| k as &dyn rusqlite::types::ToSql)
            .collect();
        let rows = stmt
            .query_map(params.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
            })
            .map_err(|e| Error::Storage(e.to_string()))?;
        let mut map = HashMap::new();
        for r in rows {
            let (k, c) = r.map_err(|e| Error::Storage(e.to_string()))?;
            map.insert(k, c);
        }
        Ok(map)
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

    fn query_task(
        &self,
        conn: &Connection,
        key: &str,
        prefix_filter: Option<&str>,
    ) -> Result<Task, Error> {
        let (sql, params_vec): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = match prefix_filter {
            Some(p) => (
                "SELECT key, title, description, status, priority, parent_key, base_branch, created_at, updated_at FROM tasks WHERE key = ?1 AND project_prefix = ?2",
                vec![Box::new(key.to_string()), Box::new(p.to_string())],
            ),
            None => (
                "SELECT key, title, description, status, priority, parent_key, base_branch, created_at, updated_at FROM tasks WHERE key = ?1",
                vec![Box::new(key.to_string())],
            ),
        };
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| Error::Storage(e.to_string()))?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        stmt.query_row(params_refs.as_slice(), |row| {
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
    /// Create a new task.
    ///
    /// If `params.key` is `Some`, that key is used directly instead of auto-generating.
    /// Idempotent: if the key already exists within this project, the existing task is
    /// returned unchanged — the remaining `CreateParams` fields are silently discarded.
    /// This supports Jira sync where repeated syncs for the same issue should not
    /// duplicate tasks; the sync loop handles updates separately.
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
            // Key already exists — idempotent create: return existing task only if it
            // belongs to this project (prevents cross-project collision).
            return self.query_task(&conn, &key, Some(&self.prefix));
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
        self.query_task(&conn, key, None)
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
#[path = "sqlite_tests.rs"]
mod tests;

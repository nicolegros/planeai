use chrono::{DateTime, Datelike, SecondsFormat, SubsecRound, Utc};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Row};
use std::collections::HashMap;
use std::sync::Mutex;

use crate::model::{
    CreateParams, ListFilter, Snooze, Status, StoredSnooze, Task, UpdateParams, DEFAULT_BASE_BRANCH,
};
use crate::provider::{Error, TaskProvider};

/// A task `insert_task` wrote, or found under the explicit key it was given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsertedTask {
    pub key: String,
    /// False when a task with the explicit key already existed and was left as is.
    pub inserted: bool,
    /// For a child task, whether its parent had no children before this insert.
    pub first_child: Option<bool>,
}

/// Insert a task inside the caller's transaction, so rows the caller writes alongside it
/// commit or roll back with it. The key is the explicit one or the prefix's next number.
pub fn insert_task(
    tx: &rusqlite::Transaction<'_>,
    prefix: &str,
    params: &CreateParams,
) -> Result<InsertedTask, Error> {
    let storage = |e: rusqlite::Error| Error::Storage(e.to_string());
    tx.execute(
        "INSERT OR IGNORE INTO task_projects (prefix) VALUES (?1)",
        params![prefix],
    )
    .map_err(storage)?;
    let key = match params.key.clone() {
        Some(key) => key,
        None => {
            let seq: i64 = tx
                .query_row(
                    "UPDATE task_projects SET next_seq = next_seq + 1 WHERE prefix = ?1 RETURNING next_seq - 1",
                    params![prefix],
                    |row| row.get(0),
                )
                .map_err(storage)?;
            format!("{prefix}-{seq}")
        }
    };
    let first_child = match &params.parent_key {
        Some(parent_key) => Some(
            !tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM tasks WHERE parent_key = ?1)",
                params![parent_key],
                |row| row.get::<_, bool>(0),
            )
            .map_err(storage)?,
        ),
        None => None,
    };
    let now = Utc::now().to_rfc3339();
    let status = params.status.unwrap_or(Status::Todo);
    let inserted = tx.execute(
        "INSERT OR IGNORE INTO tasks (key, project_prefix, title, description, status, priority, parent_key, base_branch, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![key, prefix, params.title, params.description, status.as_str(), params.priority, params.parent_key, params.base_branch, now, now],
    ).map_err(storage)? > 0;
    if inserted {
        for blocker in &params.blocked_by {
            tx.execute(
                "INSERT INTO task_blockers (task_key, blocked_by_key) VALUES (?1, ?2)",
                params![key, blocker],
            )
            .map_err(storage)?;
        }
        for tag in &params.tags {
            tx.execute(
                "INSERT INTO task_tags (task_key, tag) VALUES (?1, ?2)",
                params![key, tag],
            )
            .map_err(storage)?;
        }
    }
    Ok(InsertedTask {
        key,
        inserted,
        first_child,
    })
}

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
        "ALTER TABLE tasks ADD COLUMN last_visited_at TEXT;
         ALTER TABLE tasks ADD COLUMN last_activity_at TEXT;",
        "ALTER TABLE tasks ADD COLUMN snoozed_until TEXT;
         ALTER TABLE tasks ADD COLUMN snoozed_at TEXT;
         ALTER TABLE tasks ADD COLUMN woke_at TEXT;",
        "ALTER TABLE tasks ADD COLUMN snooze_rev INTEGER NOT NULL DEFAULT 0;",
    ];

    for (i, sql) in migrations.iter().enumerate() {
        if (i as i32) >= version {
            conn.execute_batch(sql)
                .map_err(|e| Error::Storage(e.to_string()))?;
            conn.execute(
                "UPDATE task_schema_version SET version = ?1",
                params![(i + 1) as i64],
            )
            .map_err(|e| Error::Storage(e.to_string()))?;
        }
    }
    Ok(())
}

/// A task's visit and activity, which decide whether it is unread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Visit {
    pub last_visited_at: Option<DateTime<Utc>>,
    pub last_activity_at: Option<DateTime<Utc>>,
}

/// Stored to the millisecond with a fixed width, so a reader parsing it gets back the same instant.
fn format_time(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Millis, false)
}

fn parse_time(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

fn read_visit(conn: &Connection, prefix: &str, key: &str) -> Result<Visit, Error> {
    let (visited, activity) = conn
        .query_row(
            "SELECT last_visited_at, last_activity_at FROM tasks WHERE key = ?1 AND project_prefix = ?2",
            params![key, prefix],
            |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .optional()
        .map_err(|e| Error::Storage(e.to_string()))?
        .ok_or(Error::NotFound)?;
    Ok(Visit {
        last_visited_at: visited.as_deref().and_then(parse_time),
        last_activity_at: activity.as_deref().and_then(parse_time),
    })
}

#[derive(Clone, Copy)]
enum VisitColumn {
    Visited,
    Activity,
}

impl VisitColumn {
    fn as_str(self) -> &'static str {
        match self {
            Self::Visited => "last_visited_at",
            Self::Activity => "last_activity_at",
        }
    }
}

fn set_time(
    conn: &Connection,
    column: VisitColumn,
    prefix: &str,
    key: &str,
    at: DateTime<Utc>,
) -> Result<Visit, Error> {
    let updated = conn
        .execute(
            &format!(
                "UPDATE tasks SET {} = ?1 WHERE key = ?2 AND project_prefix = ?3",
                column.as_str()
            ),
            params![format_time(at.trunc_subsecs(3)), key, prefix],
        )
        .map_err(|e| Error::Storage(e.to_string()))?;
    if updated == 0 {
        return Err(Error::NotFound);
    }
    read_visit(conn, prefix, key)
}

/// Records that an agent working on the task finished a turn, asked for attention, or exited.
pub fn record_activity(
    conn: &Connection,
    prefix: &str,
    key: &str,
    at: DateTime<Utc>,
) -> Result<Visit, Error> {
    set_time(conn, VisitColumn::Activity, prefix, key, at)
}

pub fn mark_visited(
    conn: &Connection,
    prefix: &str,
    key: &str,
    at: DateTime<Utc>,
) -> Result<Visit, Error> {
    set_time(conn, VisitColumn::Visited, prefix, key, at)
}

/// Moves the visit to just before the last activity; a task without activity stays as it is.
pub fn mark_unread(conn: &Connection, prefix: &str, key: &str) -> Result<Visit, Error> {
    let visit = read_visit(conn, prefix, key)?;
    match visit.last_activity_at {
        Some(activity) => set_time(
            conn,
            VisitColumn::Visited,
            prefix,
            key,
            activity - chrono::Duration::milliseconds(1),
        ),
        None => Ok(visit),
    }
}

fn read_status_and_snooze(
    conn: &Connection,
    prefix: &str,
    key: &str,
) -> Result<(Status, StoredSnooze), Error> {
    let (status, until, at, woke, rev) = conn
        .query_row(
            "SELECT status, snoozed_until, snoozed_at, woke_at, snooze_rev FROM tasks WHERE key = ?1 AND project_prefix = ?2",
            params![key, prefix],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .optional()
        .map_err(|e| Error::Storage(e.to_string()))?
        .ok_or(Error::NotFound)?;
    let status = Status::parse(&status).ok_or(Error::InvalidStatus(status))?;
    Ok((
        status,
        StoredSnooze {
            snooze: snooze_from_columns(until, at, woke),
            snooze_rev: rev,
        },
    ))
}

fn snooze_from_columns(until: Option<String>, at: Option<String>, woke: Option<String>) -> Snooze {
    Snooze {
        snoozed_until: until.as_deref().and_then(parse_time),
        snoozed_at: at.as_deref().and_then(parse_time),
        woke_at: woke.as_deref().and_then(parse_time),
    }
}

/// A Done task is finished business, so it can never sleep.
fn ensure_not_done(conn: &Connection, prefix: &str, key: &str) -> Result<(), Error> {
    let (status, _) = read_status_and_snooze(conn, prefix, key)?;
    if status == Status::Done {
        return Err(Error::Rejected("a done task cannot be snoozed".into()));
    }
    Ok(())
}

fn write_snooze(
    conn: &Connection,
    prefix: &str,
    key: &str,
    snooze: Snooze,
) -> Result<StoredSnooze, Error> {
    let times = [snooze.snoozed_until, snooze.snoozed_at, snooze.woke_at];
    // Times are stored as fixed-width RFC 3339 text, which only holds four-digit years.
    if times.into_iter().flatten().any(|at| at.year() > 9999) {
        return Err(Error::Rejected("the snooze time is too far ahead".into()));
    }
    let format = |at: Option<DateTime<Utc>>| at.map(|at| format_time(at.trunc_subsecs(3)));
    conn.execute(
        "UPDATE tasks SET snoozed_until = ?1, snoozed_at = ?2, woke_at = ?3, snooze_rev = snooze_rev + 1
         WHERE key = ?4 AND project_prefix = ?5",
        params![
            format(snooze.snoozed_until),
            format(snooze.snoozed_at),
            format(snooze.woke_at),
            key,
            prefix
        ],
    )
    .map_err(|e| Error::Storage(e.to_string()))?;
    Ok(read_status_and_snooze(conn, prefix, key)?.1)
}

/// Hides the task until `until`; its status and agents are left alone.
pub fn snooze(
    conn: &Connection,
    prefix: &str,
    key: &str,
    until: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<StoredSnooze, Error> {
    ensure_not_done(conn, prefix, key)?;
    if until <= now {
        return Err(Error::Rejected("the snooze time has passed".into()));
    }
    write_snooze(
        conn,
        prefix,
        key,
        Snooze {
            snoozed_until: Some(until),
            snoozed_at: Some(now),
            woke_at: None,
        },
    )
}

/// The user woke the task: no trace of the snooze is left.
pub fn wake(conn: &Connection, prefix: &str, key: &str) -> Result<StoredSnooze, Error> {
    write_snooze(conn, prefix, key, Snooze::default())
}

/// Puts back a snooze exactly as it was, to undo a snooze or a wake; as a new write, it gets a new
/// revision.
pub fn restore_snooze(
    conn: &Connection,
    prefix: &str,
    key: &str,
    snooze: Snooze,
) -> Result<StoredSnooze, Error> {
    if snooze != Snooze::default() {
        ensure_not_done(conn, prefix, key)?;
    }
    write_snooze(conn, prefix, key, snooze)
}

/// Wakes a snoozed task before its time because an agent needs the user, returning the woken
/// snooze; `None`, with nothing written, when the task was not snoozed at `at` or was snoozed after it.
pub fn wake_early(
    conn: &Connection,
    prefix: &str,
    key: &str,
    at: DateTime<Utc>,
) -> Result<Option<StoredSnooze>, Error> {
    let at = format_time(at.trunc_subsecs(3));
    // Every time column has the same fixed width, so text order is time order.
    let woke = conn
        .execute(
            "UPDATE tasks SET woke_at = ?1, snooze_rev = snooze_rev + 1
             WHERE key = ?2 AND project_prefix = ?3 AND status != 'done'
               AND woke_at IS NULL AND snoozed_at < ?1 AND snoozed_until > ?1",
            params![at, key, prefix],
        )
        .map_err(|e| Error::Storage(e.to_string()))?;
    if woke == 0 {
        return Ok(None);
    }
    Ok(Some(read_status_and_snooze(conn, prefix, key)?.1))
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

    /// Open an existing file-backed DB for reads without running migrations or
    /// creating the repository's project row.
    pub fn open_read_only(db_path: &str, prefix: &str) -> Result<Self, Error> {
        let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(Self {
            conn: Mutex::new(conn),
            prefix: prefix.to_string(),
        })
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
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
            })
            .map_err(|e| Error::Storage(e.to_string()))?;
        let mut map = HashMap::new();
        for r in rows {
            let (k, c) = r.map_err(|e| Error::Storage(e.to_string()))?;
            map.insert(k, c);
        }
        Ok(map)
    }

    /// Create a task and atomically report whether its parent had no children
    /// immediately before this committed assignment.
    pub fn create_with_first_child_assignment(
        &self,
        params: CreateParams,
    ) -> Result<(Task, Option<bool>), Error> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|e| Error::Storage(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| Error::Storage(e.to_string()))?;
        let created = insert_task(&tx, &self.prefix, &params)?;
        tx.commit().map_err(|e| Error::Storage(e.to_string()))?;
        let task = self.query_task(&conn, &created.key, Some(&self.prefix))?;
        Ok((task, created.first_child.filter(|_| created.inserted)))
    }

    /// Change a task's parent and atomically report whether the new parent had
    /// no children before this assignment committed.
    pub fn set_parent_with_first_child_assignment(
        &self,
        key: &str,
        parent_key: Option<String>,
    ) -> Result<(Task, Option<bool>), Error> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|error| Error::Storage(error.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|error| Error::Storage(error.to_string()))?;
        let existing_parent: Option<String> = tx
            .query_row(
                "SELECT parent_key FROM tasks WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => Error::NotFound,
                _ => Error::Storage(error.to_string()),
            })?;
        if existing_parent == parent_key {
            tx.commit()
                .map_err(|error| Error::Storage(error.to_string()))?;
            return Ok((self.query_task(&conn, key, None)?, None));
        }
        let is_first = match &parent_key {
            Some(parent_key) => !tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM tasks WHERE parent_key = ?1)",
                    params![parent_key],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(|error| Error::Storage(error.to_string()))?,
            None => false,
        };
        tx.execute(
            "UPDATE tasks SET parent_key = ?1, updated_at = ?2 WHERE key = ?3",
            params![parent_key, Utc::now().to_rfc3339(), key],
        )
        .map_err(|error| Error::Storage(error.to_string()))?;
        tx.commit()
            .map_err(|error| Error::Storage(error.to_string()))?;
        let task = self.query_task(&conn, key, None)?;
        Ok((task, parent_key.map(|_| is_first)))
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
            created_at: parse_time(&row.created_at).unwrap_or_else(Utc::now),
            updated_at: parse_time(&row.updated_at).unwrap_or_else(Utc::now),
            last_visited_at: row.last_visited_at.as_deref().and_then(parse_time),
            last_activity_at: row.last_activity_at.as_deref().and_then(parse_time),
            snooze: snooze_from_columns(row.snoozed_until, row.snoozed_at, row.woke_at),
            snooze_rev: row.snooze_rev,
        })
    }

    fn query_task(
        &self,
        conn: &Connection,
        key: &str,
        prefix_filter: Option<&str>,
    ) -> Result<Task, Error> {
        let row = match prefix_filter {
            Some(prefix) => conn.query_row(
                &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE key = ?1 AND project_prefix = ?2"),
                params![key, prefix],
                TaskRow::read,
            ),
            None => conn.query_row(
                &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE key = ?1"),
                params![key],
                TaskRow::read,
            ),
        }
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Error::NotFound,
            _ => Error::Storage(e.to_string()),
        })?;
        self.row_to_task(conn, row)
    }
}

/// Columns `TaskRow::read` expects, in order.
const TASK_COLUMNS: &str = "key, title, description, status, priority, parent_key, base_branch, created_at, updated_at, last_visited_at, last_activity_at, snoozed_until, snoozed_at, woke_at, snooze_rev";

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
    last_visited_at: Option<String>,
    last_activity_at: Option<String>,
    snoozed_until: Option<String>,
    snoozed_at: Option<String>,
    woke_at: Option<String>,
    snooze_rev: i64,
}

impl TaskRow {
    fn read(row: &Row) -> rusqlite::Result<Self> {
        Ok(Self {
            key: row.get(0)?,
            title: row.get(1)?,
            description: row.get(2)?,
            status: row.get(3)?,
            priority: row.get(4)?,
            parent_key: row.get(5)?,
            base_branch: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
            last_visited_at: row.get(9)?,
            last_activity_at: row.get(10)?,
            snoozed_until: row.get(11)?,
            snoozed_at: row.get(12)?,
            woke_at: row.get(13)?,
            snooze_rev: row.get(14)?,
        })
    }
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

        self.query_task(&conn, &key, Some(&self.prefix))
    }

    fn create_with_first_child_assignment(
        &self,
        params: CreateParams,
    ) -> Result<(Task, Option<bool>), Error> {
        SqliteRepository::create_with_first_child_assignment(self, params)
    }

    fn set_parent_with_first_child_assignment(
        &self,
        key: &str,
        parent_key: Option<String>,
    ) -> Result<(Task, Option<bool>), Error> {
        SqliteRepository::set_parent_with_first_child_assignment(self, key, parent_key)
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

        let mut sql = format!("SELECT {TASK_COLUMNS} FROM tasks WHERE project_prefix = ?1");
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
            .query_map(params_refs.as_slice(), TaskRow::read)
            .map_err(|e| Error::Storage(e.to_string()))?;

        let mut tasks = Vec::new();
        for row in rows {
            let row = row.map_err(|e| Error::Storage(e.to_string()))?;
            tasks.push(self.row_to_task(&conn, row)?);
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

        // Every status write lands here, so this is where Done ends a snooze.
        conn.execute(
            "UPDATE tasks SET title = ?1, description = ?2, status = ?3, priority = ?4, parent_key = ?5, base_branch = ?6, updated_at = ?7,
               snoozed_until = CASE WHEN ?3 = 'done' THEN NULL ELSE snoozed_until END,
               snoozed_at = CASE WHEN ?3 = 'done' THEN NULL ELSE snoozed_at END,
               woke_at = CASE WHEN ?3 = 'done' THEN NULL ELSE woke_at END,
               snooze_rev = snooze_rev + CASE WHEN ?3 = 'done' THEN 1 ELSE 0 END
             WHERE key = ?8",
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

        self.query_task(&conn, key, None)
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

#[cfg(test)]
mod read_only_tests {
    use super::*;

    #[test]
    fn read_only_open_does_not_create_a_project() {
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("tasks.db");
        let writer = SqliteRepository::open(path.to_str().unwrap(), "TASK").unwrap();
        writer
            .create(CreateParams {
                title: "existing task".to_string(),
                ..Default::default()
            })
            .unwrap();
        drop(writer);

        let reader = SqliteRepository::open_read_only(path.to_str().unwrap(), "PLUGIN").unwrap();
        assert_eq!(reader.get("TASK-1").unwrap().title, "existing task");
        drop(reader);

        let conn = Connection::open(path).unwrap();
        let projects = conn
            .prepare("SELECT prefix FROM task_projects ORDER BY prefix")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(projects, ["TASK"]);
    }
}

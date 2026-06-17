use rusqlite::{params, Connection};

use crate::Error;

pub fn migrate(conn: &Connection) -> Result<(), Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS jira_schema_version (version INTEGER NOT NULL);
         INSERT OR IGNORE INTO jira_schema_version (version) SELECT 0 WHERE NOT EXISTS (SELECT 1 FROM jira_schema_version);",
    )?;

    let version: i32 =
        conn.query_row("SELECT version FROM jira_schema_version", [], |r| r.get(0))?;

    let migrations: &[&str] = &[
        // v1: jira_issues table + FK column on tasks
        "CREATE TABLE IF NOT EXISTS jira_issues (
            issue_key TEXT PRIMARY KEY,
            jira_project TEXT NOT NULL,
            summary TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL,
            priority TEXT,
            labels TEXT NOT NULL DEFAULT '[]',
            sync_status TEXT NOT NULL DEFAULT 'synced',
            last_synced_at TEXT NOT NULL
        );",
    ];

    for (i, sql) in migrations.iter().enumerate() {
        if (i as i32) >= version {
            conn.execute_batch(sql)?;
            conn.execute(
                "UPDATE jira_schema_version SET version = ?1",
                params![i + 1],
            )?;
        }
    }

    // Add jira_issue_key FK column to tasks if it doesn't exist yet
    let has_column: bool = conn
        .prepare("PRAGMA table_info(tasks)")
        .and_then(|mut stmt| {
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            let mut found = false;
            for r in rows {
                if r.map(|name| name == "jira_issue_key").unwrap_or(false) {
                    found = true;
                    break;
                }
            }
            Ok(found)
        })
        .unwrap_or(true); // if tasks table doesn't exist, skip

    if !has_column {
        conn.execute_batch(
            "ALTER TABLE tasks ADD COLUMN jira_issue_key TEXT REFERENCES jira_issues(issue_key);",
        )?;
    }

    Ok(())
}

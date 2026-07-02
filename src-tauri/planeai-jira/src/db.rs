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
        // v1: jira_issues table
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
        // v2: jira_task_links — owns the mapping between tasks and jira issues
        "CREATE TABLE IF NOT EXISTS jira_task_links (
            task_key TEXT NOT NULL,
            issue_key TEXT NOT NULL REFERENCES jira_issues(issue_key),
            PRIMARY KEY (task_key),
            UNIQUE (issue_key)
        );",
        // v3: source_name — identifies which config source this issue originated from
        "ALTER TABLE jira_issues ADD COLUMN source_name TEXT NOT NULL DEFAULT '';",
    ];

    for (i, sql) in migrations.iter().enumerate() {
        if (i as i32) >= version {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(sql)?;
            tx.execute(
                "UPDATE jira_schema_version SET version = ?1",
                params![i + 1],
            )?;
            tx.commit()?;
        }
    }

    Ok(())
}

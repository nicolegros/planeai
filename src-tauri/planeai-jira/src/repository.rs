use chrono::Utc;
use rusqlite::{params, Connection};
use std::sync::Mutex;

use crate::db::migrate;
use crate::model::{JiraIssue, SyncStatus};
use crate::Error;

pub struct JiraRepository {
    conn: Mutex<Connection>,
}

fn row_to_issue(row: &rusqlite::Row) -> rusqlite::Result<JiraIssue> {
    // SELECT columns: issue_key(0), summary(1), description(2), status(3),
    //                  priority(4), labels(5), sync_status(6), last_synced_at(7), source_name(8)
    let labels_json: String = row.get(5)?;
    let sync_status_str: String = row.get(6)?;
    let ts: String = row.get(7)?;
    let source_name: String = row.get::<_, Option<String>>(8)?.unwrap_or_default();
    let issue_key: String = row.get(0)?;
    Ok(JiraIssue {
        summary: row.get(1)?,
        description: row.get(2)?,
        status: row.get(3)?,
        priority: row.get(4)?,
        labels: serde_json::from_str(&labels_json).unwrap_or_else(|e| {
            tracing::warn!(issue_key = %issue_key, error = %e, "invalid labels JSON, defaulting to empty");
            Vec::new()
        }),
        sync_status: SyncStatus::parse(&sync_status_str).unwrap_or_else(|| {
            tracing::warn!(issue_key = %issue_key, value = %sync_status_str, "unknown sync_status, defaulting to Synced");
            SyncStatus::Synced
        }),
        last_synced_at: chrono::DateTime::parse_from_rfc3339(&ts)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|e| {
                tracing::warn!(issue_key = %issue_key, error = %e, "invalid last_synced_at, defaulting to now");
                Utc::now()
            }),
        issue_key,
        source_name,
    })
}

impl JiraRepository {
    pub fn new(conn: Connection) -> Result<Self, Error> {
        migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn upsert_issue(&self, issue: &JiraIssue) -> Result<(), Error> {
        let labels_json =
            serde_json::to_string(&issue.labels).map_err(|e| Error::Storage(e.to_string()))?;
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Storage(e.to_string()))?;
        conn.execute(
            "INSERT INTO jira_issues (issue_key, jira_project, summary, description, status, priority, labels, sync_status, last_synced_at, source_name)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(issue_key) DO UPDATE SET
                jira_project = excluded.jira_project,
                summary = excluded.summary,
                description = excluded.description,
                status = excluded.status,
                priority = excluded.priority,
                labels = excluded.labels,
                sync_status = excluded.sync_status,
                last_synced_at = excluded.last_synced_at,
                source_name = excluded.source_name",
            params![
                issue.issue_key,
                issue.source_name, // write source_name to jira_project column for backwards compat
                issue.summary,
                issue.description,
                issue.status,
                issue.priority,
                labels_json,
                issue.sync_status.as_str(),
                issue.last_synced_at.to_rfc3339(),
                issue.source_name,
            ],
        )?;
        Ok(())
    }

    pub fn mark_departed(&self, issue_keys: &[&str]) -> Result<(), Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Storage(e.to_string()))?;
        for key in issue_keys {
            conn.execute(
                "UPDATE jira_issues SET sync_status = 'departed' WHERE issue_key = ?1",
                params![key],
            )?;
        }
        Ok(())
    }

    pub fn mark_synced(&self, issue_key: &str) -> Result<(), Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Storage(e.to_string()))?;
        conn.execute(
            "UPDATE jira_issues SET sync_status = 'synced', last_synced_at = ?1 WHERE issue_key = ?2",
            params![Utc::now().to_rfc3339(), issue_key],
        )?;
        Ok(())
    }

    pub fn get_issue(&self, issue_key: &str) -> Result<Option<JiraIssue>, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Storage(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT issue_key, summary, description, status, priority, labels, sync_status, last_synced_at, source_name FROM jira_issues WHERE issue_key = ?1",
        )?;

        let result = stmt.query_row(params![issue_key], row_to_issue);

        match result {
            Ok(issue) => Ok(Some(issue)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn list_synced_keys(&self, source_name: &str) -> Result<Vec<String>, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Storage(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT issue_key FROM jira_issues WHERE source_name = ?1 AND sync_status = 'synced'",
        )?;
        let rows = stmt.query_map(params![source_name], |row| row.get(0))?;
        let mut keys = Vec::new();
        for r in rows {
            keys.push(r?);
        }
        Ok(keys)
    }

    pub fn list_active_issue_keys(&self) -> Result<Vec<String>, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Storage(e.to_string()))?;
        let mut stmt =
            conn.prepare("SELECT issue_key FROM jira_issues WHERE sync_status = 'synced'")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        let mut keys = Vec::new();
        for r in rows {
            keys.push(r?);
        }
        Ok(keys)
    }

    pub fn get_task_issue_key(&self, task_key: &str) -> Result<Option<String>, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Storage(e.to_string()))?;
        let result = conn.query_row(
            "SELECT issue_key FROM jira_task_links WHERE task_key = ?1",
            params![task_key],
            |row| row.get(0),
        );
        match result {
            Ok(key) => Ok(key),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn setup() -> JiraRepository {
        let conn = Connection::open_in_memory().unwrap();
        JiraRepository::new(conn).unwrap()
    }

    fn sample_issue(key: &str) -> JiraIssue {
        JiraIssue {
            issue_key: key.to_string(),
            summary: "Test issue".to_string(),
            description: "A description".to_string(),
            status: "To Do".to_string(),
            priority: Some("High".to_string()),
            labels: vec!["backend".to_string(), "urgent".to_string()],
            sync_status: SyncStatus::Synced,
            last_synced_at: Utc::now(),
            source_name: "proj".to_string(),
        }
    }

    #[test]
    fn migration_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();
    }

    #[test]
    fn migration_creates_tables() {
        let repo = setup();
        repo.upsert_issue(&sample_issue("PROJ-1")).unwrap();
    }

    #[test]
    fn upsert_inserts_new_issue() {
        let repo = setup();
        repo.upsert_issue(&sample_issue("PROJ-1")).unwrap();

        let issue = repo.get_issue("PROJ-1").unwrap().unwrap();
        assert_eq!(issue.issue_key, "PROJ-1");
        assert_eq!(issue.summary, "Test issue");
        assert_eq!(issue.labels, vec!["backend", "urgent"]);
        assert_eq!(issue.priority, Some("High".to_string()));
    }

    #[test]
    fn upsert_updates_existing_issue() {
        let repo = setup();
        repo.upsert_issue(&sample_issue("PROJ-1")).unwrap();

        let mut updated = sample_issue("PROJ-1");
        updated.summary = "Updated summary".to_string();
        updated.status = "In Progress".to_string();
        repo.upsert_issue(&updated).unwrap();

        let issue = repo.get_issue("PROJ-1").unwrap().unwrap();
        assert_eq!(issue.summary, "Updated summary");
        assert_eq!(issue.status, "In Progress");
    }

    #[test]
    fn get_issue_returns_none_for_missing() {
        let repo = setup();
        assert_eq!(repo.get_issue("NOPE-1").unwrap(), None);
    }

    #[test]
    fn mark_departed_sets_sync_status() {
        let repo = setup();
        repo.upsert_issue(&sample_issue("PROJ-1")).unwrap();
        repo.upsert_issue(&sample_issue("PROJ-2")).unwrap();

        repo.mark_departed(&["PROJ-1", "PROJ-2"]).unwrap();

        let i1 = repo.get_issue("PROJ-1").unwrap().unwrap();
        let i2 = repo.get_issue("PROJ-2").unwrap().unwrap();
        assert_eq!(i1.sync_status, SyncStatus::Departed);
        assert_eq!(i2.sync_status, SyncStatus::Departed);
    }

    #[test]
    fn mark_synced_updates_status_and_timestamp() {
        let repo = setup();
        let mut issue = sample_issue("PROJ-1");
        issue.sync_status = SyncStatus::Departed;
        repo.upsert_issue(&issue).unwrap();

        repo.mark_synced("PROJ-1").unwrap();

        let fetched = repo.get_issue("PROJ-1").unwrap().unwrap();
        assert_eq!(fetched.sync_status, SyncStatus::Synced);
    }

    #[test]
    fn list_synced_keys_filters_by_project_and_status() {
        let repo = setup();
        repo.upsert_issue(&sample_issue("PROJ-1")).unwrap();
        repo.upsert_issue(&sample_issue("PROJ-2")).unwrap();

        let mut other = sample_issue("OTHER-1");
        other.source_name = "other".to_string();
        repo.upsert_issue(&other).unwrap();

        repo.mark_departed(&["PROJ-2"]).unwrap();

        let keys = repo.list_synced_keys("proj").unwrap();
        assert_eq!(keys, vec!["PROJ-1"]);
    }

    #[test]
    fn list_active_issue_keys_returns_only_synced() {
        let repo = setup();
        repo.upsert_issue(&sample_issue("PROJ-1")).unwrap();
        repo.upsert_issue(&sample_issue("PROJ-2")).unwrap();

        let mut other = sample_issue("OTHER-1");
        other.source_name = "other".to_string();
        repo.upsert_issue(&other).unwrap();

        repo.mark_departed(&["PROJ-2"]).unwrap();

        let mut keys = repo.list_active_issue_keys().unwrap();
        keys.sort();
        assert_eq!(keys, vec!["OTHER-1", "PROJ-1"]);
    }

    #[test]
    fn source_name_round_trips_through_upsert_and_get() {
        let repo = setup();
        let mut issue = sample_issue("PROJ-1");
        issue.source_name = "my-source".to_string();
        repo.upsert_issue(&issue).unwrap();

        let fetched = repo.get_issue("PROJ-1").unwrap().unwrap();
        assert_eq!(fetched.source_name, "my-source");
    }

    #[test]
    fn source_name_defaults_to_empty_for_legacy_issues() {
        // Insert via raw SQL without source_name to simulate pre-migration row
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO jira_issues (issue_key, jira_project, summary, description, status, priority, labels, sync_status, last_synced_at) VALUES ('PROJ-1', 'PROJ', 'Legacy', '', 'To Do', 'High', '[]', 'synced', '2024-01-01T00:00:00Z')",
            [],
        ).unwrap();
        let repo = JiraRepository::new(conn).unwrap();
        let fetched = repo.get_issue("PROJ-1").unwrap().unwrap();
        assert_eq!(fetched.source_name, "");
    }
}

use chrono::Utc;
use rusqlite::{params, Connection};

use crate::db::migrate;
use crate::model::JiraIssue;
use crate::Error;

pub struct JiraRepository {
    conn: Connection,
}

impl JiraRepository {
    pub fn new(conn: Connection) -> Result<Self, Error> {
        migrate(&conn)?;
        Ok(Self { conn })
    }

    pub fn upsert_issue(&self, issue: &JiraIssue) -> Result<(), Error> {
        let labels_json =
            serde_json::to_string(&issue.labels).map_err(|e| Error::Storage(e.to_string()))?;
        self.conn.execute(
            "INSERT INTO jira_issues (issue_key, jira_project, summary, description, status, priority, labels, sync_status, last_synced_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(issue_key) DO UPDATE SET
                jira_project = excluded.jira_project,
                summary = excluded.summary,
                description = excluded.description,
                status = excluded.status,
                priority = excluded.priority,
                labels = excluded.labels,
                sync_status = excluded.sync_status,
                last_synced_at = excluded.last_synced_at",
            params![
                issue.issue_key,
                issue.jira_project,
                issue.summary,
                issue.description,
                issue.status,
                issue.priority,
                labels_json,
                issue.sync_status,
                issue.last_synced_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn mark_stale(&self, issue_keys: &[&str]) -> Result<(), Error> {
        for key in issue_keys {
            self.conn.execute(
                "UPDATE jira_issues SET sync_status = 'stale' WHERE issue_key = ?1",
                params![key],
            )?;
        }
        Ok(())
    }

    pub fn mark_synced(&self, issue_key: &str) -> Result<(), Error> {
        self.conn.execute(
            "UPDATE jira_issues SET sync_status = 'synced', last_synced_at = ?1 WHERE issue_key = ?2",
            params![Utc::now().to_rfc3339(), issue_key],
        )?;
        Ok(())
    }

    pub fn get_issue(&self, issue_key: &str) -> Result<Option<JiraIssue>, Error> {
        let mut stmt = self.conn.prepare(
            "SELECT issue_key, jira_project, summary, description, status, priority, labels, sync_status, last_synced_at FROM jira_issues WHERE issue_key = ?1",
        )?;

        let result = stmt.query_row(params![issue_key], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
            ))
        });

        match result {
            Ok((issue_key, jira_project, summary, description, status, priority, labels_json, sync_status, last_synced_at)) => {
                let labels: Vec<String> = serde_json::from_str(&labels_json).unwrap_or_default();
                let last_synced_at = chrono::DateTime::parse_from_rfc3339(&last_synced_at)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                Ok(Some(JiraIssue {
                    issue_key,
                    jira_project,
                    summary,
                    description,
                    status,
                    priority,
                    labels,
                    sync_status,
                    last_synced_at,
                }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn list_synced_keys(&self, jira_project: &str) -> Result<Vec<String>, Error> {
        let mut stmt = self.conn.prepare(
            "SELECT issue_key FROM jira_issues WHERE jira_project = ?1 AND sync_status = 'synced'",
        )?;
        let rows = stmt.query_map(params![jira_project], |row| row.get(0))?;
        let mut keys = Vec::new();
        for r in rows {
            keys.push(r?);
        }
        Ok(keys)
    }

    pub fn get_task_issue_key(&self, task_key: &str) -> Result<Option<String>, Error> {
        let result = self.conn.query_row(
            "SELECT jira_issue_key FROM tasks WHERE key = ?1",
            params![task_key],
            |row| row.get(0),
        );
        match result {
            Ok(key) => Ok(key),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn link_task(&self, task_key: &str, issue_key: &str) -> Result<(), Error> {
        self.conn.execute(
            "UPDATE tasks SET jira_issue_key = ?1 WHERE key = ?2",
            params![issue_key, task_key],
        )?;
        Ok(())
    }

    pub fn find_task_by_issue_key(&self, issue_key: &str) -> Result<Option<String>, Error> {
        let result = self.conn.query_row(
            "SELECT key FROM tasks WHERE jira_issue_key = ?1",
            params![issue_key],
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
        planeai_tasks::sqlite::migrate(&conn).unwrap();
        JiraRepository::new(conn).unwrap()
    }

    fn sample_issue(key: &str) -> JiraIssue {
        JiraIssue {
            issue_key: key.to_string(),
            jira_project: "PROJ".to_string(),
            summary: "Test issue".to_string(),
            description: "A description".to_string(),
            status: "To Do".to_string(),
            priority: Some("High".to_string()),
            labels: vec!["backend".to_string(), "urgent".to_string()],
            sync_status: "synced".to_string(),
            last_synced_at: Utc::now(),
        }
    }

    #[test]
    fn migration_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        planeai_tasks::sqlite::migrate(&conn).unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();
    }

    #[test]
    fn migration_creates_jira_issues_table_and_fk_column() {
        let repo = setup();
        repo.upsert_issue(&sample_issue("PROJ-1")).unwrap();

        repo.conn
            .execute("INSERT INTO task_projects (prefix) VALUES ('TST')", [])
            .unwrap();
        repo.conn
            .execute(
                "INSERT INTO tasks (key, project_prefix, title, status, created_at, updated_at) VALUES ('TST-1', 'TST', 'test', 'todo', '2024-01-01', '2024-01-01')",
                [],
            )
            .unwrap();
        repo.link_task("TST-1", "PROJ-1").unwrap();
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
    fn mark_stale_sets_sync_status() {
        let repo = setup();
        repo.upsert_issue(&sample_issue("PROJ-1")).unwrap();
        repo.upsert_issue(&sample_issue("PROJ-2")).unwrap();

        repo.mark_stale(&["PROJ-1", "PROJ-2"]).unwrap();

        let i1 = repo.get_issue("PROJ-1").unwrap().unwrap();
        let i2 = repo.get_issue("PROJ-2").unwrap().unwrap();
        assert_eq!(i1.sync_status, "stale");
        assert_eq!(i2.sync_status, "stale");
    }

    #[test]
    fn mark_synced_updates_status_and_timestamp() {
        let repo = setup();
        let mut issue = sample_issue("PROJ-1");
        issue.sync_status = "stale".to_string();
        repo.upsert_issue(&issue).unwrap();

        repo.mark_synced("PROJ-1").unwrap();

        let fetched = repo.get_issue("PROJ-1").unwrap().unwrap();
        assert_eq!(fetched.sync_status, "synced");
    }

    #[test]
    fn list_synced_keys_filters_by_project_and_status() {
        let repo = setup();
        repo.upsert_issue(&sample_issue("PROJ-1")).unwrap();
        repo.upsert_issue(&sample_issue("PROJ-2")).unwrap();

        let mut other = sample_issue("OTHER-1");
        other.jira_project = "OTHER".to_string();
        repo.upsert_issue(&other).unwrap();

        repo.mark_stale(&["PROJ-2"]).unwrap();

        let keys = repo.list_synced_keys("PROJ").unwrap();
        assert_eq!(keys, vec!["PROJ-1"]);
    }

    #[test]
    fn link_task_and_get_task_issue_key() {
        let repo = setup();
        repo.upsert_issue(&sample_issue("PROJ-1")).unwrap();

        repo.conn
            .execute("INSERT INTO task_projects (prefix) VALUES ('TST')", [])
            .unwrap();
        repo.conn
            .execute(
                "INSERT INTO tasks (key, project_prefix, title, status, created_at, updated_at) VALUES ('TST-1', 'TST', 'task', 'todo', '2024-01-01', '2024-01-01')",
                [],
            )
            .unwrap();

        assert_eq!(repo.get_task_issue_key("TST-1").unwrap(), None);

        repo.link_task("TST-1", "PROJ-1").unwrap();
        assert_eq!(
            repo.get_task_issue_key("TST-1").unwrap(),
            Some("PROJ-1".to_string())
        );
    }

    #[test]
    fn find_task_by_issue_key_works() {
        let repo = setup();
        repo.upsert_issue(&sample_issue("PROJ-1")).unwrap();

        repo.conn
            .execute("INSERT INTO task_projects (prefix) VALUES ('TST')", [])
            .unwrap();
        repo.conn
            .execute(
                "INSERT INTO tasks (key, project_prefix, title, status, created_at, updated_at) VALUES ('TST-1', 'TST', 'task', 'todo', '2024-01-01', '2024-01-01')",
                [],
            )
            .unwrap();

        assert_eq!(repo.find_task_by_issue_key("PROJ-1").unwrap(), None);

        repo.link_task("TST-1", "PROJ-1").unwrap();
        assert_eq!(
            repo.find_task_by_issue_key("PROJ-1").unwrap(),
            Some("TST-1".to_string())
        );
    }
}

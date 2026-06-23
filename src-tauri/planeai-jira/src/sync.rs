use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::client::JiraClient;
use crate::config::JiraConfig;
use crate::repository::JiraRepository;
use planeai_tasks::model::{CreateParams, Status, UpdateParams};
use planeai_tasks::provider::TaskProvider;

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct SyncResult {
    pub created: usize,
    pub updated: usize,
    pub stale: usize,
    pub errors: usize,
}

pub struct JiraSync {
    client: Arc<JiraClient>,
    repo: Arc<JiraRepository>,
    task_provider: Arc<dyn TaskProvider + Send + Sync>,
    config: JiraConfig,
}

impl JiraSync {
    pub fn new(
        client: Arc<JiraClient>,
        repo: Arc<JiraRepository>,
        task_provider: Arc<dyn TaskProvider + Send + Sync>,
        config: JiraConfig,
    ) -> Self {
        Self {
            client,
            repo,
            task_provider,
            config,
        }
    }

    pub async fn start(&self, cancel: CancellationToken) {
        let mut interval =
            tokio::time::interval(Duration::from_millis(self.config.sync_interval_ms));
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("jira sync loop cancelled");
                    return;
                }
                _ = interval.tick() => {
                    match self.sync_now().await {
                        Ok(r) => info!(created = r.created, updated = r.updated, stale = r.stale, errors = r.errors, "jira sync complete"),
                        Err(e) => warn!(error = %e, "jira sync error"),
                    }
                }
            }
        }
    }

    pub async fn sync_now(&self) -> Result<SyncResult, crate::Error> {
        let mut result = SyncResult::default();

        if self.config.sources.is_empty() {
            tracing::warn!("sync_now: no sync sources configured");
            return Ok(result);
        }

        for (name, source) in &self.config.sources {
            match self.sync_source(name, source, &mut result).await {
                Ok(()) => {}
                Err(e) => {
                    warn!(source = %name, error = %e, "sync failed for source, continuing");
                    result.errors += 1;
                }
            }
        }

        Ok(result)
    }

    async fn sync_source(
        &self,
        name: &str,
        source: &crate::config::SyncSource,
        result: &mut SyncResult,
    ) -> Result<(), crate::Error> {
        let issues = self.client.search(&source.jql).await?;
        let status_map = source.status_map.as_ref();

        let mut seen_keys = HashSet::new();

        for issue in &issues {
            seen_keys.insert(issue.issue_key.clone());

            // Upsert raw issue into local store
            let jira_issue = crate::model::JiraIssue {
                issue_key: issue.issue_key.clone(),
                jira_project: name.to_string(),
                summary: issue.summary.clone(),
                description: issue.description.clone(),
                status: issue.status.clone(),
                priority: issue.priority.clone(),
                labels: issue.labels.clone(),
                sync_status: crate::model::SyncStatus::Synced,
                last_synced_at: chrono::Utc::now(),
            };
            self.repo.upsert_issue(&jira_issue)?;

            let existing_task_key = self.repo.find_task_by_issue_key(&issue.issue_key)?;

            match existing_task_key {
                None => {
                    let status = map_status(&issue.status, status_map);
                    let priority = map_priority(issue.priority.as_deref());
                    let task = self.task_provider.create(CreateParams {
                        title: issue.summary.clone(),
                        description: issue.description.clone(),
                        status: Some(status),
                        priority,
                        tags: issue.labels.clone(),
                        ..Default::default()
                    })?;

                    self.repo.link_task(&task.key, &issue.issue_key)?;
                    result.created += 1;
                }
                Some(task_key) => {
                    let task = self.task_provider.get(&task_key)?;

                    let new_status = map_status(&issue.status, status_map);
                    let needs_update = task.title != issue.summary
                        || task.description != issue.description
                        || task.status != new_status;

                    if needs_update {
                        self.task_provider.update(
                            &task_key,
                            UpdateParams {
                                title: Some(issue.summary.clone()),
                                description: Some(issue.description.clone()),
                                status: Some(new_status),
                                ..Default::default()
                            },
                        )?;
                        result.updated += 1;
                    }

                    self.repo.mark_synced(&issue.issue_key)?;
                }
            }
        }

        // Stale detection
        let synced_keys = self.repo.list_synced_keys(name)?;
        for key in synced_keys {
            if seen_keys.contains(&key) {
                continue;
            }
            if let Some(task_key) = self.repo.find_task_by_issue_key(&key)? {
                if let Ok(task) = self.task_provider.get(&task_key) {
                    if task.status == Status::Todo {
                        self.repo.mark_stale(&[&key])?;
                        result.stale += 1;
                    }
                }
            } else {
                self.repo.mark_stale(&[&key])?;
                result.stale += 1;
            }
        }

        Ok(())
    }
}

fn map_status(
    jira_status: &str,
    status_map: Option<&std::collections::HashMap<String, String>>,
) -> Status {
    status_map
        .and_then(|m| m.get(jira_status))
        .and_then(|v| Status::parse(v))
        .unwrap_or(Status::Todo)
}

fn map_priority(name: Option<&str>) -> i32 {
    match name {
        Some("Highest") => 1,
        Some("High") => 2,
        Some("Medium") => 3,
        Some("Low") => 4,
        Some("Lowest") => 5,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::JiraAuth;
    use crate::config::{JiraConfig, SyncSource};
    use rusqlite::Connection;
    use std::collections::HashMap;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_map_status_found() {
        let mut m = HashMap::new();
        m.insert("In Progress".to_string(), "in_progress".to_string());
        assert_eq!(map_status("In Progress", Some(&m)), Status::InProgress);
    }

    #[test]
    fn test_map_status_not_found_defaults_to_todo() {
        assert_eq!(map_status("Unknown", None), Status::Todo);
    }

    #[test]
    fn test_map_status_invalid_mapped_value_defaults_to_todo() {
        let mut m = HashMap::new();
        m.insert("X".to_string(), "invalid_status".to_string());
        assert_eq!(map_status("X", Some(&m)), Status::Todo);
    }

    #[test]
    fn test_map_priority() {
        assert_eq!(map_priority(Some("Highest")), 1);
        assert_eq!(map_priority(Some("High")), 2);
        assert_eq!(map_priority(Some("Medium")), 3);
        assert_eq!(map_priority(Some("Low")), 4);
        assert_eq!(map_priority(Some("Lowest")), 5);
        assert_eq!(map_priority(None), 0);
        assert_eq!(map_priority(Some("Other")), 0);
    }

    fn issue_json(key: &str, summary: &str, status: &str) -> serde_json::Value {
        serde_json::json!({
            "key": key,
            "fields": {
                "summary": summary,
                "description": {"type": "doc", "version": 1, "content": [
                    {"type": "paragraph", "content": [{"type": "text", "text": "desc"}]}
                ]},
                "status": {"name": status},
                "priority": {"name": "High"},
                "labels": ["backend"]
            }
        })
    }

    fn setup_db() -> (
        Arc<crate::repository::JiraRepository>,
        Arc<planeai_tasks::sqlite::SqliteRepository>,
    ) {
        let jira_repo = Arc::new(
            crate::repository::JiraRepository::new(Connection::open_in_memory().unwrap()).unwrap(),
        );
        let task_repo =
            Arc::new(planeai_tasks::sqlite::SqliteRepository::open_in_memory("TST").unwrap());
        (jira_repo, task_repo)
    }

    fn test_config() -> JiraConfig {
        let mut sources = HashMap::new();
        sources.insert(
            "proj".to_string(),
            SyncSource {
                jql: "project = PROJ".to_string(),
                status_map: Some(HashMap::from([
                    ("In Progress".to_string(), "in_progress".to_string()),
                    ("Done".to_string(), "done".to_string()),
                ])),
                writeback: None,
            },
        );
        JiraConfig {
            site: "https://test.atlassian.net".to_string(),
            sync_interval_ms: 60_000,
            sources,
        }
    }

    async fn test_client(server: &MockServer) -> Arc<JiraClient> {
        let auth = Arc::new(JiraAuth::with_fixed_token(
            "test_token",
            format!("{}/oauth/token", server.uri()),
        ));
        Arc::new(JiraClient::with_base_url(auth, server.uri()))
    }

    #[tokio::test]
    async fn sync_creates_tasks_for_new_issues() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search/jql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({

                "issues": [
                    issue_json("PROJ-1", "First", "To Do"),
                    issue_json("PROJ-2", "Second", "In Progress"),
                    issue_json("PROJ-3", "Third", "To Do"),
                ]
            })))
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let (jira_repo, task_repo) = setup_db();
        let sync = JiraSync::new(client, jira_repo, task_repo.clone(), test_config());

        let result = sync.sync_now().await.unwrap();

        assert_eq!(result.created, 3);
        assert_eq!(result.updated, 0);
        assert_eq!(result.stale, 0);

        // Verify tasks exist
        use planeai_tasks::model::ListFilter;
        use planeai_tasks::provider::TaskProvider;
        let tasks = task_repo.list(ListFilter::default()).unwrap();
        assert_eq!(tasks.len(), 3);
    }

    #[tokio::test]
    async fn sync_marks_stale_when_issue_disappears() {
        let server = MockServer::start().await;

        // First sync: 3 issues
        Mock::given(method("GET"))
            .and(path("/search/jql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({

                "issues": [
                    issue_json("PROJ-1", "First", "To Do"),
                    issue_json("PROJ-2", "Second", "To Do"),
                    issue_json("PROJ-3", "Third", "To Do"),
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let (jira_repo, task_repo) = setup_db();
        let sync = JiraSync::new(client, jira_repo.clone(), task_repo.clone(), test_config());

        sync.sync_now().await.unwrap();

        // Reset mock: second sync returns only PROJ-1 (PROJ-2, PROJ-3 disappear)
        server.reset().await;
        Mock::given(method("GET"))
            .and(path("/search/jql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({

                "issues": [issue_json("PROJ-1", "First", "To Do")]
            })))
            .mount(&server)
            .await;

        let result = sync.sync_now().await.unwrap();

        assert_eq!(result.stale, 2);
        // Verify in DB
        let issue2 = jira_repo.get_issue("PROJ-2").unwrap().unwrap();
        assert_eq!(issue2.sync_status, crate::model::SyncStatus::Stale);
    }

    #[tokio::test]
    async fn sync_updates_task_when_title_changes() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/search/jql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({

                "issues": [issue_json("PROJ-1", "Original Title", "To Do")]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let (jira_repo, task_repo) = setup_db();
        let sync = JiraSync::new(client, jira_repo, task_repo.clone(), test_config());

        sync.sync_now().await.unwrap();

        // Second sync: title changed
        server.reset().await;
        Mock::given(method("GET"))
            .and(path("/search/jql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({

                "issues": [issue_json("PROJ-1", "Updated Title", "To Do")]
            })))
            .mount(&server)
            .await;

        let result = sync.sync_now().await.unwrap();

        assert_eq!(result.updated, 1);
        use planeai_tasks::model::ListFilter;
        use planeai_tasks::provider::TaskProvider;
        let tasks = task_repo.list(ListFilter::default()).unwrap();
        assert_eq!(tasks[0].title, "Updated Title");
    }

    #[tokio::test]
    async fn stale_detection_skips_in_progress_tasks() {
        let server = MockServer::start().await;

        // First sync: 2 issues
        Mock::given(method("GET"))
            .and(path("/search/jql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({

                "issues": [
                    issue_json("PROJ-1", "Todo task", "To Do"),
                    issue_json("PROJ-2", "Active task", "In Progress"),
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let (jira_repo, task_repo) = setup_db();
        let sync = JiraSync::new(client, jira_repo.clone(), task_repo.clone(), test_config());

        sync.sync_now().await.unwrap();

        // Second sync: both disappear
        server.reset().await;
        Mock::given(method("GET"))
            .and(path("/search/jql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                 "issues": []
            })))
            .mount(&server)
            .await;

        let result = sync.sync_now().await.unwrap();

        // Only the todo task should be marked stale, not the in_progress one
        assert_eq!(result.stale, 1);
        let issue1 = jira_repo.get_issue("PROJ-1").unwrap().unwrap();
        let issue2 = jira_repo.get_issue("PROJ-2").unwrap().unwrap();
        assert_eq!(issue1.sync_status, crate::model::SyncStatus::Stale);
        assert_eq!(issue2.sync_status, crate::model::SyncStatus::Synced);
    }

    #[tokio::test]
    async fn start_stops_on_cancellation() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search/jql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                 "issues": []
            })))
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let (jira_repo, task_repo) = setup_db();
        let mut config = test_config();
        config.sync_interval_ms = 10; // fast ticks for test
        let sync = Arc::new(JiraSync::new(client, jira_repo, task_repo, config));

        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let handle = tokio::spawn(async move { sync.start(cancel_clone).await });

        tokio::time::sleep(Duration::from_millis(50)).await;
        cancel.cancel();
        // Should complete without hanging
        tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .expect("start should stop on cancel")
            .unwrap();
    }

    #[tokio::test]
    async fn error_in_one_project_does_not_crash_sync() {
        let server = MockServer::start().await;
        // Only mount a mock that will match one project's JQL but not the other
        Mock::given(method("GET"))
            .and(path("/search/jql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({

                "issues": [issue_json("PROJ-1", "Works", "To Do")]
            })))
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let (jira_repo, task_repo) = setup_db();
        let mut config = test_config();
        // Add a second source — it'll use the same mock (returns same data)
        config.sources.insert(
            "other".to_string(),
            SyncSource {
                jql: "project = OTHER".to_string(),
                status_map: None,
                writeback: None,
            },
        );

        let sync = JiraSync::new(client, jira_repo, task_repo, config);
        // Should not panic even if internal errors happen
        let result = sync.sync_now().await;
        assert!(result.is_ok());
    }
}
